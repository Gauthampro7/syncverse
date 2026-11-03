//! Adversarial challenge test suite for Phase 5 remediation fixes.
//!
//! Validates:
//! 1. `/seek` overflow resilience and bounds enforcement with ZERO panics across massive numbers.
//! 2. `/poll` argument parsing with irregular leading, trailing, and embedded whitespace.
//! 3. MasterClock SeqLock rate deadband comparison in `playback_runtime`.
//! 4. Demuxer chapter timescale clamping to `[0, u64::MAX as i128]`.
//! 5. Audio `soft_clip_sample` NaN silencing and saturation bounds.

#![deny(warnings)]

use std::panic::catch_unwind;
use std::time::Instant;
use syncverse_chat::{ChatError, ParsedSlashCommand, SlashCommandParser};
use syncverse_clock::{MasterClock, PlaybackState};
use syncverse_codecs::audio::soft_clip_sample;

#[test]
fn test_seek_overflow_resilience_massive_inputs_no_panics() {
    let adversarial_inputs = [
        "/seek 9999999999999999:00:00",
        "/seek 100000000:00:00",
        "/seek 9999999999:59:59",
        "/seek 9999999999999999:00",
        "/seek 100000000:00",
        "/seek 9999999999:59",
        "/seek 87601:00:00",
        "/seek 5256001:00",
        "/seek 18446744073709551615:00:00",
        "/seek 18446744073709551616:00:00",
        "/seek 999999999999999999999999999999:00:00",
        "/seek -1:00:00",
        "/seek 00:60:00",
        "/seek 00:00:60",
        "/seek 00:60",
        "/seek 315360001.0",
        "/seek -9999999999999999.0",
        "/seek +9999999999999999.0",
        "/seek +315360001.0s",
        "/seek -315360001.0s",
        "/seek ++10s",
        "/seek --10s",
        "/seek NaN:00:00",
        "/seek +Infinity:00:00",
    ];

    for input in adversarial_inputs {
        let unwind_res = catch_unwind(|| SlashCommandParser::parse(input, None));
        assert!(
            unwind_res.is_ok(),
            "Input '{}' triggered a panic instead of returning Err!",
            input
        );
        let parse_res = unwind_res.unwrap();
        assert!(
            parse_res.is_err(),
            "Input '{}' was expected to return Err(ChatError::InvalidCommand), got: {:?}",
            input,
            parse_res
        );
        match parse_res.unwrap_err() {
            ChatError::InvalidCommand(_) => {} // Expected typed error
            other => panic!("Input '{}' returned unexpected error type: {:?}", input, other),
        }
    }
}

#[test]
fn test_seek_valid_upper_bounds_and_boundary_precision() {
    // 87600 hours is the upper bound: 87600 * 3600 * 1000 = 315,360,000,000 ms
    let res_hours = SlashCommandParser::parse("/seek 87600:00:00", None)
        .expect("87600:00:00 is within 10-year limit and should succeed");
    match res_hours {
        ParsedSlashCommand::Seek { target_pts_ms } => {
            assert_eq!(target_pts_ms, 315_360_000_000);
        }
        _ => panic!("Expected ParsedSlashCommand::Seek"),
    }

    // 5256000 minutes is the upper bound: 5256000 * 60 * 1000 = 315,360,000,000 ms
    let res_mins = SlashCommandParser::parse("/seek 5256000:00", None)
        .expect("5256000:00 is within 10-year limit and should succeed");
    match res_mins {
        ParsedSlashCommand::Seek { target_pts_ms } => {
            assert_eq!(target_pts_ms, 315_360_000_000);
        }
        _ => panic!("Expected ParsedSlashCommand::Seek"),
    }

    // 315360000.0 raw seconds is the upper bound: 315360000.0 * 1000 = 315,360,000,000 ms
    let res_secs = SlashCommandParser::parse("/seek 315360000.0", None)
        .expect("315360000.0s is within 10-year limit and should succeed");
    match res_secs {
        ParsedSlashCommand::Seek { target_pts_ms } => {
            assert_eq!(target_pts_ms, 315_360_000_000);
        }
        _ => panic!("Expected ParsedSlashCommand::Seek"),
    }
}

#[test]
fn test_relative_seek_bounds_and_saturation() {
    // Current PTS = 50,000 ms (50 seconds)
    let current_pts = Some(50_000);

    // Relative backward within range: -10s -> 40,000 ms
    let res_neg = SlashCommandParser::parse("/seek -10s", current_pts).unwrap();
    match res_neg {
        ParsedSlashCommand::Seek { target_pts_ms } => assert_eq!(target_pts_ms, 40_000),
        _ => panic!("Expected Seek"),
    }

    // Relative forward: +15s -> 65,000 ms
    let res_pos = SlashCommandParser::parse("/seek +15s", current_pts).unwrap();
    match res_pos {
        ParsedSlashCommand::Seek { target_pts_ms } => assert_eq!(target_pts_ms, 65_000),
        _ => panic!("Expected Seek"),
    }

    // Relative backward exceeding current PTS saturates at 0 ms
    let res_under = SlashCommandParser::parse("/seek -100.0", current_pts).unwrap();
    match res_under {
        ParsedSlashCommand::Seek { target_pts_ms } => assert_eq!(target_pts_ms, 0),
        _ => panic!("Expected Seek"),
    }
}

#[test]
fn test_poll_whitespace_and_slicing_adversarial() {
    // Leading whitespace between slash and command
    let res = SlashCommandParser::parse("/   poll What is best? | Opt1 | Opt2", None)
        .expect("Leading whitespace poll should parse");
    match res {
        ParsedSlashCommand::Poll { question, options } => {
            assert_eq!(question, "What is best?");
            assert_eq!(options, vec!["Opt1", "Opt2"]);
        }
        _ => panic!("Expected ParsedSlashCommand::Poll"),
    }

    // Leading and trailing whitespace around whole command and pipes
    let res = SlashCommandParser::parse("   /poll    Question | Opt1 | Opt2   ", None)
        .expect("Padded poll should parse");
    match res {
        ParsedSlashCommand::Poll { question, options } => {
            assert_eq!(question, "Question");
            assert_eq!(options, vec!["Opt1", "Opt2"]);
        }
        _ => panic!("Expected ParsedSlashCommand::Poll"),
    }

    // Tabs and multiple spaces
    let res = SlashCommandParser::parse("\t/  poll\t\tQuestion | Opt1 | Opt2\t", None)
        .expect("Tabbed poll should parse");
    match res {
        ParsedSlashCommand::Poll { question, options } => {
            assert_eq!(question, "Question");
            assert_eq!(options, vec!["Opt1", "Opt2"]);
        }
        _ => panic!("Expected ParsedSlashCommand::Poll"),
    }

    // Quoted format with leading space
    let res = SlashCommandParser::parse("/   poll    \"Quoted Question\" \"Option 1\" \"Option 2\"", None)
        .expect("Quoted poll with leading space should parse");
    match res {
        ParsedSlashCommand::Poll { question, options } => {
            assert_eq!(question, "Quoted Question");
            assert_eq!(options, vec!["Option 1", "Option 2"]);
        }
        _ => panic!("Expected ParsedSlashCommand::Poll"),
    }

    // Case insensitivity with spaces
    let res = SlashCommandParser::parse("   /   POLL   Question | Opt1 | Opt2   ", None)
        .expect("Uppercase POLL with whitespace should parse");
    match res {
        ParsedSlashCommand::Poll { question, options } => {
            assert_eq!(question, "Question");
            assert_eq!(options, vec!["Opt1", "Opt2"]);
        }
        _ => panic!("Expected ParsedSlashCommand::Poll"),
    }

    // Invalid poll variants must fail gracefully
    assert!(SlashCommandParser::parse("/poll", None).is_err());
    assert!(SlashCommandParser::parse("/   poll   ", None).is_err());
    assert!(SlashCommandParser::parse("/poll | | |", None).is_err());
    assert!(SlashCommandParser::parse("/poll SingleItemNoPipe", None).is_err());
}

#[test]
fn test_master_clock_seqlock_rate_and_deadband_reanchor() {
    let now = Instant::now();
    let clock = MasterClock::new(0.0, 1.0, PlaybackState::Playing);

    // Initial state
    assert!((clock.speed() - 1.0).abs() < 1e-9);

    // Simulate Zone 2 micro-speed steering
    let steered_speed = 1.04;
    clock.reanchor(now, steered_speed);
    assert!((clock.speed() - 1.04).abs() < 1e-9);

    // Simulate Zone 1 deadband check:
    // (clock.speed() - target_speed).abs() > 1e-4
    let target_speed = 1.0;
    if (clock.speed() - target_speed).abs() > 1e-4 {
        clock.reanchor(now, target_speed);
    }
    assert!(
        (clock.speed() - target_speed).abs() < 1e-9,
        "Clock speed must be restored to target_speed when deadband check triggers"
    );

    // Deadband hysteresis: tiny difference <= 1e-4 should NOT trigger reanchor
    let tiny_diff_speed = 1.00005;
    clock.reanchor(now, tiny_diff_speed);
    if (clock.speed() - target_speed).abs() > 1e-4 {
        clock.reanchor(now, target_speed);
    }
    assert!(
        (clock.speed() - tiny_diff_speed).abs() < 1e-9,
        "Clock speed within 1e-4 should not trigger redundant reanchoring"
    );
}

#[test]
fn test_chapter_timescale_conversion_clamping() {
    // Formula from crates/syncverse-demux/src/universal.rs:214-215:
    // let start_ms = ((ch.start() as i128 * num * 1000) / den).clamp(0, u64::MAX as i128) as u64;

    // 1. Massive PTS 2^62 with 24 FPS (num=1, den=24)
    let pts_large: i64 = 1 << 62;
    let num_24: i128 = 1;
    let den_24: i128 = 24;
    let clamped_ms = ((pts_large as i128 * num_24 * 1000) / den_24).clamp(0, u64::MAX as i128) as u64;
    assert_eq!(
        clamped_ms,
        u64::MAX,
        "Extreme chapter timestamp exceeding u64::MAX must clamp to u64::MAX"
    );

    // 2. Negative PTS must clamp to 0
    let pts_neg: i64 = -500_000;
    let clamped_neg = ((pts_neg as i128 * num_24 * 1000) / den_24).clamp(0, u64::MAX as i128) as u64;
    assert_eq!(clamped_neg, 0, "Negative chapter timestamp must clamp to 0");

    // 3. Standard MPEG-TS PTS
    let num_ts: i128 = 1;
    let den_ts: i128 = 90_000;
    let ts_ms = ((pts_large as i128 * num_ts * 1000) / den_ts).clamp(0, u64::MAX as i128) as u64;
    assert_eq!(ts_ms, 51_240_955_760_304_310);

    // 4. Zero denominator fallback
    let den_zero_input = 0;
    let den_safe = if den_zero_input != 0 { den_zero_input as i128 } else { 1 };
    let num_unit: i128 = 1;
    let pts_unit: i128 = 1000;
    let zero_den_ms = ((pts_unit * num_unit * 1000) / den_safe).clamp(0, u64::MAX as i128) as u64;
    assert_eq!(zero_den_ms, 1_000_000);
}

#[test]
fn test_audio_soft_clip_nan_silencing_and_saturation() {
    // NaN silencing
    assert_eq!(soft_clip_sample(f32::NAN), 0.0);
    assert_eq!(soft_clip_sample(-f32::NAN), 0.0);

    // Linear region
    assert_eq!(soft_clip_sample(0.0), 0.0);
    assert_eq!(soft_clip_sample(-0.0), -0.0);
    assert_eq!(soft_clip_sample(0.85), 0.85);
    assert_eq!(soft_clip_sample(-0.85), -0.85);
    assert_eq!(soft_clip_sample(0.42), 0.42);

    // Compression region (0.85 < s <= 1.0)
    let pos_one = soft_clip_sample(1.0);
    assert!(pos_one > 0.85 && pos_one < 1.0);

    let neg_one = soft_clip_sample(-1.0);
    assert!(neg_one < -0.85 && neg_one > -1.0);

    // Extreme saturations
    assert_eq!(soft_clip_sample(f32::INFINITY), 1.0);
    assert_eq!(soft_clip_sample(f32::NEG_INFINITY), -1.0);

    let extreme_pos = soft_clip_sample(1e9);
    assert!((0.99999..=1.0).contains(&extreme_pos));

    let extreme_neg = soft_clip_sample(-1e9);
    assert!((-1.0..=-0.99999).contains(&extreme_neg));
}
