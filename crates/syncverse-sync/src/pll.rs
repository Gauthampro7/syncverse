//! Discrete Proportional-Integral (PI) Phase-Locked Loop (PLL) micro-speed controller.

/// Operational drift zone categorized by the PLL controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PllZone {
    /// Zone 1: Deadband In-Sync (|e| < 15ms); speed held at 1.0x.
    Deadband,
    /// Zone 2: Continuous Micro-Speed Slew (15ms <= |e| <= 1500ms); speed modulated in [0.96, 1.04].
    MicroSpeed,
    /// Zone 3: Discontinuous Hard Seek (|e| > 1500ms); hard seek commanded.
    HardSeek,
}

/// Correction command emitted by the PLL speed controller on each evaluation tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PllCorrection {
    /// Commanded playback speed multiplier (in range [0.96, 1.04]).
    pub speed_multiplier: f64,
    /// Target presentation timestamp in milliseconds if a hard seek is required.
    pub hard_seek_target: Option<u64>,
    /// Measured phase error in milliseconds (`target_pts - actual_pts`).
    pub phase_error_ms: f64,
    /// Active operational zone.
    pub zone: PllZone,
}

/// Critically damped discrete PI PLL micro-speed controller.
#[derive(Debug, Clone)]
pub struct PllSpeedController {
    /// Proportional gain Kp in s^-1.
    pub kp: f64,
    /// Integral gain Ki in s^-2.
    pub ki: f64,
    /// Anti-windup clamping ceiling in seconds.
    pub i_max: f64,
    /// Minimum allowed speed multiplier.
    pub r_min: f64,
    /// Maximum allowed speed multiplier.
    pub r_max: f64,
    /// In-sync deadband threshold in seconds (0.015s = 15ms).
    pub deadband_secs: f64,
    /// Hard seek threshold in seconds (1.500s = 1500ms).
    pub hard_seek_threshold_secs: f64,
    /// Commanded target playback speed ratio (default: 1.0).
    pub r_target: f64,
    /// Running integral accumulator in seconds.
    integral: f64,
}

impl Default for PllSpeedController {
    fn default() -> Self {
        Self {
            kp: 0.17,
            ki: 0.0072,
            i_max: 32.0,
            r_min: 0.96,
            r_max: 1.04,
            deadband_secs: 0.015,
            hard_seek_threshold_secs: 1.500,
            r_target: 1.0,
            integral: 0.0,
        }
    }
}

impl PllSpeedController {
    /// Constructs a new `PllSpeedController` with spec-standard critically damped gains.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resets the integral accumulator and controller state.
    pub fn reset(&mut self) {
        self.integral = 0.0;
    }

    /// Evaluates current phase drift and calculates the speed correction command relative to target rate `r_target`.
    ///
    /// # Arguments
    /// * `target_pts_ms` - Target media presentation timestamp from room reference timeline.
    /// * `actual_pts_ms` - Actual rendered media presentation timestamp from local clock.
    /// * `dt_secs` - Evaluation sample interval in seconds (nominally 0.2s).
    /// * `r_target` - Commanded room target speed multiplier (e.g. 1.0, 1.25, 0.75).
    pub fn update_with_target_rate(
        &mut self,
        target_pts_ms: u64,
        actual_pts_ms: u64,
        dt_secs: f64,
        r_target: f64,
    ) -> PllCorrection {
        self.r_target = r_target;
        let phase_error_ms = (target_pts_ms as f64) - (actual_pts_ms as f64);
        let e = phase_error_ms / 1000.0; // convert ms to seconds

        if e.abs() < self.deadband_secs {
            // Zone 1: Deadband In-Sync (|e| < 15ms)
            // Integral accumulator held constant to prevent hunting oscillations
            PllCorrection {
                speed_multiplier: r_target,
                hard_seek_target: None,
                phase_error_ms,
                zone: PllZone::Deadband,
            }
        } else if e.abs() <= self.hard_seek_threshold_secs {
            // Zone 2: Discrete PI controller (15ms <= |e| <= 1500ms)
            let i_candidate = self.integral + e * dt_secs;
            self.integral = i_candidate.clamp(-self.i_max, self.i_max);

            let delta_r = self.kp * e + self.ki * self.integral;
            let speed = (r_target * (1.0 + delta_r)).clamp(r_target * self.r_min, r_target * self.r_max);

            PllCorrection {
                speed_multiplier: speed,
                hard_seek_target: None,
                phase_error_ms,
                zone: PllZone::MicroSpeed,
            }
        } else {
            // Zone 3: Discontinuous Hard Seek (|e| > 1500ms)
            self.reset();
            PllCorrection {
                speed_multiplier: r_target,
                hard_seek_target: Some(target_pts_ms),
                phase_error_ms,
                zone: PllZone::HardSeek,
            }
        }
    }

    /// Evaluates current phase drift and calculates the speed correction command assuming a 1.0x target rate.
    ///
    /// Delegates to [`PllSpeedController::update_with_target_rate`] with `r_target = 1.0`.
    ///
    /// # Arguments
    /// * `target_pts_ms` - Target media presentation timestamp from room reference timeline.
    /// * `actual_pts_ms` - Actual rendered media presentation timestamp from local clock.
    /// * `dt_secs` - Evaluation sample interval in seconds (nominally 0.2s).
    pub fn update(&mut self, target_pts_ms: u64, actual_pts_ms: u64, dt_secs: f64) -> PllCorrection {
        self.update_with_target_rate(target_pts_ms, actual_pts_ms, dt_secs, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pi_controller_convergence_dynamics() {
        let mut controller = PllSpeedController::new();
        let dt = 0.2; // 200ms
        let mut e = 0.450f64; // initial phase error +450ms
        let mut time_entered_deadband: Option<f64> = None;
        let mut overshoot = false;

        let steps = (25.0 / dt) as usize;
        for step in 0..steps {
            let t = (step as f64) * dt;

            let target_ms = 100_000 + (e * 1000.0) as u64;
            let actual_ms = 100_000;
            let corr = controller.update(target_ms, actual_ms, dt);

            if e.abs() < 0.015 && time_entered_deadband.is_none() {
                time_entered_deadband = Some(t);
            }

            // Plant update: target advances at 1.0, actual advances at r
            // e[k+1] = e[k] - (r - 1.0) * dt
            let delta_phase = (corr.speed_multiplier - 1.0) * dt;
            let e_next = e - delta_phase;

            if e_next < -0.015 && e >= -0.015 {
                overshoot = true;
            }
            e = e_next;
        }

        assert!(time_entered_deadband.is_some());
        let t_entered = time_entered_deadband.unwrap();
        assert!(
            t_entered <= 20.0,
            "Controller must enter deadband within 20s, entered at {}s",
            t_entered
        );
        assert!(!overshoot, "Critically damped controller must not overshoot past -15ms");
    }

    #[test]
    fn test_pll_target_rates_scaling() {
        let mut controller = PllSpeedController::new();

        // 1.25x target rate
        let r_125 = 1.25;
        // Zone 1: Deadband (|e| < 15ms)
        let c_dead = controller.update_with_target_rate(10_005, 10_000, 0.2, r_125);
        assert_eq!(c_dead.zone, PllZone::Deadband);
        assert!((c_dead.speed_multiplier - 1.25).abs() < 1e-9);

        // Zone 2: Micro-speed steering positive error (+200ms)
        let c_pos = controller.update_with_target_rate(10_200, 10_000, 0.2, r_125);
        assert_eq!(c_pos.zone, PllZone::MicroSpeed);
        assert!(c_pos.speed_multiplier > 1.25);
        assert!(c_pos.speed_multiplier <= 1.25 * 1.04);
        assert!(c_pos.speed_multiplier >= 1.25 * 0.96);

        // Zone 2: Micro-speed steering negative error (-200ms)
        controller.reset();
        let c_neg = controller.update_with_target_rate(9_800, 10_000, 0.2, r_125);
        assert_eq!(c_neg.zone, PllZone::MicroSpeed);
        assert!(c_neg.speed_multiplier < 1.25);
        assert!(c_neg.speed_multiplier >= 1.25 * 0.96);
        assert!(c_neg.speed_multiplier <= 1.25 * 1.04);

        // Zone 3: Hard seek (|e| > 1500ms)
        let c_seek = controller.update_with_target_rate(12_000, 10_000, 0.2, r_125);
        assert_eq!(c_seek.zone, PllZone::HardSeek);
        assert_eq!(c_seek.hard_seek_target, Some(12_000));
        assert!((c_seek.speed_multiplier - 1.25).abs() < 1e-9);

        // 1.5x target rate
        let r_150 = 1.50;
        let c_150_dead = controller.update_with_target_rate(10_000, 10_000, 0.2, r_150);
        assert_eq!(c_150_dead.zone, PllZone::Deadband);
        assert!((c_150_dead.speed_multiplier - 1.50).abs() < 1e-9);

        let c_150_pos = controller.update_with_target_rate(10_500, 10_000, 0.2, r_150);
        assert!(c_150_pos.speed_multiplier <= 1.50 * 1.04 && c_150_pos.speed_multiplier >= 1.50 * 0.96);

        // 0.75x target rate
        let r_075 = 0.75;
        let c_075_dead = controller.update_with_target_rate(10_000, 10_000, 0.2, r_075);
        assert_eq!(c_075_dead.zone, PllZone::Deadband);
        assert!((c_075_dead.speed_multiplier - 0.75).abs() < 1e-9);

        let c_075_neg = controller.update_with_target_rate(9_600, 10_000, 0.2, r_075);
        assert!(c_075_neg.speed_multiplier <= 0.75 * 1.04 && c_075_neg.speed_multiplier >= 0.75 * 0.96);

        // Backward compatibility
        let mut ctrl1 = PllSpeedController::new();
        let mut ctrl2 = PllSpeedController::new();
        let res1 = ctrl1.update(10_200, 10_000, 0.2);
        let res2 = ctrl2.update_with_target_rate(10_200, 10_000, 0.2, 1.0);
        assert_eq!(res1, res2);
    }
}
