//! Comprehensive unit and integration test suite for hierarchical quick seek
//! and frame stepping navigation calculations in Syncverse UI.
//!
//! Validates:
//! 1. Frame stepping: 1 frame step = ~42ms (~41.7ms @ 24fps), bounded strictly to `[0, duration_ms]`.
//! 2. Hierarchical quick seek increments:
//!    - Standard quick seek: ±5,000ms (5s) via Left/Right arrows.
//!    - Medium / fine quick seek: ±30,000ms (30s) via Shift + Left/Right arrows.
//!    - Coarse quick seek: ±60,000ms (60s) via Ctrl + Left/Right arrows.
//! 3. Reversibility and symmetry in safe internal intervals.
//! 4. Clamping invariance: targets never underflow below 0 or exceed total media duration.
//! 5. Timecode HUD string formatting for navigation targets.

#![deny(warnings)]

use syncverse_ui::gui::ControlsBarState;
use syncverse_ui::snapshot::PlayerUiSnapshot;

/// Standard frame step duration in milliseconds (~41.67ms @ 24fps, rounded to 42ms).
pub const FRAME_STEP_MS: u64 = 42;

/// Standard quick seek increment in milliseconds (±5 seconds).
pub const SEEK_STEP_STANDARD_MS: i64 = 5_000;

/// Medium / fine quick seek increment in milliseconds (±30 seconds).
pub const SEEK_STEP_FINE_MS: i64 = 30_000;

/// Coarse quick seek increment in milliseconds (±60 seconds).
pub const SEEK_STEP_COARSE_MS: i64 = 60_000;

/// Calculates the target presentation timestamp for a single frame step forward.
///
/// Steps forward by `FRAME_STEP_MS` (42ms) and clamps to `duration_ms`.
#[inline]
pub fn step_frame_forward(current_ms: u64, duration_ms: u64) -> u64 {
    (current_ms + FRAME_STEP_MS).min(duration_ms)
}

/// Calculates the target presentation timestamp for a single frame step backward.
///
/// Steps backward by `FRAME_STEP_MS` (42ms) and saturates at 0.
#[inline]
pub fn step_frame_backward(current_ms: u64) -> u64 {
    current_ms.saturating_sub(FRAME_STEP_MS)
}

/// Calculates the target presentation timestamp for a relative quick seek.
///
/// Offsets `current_ms` by `offset_ms`, bounding the result strictly within `[0, duration_ms]`.
#[inline]
pub fn calculate_quick_seek(current_ms: i64, offset_ms: i64, duration_ms: u64) -> u64 {
    let target = (current_ms + offset_ms).max(0) as u64;
    target.min(duration_ms)
}

#[test]
fn test_frame_step_forward_duration_and_bounds() {
    let duration_ms = 10_000u64; // 10 second media

    // 1. Normal forward frame step from 0ms
    assert_eq!(step_frame_forward(0, duration_ms), 42);

    // 2. Normal forward frame step from internal position
    assert_eq!(step_frame_forward(1000, duration_ms), 1042);
    assert_eq!(step_frame_forward(5000, duration_ms), 5042);

    // 3. Step forward when within 42ms of media end (clamped to duration)
    assert_eq!(step_frame_forward(9980, duration_ms), duration_ms);
    assert_eq!(step_frame_forward(9999, duration_ms), duration_ms);

    // 4. Step forward when exactly at media end (remains at duration)
    assert_eq!(step_frame_forward(duration_ms, duration_ms), duration_ms);

    // 5. Degenerate zero-duration media
    assert_eq!(step_frame_forward(0, 0), 0);
}

#[test]
fn test_frame_step_backward_zero_bounding() {
    // 1. Normal backward frame step from internal position
    assert_eq!(step_frame_backward(1000), 958);
    assert_eq!(step_frame_backward(5000), 4958);

    // 2. Backward step exactly at 42ms reaches 0ms
    assert_eq!(step_frame_backward(42), 0);

    // 3. Backward step within 42ms of beginning saturates at 0ms (no underflow)
    assert_eq!(step_frame_backward(20), 0);
    assert_eq!(step_frame_backward(1), 0);

    // 4. Backward step at 0ms remains at 0ms
    assert_eq!(step_frame_backward(0), 0);
}

#[test]
fn test_frame_step_reversibility_in_safe_interval() {
    let duration_ms = 60_000u64;

    // Test a range of safe positions where neither boundary is reached
    for pos in (1000..50_000).step_by(373) {
        let forward = step_frame_forward(pos, duration_ms);
        let back = step_frame_backward(forward);
        assert_eq!(
            back, pos,
            "Reversibility violated for forward->backward at pos {}",
            pos
        );

        let back = step_frame_backward(pos);
        let forward = step_frame_forward(back, duration_ms);
        assert_eq!(
            forward, pos,
            "Reversibility violated for backward->forward at pos {}",
            pos
        );
    }
}

#[test]
fn test_hierarchical_seek_increments() {
    let duration_ms = 300_000u64; // 5 minutes (300,000 ms)
    let current_ms = 100_000i64; // 100 seconds

    // 1. Standard quick seek (±5s)
    let fwd_5s = calculate_quick_seek(current_ms, SEEK_STEP_STANDARD_MS, duration_ms);
    assert_eq!(fwd_5s, 105_000);
    let back_5s = calculate_quick_seek(current_ms, -SEEK_STEP_STANDARD_MS, duration_ms);
    assert_eq!(back_5s, 95_000);

    // 2. Medium / fine quick seek (±30s)
    let fwd_30s = calculate_quick_seek(current_ms, SEEK_STEP_FINE_MS, duration_ms);
    assert_eq!(fwd_30s, 130_000);
    let back_30s = calculate_quick_seek(current_ms, -SEEK_STEP_FINE_MS, duration_ms);
    assert_eq!(back_30s, 70_000);

    // 3. Coarse quick seek (±60s)
    let fwd_60s = calculate_quick_seek(current_ms, SEEK_STEP_COARSE_MS, duration_ms);
    assert_eq!(fwd_60s, 160_000);
    let back_60s = calculate_quick_seek(current_ms, -SEEK_STEP_COARSE_MS, duration_ms);
    assert_eq!(back_60s, 40_000);

    // 4. Verify step ratio hierarchy
    assert_eq!(SEEK_STEP_FINE_MS / SEEK_STEP_STANDARD_MS, 6);
    assert_eq!(SEEK_STEP_COARSE_MS / SEEK_STEP_STANDARD_MS, 12);
    assert_eq!(SEEK_STEP_COARSE_MS / SEEK_STEP_FINE_MS, 2);
}

#[test]
fn test_quick_seek_boundary_clamping() {
    let duration_ms = 60_000u64; // 1 minute clip

    // Backward seeks exceeding current position must clamp to 0
    assert_eq!(
        calculate_quick_seek(2_000, -SEEK_STEP_STANDARD_MS, duration_ms),
        0
    );
    assert_eq!(
        calculate_quick_seek(15_000, -SEEK_STEP_FINE_MS, duration_ms),
        0
    );
    assert_eq!(
        calculate_quick_seek(45_000, -SEEK_STEP_COARSE_MS, duration_ms),
        0
    );
    assert_eq!(
        calculate_quick_seek(0, -SEEK_STEP_COARSE_MS, duration_ms),
        0
    );

    // Forward seeks exceeding total duration must clamp to duration_ms
    assert_eq!(
        calculate_quick_seek(58_000, SEEK_STEP_STANDARD_MS, duration_ms),
        duration_ms
    );
    assert_eq!(
        calculate_quick_seek(40_000, SEEK_STEP_FINE_MS, duration_ms),
        duration_ms
    );
    assert_eq!(
        calculate_quick_seek(10_000, SEEK_STEP_COARSE_MS, duration_ms),
        duration_ms
    );
    assert_eq!(
        calculate_quick_seek(duration_ms as i64, SEEK_STEP_STANDARD_MS, duration_ms),
        duration_ms
    );
}

#[test]
fn test_quick_seek_with_snapshot_nanosecond_conversion() {
    // Media position: 2 minutes (120,000,000,000 nanoseconds)
    // Media duration: 10 minutes (600,000,000,000 nanoseconds)
    let snapshot = PlayerUiSnapshot {
        media_position_pts: 120_000_000_000,
        media_duration_pts: 600_000_000_000,
        ..Default::default()
    };

    let current_ms = (snapshot.media_position_pts / 1_000_000) as i64;
    let duration_ms = snapshot.media_duration_pts / 1_000_000;

    assert_eq!(current_ms, 120_000);
    assert_eq!(duration_ms, 600_000);

    // Seek forward 5s
    let target_5s = calculate_quick_seek(current_ms, SEEK_STEP_STANDARD_MS, duration_ms);
    assert_eq!(target_5s, 125_000);

    // Seek forward 30s
    let target_30s = calculate_quick_seek(current_ms, SEEK_STEP_FINE_MS, duration_ms);
    assert_eq!(target_30s, 150_000);

    // Seek forward 60s
    let target_60s = calculate_quick_seek(current_ms, SEEK_STEP_COARSE_MS, duration_ms);
    assert_eq!(target_60s, 180_000);

    // Frame step forward
    let target_frame = step_frame_forward(current_ms as u64, duration_ms);
    assert_eq!(target_frame, 120_042);
}

#[test]
fn test_timecode_hud_formatting_for_navigation_targets() {
    // 0 ms -> "00:00:00"
    assert_eq!(ControlsBarState::format_timestamp(0), "00:00:00");

    // 42 ms (1 frame) -> "00:00:00"
    assert_eq!(ControlsBarState::format_timestamp(FRAME_STEP_MS), "00:00:00");

    // 5 seconds -> "00:00:05"
    assert_eq!(
        ControlsBarState::format_timestamp(SEEK_STEP_STANDARD_MS as u64),
        "00:00:05"
    );

    // 30 seconds -> "00:00:30"
    assert_eq!(
        ControlsBarState::format_timestamp(SEEK_STEP_FINE_MS as u64),
        "00:00:30"
    );

    // 60 seconds -> "00:01:00"
    assert_eq!(
        ControlsBarState::format_timestamp(SEEK_STEP_COARSE_MS as u64),
        "00:01:00"
    );

    // 1 hour 23 minutes 45 seconds -> "01:23:45"
    let target_hms = (3600 + 23 * 60 + 45) * 1000;
    assert_eq!(
        ControlsBarState::format_timestamp(target_hms),
        "01:23:45"
    );
}
