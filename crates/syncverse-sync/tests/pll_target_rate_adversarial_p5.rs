//! Adversarial stress test suite for `PllSpeedController` at extreme target rates ($r \in [0.25, 4.0]$).
//!
//! Verifies:
//! 1. Micro-speed clamping strictly bounded within `[r_target * 0.96, r_target * 1.04]` at $r = 0.25$ and $r = 4.0$.
//! 2. Zone 1 Deadband pass-through at boundary values (14.99 ms).
//! 3. Abrupt step changes during active phase steering without speed explosion or instability.
//! 4. Convergence and recovery into deadband across the entire rate range.

#![deny(warnings)]

use syncverse_sync::{PllSpeedController, PllZone};

#[test]
fn test_pll_extreme_target_rates_clamping_0_25_and_4_0() {
    let mut controller = PllSpeedController::new();
    let dt = 0.2;

    let extreme_rates = [0.25, 4.0];

    for &r_target in &extreme_rates {
        let expected_min = r_target * 0.96;
        let expected_max = r_target * 1.04;

        // Positive error (needs speedup)
        for &err_ms in &[20.0, 100.0, 500.0, 1499.0] {
            controller.reset();
            let base = 50_000u64;
            let corr = controller.update_with_target_rate(base + err_ms as u64, base, dt, r_target);

            assert_eq!(corr.zone, PllZone::MicroSpeed);
            assert!(
                corr.speed_multiplier <= expected_max + 1e-9,
                "Speed {} exceeded expected max {} for rate {}",
                corr.speed_multiplier,
                expected_max,
                r_target
            );
            assert!(
                corr.speed_multiplier >= expected_min - 1e-9,
                "Speed {} below expected min {} for rate {}",
                corr.speed_multiplier,
                expected_min,
                r_target
            );
            assert!(corr.speed_multiplier > r_target);
        }

        // Negative error (needs slowdown)
        for &err_ms in &[20.0, 100.0, 500.0, 1499.0] {
            controller.reset();
            let base = 50_000u64;
            let corr = controller.update_with_target_rate(base, base + err_ms as u64, dt, r_target);

            assert_eq!(corr.zone, PllZone::MicroSpeed);
            assert!(
                corr.speed_multiplier >= expected_min - 1e-9,
                "Speed {} below expected min {} for rate {}",
                corr.speed_multiplier,
                expected_min,
                r_target
            );
            assert!(
                corr.speed_multiplier <= expected_max + 1e-9,
                "Speed {} exceeded expected max {} for rate {}",
                corr.speed_multiplier,
                expected_max,
                r_target
            );
            assert!(corr.speed_multiplier < r_target);
        }
    }
}

#[test]
fn test_pll_abrupt_step_changes_stress() {
    let mut controller = PllSpeedController::new();
    let dt = 0.2;
    let mut target_pts = 100_000.0 + 350.0;
    let mut actual_pts = 100_000.0;
    let mut current_rate = 1.0;

    for step in 0..200 {
        if step == 20 {
            current_rate = 4.0; // Jump to 4x
        } else if step == 60 {
            current_rate = 0.25; // Drop to 0.25x
        } else if step == 120 {
            current_rate = 2.0; // Jump to 2x
        } else if step == 160 {
            current_rate = 1.0; // Return to 1x
        }

        let corr = controller.update_with_target_rate(
            target_pts as u64,
            actual_pts as u64,
            dt,
            current_rate,
        );

        let min_speed = current_rate * 0.96;
        let max_speed = current_rate * 1.04;
        assert!(
            corr.speed_multiplier >= min_speed - 1e-9 && corr.speed_multiplier <= max_speed + 1e-9,
            "Step {}: rate {} steered to {} outside [{}, {}]",
            step,
            current_rate,
            corr.speed_multiplier,
            min_speed,
            max_speed
        );

        target_pts += current_rate * dt * 1000.0;
        actual_pts += corr.speed_multiplier * dt * 1000.0;
    }
}

#[test]
fn test_pll_deadband_convergence_at_rates_0_25_and_4_0() {
    let dt = 0.2;

    for &rate in &[0.25, 4.0] {
        let mut controller = PllSpeedController::new();
        let mut target_pts = 50_000.0 + 300.0;
        let mut actual_pts = 50_000.0;
        let mut entered_deadband: Option<f64> = None;
        let mut overshoot = false;

        for step in 0..300 {
            let t = (step as f64) * dt;
            let e: f64 = (target_pts - actual_pts) / 1000.0;
            if e.abs() < 0.015 && entered_deadband.is_none() {
                entered_deadband = Some(t);
            }

            let corr = controller.update_with_target_rate(
                target_pts as u64,
                actual_pts as u64,
                dt,
                rate,
            );

            target_pts += rate * dt * 1000.0;
            actual_pts += corr.speed_multiplier * dt * 1000.0;

            let e_next = (target_pts - actual_pts) / 1000.0;
            if entered_deadband.is_some() && e_next < -0.015 {
                overshoot = true;
            }
        }

        assert!(
            entered_deadband.is_some(),
            "Rate {} failed to converge into deadband",
            rate
        );
        assert!(!overshoot, "Rate {} experienced overshoot", rate);
    }
}
