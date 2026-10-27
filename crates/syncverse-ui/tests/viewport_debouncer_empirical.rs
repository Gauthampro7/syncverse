//! Empirical Challenge Verification Suite for Milestone 3
//! Tests:
//! 1. `ViewportGeometry::compute` across 10+ boundary resolutions:
//!    - 32:9 super-ultrawide (Letterbox)
//!    - 21:9 ultrawide (Letterbox)
//!    - 16:9 standard in 21:9 ultrawide (Pillarbox)
//!    - 16:9 standard exact match
//!    - 4:3 classic (Pillarbox)
//!    - 1:1 square (Pillarbox)
//!    - 9:16 mobile portrait in 16:9 (Pillarbox)
//!    - 16:9 standard in 9:16 mobile portrait (Letterbox)
//!    - 0x0 and degenerate dimensions (Empty, zero divisions)
//!    - Negative dimensions (Empty, zero divisions)
//!    - Extreme needle aspect ratios (1x100000 and 100000x1)
//!    - Aspect ratio preservation and centered symmetry invariants
//! 2. `SeekDebouncer` 10Hz rate limiting & immediate release seek:
//!    - 120 Hz drag over 500ms (61 events -> exactly 5 intermediate seeks)
//!    - 240 Hz drag over 500ms (121 events -> exactly 5 intermediate seeks)
//!    - 1000 Hz drag over 500ms (500 events -> exactly 5 intermediate seeks)
//!    - Short flick (50ms -> 1 leading seek + 1 release seek)
//!    - Real-time `Instant` rate limiting and unconstrained release seek
//!    - Drag state lifecycle and multi-gesture consecutive scrubs

#![deny(warnings)]

use std::time::{Duration, Instant};
use syncverse_ui::{SeekDebouncer, ViewportDisplayMode, ViewportGeometry};

// ============================================================================
// PART 1: VIEWPORT GEOMETRY BOUNDARY RESOLUTION CHALLENGES
// ============================================================================

#[test]
fn test_viewport_geometry_boundary_1_super_ultrawide_32_9_letterbox() {
    // 32:9 super-ultrawide video (5120x1440) inside standard 16:9 viewport (1920x1080)
    let geom = ViewportGeometry::compute(1920.0, 1080.0, 5120.0, 1440.0);
    assert_eq!(geom.mode, ViewportDisplayMode::Letterbox);
    assert_eq!(geom.render_width, 1920.0);
    assert_eq!(geom.render_height, 540.0);
    assert_eq!(geom.offset_x, 0.0);
    assert_eq!(geom.offset_y, 270.0);

    // Aspect ratio preserved exactly
    let aspect_rendered = geom.render_width / geom.render_height;
    let aspect_expected = 5120.0 / 1440.0;
    assert!((aspect_rendered - aspect_expected).abs() < 1e-6);

    // Centered symmetry: top margin == bottom margin == offset_y
    assert_eq!(geom.render_height + 2.0 * geom.offset_y, 1080.0);
}

#[test]
fn test_viewport_geometry_boundary_2_ultrawide_21_9_letterbox() {
    // 21:9 ultrawide video (2560x1080) inside standard 16:9 viewport (1920x1080)
    let geom = ViewportGeometry::compute(1920.0, 1080.0, 2560.0, 1080.0);
    assert_eq!(geom.mode, ViewportDisplayMode::Letterbox);
    assert_eq!(geom.render_width, 1920.0);
    assert_eq!(geom.render_height, 810.0);
    assert_eq!(geom.offset_x, 0.0);
    assert_eq!(geom.offset_y, 135.0);

    let aspect_rendered = geom.render_width / geom.render_height;
    let aspect_expected = 2560.0 / 1080.0;
    assert!((aspect_rendered - aspect_expected).abs() < 1e-6);
    assert_eq!(geom.render_height + 2.0 * geom.offset_y, 1080.0);
}

#[test]
fn test_viewport_geometry_boundary_3_standard_16_9_in_21_9_pillarbox() {
    // Standard 16:9 video (1920x1080) inside 21:9 ultrawide viewport (2560x1080)
    let geom = ViewportGeometry::compute(2560.0, 1080.0, 1920.0, 1080.0);
    assert_eq!(geom.mode, ViewportDisplayMode::Pillarbox);
    assert_eq!(geom.render_width, 1920.0);
    assert_eq!(geom.render_height, 1080.0);
    assert_eq!(geom.offset_x, 320.0);
    assert_eq!(geom.offset_y, 0.0);

    let aspect_rendered = geom.render_width / geom.render_height;
    let aspect_expected = 16.0 / 9.0;
    assert!((aspect_rendered - aspect_expected).abs() < 1e-6);
    assert_eq!(geom.render_width + 2.0 * geom.offset_x, 2560.0);
}

#[test]
fn test_viewport_geometry_boundary_4_standard_16_9_exact() {
    // Standard 16:9 video (1920x1080) inside 16:9 viewport (1280x720)
    let geom = ViewportGeometry::compute(1280.0, 720.0, 1920.0, 1080.0);
    assert_eq!(geom.mode, ViewportDisplayMode::Exact);
    assert_eq!(geom.render_width, 1280.0);
    assert_eq!(geom.render_height, 720.0);
    assert_eq!(geom.offset_x, 0.0);
    assert_eq!(geom.offset_y, 0.0);
}

#[test]
fn test_viewport_geometry_boundary_5_classic_4_3_pillarbox() {
    // Classic 4:3 video (1440x1080) inside standard 16:9 viewport (1920x1080)
    let geom = ViewportGeometry::compute(1920.0, 1080.0, 1440.0, 1080.0);
    assert_eq!(geom.mode, ViewportDisplayMode::Pillarbox);
    assert_eq!(geom.render_width, 1440.0);
    assert_eq!(geom.render_height, 1080.0);
    assert_eq!(geom.offset_x, 240.0);
    assert_eq!(geom.offset_y, 0.0);

    let aspect_rendered = geom.render_width / geom.render_height;
    let aspect_expected = 4.0 / 3.0;
    assert!((aspect_rendered - aspect_expected).abs() < 1e-6);
    assert_eq!(geom.render_width + 2.0 * geom.offset_x, 1920.0);
}

#[test]
fn test_viewport_geometry_boundary_6_square_1_1_pillarbox() {
    // Square 1:1 video (1080x1080) inside standard 16:9 viewport (1920x1080)
    let geom = ViewportGeometry::compute(1920.0, 1080.0, 1080.0, 1080.0);
    assert_eq!(geom.mode, ViewportDisplayMode::Pillarbox);
    assert_eq!(geom.render_width, 1080.0);
    assert_eq!(geom.render_height, 1080.0);
    assert_eq!(geom.offset_x, 420.0);
    assert_eq!(geom.offset_y, 0.0);

    let aspect_rendered = geom.render_width / geom.render_height;
    assert!((aspect_rendered - 1.0).abs() < 1e-6);
    assert_eq!(geom.render_width + 2.0 * geom.offset_x, 1920.0);
}

#[test]
fn test_viewport_geometry_boundary_7_mobile_portrait_9_16_pillarbox() {
    // Mobile portrait 9:16 video (1080x1920) inside standard 16:9 viewport (1920x1080)
    let geom = ViewportGeometry::compute(1920.0, 1080.0, 1080.0, 1920.0);
    assert_eq!(geom.mode, ViewportDisplayMode::Pillarbox);
    assert_eq!(geom.render_width, 607.5);
    assert_eq!(geom.render_height, 1080.0);
    assert_eq!(geom.offset_x, 656.25);
    assert_eq!(geom.offset_y, 0.0);

    let aspect_rendered = geom.render_width / geom.render_height;
    let aspect_expected = 9.0 / 16.0;
    assert!((aspect_rendered - aspect_expected).abs() < 1e-6);
    assert_eq!(geom.render_width + 2.0 * geom.offset_x, 1920.0);
}

#[test]
fn test_viewport_geometry_boundary_8_standard_16_9_in_9_16_portrait_letterbox() {
    // Standard 16:9 video (1920x1080) inside mobile portrait 9:16 viewport (1080x1920)
    let geom = ViewportGeometry::compute(1080.0, 1920.0, 1920.0, 1080.0);
    assert_eq!(geom.mode, ViewportDisplayMode::Letterbox);
    assert_eq!(geom.render_width, 1080.0);
    assert_eq!(geom.render_height, 607.5);
    assert_eq!(geom.offset_x, 0.0);
    assert_eq!(geom.offset_y, 656.25);

    let aspect_rendered = geom.render_width / geom.render_height;
    let aspect_expected = 16.0 / 9.0;
    assert!((aspect_rendered - aspect_expected).abs() < 1e-6);
    assert_eq!(geom.render_height + 2.0 * geom.offset_y, 1920.0);
}

#[test]
fn test_viewport_geometry_boundary_9_zero_and_degenerate_dimensions() {
    // 0x0 viewport or video dimensions must return ViewportDisplayMode::Empty with zero dimensions
    let cases = [
        (0.0, 0.0, 1920.0, 1080.0),
        (1280.0, 720.0, 0.0, 0.0),
        (0.0, 720.0, 1920.0, 1080.0),
        (1280.0, 0.0, 1920.0, 1080.0),
        (1280.0, 720.0, 0.0, 1080.0),
        (1280.0, 720.0, 1920.0, 0.0),
        (0.0, 0.0, 0.0, 0.0),
    ];

    for (aw, ah, vw, vh) in cases {
        let geom = ViewportGeometry::compute(aw, ah, vw, vh);
        assert_eq!(
            geom.mode,
            ViewportDisplayMode::Empty,
            "Failed on input ({aw}, {ah}, {vw}, {vh})"
        );
        assert_eq!(geom.render_width, 0.0);
        assert_eq!(geom.render_height, 0.0);
        assert_eq!(geom.offset_x, 0.0);
        assert_eq!(geom.offset_y, 0.0);
    }
}

#[test]
fn test_viewport_geometry_boundary_10_negative_values() {
    // Negative dimensions must return ViewportDisplayMode::Empty without panicking
    let cases = [
        (-100.0, 720.0, 1920.0, 1080.0),
        (1280.0, -720.0, 1920.0, 1080.0),
        (1280.0, 720.0, -1920.0, 1080.0),
        (1280.0, 720.0, 1920.0, -1080.0),
        (-500.0, -500.0, 1920.0, 1080.0),
        (1280.0, 720.0, -500.0, -500.0),
        (-1.0, -1.0, -1.0, -1.0),
    ];

    for (aw, ah, vw, vh) in cases {
        let geom = ViewportGeometry::compute(aw, ah, vw, vh);
        assert_eq!(
            geom.mode,
            ViewportDisplayMode::Empty,
            "Failed on negative input ({aw}, {ah}, {vw}, {vh})"
        );
        assert_eq!(geom.render_width, 0.0);
        assert_eq!(geom.render_height, 0.0);
        assert_eq!(geom.offset_x, 0.0);
        assert_eq!(geom.offset_y, 0.0);
    }
}

#[test]
fn test_viewport_geometry_boundary_extreme_needle_aspect_ratios() {
    // Extreme horizontal needle: 100,000 x 1 video in 1920x1080 viewport
    let geom_h = ViewportGeometry::compute(1920.0, 1080.0, 100_000.0, 1.0);
    assert_eq!(geom_h.mode, ViewportDisplayMode::Letterbox);
    assert_eq!(geom_h.render_width, 1920.0);
    assert!(geom_h.render_height < 0.1);
    assert_eq!(geom_h.offset_x, 0.0);
    assert!((geom_h.offset_y - 540.0).abs() < 0.1);

    // Extreme vertical needle: 1 x 100,000 video in 1920x1080 viewport
    let geom_v = ViewportGeometry::compute(1920.0, 1080.0, 1.0, 100_000.0);
    assert_eq!(geom_v.mode, ViewportDisplayMode::Pillarbox);
    assert!(geom_v.render_width < 0.1);
    assert_eq!(geom_v.render_height, 1080.0);
    assert_eq!(geom_v.offset_y, 0.0);
    assert!((geom_v.offset_x - 960.0).abs() < 0.1);
}

#[test]
fn test_viewport_geometry_anamorphic_239_aspect() {
    // 2.39:1 widescreen cinema format (1920x803) in 16:9 viewport (1920x1080)
    let geom = ViewportGeometry::compute(1920.0, 1080.0, 1920.0, 803.0);
    assert_eq!(geom.mode, ViewportDisplayMode::Letterbox);
    assert_eq!(geom.render_width, 1920.0);
    assert_eq!(geom.render_height, 803.0);
    assert_eq!(geom.offset_x, 0.0);
    assert_eq!(geom.offset_y, 138.5);
    assert_eq!(geom.render_height + 2.0 * geom.offset_y, 1080.0);
}

// ============================================================================
// PART 2: SEEK DEBOUNCER 10 HZ RATE LIMITING & RELEASE SEEK CHALLENGES
// ============================================================================

#[test]
fn test_seek_debouncer_120hz_drag_over_500ms() {
    let mut debouncer = SeekDebouncer::new(100.0);
    let total_drag_ms = 500.0;
    let event_interval_ms = 1000.0 / 120.0; // 8.3333 ms

    let mut intermediate_commands = Vec::new();
    let mut t = 0.0;
    while t <= total_drag_ms {
        if let Some(seek_t) = debouncer.on_drag_move(t) {
            intermediate_commands.push(seek_t);
        }
        t += event_interval_ms;
    }

    // Verify visual thumb received all 61 move events without lag
    assert_eq!(debouncer.visual_updates_count(), 61);

    // Verify exactly 5 intermediate seeks emitted (at t = 0.0, 108.3, 208.3, 316.7, 425.0)
    assert_eq!(
        intermediate_commands.len(),
        5,
        "120 Hz 500ms drag must emit exactly 5 intermediate seeks"
    );

    // Verify rate limit invariant: interval between consecutive seeks >= 100ms
    for window in intermediate_commands.windows(2) {
        let delta = window[1] - window[0];
        assert!(
            delta >= 100.0 - 1e-6,
            "Intermediate seek interval ({delta}ms) violated 100ms lower bound"
        );
    }

    // Release at t = 505.0 ms: immediately emits authoritative final seek
    let release_seek = debouncer.on_drag_release(505.0);
    assert_eq!(release_seek, 505.0);
    assert!(!debouncer.is_dragging());
}

#[test]
fn test_seek_debouncer_240hz_drag_over_500ms() {
    let mut debouncer = SeekDebouncer::new(100.0);
    let total_drag_ms = 500.0;
    let event_interval_ms = 1000.0 / 240.0; // 4.1667 ms

    let mut intermediate_commands = Vec::new();
    let mut t = 0.0;
    while t <= total_drag_ms + 1e-6 {
        if let Some(seek_t) = debouncer.on_drag_move(t) {
            intermediate_commands.push(seek_t);
        }
        t += event_interval_ms;
    }

    // 121 visual thumb updates
    assert_eq!(debouncer.visual_updates_count(), 121);

    // Exactly 5 intermediate seeks
    assert_eq!(
        intermediate_commands.len(),
        5,
        "240 Hz 500ms drag must emit exactly 5 intermediate seeks"
    );

    let release_seek = debouncer.on_drag_release(505.0);
    assert_eq!(release_seek, 505.0);
}

#[test]
fn test_seek_debouncer_1000hz_drag_event_flooding() {
    let mut debouncer = SeekDebouncer::new(100.0);
    let total_drag_ms = 500.0;
    let event_interval_ms = 1.0; // 1.0 ms = 1000 Hz

    let mut intermediate_commands = Vec::new();
    let mut t = 0.0;
    while t < total_drag_ms {
        if let Some(seek_t) = debouncer.on_drag_move(t) {
            intermediate_commands.push(seek_t);
        }
        t += event_interval_ms;
    }

    assert_eq!(debouncer.visual_updates_count(), 500);
    assert_eq!(
        intermediate_commands.len(),
        5,
        "1000 Hz 500ms drag must emit exactly 5 intermediate seeks"
    );
    assert_eq!(
        intermediate_commands,
        vec![0.0, 100.0, 200.0, 300.0, 400.0]
    );

    let release_seek = debouncer.on_drag_release(500.0);
    assert_eq!(release_seek, 500.0);
}

#[test]
fn test_seek_debouncer_short_flick_50ms() {
    let mut debouncer = SeekDebouncer::new(100.0);
    let dt = 1000.0 / 120.0;
    let mut t = 0.0;
    let mut intermediate = Vec::new();

    while t < 50.0 {
        if let Some(st) = debouncer.on_drag_move(t) {
            intermediate.push(st);
        }
        t += dt;
    }

    // Only leading-edge seek at t=0
    assert_eq!(
        intermediate.len(),
        1,
        "50ms flick must emit only 1 leading-edge intermediate seek"
    );
    assert_eq!(intermediate[0], 0.0);

    // Immediate authoritative release seek at t=50ms
    let rel = debouncer.on_drag_release(50.0);
    assert_eq!(rel, 50.0);
}

#[test]
fn test_seek_debouncer_instant_rate_limiting_and_release() {
    let mut debouncer = SeekDebouncer::new(100.0);
    let t0 = Instant::now();

    // 1. Initial drag event emits immediately (leading-edge)
    assert!(!debouncer.is_dragging());
    let s1 = debouncer.on_drag_instant(10_000, t0);
    assert_eq!(s1, Some(10_000));
    assert!(debouncer.is_dragging());

    // 2. High-frequency drag events within 100ms interval are suppressed
    let s2 = debouncer.on_drag_instant(10_500, t0 + Duration::from_millis(15));
    assert_eq!(s2, None);
    let s3 = debouncer.on_drag_instant(11_000, t0 + Duration::from_millis(45));
    assert_eq!(s3, None);
    let s4 = debouncer.on_drag_instant(11_500, t0 + Duration::from_millis(85));
    assert_eq!(s4, None);
    let s5 = debouncer.on_drag_instant(11_900, t0 + Duration::from_millis(99));
    assert_eq!(s5, None);

    // 3. Next event after >= 100ms emits
    let s6 = debouncer.on_drag_instant(12_000, t0 + Duration::from_millis(101));
    assert_eq!(s6, Some(12_000));

    // 4. Another suppressed event within the new window
    let s7 = debouncer.on_drag_instant(12_200, t0 + Duration::from_millis(104));
    assert_eq!(s7, None);

    // 5. Mouse release at t = 105ms (only 4ms after last emitted seek!):
    // MUST immediately emit authoritative final seek regardless of debounce interval!
    let s_release = debouncer.on_release_instant(12_500, t0 + Duration::from_millis(105));
    assert_eq!(s_release, 12_500);
    assert!(!debouncer.is_dragging());

    // Total intermediate seeks: 2 (at t0 and t0+101ms)
    assert_eq!(debouncer.intermediate_seeks().len(), 2);
    // Total visual updates: 7 drag events
    assert_eq!(debouncer.visual_updates_count(), 7);
}

#[test]
fn test_seek_debouncer_reset_and_reuse() {
    let mut debouncer = SeekDebouncer::new(100.0);
    let _ = debouncer.on_drag_move(0.0);
    let _ = debouncer.on_drag_move(50.0);
    assert_eq!(debouncer.visual_updates_count(), 2);
    assert_eq!(debouncer.intermediate_seeks().len(), 1);
    assert!(debouncer.is_dragging());

    // Reset clears all counters and state
    debouncer.reset();
    assert_eq!(debouncer.visual_updates_count(), 0);
    assert!(debouncer.intermediate_seeks().is_empty());
    assert!(!debouncer.is_dragging());

    // Ready to drag again immediately
    let s = debouncer.on_drag_move(0.0);
    assert_eq!(s, Some(0.0));
    assert!(debouncer.is_dragging());
}

#[test]
fn test_seek_debouncer_long_sustained_drag_2000ms() {
    let mut debouncer = SeekDebouncer::new(100.0);
    let total_drag_ms = 2000.0;
    let event_interval_ms = 1000.0 / 120.0; // 8.3333 ms

    let mut intermediate_commands = Vec::new();
    let mut t = 0.0;
    while t < total_drag_ms {
        if let Some(seek_t) = debouncer.on_drag_move(t) {
            intermediate_commands.push(seek_t);
        }
        t += event_interval_ms;
    }

    let release_seek = debouncer.on_drag_release(2000.0);
    assert_eq!(release_seek, 2000.0);

    // 241 drag moves processed visually (from t=0.0 to t=1999.999ms)
    assert_eq!(debouncer.visual_updates_count(), 241);

    // Exactly 19 intermediate seeks emitted due to 108.33ms quantization (19 * 108.33 = 1958ms)
    assert_eq!(
        intermediate_commands.len(),
        19,
        "2000ms scrub at 120 Hz emits exactly 19 intermediate seeks"
    );
}
