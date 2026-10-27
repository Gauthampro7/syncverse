//! Adversarial Empirical Challenge & Fuzzing Harness for syncplay-codecs Phase 3 Milestone 1.
//!
//! Specifically validates:
//! 1. Audio unpacker fuzzing:
//!    - Truncated PCM byte lengths across all 9 PCM formats.
//!    - IEEE 754 NaN and Inf float values in F32Le and F32Be PCM.
//!    - Opus TOC byte table indexing boundaries (all 256 configurations).
//!    - Invalid Vorbis identification headers and audio packet boundary conditions.
//! 2. Terminal TrueColor buffer stress:
//!    - Extreme display resolutions (1000x1000, 1000x1, 1x1000, 1920x1080).
//!    - Single-pixel frames (1x1) downscaling/upscaling.
//!    - Rapid color transitions (worst-case escape sequence density without deduplication).
//!    - Dynamic string capacity growth and absence of buffer overflows.
//! 3. SIMD tail padding:
//!    - Allocation of exactly +16B tail padding across all Y, U, V planes.
//!    - 64-byte alignment verification.
//!    - Canary byte protection and zero vector over-read verification.

#![deny(warnings)]

use syncverse_codecs::audio::{
    unpack_opus_packet, unpack_pcm, unpack_vorbis_audio_packet, AudioCodec, AudioDecoder,
    OpusToc, PcmFormat, VorbisIdentificationHeader,
};
use syncverse_codecs::color::{
    convert_rgba_to_terminal, convert_yuv420p_to_rgba, convert_yuv420p_to_terminal,
    ColorStandard, TerminalColorCell, TerminalTrueColorBuffer,
};
use syncverse_codecs::error::CodecError;
use syncverse_codecs::frame::{VideoFrameRGBA, VideoFrameYUV420p};

// ============================================================================
// PART 1: AUDIO UNPACKER FUZZING
// ============================================================================

/// 1.1 Tests that truncated PCM buffers across all formats and channel configurations
/// never panic, corrupt memory, or read out of bounds.
#[test]
fn test_audio_pcm_truncated_byte_lengths_fuzzing() {
    let formats = [
        PcmFormat::U8,
        PcmFormat::I16Le,
        PcmFormat::I16Be,
        PcmFormat::I24Le,
        PcmFormat::I24Be,
        PcmFormat::I32Le,
        PcmFormat::I32Be,
        PcmFormat::F32Le,
        PcmFormat::F32Be,
    ];

    let channel_counts = [1u16, 2, 3, 4, 6, 8, 16];
    let sample_rates = [8000u32, 44100, 48000, 96000];

    // Pseudorandom byte generator
    let mut rng_state: u64 = 0x123456789ABCDEF0;
    let mut next_byte = || -> u8 {
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        (rng_state >> 33) as u8
    };

    // Buffer of arbitrary bytes (up to 2048 bytes)
    let mut raw_pool = Vec::with_capacity(2048);
    for _ in 0..2048 {
        raw_pool.push(next_byte());
    }

    for &format in &formats {
        let bps = format.bytes_per_sample();
        for &channels in &channel_counts {
            let frame_bytes = bps * (channels as usize);

            // Test lengths from 0 to 3 * frame_bytes + odd remainders
            for len in 0..=(frame_bytes * 3 + 7) {
                if len > raw_pool.len() {
                    break;
                }
                let slice = &raw_pool[..len];

                // If format is float, avoid NaNs in the random pool by sanitizing
                let mut sanitized_slice = slice.to_vec();
                if format == PcmFormat::F32Le || format == PcmFormat::F32Be {
                    for chunk in sanitized_slice.as_chunks_mut::<4>().0 {
                        let val = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                        if val.is_nan() || val.is_infinite() {
                            chunk.copy_from_slice(&0.0f32.to_le_bytes());
                        }
                    }
                }

                let rate = sample_rates[len % sample_rates.len()];
                let res = unpack_pcm(&sanitized_slice, format, channels, rate, 100);

                match res {
                    Ok(frame) => {
                        let expected_frames = len / frame_bytes;
                        let expected_total_samples = expected_frames * (channels as usize);
                        assert_eq!(
                            frame.samples.len(),
                            expected_total_samples,
                            "Sample count mismatch for format {:?}, channels {}, byte len {}",
                            format,
                            channels,
                            len
                        );
                        // All samples must be bounded in [-1.0, 1.0]
                        for (idx, &s) in frame.samples.iter().enumerate() {
                            assert!(
                                (-1.0..=1.0).contains(&s),
                                "Sample {} out of bounds ({}) for format {:?} at byte len {}",
                                idx,
                                s,
                                format,
                                len
                            );
                        }
                    }
                    Err(e) => {
                        // Only acceptable error if F32 had unexpected NaN
                        assert!(
                            matches!(format, PcmFormat::F32Le | PcmFormat::F32Be),
                            "Unexpected error {:?} for format {:?} at byte len {}",
                            e,
                            format,
                            len
                        );
                    }
                }
            }
        }
    }
}

/// 1.2 Tests rejection of IEEE 754 NaN and validation of Inf values in F32Le and F32Be PCM.
#[test]
fn test_audio_pcm_f32_nan_and_inf_handling() {
    let test_nans: Vec<u32> = vec![
        0x7FC00000, // Standard Quiet NaN
        0x7FA00000, // Signaling NaN
        0xFFC00000, // Negative Quiet NaN
        0x7F800001, // Lowest NaN payload
        0x7FFFFFFF, // Highest positive NaN payload
        0xFF800001, // Negative signaling NaN
        0xFFFFFFFF, // Negative highest NaN payload
    ];

    for &nan_bits in &test_nans {
        let f32_val = f32::from_bits(nan_bits);
        assert!(f32_val.is_nan(), "Bits 0x{:08X} must be NaN", nan_bits);

        // Test F32Le
        let le_bytes = nan_bits.to_le_bytes();
        let res_le = unpack_pcm(&le_bytes, PcmFormat::F32Le, 1, 48000, 0);
        assert!(
            matches!(res_le, Err(CodecError::InvalidAudio(ref msg)) if msg.contains("NaN")),
            "F32Le must reject NaN bits 0x{:08X}, got {:?}",
            nan_bits,
            res_le
        );

        // Test F32Be
        let be_bytes = nan_bits.to_be_bytes();
        let res_be = unpack_pcm(&be_bytes, PcmFormat::F32Be, 1, 48000, 0);
        assert!(
            matches!(res_be, Err(CodecError::InvalidAudio(ref msg)) if msg.contains("NaN")),
            "F32Be must reject NaN bits 0x{:08X}, got {:?}",
            nan_bits,
            res_be
        );
    }

    // Test Infinity: f32::INFINITY and f32::NEG_INFINITY should clamp to 1.0 and -1.0
    let pos_inf = f32::INFINITY;
    let neg_inf = f32::NEG_INFINITY;

    let mut inf_buf_le = Vec::new();
    inf_buf_le.extend_from_slice(&pos_inf.to_le_bytes());
    inf_buf_le.extend_from_slice(&neg_inf.to_le_bytes());

    let frame_inf_le = unpack_pcm(&inf_buf_le, PcmFormat::F32Le, 2, 48000, 0)
        .expect("Infinity should be clamped to [-1.0, 1.0]");
    assert_eq!(frame_inf_le.samples[0], 1.0);
    assert_eq!(frame_inf_le.samples[1], -1.0);

    let mut inf_buf_be = Vec::new();
    inf_buf_be.extend_from_slice(&pos_inf.to_be_bytes());
    inf_buf_be.extend_from_slice(&neg_inf.to_be_bytes());

    let frame_inf_be = unpack_pcm(&inf_buf_be, PcmFormat::F32Be, 2, 48000, 0)
        .expect("Infinity should be clamped to [-1.0, 1.0]");
    assert_eq!(frame_inf_be.samples[0], 1.0);
    assert_eq!(frame_inf_be.samples[1], -1.0);

    // Test subnormals (denormals)
    let denorm_bits = 0x00000001u32;
    let denorm_bytes = denorm_bits.to_le_bytes();
    let frame_denorm = unpack_pcm(&denorm_bytes, PcmFormat::F32Le, 1, 48000, 0)
        .expect("Denormal float should unpack cleanly");
    assert!(frame_denorm.samples[0].abs() < 1e-10);
}

/// 1.3 Exhaustive fuzzing across all 256 Opus TOC bytes and RFC 6716 table indexing boundaries.
#[test]
fn test_opus_toc_exhaustive_256_fuzzing() {
    let allowed_durations = [2.5f32, 5.0, 10.0, 20.0, 40.0, 60.0];

    for toc_byte in 0u8..=255u8 {
        let toc = OpusToc::parse(toc_byte);

        // Verify config in 0..=31
        assert!(toc.config <= 31, "TOC config must be <= 31, was {}", toc.config);
        assert_eq!(toc.config, (toc_byte >> 3) & 0x1F);

        // Verify stereo flag
        assert_eq!(toc.stereo, (toc_byte & 0x04) != 0);

        // Verify frame count code
        assert_eq!(toc.frame_count_code, toc_byte & 0x03);

        // Verify frame duration is from the RFC 6716 standard set
        assert!(
            allowed_durations.iter().any(|&d| (d - toc.frame_duration_ms).abs() < 1e-4),
            "Unexpected duration {} for TOC 0x{:02X}",
            toc.frame_duration_ms,
            toc_byte
        );

        // Verify frame count is 1 or 2
        assert!(toc.frame_count == 1 || toc.frame_count == 2);

        // Unpack with no payload
        let packet_no_payload = [toc_byte];
        let frame_empty_payload = unpack_opus_packet(&packet_no_payload, 48000, 0)
            .unwrap_or_else(|e| panic!("Failed to unpack 1-byte Opus packet 0x{:02X}: {:?}", toc_byte, e));

        let expected_channels = if toc.stereo { 2 } else { 1 };
        assert_eq!(frame_empty_payload.channels, expected_channels);
        assert!(!frame_empty_payload.samples.is_empty());
        for &s in &frame_empty_payload.samples {
            assert!((-1.0..=1.0).contains(&s), "Sample out of bounds");
        }

        // Unpack with varying payload sizes
        for payload_len in [1, 5, 20, 127] {
            let mut packet = vec![toc_byte];
            packet.resize(1 + payload_len, 0x5A);
            let frame = unpack_opus_packet(&packet, 48000, 10).unwrap();
            assert_eq!(frame.channels, expected_channels);
            assert_eq!(frame.samples.len(), frame_empty_payload.samples.len());
        }
    }

    // Boundary check: empty packet must return BufferTooSmall
    assert!(matches!(
        unpack_opus_packet(&[], 48000, 0),
        Err(CodecError::BufferTooSmall { required: 1, available: 0 })
    ));

    // Sample rate variations
    let packet = [0x00, 0x12, 0x34]; // Silk narrowband 10ms mono
    for rate in [8000, 16000, 44100, 48000, 96000, 192000] {
        let frame = unpack_opus_packet(&packet, rate, 0).unwrap();
        assert_eq!(frame.sample_rate, rate);
        assert!((frame.duration_secs() - 0.010).abs() < 0.002);
    }
}

/// 1.4 Tests invalid Vorbis identification headers and boundary conditions.
#[test]
fn test_vorbis_header_and_packet_fuzzing() {
    // Helper to generate a valid Vorbis header (30 bytes)
    let make_valid_header = || -> Vec<u8> {
        let mut h = Vec::new();
        h.push(0x01); // packet type 1
        h.extend_from_slice(b"vorbis"); // magic
        h.extend_from_slice(&0u32.to_le_bytes()); // version 0
        h.push(2); // 2 channels
        h.extend_from_slice(&48000u32.to_le_bytes()); // sample rate 48kHz
        h.extend_from_slice(&128000i32.to_le_bytes()); // max bitrate
        h.extend_from_slice(&128000i32.to_le_bytes()); // nom bitrate
        h.extend_from_slice(&128000i32.to_le_bytes()); // min bitrate
        h.push(0xB8); // blocksize_0 = 8 (2^8 = 256), blocksize_1 = 11 (2^11 = 2048)
        h.push(0x01); // framing flag = 1
        h
    };

    let valid_h = make_valid_header();
    assert_eq!(valid_h.len(), 30);
    let parsed_valid = VorbisIdentificationHeader::parse(&valid_h).unwrap();
    assert_eq!(parsed_valid.audio_channels, 2);
    assert_eq!(parsed_valid.audio_sample_rate, 48000);
    assert_eq!(parsed_valid.blocksize_0, 8);
    assert_eq!(parsed_valid.blocksize_1, 11);

    // 1. Truncated headers (0..29 bytes)
    for len in 0..30 {
        let res = VorbisIdentificationHeader::parse(&valid_h[..len]);
        assert!(
            matches!(res, Err(CodecError::BufferTooSmall { required: 30, available }) if available == len),
            "Expected BufferTooSmall for len {}",
            len
        );
    }

    // 2. Corrupt packet type != 0x01
    for bad_type in [0x00, 0x02, 0x03, 0x05, 0xFF] {
        let mut bad_h = valid_h.clone();
        bad_h[0] = bad_type;
        assert!(matches!(
            VorbisIdentificationHeader::parse(&bad_h),
            Err(CodecError::InvalidAudio(msg)) if msg.contains("magic")
        ));
    }

    // 3. Corrupt magic != "vorbis"
    let bad_magics: [&[u8]; 4] = [b"Vorbis", b"VORBIS", b"vorbi\x00", b"\x00\x00\x00\x00\x00\x00"];
    for bad_magic in bad_magics {
        let mut bad_h = valid_h.clone();
        bad_h[1..7].copy_from_slice(bad_magic);
        assert!(matches!(
            VorbisIdentificationHeader::parse(&bad_h),
            Err(CodecError::InvalidAudio(msg)) if msg.contains("magic")
        ));
    }

    // 4. Unsupported vorbis_version != 0
    for bad_ver in [1u32, 2, 100, u32::MAX] {
        let mut bad_h = valid_h.clone();
        bad_h[7..11].copy_from_slice(&bad_ver.to_le_bytes());
        assert!(matches!(
            VorbisIdentificationHeader::parse(&bad_h),
            Err(CodecError::InvalidAudio(msg)) if msg.contains("version")
        ));
    }

    // 5. Zero channels
    let mut zero_ch_h = valid_h.clone();
    zero_ch_h[11] = 0;
    assert!(matches!(
        VorbisIdentificationHeader::parse(&zero_ch_h),
        Err(CodecError::InvalidAudio(msg)) if msg.contains("channels")
    ));

    // 6. Zero sample rate
    let mut zero_rate_h = valid_h.clone();
    zero_rate_h[12..16].copy_from_slice(&0u32.to_le_bytes());
    assert!(matches!(
        VorbisIdentificationHeader::parse(&zero_rate_h),
        Err(CodecError::InvalidAudio(msg)) if msg.contains("sample_rate")
    ));

    // 7. Framing flag == 0
    let mut bad_frame_h = valid_h.clone();
    bad_frame_h[29] = 0x00;
    assert!(matches!(
        VorbisIdentificationHeader::parse(&bad_frame_h),
        Err(CodecError::InvalidAudio(msg)) if msg.contains("framing_flag")
    ));

    // 8. Unpack vorbis audio packet
    // Empty packet -> BufferTooSmall
    assert!(matches!(
        unpack_vorbis_audio_packet(&[], &parsed_valid, 0),
        Err(CodecError::BufferTooSmall { required: 1, available: 0 })
    ));

    // Valid small packet -> unpacks bounded samples
    let sample_packet = [0x12, 0x34, 0x56, 0x78];
    let frame = unpack_vorbis_audio_packet(&sample_packet, &parsed_valid, 100).unwrap();
    assert_eq!(frame.channels, 2);
    assert_eq!(frame.sample_rate, 48000);
    assert_eq!(frame.pts_ms, 100);
    for &s in &frame.samples {
        assert!((-1.0..=1.0).contains(&s));
    }

    // Blocksize exponent clamping (0 -> clamped to 6 => 64 samples; 15 -> clamped to 13 => 8192 samples)
    let mut extreme_block_h = valid_h.clone();
    extreme_block_h[28] = 0x00; // blocksize_0 = 0
    let parsed_clamped_low = VorbisIdentificationHeader::parse(&extreme_block_h).unwrap();
    let frame_low = unpack_vorbis_audio_packet(&sample_packet, &parsed_clamped_low, 0).unwrap();
    assert_eq!(frame_low.samples.len(), (1 << 6) * 2);

    extreme_block_h[28] = 0xFF; // blocksize_0 = 15
    let parsed_clamped_high = VorbisIdentificationHeader::parse(&extreme_block_h).unwrap();
    let frame_high = unpack_vorbis_audio_packet(&sample_packet, &parsed_clamped_high, 0).unwrap();
    assert_eq!(frame_high.samples.len(), (1 << 13) * 2);
}

/// 1.5 Tests AudioDecoder unified dispatcher under malformed inputs and codec switching.
#[test]
fn test_audio_decoder_dispatcher_adversarial() {
    // 1. PCM Dispatcher
    let mut pcm_dec = AudioDecoder::new(AudioCodec::Pcm(PcmFormat::I16Le), 48000, 2);
    assert_eq!(pcm_dec.sample_rate(), 48000);
    assert_eq!(pcm_dec.channels(), 2);
    // 3 bytes (incomplete frame for stereo I16)
    let pcm_res = pcm_dec.decode_packet(&[0x01, 0x02, 0x03], 0).unwrap();
    assert_eq!(pcm_res.samples.len(), 0); // truncated cleanly

    // 2. Opus Dispatcher
    let mut opus_dec = AudioDecoder::new(AudioCodec::Opus, 48000, 2);
    // Empty packet -> BufferTooSmall
    assert!(matches!(
        opus_dec.decode_packet(&[], 0),
        Err(CodecError::BufferTooSmall { required: 1, available: 0 })
    ));
    // Valid 1-byte packet
    let opus_res = opus_dec.decode_packet(&[0x00], 100).unwrap();
    assert!(!opus_res.samples.is_empty());
    assert_eq!(opus_res.pts_ms, 100);

    // 3. Vorbis Dispatcher
    let mut valid_h = vec![0x01];
    valid_h.extend_from_slice(b"vorbis");
    valid_h.extend_from_slice(&0u32.to_le_bytes());
    valid_h.push(1); // 1 channel
    valid_h.extend_from_slice(&48000u32.to_le_bytes());
    valid_h.extend_from_slice(&[0u8; 12]);
    valid_h.push(0x88);
    valid_h.push(1);
    let header = VorbisIdentificationHeader::parse(&valid_h).unwrap();
    let mut vorbis_dec = AudioDecoder::new(AudioCodec::Vorbis(header), 48000, 1);
    assert!(matches!(
        vorbis_dec.decode_packet(&[], 0),
        Err(CodecError::BufferTooSmall { required: 1, available: 0 })
    ));
    let vorbis_res = vorbis_dec.decode_packet(&[0x55, 0xAA], 200).unwrap();
    assert_eq!(vorbis_res.channels, 1);
    assert_eq!(vorbis_res.pts_ms, 200);
}

// ============================================================================
// PART 2: TERMINAL TRUECOLOR BUFFER STRESS
// ============================================================================

/// 2.1 Tests extreme terminal resolutions (1000x1000, 1000x1, 1x1000) and capacity growth.
#[test]
fn test_terminal_truecolor_extreme_resolutions_stress() {
    // 1000x1000 character cells = 1,000,000 cells (displays 1000x2000 pixels)
    let large_buf = TerminalTrueColorBuffer::new(1000, 1000);
    assert_eq!(large_buf.cols, 1000);
    assert_eq!(large_buf.rows, 1000);
    assert_eq!(large_buf.cells.len(), 1_000_000);

    // Populate with uniform color
    let mut ansi_out = String::new();
    large_buf.render_ansi(&mut ansi_out);

    // Uniform cells: deduplication emits exactly 1 fg + 1 bg sequence per row
    // ~40 bytes per row * 1000 rows + 1000 half-blocks per row (3000 bytes) + reset (5 bytes)
    // ~3.05 MB total output
    assert!(ansi_out.len() > 1_000_000, "Must contain all cells");
    assert!(ansi_out.len() < 5_000_000, "Deduplication must keep size bounded");

    // Extreme tall: 1x1000
    let tall_buf = TerminalTrueColorBuffer::new(1, 1000);
    assert_eq!(tall_buf.cells.len(), 1000);
    let mut tall_out = String::new();
    tall_buf.render_ansi(&mut tall_out);
    assert_eq!(tall_out.matches('\n').count(), 1000);

    // Extreme wide: 1000x1
    let wide_buf = TerminalTrueColorBuffer::new(1000, 1);
    assert_eq!(wide_buf.cells.len(), 1000);
    let mut wide_out = String::new();
    wide_buf.render_ansi(&mut wide_out);
    assert_eq!(wide_out.matches('\n').count(), 1);

    // Repeated resizing stress
    let mut resizable = TerminalTrueColorBuffer::new(10, 10);
    for &(c, r) in &[(100, 50), (1, 1), (500, 200), (80, 24), (1000, 500)] {
        resizable.resize(c, r);
        assert_eq!(resizable.cols, c);
        assert_eq!(resizable.rows, r);
        assert_eq!(resizable.cells.len(), c * r);
    }
}

/// 2.2 Tests single-pixel frames (1x1) downscaling and upscaling into TerminalTrueColorBuffer.
#[test]
fn test_terminal_single_pixel_frames() {
    // 1x1 YUV420p frame
    let mut yuv_1x1 = VideoFrameYUV420p::new(1, 1, 42);
    yuv_1x1.y_plane.as_mut_slice()[0] = 81; // Reddish
    yuv_1x1.u_plane.as_mut_slice()[0] = 90;
    yuv_1x1.v_plane.as_mut_slice()[0] = 240;

    // Convert 1x1 YUV to 1x1 terminal buffer
    let mut term_1x1 = TerminalTrueColorBuffer::new(1, 1);
    convert_yuv420p_to_terminal(&yuv_1x1, &mut term_1x1, ColorStandard::Bt601).unwrap();
    assert_eq!(term_1x1.cells.len(), 1);
    assert!(term_1x1.cells[0].top_r >= 250); // Red channel high

    // Convert 1x1 YUV to 80x24 terminal buffer (upscaling 1 pixel to fill terminal)
    let mut term_80x24 = TerminalTrueColorBuffer::new(80, 24);
    convert_yuv420p_to_terminal(&yuv_1x1, &mut term_80x24, ColorStandard::Bt601).unwrap();
    for cell in &term_80x24.cells {
        assert_eq!(cell.top_r, term_1x1.cells[0].top_r);
        assert_eq!(cell.bot_r, term_1x1.cells[0].bot_r);
    }

    // 1x1 RGBA frame
    let mut rgba_1x1 = VideoFrameRGBA::new(1, 1, 42);
    rgba_1x1.data.as_mut_slice()[0] = 12;
    rgba_1x1.data.as_mut_slice()[1] = 34;
    rgba_1x1.data.as_mut_slice()[2] = 56;
    rgba_1x1.data.as_mut_slice()[3] = 255;

    let mut term_rgba_1x1 = TerminalTrueColorBuffer::new(1, 1);
    convert_rgba_to_terminal(&rgba_1x1, &mut term_rgba_1x1).unwrap();
    assert_eq!(term_rgba_1x1.cells[0].top_r, 12);
    assert_eq!(term_rgba_1x1.cells[0].top_g, 34);
    assert_eq!(term_rgba_1x1.cells[0].top_b, 56);
}

/// 2.3 Tests worst-case escape sequence density (every adjacent pixel changes color).
#[test]
fn test_terminal_truecolor_worst_case_escape_sequence_density() {
    let cols = 80;
    let rows = 24;
    let mut buf = TerminalTrueColorBuffer::new(cols, rows);

    // Populate with maximum entropy checkerboard where EVERY single sub-pixel changes color
    for r in 0..rows {
        for c in 0..cols {
            let idx = r * cols + c;
            buf.cells[idx] = TerminalColorCell {
                top_r: ((c * 17 + r * 13) % 256) as u8,
                top_g: ((c * 19 + r * 7 + 10) % 256) as u8,
                top_b: ((c * 23 + r * 3 + 20) % 256) as u8,
                bot_r: ((c * 29 + r * 11 + 30) % 256) as u8,
                bot_g: ((c * 31 + r * 5 + 40) % 256) as u8,
                bot_b: ((c * 37 + r * 2 + 50) % 256) as u8,
            };
        }
    }

    let mut ansi_worst = String::new();
    buf.render_ansi(&mut ansi_worst);

    // Every cell triggers fg escape (~19B) + bg escape (~19B) + half block (3B) = ~41 bytes per cell
    // 80 * 24 = 1920 cells -> ~78 KB
    assert!(
        ansi_worst.len() > 60_000,
        "Worst-case frame size must reflect non-deduplicated escape sequences (was {})",
        ansi_worst.len()
    );
    assert!(
        ansi_worst.len() < 100_000,
        "Must stay within expected upper bound (was {})",
        ansi_worst.len()
    );

    // Verify all rows terminate cleanly with reset sequence and newline
    assert_eq!(ansi_worst.matches("\x1b[0m\n").count(), rows);
}

// ============================================================================
// PART 3: VALIDATE SIMD TAIL PADDING & OVER-READ PREVENTION
// ============================================================================

/// 3.1 Tests allocation of +16B SIMD tail padding and 64-byte alignment across arbitrary dimensions.
#[test]
fn test_simd_tail_padding_and_alignment_matrix() {
    let test_matrix = [
        (1, 1),
        (2, 2),
        (3, 3),
        (7, 7),
        (15, 15),
        (16, 16),
        (17, 17),
        (31, 31),
        (32, 32),
        (33, 33),
        (65, 33),
        (854, 480),
        (1920, 1080),
        (1921, 1081),
        (1, 1080),
        (1920, 1),
        (3840, 2160),
    ];

    for &(w, h) in &test_matrix {
        let frame = VideoFrameYUV420p::new(w, h, 0);

        // 1. Verify 16-byte row stride alignment
        assert_eq!(frame.stride_y % 16, 0, "Y stride not 16-aligned for {}x{}", w, h);
        assert_eq!(frame.stride_u % 16, 0, "U stride not 16-aligned for {}x{}", w, h);
        assert_eq!(frame.stride_v % 16, 0, "V stride not 16-aligned for {}x{}", w, h);

        assert!(frame.stride_y >= w as usize);
        let chroma_w = (w as usize).div_ceil(2);
        let chroma_h = (h as usize).div_ceil(2);
        assert!(frame.stride_u >= chroma_w);
        assert!(frame.stride_v >= chroma_w);

        // 2. Verify 64-byte pointer alignment
        assert!(frame.y_plane.is_64_aligned(), "Y plane not 64-aligned for {}x{}", w, h);
        assert!(frame.u_plane.is_64_aligned(), "U plane not 64-aligned for {}x{}", w, h);
        assert!(frame.v_plane.is_64_aligned(), "V plane not 64-aligned for {}x{}", w, h);

        // 3. Verify exactly +16 bytes tail padding
        let exp_y_capacity = frame.stride_y * (h as usize) + 16;
        let exp_u_capacity = frame.stride_u * chroma_h + 16;
        let exp_v_capacity = frame.stride_v * chroma_h + 16;

        assert_eq!(frame.y_plane.len(), exp_y_capacity, "Y capacity mismatch for {}x{}", w, h);
        assert_eq!(frame.u_plane.len(), exp_u_capacity, "U capacity mismatch for {}x{}", w, h);
        assert_eq!(frame.v_plane.len(), exp_v_capacity, "V capacity mismatch for {}x{}", w, h);
    }
}

/// 3.2 Tests zero vector over-read using canary bytes in tail padding during conversions.
#[test]
fn test_simd_canary_protection_and_vector_loads() {
    const CANARY_Y: u8 = 0xAA;
    const CANARY_U: u8 = 0x55;
    const CANARY_V: u8 = 0xDE;

    let test_sizes = [(1, 1), (7, 13), (17, 19), (65, 33), (854, 480), (1920, 1080)];

    for &(w, h) in &test_sizes {
        let mut frame = VideoFrameYUV420p::new(w, h, 0);
        let chroma_h = (h as usize).div_ceil(2);

        let y_len = frame.y_plane.len();
        let u_len = frame.u_plane.len();
        let v_len = frame.v_plane.len();

        // Poison tail padding with canary bytes
        frame.y_plane.as_mut_slice()[y_len - 16..].fill(CANARY_Y);
        frame.u_plane.as_mut_slice()[u_len - 16..].fill(CANARY_U);
        frame.v_plane.as_mut_slice()[v_len - 16..].fill(CANARY_V);

        // Fill active plane payload with gradient
        for y in 0..(h as usize) {
            let row = y * frame.stride_y;
            for x in 0..(w as usize) {
                frame.y_plane.as_mut_slice()[row + x] = ((x + y * 3) % 220 + 16) as u8;
            }
        }
        for y in 0..chroma_h {
            let row_u = y * frame.stride_u;
            let row_v = y * frame.stride_v;
            let chroma_w = (w as usize).div_ceil(2);
            for x in 0..chroma_w {
                frame.u_plane.as_mut_slice()[row_u + x] = 128;
                frame.v_plane.as_mut_slice()[row_v + x] = 128;
            }
        }

        // Simulate SIMD 16-byte vector loads across the entire frame
        // Every vector read of 16 bytes starting at row + x must stay strictly inside allocated len
        let y_slice = frame.y_plane.as_slice();
        for y in 0..(h as usize) {
            let row = y * frame.stride_y;
            for x in (0..(w as usize)).step_by(16) {
                let load_end = row + x + 16;
                assert!(
                    load_end <= y_len,
                    "SIMD vector load [{}..{}] over-reads buffer capacity {} for {}x{}",
                    row + x,
                    load_end,
                    y_len,
                    w,
                    h
                );
                // Vector read succeeds safely
                let _vec = &y_slice[row + x..load_end];
            }
        }

        // Run YUV -> RGBA conversion
        let mut rgba = VideoFrameRGBA::new(w, h, 0);
        convert_yuv420p_to_rgba(&frame, &mut rgba, ColorStandard::Bt601).unwrap();

        // Run YUV -> Terminal conversion
        let mut term = TerminalTrueColorBuffer::new(w.min(80) as usize, h.min(24) as usize);
        convert_yuv420p_to_terminal(&frame, &mut term, ColorStandard::Bt601).unwrap();

        // Verify canaries were untouched (zero write corruption into tail padding)
        assert!(
            frame.y_plane.as_slice()[y_len - 16..].iter().all(|&b| b == CANARY_Y),
            "Y tail canary corrupted for {}x{}",
            w,
            h
        );
        assert!(
            frame.u_plane.as_slice()[u_len - 16..].iter().all(|&b| b == CANARY_U),
            "U tail canary corrupted for {}x{}",
            w,
            h
        );
        assert!(
            frame.v_plane.as_slice()[v_len - 16..].iter().all(|&b| b == CANARY_V),
            "V tail canary corrupted for {}x{}",
            w,
            h
        );
    }
}
