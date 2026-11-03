//! Integration tests for discrete PI PLL target rate tracking in `syncverse-sync`.
//!
//! Verifies that `PllSpeedController::update_with_target_rate` correctly scales
//! micro-speed steering relative to arbitrary target rates (0.5x, 1.0x, 1.25x, 1.5x, 2.0x),
//! enforces `[r_target * 0.96, r_target * 1.04]` clamping in Zone 2, holds `r_target`
//! in Zone 1 (Deadband), and triggers hard seeks in Zone 3 when $|e| > 1500$ ms.

#![deny(warnings)]

use syncverse_sync::{PllSpeedController, PllZone};

const TEST_TARGET_RATES: [f64; 5] = [0.5, 1.0, 1.25, 1.5, 2.0];

#[test]
fn test_pll_zone1_deadband_exact_target_rate() {
    let mut controller = PllSpeedController::new();
    let dt = 0.2; // 200 ms sample interval

    // Deadband is |e| < 15 ms (-14 ms to +14 ms)
    let deadband_errors_ms = [-14.0, -10.0, -5.0, 0.0, 5.0, 10.0, 14.0];

    for &r_target in &TEST_TARGET_RATES {
        for &err_ms in &deadband_errors_ms {
            controller.reset();
            let base_pts = 100_000u64;
            let target_pts = (base_pts as f64 + err_ms).round() as u64;
            let actual_pts = base_pts;

            let corr = controller.update_with_target_rate(target_pts, actual_pts, dt, r_target);

            assert_eq!(
                corr.zone,
                PllZone::Deadband,
                "Expected Zone 1 Deadband for error {} ms at rate {}",
                err_ms,
                r_target
            );
            assert!(
                (corr.speed_multiplier - r_target).abs() < 1e-9,
                "Deadband speed must equal r_target ({}), got {}",
                r_target,
                corr.speed_multiplier
            );
            assert_eq!(corr.hard_seek_target, None);
            assert!(
                (corr.phase_error_ms - (target_pts as f64 - actual_pts as f64)).abs() < 1e-9
            );
        }
    }
}

#[test]
fn test_pll_zone2_microspeed_bounds_and_directionality() {
    let mut controller = PllSpeedController::new();
    let dt = 0.2;

    // Test errors across Zone 2: 15 ms <= |e| <= 1500 ms
    let positive_errors_ms: [f64; 7] = [20.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 1500.0];
    let negative_errors_ms: [f64; 7] = [-20.0, -50.0, -100.0, -250.0, -500.0, -1000.0, -1500.0];

    for &r_target in &TEST_TARGET_RATES {
        let expected_min = r_target * 0.96;
        let expected_max = r_target * 1.04;

        // Positive phase error (client is behind -> must speed up)
        for &err_ms in &positive_errors_ms {
            controller.reset();
            let actual_pts = 100_000u64;
            let target_pts = actual_pts + err_ms as u64;

            let corr = controller.update_with_target_rate(target_pts, actual_pts, dt, r_target);

            assert_eq!(
                corr.zone,
                PllZone::MicroSpeed,
                "Expected MicroSpeed for error +{} ms at rate {}",
                err_ms,
                r_target
            );
            assert!(
                corr.speed_multiplier > r_target,
                "Positive error must speed up above r_target ({}), got {}",
                r_target,
                corr.speed_multiplier
            );
            assert!(
                corr.speed_multiplier <= expected_max + 1e-9,
                "Speed must not exceed r_target * 1.04 ({}), got {}",
                expected_max,
                corr.speed_multiplier
            );
            assert!(
                corr.speed_multiplier >= expected_min - 1e-9,
                "Speed must not drop below r_target * 0.96 ({}), got {}",
                expected_min,
                corr.speed_multiplier
            );
            assert_eq!(corr.hard_seek_target, None);
        }

        // Negative phase error (client is ahead -> must slow down)
        for &err_ms in &negative_errors_ms {
            controller.reset();
            let actual_pts = 100_000u64;
            let target_pts = actual_pts - err_ms.abs() as u64;

            let corr = controller.update_with_target_rate(target_pts, actual_pts, dt, r_target);

            assert_eq!(
                corr.zone,
                PllZone::MicroSpeed,
                "Expected MicroSpeed for error {} ms at rate {}",
                err_ms,
                r_target
            );
            assert!(
                corr.speed_multiplier < r_target,
                "Negative error must slow down below r_target ({}), got {}",
                r_target,
                corr.speed_multiplier
            );
            assert!(
                corr.speed_multiplier >= expected_min - 1e-9,
                "Speed must not drop below r_target * 0.96 ({}), got {}",
                expected_min,
                corr.speed_multiplier
            );
            assert!(
                corr.speed_multiplier <= expected_max + 1e-9,
                "Speed must not exceed r_target * 1.04 ({}), got {}",
                expected_max,
                corr.speed_multiplier
            );
            assert_eq!(corr.hard_seek_target, None);
        }
    }
}

#[test]
fn test_pll_zone3_hard_seek_trigger_and_reset() {
    let mut controller = PllSpeedController::new();
    let dt = 0.2;

    // Hard seek triggered when |e| > 1500 ms (1.500 s)
    let hard_seek_offsets_ms = [1501, 2000, 5000, 60000];

    for &r_target in &TEST_TARGET_RATES {
        for &offset in &hard_seek_offsets_ms {
            // Forward jump (target is ahead by > 1500 ms)
            let actual_pts = 50_000u64;
            let target_pts = actual_pts + offset;

            let corr = controller.update_with_target_rate(target_pts, actual_pts, dt, r_target);

            assert_eq!(
                corr.zone,
                PllZone::HardSeek,
                "Expected HardSeek for +{} ms at rate {}",
                offset,
                r_target
            );
            assert_eq!(
                corr.hard_seek_target,
                Some(target_pts),
                "Hard seek target must equal target_pts"
            );
            assert!(
                (corr.speed_multiplier - r_target).abs() < 1e-9,
                "Hard seek should hold r_target ({}), got {}",
                r_target,
                corr.speed_multiplier
            );

            // Backward jump (target is behind by > 1500 ms)
            let actual_pts_back = 100_000u64;
            let target_pts_back = actual_pts_back - offset;

            let corr_back =
                controller.update_with_target_rate(target_pts_back, actual_pts_back, dt, r_target);

            assert_eq!(
                corr_back.zone,
                PllZone::HardSeek,
                "Expected HardSeek for -{} ms at rate {}",
                offset,
                r_target
            );
            assert_eq!(corr_back.hard_seek_target, Some(target_pts_back));
            assert!((corr_back.speed_multiplier - r_target).abs() < 1e-9);
        }
    }
}

#[test]
fn test_pll_closed_loop_convergence_at_target_rates() {
    // Simulate plant convergence for each target rate starting with +400 ms initial drift
    let dt = 0.2; // 200 ms ticks
    let steps = (25.0 / dt) as usize; // 25 seconds simulation

    for &r_target in &TEST_TARGET_RATES {
        let mut controller = PllSpeedController::new();
        let mut initial_error = 0.400f64; // +400 ms
        let mut time_entered_deadband: Option<f64> = None;
        let mut excessive_overshoot = false;

        for step in 0..steps {
            let t = (step as f64) * dt;

            let target_ms = 100_000 + (initial_error * 1000.0).round() as u64;
            let actual_ms = 100_000u64;

            let corr = controller.update_with_target_rate(target_ms, actual_ms, dt, r_target);

            if initial_error.abs() < 0.015 && time_entered_deadband.is_none() {
                time_entered_deadband = Some(t);
            }

            // Plant update:
            // target advances at r_target, actual advances at speed_multiplier
            // delta_phase = (corr.speed_multiplier - r_target) * dt
            // error[k+1] = error[k] - delta_phase
            let delta_phase = (corr.speed_multiplier - r_target) * dt;
            let next_error = initial_error - delta_phase;

            // Check if controller overshoots heavily beyond -15 ms deadband
            if next_error < -0.050 {
                excessive_overshoot = true;
            }

            initial_error = next_error;
        }

        assert!(
            time_entered_deadband.is_some(),
            "Rate {} must converge into deadband within 25 seconds",
            r_target
        );
        let t_entered = time_entered_deadband.unwrap();
        assert!(
            t_entered <= 20.0,
            "Rate {} must enter deadband within 20s, entered at {}s",
            r_target,
            t_entered
        );
        assert!(
            !excessive_overshoot,
            "Rate {} exhibited excessive overshoot ({})",
            r_target,
            initial_error
        );
    }
}

#[test]
fn test_pll_update_backward_compatibility() {
    let mut ctrl_legacy = PllSpeedController::new();
    let mut ctrl_new = PllSpeedController::new();

    let dt = 0.2;
    let actual = 100_000u64;

    // Test errors across Zone 1, Zone 2, and Zone 3
    for err in &[-2000, -800, -200, -5, 0, 8, 300, 1200, 3500] {
        let target = (actual as i64 + err) as u64;

        let legacy_corr = ctrl_legacy.update(target, actual, dt);
        let new_corr = ctrl_new.update_with_target_rate(target, actual, dt, 1.0);

        assert_eq!(legacy_corr.zone, new_corr.zone);
        assert_eq!(legacy_corr.hard_seek_target, new_corr.hard_seek_target);
        assert!((legacy_corr.speed_multiplier - new_corr.speed_multiplier).abs() < 1e-12);
        assert!((legacy_corr.phase_error_ms - new_corr.phase_error_ms).abs() < 1e-12);
    }
}
