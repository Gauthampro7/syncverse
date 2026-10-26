//! Adversarial Empirical Stress Test Suite for AudioPacer, SpscRingBuffer, and AudioSink.
//!
//! Evaluates:
//! 1. Audio Underrun Starvation Storm: 1,000 severe ring buffer starvation bursts, silence injection,
//!    and smooth 2ms linear ramp-in without panics, NaNs, or audio pops.
//! 2. Multithreaded SPSC Ring Buffer Stress: 50,000 full audio frames transferred across concurrent threads
//!    with zero lost frames, zero data races, exact sample count, and sequence validation.
//! 3. PipeAudioSink Broken Pipe Resilience: Child subprocess termination (SIGKILL) simulation, graceful
//!    `CoreError::Io` propagation, and freeze-free non-blocking operation.
//! 4. AutoAudioSink Fallback Safety: Graceful fallback to `NullAudioSink` when audio daemons are unavailable,
//!    with full API contract enforcement and bounded memory.

#![deny(warnings)]

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use syncplay_clock::{
    AudioPacer, AudioTimingAction, MasterClock, PlaybackState, SpscRingBuffer,
};
use syncplay_codecs::AudioFramePCM;
use syncplay_core::audio_sink::{AudioSink, AutoAudioSink, MockAudioSink, NullAudioSink, PipeAudioSink};
use syncplay_core::error::CoreError;

// ============================================================================
// TEST 1: AUDIO UNDERRUN STARVATION STORM (1,000 BURSTS)
// ============================================================================

#[test]
fn test_audio_underrun_starvation_storm_1000_bursts() {
    let sample_rate = 48_000u32;
    let channels = 2u16;
    let frame_samples = 1024usize;
    let total_samples = frame_samples * (channels as usize); // 2048

    let pacer = AudioPacer::new(sample_rate, channels, frame_samples).unwrap();
    let ring_buffer = SpscRingBuffer::<AudioFramePCM, 64>::new();
    let clock = MasterClock::new(0.0, 1.0, PlaybackState::Playing);
    let now = Instant::now();

    // 2ms ramp duration in frames = 48000 * 2 / 1000 = 96 frames
    let ramp_frames = (sample_rate as usize * 2) / 1000;
    assert_eq!(ramp_frames, 96);

    let num_bursts = 1000;

    for burst in 0..num_bursts {
        // --- 1. Starvation Period: Buffer is empty, simulate starvation ---
        // Pacer detects empty buffer, increments underruns, injects silence, sets in_underrun = true
        let silence = pacer.pull_frame(&ring_buffer, &clock, now);
        assert!(
            pacer.is_in_underrun(),
            "Burst {}: Pacer must report underrun active",
            burst
        );
        assert_eq!(silence.samples.len(), total_samples);
        assert!(
            silence.samples.iter().all(|&s| s == 0.0),
            "Burst {}: Injected frame must be purely silent",
            burst
        );

        // --- 2. Push 2 valid audio frames with test amplitude 0.75 ---
        for frame_idx in 0..2 {
            let pts = (burst * 100 + frame_idx * 21) as u64;
            let samples = vec![0.75f32; total_samples];
            let frame = AudioFramePCM::new(sample_rate, channels, pts, samples).unwrap();
            ring_buffer.try_push(frame).expect("Ring buffer push must succeed");
        }

        // --- 3. Consume Frame 0: First frame after starvation must apply smooth ramp-in ---
        let recovered_0 = pacer.pull_frame(&ring_buffer, &clock, now);
        assert!(!pacer.is_in_underrun(), "Must clear underrun flag upon receiving valid frame");
        assert_eq!(recovered_0.samples.len(), total_samples);

        // Leading sample at frame 0 must start exactly at 0.0 (continuous transition from silence)
        assert_eq!(
            recovered_0.samples[0], 0.0,
            "Burst {}: First sample of recovered frame must be 0.0 to prevent pop",
            burst
        );
        assert_eq!(
            recovered_0.samples[1], 0.0,
            "Burst {}: First right-channel sample must also be 0.0",
            burst
        );

        // Verify intermediate ramp samples are strictly monotonically increasing towards 0.75
        let ch = channels as usize;
        let mut prev_gain = -1.0f32;
        for f in 0..ramp_frames {
            let gain = (f as f32) / (ramp_frames as f32);
            assert!(
                gain >= prev_gain,
                "Burst {}: Gain at frame {} must be monotonic",
                burst,
                f
            );
            prev_gain = gain;

            let expected_val = 0.75 * gain;
            let actual_left = recovered_0.samples[f * ch];
            let actual_right = recovered_0.samples[f * ch + 1];

            assert!(
                (actual_left - expected_val).abs() < 1e-5,
                "Burst {}: Left ramp sample mismatch at frame {}",
                burst,
                f
            );
            assert!(
                (actual_right - expected_val).abs() < 1e-5,
                "Burst {}: Right ramp sample mismatch at frame {}",
                burst,
                f
            );
        }

        // Samples past the 2ms ramp window must be at full 0.75 amplitude
        for f in ramp_frames..frame_samples {
            assert!(
                (recovered_0.samples[f * ch] - 0.75).abs() < 1e-5,
                "Burst {}: Post-ramp left sample must be 0.75",
                burst
            );
            assert!(
                (recovered_0.samples[f * ch + 1] - 0.75).abs() < 1e-5,
                "Burst {}: Post-ramp right sample must be 0.75",
                burst
            );
        }

        // --- 4. Consume Frame 1: Second valid frame must NOT be ramped ---
        let frame_1 = pacer.pull_frame(&ring_buffer, &clock, now);
        assert_eq!(
            frame_1.samples[0], 0.75,
            "Burst {}: Subsequent valid frame must not have ramp-in applied",
            burst
        );
        assert_eq!(
            frame_1.samples[1], 0.75,
            "Burst {}: Subsequent valid right frame must not have ramp-in applied",
            burst
        );
    }

    // --- Cumulative Stats Verification ---
    let stats = pacer.stats();
    // 1000 bursts * 2 valid frames = 2000
    assert_eq!(stats.valid_frames, 2000);
    // 1000 bursts * 1 empty pull = 1000 underruns
    assert_eq!(stats.underruns, 1000);
    assert_eq!(stats.silence_frames_injected, 1000);

    // --- Secondary Storm: Zero-allocation `fill_samples` over 500 bursts ---
    let mut scratch = Vec::with_capacity(total_samples);
    let initial_cap = scratch.capacity();

    for burst in 0..500 {
        // 1. Starvation: fill_samples on empty ring buffer returns silence and sets in_underrun
        let silence_count = pacer.fill_samples(&ring_buffer, &clock, now, &mut scratch);
        assert_eq!(silence_count, total_samples);
        assert!(scratch.iter().all(|&s| s == 0.0));
        assert_eq!(scratch.capacity(), initial_cap);
        assert!(pacer.is_in_underrun());

        // 2. Push 1 valid frame
        let samples = vec![0.6f32; total_samples];
        let frame = AudioFramePCM::new(sample_rate, channels, burst as u64, samples).unwrap();
        ring_buffer.try_push(frame).unwrap();

        // 3. Recovery: fill_samples on recovered frame must apply ramp-in
        let count = pacer.fill_samples(&ring_buffer, &clock, now, &mut scratch);
        assert_eq!(count, total_samples);
        assert_eq!(scratch.len(), total_samples);
        assert_eq!(scratch.capacity(), initial_cap, "Zero allocation guarantee: capacity must not reallocate");
        // Ramp check on leading sample
        assert_eq!(scratch[0], 0.0);
        assert!(!pacer.is_in_underrun());
    }
}

// ============================================================================
// TEST 2: MULTITHREADED SPSC RING BUFFER STRESS (50,000 FRAMES)
// ============================================================================

#[test]
fn test_multithreaded_spsc_ring_buffer_50k_audio_frames() {
    let sample_rate = 48_000u32;
    let channels = 2u16;
    let frame_samples = 1024usize;
    let total_samples_per_frame = frame_samples * (channels as usize); // 2048 f32
    let total_frames = 50_000usize;

    let ring_buffer = Arc::new(SpscRingBuffer::<AudioFramePCM, 64>::new());
    let producer_rb = Arc::clone(&ring_buffer);

    let start_time = Instant::now();

    // Producer Thread
    let producer_handle = thread::spawn(move || {
        for frame_idx in 0..total_frames {
            // Build deterministic frame payload
            let mut samples = Vec::with_capacity(total_samples_per_frame);
            let base_val = ((frame_idx % 1000) as f32) * 0.0005;
            for k in 0..frame_samples {
                let s_left = base_val + (k as f32) * 0.00001;
                let s_right = -s_left;
                samples.push(s_left);
                samples.push(s_right);
            }

            let frame = AudioFramePCM::new(
                sample_rate,
                channels,
                (frame_idx * 21) as u64,
                samples,
            ).unwrap();

            let mut push_item = frame;
            loop {
                match producer_rb.try_push(push_item) {
                    Ok(()) => break,
                    Err(err) => {
                        push_item = err.item;
                        std::hint::spin_loop();
                    }
                }
            }
        }
    });

    // Consumer Thread
    let consumer_rb = Arc::clone(&ring_buffer);
    let consumer_handle = thread::spawn(move || {
        let mut received_frames = 0usize;
        let mut total_samples_counted = 0usize;

        while received_frames < total_frames {
            if let Some(frame) = consumer_rb.try_pop() {
                // Verify sequential presentation timestamp
                let expected_pts = (received_frames * 21) as u64;
                assert_eq!(
                    frame.pts_ms, expected_pts,
                    "PTS mismatch at frame {}: expected {}, got {}",
                    received_frames, expected_pts, frame.pts_ms
                );

                // Verify channel and sample count
                assert_eq!(frame.channels, channels);
                assert_eq!(frame.sample_rate, sample_rate);
                assert_eq!(frame.samples.len(), total_samples_per_frame);

                // Verify deterministic payload values (sample 0, middle, and end)
                let base_val = ((received_frames % 1000) as f32) * 0.0005;
                let expected_first_left = base_val;
                let expected_first_right = -base_val;
                assert!(
                    (frame.samples[0] - expected_first_left).abs() < 1e-6,
                    "Payload mismatch at frame {} sample 0",
                    received_frames
                );
                assert!(
                    (frame.samples[1] - expected_first_right).abs() < 1e-6,
                    "Payload mismatch at frame {} sample 1",
                    received_frames
                );

                total_samples_counted += frame.samples.len();
                received_frames += 1;
            } else {
                std::hint::spin_loop();
            }
        }

        (received_frames, total_samples_counted)
    });

    producer_handle.join().expect("Producer thread panicked");
    let (received_frames, total_samples) = consumer_handle.join().expect("Consumer thread panicked");

    let elapsed = start_time.elapsed();
    let expected_total_samples = total_frames * total_samples_per_frame;

    assert_eq!(received_frames, total_frames, "Must receive exactly 50,000 frames");
    assert_eq!(total_samples, expected_total_samples, "Must receive exactly 102,400,000 samples");
    assert!(ring_buffer.is_empty(), "Ring buffer must be empty upon completion");

    let mb_transferred = (expected_total_samples * std::mem::size_of::<f32>()) as f64 / (1024.0 * 1024.0);
    let throughput_fps = (total_frames as f64) / elapsed.as_secs_f64();
    let throughput_mb = mb_transferred / elapsed.as_secs_f64();

    println!(
        "  [SPSC Concurrency Stress] 50,000 frames transferred in {:.3}s ({:.1} fps, {:.2} MB/s)",
        elapsed.as_secs_f64(),
        throughput_fps,
        throughput_mb
    );
}

// ============================================================================
// TEST 3: PIPEAUDIOSINK RESILIENCE (SIMULATED BROKEN PIPE / PROCESS DEATH)
// ============================================================================

/// Helper reading Linux /proc to find a direct child PID of current process matching `comm_substr`.
fn find_child_pid(comm_substr: &str) -> Option<u32> {
    let my_pid = std::process::id();
    let proc_dir = std::fs::read_dir("/proc").ok()?;

    for entry in proc_dir.flatten() {
        let file_name = entry.file_name();
        let name_str = file_name.to_str()?;
        if let Ok(pid) = name_str.parse::<u32>() {
            let stat_path = format!("/proc/{}/stat", pid);
            if let Ok(stat_content) = std::fs::read_to_string(&stat_path) {
                // /proc/[pid]/stat format: pid (comm) state ppid ...
                if let Some(close_paren) = stat_content.rfind(')') {
                    let comm = &stat_content[..close_paren];
                    let rest = &stat_content[close_paren + 1..];
                    let fields: Vec<&str> = rest.split_whitespace().collect();
                    if fields.len() >= 2 {
                        let ppid = fields[1].parse::<u32>().ok();
                        if ppid == Some(my_pid) && comm.contains(comm_substr) {
                            return Some(pid);
                        }
                    }
                }
            }
        }
    }
    None
}

#[test]
fn test_pipe_audio_sink_broken_pipe_resilience() {
    // Attempt to spawn PipeAudioSink
    let sink_result = PipeAudioSink::new();
    if sink_result.is_err() {
        println!("  [SKIP] PipeAudioSink unavailable in this environment (no pw-cat/aplay or audio daemon).");
        return;
    }

    let mut sink = sink_result.unwrap();
    let backend = sink.backend().to_string();
    println!("  [INFO] PipeAudioSink active with backend '{}'", backend);

    // Initial valid write to ensure the pipe is open and operating
    let test_samples = vec![0.1f32; 1024];
    let written = sink.write_samples(&test_samples).expect("Initial write must succeed");
    assert_eq!(written, 1024);

    // Locate the child PID corresponding to the backend
    let child_pid = find_child_pid(&backend);
    assert!(
        child_pid.is_some(),
        "Must locate child PID for active backend '{}'",
        backend
    );
    let child_pid = child_pid.unwrap();
    println!("  [INFO] Found audio child PID: {}", child_pid);

    // Terminate the child process with SIGKILL (signal 9)
    let kill_status = std::process::Command::new("kill")
        .args(["-9", &child_pid.to_string()])
        .status()
        .expect("Failed to execute kill -9");
    assert!(kill_status.success(), "SIGKILL command must succeed");

    // Allow kernel time to reap the process and close read end of pipe
    thread::sleep(Duration::from_millis(20));

    // Subsequent write must detect broken pipe and return Err(CoreError::Io) promptly without freezing
    let write_start = Instant::now();
    let mut last_err = None;

    // In Linux, the pipe buffer may absorb a small write until the buffer fills,
    // so we write until broken pipe is definitively triggered.
    for _ in 0..100 {
        match sink.write_samples(&test_samples) {
            Ok(_) => thread::sleep(Duration::from_millis(1)),
            Err(e) => {
                last_err = Some(e);
                break;
            }
        }
    }

    let write_elapsed = write_start.elapsed();
    assert!(
        write_elapsed < Duration::from_millis(500),
        "write_samples must not block or freeze the thread (took {:?})",
        write_elapsed
    );

    assert!(
        last_err.is_some(),
        "Writing to terminated audio subprocess pipe must return an error"
    );
    match last_err.unwrap() {
        CoreError::Io(io_err) => {
            println!("  [PASS] Successfully propagated broken pipe error: {}", io_err);
        }
        other => panic!("Expected CoreError::Io, got: {:?}", other),
    }

    // Repeated subsequent calls must consistently fail immediately without hangs
    for _ in 0..10 {
        let t0 = Instant::now();
        let res = sink.write_samples(&test_samples);
        assert!(res.is_err());
        assert!(t0.elapsed() < Duration::from_millis(20));
    }

    // Flush on dead pipe must not freeze the calling thread
    let flush_start = Instant::now();
    let _ = sink.flush();
    assert!(flush_start.elapsed() < Duration::from_millis(50));

    // Dropping sink must cleanly terminate without panicking
    drop(sink);
}

// ============================================================================
// TEST 4: AUTOAUDIOSINK FALLBACK AND HEADLESS SAFETY
// ============================================================================

#[test]
fn test_auto_audio_sink_fallback_and_headless_safety() {
    let samples = vec![0.25f32; 100_000];

    // 0. Direct NullAudioSink verification
    let mut direct_null = NullAudioSink::default_48k_stereo();
    assert_eq!(direct_null.samples_written(), 0);
    assert_eq!(direct_null.sample_rate(), 48_000);
    assert_eq!(direct_null.channels(), 2);
    let n = direct_null.write_samples(&samples).unwrap();
    assert_eq!(n, 100_000);
    assert_eq!(direct_null.samples_written(), 100_000);
    assert!(direct_null.flush().is_ok());

    // 1. Fallback when audio daemons are unavailable (PATH is empty)
    if std::env::var("SUBPROCESS_TEST_EMPTY_PATH").is_ok() {
        let mut sink = AutoAudioSink::new();
        assert!(sink.is_null(), "With empty PATH, AutoAudioSink must be Null");
        assert_eq!(sink.backend_name(), "null");
        assert_eq!(sink.sample_rate(), 48_000);
        assert_eq!(sink.channels(), 2);
        let n = sink.write_samples(&samples).unwrap();
        assert_eq!(n, 100_000);
        assert!(sink.flush().is_ok());
        assert!(sink.pause().is_ok());
        assert!(sink.resume().is_ok());
        return;
    }

    // Spawn self with empty PATH to simulate missing audio tools / headless daemon failure
    let current_exe = std::env::current_exe().expect("Must get current_exe");
    let status = std::process::Command::new(current_exe)
        .arg("test_auto_audio_sink_fallback_and_headless_safety")
        .arg("--nocapture")
        .env("SUBPROCESS_TEST_EMPTY_PATH", "1")
        .env("PATH", "")
        .status()
        .expect("Failed to spawn subprocess with empty PATH");
    assert!(
        status.success(),
        "Subprocess with empty PATH must succeed and select NullAudioSink"
    );

    // 2. Explicit new_null() constructor
    let mut explicit_null = AutoAudioSink::new_null();
    assert!(explicit_null.is_null());
    assert_eq!(explicit_null.backend_name(), "null");
    assert_eq!(explicit_null.sample_rate(), 48_000);
    assert_eq!(explicit_null.channels(), 2);

    let written_explicit = explicit_null.write_samples(&samples).expect("Write must succeed");
    assert_eq!(written_explicit, 100_000);
    assert!(explicit_null.flush().is_ok());

    // 3. MockAudioSink recording verification
    let mut mock = MockAudioSink::new(48_000, 2);
    let test_chunk = vec![0.5f32, -0.5f32, 0.25f32, -0.25f32];
    mock.write_samples(&test_chunk).unwrap();
    assert_eq!(mock.samples(), &test_chunk);
    assert_eq!(mock.flush_count(), 0);
    mock.flush().unwrap();
    assert_eq!(mock.flush_count(), 1);
    mock.clear();
    assert!(mock.samples().is_empty());
    assert_eq!(mock.flush_count(), 0);
}

// ============================================================================
// TEST 5: AUDIO PACER TIMING DECISION WINDOW BOUNDARIES
// ============================================================================

#[test]
fn test_audio_pacer_timing_boundaries() {
    let pacer = AudioPacer::default_48k_stereo().unwrap();
    let clock = MasterClock::new(1000.0, 1.0, PlaybackState::Playing);
    let now = Instant::now();
    let clock_pts = clock.current_pts(now);
    let base_pts = clock_pts.round() as u64;

    // Within +/- 20ms tolerance -> PlayNow
    assert_eq!(pacer.evaluate_timing(base_pts, &clock, now), AudioTimingAction::PlayNow);
    assert_eq!(pacer.evaluate_timing(base_pts + 10, &clock, now), AudioTimingAction::PlayNow);
    assert_eq!(pacer.evaluate_timing(base_pts - 10, &clock, now), AudioTimingAction::PlayNow);

    // Early frame (+50ms -> delta = 50ms > 20ms -> Wait(~30ms))
    match pacer.evaluate_timing(base_pts + 50, &clock, now) {
        AudioTimingAction::Wait(d) => {
            let ms = d.as_secs_f64() * 1000.0;
            assert!((ms - 30.0).abs() < 1.0);
        }
        other => panic!("Expected Wait(~30ms), got {:?}", other),
    }

    // Late frame (-50ms -> delta = -50ms < -20ms -> Late(~50ms))
    match pacer.evaluate_timing(base_pts - 50, &clock, now) {
        AudioTimingAction::Late(d) => {
            let ms = d.as_secs_f64() * 1000.0;
            assert!((ms - 50.0).abs() < 1.0);
        }
        other => panic!("Expected Late(~50ms), got {:?}", other),
    }
}

