//! Adversarial Empirical Challenge Suite for syncplay-codecs.
//!
//! Stress-tests:
//! 1. Frame geometry stress testing with odd dimensions (1x1, 3x3, 17x19, 1921x1081, 1x1080, 1920x1).
//! 2. Zero buffer over-reads with 16-byte stride alignment and +16B SIMD tail padding.
//! 3. Q8 fixed-point arithmetic under extreme boundary values and absence of integer wrapping.
//! 4. AudioFramePCM boundary and malformed inputs.

#![deny(warnings)]

use syncverse_codecs::{
    convert_yuv420p_to_rgba, yuv_to_rgb_bt601, yuv_to_rgb_bt709, AudioFramePCM,
    Bt601Converter, Bt709Converter, CodecError, ColorSpaceConverter, ColorStandard,
    VideoFrameRGBA, VideoFrameYUV420p,
};

// ============================================================================
// 1. ODD FRAME GEOMETRY & BUFFER BOUNDARY STRESS
// ============================================================================

#[test]
fn test_adversarial_odd_dimensions_and_tail_padding() {
    let test_dimensions = [
        (1u32, 1u32, "1x1 absolute minimal frame"),
        (3, 3, "3x3 odd width and odd height"),
        (17, 19, "17x19 prime dimensions"),
        (1, 1080, "1x1080 single column tall frame"),
        (1920, 1, "1920x1 single row wide frame"),
        (7, 13, "7x13 sub-vector odd dimensions"),
        (65, 33, "65x33 non-multiple of 16/32"),
        (854, 480, "854x480 standard 480p widescreen"),
        (1921, 1081, "1921x1081 1080p + 1 pixel edge case"),
        (255, 255, "255x255 odd byte boundary"),
    ];

    for &(width, height, desc) in &test_dimensions {
        let mut yuv = VideoFrameYUV420p::new(width, height, 42);

        // 1. Verify stride alignment: must be multiple of 16
        assert_eq!(
            yuv.stride_y % 16,
            0,
            "stride_y not 16-byte aligned for {}",
            desc
        );
        assert_eq!(
            yuv.stride_u % 16,
            0,
            "stride_u not 16-byte aligned for {}",
            desc
        );
        assert_eq!(
            yuv.stride_v % 16,
            0,
            "stride_v not 16-byte aligned for {}",
            desc
        );

        assert!(yuv.stride_y >= width as usize);
        let chroma_w = (width as usize).div_ceil(2);
        assert!(yuv.stride_u >= chroma_w);
        assert!(yuv.stride_v >= chroma_w);

        // 2. Verify 64-byte alignment
        assert!(yuv.y_plane.is_64_aligned(), "Y plane not 64-byte aligned for {}", desc);
        assert!(yuv.u_plane.is_64_aligned(), "U plane not 64-byte aligned for {}", desc);
        assert!(yuv.v_plane.is_64_aligned(), "V plane not 64-byte aligned for {}", desc);

        // 3. Verify +16B SIMD tail padding in allocated capacity
        let chroma_h = (height as usize).div_ceil(2);
        let exp_y_len = yuv.stride_y * (height as usize) + 16;
        let exp_u_len = yuv.stride_u * chroma_h + 16;
        let exp_v_len = yuv.stride_v * chroma_h + 16;
        assert_eq!(yuv.y_plane.len(), exp_y_len);
        assert_eq!(yuv.u_plane.len(), exp_u_len);
        assert_eq!(yuv.v_plane.len(), exp_v_len);

        // 4. Fill tail padding with canary sentinel bytes
        const CANARY: u8 = 0xDE;
        let y_data = yuv.y_plane.as_mut_slice();
        y_data[exp_y_len - 16..].fill(CANARY);
        let u_data = yuv.u_plane.as_mut_slice();
        u_data[exp_u_len - 16..].fill(CANARY);
        let v_data = yuv.v_plane.as_mut_slice();
        v_data[exp_v_len - 16..].fill(CANARY);

        // Fill active plane data with recognizable gradient
        for y in 0..(height as usize) {
            for x in 0..(width as usize) {
                y_data[y * yuv.stride_y + x] = ((x * 13 + y * 7 + 16) % 220 + 16) as u8;
            }
        }
        for y in 0..chroma_h {
            for x in 0..chroma_w {
                u_data[y * yuv.stride_u + x] = ((x * 17 + y * 19 + 64) % 200 + 16) as u8;
                v_data[y * yuv.stride_v + x] = ((x * 23 + y * 29 + 32) % 200 + 16) as u8;
            }
        }

        // 5. Convert to RGBA for both BT.601 and BT.709
        for standard in [ColorStandard::Bt601, ColorStandard::Bt709] {
            let mut rgba = VideoFrameRGBA::new(width, height, 0);
            assert_eq!(rgba.stride % 64, 0, "RGBA stride not 64-byte aligned");
            assert!(rgba.data.is_64_aligned(), "RGBA data not 64-byte aligned");

            // Conversion must succeed
            convert_yuv420p_to_rgba(&yuv, &mut rgba, standard)
                .unwrap_or_else(|e| panic!("convert_yuv420p_to_rgba failed for {}: {:?}", desc, e));

            // Verify canary tail padding bytes were NOT corrupted by converter
            assert!(
                yuv.y_plane.as_slice()[exp_y_len - 16..].iter().all(|&b| b == CANARY),
                "Y plane tail canary corrupted for {}",
                desc
            );
            assert!(
                yuv.u_plane.as_slice()[exp_u_len - 16..].iter().all(|&b| b == CANARY),
                "U plane tail canary corrupted for {}",
                desc
            );
            assert!(
                yuv.v_plane.as_slice()[exp_v_len - 16..].iter().all(|&b| b == CANARY),
                "V plane tail canary corrupted for {}",
                desc
            );

            // 6. Verify bit-for-bit exact mathematical match against scalar oracle for every visible pixel
            let y_slice = yuv.y_plane.as_slice();
            let u_slice = yuv.u_plane.as_slice();
            let v_slice = yuv.v_plane.as_slice();
            let rgba_data = rgba.data.as_slice();

            for y in 0..(height as usize) {
                for x in 0..(width as usize) {
                    let y_val = y_slice[y * yuv.stride_y + x];
                    let u_val = u_slice[(y >> 1) * yuv.stride_u + (x >> 1)];
                    let v_val = v_slice[(y >> 1) * yuv.stride_v + (x >> 1)];

                    let (exp_r, exp_g, exp_b) = match standard {
                        ColorStandard::Bt601 => yuv_to_rgb_bt601(y_val, u_val, v_val),
                        ColorStandard::Bt709 => yuv_to_rgb_bt709(y_val, u_val, v_val),
                    };

                    let px_idx = y * rgba.stride + x * 4;
                    let act_r = rgba_data[px_idx];
                    let act_g = rgba_data[px_idx + 1];
                    let act_b = rgba_data[px_idx + 2];
                    let act_a = rgba_data[px_idx + 3];

                    assert_eq!(
                        (act_r, act_g, act_b, act_a),
                        (exp_r, exp_g, exp_b, 255),
                        "Mismatch at ({}, {}) in {}: expected ({}, {}, {}), got ({}, {}, {})",
                        x, y, desc, exp_r, exp_g, exp_b, act_r, act_g, act_b
                    );
                }
            }
        }
    }
}

#[test]
fn test_adversarial_traits_dispatch() {
    let conv601 = Bt601Converter::new();
    let conv709 = Bt709Converter::new();

    assert_eq!(conv601.standard(), ColorStandard::Bt601);
    assert_eq!(conv709.standard(), ColorStandard::Bt709);

    let yuv = VideoFrameYUV420p::new(32, 32, 10);
    let mut rgba = VideoFrameRGBA::new(32, 32, 0);

    conv601.convert_frame(&yuv, &mut rgba).unwrap();
    assert_eq!(rgba.pts_ms, 10);

    conv709.convert_frame(&yuv, &mut rgba).unwrap();
    assert_eq!(rgba.pts_ms, 10);
}

#[test]
fn test_adversarial_dimension_mismatch_rejection() {
    let yuv = VideoFrameYUV420p::new(1920, 1080, 0);
    let mut rgba_wrong_w = VideoFrameRGBA::new(1921, 1080, 0);
    let mut rgba_wrong_h = VideoFrameRGBA::new(1920, 1081, 0);

    assert!(matches!(
        convert_yuv420p_to_rgba(&yuv, &mut rgba_wrong_w, ColorStandard::Bt601),
        Err(CodecError::DimensionMismatch { src_w: 1920, src_h: 1080, dst_w: 1921, dst_h: 1080 })
    ));
    assert!(matches!(
        convert_yuv420p_to_rgba(&yuv, &mut rgba_wrong_h, ColorStandard::Bt709),
        Err(CodecError::DimensionMismatch { src_w: 1920, src_h: 1080, dst_w: 1920, dst_h: 1081 })
    ));
}

// ============================================================================
// 2. Q8 FIXED-POINT ARITHMETIC EXTREME BOUNDARIES & OVERFLOW TEST
// ============================================================================

#[test]
fn test_q8_arithmetic_extremes_and_boundary_values() {
    // 2A. Standard references
    assert_eq!(yuv_to_rgb_bt601(16, 128, 128), (0, 0, 0));
    assert_eq!(yuv_to_rgb_bt709(16, 128, 128), (0, 0, 0));
    assert_eq!(yuv_to_rgb_bt601(235, 128, 128), (255, 255, 255));
    assert_eq!(yuv_to_rgb_bt709(235, 128, 128), (255, 255, 255));

    // 2B. Extreme boundary combinations
    let extreme_sample_values = [0u8, 1, 15, 16, 17, 127, 128, 129, 234, 235, 236, 239, 240, 254, 255];

    for &y in &extreme_sample_values {
        for &u in &extreme_sample_values {
            for &v in &extreme_sample_values {
                // Test BT.601: must never panic, wrap, or exceed [0, 255]
                let (r6, g6, b6) = yuv_to_rgb_bt601(y, u, v);
                let _ = (r6, g6, b6);

                // Test BT.709: must never panic, wrap, or exceed [0, 255]
                let (r7, g7, b7) = yuv_to_rgb_bt709(y, u, v);
                let _ = (r7, g7, b7);

                // Sub-black clamping verification
                if y == 0 && u == 128 && v == 128 {
                    assert_eq!((r6, g6, b6), (0, 0, 0));
                    assert_eq!((r7, g7, b7), (0, 0, 0));
                }

                // Super-white clamping verification
                if y == 255 && u == 128 && v == 128 {
                    assert_eq!((r6, g6, b6), (255, 255, 255));
                    assert_eq!((r7, g7, b7), (255, 255, 255));
                }
            }
        }
    }
}

#[test]
fn test_q8_comparison_against_floating_point_reference() {
    // Floating point ITU-R reference:
    // C = Y - 16, D = U - 128, E = V - 128
    // R = 1.164 * C + 1.596 * E
    // G = 1.164 * C - 0.392 * D - 0.813 * E
    // B = 1.164 * C + 2.017 * D
    let fp_bt601 = |y: u8, u: u8, v: u8| -> (u8, u8, u8) {
        let c = (y as f64) - 16.0;
        let d = (u as f64) - 128.0;
        let e = (v as f64) - 128.0;
        let r = (1.164383 * c + 1.596027 * e).round().clamp(0.0, 255.0) as u8;
        let g = (1.164383 * c - 0.391762 * d - 0.812968 * e).round().clamp(0.0, 255.0) as u8;
        let b = (1.164383 * c + 2.017232 * d).round().clamp(0.0, 255.0) as u8;
        (r, g, b)
    };

    // Sample across grid
    for y in (0..=255).step_by(8) {
        for u in (0..=255).step_by(8) {
            for v in (0..=255).step_by(8) {
                let (act_r, act_g, act_b) = yuv_to_rgb_bt601(y, u, v);
                let (exp_r, exp_g, exp_b) = fp_bt601(y, u, v);

                // Integer Q8 approximation allows at most 2 LSB difference from continuous float rounding
                let diff_r = (act_r as i32).abs_diff(exp_r as i32);
                let diff_g = (act_g as i32).abs_diff(exp_g as i32);
                let diff_b = (act_b as i32).abs_diff(exp_b as i32);

                assert!(
                    diff_r <= 2 && diff_g <= 2 && diff_b <= 2,
                    "Excessive discrepancy at Y={}, U={}, V={}: act=({}, {}, {}), exp=({}, {}, {})",
                    y, u, v, act_r, act_g, act_b, exp_r, exp_g, exp_b
                );
            }
        }
    }
}

// ============================================================================
// 3. AUDIO FRAME PCM ADVERSARIAL BOUNDS
// ============================================================================

#[test]
fn test_audio_pcm_adversarial_validation() {
    // 3A. Valid boundaries: exactly -1.0, 0.0, +1.0
    let valid_samples = vec![-1.0f32, 0.0, 1.0, -0.5, 0.5];
    let frame = AudioFramePCM::new(44100, 1, 0, valid_samples).unwrap();
    assert_eq!(frame.sample_rate, 44100);
    assert_eq!(frame.channels, 1);

    // 3B. Invalid samples > 1.0 or < -1.0
    assert!(AudioFramePCM::new(48000, 2, 0, vec![1.0001f32]).is_err());
    assert!(AudioFramePCM::new(48000, 2, 0, vec![-1.0001f32]).is_err());
    assert!(AudioFramePCM::new(48000, 2, 0, vec![f32::INFINITY]).is_err());
    assert!(AudioFramePCM::new(48000, 2, 0, vec![f32::NEG_INFINITY]).is_err());

    assert!(AudioFramePCM::new(48000, 2, 0, vec![f32::NAN]).is_err());

    // 3C. Zero sample rate or channels
    assert!(AudioFramePCM::new(0, 2, 0, vec![0.0]).is_err());
    assert!(AudioFramePCM::new(48000, 0, 0, vec![0.0]).is_err());

    // 3D. Duration calculation
    let stereo_1sec = vec![0.0f32; 96000]; // 48000 samples per channel = 1.0 sec
    let frame1 = AudioFramePCM::new(48000, 2, 0, stereo_1sec).unwrap();
    assert!((frame1.duration_secs() - 1.0).abs() < 1e-6);
}
