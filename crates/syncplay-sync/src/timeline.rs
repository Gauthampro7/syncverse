//! Deterministic reference timeline mapping room master clock to local media presentation.

/// Monotonic room state broadcast by the session leader or sync server.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomState {
    /// Server monotonic anchor timestamp in nanoseconds.
    pub t_ref_server_nanos: u64,
    /// Reference media presentation timestamp in milliseconds.
    pub tau_ref_media_ms: u64,
    /// Commanded target playback speed ratio (e.g. 1.0).
    pub r_target: f64,
    /// Whether media playback is currently paused.
    pub is_paused: bool,
    /// Monotonic sequence number for discarding out-of-order state updates.
    pub seq: u64,
}

impl RoomState {
    /// Evaluates the target media presentation timestamp at local monotonic instant `t_local_nanos`.
    ///
    /// Formula:
    /// `tau_target(t_local) = tau_ref_media + r_target * [(t_local + theta_hat) - t_ref_server]`
    pub fn compute_target_media_pts(&self, t_local_nanos: u64, theta_hat_nanos: f64) -> u64 {
        if self.is_paused {
            return self.tau_ref_media_ms;
        }

        let estimated_server_nanos = (t_local_nanos as f64) + theta_hat_nanos;
        let elapsed_server_nanos = estimated_server_nanos - (self.t_ref_server_nanos as f64);
        let elapsed_server_ms = elapsed_server_nanos / 1_000_000.0;
        let target_pts_ms = (self.tau_ref_media_ms as f64) + self.r_target * elapsed_server_ms;

        if target_pts_ms < 0.0 {
            0
        } else {
            target_pts_ms as u64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reference_timeline_evaluation() {
        let room = RoomState {
            t_ref_server_nanos: 5_000_000_000, // 5.0s
            tau_ref_media_ms: 60_000,          // 60.0s
            r_target: 1.0,
            is_paused: false,
            seq: 1,
        };

        // Local time: 6.8s, theta_hat = +200ms (+200,000,000 ns)
        let t_local = 6_800_000_000;
        let theta_hat = 200_000_000.0;
        let target_pts = room.compute_target_media_pts(t_local, theta_hat);
        assert_eq!(target_pts, 62_000); // 60,000 + 2000 = 62,000 ms
    }
}
