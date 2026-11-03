//! Integration tests for the soft-knee audio saturation limiter in `syncverse-codecs`.
//!
//! Verifies that `soft_clip_sample` provides bit-exact identity for $|s| \le 0.85$,
//! smooth saturation bounded within `[-1.0, 1.0]` for large amplitudes without discontinuity,
//! odd symmetry, and smooth derivative continuity at the knee.

#![deny(warnings)]

use syncverse_codecs::soft_clip_sample;

#[test]
fn test_soft_clip_linear_region_exact_identity() {
    // Exact passthrough for |s| <= 0.85
    let test_points = [
        0.0f32,
        0.001,
        -0.001,
        0.1,
        -0.1,
        0.25,
        -0.25,
        0.5,
        -0.5,
        0.75,
        -0.75,
        0.80,
        -0.80,
        0.8499,
        -0.8499,
        0.85,
        -0.85,
    ];

    for &s in &test_points {
        let clipped = soft_clip_sample(s);
        assert_eq!(
            clipped, s,
            "Linear region must preserve sample exactly for s = {}",
            s
        );
    }

    // Dense sweep in linear region [-0.85, 0.85]
    for i in 0..=850 {
        let s = (i as f32) / 1000.0;
        assert_eq!(soft_clip_sample(s), s);
        assert_eq!(soft_clip_sample(-s), -s);
    }
}

#[test]
fn test_soft_clip_sweep_neg2_to_pos2() {
    let mut prev_clipped = -2.0f32;

    // Sweep 4,001 points from -2.0 to 2.0 in steps of 0.001
    for i in 0..=4000 {
        let s = -2.0 + (i as f32) * 0.001;
        let clipped = soft_clip_sample(s);

        // Saturation bounds in [-1.0, 1.0]
        assert!(
            (-1.0..=1.0).contains(&clipped),
            "Sample must remain bounded in [-1.0, 1.0], got {} for s = {}",
            clipped,
            s
        );

        // Linear passthrough verification
        if s.abs() <= 0.85 {
            assert_eq!(clipped, s);
        } else if s > 0.85 {
            assert!(
                clipped > 0.85 && clipped <= 1.0,
                "Positive saturation region must be in (0.85, 1.0], got {}",
                clipped
            );
        } else {
            assert!(
                (-1.0..-0.85).contains(&clipped),
                "Negative saturation region must be in [-1.0, -0.85), got {}",
                clipped
            );
        }

        // Odd symmetry: f(-s) == -f(s)
        let neg_clipped = soft_clip_sample(-s);
        assert!(
            (clipped + neg_clipped).abs() < 1e-6,
            "Odd symmetry violated at s = {}: f(s) = {}, f(-s) = {}",
            s,
            clipped,
            neg_clipped
        );

        // Monotonic non-decreasing across full range
        if i > 0 {
            assert!(
                clipped >= prev_clipped,
                "Monotonicity violated at s = {}: prev = {}, curr = {}",
                s,
                prev_clipped,
                clipped
            );
        }
        prev_clipped = clipped;
    }
}

#[test]
fn test_soft_clip_strict_increase_below_saturation_ceiling() {
    // For s in [-1.5, 1.5], soft_clip_sample is strictly increasing in f32
    let mut prev = soft_clip_sample(-1.5);
    for i in 1..=300 {
        let s = -1.5 + (i as f32) * 0.01;
        let curr = soft_clip_sample(s);
        assert!(
            curr > prev,
            "Strict increase violated at s = {}: curr = {}, prev = {}",
            s,
            curr,
            prev
        );
        prev = curr;
    }
}

#[test]
fn test_soft_clip_continuity_at_knee_boundary() {
    let knee = 0.85f32;
    let delta = 0.001f32;

    let just_below = soft_clip_sample(knee - delta);
    let at_knee = soft_clip_sample(knee);
    let just_above = soft_clip_sample(knee + delta);

    // C0 Continuity: value is continuous across knee
    assert_eq!(at_knee, knee);
    assert!((at_knee - just_below).abs() < delta + 1e-5);
    assert!((just_above - at_knee).abs() < delta + 1e-5);

    // C1 Continuity: derivative matches 1.0 on both sides
    let deriv_left = (at_knee - just_below) / delta;
    let deriv_right = (just_above - at_knee) / delta;

    assert!(
        (deriv_left - 1.0).abs() < 1e-2,
        "Left derivative should be ~1.0, got {}",
        deriv_left
    );
    assert!(
        (deriv_right - 1.0).abs() < 1e-2,
        "Right derivative should be ~1.0, got {}",
        deriv_right
    );
    assert!(
        (deriv_left - deriv_right).abs() < 1e-2,
        "Derivative jump across knee: left = {}, right = {}",
        deriv_left,
        deriv_right
    );

    // Negative knee -0.85
    let neg_knee = -0.85f32;
    let neg_just_below = soft_clip_sample(neg_knee - delta);
    let neg_at_knee = soft_clip_sample(neg_knee);
    let neg_just_above = soft_clip_sample(neg_knee + delta);

    assert_eq!(neg_at_knee, neg_knee);
    let neg_deriv_left = (neg_at_knee - neg_just_below) / delta;
    let neg_deriv_right = (neg_just_above - neg_at_knee) / delta;

    assert!((neg_deriv_left - 1.0).abs() < 1e-2);
    assert!((neg_deriv_right - 1.0).abs() < 1e-2);
}

#[test]
fn test_soft_clip_extreme_amplitudes_bounded() {
    let extreme_values = [
        2.5f32,
        3.0,
        5.0,
        10.0,
        50.0,
        100.0,
        1000.0,
        100_000.0,
        1e6,
    ];

    for &val in &extreme_values {
        let pos = soft_clip_sample(val);
        let neg = soft_clip_sample(-val);

        assert!(
            pos > 0.85 && pos <= 1.0,
            "Extreme positive {} saturated to {}, must be in (0.85, 1.0]",
            val,
            pos
        );
        assert!(
            (-1.0..-0.85).contains(&neg),
            "Extreme negative {} saturated to {}, must be in [-1.0, -0.85)",
            -val,
            neg
        );
        assert!(
            (pos + neg).abs() < 1e-5,
            "Symmetry violated at extreme value {}",
            val
        );
    }
}

#[test]
fn test_multichannel_downmix_smooth_compression() {
    // Moderate downmix overshoot (e.g. 1.25)
    let moderate_sum = 1.25f32;
    let soft_moderate = soft_clip_sample(moderate_sum);
    assert!(
        soft_moderate > 0.85 && soft_moderate < 1.0,
        "Moderate downmix peak (1.25) must compress to (0.85, 1.0), got {}",
        soft_moderate
    );
    assert!(
        soft_moderate < moderate_sum,
        "Limiter must attenuate signal above 0.85"
    );

    // High downmix peak (e.g. 1.6)
    let high_sum = 1.6f32;
    let soft_high = soft_clip_sample(high_sum);
    assert!(
        soft_high > soft_moderate && soft_high <= 1.0,
        "High downmix peak must compress monotonically, got {}",
        soft_high
    );
}
