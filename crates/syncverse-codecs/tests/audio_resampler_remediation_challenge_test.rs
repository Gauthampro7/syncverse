//! Empirical Challenge and Stress Suite for Remediated LinearResampler
//!
//! Specifically validates:
//! 1. Micro-speed ratio scaling formula: N_out ≈ N_in * f_out / (f_in * r) for r in [0.96, 1.04].
//! 2. Strict monotonic relationship: as r increases (0.96 -> 1.04), N_out strictly decreases.
//! 3. Tiny buffer fragmentation: 1-frame buffers streaming continuously under micro-speed.
//! 4. Prime-length buffer fragmentation across a full spectrum of primes.
//! 5. High channel count surround sound: 5.1 (6 channels) and 7.1 (8 channels) orthogonal isolation.
//! 6. Dynamic micro-speed shifts without reset: boundary continuity and click freedom.
//! 7. Extreme speed ratios (0.1x to 10.0x) and rapid oscillatory transitions.

#![deny(warnings)]

use std::f32::consts::PI;
use syncverse_codecs::audio::resampler::LinearResampler;

// ============================================================================
// 1. MICRO-SPEED RATIO SCALING FORMULA VERIFICATION
// ============================================================================

#[test]
fn test_empirical_microspeed_exact_formula() {
    let test_configs: &[(u32, u32, u16)] = &[
        (48000, 48000, 2), // 48k -> 48k stereo
        (44100, 48000, 2), // 44.1k -> 48k stereo
        (48000, 44100, 2), // 48k -> 44.1k stereo
        (96000, 48000, 1), // 96k -> 48k mono
        (48000, 96000, 1), // 48k -> 96k mono
    ];

    let num_in_frames = 4800; // 100ms at 48k

    for &(in_rate, out_rate, channels) in test_configs {
        let ch = channels as usize;
        let input = vec![0.33f32; num_in_frames * ch];
        let resampler = LinearResampler::new(in_rate, out_rate, channels).unwrap();

        // Dense sweep of 41 steps in [0.96, 1.04] (step = 0.002)
        for i in 0..=40 {
            let r = 0.960 + (i as f64) * 0.002;
            resampler.reset();
            resampler.set_speed_ratio(r).unwrap();

            let output = resampler.resample(&input).unwrap();
            let actual_out_frames = output.len() / ch;

            // Theoretical formula: N_out ≈ (N_in * f_out) / (f_in * r)
            let theoretical_out =
                ((num_in_frames as f64) * (out_rate as f64) / ((in_rate as f64) * r)).round() as usize;

            let diff = (actual_out_frames as isize - theoretical_out as isize).abs();
            assert!(
                diff <= 2,
                "Config ({in_rate}->{out_rate}, ch={ch}, r={r:.4}): actual={actual_out_frames}, theoretical={theoretical_out}, diff={diff}"
            );
        }
    }
}

// ============================================================================
// 2. STRICT MONOTONICITY VERIFICATION
// ============================================================================

#[test]
fn test_empirical_strict_monotonicity_sweep() {
    let in_rate = 48000;
    let out_rate = 48000;
    let channels = 2;
    let ch = channels as usize;
    let num_in_frames = 10000; // Large buffer for fine resolution

    let input = vec![0.42f32; num_in_frames * ch];
    let resampler = LinearResampler::new(in_rate, out_rate, channels).unwrap();

    let mut prev_frames: Option<usize> = None;
    let mut prev_ratio: f64 = 0.0;

    // 101 fine steps across [0.960, 1.040] (step = 0.0008)
    for i in 0..=100 {
        let r = 0.960 + (i as f64) * 0.0008;
        resampler.reset();
        resampler.set_speed_ratio(r).unwrap();

        let output = resampler.resample(&input).unwrap();
        let actual_frames = output.len() / ch;

        if let Some(prev) = prev_frames {
            // Monotonic non-increasing property for adjacent micro-steps
            assert!(
                actual_frames <= prev,
                "Monotonicity violation between r={prev_ratio:.5} ({prev} frames) and r={r:.5} ({actual_frames} frames)"
            );

            // Strict decrease check across steps with |Δr| >= 0.002
            if (r - prev_ratio) >= 0.002 {
                assert!(
                    actual_frames < prev,
                    "Strict decrease expected: r={prev_ratio:.5} ({prev} frames) vs r={r:.5} ({actual_frames} frames)"
                );
            }
        }

        prev_frames = Some(actual_frames);
        prev_ratio = r;
    }

    // Direct endpoint check: r=0.960 vs r=1.040
    resampler.reset();
    resampler.set_speed_ratio(0.960).unwrap();
    let out_slow = resampler.resample(&input).unwrap();

    resampler.reset();
    resampler.set_speed_ratio(1.040).unwrap();
    let out_fast = resampler.resample(&input).unwrap();

    let slow_frames = out_slow.len() / ch;
    let fast_frames = out_fast.len() / ch;

    assert!(
        fast_frames < slow_frames,
        "Fast playback (1.04x) frames {fast_frames} must be strictly less than slow (0.96x) frames {slow_frames}"
    );

    let theoretical_slow = (10000.0f64 / 0.960).round() as usize; // ~10417
    let theoretical_fast = (10000.0f64 / 1.040).round() as usize; // ~9615
    assert!((slow_frames as isize - theoretical_slow as isize).abs() <= 2);
    assert!((fast_frames as isize - theoretical_fast as isize).abs() <= 2);
}

// ============================================================================
// 3. 1-FRAME AND PRIME BUFFER FRAGMENTATION UNDER MICRO-SPEED
// ============================================================================

#[test]
fn test_empirical_1_frame_buffer_streaming_under_microspeed() {
    let in_rate = 48000;
    let out_rate = 44100;
    let channels = 2;
    let ch = channels as usize;
    let total_frames = 600;

    // Generate stereo sine wave
    let mut audio = Vec::with_capacity(total_frames * ch);
    for i in 0..total_frames {
        let l = (2.0 * PI * 440.0 * (i as f32) / (in_rate as f32)).sin();
        let r = (2.0 * PI * 880.0 * (i as f32) / (in_rate as f32)).cos();
        audio.push(l);
        audio.push(r);
    }

    let speed_ratios = [0.96, 0.985, 1.0, 1.015, 1.04];

    for &ratio in &speed_ratios {
        // 1. Monolithic processing
        let resampler_mono = LinearResampler::with_speed_ratio(in_rate, out_rate, channels, ratio).unwrap();
        let out_mono = resampler_mono.resample(&audio).unwrap();

        // 2. Streamed 1-frame at a time (total_frames calls with ch samples each)
        let resampler_1f = LinearResampler::with_speed_ratio(in_rate, out_rate, channels, ratio).unwrap();
        let mut out_stream = Vec::new();
        let mut scratch = Vec::new();

        for i in 0..total_frames {
            let frame = &audio[i * ch..(i + 1) * ch];
            resampler_1f.resample_into(frame, &mut scratch).unwrap();
            out_stream.extend_from_slice(&scratch);
        }

        assert_eq!(
            out_mono.len(),
            out_stream.len(),
            "Ratio {ratio}: 1-frame chunk streaming produced {} samples, monolithic produced {}",
            out_stream.len(),
            out_mono.len()
        );

        let mut max_err = 0.0f32;
        for (i, (&m, &s)) in out_mono.iter().zip(out_stream.iter()).enumerate() {
            let err = (m - s).abs();
            if err > max_err {
                max_err = err;
            }
            assert!(
                err < 1e-4,
                "Ratio {ratio}: mismatch at sample {i}: mono={m}, streamed={s}, err={err}"
            );
        }
        assert!(max_err < 1e-4, "Ratio {ratio}: max error was {max_err}");
    }
}

#[test]
fn test_empirical_prime_chunk_spectrum_under_microspeed() {
    let in_rate = 44100;
    let out_rate = 48000;
    let channels = 2;
    let ch = channels as usize;
    let total_frames = 1200;

    let mut audio = Vec::with_capacity(total_frames * ch);
    for i in 0..total_frames {
        let sample = (2.0 * PI * 350.0 * (i as f32) / (in_rate as f32)).sin();
        audio.push(sample);
        audio.push(-sample);
    }

    // Spectrum of primes from 2 to 97
    let primes = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89, 97];
    let ratios = [0.965, 1.035];

    for &ratio in &ratios {
        let resampler_mono = LinearResampler::with_speed_ratio(in_rate, out_rate, channels, ratio).unwrap();
        let out_mono = resampler_mono.resample(&audio).unwrap();

        let resampler_prime = LinearResampler::with_speed_ratio(in_rate, out_rate, channels, ratio).unwrap();
        let mut out_prime = Vec::new();
        let mut scratch = Vec::new();

        let mut offset = 0;
        let mut prime_idx = 0;

        while offset < audio.len() {
            let prime_frames = primes[prime_idx % primes.len()];
            let samples_to_take = (prime_frames * ch).min(audio.len() - offset);
            let chunk = &audio[offset..offset + samples_to_take];

            resampler_prime.resample_into(chunk, &mut scratch).unwrap();
            out_prime.extend_from_slice(&scratch);

            offset += samples_to_take;
            prime_idx += 1;
        }

        assert_eq!(
            out_mono.len(),
            out_prime.len(),
            "Ratio {ratio}: Prime spectrum length mismatch: mono={}, prime={}",
            out_mono.len(),
            out_prime.len()
        );

        for (i, (&m, &p)) in out_mono.iter().zip(out_prime.iter()).enumerate() {
            assert!(
                (m - p).abs() < 1e-4,
                "Ratio {ratio}: Prime chunk mismatch at sample {i}: mono={m}, prime={p}"
            );
        }
    }
}

// ============================================================================
// 4. HIGH CHANNEL COUNTS (5.1 & 7.1 SURROUND) ORTHOGONALITY
// ============================================================================

#[test]
fn test_empirical_surround_sound_channel_isolation() {
    // Test 6-channel (5.1) and 8-channel (7.1) configurations
    let channel_configs: &[u16] = &[6, 8];

    for &channels in channel_configs {
        let ch = channels as usize;
        let in_rate = 48000;
        let out_rate = 44100;
        let num_frames = 1000;

        let resampler = LinearResampler::new(in_rate, out_rate, channels).unwrap();

        // For each channel c_target, put a signal ONLY on c_target and zeros on all others.
        // Verify that in the output, only c_target has non-zero samples!
        for c_target in 0..ch {
            let mut audio = vec![0.0f32; num_frames * ch];
            for f in 0..num_frames {
                audio[f * ch + c_target] = (2.0 * PI * 400.0 * (f as f32) / (in_rate as f32)).sin();
            }

            let output = resampler.resample(&audio).unwrap();
            let out_frames = output.len() / ch;

            for f in 0..out_frames {
                for c in 0..ch {
                    let s = output[f * ch + c];
                    if c == c_target {
                        // Target channel should contain the resampled sine
                        // (value can be anything in [-1.0, 1.0])
                        assert!(!s.is_nan());
                    } else {
                        // Non-target channel MUST be strictly zero (no cross-channel bleed)
                        assert_eq!(
                            s, 0.0,
                            "Cross-channel bleed detected on channel {c} when only channel {c_target} was active (sample {s})"
                        );
                    }
                }
            }
            resampler.reset();
        }
    }
}

// ============================================================================
// 5. DYNAMIC MICRO-SPEED SHIFTS WITHOUT RESET
// ============================================================================

#[test]
fn test_empirical_continuous_speed_steering_and_click_freedom() {
    let in_rate = 48000;
    let out_rate = 48000;
    let channels = 2;
    let ch = channels as usize;
    let chunk_frames = 480; // 10ms per chunk

    let resampler = LinearResampler::new(in_rate, out_rate, channels).unwrap();

    // Generate continuous smooth sine wave across all chunks
    let total_chunks = 40;
    let speed_profile = [
        0.960, 0.965, 0.970, 0.980, 0.990, 1.000, 1.010, 1.020, 1.030, 1.040,
        1.040, 1.035, 1.025, 1.015, 1.005, 0.995, 0.985, 0.975, 0.965, 0.960,
    ];

    let mut full_output = Vec::new();
    let mut scratch = Vec::new();

    let mut global_frame_idx = 0;
    for chunk_idx in 0..total_chunks {
        let r = speed_profile[chunk_idx % speed_profile.len()];
        resampler.set_speed_ratio(r).unwrap();

        let mut chunk = Vec::with_capacity(chunk_frames * ch);
        for _ in 0..chunk_frames {
            let s = (2.0 * PI * 220.0 * (global_frame_idx as f32) / (in_rate as f32)).sin();
            chunk.push(s);
            chunk.push(s);
            global_frame_idx += 1;
        }

        resampler.resample_into(&chunk, &mut scratch).unwrap();
        full_output.extend_from_slice(&scratch);
    }

    // Verify output sanity
    assert!(!full_output.is_empty());
    for &s in &full_output {
        assert!(!s.is_nan());
        assert!((-1.0..=1.0).contains(&s));
    }

    // Verify click freedom: adjacent sample difference must be bounded by derivative
    // For 220 Hz sine at 48 kHz, max derivative is 2*PI*220/48000 ≈ 0.0288.
    // Allow generous margin for speed compression (up to ~0.06), but no discontinuous clicks (e.g. > 0.5)
    let mut max_adjacent_diff = 0.0f32;
    for i in 1..(full_output.len() / ch) {
        for c in 0..ch {
            let diff = (full_output[i * ch + c] - full_output[(i - 1) * ch + c]).abs();
            if diff > max_adjacent_diff {
                max_adjacent_diff = diff;
            }
            assert!(
                diff < 0.10,
                "Potential click detected at frame {i}, ch {c}: adjacent diff was {diff}"
            );
        }
    }
    assert!(max_adjacent_diff < 0.10, "Max adjacent diff: {max_adjacent_diff}");
}

// ============================================================================
// 6. EXTREME SPEED RATIOS AND RAPID TRANSITIONS
// ============================================================================

#[test]
fn test_empirical_extreme_speed_ratios_and_oscillations() {
    let in_rate = 48000;
    let out_rate = 48000;
    let channels = 1;
    let resampler = LinearResampler::new(in_rate, out_rate, channels).unwrap();

    let num_in = 2000;
    let input = vec![0.5f32; num_in];

    // Extreme ratios
    let extreme_cases: &[(f64, usize)] = &[
        (0.1, 20000), // 10x output frames
        (0.25, 8000), // 4x output frames
        (0.5, 4000),  // 2x output frames
        (1.0, 2000),  // 1x output frames
        (2.0, 1000),  // 0.5x output frames
        (4.0, 500),   // 0.25x output frames
        (10.0, 200),  // 0.1x output frames
    ];

    for &(r, expected_out) in extreme_cases {
        resampler.reset();
        resampler.set_speed_ratio(r).unwrap();
        let output = resampler.resample(&input).unwrap();

        let max_deferral = (1.0 / r).ceil() as isize;
        let diff = (output.len() as isize - expected_out as isize).abs();
        assert!(
            diff <= max_deferral.max(2),
            "Extreme ratio {r}: expected ~{expected_out}, got {}, diff={diff}",
            output.len()
        );

        for &s in &output {
            assert!((s - 0.5).abs() < 1e-4);
        }
    }

    // Rapid oscillation: alternate between 0.25 and 4.0 every chunk
    resampler.reset();
    let chunk = vec![0.3f32; 400];
    let mut total_out = 0;
    let mut scratch = Vec::new();

    for i in 0..10 {
        let r = if i % 2 == 0 { 0.25 } else { 4.0 };
        resampler.set_speed_ratio(r).unwrap();
        resampler.resample_into(&chunk, &mut scratch).unwrap();
        total_out += scratch.len();
        for &s in &scratch {
            assert!((s - 0.3).abs() < 1e-4);
        }
    }
    assert!(total_out > 0);
}
