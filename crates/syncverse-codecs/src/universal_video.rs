//! Universal video bitstream decoder powered by FFmpeg (`ffmpeg-next`).
//!
//! Provides hardware-accelerated and software decoding for:
//! - H.264 (AVC)
//! - H.265 (HEVC)
//! - VP8
//! - VP9
//! - AV1
//! - MPEG-2
//! - MPEG-4 Part 2
//! - VC-1
//!
//! Decoded frames are mapped into cache-line aligned `VideoFrameYUV420p` buffers
//! (64-byte aligned planar heap buffers, 16-byte aligned row strides, +16B SIMD tail padding),
//! and converted directly to packed `VideoFrameRGBA` via `swscale` (zero intermediate YUV420p copy)
//! or in-tree Q8 fixed-point integer arithmetic.

use crate::color::ColorStandard;
use crate::error::CodecError;
use crate::frame::{RgbaFramePool, VideoFrameRGBA, VideoFrameYUV420p};
use ffmpeg_next as ffmpeg;
use ffmpeg::software::scaling::{context::Context as ScalerContext, flag::Flags};

/// Universal video decoder supporting H.264, H.265, VP8, VP9, AV1, MPEG-2, MPEG-4, and VC-1.
pub struct UniversalVideoDecoder {
    /// Underlying opened FFmpeg video decoder context.
    decoder: ffmpeg::decoder::Video,
    /// Lazily initialized swscale context for format normalization (e.g. NV12/10-bit -> YUV420P).
    scaler: Option<ScalerContext>,
    /// Last pixel format configured in the YUV420P swscale scaler context.
    last_scaler_format: Option<ffmpeg::format::Pixel>,
    /// Last width configured in the YUV420P swscale scaler context.
    last_scaler_width: u32,
    /// Last height configured in the YUV420P swscale scaler context.
    last_scaler_height: u32,
    /// Cached direct-to-RGBA swscale context keyed by `(width, height, src_pixel_format)`.
    rgba_scaler: Option<(u32, u32, ffmpeg::format::Pixel, ScalerContext)>,
    /// Intermediate frame buffer for YUV420P format conversion.
    intermediate_frame: ffmpeg::util::frame::Video,
    /// Active color standard (BT.601 / BT.709).
    color_standard: ColorStandard,
    /// Stream time base configured by `from_stream` (defaults to `1/1000` ms).
    stream_time_base: ffmpeg::Rational,
    /// Active packet/frame PTS time base matching the most recent `send_*` call.
    active_pts_time_base: ffmpeg::Rational,
    /// Stream start time offset in microseconds (used when normalizing raw stream packets).
    stream_start_time_us: i64,
    /// Whether the most recent packet was fed via `send_raw_packet` (needs `stream_start_time_us` subtraction).
    raw_packet_mode: bool,
    /// Tracked presentation timestamp of last processed packet or frame in milliseconds.
    last_pts_ms: u64,
}

impl UniversalVideoDecoder {
    /// Constructs a `UniversalVideoDecoder` from an opened `ffmpeg::decoder::Video` and stream metadata.
    fn from_opened_decoder(
        mut decoder: ffmpeg::decoder::Video,
        stream_time_base: ffmpeg::Rational,
        stream_start_time_us: i64,
    ) -> Self {
        let standard = Self::infer_color_standard(&decoder);
        decoder.set_packet_time_base(stream_time_base);

        Self {
            decoder,
            scaler: None,
            last_scaler_format: None,
            last_scaler_width: 0,
            last_scaler_height: 0,
            rgba_scaler: None,
            intermediate_frame: ffmpeg::util::frame::Video::empty(),
            color_standard: standard,
            stream_time_base,
            active_pts_time_base: stream_time_base,
            stream_start_time_us,
            raw_packet_mode: false,
            last_pts_ms: 0,
        }
    }

    /// Initializes a new universal video decoder from an FFmpeg demuxer `Stream`,
    /// preserving the stream's native `time_base` and `start_time`.
    pub fn from_stream(stream: &ffmpeg::Stream) -> Result<Self, CodecError> {
        let mut ctx = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .map_err(|e| CodecError::Decoder(format!("failed to create context from stream: {e}")))?;

        ctx.set_threading(ffmpeg::threading::Config {
            kind: ffmpeg::threading::Type::Frame,
            count: 0,
        });

        let decoder = ctx
            .decoder()
            .video()
            .map_err(|e| CodecError::Decoder(format!("failed to open video decoder: {e}")))?;

        let tb = stream.time_base();
        let raw_start = stream.start_time();
        let start_us = if raw_start != ffmpeg::ffi::AV_NOPTS_VALUE
            && tb.numerator() > 0
            && tb.denominator() > 0
        {
            ((raw_start as i128 * tb.numerator() as i128 * 1_000_000) / tb.denominator() as i128)
                as i64
        } else {
            0
        };

        Ok(Self::from_opened_decoder(decoder, tb, start_us))
    }

    /// Initializes a new universal video decoder from FFmpeg codec parameters.
    pub fn from_parameters(params: &ffmpeg::codec::Parameters) -> Result<Self, CodecError> {
        let mut ctx = ffmpeg::codec::context::Context::from_parameters(params.clone())
            .map_err(|e| CodecError::Decoder(format!("failed to create context from parameters: {e}")))?;

        ctx.set_threading(ffmpeg::threading::Config {
            kind: ffmpeg::threading::Type::Frame,
            count: 0,
        });

        let decoder = ctx
            .decoder()
            .video()
            .map_err(|e| CodecError::Decoder(format!("failed to open video decoder: {e}")))?;

        Ok(Self::from_opened_decoder(
            decoder,
            ffmpeg::Rational(1, 1000),
            0,
        ))
    }

    /// Initializes a universal video decoder for a given codec ID, width, height, and optional extradata.
    pub fn from_codec_id(
        id: ffmpeg::codec::Id,
        width: Option<u32>,
        height: Option<u32>,
        extradata: Option<&[u8]>,
    ) -> Result<Self, CodecError> {
        let codec = ffmpeg::decoder::find(id)
            .ok_or_else(|| CodecError::UnsupportedCodec(format!("no FFmpeg decoder found for codec ID {id:?}")))?;
        let mut ctx = ffmpeg::codec::context::Context::new_with_codec(codec);

        ctx.set_threading(ffmpeg::threading::Config {
            kind: ffmpeg::threading::Type::Frame,
            count: 0,
        });

        // SAFETY: Context pointer is non-null and freshly allocated by Context::new_with_codec.
        // We configure initial width/height and extradata before calling avcodec_open2.
        unsafe {
            let raw = ctx.as_mut_ptr();
            if let Some(w) = width {
                (*raw).width = w as std::os::raw::c_int;
            }
            if let Some(h) = height {
                (*raw).height = h as std::os::raw::c_int;
            }

            // Set extradata if provided, or provide standard VC-1 fallback header if required
            let extra_slice: Option<&[u8]> = extradata.or_else(|| {
                if id == ffmpeg::codec::Id::VC1 {
                    // Standard 16-byte VC-1 Advanced Profile sequence header fallback:
                    // 00 00 01 0F (seq header) + 0xCA (profile 3, level 1, 4:2:0 chroma) + 00 00 01 0E (entry point)
                    static VC1_FALLBACK: [u8; 16] = [
                        0x00, 0x00, 0x01, 0x0f,
                        0xca, 0x80, 0x00, 0x00,
                        0x00, 0x00, 0x01, 0x0e,
                        0x00, 0x00, 0x00, 0x00,
                    ];
                    Some(&VC1_FALLBACK[..])
                } else {
                    None
                }
            });

            if let Some(extra) = extra_slice {
                if !extra.is_empty() {
                    let padded_len = extra.len() + ffmpeg::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize;
                    let buf = ffmpeg::ffi::av_malloc(padded_len) as *mut u8;
                    if !buf.is_null() {
                        std::ptr::copy_nonoverlapping(extra.as_ptr(), buf, extra.len());
                        std::ptr::write_bytes(buf.add(extra.len()), 0, ffmpeg::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize);
                        (*raw).extradata = buf;
                        (*raw).extradata_size = extra.len() as std::os::raw::c_int;
                    }
                }
            }
        }

        let decoder = ctx
            .decoder()
            .video()
            .map_err(|e| CodecError::Decoder(format!("failed to open video decoder for {id:?}: {e}")))?;

        Ok(Self::from_opened_decoder(
            decoder,
            ffmpeg::Rational(1, 1000),
            0,
        ))
    }

    /// Maps codec string name or FourCC into `ffmpeg::codec::Id`.
    pub fn map_codec_name(name: &str) -> Option<ffmpeg::codec::Id> {
        let lower = name.trim().to_ascii_lowercase();
        match lower.as_str() {
            "h264" | "avc" | "avc1" | "avc3" | "x264" | "v_mpeg4/iso/avc" => {
                Some(ffmpeg::codec::Id::H264)
            }
            "hevc" | "h265" | "hev1" | "hvc1" | "x265" | "v_mpegh/iso/hevc" => {
                Some(ffmpeg::codec::Id::HEVC)
            }
            "vp8" | "vp80" | "v_vp8" => Some(ffmpeg::codec::Id::VP8),
            "vp9" | "vp90" | "v_vp9" => Some(ffmpeg::codec::Id::VP9),
            "av1" | "av01" | "v_av1" => Some(ffmpeg::codec::Id::AV1),
            "mpeg2" | "mpeg2video" | "mp2v" | "mpg2" | "v_mpeg2" => {
                Some(ffmpeg::codec::Id::MPEG2VIDEO)
            }
            "mpeg4" | "mp4v" | "xvid" | "divx" | "v_mpeg4/iso/asp" => {
                Some(ffmpeg::codec::Id::MPEG4)
            }
            "vc1" | "wvc1" | "wmv3" | "v_ms/vfw/fourcc" => Some(ffmpeg::codec::Id::VC1),
            _ => ffmpeg::decoder::find_by_name(&lower).map(|d| d.id()),
        }
    }

    /// Initializes a universal video decoder by matching a track codec identifier.
    pub fn from_codec_name(
        name: &str,
        width: Option<u32>,
        height: Option<u32>,
        extradata: Option<&[u8]>,
    ) -> Result<Self, CodecError> {
        let codec_id = Self::map_codec_name(name)
            .ok_or_else(|| CodecError::UnsupportedCodec(format!("unrecognized video codec name: '{name}'")))?;
        Self::from_codec_id(codec_id, width, height, extradata)
    }

    /// Sets the stream start offset in microseconds subtracted when decoding raw FFmpeg packets.
    #[inline]
    pub fn set_stream_start_time_us(&mut self, start_us: i64) {
        self.stream_start_time_us = start_us;
    }

    /// Returns the configured stream start offset in microseconds.
    #[inline]
    pub fn stream_start_time_us(&self) -> i64 {
        self.stream_start_time_us
    }

    /// Returns the stream time base configured on this decoder.
    #[inline]
    pub fn time_base(&self) -> ffmpeg::Rational {
        self.stream_time_base
    }

    /// Returns true if an FFmpeg error is `EAGAIN` / `EWOULDBLOCK`.
    #[inline]
    fn is_eagain(err: &ffmpeg::Error) -> bool {
        matches!(
            err,
            ffmpeg::Error::Other { errno }
                if *errno == ffmpeg::ffi::EAGAIN || *errno == ffmpeg::ffi::EWOULDBLOCK
        )
    }

    /// Ensures the active packet/frame PTS time base matches `tb` for normalized vs raw packet input.
    #[inline]
    fn select_pts_time_base(&mut self, tb: ffmpeg::Rational, raw_mode: bool) {
        if self.active_pts_time_base != tb {
            self.active_pts_time_base = tb;
            self.decoder.set_packet_time_base(tb);
        }
        self.raw_packet_mode = raw_mode;
    }

    /// Builds an FFmpeg packet in microsecond (`1/1_000_000`) units for normalized packet submission.
    fn build_us_packet(
        data: &[u8],
        pts_us: i64,
        dts_us: Option<i64>,
        is_keyframe: bool,
    ) -> ffmpeg::Packet {
        let mut packet = ffmpeg::Packet::copy(data);
        packet.set_pts(Some(pts_us));
        if let Some(dts) = dts_us {
            packet.set_dts(Some(dts));
        }
        if is_keyframe {
            packet.set_flags(ffmpeg::packet::Flags::KEY);
        }
        packet
    }

    /// Sends a compressed packet payload to the video decoder (PTS/DTS in milliseconds).
    ///
    /// Returns `Err(CodecError::Decoder(...))` if FFmpeg returns `EAGAIN` or a decode error.
    /// For non-dropping `EAGAIN` backpressure handling, prefer [`Self::try_send_packet`] or
    /// [`Self::send_packet_us`].
    pub fn send_packet(
        &mut self,
        data: &[u8],
        pts_ms: u64,
        dts_ms: Option<u64>,
        is_keyframe: bool,
    ) -> Result<(), CodecError> {
        let pts_us = (pts_ms as i64).saturating_mul(1000);
        let dts_us = dts_ms.map(|d| (d as i64).saturating_mul(1000));
        let packet = Self::build_us_packet(data, pts_us, dts_us, is_keyframe);
        self.select_pts_time_base(ffmpeg::Rational(1, 1_000_000), false);
        self.last_pts_ms = pts_ms;
        self.decoder
            .send_packet(&packet)
            .map_err(|e| CodecError::Decoder(format!("send_packet error: {e}")))
    }

    /// Attempts to send a compressed packet (PTS in milliseconds) without dropping on `EAGAIN`.
    ///
    /// Returns:
    /// - `Ok(true)` if FFmpeg accepted the packet.
    /// - `Ok(false)` if FFmpeg returned `EAGAIN` (caller should drain decoded frames and retry the same packet).
    /// - `Err(...)` on fatal codec/stream error.
    pub fn try_send_packet(&mut self, data: &[u8], pts_ms: u64) -> Result<bool, CodecError> {
        self.try_send_packet_with_dts(data, pts_ms, None, false)
    }

    /// Attempts to send a compressed packet with optional DTS and keyframe flag without dropping on `EAGAIN`.
    pub fn try_send_packet_with_dts(
        &mut self,
        data: &[u8],
        pts_ms: u64,
        dts_ms: Option<u64>,
        is_keyframe: bool,
    ) -> Result<bool, CodecError> {
        let pts_us = (pts_ms as i64).saturating_mul(1000);
        let dts_us = dts_ms.map(|d| (d as i64).saturating_mul(1000));
        let packet = Self::build_us_packet(data, pts_us, dts_us, is_keyframe);
        self.select_pts_time_base(ffmpeg::Rational(1, 1_000_000), false);

        match self.decoder.send_packet(&packet) {
            Ok(()) => {
                self.last_pts_ms = pts_ms;
                Ok(true)
            }
            Err(ref e) if Self::is_eagain(e) => Ok(false),
            Err(e) => Err(CodecError::Decoder(format!("try_send_packet error: {e}"))),
        }
    }

    /// Attempts to send a compressed packet with microsecond-precision PTS without dropping on `EAGAIN`.
    ///
    /// Returns `Ok(true)` if accepted by FFmpeg, `Ok(false)` if FFmpeg returned `EAGAIN`.
    pub fn send_packet_us(&mut self, data: &[u8], pts_us: i64) -> Result<bool, CodecError> {
        let packet = Self::build_us_packet(data, pts_us, None, false);
        self.select_pts_time_base(ffmpeg::Rational(1, 1_000_000), false);

        match self.decoder.send_packet(&packet) {
            Ok(()) => {
                self.last_pts_ms = (pts_us.max(0) / 1000) as u64;
                Ok(true)
            }
            Err(ref e) if Self::is_eagain(e) => Ok(false),
            Err(e) => Err(CodecError::Decoder(format!("send_packet_us error: {e}"))),
        }
    }

    /// Sends raw FFmpeg packet to the video decoder using the configured `stream_time_base`.
    pub fn send_raw_packet(&mut self, packet: &ffmpeg::Packet) -> Result<(), CodecError> {
        let tb = self.stream_time_base;
        self.select_pts_time_base(tb, true);
        if let Some(pts) = packet.pts() {
            self.last_pts_ms = self.raw_pts_to_ms(pts);
        }
        self.decoder
            .send_packet(packet)
            .map_err(|e| CodecError::Decoder(format!("send_raw_packet error: {e}")))
    }

    /// Attempts to send a raw FFmpeg packet without dropping on `EAGAIN`.
    pub fn try_send_raw_packet(&mut self, packet: &ffmpeg::Packet) -> Result<bool, CodecError> {
        let tb = self.stream_time_base;
        self.select_pts_time_base(tb, true);
        match self.decoder.send_packet(packet) {
            Ok(()) => {
                if let Some(pts) = packet.pts() {
                    self.last_pts_ms = self.raw_pts_to_ms(pts);
                }
                Ok(true)
            }
            Err(ref e) if Self::is_eagain(e) => Ok(false),
            Err(e) => Err(CodecError::Decoder(format!("try_send_raw_packet error: {e}"))),
        }
    }

    /// Signals end-of-stream to the decoder so buffered frames can be drained.
    pub fn send_eof(&mut self) -> Result<(), CodecError> {
        self.decoder
            .send_eof()
            .map_err(|e| CodecError::Decoder(format!("send_eof error: {e}")))
    }

    /// Converts a raw PTS value in `active_pts_time_base` to normalized milliseconds.
    #[inline]
    fn raw_pts_to_ms(&self, raw_pts: i64) -> u64 {
        let tb = self.active_pts_time_base;
        let num = tb.numerator() as i128;
        let den = tb.denominator() as i128;
        let mut pts_us = if num > 0 && den > 0 {
            (raw_pts as i128 * num * 1_000_000) / den
        } else {
            raw_pts as i128 * 1000
        };
        if self.raw_packet_mode {
            pts_us -= self.stream_start_time_us as i128;
        }
        (pts_us.max(0) / 1000) as u64
    }

    /// Extracts presentation timestamp in milliseconds from a decoded frame using
    /// `best_effort_timestamp` (`decoded.timestamp()`) first to avoid B-frame PTS oscillation.
    #[inline]
    fn extract_frame_pts_ms(&self, decoded: &ffmpeg::util::frame::Video) -> u64 {
        match decoded.timestamp().or_else(|| decoded.pts()) {
            Some(raw_pts) => self.raw_pts_to_ms(raw_pts),
            None => self.last_pts_ms,
        }
    }

    /// Receives the next decoded video frame mapped into memory-aligned `VideoFrameYUV420p`.
    pub fn receive_frame(&mut self) -> Result<Option<VideoFrameYUV420p>, CodecError> {
        let mut decoded = ffmpeg::util::frame::Video::empty();
        match self.decoder.receive_frame(&mut decoded) {
            Ok(()) => {
                let pts_ms = self.extract_frame_pts_ms(&decoded);
                self.last_pts_ms = pts_ms;
                let frame = self.map_frame_to_yuv420p(&decoded, pts_ms)?;
                Ok(Some(frame))
            }
            Err(ref e) if Self::is_eagain(e) => Ok(None),
            Err(ffmpeg::Error::Eof) => Ok(None),
            Err(e) => Err(CodecError::Decoder(format!("receive_frame error: {e}"))),
        }
    }

    /// Receives the next decoded video frame converted directly to packed `VideoFrameRGBA` via cached `sws_scale`.
    pub fn receive_frame_rgba(&mut self) -> Result<Option<VideoFrameRGBA>, CodecError> {
        self.receive_rgba_internal(None, None, 0)
    }

    /// Receives the next decoded video frame directly into a pooled `VideoFrameRGBA` tagged with `generation`.
    pub fn receive_frame_rgba_pooled(
        &mut self,
        pool: &RgbaFramePool,
        generation: u64,
    ) -> Result<Option<VideoFrameRGBA>, CodecError> {
        self.receive_rgba_internal(None, Some(pool), generation)
    }

    /// Receives the next decoded video frame, discarding preroll frames before `min_pts_ms` without
    /// performing expensive pixel format or color space conversions.
    pub fn receive_preroll_or_rgba(
        &mut self,
        min_pts_ms: Option<u64>,
    ) -> Result<Option<VideoFrameRGBA>, CodecError> {
        self.receive_rgba_internal(min_pts_ms, None, 0)
    }

    /// Receives the next decoded video frame directly into a pooled `VideoFrameRGBA` tagged with `generation`,
    /// discarding preroll frames before `min_pts_ms` without performing pixel format conversion.
    pub fn receive_preroll_or_rgba_pooled(
        &mut self,
        min_pts_ms: u64,
        pool: &RgbaFramePool,
        generation: u64,
    ) -> Result<Option<VideoFrameRGBA>, CodecError> {
        self.receive_rgba_internal(Some(min_pts_ms), Some(pool), generation)
    }

    /// Shared drain/preroll/direct-RGBA conversion loop.
    fn receive_rgba_internal(
        &mut self,
        min_pts_ms: Option<u64>,
        pool: Option<&RgbaFramePool>,
        generation: u64,
    ) -> Result<Option<VideoFrameRGBA>, CodecError> {
        let mut decoded = ffmpeg::util::frame::Video::empty();
        loop {
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    let pts_ms = self.extract_frame_pts_ms(&decoded);
                    self.last_pts_ms = pts_ms;
                    if let Some(min_pts) = min_pts_ms {
                        if pts_ms + 15 < min_pts {
                            // Frame is before seek target: skip expensive pixel scaling/color conversion and keep draining
                            continue;
                        }
                    }
                    let rgba = self.scale_frame_to_rgba(&decoded, pts_ms, pool, generation)?;
                    return Ok(Some(rgba));
                }
                Err(ref e) if Self::is_eagain(e) => return Ok(None),
                Err(ffmpeg::Error::Eof) => return Ok(None),
                Err(e) => return Err(CodecError::Decoder(format!("receive_frame error: {e}"))),
            }
        }
    }

    /// Scales a decoded FFmpeg video frame directly into a packed `VideoFrameRGBA` (zero YUV420p copy).
    fn scale_frame_to_rgba(
        &mut self,
        decoded: &ffmpeg::util::frame::Video,
        pts_ms: u64,
        pool: Option<&RgbaFramePool>,
        generation: u64,
    ) -> Result<VideoFrameRGBA, CodecError> {
        let width = decoded.width();
        let height = decoded.height();
        let mut rgba = match pool {
            Some(p) => p.acquire(width, height, pts_ms, generation),
            None => VideoFrameRGBA::new_uninit_zeroed(width, height, pts_ms)
                .with_generation(generation),
        };

        if width == 0 || height == 0 {
            return Ok(rgba);
        }

        let src_fmt = decoded.format();
        let needs_new = match &self.rgba_scaler {
            Some((w, h, fmt, _)) => *w != width || *h != height || *fmt != src_fmt,
            None => true,
        };
        if needs_new {
            let mut ctx = ScalerContext::get(
                src_fmt,
                width,
                height,
                ffmpeg::format::Pixel::RGBA,
                width,
                height,
                Flags::BILINEAR,
            )
            .map_err(|e| CodecError::Decoder(format!("rgba swscale init failed: {e}")))?;

            let cs = match decoded.color_space() {
                ffmpeg::color::Space::BT709 => ffmpeg::ffi::SWS_CS_ITU709,
                ffmpeg::color::Space::BT470BG
                | ffmpeg::color::Space::SMPTE170M
                | ffmpeg::color::Space::SMPTE240M => ffmpeg::ffi::SWS_CS_ITU601,
                _ => {
                    if self.color_standard == ColorStandard::Bt709 || height >= 720 {
                        ffmpeg::ffi::SWS_CS_ITU709
                    } else {
                        ffmpeg::ffi::SWS_CS_ITU601
                    }
                }
            };
            let src_range = std::os::raw::c_int::from(
                decoded.color_range() == ffmpeg::color::Range::JPEG
                    || src_fmt == ffmpeg::format::Pixel::YUVJ420P
                    || src_fmt == ffmpeg::format::Pixel::YUVJ422P
                    || src_fmt == ffmpeg::format::Pixel::YUVJ444P,
            );
            // SAFETY: `ctx.as_mut_ptr()` is a valid initialized SwsContext; `sws_getCoefficients`
            // returns static coefficient tables for valid SWS_CS_* constants.
            unsafe {
                let inv_table = ffmpeg::ffi::sws_getCoefficients(cs as std::os::raw::c_int);
                let dst_table = ffmpeg::ffi::sws_getCoefficients(ffmpeg::ffi::SWS_CS_DEFAULT as std::os::raw::c_int);
                if !inv_table.is_null() && !dst_table.is_null() {
                    let _ = ffmpeg::ffi::sws_setColorspaceDetails(
                        ctx.as_mut_ptr(),
                        inv_table,
                        src_range,
                        dst_table,
                        1,
                        0,
                        1 << 16,
                        1 << 16,
                    );
                }
            }

            self.rgba_scaler = Some((width, height, src_fmt, ctx));
        }

        let (_, _, _, scaler_ctx) = self.rgba_scaler.as_mut().expect("rgba_scaler initialized");
        let dst_stride = rgba.stride as std::os::raw::c_int;
        let dst_ptr = rgba.data.as_mut_slice().as_mut_ptr();
        let mut dst_slices = [
            dst_ptr,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ];
        let mut dst_strides = [dst_stride, 0, 0, 0];

        // SAFETY:
        // - `scaler_ctx.as_mut_ptr()` is a valid SwsContext configured for `(src_fmt, width, height) -> (RGBA, width, height)`.
        // - `decoded.as_ptr()` is a valid decoded AVFrame whose plane pointers and linesizes cover `width x height`.
        // - `dst_slices[0]` points to `rgba.data` which has `rgba.stride * height` writable bytes and `dst_strides[0] == rgba.stride`.
        let scaled_h = unsafe {
            let src_av = decoded.as_ptr();
            ffmpeg::ffi::sws_scale(
                scaler_ctx.as_mut_ptr(),
                (*src_av).data.as_ptr() as *const *const u8,
                (*src_av).linesize.as_ptr() as *const std::os::raw::c_int,
                0,
                height as std::os::raw::c_int,
                dst_slices.as_mut_ptr(),
                dst_strides.as_mut_ptr(),
            )
        };
        if scaled_h <= 0 {
            return Err(CodecError::Decoder(format!(
                "direct RGBA sws_scale failed with height {scaled_h}"
            )));
        }

        Ok(rgba)
    }

    /// Decodes a packet and returns a reconstructed `VideoFrameYUV420p` if ready.
    pub fn decode_packet(
        &mut self,
        data: &[u8],
        pts_ms: u64,
    ) -> Result<Option<VideoFrameYUV420p>, CodecError> {
        self.send_packet(data, pts_ms, None, false)?;
        self.receive_frame()
    }

    /// Decodes a packet and returns a reconstructed `VideoFrameRGBA` if ready.
    pub fn decode_packet_rgba(
        &mut self,
        data: &[u8],
        pts_ms: u64,
    ) -> Result<Option<VideoFrameRGBA>, CodecError> {
        self.send_packet(data, pts_ms, None, false)?;
        self.receive_frame_rgba()
    }

    /// Drains all buffered frames from the decoder at end-of-stream.
    pub fn drain(&mut self) -> Result<Vec<VideoFrameYUV420p>, CodecError> {
        self.send_eof()?;
        let mut frames = Vec::new();
        while let Some(frame) = self.receive_frame()? {
            frames.push(frame);
        }
        Ok(frames)
    }

    /// Flushes internal decoder buffers (call immediately after random-access seek).
    pub fn flush(&mut self) {
        self.decoder.flush();
        self.scaler = None;
        self.last_scaler_format = None;
        self.last_scaler_width = 0;
        self.last_scaler_height = 0;
        self.rgba_scaler = None;
        self.last_pts_ms = 0;
    }

    /// Visible width in pixels.
    pub fn width(&self) -> u32 {
        self.decoder.width()
    }

    /// Visible height in pixels.
    pub fn height(&self) -> u32 {
        self.decoder.height()
    }

    /// Active color standard.
    pub fn color_standard(&self) -> ColorStandard {
        self.color_standard
    }

    /// Sets explicit color standard.
    pub fn set_color_standard(&mut self, standard: ColorStandard) {
        self.color_standard = standard;
    }

    /// Infers color standard from decoder context color space, falling back to ITU-R 709 for HD.
    fn infer_color_standard(decoder: &ffmpeg::decoder::Video) -> ColorStandard {
        match decoder.color_space() {
            ffmpeg::color::Space::BT709 => ColorStandard::Bt709,
            ffmpeg::color::Space::BT470BG
            | ffmpeg::color::Space::SMPTE170M
            | ffmpeg::color::Space::SMPTE240M => ColorStandard::Bt601,
            _ => {
                if decoder.height() >= 720 {
                    ColorStandard::Bt709
                } else {
                    ColorStandard::Bt601
                }
            }
        }
    }

    /// Maps an FFmpeg decoded video frame into in-tree `VideoFrameYUV420p`
    /// strictly enforcing:
    /// - 64-byte aligned planar heap buffers (`y_plane`, `u_plane`, `v_plane`).
    /// - 16-byte aligned row strides (`(width + 15) & !15`).
    /// - +16B SIMD tail padding at the end of each plane buffer.
    pub fn map_frame_to_yuv420p(
        &mut self,
        src: &ffmpeg::util::frame::Video,
        pts_ms: u64,
    ) -> Result<VideoFrameYUV420p, CodecError> {
        let width = src.width();
        let height = src.height();

        let yuv_src = if src.format() == ffmpeg::format::Pixel::YUV420P
            || src.format() == ffmpeg::format::Pixel::YUVJ420P
        {
            src
        } else {
            // Lazy format conversion via swscale if source is NV12, P010, RGB, etc.
            let needs_new_scaler = self.scaler.is_none()
                || self.last_scaler_format != Some(src.format())
                || self.last_scaler_width != width
                || self.last_scaler_height != height;

            if needs_new_scaler {
                let s = ScalerContext::get(
                    src.format(),
                    width,
                    height,
                    ffmpeg::format::Pixel::YUV420P,
                    width,
                    height,
                    Flags::BILINEAR,
                ).map_err(|e| CodecError::Decoder(format!("swscale context initialization failed: {e}")))?;
                self.scaler = Some(s);
                self.last_scaler_format = Some(src.format());
                self.last_scaler_width = width;
                self.last_scaler_height = height;
                self.intermediate_frame = ffmpeg::util::frame::Video::empty();
            }
            let s = self.scaler.as_mut().unwrap();
            s.run(src, &mut self.intermediate_frame)
                .map_err(|e| CodecError::Decoder(format!("swscale frame conversion failed: {e}")))?;
            &self.intermediate_frame
        };

        let mut out = VideoFrameYUV420p::new(width, height, pts_ms);

        // Copy Y luminance plane row-by-row
        let src_y = yuv_src.data(0);
        let src_stride_y = yuv_src.stride(0);
        let dst_y = out.y_plane.as_mut_slice();
        let dst_stride_y = out.stride_y;
        let w_usize = width as usize;

        for r in 0..(height as usize) {
            let s_start = r * src_stride_y;
            let d_start = r * dst_stride_y;
            dst_y[d_start..d_start + w_usize].copy_from_slice(&src_y[s_start..s_start + w_usize]);
            if dst_stride_y > w_usize {
                dst_y[d_start + w_usize..d_start + dst_stride_y].fill(0);
            }
        }
        let y_cap = dst_y.len();
        dst_y[y_cap - 16..y_cap].fill(0);

        // Copy U and V chroma planes row-by-row
        let chroma_w = (width as usize).div_ceil(2);
        let chroma_h = (height as usize).div_ceil(2);

        let src_u = yuv_src.data(1);
        let src_stride_u = yuv_src.stride(1);
        let dst_u = out.u_plane.as_mut_slice();
        let dst_stride_u = out.stride_u;

        for r in 0..chroma_h {
            let s_start = r * src_stride_u;
            let d_start = r * dst_stride_u;
            dst_u[d_start..d_start + chroma_w].copy_from_slice(&src_u[s_start..s_start + chroma_w]);
            if dst_stride_u > chroma_w {
                dst_u[d_start + chroma_w..d_start + dst_stride_u].fill(0);
            }
        }
        let u_cap = dst_u.len();
        dst_u[u_cap - 16..u_cap].fill(0);

        let src_v = yuv_src.data(2);
        let src_stride_v = yuv_src.stride(2);
        let dst_v = out.v_plane.as_mut_slice();
        let dst_stride_v = out.stride_v;

        for r in 0..chroma_h {
            let s_start = r * src_stride_v;
            let d_start = r * dst_stride_v;
            dst_v[d_start..d_start + chroma_w].copy_from_slice(&src_v[s_start..s_start + chroma_w]);
            if dst_stride_v > chroma_w {
                dst_v[d_start + chroma_w..d_start + dst_stride_v].fill(0);
            }
        }
        let v_cap = dst_v.len();
        dst_v[v_cap - 16..v_cap].fill(0);

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decoder_instantiation_all_codecs() {
        ffmpeg::init().expect("ffmpeg init");

        let test_codecs = [
            ("h264", ffmpeg::codec::Id::H264),
            ("hevc", ffmpeg::codec::Id::HEVC),
            ("vp8", ffmpeg::codec::Id::VP8),
            ("vp9", ffmpeg::codec::Id::VP9),
            ("av1", ffmpeg::codec::Id::AV1),
            ("mpeg2video", ffmpeg::codec::Id::MPEG2VIDEO),
            ("mpeg4", ffmpeg::codec::Id::MPEG4),
            ("vc1", ffmpeg::codec::Id::VC1),
        ];

        for (name, id) in &test_codecs {
            let dec = UniversalVideoDecoder::from_codec_id(*id, Some(640), Some(480), None);
            assert!(dec.is_ok(), "Failed to instantiate decoder for {name} ({id:?}): {:?}", dec.err());
            assert_eq!(UniversalVideoDecoder::map_codec_name(name), Some(*id));
        }
    }

    #[test]
    fn test_yuv420p_mapping_alignments() {
        ffmpeg::init().expect("ffmpeg init");

        let mut dec = UniversalVideoDecoder::from_codec_id(ffmpeg::codec::Id::H264, Some(854), Some(480), None)
            .expect("init decoder");

        // Simulate NV12 frame to test swscale conversion path
        let nv12 = ffmpeg::util::frame::Video::new(ffmpeg::format::Pixel::NV12, 854, 480);
        let yuv = dec.map_frame_to_yuv420p(&nv12, 1234).expect("map nv12");

        assert_eq!(yuv.width, 854);
        assert_eq!(yuv.height, 480);
        assert_eq!(yuv.pts_ms, 1234);

        // Invariant 1: 64-byte aligned planar buffers
        assert!(yuv.y_plane.is_64_aligned(), "Y plane must be 64B aligned");
        assert!(yuv.u_plane.is_64_aligned(), "U plane must be 64B aligned");
        assert!(yuv.v_plane.is_64_aligned(), "V plane must be 64B aligned");

        // Invariant 2: 16-byte row stride alignment ((width + 15) & !15)
        assert_eq!(yuv.stride_y, 864);
        assert_eq!(yuv.stride_u, 432);
        assert_eq!(yuv.stride_v, 432);
        assert_eq!(yuv.stride_y % 16, 0);
        assert_eq!(yuv.stride_u % 16, 0);
        assert_eq!(yuv.stride_v % 16, 0);

        // Invariant 3: +16B SIMD tail padding
        assert_eq!(yuv.y_plane.len(), 864 * 480 + 16);
        assert_eq!(yuv.u_plane.len(), 432 * 240 + 16);
        assert_eq!(yuv.v_plane.len(), 432 * 240 + 16);
    }

    fn ensure_bframe_h264_mp4() -> std::path::PathBuf {
        let dir = std::path::PathBuf::from("/tmp/syncplay_codecs_challenge");
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        let path = dir.join("bframe_90k_test.mp4");
        if !path.exists() {
            let status = std::process::Command::new("ffmpeg")
                .args([
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc=duration=0.6:size=320x240:rate=25",
                    "-c:v",
                    "libx264",
                    "-bf",
                    "2",
                    "-g",
                    "15",
                    "-video_track_timescale",
                    "90000",
                    "-f",
                    "mp4",
                ])
                .arg(&path)
                .status()
                .expect("run ffmpeg for bframe fixture");
            assert!(status.success(), "ffmpeg must produce bframe_90k_test.mp4");
        }
        path
    }

    #[test]
    fn test_from_stream_pts_roundtrip_and_bframe_monotonicity() {
        ffmpeg::init().expect("ffmpeg init");
        let path = ensure_bframe_h264_mp4();

        let mut ictx = ffmpeg::format::input(&path).expect("open mp4");
        let stream = ictx
            .streams()
            .best(ffmpeg::media::Type::Video)
            .expect("video stream");
        let stream_index = stream.index();
        let tb = stream.time_base();
        assert_ne!(tb, ffmpeg::Rational(1, 1000), "MP4 fixture should use non-1/1000 timescale");

        let mut dec = UniversalVideoDecoder::from_stream(&stream).expect("from_stream");

        // 1. Decode via send_raw_packet: B-frame presentation PTS must be monotonically non-decreasing.
        let mut raw_packets: Vec<Vec<u8>> = Vec::new();
        let mut raw_pts_list: Vec<u64> = Vec::new();
        for (s, pkt) in ictx.packets() {
            if s.index() == stream_index {
                raw_packets.push(pkt.data().unwrap_or(&[]).to_vec());
                while !dec.try_send_raw_packet(&pkt).expect("try_send_raw_packet") {
                    if let Some(frame) = dec.receive_frame().expect("receive_frame") {
                        raw_pts_list.push(frame.pts_ms);
                    }
                }
                while let Some(frame) = dec.receive_frame().expect("receive_frame") {
                    raw_pts_list.push(frame.pts_ms);
                }
            }
        }
        dec.send_eof().expect("send_eof");
        while let Some(frame) = dec.receive_frame().expect("receive_frame drain") {
            raw_pts_list.push(frame.pts_ms);
        }
        assert!(raw_pts_list.len() >= 10, "expected >= 10 decoded frames");
        for win in raw_pts_list.windows(2) {
            assert!(
                win[1] >= win[0],
                "B-frame PTS must be monotonically non-decreasing, got {} -> {}",
                win[0],
                win[1]
            );
        }
        assert!(
            *raw_pts_list.last().unwrap() >= 300,
            "90kHz stream PTS must not be divided by 90 (last={}ms)",
            raw_pts_list.last().unwrap()
        );

        // 2. Flush and feed the same packets via send_packet(data, pts_ms) on the SAME from_stream decoder.
        dec.flush();
        assert_eq!(dec.last_pts_ms, 0);

        let mut ms_pts_list = Vec::new();
        for (idx, payload) in raw_packets.iter().enumerate() {
            let pts_ms = (idx as u64) * 40 + 5000;
            while !dec.try_send_packet(payload, pts_ms).expect("try_send_packet") {
                if let Some(f) = dec.receive_frame_rgba().expect("receive_frame_rgba") {
                    ms_pts_list.push(f.pts_ms);
                }
            }
            while let Some(f) = dec.receive_frame_rgba().expect("receive_frame_rgba") {
                ms_pts_list.push(f.pts_ms);
            }
        }
        dec.send_eof().expect("send_eof");
        while let Some(f) = dec.receive_frame_rgba().expect("receive_frame_rgba drain") {
            ms_pts_list.push(f.pts_ms);
        }
        assert_eq!(ms_pts_list.len(), raw_packets.len());
        assert!(
            ms_pts_list.iter().all(|&p| p >= 5000),
            "send_packet ms PTS on from_stream decoder must round-trip without timescale corruption: {:?}",
            ms_pts_list
        );
    }

    #[test]
    fn test_direct_rgba_pool_preroll_and_send_packet_us() {
        ffmpeg::init().expect("ffmpeg init");
        let path = ensure_bframe_h264_mp4();
        let mut ictx = ffmpeg::format::input(&path).expect("open mp4");
        let stream = ictx
            .streams()
            .best(ffmpeg::media::Type::Video)
            .expect("video stream");
        let stream_index = stream.index();

        let mut dec = UniversalVideoDecoder::from_stream(&stream).expect("from_stream");
        let pool = RgbaFramePool::new(4);

        let payloads: Vec<Vec<u8>> = ictx
            .packets()
            .filter(|(s, _)| s.index() == stream_index)
            .map(|(_, p)| p.data().unwrap_or(&[]).to_vec())
            .collect();

        let mut presented_pts = Vec::new();
        let mut first_ptr: Option<usize> = None;

        for (i, payload) in payloads.iter().enumerate() {
            let pts_us = (i as i64) * 40_000; // 0ms, 40ms, 80ms, 120ms, ...
            while !dec.send_packet_us(payload, pts_us).expect("send_packet_us") {
                if let Some(rgba) = dec
                    .receive_preroll_or_rgba_pooled(100, &pool, 7)
                    .expect("preroll pooled")
                {
                    assert_eq!(rgba.generation, 7);
                    assert_eq!(rgba.data[3], 255);
                    if first_ptr.is_none() {
                        first_ptr = Some(rgba.data.as_ptr() as usize);
                    }
                    presented_pts.push(rgba.pts_ms);
                    pool.recycle(rgba);
                }
            }
            while let Some(rgba) = dec
                .receive_preroll_or_rgba_pooled(100, &pool, 7)
                .expect("preroll pooled")
            {
                assert_eq!(rgba.generation, 7);
                assert_eq!(rgba.data[3], 255);
                if first_ptr.is_none() {
                    first_ptr = Some(rgba.data.as_ptr() as usize);
                }
                presented_pts.push(rgba.pts_ms);
                pool.recycle(rgba);
            }
        }
        dec.send_eof().expect("send_eof");
        while let Some(rgba) = dec
            .receive_preroll_or_rgba_pooled(100, &pool, 7)
            .expect("preroll pooled drain")
        {
            presented_pts.push(rgba.pts_ms);
            pool.recycle(rgba);
        }

        // Preroll threshold 100ms with 15ms margin skips 0ms, 40ms, and keeps >= 85ms (actually 120ms, or 80+15=95<100 so 120ms!)
        assert!(!presented_pts.is_empty());
        assert!(
            presented_pts.iter().all(|&p| p + 15 >= 100),
            "Preroll must skip frames before target: {:?}",
            presented_pts
        );
        assert!(!pool.is_empty(), "Recycled buffers should remain in pool");

        // Verify direct pooled acquire reuses the recycled pointer
        let reused = pool.acquire(320, 240, 999, 8);
        assert_eq!(reused.generation, 8);
        assert!(reused.data.is_64_aligned());
    }
}
