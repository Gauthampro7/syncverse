//! Opt-in VA-API hardware video decode with transparent software fallback.
//!
//! # What this is (and is not)
//!
//! This module implements **hardware decode with CPU readback**. The GPU decodes the bitstream
//! into VA surfaces; every decoded frame is then copied back to system memory
//! (`av_hwframe_transfer_data`, `NV12` for 8-bit and `P010` for 10-bit content) and fed into the
//! *existing* software conversion paths (`swscale` to YUV420p / RGBA). It is **not** GPU
//! zero-copy: uploading VA surfaces straight to a `wgpu` texture is explicitly out of scope.
//! The saving is the (dominant) entropy-decode / motion-compensation / loop-filter CPU work, not
//! the final colour conversion.
//!
//! # Policy
//!
//! [`HwdecPolicy`] is `off` (default), `auto` or `vaapi`. Resolution order in
//! [`HwdecConfig::resolve`]: explicit flag value, then `SYNCVERSE_HWDEC`, then `off`.
//! `SYNCVERSE_HWDEC_DEVICE` overrides the DRM render node (otherwise `/dev/dri/renderD*` are
//! probed in order). `auto` and `vaapi` decode identically; they differ only in intent:
//! [`HwdecPolicy::is_explicit`] lets the UI surface a fallback as a warning for `vaapi` and keep it
//! quiet for `auto`.
//!
//! # Fallback
//!
//! Any failure (no device, codec without a VA-API config, driver rejects the profile, `get_format`
//! offers no VA-API surface format, frame transfer error, repeated consecutive decode errors)
//! rebuilds a software decoder and records a typed [`HwFallbackReason`]. When the failure is
//! detected during format negotiation (before the first hardware frame), packets already
//! submitted are replayed into the software decoder so no leading frames are lost.
//!
//! # Ownership
//!
//! [`HwDeviceContext`] owns an `AVBufferRef` to the VA-API device and unrefs it on `Drop`.
//! `AVCodecContext` takes its own reference, so the device outlives the codec context.

use ffmpeg_next as ffmpeg;
use std::ffi::CString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use thiserror::Error;

/// Environment variable selecting the hardware decode policy (`off|auto|vaapi`).
pub const ENV_HWDEC: &str = "SYNCVERSE_HWDEC";
/// Environment variable overriding the VA-API DRM render node path.
pub const ENV_HWDEC_DEVICE: &str = "SYNCVERSE_HWDEC_DEVICE";

/// Hardware decode policy requested by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HwdecPolicy {
    /// Software decode only (default in v0.5.0).
    #[default]
    Off,
    /// Try VA-API; silently use software when unavailable.
    Auto,
    /// User explicitly asked for VA-API; fall back to software but report it loudly.
    Vaapi,
}

impl HwdecPolicy {
    /// Parses `off|auto|vaapi` (ASCII case-insensitive, surrounding whitespace ignored).
    ///
    /// # Errors
    /// [`HwdecPolicyError::Unknown`] for any other value.
    pub fn parse(value: &str) -> Result<Self, HwdecPolicyError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "no" | "0" | "false" => Ok(Self::Off),
            "auto" => Ok(Self::Auto),
            "vaapi" | "va-api" => Ok(Self::Vaapi),
            _ => Err(HwdecPolicyError::Unknown {
                value: value.to_string(),
            }),
        }
    }

    /// Returns `true` when hardware decode should be attempted.
    pub fn is_enabled(self) -> bool {
        self != Self::Off
    }

    /// Returns `true` when the user explicitly demanded VA-API (`vaapi`).
    pub fn is_explicit(self) -> bool {
        self == Self::Vaapi
    }

    /// Canonical lowercase name (`off`, `auto`, `vaapi`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Auto => "auto",
            Self::Vaapi => "vaapi",
        }
    }
}

impl fmt::Display for HwdecPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error parsing a hardware decode policy string.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum HwdecPolicyError {
    /// The value is not one of `off`, `auto`, `vaapi`.
    #[error("unknown hwdec policy '{value}' (expected off, auto or vaapi)")]
    Unknown {
        /// The rejected input.
        value: String,
    },
}

/// Hardware decode configuration: policy plus optional device override.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HwdecConfig {
    /// Requested policy.
    pub policy: HwdecPolicy,
    /// Explicit DRM render node (e.g. `/dev/dri/renderD128`); `None` probes `/dev/dri/renderD*`.
    pub device: Option<PathBuf>,
}

impl HwdecConfig {
    /// Creates a configuration with `policy` and no device override.
    pub fn new(policy: HwdecPolicy) -> Self {
        Self {
            policy,
            device: None,
        }
    }

    /// Returns a copy with an explicit device path.
    pub fn with_device(mut self, device: impl Into<PathBuf>) -> Self {
        self.device = Some(device.into());
        self
    }

    /// Pure resolution of flag / environment values.
    ///
    /// Precedence for the policy: `flag` (CLI or settings toggle) > `env_policy`
    /// (`SYNCVERSE_HWDEC`) > `off`. The device comes from `env_device` when non-empty.
    ///
    /// # Errors
    /// [`HwdecPolicyError`] when the *selected* source is not a valid policy; a garbage env value
    /// shadowed by a valid flag is ignored.
    pub fn resolve(
        flag: Option<&str>,
        env_policy: Option<&str>,
        env_device: Option<&str>,
    ) -> Result<Self, HwdecPolicyError> {
        let policy = match (flag, env_policy) {
            (Some(f), _) => HwdecPolicy::parse(f)?,
            (None, Some(e)) if !e.trim().is_empty() => HwdecPolicy::parse(e)?,
            _ => HwdecPolicy::Off,
        };
        let device = env_device
            .filter(|d| !d.trim().is_empty())
            .map(PathBuf::from);
        Ok(Self { policy, device })
    }

    /// Resolves from the process environment ([`ENV_HWDEC`], [`ENV_HWDEC_DEVICE`]) and `flag`.
    ///
    /// # Errors
    /// See [`HwdecConfig::resolve`].
    pub fn from_env(flag: Option<&str>) -> Result<Self, HwdecPolicyError> {
        let p = std::env::var(ENV_HWDEC).ok();
        let d = std::env::var(ENV_HWDEC_DEVICE).ok();
        Self::resolve(flag, p.as_deref(), d.as_deref())
    }
}

/// Decoder backend currently producing frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecoderBackend {
    /// FFmpeg software decoder (frame-threaded).
    Software,
    /// VA-API hardware decode with CPU readback.
    Vaapi {
        /// DRM render node the device was opened on.
        device: PathBuf,
    },
}

impl DecoderBackend {
    /// Returns `true` for [`DecoderBackend::Vaapi`].
    pub fn is_hardware(&self) -> bool {
        matches!(self, Self::Vaapi { .. })
    }

    /// Short label for UI / media info, e.g. `"VA-API"` or `"software"`.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Software => "software",
            Self::Vaapi { .. } => "VA-API",
        }
    }
}

impl fmt::Display for DecoderBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Software => f.write_str("software"),
            Self::Vaapi { device } => write!(f, "VA-API ({})", device.display()),
        }
    }
}

/// Why a requested hardware decode session ended up (or switched to) software decode.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum HwFallbackReason {
    /// The VA-API device could not be opened (missing node, no permission, no driver).
    #[error("VA-API device {} could not be opened (ffmpeg error {code})", device.display())]
    DeviceOpenFailed {
        /// Device path tried last (`"auto"` when no render node exists).
        device: PathBuf,
        /// Negative FFmpeg/`errno` style error code.
        code: i32,
    },
    /// FFmpeg's decoder for this codec has no VA-API device-context hardware configuration.
    #[error("codec {codec} has no VA-API hardware decoder")]
    CodecUnsupported {
        /// Codec name.
        codec: String,
    },
    /// The driver rejected the stream profile / level / bit depth (e.g. AV1 on older Intel).
    #[error("VA-API driver does not support this stream profile")]
    ProfileUnsupported,
    /// `get_format` was offered no VA-API surface format at all.
    #[error("decoder offered no VA-API pixel format")]
    NoHwPixelFormat,
    /// Opening the codec context with the hardware device attached failed.
    #[error("opening the hardware codec context failed (ffmpeg error {code})")]
    ContextOpenFailed {
        /// FFmpeg error code (0 when unavailable).
        code: i32,
    },
    /// `av_hwframe_transfer_data` failed for a decoded surface.
    #[error("hardware frame transfer to system memory failed (ffmpeg error {code})")]
    TransferFailed {
        /// FFmpeg error code.
        code: i32,
    },
    /// Too many consecutive decode errors while on the hardware path.
    #[error("{count} consecutive hardware decode errors")]
    ConsecutiveDecodeErrors {
        /// Number of consecutive errors seen.
        count: u32,
    },
}

/// Consecutive decode errors tolerated on the hardware path before falling back.
pub const MAX_CONSECUTIVE_HW_ERRORS: u32 = 8;

/// RAII owner of a VA-API `AVHWDeviceContext` reference.
///
/// Dropping unrefs the underlying `AVBufferRef`. Codec contexts take their own reference, so a
/// decoder keeps working even if this wrapper is dropped first.
pub struct HwDeviceContext {
    buf: *mut ffmpeg::ffi::AVBufferRef,
    path: PathBuf,
}

// SAFETY: `AVBufferRef` reference counting is atomic and this wrapper never mutates the
// referenced `AVHWDeviceContext`; the raw pointer is only passed to `av_buffer_ref` /
// `av_buffer_unref`, which are thread-safe on a buffer whose refcount we own.
unsafe impl Send for HwDeviceContext {}
// SAFETY: see `Send`; `&HwDeviceContext` only permits `av_buffer_ref`, which is atomic.
unsafe impl Sync for HwDeviceContext {}

impl fmt::Debug for HwDeviceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HwDeviceContext")
            .field("path", &self.path)
            .finish()
    }
}

impl HwDeviceContext {
    /// Opens a VA-API device. With `device == None`, `/dev/dri/renderD*` nodes are tried in
    /// sorted order and the first that opens wins.
    ///
    /// # Errors
    /// [`HwFallbackReason::DeviceOpenFailed`] if no node could be opened.
    pub fn open(device: Option<&Path>) -> Result<Self, HwFallbackReason> {
        if let Some(p) = device {
            return Self::open_path(p);
        }
        let mut nodes: Vec<PathBuf> = std::fs::read_dir("/dev/dri")
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .filter(|e| e.file_name().to_string_lossy().starts_with("renderD"))
                    .map(|e| e.path())
                    .collect()
            })
            .unwrap_or_default();
        nodes.sort();
        let mut last = HwFallbackReason::DeviceOpenFailed {
            device: PathBuf::from("auto"),
            code: -19, // -ENODEV: no render node present
        };
        for node in nodes {
            match Self::open_path(&node) {
                Ok(ctx) => return Ok(ctx),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn open_path(path: &Path) -> Result<Self, HwFallbackReason> {
        let fail = |code: i32| HwFallbackReason::DeviceOpenFailed {
            device: path.to_path_buf(),
            code,
        };
        let c_path = CString::new(path.as_os_str().as_encoded_bytes()).map_err(|_| fail(-22))?;
        let mut buf: *mut ffmpeg::ffi::AVBufferRef = std::ptr::null_mut();
        // SAFETY: `buf` is a valid out-pointer; `c_path` is a NUL-terminated string that outlives
        // the call; the options dictionary is NULL (allowed); flags are 0.
        let ret = unsafe {
            ffmpeg::ffi::av_hwdevice_ctx_create(
                &mut buf,
                ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
                c_path.as_ptr(),
                std::ptr::null_mut(),
                0,
            )
        };
        if ret < 0 || buf.is_null() {
            return Err(fail(if ret < 0 { ret } else { -12 }));
        }
        Ok(Self {
            buf,
            path: path.to_path_buf(),
        })
    }

    /// Device path this context was opened on.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns a new `AVBufferRef` reference owned by the caller (handed to FFmpeg).
    pub(crate) fn new_ref(&self) -> *mut ffmpeg::ffi::AVBufferRef {
        // SAFETY: `self.buf` is a valid buffer reference for the lifetime of `self`.
        unsafe { ffmpeg::ffi::av_buffer_ref(self.buf) }
    }
}

impl Drop for HwDeviceContext {
    fn drop(&mut self) {
        // SAFETY: `self.buf` is the reference created by `av_hwdevice_ctx_create` and is unref'd
        // exactly once here; `av_buffer_unref` nulls the pointer.
        unsafe { ffmpeg::ffi::av_buffer_unref(&mut self.buf) };
    }
}

/// `get_format` negotiation outcome shared between the C callback and the decoder.
#[derive(Debug, Default)]
pub(crate) struct HwNegotiation {
    chose_vaapi: AtomicBool,
    /// 0 = none, 1 = no hw format offered, 2 = profile unsupported.
    failure: AtomicU8,
}

impl HwNegotiation {
    /// Takes a pending negotiation failure, if any.
    pub(crate) fn take_failure(&self) -> Option<HwFallbackReason> {
        match self.failure.swap(0, Ordering::AcqRel) {
            1 => Some(HwFallbackReason::NoHwPixelFormat),
            2 => Some(HwFallbackReason::ProfileUnsupported),
            _ => None,
        }
    }
}

/// `AVCodecContext::get_format` callback selecting `AV_PIX_FMT_VAAPI` when offered.
///
/// FFmpeg calls this again without the VA-API entry when the hwaccel failed to initialise
/// (driver rejects profile), which is how `ProfileUnsupported` is detected.
///
/// # Safety
/// Must only be installed on a codec context whose `opaque` points to a live [`HwNegotiation`]
/// (or is NULL). `fmts` must be an `AV_PIX_FMT_NONE`-terminated list, as FFmpeg guarantees.
pub(crate) unsafe extern "C" fn vaapi_get_format(
    ctx: *mut ffmpeg::ffi::AVCodecContext,
    fmts: *const ffmpeg::ffi::AVPixelFormat,
) -> ffmpeg::ffi::AVPixelFormat {
    use ffmpeg::ffi::AVPixelFormat as P;
    if fmts.is_null() {
        return P::AV_PIX_FMT_NONE;
    }
    // SAFETY: FFmpeg guarantees `fmts` is a valid NONE-terminated array; we stop at NONE.
    // `ctx` is the live context invoking us; `opaque` is either NULL or the `HwNegotiation`
    // kept alive by the owning decoder for the codec context's whole lifetime.
    unsafe {
        let neg = if ctx.is_null() {
            std::ptr::null()
        } else {
            (*ctx).opaque as *const HwNegotiation
        };
        let mut has_vaapi = false;
        let mut first_sw = P::AV_PIX_FMT_NONE;
        let mut p = fmts;
        while *p != P::AV_PIX_FMT_NONE {
            if *p == P::AV_PIX_FMT_VAAPI {
                has_vaapi = true;
            } else if first_sw == P::AV_PIX_FMT_NONE {
                let desc = ffmpeg::ffi::av_pix_fmt_desc_get(*p);
                let is_hw = !desc.is_null()
                    && ((*desc).flags & ffmpeg::ffi::AV_PIX_FMT_FLAG_HWACCEL as u64) != 0;
                if !is_hw {
                    first_sw = *p;
                }
            }
            p = p.add(1);
        }
        if has_vaapi {
            if let Some(n) = neg.as_ref() {
                n.chose_vaapi.store(true, Ordering::Release);
            }
            return P::AV_PIX_FMT_VAAPI;
        }
        if let Some(n) = neg.as_ref() {
            // VA-API absent: either never offered, or removed after hwaccel init failed.
            let code = if n.chose_vaapi.load(Ordering::Acquire) { 2 } else { 1 };
            n.failure.store(code, Ordering::Release);
        }
        if first_sw != P::AV_PIX_FMT_NONE {
            first_sw
        } else {
            *fmts
        }
    }
}

/// Returns `true` if `codec` exposes a VA-API device-context hardware configuration.
pub(crate) fn codec_has_vaapi_config(codec: &ffmpeg::Codec) -> bool {
    let mut i = 0;
    loop {
        // SAFETY: `codec.as_ptr()` is a valid `AVCodec`; `avcodec_get_hw_config` returns NULL
        // past the last configuration and otherwise a pointer to static data.
        let cfg = unsafe { ffmpeg::ffi::avcodec_get_hw_config(codec.as_ptr(), i) };
        if cfg.is_null() {
            return false;
        }
        // SAFETY: `cfg` is non-null static configuration data.
        let (methods, dev) = unsafe { ((*cfg).methods, (*cfg).device_type) };
        if methods & (ffmpeg::ffi::AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i32) != 0
            && dev == ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI
        {
            return true;
        }
        i += 1;
    }
}

/// Copies a hardware (`AV_PIX_FMT_VAAPI`) frame into system memory, preserving timestamps and
/// colour properties. Returns the FFmpeg error code on failure.
pub(crate) fn transfer_to_system(
    hw: &ffmpeg::util::frame::Video,
) -> Result<ffmpeg::util::frame::Video, i32> {
    let mut sw = ffmpeg::util::frame::Video::empty();
    // SAFETY: both pointers are valid AVFrames owned by `hw` / `sw`. The destination is empty
    // (format unset), so FFmpeg picks the first supported transfer format (NV12 / P010) and
    // allocates its buffers; `av_frame_copy_props` copies pts / best_effort_timestamp / colour
    // metadata from the source into the destination.
    unsafe {
        let ret = ffmpeg::ffi::av_hwframe_transfer_data(sw.as_mut_ptr(), hw.as_ptr(), 0);
        if ret < 0 {
            return Err(ret);
        }
        let ret = ffmpeg::ffi::av_frame_copy_props(sw.as_mut_ptr(), hw.as_ptr());
        if ret < 0 {
            return Err(ret);
        }
    }
    Ok(sw)
}
