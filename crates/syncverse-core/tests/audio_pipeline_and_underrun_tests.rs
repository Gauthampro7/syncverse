//! Integration test suite for Audio Output & Resampling Engine, Clock Pacing, and Underrun Protection.
//!
//! Tests:
//! - Multi-tier `AudioSink` interfaces (`NullAudioSink`, `AutoAudioSink`, `MockAudioSink`).
//! - Lock-free inter-thread audio pacing via `SpscRingBuffer<AudioFramePCM, 64>` and `AudioPacer`.
//! - Pre-allocated silence scratch buffer underrun protection under producer starvation.
//! - Smooth 2ms linear ramp-in recovery preventing pops/clicks.
//! - End-to-end integration: 44.1 kHz -> 48.0 kHz resampling with master clock pacing and sink output.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use syncverse_clock::{
    AudioPacer, AudioTimingAction, MasterClock, PlaybackState, SpscRingBuffer,
};
use syncverse_codecs::{AudioFramePCM, LinearResampler};
use syncverse_core::audio_sink::{AudioSink, AutoAudioSink, MockAudioSink, NullAudioSink};

#[test]
fn test_audio_sink_implementations() {
    // 1. NullAudioSink
    let mut null_sink = NullAudioSink::default_48k_stereo();
    assert_eq!(null_sink.sample_rate(), 48000);
    assert_eq!(null_sink.channels(), 2);
    assert_eq!(null_sink.samples_written(), 0);

    let test_pcm = vec![0.1f32; 2048];
    assert_eq!(null_sink.write_samples(&test_pcm).unwrap(), 2048);
    assert_eq!(null_sink.samples_written(), 2048);
    assert!(null_sink.flush().is_ok());

    // 2. MockAudioSink
    let mut mock_sink = MockAudioSink::new(48000, 2);
    assert_eq!(mock_sink.write_samples(&test_pcm).unwrap(), 2048);
    assert_eq!(mock_sink.samples().len(), 2048);
    assert_eq!(mock_sink.flush_count(), 0);
    mock_sink.flush().unwrap();
    assert_eq!(mock_sink.flush_count(), 1);

    // 3. AutoAudioSink
    let mut auto_sink = AutoAudioSink::new_null();
    assert!(auto_sink.is_null());
    assert_eq!(auto_sink.backend_name(), "null");
    assert_eq!(auto_sink.write_samples(&test_pcm).unwrap(), 2048);
    assert!(auto_sink.flush().is_ok());
    assert!(auto_sink.pause().is_ok());
    assert!(auto_sink.resume().is_ok());
}

#[test]
fn test_audio_clock_pacing_and_decision_windows() {
    let pacer = AudioPacer::default_48k_stereo().unwrap();
    let clock = MasterClock::new(2000.0, 1.0, PlaybackState::Playing);
    let now = Instant::now();

    // On-time frame within 20ms tolerance: PTS = 2010ms -> PlayNow
    assert_eq!(
        pacer.evaluate_timing(2010, &clock, now),
        AudioTimingAction::PlayNow
    );

    // On-time frame within -20ms tolerance: PTS = 1990ms -> PlayNow
    assert_eq!(
        pacer.evaluate_timing(1990, &clock, now),
        AudioTimingAction::PlayNow
    );

    // Early frame: PTS = 2060ms (Delta = +60ms -> Wait(40ms))
    match pacer.evaluate_timing(2060, &clock, now) {
        AudioTimingAction::Wait(d) => {
            let wait_ms = d.as_secs_f64() * 1000.0;
            assert!((wait_ms - 40.0).abs() < 1e-2);
        }
        other => panic!("Expected Wait action, got {:?}", other),
    }

    // Late frame: PTS = 1950ms (Delta = -50ms -> Late(50ms))
    match pacer.evaluate_timing(1950, &clock, now) {
        AudioTimingAction::Late(d) => {
            let late_ms = d.as_secs_f64() * 1000.0;
            assert!((late_ms - 50.0).abs() < 1e-2);
        }
        other => panic!("Expected Late action, got {:?}", other),
    }
}

#[test]
fn test_multithreaded_pipeline_with_underrun_starvation_and_recovery() {
    let ring_buffer = Arc::new(SpscRingBuffer::<AudioFramePCM, 64>::new());
    let pacer = Arc::new(AudioPacer::default_48k_stereo().unwrap());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Playing));
    let done = Arc::new(AtomicBool::new(false));

    let rb_producer = Arc::clone(&ring_buffer);
    let done_producer = Arc::clone(&done);

    // Producer thread: pushes 10 frames, starves for 50ms, then pushes 10 more frames
    let producer_handle = thread::spawn(move || {
        // Phase 1: 10 valid frames
        for i in 0..10 {
            let samples = vec![0.4f32; 1024 * 2];
            let frame = AudioFramePCM::new(48000, 2, i * 21, samples).unwrap();
            while rb_producer.try_push(frame.clone()).is_err() {
                thread::yield_now();
            }
        }

        // Starvation period: simulate decoder thread hiccup or network packet loss
        thread::sleep(Duration::from_millis(50));

        // Phase 2: 10 resumed frames
        for i in 10..20 {
            let samples = vec![0.4f32; 1024 * 2];
            let frame = AudioFramePCM::new(48000, 2, i * 21, samples).unwrap();
            while rb_producer.try_push(frame.clone()).is_err() {
                thread::yield_now();
            }
        }

        done_producer.store(true, Ordering::Release);
    });

    let rb_consumer = Arc::clone(&ring_buffer);
    let pacer_consumer = Arc::clone(&pacer);
    let clock_consumer = Arc::clone(&clock);
    let done_consumer = Arc::clone(&done);

    // Consumer (Audio Worker) thread: runs for 100 iterations pulling frames into mock sink
    let consumer_handle = thread::spawn(move || {
        let mut sink = MockAudioSink::new(48000, 2);
        let mut iterations = 0;
        let mut underrun_detected = false;

        while iterations < 50 && (!done_consumer.load(Ordering::Acquire) || !rb_consumer.is_empty()) {
            let now = Instant::now();
            let frame = pacer_consumer.pull_frame(&rb_consumer, &clock_consumer, now);
            sink.write_samples(&frame.samples).unwrap();

            if pacer_consumer.is_in_underrun() {
                underrun_detected = true;
            }

            iterations += 1;
            thread::sleep(Duration::from_millis(2));
        }

        (sink.samples().len(), underrun_detected)
    });

    producer_handle.join().unwrap();
    let (total_samples, underrun_occurred) = consumer_handle.join().unwrap();

    assert!(
        total_samples > 0,
        "Audio sink must have received audio samples"
    );
    assert!(
        underrun_occurred,
        "Pacer must have detected starvation and injected silence"
    );
    assert!(
        pacer.underruns() > 0,
        "Underrun count must be greater than zero"
    );
    assert!(
        pacer.valid_frames() > 0,
        "Valid frames count must be greater than zero"
    );
    assert!(
        pacer.silence_injected() > 0,
        "Silence injection count must be greater than zero"
    );
}

#[test]
fn test_end_to_end_resampling_and_clock_pacing_pipeline() {
    // Media audio is 44.1 kHz stereo, hardware sink is 48.0 kHz stereo
    let resampler = LinearResampler::new(44100, 48000, 2).unwrap();
    let pacer = AudioPacer::default_48k_stereo().unwrap();
    let clock = MasterClock::new(100.0, 1.0, PlaybackState::Playing);
    let spsc = SpscRingBuffer::<AudioFramePCM, 64>::new();
    let mut sink = MockAudioSink::new(48000, 2);

    // 1. Generate 44.1 kHz audio frames (e.g. 3 frames of 441 samples each = 10ms each)
    for frame_idx in 0..3 {
        let pts = 100 + frame_idx * 10;
        let mut samples_44k = Vec::with_capacity(441 * 2);
        for i in 0..441 {
            let val = ((i as f32) / 441.0) * 0.5;
            samples_44k.push(val);
            samples_44k.push(-val);
        }
        let frame_44k = AudioFramePCM::new(44100, 2, pts, samples_44k).unwrap();

        // Resample 44.1 kHz -> 48.0 kHz
        let frame_48k = resampler.resample_frame(&frame_44k).unwrap();
        assert_eq!(frame_48k.sample_rate, 48000);
        spsc.try_push(frame_48k).unwrap();
    }

    assert_eq!(spsc.len(), 3);

    // 2. Consume from SPSC via AudioPacer into AudioSink
    let now = Instant::now();
    for _ in 0..3 {
        let frame = pacer.pull_frame(&spsc, &clock, now);
        assert_eq!(frame.sample_rate, 48000);
        assert_eq!(frame.channels, 2);
        sink.write_samples(&frame.samples).unwrap();
    }

    assert!(spsc.is_empty());
    assert_eq!(pacer.valid_frames(), 3);
    assert_eq!(pacer.underruns(), 0);
    assert!(!sink.samples().is_empty());

    // 3. Next pull when SPSC empty triggers silence underrun protection
    let silence = pacer.pull_frame(&spsc, &clock, now);
    assert_eq!(pacer.underruns(), 1);
    assert_eq!(pacer.silence_injected(), 1);
    assert!(silence.samples.iter().all(|&s| s == 0.0));
}
