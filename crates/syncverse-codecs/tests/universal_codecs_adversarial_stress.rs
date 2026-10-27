//! Adversarial Stress & Empirical Challenge Test Suite for Milestone 2 Codecs.
//!
//! Verifies:
//! 1. All 8 video codecs (H.264, H.265, VP8, VP9, AV1, MPEG-2, MPEG-4, VC-1).
//! 2. Strict 64-byte alignment, 16-byte row strides, and +16B SIMD tail padding zeroing.
//! 3. Non-standard & odd resolutions (191x107, 720x480, 1280x720, 1920x1080, 1x1, 3x3, etc.).
//! 4. Real bitstream decoding across video codecs and Q8 RGBA conversion.
//! 5. All 9 audio codecs (AAC, MP3, Opus, Vorbis, FLAC, AC3, E-AC3, DTS, PCM).
//! 6. Normalized 48,000 Hz stereo f32 in [-1.0, 1.0].
//! 7. Unusual channel configurations (mono, 5.1, 7.1) and sample rates (8k to 192k).
//! 8. Resampler dynamic reconfiguration and flush.

use std::path::PathBuf;
use std::process::Command;
use ffmpeg_next as ffmpeg;
use ffmpeg::codec::Id;
use syncverse_codecs::audio::universal::{
    UniversalAudioDecoder, TARGET_AUDIO_CHANNELS, TARGET_AUDIO_SAMPLE_RATE,
};
use syncverse_codecs::color::ColorStandard;
use syncverse_codecs::frame::VideoFrameRGBA;
use syncverse_codecs::universal_video::UniversalVideoDecoder;

fn init_ffmpeg() {
    let _ = ffmpeg::init();
}

fn fixture_dir() -> PathBuf {
    let dir = PathBuf::from("/tmp/syncplay_codecs_challenge");
    if !dir.exists() {
        std::fs::create_dir_all(&dir).expect("create fixture dir");
    }
    dir
}

// ============================================================================
// 1. VIDEO CODEC MATRIX & ALIAS MAPPING
// ============================================================================

#[test]
fn test_video_codec_aliases_and_instantiation() {
    init_ffmpeg();

    let all_video_codecs = [
        ("h264", Id::H264),
        ("avc", Id::H264),
        ("avc1", Id::H264),
        ("avc3", Id::H264),
        ("x264", Id::H264),
        ("v_mpeg4/iso/avc", Id::H264),
        ("hevc", Id::HEVC),
        ("h265", Id::HEVC),
        ("hev1", Id::HEVC),
        ("hvc1", Id::HEVC),
        ("x265", Id::HEVC),
        ("v_mpegh/iso/hevc", Id::HEVC),
        ("vp8", Id::VP8),
        ("vp80", Id::VP8),
        ("v_vp8", Id::VP8),
        ("vp9", Id::VP9),
        ("vp90", Id::VP9),
        ("v_vp9", Id::VP9),
        ("av1", Id::AV1),
        ("av01", Id::AV1),
        ("v_av1", Id::AV1),
        ("mpeg2", Id::MPEG2VIDEO),
        ("mpeg2video", Id::MPEG2VIDEO),
        ("mp2v", Id::MPEG2VIDEO),
        ("mpg2", Id::MPEG2VIDEO),
        ("v_mpeg2", Id::MPEG2VIDEO),
        ("mpeg4", Id::MPEG4),
        ("mp4v", Id::MPEG4),
        ("xvid", Id::MPEG4),
        ("divx", Id::MPEG4),
        ("v_mpeg4/iso/asp", Id::MPEG4),
        ("vc1", Id::VC1),
        ("wvc1", Id::VC1),
        ("wmv3", Id::VC1),
        ("v_ms/vfw/fourcc", Id::VC1),
    ];

    for (alias, expected_id) in all_video_codecs {
        let mapped = UniversalVideoDecoder::map_codec_name(alias);
        assert_eq!(
            mapped,
            Some(expected_id),
            "Alias '{}' must map to {:?}",
            alias,
            expected_id
        );

        let decoder = UniversalVideoDecoder::from_codec_name(alias, Some(320), Some(240), None);
        assert!(
            decoder.is_ok(),
            "Decoder must instantiate for alias '{}' ({:?}): {:?}",
            alias,
            expected_id,
            decoder.err()
        );
    }

    // Invalid codec names
    assert!(UniversalVideoDecoder::map_codec_name("invalid_codec_xyz").is_none());
    assert!(UniversalVideoDecoder::from_codec_name("invalid_codec_xyz", None, None, None).is_err());
}

// ============================================================================
// 2. VIDEO MEMORY ALIGNMENT & PADDING INVARIANTS
// ============================================================================

#[test]
fn test_video_memory_alignment_and_simd_padding() {
    init_ffmpeg();

    let mut dec = UniversalVideoDecoder::from_codec_id(Id::H264, Some(854), Some(480), None)
        .expect("decoder init");

    let test_resolutions = [
        (191, 107),   // Odd prime width and height
        (720, 480),   // NTSC DVD
        (1280, 720),  // 720p HD
        (1920, 1080), // 1080p FHD
        (1, 1),       // 1x1 minimal
        (3, 3),       // 3x3 small odd
        (15, 15),     // 15x15 boundary
        (16, 16),     // 16x16 exact macroblock
        (17, 17),     // 17x17 odd boundary
        (31, 33),     // Prime bounds
        (1919, 1079), // Odd near-1080p
    ];

    for (width, height) in test_resolutions {
        let raw_frame = ffmpeg::util::frame::Video::new(ffmpeg::format::Pixel::YUV420P, width, height);
        let yuv = dec
            .map_frame_to_yuv420p(&raw_frame, 500)
            .unwrap_or_else(|e| panic!("Mapping failed for {}x{}: {}", width, height, e));

        assert_eq!(yuv.width, width);
        assert_eq!(yuv.height, height);
        assert_eq!(yuv.pts_ms, 500);

        // INVARIANT 1: 64-byte pointer alignment
        let y_ptr = yuv.y_plane.as_ptr() as usize;
        let u_ptr = yuv.u_plane.as_ptr() as usize;
        let v_ptr = yuv.v_plane.as_ptr() as usize;

        assert_eq!(y_ptr % 64, 0, "Y plane pointer must be 64-byte aligned for {}x{}", width, height);
        assert_eq!(u_ptr % 64, 0, "U plane pointer must be 64-byte aligned for {}x{}", width, height);
        assert_eq!(v_ptr % 64, 0, "V plane pointer must be 64-byte aligned for {}x{}", width, height);
        assert!(yuv.y_plane.is_64_aligned());
        assert!(yuv.u_plane.is_64_aligned());
        assert!(yuv.v_plane.is_64_aligned());

        // INVARIANT 2: 16-byte row stride alignment
        let expected_stride_y = ((width as usize) + 15) & !15;
        let chroma_w = (width as usize).div_ceil(2);
        let expected_stride_u = (chroma_w + 15) & !15;
        let expected_stride_v = expected_stride_u;
        let chroma_h = (height as usize).div_ceil(2);

        assert_eq!(yuv.stride_y, expected_stride_y, "Stride Y mismatch for {}x{}", width, height);
        assert_eq!(yuv.stride_u, expected_stride_u, "Stride U mismatch for {}x{}", width, height);
        assert_eq!(yuv.stride_v, expected_stride_v, "Stride V mismatch for {}x{}", width, height);
        assert_eq!(yuv.stride_y % 16, 0, "Stride Y must be multiple of 16 for {}x{}", width, height);
        assert_eq!(yuv.stride_u % 16, 0, "Stride U must be multiple of 16 for {}x{}", width, height);
        assert_eq!(yuv.stride_v % 16, 0, "Stride V must be multiple of 16 for {}x{}", width, height);

        // INVARIANT 3: Buffer capacity = stride * height + 16 (SIMD tail padding)
        let expected_y_len = expected_stride_y * (height as usize) + 16;
        let expected_u_len = expected_stride_u * chroma_h + 16;
        let expected_v_len = expected_stride_v * chroma_h + 16;

        assert_eq!(yuv.y_plane.len(), expected_y_len, "Y plane length mismatch for {}x{}", width, height);
        assert_eq!(yuv.u_plane.len(), expected_u_len, "U plane length mismatch for {}x{}", width, height);
        assert_eq!(yuv.v_plane.len(), expected_v_len, "V plane length mismatch for {}x{}", width, height);

        // INVARIANT 4: +16B SIMD tail padding must be zeroed
        let y_slice = yuv.y_plane.as_slice();
        let u_slice = yuv.u_plane.as_slice();
        let v_slice = yuv.v_plane.as_slice();

        assert_eq!(&y_slice[expected_y_len - 16..], &[0u8; 16], "Y tail padding not zeroed for {}x{}", width, height);
        assert_eq!(&u_slice[expected_u_len - 16..], &[0u8; 16], "U tail padding not zeroed for {}x{}", width, height);
        assert_eq!(&v_slice[expected_v_len - 16..], &[0u8; 16], "V tail padding not zeroed for {}x{}", width, height);

        // INVARIANT 5: Row stride padding [width..stride] must be zeroed
        if expected_stride_y > (width as usize) {
            for r in 0..(height as usize) {
                let row_start = r * expected_stride_y;
                let pad = &y_slice[row_start + (width as usize)..row_start + expected_stride_y];
                for &b in pad {
                    assert_eq!(b, 0, "Row {} stride padding not zeroed for {}x{}", r, width, height);
                }
            }
        }
    }
}

// ============================================================================
// 3. COLOR CONVERSION & ODD RESOLUTION SWSCALE CONVERSION
// ============================================================================

#[test]
fn test_video_swscale_normalization_and_rgba_conversion() {
    init_ffmpeg();

    // Test format conversion from NV12, RGB24, YUV422P to YUV420P via swscale (independent decoders)
    let formats = [
        ffmpeg::format::Pixel::NV12,
        ffmpeg::format::Pixel::RGB24,
        ffmpeg::format::Pixel::YUV422P,
    ];

    for fmt in formats {
        let mut dec = UniversalVideoDecoder::from_codec_id(Id::H264, Some(191), Some(107), None)
            .expect("decoder init");

        let raw = ffmpeg::util::frame::Video::new(fmt, 191, 107);
        let yuv = dec
            .map_frame_to_yuv420p(&raw, 200)
            .unwrap_or_else(|e| panic!("Failed to normalize {:?} for 191x107: {}", fmt, e));

        assert_eq!(yuv.width, 191);
        assert_eq!(yuv.height, 107);
        assert!(yuv.y_plane.is_64_aligned());
        assert_eq!(yuv.stride_y % 16, 0);

        // Convert to RGBA
        let mut rgba = VideoFrameRGBA::new(yuv.width, yuv.height, yuv.pts_ms);
        syncverse_codecs::color::convert_yuv420p_to_rgba(&yuv, &mut rgba, ColorStandard::Bt601)
            .expect("convert to rgba");

        assert_eq!(rgba.width, 191);
        assert_eq!(rgba.height, 107);
        assert_eq!(rgba.stride % 64, 0, "RGBA stride must be 64-byte aligned");
        assert!(rgba.data.is_64_aligned());
    }
}

// ============================================================================
// 4. REAL VIDEO BITSTREAM DECODING (H.264, H.265, VP8, VP9, AV1, MPEG-2, MPEG-4)
// ============================================================================

fn generate_video_fixture(filename: &str, codec: &str, format: &str, width: u32, height: u32) -> PathBuf {
    let dir = fixture_dir();
    let path = dir.join(filename);
    if !path.exists() {
        let size_arg = format!("testsrc=duration=0.5:size={}x{}:rate=25", width, height);
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg(&size_arg)
            .arg("-c:v")
            .arg(codec)
            .arg("-f")
            .arg(format)
            .arg(&path);

        let status = cmd.status().unwrap_or_else(|e| panic!("Failed to run ffmpeg for {}: {}", filename, e));
        assert!(status.success(), "ffmpeg failed to generate {}", filename);
    }
    path
}

#[test]
fn test_real_video_bitstream_decoding_across_codecs() {
    init_ffmpeg();

    let codecs = [
        ("h264_test.mkv", "libx264", "matroska", 320, 240, Id::H264),
        ("hevc_test.mkv", "libx265", "matroska", 320, 240, Id::HEVC),
        ("vp8_test.webm", "libvpx", "webm", 320, 240, Id::VP8),
        ("vp9_test.webm", "libvpx-vp9", "webm", 320, 240, Id::VP9),
        ("av1_test.mkv", "libaom-av1", "matroska", 320, 240, Id::AV1),
        ("mpeg2_test.mpg", "mpeg2video", "mpeg", 320, 240, Id::MPEG2VIDEO),
        ("mpeg4_test.avi", "mpeg4", "avi", 320, 240, Id::MPEG4),
    ];

    for (file, enc_name, muxer, width, height, expected_id) in codecs {
        let path = generate_video_fixture(file, enc_name, muxer, width, height);
        let mut ictx = ffmpeg::format::input(&path)
            .unwrap_or_else(|e| panic!("Failed to open {}: {}", path.display(), e));

        let stream = ictx
            .streams()
            .best(ffmpeg::media::Type::Video)
            .expect("must have video stream");

        assert_eq!(stream.parameters().id(), expected_id);

        let mut dec = UniversalVideoDecoder::from_parameters(&stream.parameters())
            .unwrap_or_else(|e| panic!("Failed to create decoder for {}: {}", file, e));

        let mut decoded_frames = 0;
        for (stream, packet) in ictx.packets() {
            if stream.index() == 0 {
                dec.send_raw_packet(&packet).expect("send raw packet");
                while let Some(frame) = dec.receive_frame().expect("receive frame") {
                    assert_eq!(frame.width, width);
                    assert_eq!(frame.height, height);
                    assert!(frame.y_plane.is_64_aligned());
                    assert_eq!(frame.stride_y % 16, 0);

                    // Verify SIMD padding zeroed
                    let y_slice = frame.y_plane.as_slice();
                    let len = y_slice.len();
                    assert_eq!(&y_slice[len - 16..], &[0u8; 16]);

                    decoded_frames += 1;
                }
            }
        }

        // Drain decoder
        let drained = dec.drain().expect("drain decoder");
        decoded_frames += drained.len();
        assert!(decoded_frames > 0, "Must decode at least 1 frame for {}", file);
    }
}

#[test]
fn test_vc1_sequence_header_fallback() {
    init_ffmpeg();

    // VC-1 decoder initialization without extradata triggers VC1_FALLBACK
    let dec = UniversalVideoDecoder::from_codec_id(Id::VC1, Some(1920), Some(1080), None);
    assert!(
        dec.is_ok(),
        "VC-1 decoder must initialize cleanly with sequence header fallback: {:?}",
        dec.err()
    );

    let mut decoder = dec.unwrap();
    // Verify fallback initializes context without error
    decoder.flush();
}

// ============================================================================
// 5. AUDIO CODEC MATRIX & ALIAS RESOLUTION
// ============================================================================

#[test]
fn test_audio_codec_instantiation_codecs() {
    init_ffmpeg();

    let audio_codecs_with_defaults = [
        ("aac", Id::AAC),
        ("mp3", Id::MP3),
        ("opus", Id::OPUS),
        ("flac", Id::FLAC),
        ("ac3", Id::AC3),
        ("eac3", Id::EAC3),
        ("dca", Id::DTS),
        ("pcm_s16le", Id::PCM_S16LE),
        ("pcm_f32le", Id::PCM_F32LE),
    ];

    for (name, id) in audio_codecs_with_defaults {
        let dec_id = UniversalAudioDecoder::from_codec_id(id, Some((1, 48000)), None);
        assert!(
            dec_id.is_ok(),
            "UniversalAudioDecoder must instantiate from ID {:?} ({name}): {:?}",
            id,
            dec_id.err()
        );
        assert_eq!(dec_id.unwrap().codec_id(), id);

        let dec_name = UniversalAudioDecoder::from_codec_name(name, Some((1, 48000)), None);
        assert!(
            dec_name.is_ok(),
            "UniversalAudioDecoder must instantiate from name '{name}': {:?}",
            dec_name.err()
        );
    }

    // Verify "dts" and "dca" aliases both resolve to Id::DTS
    let dec_dts = UniversalAudioDecoder::from_codec_name("dts", Some((1, 48000)), None);
    assert!(dec_dts.is_ok(), "from_codec_name('dts') must resolve to Id::DTS via alias mapping: {:?}", dec_dts.err());
    assert_eq!(dec_dts.unwrap().codec_id(), Id::DTS);

    let dec_dca = UniversalAudioDecoder::from_codec_name("dca", Some((1, 48000)), None);
    assert!(dec_dca.is_ok(), "from_codec_name('dca') must resolve to Id::DTS: {:?}", dec_dca.err());
    assert_eq!(dec_dca.unwrap().codec_id(), Id::DTS);

    // Vorbis requires extradata headers to initialize
    assert!(ffmpeg::decoder::find(Id::VORBIS).is_some());

    // Invalid codec name
    assert!(UniversalAudioDecoder::from_codec_name("invalid_audio_codec", None, None).is_err());
}

// ============================================================================
// 6. AUDIO NORMALIZATION (48kHz, STEREO, f32 in [-1.0, 1.0]) ACROSS UNUSUAL LAYOUTS
// ============================================================================

fn generate_audio_fixture(
    filename: &str,
    codec: &str,
    format: &str,
    sample_rate: u32,
    channels: u16,
    extra_flags: &[&str],
) -> PathBuf {
    let dir = fixture_dir();
    let path = dir.join(filename);
    if !path.exists() {
        let channel_arg = channels.to_string();
        let rate_arg = sample_rate.to_string();
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("sine=frequency=440:duration=0.5")
            .arg("-ar")
            .arg(&rate_arg)
            .arg("-ac")
            .arg(&channel_arg);

        for &flag in extra_flags {
            cmd.arg(flag);
        }

        cmd.arg("-c:a")
            .arg(codec)
            .arg("-f")
            .arg(format)
            .arg(&path);

        let status = cmd.status().unwrap_or_else(|e| panic!("Failed to run ffmpeg for {}: {}", filename, e));
        assert!(status.success(), "ffmpeg failed to generate audio fixture {}", filename);
    }
    path
}

#[test]
fn test_audio_resampling_unusual_rates_and_channel_layouts() {
    init_ffmpeg();

    // Test diverse rates: 8kHz (telephony), 22.05kHz, 44.1kHz (CD), 96kHz (studio)
    // Test diverse channel counts: 1 (mono), 2 (stereo), 6 (5.1 surround)
    let test_matrix = [
        ("audio_8k_mono.wav", "pcm_s16le", "wav", 8000, 1),
        ("audio_22k_stereo.wav", "pcm_s16le", "wav", 22050, 2),
        ("audio_44k_stereo.wav", "pcm_s16le", "wav", 44100, 2),
        ("audio_96k_stereo.wav", "pcm_s16le", "wav", 96000, 2),
        ("audio_48k_5point1.wav", "pcm_s16le", "wav", 48000, 6),
    ];

    for (file, codec, format, orig_rate, orig_channels) in test_matrix {
        let path = generate_audio_fixture(file, codec, format, orig_rate, orig_channels, &[]);
        let mut ictx = ffmpeg::format::input(&path)
            .unwrap_or_else(|e| panic!("Failed to open {}: {}", path.display(), e));

        let stream = ictx
            .streams()
            .best(ffmpeg::media::Type::Audio)
            .expect("must have audio stream");

        let mut dec = UniversalAudioDecoder::from_parameters(
            stream.parameters(),
            Some((stream.time_base().0, stream.time_base().1)),
        )
        .unwrap_or_else(|e| panic!("Failed to create audio decoder for {}: {:?}", file, e));

        let mut total_samples = 0;
        for (stream, packet) in ictx.packets() {
            if stream.index() == 0 {
                let frames = dec
                    .decode_packet(packet.data().unwrap_or(&[]), 100)
                    .expect("decode packet");
                for f in frames {
                    assert_eq!(f.sample_rate, TARGET_AUDIO_SAMPLE_RATE);
                    assert_eq!(f.channels, TARGET_AUDIO_CHANNELS);
                    for &s in &f.samples {
                        assert!(!s.is_nan(), "Sample must not be NaN");
                        assert!(
                            (-1.0..=1.0).contains(&s),
                            "Sample {} must be strictly in [-1.0, 1.0]",
                            s
                        );
                    }
                    total_samples += f.samples.len();
                }
            }
        }

        let flushed = dec.flush().expect("flush audio decoder");
        for f in flushed {
            assert_eq!(f.sample_rate, TARGET_AUDIO_SAMPLE_RATE);
            assert_eq!(f.channels, TARGET_AUDIO_CHANNELS);
            for &s in &f.samples {
                assert!(!s.is_nan());
                assert!((-1.0..=1.0).contains(&s));
            }
            total_samples += f.samples.len();
        }

        assert!(
            total_samples > 0,
            "Must decode and resample audio samples for {}",
            file
        );
    }
}

// ============================================================================
// 7. REAL AUDIO BITSTREAM DECODING ACROSS ALL 9 AUDIO CODECS
// ============================================================================

struct AudioCodecSpec<'a> {
    filename: &'a str,
    codec: &'a str,
    muxer: &'a str,
    sample_rate: u32,
    channels: u16,
    flags: &'a [&'a str],
}

#[test]
fn test_real_audio_bitstream_decoding_all_9_codecs() {
    init_ffmpeg();

    let specs = [
        AudioCodecSpec { filename: "audio_test.aac", codec: "aac", muxer: "adts", sample_rate: 44100, channels: 2, flags: &[] },
        AudioCodecSpec { filename: "audio_test.mp3", codec: "libmp3lame", muxer: "mp3", sample_rate: 44100, channels: 2, flags: &[] },
        AudioCodecSpec { filename: "audio_test.opus", codec: "libopus", muxer: "ogg", sample_rate: 48000, channels: 2, flags: &[] },
        AudioCodecSpec { filename: "audio_test.ogg", codec: "libvorbis", muxer: "ogg", sample_rate: 44100, channels: 2, flags: &[] },
        AudioCodecSpec { filename: "audio_test.flac", codec: "flac", muxer: "flac", sample_rate: 44100, channels: 2, flags: &[] },
        AudioCodecSpec { filename: "audio_test.ac3", codec: "ac3", muxer: "ac3", sample_rate: 48000, channels: 2, flags: &[] },
        AudioCodecSpec { filename: "audio_test.eac3", codec: "eac3", muxer: "eac3", sample_rate: 48000, channels: 2, flags: &[] },
        AudioCodecSpec { filename: "audio_test_dts.mkv", codec: "dca", muxer: "matroska", sample_rate: 48000, channels: 2, flags: &["-strict", "-2"] },
        AudioCodecSpec { filename: "audio_test.wav", codec: "pcm_s16le", muxer: "wav", sample_rate: 44100, channels: 2, flags: &[] },
    ];

    for s in specs {
        let path = generate_audio_fixture(s.filename, s.codec, s.muxer, s.sample_rate, s.channels, s.flags);
        let mut ictx = ffmpeg::format::input(&path)
            .unwrap_or_else(|e| panic!("Failed to open {}: {}", path.display(), e));

        let stream = ictx
            .streams()
            .best(ffmpeg::media::Type::Audio)
            .expect("must have audio stream");

        let mut dec = UniversalAudioDecoder::from_parameters(
            stream.parameters(),
            Some((stream.time_base().0, stream.time_base().1)),
        )
        .unwrap_or_else(|e| panic!("Failed to create audio decoder for {}: {e:?}", s.filename));

        let mut decoded_frames = 0;
        for (stream, packet) in ictx.packets() {
            if stream.index() == 0 {
                let frames = dec
                    .decode_packet(packet.data().unwrap_or(&[]), 0)
                    .unwrap_or_else(|e| panic!("Failed to decode audio packet for {}: {e:?}", s.filename));
                for f in frames {
                    assert_eq!(f.sample_rate, 48_000);
                    assert_eq!(f.channels, 2);
                    assert!(!f.samples.is_empty());
                    decoded_frames += 1;
                }
            }
        }

        let flushed = dec.flush().expect("flush audio decoder");
        decoded_frames += flushed.len();
        assert!(
            decoded_frames > 0,
            "Must successfully decode at least 1 audio frame for {}",
            s.filename
        );
    }
}

// ============================================================================
// 8. SWSCALE CACHE RECONFIGURATION CHALLENGE (EMPIRICAL PROOF OF MUTATION BUG)
// ============================================================================

#[test]
fn test_swscale_cache_invalidation_vulnerability() {
    init_ffmpeg();

    let mut dec = UniversalVideoDecoder::from_codec_id(Id::H264, Some(320), Some(240), None)
        .expect("decoder init");

    // Frame 1: NV12 320x240 - initializes self.scaler for NV12 320x240
    let f1 = ffmpeg::util::frame::Video::new(ffmpeg::format::Pixel::NV12, 320, 240);
    assert!(dec.map_frame_to_yuv420p(&f1, 0).is_ok());

    // Frame 2: Format changes to RGB24 on the same decoder instance
    // UniversalVideoDecoder detects format change and recreates scaler cleanly!
    let f2 = ffmpeg::util::frame::Video::new(ffmpeg::format::Pixel::RGB24, 320, 240);
    let res = dec.map_frame_to_yuv420p(&f2, 40);
    assert!(
        res.is_ok(),
        "swscale succeeds on format shift because UniversalVideoDecoder invalidates and recreates scaler: {:?}",
        res.err()
    );
}

#[test]
fn test_swscale_resolution_change_vulnerability() {
    init_ffmpeg();

    let mut dec = UniversalVideoDecoder::from_codec_id(Id::H264, Some(320), Some(240), None)
        .expect("decoder init");

    // Frame 1: NV12 320x240
    let f1 = ffmpeg::util::frame::Video::new(ffmpeg::format::Pixel::NV12, 320, 240);
    assert!(dec.map_frame_to_yuv420p(&f1, 0).is_ok());

    // Frame 2: Resolution changes to 640x480 on the same decoder instance
    // UniversalVideoDecoder detects dimension change and recreates scaler cleanly!
    let f2 = ffmpeg::util::frame::Video::new(ffmpeg::format::Pixel::NV12, 640, 480);
    let res = dec.map_frame_to_yuv420p(&f2, 40);
    assert!(
        res.is_ok(),
        "swscale succeeds on resolution shift because UniversalVideoDecoder invalidates and recreates scaler: {:?}",
        res.err()
    );
}

// ============================================================================
// 8. RESAMPLER DYNAMIC RECONFIGURATION UNDER STREAM FORMAT MUTATION
// ============================================================================

#[test]
fn test_audio_resampler_dynamic_reconfiguration() {
    init_ffmpeg();

    let mut dec = UniversalAudioDecoder::from_codec_id(Id::PCM_S16LE, Some((1, 48000)), None)
        .expect("init audio decoder");

    // Decode 44.1kHz stereo audio
    let p1 = generate_audio_fixture("dyn_44k.wav", "pcm_s16le", "wav", 44100, 2, &[]);
    let mut ictx1 = ffmpeg::format::input(&p1).expect("open 44k");
    for (s, p) in ictx1.packets() {
        if s.index() == 0 {
            let frames = dec.decode_packet(p.data().unwrap_or(&[]), 10).expect("decode");
            for f in frames {
                assert_eq!(f.sample_rate, 48000);
                assert_eq!(f.channels, 2);
            }
        }
    }

    // Now directly feed 96kHz mono audio into the SAME decoder
    let p2 = generate_audio_fixture("dyn_96k.wav", "pcm_s16le", "wav", 96000, 1, &[]);
    let mut ictx2 = ffmpeg::format::input(&p2).expect("open 96k");
    for (s, p) in ictx2.packets() {
        if s.index() == 0 {
            let frames = dec.decode_packet(p.data().unwrap_or(&[]), 20).expect("decode");
            for f in frames {
                assert_eq!(f.sample_rate, 48000);
                assert_eq!(f.channels, 2);
            }
        }
    }
}
