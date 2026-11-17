//! VA-API hardware decode integration tests (policy parsing, fallback, correctness, seek stress).
//!
//! Hardware tests print `SKIP: no VA-API` only when no render node / `vainfo` is available (or the
//! required profile is not listed); on a machine with a working VA-API driver they really run.

use ffmpeg_next as ffmpeg;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use syncverse_codecs::{
    DecoderBackend, HwFallbackReason, HwdecConfig, HwdecPolicy, UniversalVideoDecoder,
    VideoFrameRGBA, ENV_HWDEC, ENV_HWDEC_DEVICE,
};

/// Serialises tests that touch process environment variables.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn fixture_dir() -> PathBuf {
    let d = std::env::temp_dir().join("syncverse_hwdec_fixtures");
    std::fs::create_dir_all(&d).expect("fixture dir");
    d
}

/// Generates (once) a fixture with the given ffmpeg encoder arguments. Returns `None` when the
/// local ffmpeg cannot encode it.
fn ensure_clip(name: &str, size: &str, secs: &str, enc: &[&str]) -> Option<PathBuf> {
    static GEN_LOCK: Mutex<()> = Mutex::new(());
    let _g = GEN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = fixture_dir().join(name);
    if path.exists() && std::fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false) {
        return Some(path);
    }
    let tmp = fixture_dir().join(format!("{}.{}.tmp.{}", name, std::process::id(), name.rsplit('.').next().unwrap()));
    let ok = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=size={size}:rate=30"))
        .args(["-t", secs])
        .args(enc)
        .arg(&tmp)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    std::fs::rename(&tmp, &path).ok()?;
    Some(path)
}

fn h264_clip() -> PathBuf {
    ensure_clip(
        "h264_720p.mp4",
        "1280x720",
        "3",
        &["-c:v", "libx264", "-pix_fmt", "yuv420p", "-g", "30", "-bf", "2"],
    )
    .expect("ffmpeg must be able to encode H.264 test clip")
}

/// `vainfo` output when VA-API is usable on this machine.
fn vainfo() -> Option<String> {
    if !Path::new("/dev/dri/renderD128").exists() {
        return None;
    }
    let out = Command::new("vainfo").output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

macro_rules! require_vaapi {
    ($needle:expr) => {{
        match vainfo() {
            Some(info) if $needle.is_empty() || info.contains($needle) => info,
            _ => {
                println!("SKIP: no VA-API ({})", $needle);
                return;
            }
        }
    }};
}

struct Decoded {
    frames: Vec<VideoFrameRGBA>,
    backend: DecoderBackend,
    reason: Option<HwFallbackReason>,
    hw_frames: u64,
}

fn decode_clip(path: &Path, cfg: &HwdecConfig, max_frames: usize) -> Decoded {
    ffmpeg::init().expect("ffmpeg init");
    let mut ictx = ffmpeg::format::input(&path).expect("open clip");
    let stream = ictx.streams().best(ffmpeg::media::Type::Video).expect("video stream");
    let idx = stream.index();
    let mut dec = UniversalVideoDecoder::from_stream_with_hw(&stream, cfg).expect("decoder");
    let mut frames = Vec::new();
    'outer: for (s, pkt) in ictx.packets() {
        if s.index() != idx {
            continue;
        }
        while !dec.try_send_raw_packet(&pkt).expect("send") {
            while let Some(f) = dec.receive_frame_rgba().expect("recv") {
                frames.push(f);
            }
        }
        while let Some(f) = dec.receive_frame_rgba().expect("recv") {
            frames.push(f);
        }
        if frames.len() >= max_frames {
            break 'outer;
        }
    }
    if frames.len() < max_frames {
        dec.send_eof().expect("eof");
        while let Some(f) = dec.receive_frame_rgba().expect("drain") {
            frames.push(f);
        }
    }
    Decoded {
        frames,
        backend: dec.active_backend(),
        reason: dec.fallback_reason().cloned(),
        hw_frames: dec.hw_frames_decoded(),
    }
}

fn mean_abs_diff(a: &VideoFrameRGBA, b: &VideoFrameRGBA) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let (w, h) = (a.width as usize, a.height as usize);
    let mut sum = 0u64;
    for r in 0..h {
        let ra = &a.data[r * a.stride..r * a.stride + w * 4];
        let rb = &b.data[r * b.stride..r * b.stride + w * 4];
        sum += ra.iter().zip(rb).map(|(x, y)| u64::from(x.abs_diff(*y))).sum::<u64>();
    }
    sum as f64 / (w * h * 4) as f64
}

fn assert_hw_matches_sw(clip: &Path) {
    let sw = decode_clip(clip, &HwdecConfig::default(), usize::MAX);
    let hw = decode_clip(clip, &HwdecConfig::new(HwdecPolicy::Vaapi), usize::MAX);
    assert_eq!(sw.backend, DecoderBackend::Software);
    assert!(sw.reason.is_none(), "policy off reports no fallback");
    assert!(hw.backend.is_hardware(), "expected VA-API, got {} (reason {:?})", hw.backend, hw.reason);
    assert!(hw.reason.is_none());
    assert!(hw.hw_frames > 0);
    assert_eq!(hw.frames.len(), sw.frames.len(), "frame count");
    let mut worst = 0.0f64;
    for (a, b) in sw.frames.iter().zip(&hw.frames) {
        assert_eq!(a.pts_ms, b.pts_ms, "PTS must be preserved through hw transfer");
        worst = worst.max(mean_abs_diff(a, b));
    }
    println!("{}: {} frames, worst mean-abs-diff {worst:.3}", clip.display(), hw.frames.len());
    assert!(worst <= 2.0, "mean abs diff {worst} > 2");
}

#[test]
fn policy_parse_env_and_flag() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(HwdecPolicy::parse("off").unwrap(), HwdecPolicy::Off);
    assert_eq!(HwdecPolicy::parse(" Auto ").unwrap(), HwdecPolicy::Auto);
    assert_eq!(HwdecPolicy::parse("VAAPI").unwrap(), HwdecPolicy::Vaapi);
    assert!(HwdecPolicy::parse("garbage").is_err());
    assert!(HwdecPolicy::parse("").is_err());
    assert_eq!(HwdecPolicy::default(), HwdecPolicy::Off);

    // Pure resolution: flag > env > off.
    let c = HwdecConfig::resolve(None, None, None).unwrap();
    assert_eq!(c.policy, HwdecPolicy::Off);
    assert_eq!(HwdecConfig::resolve(None, Some("auto"), None).unwrap().policy, HwdecPolicy::Auto);
    assert_eq!(HwdecConfig::resolve(Some("off"), Some("vaapi"), None).unwrap().policy, HwdecPolicy::Off);
    assert_eq!(HwdecConfig::resolve(Some("vaapi"), Some("garbage"), None).unwrap().policy, HwdecPolicy::Vaapi);
    assert!(HwdecConfig::resolve(Some("garbage"), None, None).is_err());
    assert!(HwdecConfig::resolve(None, Some("garbage"), None).is_err());
    assert_eq!(HwdecConfig::resolve(None, Some(""), None).unwrap().policy, HwdecPolicy::Off);
    let c = HwdecConfig::resolve(None, Some("auto"), Some("/dev/dri/renderD129")).unwrap();
    assert_eq!(c.device.as_deref(), Some(Path::new("/dev/dri/renderD129")));
    assert!(HwdecConfig::resolve(None, None, Some("  ")).unwrap().device.is_none());

    // Real environment.
    std::env::remove_var(ENV_HWDEC);
    std::env::remove_var(ENV_HWDEC_DEVICE);
    assert_eq!(HwdecConfig::from_env(None).unwrap().policy, HwdecPolicy::Off);
    std::env::set_var(ENV_HWDEC, "auto");
    std::env::set_var(ENV_HWDEC_DEVICE, "/dev/dri/renderD130");
    let c = HwdecConfig::from_env(None).unwrap();
    assert_eq!(c.policy, HwdecPolicy::Auto);
    assert_eq!(c.device.as_deref(), Some(Path::new("/dev/dri/renderD130")));
    assert_eq!(HwdecConfig::from_env(Some("off")).unwrap().policy, HwdecPolicy::Off);
    std::env::set_var(ENV_HWDEC, "garbage");
    assert!(HwdecConfig::from_env(None).is_err());
    assert_eq!(HwdecConfig::from_env(Some("vaapi")).unwrap().policy, HwdecPolicy::Vaapi);
    std::env::remove_var(ENV_HWDEC);
    std::env::remove_var(ENV_HWDEC_DEVICE);
}

#[test]
fn forced_bad_device_falls_back_to_software() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let clip = h264_clip();
    std::env::set_var(ENV_HWDEC, "vaapi");
    std::env::set_var(ENV_HWDEC_DEVICE, "/nonexistent");
    let cfg = HwdecConfig::from_env(None).unwrap();
    std::env::remove_var(ENV_HWDEC);
    std::env::remove_var(ENV_HWDEC_DEVICE);
    assert_eq!(cfg.policy, HwdecPolicy::Vaapi);

    let d = decode_clip(&clip, &cfg, 30);
    assert!(d.frames.len() >= 30, "decoded {} frames", d.frames.len());
    assert_eq!(d.backend, DecoderBackend::Software);
    assert!(
        matches!(d.reason, Some(HwFallbackReason::DeviceOpenFailed { .. })),
        "reason {:?}",
        d.reason
    );
    assert!(d.frames.iter().all(|f| f.data[3] == 255));
    assert!(d.reason.unwrap().to_string().contains("/nonexistent"));
}

#[test]
fn off_policy_never_opens_device() {
    let clip = h264_clip();
    let d = decode_clip(&clip, &HwdecConfig::new(HwdecPolicy::Off).with_device("/nonexistent"), 10);
    assert_eq!(d.backend, DecoderBackend::Software);
    assert!(d.reason.is_none());
}

#[test]
fn from_codec_name_with_hw_bad_device_is_software() {
    ffmpeg::init().unwrap();
    let cfg = HwdecConfig::new(HwdecPolicy::Auto).with_device("/nonexistent");
    let dec = UniversalVideoDecoder::from_codec_name_with_hw("h264", Some(640), Some(480), None, &cfg)
        .expect("software fallback decoder");
    assert_eq!(dec.active_backend(), DecoderBackend::Software);
    assert!(matches!(dec.fallback_reason(), Some(HwFallbackReason::DeviceOpenFailed { .. })));
    assert_eq!(dec.decoder_description(), "h264 (software)");
}

#[test]
fn vaapi_h264_matches_software() {
    require_vaapi!("VAProfileH264");
    assert_hw_matches_sw(&h264_clip());
}

#[test]
fn vaapi_hevc_main10_p010_converts() {
    require_vaapi!("VAProfileHEVCMain10");
    let Some(clip) = ensure_clip(
        "hevc_main10_720p.mkv",
        "1280x720",
        "2",
        &["-c:v", "libx265", "-pix_fmt", "yuv420p10le", "-x265-params", "log-level=error:keyint=30"],
    ) else {
        println!("SKIP: ffmpeg cannot encode HEVC Main10");
        return;
    };
    assert_hw_matches_sw(&clip);
}

#[test]
fn vaapi_seek_flush_no_stale_frames() {
    require_vaapi!("VAProfileH264");
    let clip = h264_clip();
    ffmpeg::init().unwrap();
    let mut ictx = ffmpeg::format::input(&clip).unwrap();
    let stream = ictx.streams().best(ffmpeg::media::Type::Video).unwrap();
    let idx = stream.index();
    let mut dec =
        UniversalVideoDecoder::from_stream_with_hw(&stream, &HwdecConfig::new(HwdecPolicy::Vaapi)).unwrap();
    let tb = stream.time_base();
    let start_us = dec.stream_start_time_us();
    let packets: Vec<ffmpeg::Packet> = ictx.packets().filter(|(s, _)| s.index() == idx).map(|(_, p)| p).collect();
    let to_ms = |pts: i64| -> u64 {
        let us = pts as i128 * i128::from(tb.numerator()) * 1_000_000 / i128::from(tb.denominator());
        ((us - i128::from(start_us)).max(0) / 1000) as u64
    };
    let keys: Vec<usize> = packets.iter().enumerate().filter(|(_, p)| p.is_key()).map(|(i, _)| i).collect();
    assert!(keys.len() >= 3, "need several keyframes, got {keys:?}");

    // Play some frames first so the hw surface pool is busy.
    for p in packets.iter().take(20) {
        while !dec.try_send_raw_packet(p).unwrap() {
            while dec.receive_frame_rgba().unwrap().is_some() {}
        }
        while dec.receive_frame_rgba().unwrap().is_some() {}
    }

    for round in 0..20usize {
        let k = keys[(round * 7 + 1) % keys.len()];
        let target = to_ms(packets[k].pts().or(packets[k].dts()).unwrap());
        dec.flush();
        let mut first: Option<u64> = None;
        'feed: for p in &packets[k..] {
            while !dec.try_send_raw_packet(p).unwrap() {
                if let Some(f) = dec.receive_frame_rgba().unwrap() {
                    first = Some(f.pts_ms);
                    break 'feed;
                }
            }
            if let Some(f) = dec.receive_frame_rgba().unwrap() {
                first = Some(f.pts_ms);
                break 'feed;
            }
        }
        let first = first.unwrap_or_else(|| {
            dec.send_eof().unwrap();
            dec.receive_frame_rgba().unwrap().expect("frame after eof").pts_ms
        });
        assert!(first + 1 >= target, "round {round}: first pts {first} < seek target {target} (stale frame)");
        if round + 1 < 20 {
            // leave the decoder mid-stream (with in-flight surfaces) before the next flush
            continue;
        }
    }
    assert!(dec.active_backend().is_hardware(), "still hardware after stress: {:?}", dec.fallback_reason());
    assert!(dec.hw_frames_decoded() > 20);
}

/// Decodes a clip the GPU cannot handle and checks the typed reason, backend and frame parity.
fn assert_unsupported_clip_falls_back(clip: &Path) {
    let sw = decode_clip(clip, &HwdecConfig::default(), usize::MAX);
    let hw = decode_clip(clip, &HwdecConfig::new(HwdecPolicy::Vaapi), usize::MAX);
    assert_eq!(hw.backend, DecoderBackend::Software);
    assert!(
        matches!(
            hw.reason,
            Some(HwFallbackReason::ProfileUnsupported) | Some(HwFallbackReason::NoHwPixelFormat)
        ),
        "reason {:?}",
        hw.reason
    );
    assert!(!sw.frames.is_empty());
    assert_eq!(hw.frames.len(), sw.frames.len(), "no frames lost across fallback replay");
    for (a, b) in sw.frames.iter().zip(&hw.frames) {
        assert_eq!(a.pts_ms, b.pts_ms);
    }
    println!("{}: fallback reason {:?}", clip.display(), hw.reason);
}

#[test]
fn av1_unsupported_profile_falls_back() {
    let info = require_vaapi!("");
    // Plan assumed no AV1 on this iGPU; iHD here lists AV1 Profile0 only, so use a profile the
    // driver cannot do: AV1 Profile 1 (4:4:4). On a GPU without any AV1 the same path triggers.
    let _ = info;
    let Some(clip) = ensure_clip(
        "av1_444_480p.mkv",
        "640x480",
        "1",
        &["-c:v", "libaom-av1", "-cpu-used", "8", "-pix_fmt", "yuv444p", "-g", "15"],
    ) else {
        println!("SKIP: ffmpeg cannot encode AV1 4:4:4");
        return;
    };
    assert_unsupported_clip_falls_back(&clip);
}

#[test]
fn h264_high10_unsupported_profile_falls_back() {
    let info = require_vaapi!("");
    if info.contains("VAProfileH264High10") {
        println!("SKIP: GPU decodes H.264 High10");
        return;
    }
    let Some(clip) = ensure_clip(
        "h264_high10_480p.mkv",
        "640x480",
        "1",
        &["-c:v", "libx264", "-pix_fmt", "yuv420p10le", "-g", "15"],
    ) else {
        println!("SKIP: ffmpeg cannot encode H.264 10-bit");
        return;
    };
    assert_unsupported_clip_falls_back(&clip);
}

#[test]
fn vaapi_av1_profile0_matches_software() {
    let info = require_vaapi!("VAProfileAV1Profile0");
    let _ = info;
    let Some(clip) = ensure_clip(
        "av1_420_480p.mkv",
        "640x480",
        "1",
        &["-c:v", "libsvtav1", "-pix_fmt", "yuv420p", "-g", "15"],
    )
    .or_else(|| {
        ensure_clip("av1_420_480p.mkv", "640x480", "1", &["-c:v", "libaom-av1", "-cpu-used", "8", "-pix_fmt", "yuv420p", "-g", "15"])
    }) else {
        println!("SKIP: ffmpeg cannot encode AV1");
        return;
    };
    assert_hw_matches_sw(&clip);
}

/// Benchmark entry used by `scripts/bench_hwdec.sh` (ignored in normal runs).
/// Decodes `BENCH_CLIP` to RGBA for up to `BENCH_SECS` seconds of media using the policy from
/// `SYNCVERSE_HWDEC` and prints the backend and frame count.
#[test]
#[ignore]
fn bench_decode_clip() {
    let clip = std::env::var("BENCH_CLIP").expect("BENCH_CLIP");
    let secs: u64 = std::env::var("BENCH_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(30);
    let cfg = HwdecConfig::from_env(None).expect("policy");
    ffmpeg::init().unwrap();
    let mut ictx = ffmpeg::format::input(&clip).unwrap();
    let stream = ictx.streams().best(ffmpeg::media::Type::Video).unwrap();
    let idx = stream.index();
    let mut dec = UniversalVideoDecoder::from_stream_with_hw(&stream, &cfg).unwrap();
    let mut n = 0u64;
    let mut last = 0u64;
    let mut sink = 0u64;
    for (s, pkt) in ictx.packets() {
        if s.index() != idx {
            continue;
        }
        while !dec.try_send_raw_packet(&pkt).unwrap() {
            while let Some(f) = dec.receive_frame_rgba().unwrap() {
                n += 1;
                last = f.pts_ms;
                sink = sink.wrapping_add(u64::from(f.data[0]));
            }
        }
        while let Some(f) = dec.receive_frame_rgba().unwrap() {
            n += 1;
            last = f.pts_ms;
            sink = sink.wrapping_add(u64::from(f.data[0]));
        }
        if last >= secs * 1000 {
            break;
        }
    }
    println!(
        "BENCH backend={} frames={} last_pts_ms={} reason={:?} hw_frames={} sink={}",
        dec.active_backend(),
        n,
        last,
        dec.fallback_reason(),
        dec.hw_frames_decoded(),
        sink
    );
}
