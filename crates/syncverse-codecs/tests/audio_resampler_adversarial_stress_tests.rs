//! Empirical Adversarial Stress Test Suite for LinearResampler.
//!
//! Tests:
//! 1. Resampling Ratio Boundaries: 8kHz -> 192kHz, 192kHz -> 8kHz, 44.1kHz -> 48kHz, 48kHz -> 44.1kHz,
//!    8kHz -> 8kHz, 192kHz -> 192kHz, and extreme ratios (8k -> 384k, 384k -> 8k).
//! 2. Speed Ratio Sweeps: fine sweeps across r in [0.96, 1.04], extreme ratios (0.5x, 2.0x, 0.25x, 4.0x),
//!    and mid-stream dynamic speed transitions without reset.
//! 3. Irregular Chunk Boundaries: 1-sample chunks, prime-sized chunks (3, 7, 13, 31, 1024),
//!    and randomized fragmentations, verifying zero phase discontinuities and zero clicks (diff < 1e-4).
//! 4. Hot Loop Allocation Verification: zero heap allocations in steady-state loop verified via
//!    custom global counting allocator.
//! 5. Empty and Corrupt Inputs: zero rates, negative/zero/NaN/Inf speed ratios, empty slices,
//!    non-multiple channel lengths, NaNs/Infs/extremes in audio data, and multi-thread concurrency.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

use syncverse_codecs::{AudioFramePCM, LinearResampler};

// ============================================================================
// GLOBAL COUNTING ALLOCATOR FOR HOT LOOP ALLOCATION VERIFICATION
// ============================================================================

struct CountingAlloc;

static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static TRACK_THIS_THREAD: Cell<bool> = const { Cell::new(false) };
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if TRACK_THIS_THREAD.with(|t| t.get()) {
            ALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
        }
        // SAFETY: Delegating to the standard system allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: Delegating to the standard system allocator.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL_ALLOC: CountingAlloc = CountingAlloc;

// ============================================================================
// 1. RESAMPLING RATIO BOUNDARIES
// ============================================================================

#[test]
fn test_boundary_8k_to_192k_upsampling_24x() {
    let in_rate = 8_000;
    let out_rate = 192_000;
    let channels = 2;
    let resampler = LinearResampler::new(in_rate, out_rate, channels).unwrap();

    // 100ms of stereo audio (800 frames = 1600 samples)
    let num_in_frames = 800;
    let mut chunk1 = Vec::with_capacity(num_in_frames * channels as usize);
    let mut chunk2 = Vec::with_capacity(num_in_frames * channels as usize);
    for i in 0..num_in_frames {
        let t1 = (i as f64) / (in_rate as f64);
        let t2 = ((i + num_in_frames) as f64) / (in_rate as f64);
        chunk1.push((2.0 * std::f64::consts::PI * 250.0 * t1).sin() as f32);
        chunk1.push((2.0 * std::f64::consts::PI * 500.0 * t1).cos() as f32);
        chunk2.push((2.0 * std::f64::consts::PI * 250.0 * t2).sin() as f32);
        chunk2.push((2.0 * std::f64::consts::PI * 500.0 * t2).cos() as f32);
    }

    // First chunk in a continuous stream:
    // Without lookahead past the final frame (index 799), linear interpolation outputs
    // (num_in_frames - 1) * 24 + 1 = 799 * 24 + 1 = 19177 frames, saving the trailing
    // fractional phase (1/24) for the subsequent chunk.
    let mut out1 = Vec::new();
    resampler.resample_into(&chunk1, &mut out1).unwrap();
    let num_out_frames_1 = out1.len() / (channels as usize);
    let expected_first_chunk = (num_in_frames - 1) * 24 + 1;
    assert_eq!(
        num_out_frames_1, expected_first_chunk,
        "First chunk of 24x upsampler must produce exactly (N-1)*24 + 1 frames"
    );

    // Second chunk in steady-state streaming:
    // Seamlessly completes the 23 missing fractional boundary frames between chunk 1 and chunk 2,
    // yielding exactly 800 * 24 = 19200 frames!
    let mut out2 = Vec::new();
    resampler.resample_into(&chunk2, &mut out2).unwrap();
    let num_out_frames_2 = out2.len() / (channels as usize);
    let expected_steady_state = num_in_frames * 24;
    assert_eq!(
        num_out_frames_2, expected_steady_state,
        "Subsequent steady-state chunk of 24x upsampler must produce exactly N*24 frames"
    );

    // Amplitudes strictly bounded
    for &s in out1.iter().chain(out2.iter()) {
        assert!((-1.0..=1.0).contains(&s));
    }
}

#[test]
fn test_boundary_192k_to_8k_downsampling_24x() {
    let in_rate = 192_000;
    let out_rate = 8_000;
    let channels = 2;
    let resampler = LinearResampler::new(in_rate, out_rate, channels).unwrap();

    // 100ms of stereo audio (19200 frames = 38400 samples)
    let num_in_frames = 19_200;
    let mut input = Vec::with_capacity(num_in_frames * channels as usize);
    for i in 0..num_in_frames {
        let t = (i as f64) / (in_rate as f64);
        let s_l = (2.0 * std::f64::consts::PI * 300.0 * t).sin() as f32;
        let s_r = (2.0 * std::f64::consts::PI * 600.0 * t).cos() as f32;
        input.push(s_l);
        input.push(s_r);
    }

    let output = resampler.resample(&input).unwrap();
    let num_out_frames = output.len() / (channels as usize);
    let expected_out_frames = (num_in_frames as f64 * (out_rate as f64 / in_rate as f64)).round() as usize;

    assert!(
        (num_out_frames as isize - expected_out_frames as isize).abs() <= 2,
        "192k->8k: expected ~{} frames, got {}",
        expected_out_frames,
        num_out_frames
    );

    for &s in &output {
        assert!((-1.0..=1.0).contains(&s));
    }
}

#[test]
fn test_boundary_44k1_and_48k_bidirectional_and_identity() {
    let test_cases = [
        (44_100, 48_000, 2),
        (48_000, 44_100, 2),
        (8_000, 8_000, 1),
        (192_000, 192_000, 2),
        (48_000, 96_000, 6),
        (96_000, 48_000, 6),
    ];

    for (in_rate, out_rate, channels) in test_cases {
        let resampler = LinearResampler::new(in_rate, out_rate, channels).unwrap();
        let num_in_frames = 4800;
        let total_samples = num_in_frames * channels as usize;
        let input = vec![0.35f32; total_samples];

        let output = resampler.resample(&input).unwrap();
        let num_out_frames = output.len() / (channels as usize);
        let expected_out_frames = (num_in_frames as f64 * (out_rate as f64 / in_rate as f64)).round() as usize;

        assert!(
            (num_out_frames as isize - expected_out_frames as isize).abs() <= 2,
            "{}->{} (ch={}): expected ~{} frames, got {}",
            in_rate,
            out_rate,
            channels,
            expected_out_frames,
            num_out_frames
        );

        // Constant DC signal must be preserved identically
        for &s in &output {
            assert!(
                (s - 0.35).abs() < 1e-4,
                "DC level altered: expected 0.35, got {}",
                s
            );
        }
    }
}

#[test]
fn test_linear_ramp_reconstruction_mathematical_oracle() {
    // Under linear interpolation, resampling a linear ramp y = a*t + b MUST produce
    // exact linear values matching the theoretical formula at each output sample time.
    let in_rate = 44_100;
    let out_rate = 48_000;
    let channels = 1;
    let resampler = LinearResampler::new(in_rate, out_rate, channels).unwrap();

    let num_in_frames = 2000;
    let mut input = Vec::with_capacity(num_in_frames);
    for i in 0..num_in_frames {
        // Ramp from -0.8 to +0.8
        let s = -0.8 + 1.6 * (i as f32) / (num_in_frames as f32);
        input.push(s);
    }

    let output = resampler.resample(&input).unwrap();
    let step = in_rate as f64 / out_rate as f64;

    for (m, &s_out) in output.iter().enumerate() {
        let t_in = m as f64 * step;
        let expected_s = (-0.8 + 1.6 * (t_in as f32) / (num_in_frames as f32)).clamp(-1.0, 1.0);
        let err = (s_out - expected_s).abs();
        assert!(
            err < 1e-4,
            "Linear ramp error at out index {}: expected {}, got {}, err={}",
            m,
            expected_s,
            s_out,
            err
        );
    }
}

// ============================================================================
// 2. SPEED RATIO SWEEPS
// ============================================================================

#[test]
fn test_fine_speed_ratio_sweep_within_pll_range() {
    let in_rate = 48_000;
    let out_rate = 48_000;
    let channels = 2;
    let resampler = LinearResampler::new(in_rate, out_rate, channels).unwrap();

    let input = vec![0.5f32; 2400 * 2]; // 50ms of stereo

    // Fine sweep from 0.960 to 1.040 in steps of 0.005 (17 steps)
    for step_i in 0..=16 {
        let r = 0.960 + (step_i as f64) * 0.005;
        resampler.set_speed_ratio(r).unwrap();
        assert!((resampler.speed_ratio() - r).abs() < 1e-7);

        let output = resampler.resample(&input).unwrap();
        let expected_frames = (2400.0 / r).round() as usize;
        let actual_frames = output.len() / 2;

        assert!(
            (actual_frames as isize - expected_frames as isize).abs() <= 2,
            "Ratio {}: expected ~{} frames, got {}",
            r,
            expected_frames,
            actual_frames
        );
    }

    // Invariant: r = 1.04 produces fewer output frames than r = 0.96 from the same input
    resampler.set_speed_ratio(1.04).unwrap();
    let out_fast = resampler.resample(&input).unwrap();
    resampler.set_speed_ratio(0.96).unwrap();
    let out_slow = resampler.resample(&input).unwrap();
    assert!(
        out_fast.len() < out_slow.len(),
        "Higher speed ratio 1.04 ({} frames) must produce fewer output frames than 0.96 ({} frames)",
        out_fast.len() / 2,
        out_slow.len() / 2
    );
}

#[test]
fn test_extreme_speed_ratios() {
    let in_rate = 48_000;
    let out_rate = 48_000;
    let channels = 1;
    let resampler = LinearResampler::new(in_rate, out_rate, channels).unwrap();

    let input = vec![0.4f32; 1000];
    let extreme_ratios = [0.25, 0.5, 2.0, 4.0];

    for &r in &extreme_ratios {
        resampler.reset();
        resampler.set_speed_ratio(r).unwrap();
        let output = resampler.resample(&input).unwrap();
        let expected_frames = (1000.0 / r).round() as usize;
        let actual_frames = output.len();
        // First chunk without lookahead can defer up to ceil(1/step) samples to subsequent chunk
        let max_deferral = (1.0 / r).ceil() as isize;

        assert!(
            (actual_frames as isize - expected_frames as isize).abs() <= max_deferral.max(2),
            "Extreme ratio {}: expected ~{} frames, got {}",
            r,
            expected_frames,
            actual_frames
        );

        for &s in &output {
            assert!((s - 0.4).abs() < 1e-4);
        }
    }
}

#[test]
fn test_dynamic_speed_steering_transitions_across_chunks() {
    // Dynamically shift speed ratio on consecutive chunks without resetting
    let in_rate = 44_100;
    let out_rate = 48_000;
    let channels = 2;
    let resampler = LinearResampler::new(in_rate, out_rate, channels).unwrap();

    let chunk_frames = 441; // 10ms chunks
    let mut chunk = Vec::with_capacity(chunk_frames * 2);
    for i in 0..chunk_frames {
        let s = (2.0 * std::f32::consts::PI * 400.0 * (i as f32) / (in_rate as f32)).sin();
        chunk.push(s);
        chunk.push(-s);
    }

    let speed_profile = [1.0, 1.02, 0.98, 1.04, 0.96, 2.0, 0.5, 1.0];
    let mut total_out_frames = 0;
    let mut out_buffer = Vec::new();

    for &speed in &speed_profile {
        resampler.set_speed_ratio(speed).unwrap();
        let written = resampler.resample_into(&chunk, &mut out_buffer).unwrap();
        let frames = written / 2;
        total_out_frames += frames;

        // No NaNs or runaway values
        for &s in &out_buffer {
            assert!(!s.is_nan());
            assert!((-1.0..=1.0).contains(&s));
        }
    }

    assert!(total_out_frames > 0);
}

// ============================================================================
// 3. IRREGULAR CHUNK BOUNDARIES & CLICK DETECTION
// ============================================================================

#[test]
fn test_single_sample_chunk_fragmentation_vs_monolithic() {
    // Stress test: Feed audio 1 frame (1 sample per channel) at a time!
    // Compare against monolithic single-chunk processing of the exact same audio.
    let in_rate = 44_100;
    let out_rate = 48_000;
    let channels = 2; // stereo

    let num_frames = 2000;
    let mut full_audio = Vec::with_capacity(num_frames * 2);
    for i in 0..num_frames {
        let l = (2.0 * std::f64::consts::PI * 440.0 * (i as f64) / (in_rate as f64)).sin() as f32;
        let r = (2.0 * std::f64::consts::PI * 880.0 * (i as f64) / (in_rate as f64)).cos() as f32;
        full_audio.push(l);
        full_audio.push(r);
    }

    // Monolithic
    let resampler_mono = LinearResampler::new(in_rate, out_rate, channels).unwrap();
    let out_mono = resampler_mono.resample(&full_audio).unwrap();

    // 1-frame chunks (2 samples per chunk for stereo)
    let resampler_1sample = LinearResampler::new(in_rate, out_rate, channels).unwrap();
    let mut out_1sample = Vec::new();
    let mut chunk_out = Vec::new();

    for frame_idx in 0..num_frames {
        let frame_slice = &full_audio[frame_idx * 2..(frame_idx + 1) * 2];
        resampler_1sample.resample_into(frame_slice, &mut chunk_out).unwrap();
        out_1sample.extend_from_slice(&chunk_out);
    }

    assert_eq!(
        out_mono.len(),
        out_1sample.len(),
        "1-sample chunk stream length must exactly match monolithic processing"
    );

    let mut max_err = 0.0f32;
    for (i, (&s_mono, &s_1s)) in out_mono.iter().zip(out_1sample.iter()).enumerate() {
        let err = (s_mono - s_1s).abs();
        if err > max_err {
            max_err = err;
        }
        assert!(
            err < 1e-4,
            "1-sample chunk mismatch at index {}: mono={}, 1s={}, err={}",
            i,
            s_mono,
            s_1s,
            err
        );
    }
    assert!(max_err < 1e-4, "Max error between 1-sample chunking and monolithic was {}", max_err);
}

#[test]
fn test_prime_and_irregular_chunk_sizes_and_click_freedom() {
    let in_rate = 48_000;
    let out_rate = 44_100;
    let channels = 1;

    let num_frames = 5000;
    let mut full_audio = Vec::with_capacity(num_frames);
    for i in 0..num_frames {
        let s = (2.0 * std::f64::consts::PI * 350.0 * (i as f64) / (in_rate as f64)).sin() as f32;
        full_audio.push(s);
    }

    // Monolithic reference
    let resampler_mono = LinearResampler::new(in_rate, out_rate, channels).unwrap();
    let out_mono = resampler_mono.resample(&full_audio).unwrap();

    // Prime chunks: 3, 7, 13, 31, 1024
    let prime_chunks = [3, 7, 13, 31, 1024];
    let resampler_prime = LinearResampler::new(in_rate, out_rate, channels).unwrap();
    let mut out_prime = Vec::new();
    let mut scratch = Vec::new();

    let mut offset = 0;
    let mut p_idx = 0;
    while offset < full_audio.len() {
        let take = prime_chunks[p_idx % prime_chunks.len()].min(full_audio.len() - offset);
        resampler_prime.resample_into(&full_audio[offset..offset + take], &mut scratch).unwrap();
        out_prime.extend_from_slice(&scratch);
        offset += take;
        p_idx += 1;
    }

    assert_eq!(out_mono.len(), out_prime.len());

    // Check click-freedom: verify delta between adjacent samples matches monolithic reference
    let mut max_diff_err = 0.0f32;
    for i in 1..out_mono.len() {
        let diff_mono = out_mono[i] - out_mono[i - 1];
        let diff_prime = out_prime[i] - out_prime[i - 1];
        let diff_err = (diff_mono - diff_prime).abs();
        if diff_err > max_diff_err {
            max_diff_err = diff_err;
        }
        assert!(
            diff_err < 1e-4,
            "Phase click/discontinuity at sample {}: diff_mono={}, diff_prime={}",
            i,
            diff_mono,
            diff_prime
        );
    }
    assert!(max_diff_err < 1e-4);
}

// ============================================================================
// 4. HOT LOOP ALLOCATION VERIFICATION
// ============================================================================

#[test]
fn test_zero_heap_allocation_in_hot_loop() {
    let in_rate = 44_100;
    let out_rate = 48_000;
    let channels = 2;
    let resampler = LinearResampler::new(in_rate, out_rate, channels).unwrap();

    let chunk = vec![0.125f32; 1024 * 2]; // 1024 frames of stereo
    let mut preallocated_output = Vec::with_capacity(4096);

    // Warm-up call: initializes any internal structures
    resampler.resample_into(&chunk, &mut preallocated_output).unwrap();
    let cap_initial = preallocated_output.capacity();

    // Enable thread-local allocation tracking on this thread ONLY
    TRACK_THIS_THREAD.with(|t| t.set(true));
    let alloc_start = ALLOC_COUNT.load(Ordering::SeqCst);

    // Execute 1,000 hot-loop iterations
    let iterations = 1000;
    for _ in 0..iterations {
        let count = resampler.resample_into(&chunk, &mut preallocated_output).unwrap();
        assert!(count > 0);
        assert_eq!(
            preallocated_output.capacity(),
            cap_initial,
            "Output vector capacity must not increase during hot loop"
        );
    }

    let alloc_end = ALLOC_COUNT.load(Ordering::SeqCst);
    TRACK_THIS_THREAD.with(|t| t.set(false));
    let total_allocations = alloc_end - alloc_start;

    assert_eq!(
        total_allocations, 0,
        "EMPIRICAL VIOLATION: resample_into triggered {} heap allocations across {} hot loop iterations!",
        total_allocations, iterations
    );
}

// ============================================================================
// 5. EMPTY AND CORRUPT INPUTS
// ============================================================================

#[test]
fn test_invalid_parameters_and_corrupt_inputs() {
    // 1. Zero rates or channels must return CodecError without panic
    assert!(LinearResampler::new(0, 48000, 2).is_err());
    assert!(LinearResampler::new(48000, 0, 2).is_err());
    assert!(LinearResampler::new(48000, 48000, 0).is_err());

    // 2. Invalid speed ratios must return CodecError without panic
    assert!(LinearResampler::with_speed_ratio(48000, 48000, 2, 0.0).is_err());
    assert!(LinearResampler::with_speed_ratio(48000, 48000, 2, -1.0).is_err());
    assert!(LinearResampler::with_speed_ratio(48000, 48000, 2, f64::NAN).is_err());
    assert!(LinearResampler::with_speed_ratio(48000, 48000, 2, f64::INFINITY).is_err());
    assert!(LinearResampler::with_speed_ratio(48000, 48000, 2, f64::NEG_INFINITY).is_err());

    let resampler = LinearResampler::new(48000, 48000, 2).unwrap();
    assert!(resampler.set_speed_ratio(0.0).is_err());
    assert!(resampler.set_speed_ratio(-0.1).is_err());
    assert!(resampler.set_speed_ratio(f64::NAN).is_err());
    assert!(resampler.set_speed_ratio(f64::INFINITY).is_err());

    // 3. Slice length not multiple of channels
    let mut out = Vec::new();
    assert!(resampler.resample_into(&[0.1, 0.2, 0.3], &mut out).is_err());

    // 4. Empty slice handling: must return Ok(0) and keep output empty
    assert_eq!(resampler.resample_into(&[], &mut out).unwrap(), 0);
    assert!(out.is_empty());

    // 5. Empty slice mid-stream does not corrupt phase or history
    let chunk = vec![0.5f32; 100 * 2];
    resampler.resample_into(&chunk, &mut out).unwrap();
    let out_len_before = out.len();
    assert!(out_len_before > 0);

    // Empty slice in middle
    assert_eq!(resampler.resample_into(&[], &mut out).unwrap(), 0);
    assert!(out.is_empty());

    // Subsequent normal slice continues cleanly
    resampler.resample_into(&chunk, &mut out).unwrap();
    assert_eq!(out.len(), out_len_before);

    // 6. Extreme sample values clamped without panic
    let extreme_chunk = vec![1e10f32, -1e10f32, 50.0f32, -50.0f32];
    resampler.resample_into(&extreme_chunk, &mut out).unwrap();
    for &s in &out {
        assert!((-1.0..=1.0).contains(&s), "Extreme sample was not clamped: {}", s);
    }

    // 7. AudioFramePCM mismatch detection
    let frame_wrong_rate = AudioFramePCM::new(44100, 2, 0, vec![0.0; 200]).unwrap();
    assert!(resampler.resample_frame(&frame_wrong_rate).is_err());

    let frame_wrong_channels = AudioFramePCM::new(48000, 1, 0, vec![0.0; 100]).unwrap();
    assert!(resampler.resample_frame(&frame_wrong_channels).is_err());
}

#[test]
fn test_multithreaded_concurrent_access_stress() {
    let resampler = Arc::new(LinearResampler::new(44100, 48000, 2).unwrap());
    let mut handles = Vec::new();

    for thread_id in 0..8 {
        let resampler_clone = Arc::clone(&resampler);
        let handle = thread::spawn(move || {
            let chunk = vec![0.1f32 * (thread_id as f32); 256 * 2];
            let mut out = Vec::with_capacity(1024);
            for i in 0..100 {
                if i % 10 == 0 {
                    let _ = resampler_clone.set_speed_ratio(1.0 + 0.01 * ((thread_id % 3) as f64));
                }
                let count = resampler_clone.resample_into(&chunk, &mut out).unwrap();
                assert!(count > 0);
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("Worker thread panicked under concurrency");
    }
}

#[test]
fn test_8k_to_192k_single_sample_and_prime_chunking() {
    let in_rate = 8_000;
    let out_rate = 192_000;
    let channels = 1;

    let num_frames = 200; // 25ms of 8kHz
    let mut full_audio = Vec::with_capacity(num_frames);
    for i in 0..num_frames {
        let s = (2.0 * std::f64::consts::PI * 300.0 * (i as f64) / (in_rate as f64)).sin() as f32;
        full_audio.push(s);
    }

    // Monolithic
    let resampler_mono = LinearResampler::new(in_rate, out_rate, channels).unwrap();
    let out_mono = resampler_mono.resample(&full_audio).unwrap();

    // 1-sample chunks
    let resampler_1s = LinearResampler::new(in_rate, out_rate, channels).unwrap();
    let mut out_1s = Vec::new();
    let mut scratch = Vec::new();

    for &sample in &full_audio {
        resampler_1s.resample_into(&[sample], &mut scratch).unwrap();
        out_1s.extend_from_slice(&scratch);
    }

    assert_eq!(out_mono.len(), out_1s.len(), "8k->192k: 1-sample chunk length mismatch");
    for (i, (&m, &s)) in out_mono.iter().zip(out_1s.iter()).enumerate() {
        assert!(
            (m - s).abs() < 1e-4,
            "8k->192k 1-sample mismatch at index {}: mono={}, 1s={}",
            i,
            m,
            s
        );
    }

    // Prime chunks: 3, 7, 13, 31
    let prime_sizes = [3, 7, 13, 31];
    let resampler_prime = LinearResampler::new(in_rate, out_rate, channels).unwrap();
    let mut out_prime = Vec::new();
    let mut offset = 0;
    let mut p_idx = 0;

    while offset < full_audio.len() {
        let take = prime_sizes[p_idx % prime_sizes.len()].min(full_audio.len() - offset);
        resampler_prime.resample_into(&full_audio[offset..offset + take], &mut scratch).unwrap();
        out_prime.extend_from_slice(&scratch);
        offset += take;
        p_idx += 1;
    }

    assert_eq!(out_mono.len(), out_prime.len(), "8k->192k: prime chunk length mismatch");
    for (i, (&m, &s)) in out_mono.iter().zip(out_prime.iter()).enumerate() {
        assert!(
            (m - s).abs() < 1e-4,
            "8k->192k prime chunk mismatch at index {}: mono={}, prime={}",
            i,
            m,
            s
        );
    }
}

#[test]
fn test_192k_to_8k_single_sample_and_prime_chunking() {
    let in_rate = 192_000;
    let out_rate = 8_000;
    let channels = 1;

    let num_frames = 2400; // 12.5ms of 192kHz
    let mut full_audio = Vec::with_capacity(num_frames);
    for i in 0..num_frames {
        let s = (2.0 * std::f64::consts::PI * 400.0 * (i as f64) / (in_rate as f64)).sin() as f32;
        full_audio.push(s);
    }

    // Monolithic
    let resampler_mono = LinearResampler::new(in_rate, out_rate, channels).unwrap();
    let out_mono = resampler_mono.resample(&full_audio).unwrap();

    // 1-sample chunks
    let resampler_1s = LinearResampler::new(in_rate, out_rate, channels).unwrap();
    let mut out_1s = Vec::new();
    let mut scratch = Vec::new();

    for &sample in &full_audio {
        resampler_1s.resample_into(&[sample], &mut scratch).unwrap();
        out_1s.extend_from_slice(&scratch);
    }

    assert_eq!(out_mono.len(), out_1s.len(), "192k->8k: 1-sample chunk length mismatch");
    for (i, (&m, &s)) in out_mono.iter().zip(out_1s.iter()).enumerate() {
        assert!(
            (m - s).abs() < 1e-4,
            "192k->8k 1-sample mismatch at index {}: mono={}, 1s={}",
            i,
            m,
            s
        );
    }

    // Prime chunks: 3, 7, 13, 31, 1024
    let prime_sizes = [3, 7, 13, 31, 1024];
    let resampler_prime = LinearResampler::new(in_rate, out_rate, channels).unwrap();
    let mut out_prime = Vec::new();
    let mut offset = 0;
    let mut p_idx = 0;

    while offset < full_audio.len() {
        let take = prime_sizes[p_idx % prime_sizes.len()].min(full_audio.len() - offset);
        resampler_prime.resample_into(&full_audio[offset..offset + take], &mut scratch).unwrap();
        out_prime.extend_from_slice(&scratch);
        offset += take;
        p_idx += 1;
    }

    assert_eq!(out_mono.len(), out_prime.len(), "192k->8k: prime chunk length mismatch");
    for (i, (&m, &s)) in out_mono.iter().zip(out_prime.iter()).enumerate() {
        assert!(
            (m - s).abs() < 1e-4,
            "192k->8k prime chunk mismatch at index {}: mono={}, prime={}",
            i,
            m,
            s
        );
    }
}

#[test]
fn test_resampler_reset_invariants() {
    let resampler = LinearResampler::new(44100, 48000, 2).unwrap();
    let chunk1 = vec![0.3f32; 500 * 2];
    let chunk2 = vec![0.7f32; 500 * 2];

    let mut out1 = Vec::new();
    let mut out2 = Vec::new();

    // Process chunk 1
    resampler.resample_into(&chunk1, &mut out1).unwrap();

    // Call reset
    resampler.reset();

    // Process chunk 2 on reset instance
    resampler.resample_into(&chunk2, &mut out2).unwrap();

    // Process chunk 2 on brand-new instance
    let fresh_resampler = LinearResampler::new(44100, 48000, 2).unwrap();
    let mut fresh_out = Vec::new();
    fresh_resampler.resample_into(&chunk2, &mut fresh_out).unwrap();

    // Must be bit-for-bit identical!
    assert_eq!(out2.len(), fresh_out.len());
    for (a, b) in out2.iter().zip(fresh_out.iter()) {
        assert_eq!(a.to_bits(), b.to_bits(), "reset() did not perfectly restore initial state");
    }
}

#[test]
fn test_multichannel_5_1_and_7_1_prime_chunking() {
    for &ch in &[6u16, 8u16] {
        let in_rate = 48000;
        let out_rate = 44100;
        let resampler_mono = LinearResampler::new(in_rate, out_rate, ch).unwrap();
        let resampler_prime = LinearResampler::new(in_rate, out_rate, ch).unwrap();

        let num_frames = 1000;
        let mut audio = Vec::with_capacity(num_frames * ch as usize);
        for f in 0..num_frames {
            for c in 0..ch {
                let sample = ((f * (c as usize + 1)) as f32 * 0.01).sin();
                audio.push(sample);
            }
        }

        let out_mono = resampler_mono.resample(&audio).unwrap();

        let prime_frames = [3, 7, 13, 31, 127];
        let mut out_prime = Vec::new();
        let mut scratch = Vec::new();
        let mut offset = 0;
        let mut p_idx = 0;

        while offset < audio.len() {
            let frames_take = prime_frames[p_idx % prime_frames.len()];
            let samples_take = (frames_take * ch as usize).min(audio.len() - offset);
            resampler_prime.resample_into(&audio[offset..offset + samples_take], &mut scratch).unwrap();
            out_prime.extend_from_slice(&scratch);
            offset += samples_take;
            p_idx += 1;
        }

        assert_eq!(out_mono.len(), out_prime.len(), "Channel {} length mismatch", ch);
        for (i, (&m, &p)) in out_mono.iter().zip(out_prime.iter()).enumerate() {
            assert!(
                (m - p).abs() < 1e-4,
                "Channel {} mismatch at index {}: mono={}, prime={}",
                ch,
                i,
                m,
                p
            );
        }
    }
}

#[test]
fn test_nan_and_inf_sample_resilience() {
    let resampler = LinearResampler::new(48000, 48000, 2).unwrap();
    let mut out = Vec::new();

    // Input with NaN and Infinity
    let corrupt_input = vec![0.5f32, f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.25f32, -0.25f32];
    let result = resampler.resample_into(&corrupt_input, &mut out);

    // Must not panic!
    assert!(result.is_ok());
    assert_eq!(out.len(), corrupt_input.len());
}
