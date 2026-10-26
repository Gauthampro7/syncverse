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

    /// Evaluates current phase drift and calculates the speed correction command.
    ///
    /// # Arguments
    /// * `target_pts_ms` - Target media presentation timestamp from room reference timeline.
    /// * `actual_pts_ms` - Actual rendered media presentation timestamp from local clock.
    /// * `dt_secs` - Evaluation sample interval in seconds (nominally 0.2s).
    pub fn update(&mut self, target_pts_ms: u64, actual_pts_ms: u64, dt_secs: f64) -> PllCorrection {
        let phase_error_ms = (target_pts_ms as f64) - (actual_pts_ms as f64);
        let e = phase_error_ms / 1000.0; // convert ms to seconds

        if e.abs() < self.deadband_secs {
            // Zone 1: Deadband In-Sync (|e| < 15ms)
            // Integral accumulator held constant to prevent hunting oscillations
            PllCorrection {
                speed_multiplier: 1.0,
                hard_seek_target: None,
                phase_error_ms,
                zone: PllZone::Deadband,
            }
        } else if e.abs() <= self.hard_seek_threshold_secs {
            // Zone 2: Discrete PI controller (15ms <= |e| <= 1500ms)
            let i_candidate = self.integral + e * dt_secs;
            self.integral = i_candidate.clamp(-self.i_max, self.i_max);

            let delta_r = self.kp * e + self.ki * self.integral;
            let speed = (1.0 + delta_r).clamp(self.r_min, self.r_max);

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
                speed_multiplier: 1.0,
                hard_seek_target: Some(target_pts_ms),
                phase_error_ms,
                zone: PllZone::HardSeek,
            }
        }
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
}
