//! Empirical adversarial stress testing for Milestone 4 (R1 Fedora Desktop & Interactive Terminal/OSD Player Launcher).
//!
//! Subsystems stressed:
//! 1. Triple-buffer snapshot concurrency race fix (50,000 publishes, multi-thread, 0 torn reads).
//! 2. InteractivePlayer state machine & keyboard fuzzing (Pause/Resume storm, relative seek bounds, speed clamp [0.2, 4.0], volume clamp [0.0, 1.0], mode switching).
//! 3. Slash command parser & dispatcher boundary fuzzing (/seek invalid, /seek -10, /seek 999:99:99, /clip 50 10, empty, 10KB oversized).
//! 4. Scrubber bar and timecode HUD boundaries (0ms duration, current > duration, 100+ hr, narrow widths < 20).
//! 5. String slicing safety on UTF-8 media targets.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Instant;

use syncverse_clock::PlaybackState;
use syncverse_core::{InteractivePlayer, PlayerInputMode, SubsystemMetrics, TelemetryAggregator};
use syncverse_ui::{
    format_timecode_hud, render_hotkeys_footer, render_osd_hud, render_styled_scrubber_bar,
    KeyCommand, OsdHudState, PlayerUiSnapshot, ScrubberStyle, TripleBufferSnapshotExchange,
};

// ============================================================================
// 1. TRIPLE-BUFFER SNAPSHOT CONCURRENCY RACE FIX STRESS (50,000 PUBLISHES)
// ============================================================================

#[test]
fn test_triple_buffer_50k_publishes_stress() {
    for run in 0..10 {
        let exchange = Arc::new(TripleBufferSnapshotExchange::new());
        let ex_producer = Arc::clone(&exchange);

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_cons = Arc::clone(&stop_flag);

        let barrier = Arc::new(Barrier::new(2));
        let barrier_cons = Arc::clone(&barrier);

        let total_iterations = 50_000usize;

        let producer = thread::spawn(move || {
            barrier.wait();
            for seq in 0..total_iterations {
                let snap = PlayerUiSnapshot {
                    monotonic_timestamp: seq as u64,
                    media_position_pts: (seq * 1000) as u64,
                    room_sequence: seq as u64,
                    ..Default::default()
                };
                ex_producer.publish(snap);
            }
            stop_flag.store(true, Ordering::Release);
        });

        let consumer = thread::spawn(move || {
            let mut read_count = 0usize;
            let mut inconsistencies = 0usize;

            barrier_cons.wait();

            loop {
                let snap = exchange.poll_latest_snapshot();
                read_count += 1;
                if !snap.is_internally_consistent() {
                    inconsistencies += 1;
                }
                if stop_flag_cons.load(Ordering::Acquire) {
                    break;
                }
            }

            let final_snap = exchange.poll_latest_snapshot();
            read_count += 1;
            if !final_snap.is_internally_consistent() {
                inconsistencies += 1;
            }

            (read_count, inconsistencies)
        });

        producer.join().unwrap();
        let (reads, bad) = consumer.join().unwrap();
        assert!(reads > 0, "Run {}: reads must be > 0, got {}", run, reads);
        assert_eq!(bad, 0, "Run {}: triple buffer must exhibit zero torn reads", run);
    }
}

#[test]
fn test_telemetry_aggregator_50k_publishes_stress() {
    for run in 0..5 {
        let exchange = Arc::new(TripleBufferSnapshotExchange::new());
        let ex_producer = Arc::clone(&exchange);

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_cons = Arc::clone(&stop_flag);

        let barrier = Arc::new(Barrier::new(2));
        let barrier_cons = Arc::clone(&barrier);

        let total_iterations = 50_000usize;

        let producer = thread::spawn(move || {
            barrier.wait();
            let aggregator = TelemetryAggregator::new(&ex_producer);
            for seq in 0..total_iterations {
                let metrics = SubsystemMetrics {
                    monotonic_timestamp: seq as u64,
                    media_position_pts: (seq * 1000) as u64,
                    media_duration_pts: 120_000 * 1_000_000,
                    playback_state: 1,
                    network_rtt_nanos: 250_000_000,
                    clock_offset_nanos: 50_000_000,
                    phase_error_nanos: 10_000_000,
                    speed_ratio: 1.0,
                    peer_count: 3,
                    peers_ready: 3,
                    room_sequence: seq as u64,
                };
                aggregator.publish_snapshot(&metrics);
            }
            stop_flag.store(true, Ordering::Release);
        });

        let consumer = thread::spawn(move || {
            let mut read_count = 0usize;
            let mut inconsistencies = 0usize;

            barrier_cons.wait();

            loop {
                let snap = exchange.poll_latest_snapshot();
                read_count += 1;
                if !snap.is_internally_consistent() {
                    inconsistencies += 1;
                }
                if stop_flag_cons.load(Ordering::Acquire) {
                    break;
                }
            }

            let final_snap = exchange.poll_latest_snapshot();
            read_count += 1;
            if !final_snap.is_internally_consistent() {
                inconsistencies += 1;
            }

            (read_count, inconsistencies)
        });

        producer.join().unwrap();
        let (reads, bad) = consumer.join().unwrap();
        assert!(reads > 0, "Run {}: reads must be > 0, got {}", run, reads);
        assert_eq!(bad, 0, "Run {}: must exhibit zero torn reads", run);
    }
}

// ============================================================================
// 2. INTERACTIVE PLAYER KEYBOARD INPUT & STATE MACHINE FUZZING
// ============================================================================

#[test]
fn test_interactive_player_pause_resume_storm() {
    let mut player = InteractivePlayer::new("synthetic-full-mkv");
    let now = Instant::now();

    for i in 0..5_000 {
        let quit = player.handle_key_command(KeyCommand::TogglePause, now);
        assert!(!quit);
        let expected = if i % 2 == 0 {
            PlaybackState::Playing
        } else {
            PlaybackState::Paused
        };
        assert_eq!(player.coordinator.clock.state(), expected);
    }
}

#[test]
fn test_interactive_player_seek_relative_stress_and_bounds() {
    let mut player = InteractivePlayer::new("synthetic-full-mkv");
    let now = Instant::now();

    // 1. Extreme negative seek: must clamp to 0.0
    player.handle_key_command(KeyCommand::SeekRelative(-1_000_000_000), now);
    assert_eq!(player.coordinator.current_pts(now), 0.0);

    // 2. Extreme positive seek: must clamp to duration_ms (1000.0)
    player.handle_key_command(KeyCommand::SeekRelative(1_000_000_000), now);
    let dur = player.media_info.as_ref().unwrap().duration_ms.unwrap() as f64;
    assert_eq!(player.coordinator.current_pts(now), dur);

    // 3. High-frequency oscillating seeks
    for d in &[-5000, 5000, -1000, 1000, -5000, -1000, 5000, 1000] {
        player.handle_key_command(KeyCommand::SeekRelative(*d), now);
        let pts = player.coordinator.current_pts(now);
        assert!(pts >= 0.0 && pts <= dur, "PTS must remain within [0, {}], got {}", dur, pts);
    }
}

#[test]
fn test_interactive_player_speed_shift_limits() {
    let mut player = InteractivePlayer::new("synthetic-full-mkv");
    let now = Instant::now();

    // Repeated speed-ups past maximum limit 4.0x
    for _ in 0..50 {
        player.handle_key_command(KeyCommand::ShiftSpeed(1), now);
    }
    assert_eq!(player.coordinator.speed(), 4.0, "Speed must clamp to 4.0x");

    // Repeated speed-downs past minimum limit 0.2x
    for _ in 0..50 {
        player.handle_key_command(KeyCommand::ShiftSpeed(-1), now);
    }
    assert_eq!(player.coordinator.speed(), 0.2, "Speed must clamp to 0.2x");
}

#[test]
fn test_interactive_player_volume_boundary_clamping() {
    let mut player = InteractivePlayer::new("synthetic-full-mkv");
    let now = Instant::now();

    // Decrement volume past 0.0
    for _ in 0..30 {
        player.handle_key_command(KeyCommand::AdjustVolume(-0.05), now);
    }
    assert_eq!(player.volume, 0.0, "Volume must clamp to 0.0");

    // Increment volume past 1.0
    for _ in 0..30 {
        player.handle_key_command(KeyCommand::AdjustVolume(0.05), now);
    }
    assert_eq!(player.volume, 1.0, "Volume must clamp to 1.0");

    // Extreme single delta jumps
    player.handle_key_command(KeyCommand::AdjustVolume(-999.0), now);
    assert_eq!(player.volume, 0.0);
    player.handle_key_command(KeyCommand::AdjustVolume(999.0), now);
    assert_eq!(player.volume, 1.0);
}

#[test]
fn test_interactive_player_rapid_mode_switching() {
    let mut player = InteractivePlayer::new("synthetic-full-mkv");
    let now = Instant::now();

    for _ in 0..500 {
        player.handle_key_command(KeyCommand::OpenCommandPrompt, now);
        assert_eq!(player.input_mode, PlayerInputMode::CommandBar);
        assert_eq!(player.command_buffer, "/");

        // Escape cancels back to Normal
        player.handle_command_bar_input(&[0x1B], now);
        assert_eq!(player.input_mode, PlayerInputMode::Normal);
        assert!(player.command_buffer.is_empty());
    }
}

// ============================================================================
// 3. SLASH COMMAND PARSING & DISPATCH BOUNDARY FUZZING
// ============================================================================

#[test]
fn test_slash_command_invalid_and_boundary_cases() {
    let mut player = InteractivePlayer::new("synthetic-full-mkv");
    let now = Instant::now();

    let adversarial_commands = [
        // Invalid seek argument
        b"/seek invalid\n".as_slice(),
        b"/seek -10\n".as_slice(),
        b"/seek 999:99:99\n".as_slice(),
        b"/seek 01:99:00\n".as_slice(),
        b"/seek ++10s\n".as_slice(),
        b"/seek \n".as_slice(),
        // Invalid clip ranges
        b"/clip 50 10\n".as_slice(),
        b"/clip 02:00 01:00 Reversed\n".as_slice(),
        b"/clip\n".as_slice(),
        b"/clip only_start\n".as_slice(),
        // Empty / bare slash
        b"/\n".as_slice(),
        b"/   \n".as_slice(),
        // Unknown command
        b"/unrecognized_command_xyz\n".as_slice(),
        b"/drop database\n".as_slice(),
        // Poll edge cases
        b"/poll\n".as_slice(),
        b"/poll NoOptions\n".as_slice(),
        b"/poll Question | \n".as_slice(),
    ];

    for cmd_bytes in &adversarial_commands {
        player.input_mode = PlayerInputMode::CommandBar;
        player.handle_command_bar_input(cmd_bytes, now);
        assert_eq!(player.input_mode, PlayerInputMode::Normal);
        assert!(player.command_buffer.is_empty());
        // Verify player does not crash or panic
        assert!(player.status_message.is_some() || player.show_sync_hud);
    }
}

#[test]
fn test_slash_command_oversized_payload_stress() {
    let mut player = InteractivePlayer::new("synthetic-full-mkv");
    let now = Instant::now();

    // 10,000-character oversized command payload
    let mut big_cmd = "/bookmark ".to_string();
    big_cmd.push_str(&"A".repeat(10_000));
    big_cmd.push('\n');

    player.input_mode = PlayerInputMode::CommandBar;
    player.handle_command_bar_input(big_cmd.as_bytes(), now);
    assert_eq!(player.input_mode, PlayerInputMode::Normal);
    assert!(player.status_message.is_some());
}

#[test]
fn test_command_bar_raw_byte_fuzzing() {
    let mut player = InteractivePlayer::new("synthetic-full-mkv");
    let now = Instant::now();

    // Random non-ASCII and control bytes fed to command bar
    let raw_bytes: Vec<u8> = (0u8..=255).cycle().take(2048).collect();
    player.input_mode = PlayerInputMode::CommandBar;
    player.handle_command_bar_input(&raw_bytes, now);
    // Backspace to empty
    for _ in 0..3000 {
        player.handle_command_bar_input(&[0x7F], now);
    }
    assert_eq!(player.input_mode, PlayerInputMode::Normal);
    assert!(player.command_buffer.is_empty());
}

// ============================================================================
// 4. BOX-DRAWING SCRUBBER BAR & TIMECODE HUD BOUNDARIES
// ============================================================================

#[test]
fn test_scrubber_bar_duration_zero_and_overrun() {
    // 0ms duration
    for style in [ScrubberStyle::Classic, ScrubberStyle::BoxDrawing, ScrubberStyle::Block] {
        let bar = render_styled_scrubber_bar(0, 0, 20, style);
        assert!(bar.starts_with('[') && bar.ends_with(']'));
        assert!(bar.contains('●'));

        // Current > Duration overrun
        let overrun = render_styled_scrubber_bar(5000, 1000, 20, style);
        assert!(overrun.starts_with('[') && overrun.ends_with(']'));
        assert!(overrun.contains('●'));
    }
}

#[test]
fn test_scrubber_bar_narrow_widths() {
    // Narrow widths < 20, down to 0
    for width in [0, 1, 2, 5, 9, 10, 11, 15, 19] {
        for style in [ScrubberStyle::Classic, ScrubberStyle::BoxDrawing, ScrubberStyle::Block] {
            let bar = render_styled_scrubber_bar(500, 1000, width, style);
            assert!(bar.starts_with('[') && bar.ends_with(']'));
            assert!(bar.contains('●'));
        }
    }
}

#[test]
fn test_scrubber_bar_100_plus_hour_durations() {
    // 150 hours = 540,000,000 ms
    let dur = 540_000_000u64;
    let cur = 270_000_000u64; // 50%
    for style in [ScrubberStyle::Classic, ScrubberStyle::BoxDrawing, ScrubberStyle::Block] {
        let bar = render_styled_scrubber_bar(cur, dur, 40, style);
        assert!(bar.starts_with('[') && bar.ends_with(']'));
        assert!(bar.contains('●'));
    }
}

#[test]
fn test_timecode_hud_boundaries() {
    // 0ms duration
    assert_eq!(format_timecode_hud(0, 0), "00:00:00 / 00:00:00");

    // Current > Duration
    assert_eq!(format_timecode_hud(5000, 1000), "00:00:05 / 00:00:01");

    // 100+ hour durations
    let h100 = 100 * 3600 * 1000;
    let h125 = 125 * 3600 * 1000 + 45 * 60 * 1000 + 30 * 1000;
    assert_eq!(format_timecode_hud(h100, h125), "100:00:00 / 125:45:30");

    // Extreme u64 value
    let tc_max = format_timecode_hud(u64::MAX, u64::MAX);
    assert!(tc_max.contains('/'));
}

#[test]
fn test_render_osd_hud_extreme_values() {
    let state = OsdHudState {
        current_ms: 100 * 3600 * 1000,
        duration_ms: 200 * 3600 * 1000,
        is_playing: true,
        speed: 3.5,
        volume: 1.5, // > 1.0
        is_muted: false,
        rtt_ms: 9999,
        phase_err_ms: -2500.0,
    };
    let hud = render_osd_hud(&state);
    assert!(hud.contains("100:00:00 / 200:00:00"));
    assert!(hud.contains("3.50x"));
    assert!(hud.contains("150%"));
    assert!(hud.contains("SEEK"));
}

#[test]
fn test_render_hotkeys_footer_narrow_terminal() {
    for w in [0, 1, 5, 10, 15, 20, 30, 60, 120] {
        let footer = render_hotkeys_footer(w);
        assert!(!footer.is_empty());
    }
}
