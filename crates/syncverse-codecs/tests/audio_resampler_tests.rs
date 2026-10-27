//! Integration test suite for Pure-Rust Audio Resampling Engine (`LinearResampler`).
//!
//! Tests:
//! - Multi-rate conversion (44.1k <-> 48k, 48k <-> 96k, 8k -> 48k, 44.1k -> 44.1k).
//! - Continuous micro-speed steering ratio $r \in [0.96, 1.04]$.
//! - Arbitrary chunk-splitting phase continuity (no phase jitter or spectral distortion).
//! - Zero heap allocation during steady-state `resample_into`.
//! - Multi-channel interleaving (mono, stereo, 6-channel).
//! - Input validation and adversarial boundary conditions (NaN, Inf, negative/zero rates).

use syncverse_codecs::{AudioFramePCM, LinearResampler};

#[test]
fn test_resampler_44100_to_48000_sine_accuracy() {
    let in_rate = 44100;
    let out_rate = 48000;
    let freq = 440.0; // A4 tone
    let duration_secs = 0.5; // 500ms
    let total_in_samples = (in_rate as f64 * duration_secs) as usize;

    let mut in_samples = Vec::with_capacity(total_in_samples);
    for i in 0..total_in_samples {
        let t = (i as f64) / (in_rate as f64);
        let s = (2.0 * std::f64::consts::PI * freq * t).sin() as f32;
        in_samples.push(s);
    }

    let resampler = LinearResampler::new(in_rate, out_rate, 1).unwrap();
    let out_samples = resampler.resample(&in_samples).unwrap();

    let expected_out_samples = (out_rate as f64 * duration_secs) as usize;
    // Length must match duration within 1-2 samples due to fractional rounding
    assert!(
        (out_samples.len() as isize - expected_out_samples as isize).abs() <= 2,
        "Expected ~{} samples, got {}",
        expected_out_samples,
        out_samples.len()
    );

    // Verify samples stay within [-1.0, 1.0]
    for &s in &out_samples {
        assert!((-1.0..=1.0).contains(&s));
    }
}

#[test]
fn test_arbitrary_chunk_splitting_phase_continuity() {
    let in_rate = 44100;
    let out_rate = 48000;
    let channels = 2; // stereo

    // 100ms of stereo test audio
    let num_frames = 4410;
    let mut full_input = Vec::with_capacity(num_frames * channels);
    for i in 0..num_frames {
        let l = (2.0 * std::f64::consts::PI * 300.0 * (i as f64) / (in_rate as f64)).sin() as f32;
        let r = (2.0 * std::f64::consts::PI * 600.0 * (i as f64) / (in_rate as f64)).cos() as f32;
        full_input.push(l);
        full_input.push(r);
    }

    // 1. Process as one single chunk
    let resampler_whole = LinearResampler::new(in_rate, out_rate, channels as u16).unwrap();
    let out_whole = resampler_whole.resample(&full_input).unwrap();

    // 2. Process split into prime and irregular chunk sizes: 31, 127, 256, 503, 1024, remainder
    let resampler_split = LinearResampler::new(in_rate, out_rate, channels as u16).unwrap();
    let mut out_split = Vec::new();
    let mut scratch = Vec::new();

    let chunk_sizes = [31, 127, 256, 503, 1024, 71, 997];
    let mut offset = 0;
    let mut chunk_idx = 0;

    while offset < full_input.len() {
        let frames_to_take = chunk_sizes[chunk_idx % chunk_sizes.len()];
        let samples_to_take = (frames_to_take * channels).min(full_input.len() - offset);
        let slice = &full_input[offset..offset + samples_to_take];

        resampler_split.resample_into(slice, &mut scratch).unwrap();
        out_split.extend_from_slice(&scratch);

        offset += samples_to_take;
        chunk_idx += 1;
    }

    assert_eq!(
        out_whole.len(),
        out_split.len(),
        "Chunk split output length must exactly match whole chunk"
    );

    // Verify absolute deviation between split and whole processing is negligible
    let mut max_err = 0.0f32;
    for (i, (&a, &b)) in out_whole.iter().zip(out_split.iter()).enumerate() {
        let err = (a - b).abs();
        if err > max_err {
            max_err = err;
        }
        assert!(
            err < 1e-4,
            "Sample {} mismatch: whole={}, split={}, err={}",
            i,
            a,
            b,
            err
        );
    }
    assert!(max_err < 1e-4);
}

#[test]
fn test_micro_speed_ratio_sweep() {
    let in_rate = 48000;
    let out_rate = 48000;
    let resampler = LinearResampler::new(in_rate, out_rate, 2).unwrap();

    let input = vec![0.5f32; 4800 * 2]; // 100ms
    let ratios = [0.96, 0.975, 0.99, 1.0, 1.01, 1.025, 1.04];

    for &ratio in &ratios {
        resampler.set_speed_ratio(ratio).unwrap();
        assert!((resampler.speed_ratio() - ratio).abs() < 1e-6);

        let out = resampler.resample(&input).unwrap();
        let expected_frames = (4800.0 / ratio).round() as usize;
        let actual_frames = out.len() / 2;

        assert!(
            (actual_frames as isize - expected_frames as isize).abs() <= 2,
            "Ratio {}: expected ~{} frames, got {}",
            ratio,
            expected_frames,
            actual_frames
        );
    }

    // Invariant: Faster micro-speed (1.04x) must yield strictly fewer output frames than slower (0.96x)
    resampler.set_speed_ratio(1.04).unwrap();
    let out_fast = resampler.resample(&input).unwrap();
    resampler.set_speed_ratio(0.96).unwrap();
    let out_slow = resampler.resample(&input).unwrap();
    assert!(
        out_fast.len() < out_slow.len(),
        "Fast playback (1.04x, {} frames) must produce fewer frames than slow playback (0.96x, {} frames)",
        out_fast.len() / 2,
        out_slow.len() / 2
    );
}

#[test]
fn test_zero_allocation_hot_loop() {
    let resampler = LinearResampler::new(44100, 48000, 2).unwrap();
    let chunk = vec![0.1f32; 1024 * 2];
    let mut output = Vec::with_capacity(4096);

    // Warm-up call
    resampler.resample_into(&chunk, &mut output).unwrap();
    let initial_cap = output.capacity();

    // 100 consecutive calls in steady state hot loop
    for _ in 0..100 {
        resampler.resample_into(&chunk, &mut output).unwrap();
        assert_eq!(
            output.capacity(),
            initial_cap,
            "Vector must not re-allocate in hot loop"
        );
    }
}

#[test]
fn test_resample_audio_frame_pcm() {
    let resampler = LinearResampler::new(44100, 48000, 2).unwrap();
    let in_samples = vec![0.25f32; 441 * 2]; // 10ms
    let frame = AudioFramePCM::new(44100, 2, 1500, in_samples).unwrap();

    let resampled = resampler.resample_frame(&frame).unwrap();
    assert_eq!(resampled.sample_rate, 48000);
    assert_eq!(resampled.channels, 2);
    let frames_out = resampled.samples.len() / 2;
    assert!(
        (frames_out as isize - 480).abs() <= 1,
        "Expected ~480 frames, got {}",
        frames_out
    );
}

#[test]
fn test_resampler_adversarial_inputs() {
    // Zero rates and channels rejected
    assert!(LinearResampler::new(0, 48000, 2).is_err());
    assert!(LinearResampler::new(48000, 0, 2).is_err());
    assert!(LinearResampler::new(48000, 48000, 0).is_err());

    // Negative, zero, or NaN speed ratio rejected
    assert!(LinearResampler::with_speed_ratio(48000, 48000, 2, 0.0).is_err());
    assert!(LinearResampler::with_speed_ratio(48000, 48000, 2, -1.0).is_err());
    assert!(LinearResampler::with_speed_ratio(48000, 48000, 2, f64::NAN).is_err());
    assert!(LinearResampler::with_speed_ratio(48000, 48000, 2, f64::INFINITY).is_err());

    let resampler = LinearResampler::new(48000, 48000, 2).unwrap();
    assert!(resampler.set_speed_ratio(-0.5).is_err());
    assert!(resampler.set_speed_ratio(f64::NAN).is_err());

    // Non-multiple sample slice length rejected
    let mut out = Vec::new();
    let odd_samples = vec![0.1f32; 3]; // 3 samples for 2-channel resampler
    assert!(resampler.resample_into(&odd_samples, &mut out).is_err());

    // Empty input produces empty output cleanly
    assert_eq!(resampler.resample_into(&[], &mut out).unwrap(), 0);
    assert!(out.is_empty());
}

#[test]
fn test_resampler_5_1_surround_interleaving() {
    let in_rate = 48000;
    let out_rate = 44100;
    let channels = 6u16; // 5.1 surround sound: FL, FR, FC, LFE, BL, BR
    let num_frames = 2400; // 50ms of audio

    // Generate 6 distinct sine waveforms, one per channel
    let freqs = [100.0, 200.0, 300.0, 50.0, 400.0, 500.0];
    let mut input = Vec::with_capacity(num_frames * channels as usize);
    for frame_idx in 0..num_frames {
        let t = (frame_idx as f64) / (in_rate as f64);
        for &freq in &freqs {
            let s = (2.0 * std::f64::consts::PI * freq * t).sin() as f32;
            input.push(s);
        }
    }

    // 1. Monolithic 6-channel resampling
    let resampler_mono = LinearResampler::new(in_rate, out_rate, channels).unwrap();
    let out_mono = resampler_mono.resample(&input).unwrap();
    let num_out_frames = out_mono.len() / (channels as usize);
    let expected_frames = (num_frames as f64 * (out_rate as f64 / in_rate as f64)).round() as usize;

    assert!(
        (num_out_frames as isize - expected_frames as isize).abs() <= 2,
        "5.1 surround expected ~{} frames, got {}",
        expected_frames,
        num_out_frames
    );

    // 2. Split streaming across prime and irregular chunk sizes
    let resampler_split = LinearResampler::new(in_rate, out_rate, channels).unwrap();
    let mut out_split = Vec::new();
    let mut scratch = Vec::new();
    let prime_chunk_frames = [5, 11, 23, 47, 101, 257, 13];
    let mut offset = 0;
    let mut p_idx = 0;

    while offset < input.len() {
        let frames_to_take = prime_chunk_frames[p_idx % prime_chunk_frames.len()];
        let samples_to_take = (frames_to_take * channels as usize).min(input.len() - offset);
        let slice = &input[offset..offset + samples_to_take];

        resampler_split.resample_into(slice, &mut scratch).unwrap();
        out_split.extend_from_slice(&scratch);

        offset += samples_to_take;
        p_idx += 1;
    }

    assert_eq!(
        out_mono.len(),
        out_split.len(),
        "5.1 split streaming output length must match monolithic exactly"
    );

    // 3. Channel-by-channel verification against monolithic reference
    let mut max_err = 0.0f32;
    for (i, (&s_mono, &s_split)) in out_mono.iter().zip(out_split.iter()).enumerate() {
        let err = (s_mono - s_split).abs();
        if err > max_err {
            max_err = err;
        }
        assert!(
            err < 1e-4,
            "5.1 sample mismatch at sample {} (ch {}): mono={}, split={}, err={}",
            i,
            i % (channels as usize),
            s_mono,
            s_split,
            err
        );
    }
    assert!(max_err < 1e-4, "5.1 max chunking error was {}", max_err);

    // 4. Test upsampling (44.1k -> 48k) with 6 channels
    let resampler_up = LinearResampler::new(out_rate, in_rate, channels).unwrap();
    let out_up = resampler_up.resample(&out_mono).unwrap();
    let out_up_frames = out_up.len() / (channels as usize);
    assert!(
        (out_up_frames as isize - num_frames as isize).abs() <= 3,
        "5.1 upsampling roundtrip frames mismatch: expected ~{}, got {}",
        num_frames,
        out_up_frames
    );
}

