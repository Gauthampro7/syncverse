//! Adversarial empirical challenge tests for `syncplay-ui`.
//!
//! Validates:
//! 1. High-concurrency snapshot exchange stress test (10 kHz publisher, 120 Hz reader, 0 torn reads).
//! 2. Scrubber debouncer event flooding (120 Hz, 240 Hz, 1000 Hz over 50ms, 500ms, 2000ms).
//! 3. Bounded chat overlay memory under 10,000-message burst and 5.0s lifecycle eviction.
//! 4. Alpha8 font atlas texture footprint and 15.0MB total UI RAM ceiling.
//! 5. OSD transport glyph exponential decay and tri-color sync health diagnostics.

#![deny(warnings)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use syncverse_ui::{
    evaluate_sync_health, format_timecode_hud, BoundedChatOverlay, FontAtlasAlpha8,
    PlayerUiSnapshot, ScrubberDebouncer, SyncBadgeColor, TransportGlyph, TransportGlyphKind,
    TripleBufferSnapshotExchange,
};

// ============================================================================
// 1. HIGH-CONCURRENCY SNAPSHOT EXCHANGE STRESS TEST
// ============================================================================

#[test]
fn test_triple_buffer_high_concurrency_stress_10khz_120hz() {
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let ex_pub = Arc::clone(&exchange);

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_cons = Arc::clone(&stop_flag);

    let total_published = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let total_pub_clone = Arc::clone(&total_published);

    // Producer thread: publishes at ~10 kHz (100 µs interval) for 1.0 second (>= 10,000 snapshots)
    let producer = thread::spawn(move || {
        let start = Instant::now();
        let target_duration = Duration::from_millis(1000);
        let mut seq: u64 = 1;

        while start.elapsed() < target_duration {
            let snap = PlayerUiSnapshot {
                monotonic_timestamp: seq,
                media_position_pts: seq * 1_000_000,
                media_duration_pts: 3_600_000_000_000,
                playback_state: seq % 4,
                network_rtt_nanos: 50_000_000 + (seq % 10_000_000),
                clock_offset_nanos: 15_000_000,
                phase_error_nanos: -(seq as i64),
                pll_speed_ratio_q16: (65536 + (seq % 1000)) as u32,
                peer_count: 5,
                peers_ready: 5,
                room_sequence: seq,
            };

            ex_pub.publish(snap);
            total_pub_clone.fetch_add(1, Ordering::Relaxed);
            seq += 1;

            // Target ~10 kHz pacing: sleep 100 µs (or spin/yield)
            thread::sleep(Duration::from_micros(100));
        }

        stop_flag.store(true, Ordering::Release);
        seq - 1
    });

    // Consumer thread: polls at ~120 Hz (8.33 ms interval)
    let consumer = thread::spawn(move || {
        let mut poll_count: usize = 0;
        let mut torn_reads: usize = 0;
        let mut prev_seq: u64 = 0;
        let mut non_monotonic_reads: usize = 0;

        while !stop_cons.load(Ordering::Acquire) {
            let snap = exchange.poll_latest_snapshot();
            if snap.room_sequence == 0 {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            poll_count += 1;

            // Integrity verification: multi-field atomic consistency
            let is_consistent = snap.monotonic_timestamp == snap.room_sequence
                && snap.media_position_pts == snap.room_sequence * 1_000_000
                && snap.playback_state == (snap.room_sequence % 4)
                && snap.phase_error_nanos == -(snap.room_sequence as i64)
                && snap.pll_speed_ratio_q16 == (65536 + (snap.room_sequence % 1000)) as u32;

            if !is_consistent {
                println!("Torn snapshot observed: {:?}", snap);
                torn_reads += 1;
            }

            if snap.room_sequence < prev_seq {
                println!(
                    "Non-monotonic read: prev_seq={}, current_seq={}",
                    prev_seq, snap.room_sequence
                );
                non_monotonic_reads += 1;
            }
            prev_seq = snap.room_sequence;

            // 120 Hz polling: ~8.33 ms
            thread::sleep(Duration::from_millis(8));
        }

        (poll_count, torn_reads, non_monotonic_reads)
    });

    let published_count = producer.join().expect("Producer thread panicked");
    let (polled_count, torn_count, non_monotonic_count) =
        consumer.join().expect("Consumer thread panicked");

    assert!(
        published_count >= 5_000,
        "Producer should publish at high throughput (published: {})",
        published_count
    );
    assert!(
        polled_count >= 50,
        "Consumer should poll regularly at ~120 Hz (polled: {})",
        polled_count
    );
    assert_eq!(
        torn_count, 0,
        "Encountered {} torn/inconsistent reads under 10 kHz concurrency!",
        torn_count
    );
    assert_eq!(
        non_monotonic_count, 0,
        "Encountered {} non-monotonic snapshot sequences!",
        non_monotonic_count
    );
}

#[test]
fn test_triple_buffer_extreme_spin_race() {
    // Zero-delay tight loop race condition stress test:
    // Forces concurrent execution of every atomic instruction without thread sleep
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let ex_pub = Arc::clone(&exchange);

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_cons = Arc::clone(&stop_flag);

    let iterations = 100_000u64;

    let producer = thread::spawn(move || {
        for seq in 1..=iterations {
            let snap = PlayerUiSnapshot {
                monotonic_timestamp: seq,
                media_position_pts: seq * 100,
                playback_state: seq % 4,
                room_sequence: seq,
                ..Default::default()
            };
            ex_pub.publish(snap);
        }
        stop_flag.store(true, Ordering::Release);
    });

    let consumer = thread::spawn(move || {
        let mut poll_count: usize = 0;
        let mut torn_reads: usize = 0;
        let mut prev_seq: u64 = 0;
        let mut regressions: usize = 0;

        while !stop_cons.load(Ordering::Acquire) {
            let snap = exchange.poll_latest_snapshot();
            poll_count += 1;

            if !snap.is_internally_consistent()
                || snap.media_position_pts != snap.room_sequence * 100
                || snap.playback_state != (snap.room_sequence % 4)
            {
                torn_reads += 1;
            }

            if snap.room_sequence < prev_seq {
                regressions += 1;
            }
            prev_seq = snap.room_sequence;
        }

        (poll_count, torn_reads, regressions)
    });

    producer.join().expect("Producer thread panicked");
    let (polls, bad, regs) = consumer.join().expect("Consumer thread panicked");

    println!(
        "Spin race finished: polls={}, torn_reads={}, regressions={}",
        polls, bad, regs
    );
    assert_eq!(bad, 0, "Extreme spin race produced {} torn snapshots", bad);
    assert_eq!(regs, 0, "Extreme spin race produced {} sequence regressions", regs);
}

// ============================================================================
// 2. SCRUBBER DEBOUNCER ADVERSARIAL EVENT FLOODING
// ============================================================================

#[test]
fn test_scrubber_event_flooding_120hz_500ms() {
    let mut debouncer = ScrubberDebouncer::new(100.0);
    let total_drag_ms = 500.0;
    let event_interval_ms = 1000.0 / 120.0; // 8.333ms

    let mut intermediate_commands = Vec::new();
    let mut t = 0.0;
    while t <= total_drag_ms {
        if let Some(seek_t) = debouncer.on_drag_move(t) {
            intermediate_commands.push(seek_t);
        }
        t += event_interval_ms;
    }

    // Release at t = 505.0 ms
    let release_seek = debouncer.on_drag_release(505.0);

    // Exactly 5 intermediate seeks + 1 authoritative release seek = 6 commands
    assert_eq!(
        intermediate_commands.len(),
        5,
        "120 Hz 500ms drag must emit exactly 5 intermediate seeks"
    );
    assert_eq!(debouncer.visual_updates_count(), 61);
    assert_eq!(release_seek, 505.0);
}

#[test]
fn test_scrubber_event_flooding_240hz_500ms() {
    let mut debouncer = ScrubberDebouncer::new(100.0);
    let total_drag_ms = 500.0;
    let event_interval_ms = 1000.0 / 240.0; // 4.167ms

    let mut intermediate_commands = Vec::new();
    let mut t = 0.0;
    // Account for float accumulation at boundary
    while t <= total_drag_ms + 1e-6 {
        if let Some(seek_t) = debouncer.on_drag_move(t) {
            intermediate_commands.push(seek_t);
        }
        t += event_interval_ms;
    }

    let release_seek = debouncer.on_drag_release(505.0);

    // 240 Hz emits exactly 5 intermediate seeks over 500ms drag
    assert_eq!(
        intermediate_commands.len(),
        5,
        "240 Hz 500ms drag must emit exactly 5 intermediate seeks"
    );
    assert_eq!(debouncer.visual_updates_count(), 121);
    assert_eq!(release_seek, 505.0);
}

#[test]
fn test_scrubber_event_flooding_1000hz_500ms() {
    let mut debouncer = ScrubberDebouncer::new(100.0);
    let total_drag_ms = 500.0;
    let event_interval_ms = 1.0; // 1.0ms = 1000 Hz

    let mut intermediate_commands = Vec::new();
    let mut t = 0.0;
    // Drag move events occur during active drag [0.0, 500.0)
    while t < total_drag_ms {
        if let Some(seek_t) = debouncer.on_drag_move(t) {
            intermediate_commands.push(seek_t);
        }
        t += event_interval_ms;
    }

    // Authoritative release at t = 500.0ms
    let release_seek = debouncer.on_drag_release(500.0);

    assert_eq!(
        intermediate_commands.len(),
        5,
        "1000 Hz 500ms drag must emit exactly 5 intermediate seeks"
    );
    assert_eq!(debouncer.visual_updates_count(), 500);
    assert_eq!(release_seek, 500.0);
}

#[test]
fn test_scrubber_short_and_long_drag_intervals() {
    // 50ms short flick at 120Hz, 240Hz, 1000Hz
    for &hz in &[120.0, 240.0, 1000.0] {
        let mut debouncer = ScrubberDebouncer::new(100.0);
        let dt = 1000.0 / hz;
        let mut t = 0.0;
        let mut intermediate = Vec::new();
        while t < 50.0 {
            if let Some(st) = debouncer.on_drag_move(t) {
                intermediate.push(st);
            }
            t += dt;
        }
        let rel = debouncer.on_drag_release(50.0);
        // Only 1 leading-edge seek at t=0, then release
        assert_eq!(intermediate.len(), 1, "50ms flick at {} Hz should have 1 intermediate seek", hz);
        assert_eq!(rel, 50.0);
    }

    // 2000ms sustained scrub at 120Hz
    let mut debouncer = ScrubberDebouncer::new(100.0);
    let dt = 1000.0 / 120.0;
    let mut t = 0.0;
    let mut intermediate = Vec::new();
    while t < 2000.0 {
        if let Some(st) = debouncer.on_drag_move(t) {
            intermediate.push(st);
        }
        t += dt;
    }
    let rel = debouncer.on_drag_release(2000.0);
    // Rate limited to ~10 Hz: exactly 19 intermediate seeks due to 108.33ms quantization (19 * 108.33 = 1958ms)
    assert_eq!(intermediate.len(), 19, "2000ms scrub at 120 Hz emits 19 intermediate seeks due to 108.33ms quantization");
    assert_eq!(rel, 2000.0);
}

// ============================================================================
// 3. MEMORY BOUNDING: CHAT OVERLAY & ALPHA8 FONT ATLAS
// ============================================================================

#[test]
fn test_chat_overlay_flood_10000_messages_and_fade_eviction() {
    let mut overlay = BoundedChatOverlay::new(5);

    // Flood overlay with 10,000 messages within 1.0 second
    for i in 0..10_000 {
        let msg = format!("Watch party burst spam #{} with reaction emojis 🎉🚀", i);
        let t_secs = i as f64 * 0.0001; // 0.0000s to 0.9999s
        overlay.on_message(msg, t_secs);
    }

    // Invariant: active visible bubbles MUST be strictly clamped to 5
    assert_eq!(
        overlay.visible_bubbles().len(),
        5,
        "Active bubbles count must be strictly clamped to 5"
    );
    assert_eq!(
        overlay.dropped_count(),
        9_995,
        "Dropped bubble count must be exactly 9,995 after 10,000 incoming messages"
    );

    // Memory footprint of bounded list must not exceed 1.5 KB
    assert!(
        overlay.estimated_memory_bytes() <= 1280,
        "Estimated memory ({} B) must be <= 1280 B",
        overlay.estimated_memory_bytes()
    );

    // Verify bubble age lifecycle eviction:
    // Advancing wall time to 6.5s (> 5.0s lifecycle = 4.0s dwell + 1.0s fade)
    // Sending a single message at t = 6.5s should evict all previous 5 bubbles
    overlay.on_message("New message after 5.5s silence".to_string(), 6.5);
    assert_eq!(
        overlay.visible_bubbles().len(),
        1,
        "Old bubbles must be evicted when t_delta >= 5.0s"
    );
    assert_eq!(
        overlay.visible_bubbles()[0].message,
        "New message after 5.5s silence"
    );
}

#[test]
fn test_alpha8_font_atlas_and_ui_memory_budget() {
    // 2048x2048 Alpha8 atlas = 4.0 MB
    let atlas_2048 = FontAtlasAlpha8::new(2048, 2048);
    assert_eq!(atlas_2048.width(), 2048);
    assert_eq!(atlas_2048.height(), 2048);
    assert_eq!(atlas_2048.as_slice().len(), 2048 * 2048);
    assert!(
        (atlas_2048.memory_mb() - 4.0).abs() < 1e-4,
        "2048x2048 Alpha8 atlas must be exactly 4.0 MB (got {:.3} MB)",
        atlas_2048.memory_mb()
    );

    // 1024x1024 Alpha8 atlas = 1.0 MB
    let atlas_1024 = FontAtlasAlpha8::new(1024, 1024);
    assert!(
        (atlas_1024.memory_mb() - 1.0).abs() < 1e-4,
        "1024x1024 Alpha8 atlas must be exactly 1.0 MB"
    );

    // Strict UI RAM budget audit:
    // Font Atlas (4.0 MB) + RGBA8 Framebuffer (8.5 MB) + State/Snapshots (1.0 MB) = 13.5 MB <= 15.0 MB
    let atlas_mb = atlas_2048.memory_mb();
    let hud_fb_mb = 8.5;
    let state_mb = 1.0;
    let total_ui_mb = atlas_mb + hud_fb_mb + state_mb;

    assert!(
        atlas_mb <= 4.0,
        "Font atlas ({:.2} MB) must not exceed 4.0 MB",
        atlas_mb
    );
    assert!(
        total_ui_mb <= 15.0,
        "Total UI RAM consumption ({:.2} MB) must strictly satisfy <= 15.0 MB budget",
        total_ui_mb
    );
}

// ============================================================================
// 4. OSD TRANSPORT GLYPH DECAY & SYNC BADGE HEALTH DIAGNOSTICS
// ============================================================================

#[test]
fn test_transport_glyph_exponential_decay_boundary() {
    let glyph = TransportGlyph::new(TransportGlyphKind::SeekForward, 100.0);

    // t = 100.0s: alpha = 0.8 * e^(0) = 0.8
    assert!((glyph.opacity(100.0) - 0.8).abs() < 1e-4);

    // t = 100.1s: alpha = 0.8 * e^(-0.5) ≈ 0.4852
    assert!((glyph.opacity(100.1) - 0.48522277).abs() < 1e-3);

    // t = 100.6s: exact 600ms boundary: alpha = 0.8 * e^(-3.0) ≈ 0.0398
    assert!((glyph.opacity(100.6) - 0.03982963).abs() < 1e-3);

    // t = 100.60001s: strictly beyond 600ms lifecycle -> returns 0.0
    assert_eq!(glyph.opacity(100.6001), 0.0);
    assert_eq!(glyph.opacity(101.0), 0.0);

    // Negative dt (before start time) -> clamps to dt = 0.0 -> returns 0.8
    assert_eq!(glyph.opacity(99.0), 0.8);
}

#[test]
fn test_sync_health_badge_tri_color_adversarial_boundaries() {
    // Deadband Zone 1: |e| < 15ms
    assert_eq!(evaluate_sync_health(0.0, 0.0), SyncBadgeColor::Green);
    assert_eq!(evaluate_sync_health(14.999, 0.05), SyncBadgeColor::Green);
    assert_eq!(evaluate_sync_health(-14.999, 0.099), SyncBadgeColor::Green);

    // Micro-speed Zone 2: 15ms <= |e| <= 1500ms
    assert_eq!(evaluate_sync_health(15.0, 0.0), SyncBadgeColor::Amber);
    assert_eq!(evaluate_sync_health(-15.0, 0.05), SyncBadgeColor::Amber);
    assert_eq!(evaluate_sync_health(750.0, 0.10), SyncBadgeColor::Amber);
    assert_eq!(evaluate_sync_health(-1500.0, 0.10), SyncBadgeColor::Amber);

    // Hard Seek Zone 3: |e| > 1500ms
    assert_eq!(evaluate_sync_health(1500.001, 0.0), SyncBadgeColor::Red);
    assert_eq!(evaluate_sync_health(-1500.001, 0.0), SyncBadgeColor::Red);
    assert_eq!(evaluate_sync_health(5000.0, 0.0), SyncBadgeColor::Red);

    // Packet loss > 10% triggers Red regardless of phase alignment
    assert_eq!(evaluate_sync_health(0.0, 0.1001), SyncBadgeColor::Red);
    assert_eq!(evaluate_sync_health(10.0, 0.25), SyncBadgeColor::Red);
}

#[test]
fn test_format_timecode_hud_adversarial_cases() {
    // Zero timecode
    assert_eq!(format_timecode_hud(0, 0), "00:00:00 / 00:00:00");

    // Millisecond truncation check
    assert_eq!(format_timecode_hud(999, 999), "00:00:00 / 00:00:00");

    // Standard 1h 23m 45s
    let current_ms = (3600 + 23 * 60 + 45) * 1000;
    let duration_ms = (2 * 3600 + 30 * 60) * 1000;
    assert_eq!(
        format_timecode_hud(current_ms, duration_ms),
        "01:23:45 / 02:30:00"
    );

    // Multi-hour duration (e.g. 24h playback)
    let day_ms = 24 * 3600 * 1000;
    assert_eq!(format_timecode_hud(day_ms, day_ms), "24:00:00 / 24:00:00");
}
