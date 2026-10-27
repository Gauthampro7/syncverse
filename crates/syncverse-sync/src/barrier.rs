//! Two-phase scheduled barrier unpause state machine.

/// Barrier synchronization phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarrierPhase {
    /// Normal playback or stationary pause.
    Idle,
    /// Phase 1: Server broadcast unpause preparation with scheduled start time.
    Preparing {
        /// Target unpause time in server clock milliseconds.
        target_server_ms: u64,
        /// Media presentation timestamp where playback will unpause.
        tau_unpause_ms: u64,
    },
    /// Phase 2: Client completed pre-roll decode and reported ready.
    Ready {
        /// Client identifier.
        client_id: u64,
    },
    /// Phase 3: Server broadcast commit confirmation; clients awaiting scheduled start instant.
    Committed {
        /// Absolute server timestamp when presentation resumes.
        start_time_server_ms: u64,
    },
    /// Barrier unpause completed; playback actively progressing.
    Active,
}

/// Computes the authoritative future unpause target server timestamp.
///
/// Formula:
/// `T_target_server = t_server_now + 1.5 * max(RTT_i) + t_preroll_budget + 100ms`
pub fn compute_target_unpause_server(
    t_server_now_ms: u64,
    max_rtt_ms: u64,
    preroll_budget_ms: u64,
) -> u64 {
    let rtt_factor = (1.5 * (max_rtt_ms as f64)) as u64;
    t_server_now_ms + rtt_factor + preroll_budget_ms + 100
}

/// State machine coordinating the two-phase barrier unpause sequence.
#[derive(Debug, Clone)]
pub struct BarrierStateMachine {
    phase: BarrierPhase,
    preroll_budget_ms: u64,
}

impl Default for BarrierStateMachine {
    fn default() -> Self {
        Self::new(200) // 200ms pre-roll budget
    }
}

impl BarrierStateMachine {
    /// Constructs a new `BarrierStateMachine`.
    pub fn new(preroll_budget_ms: u64) -> Self {
        Self {
            phase: BarrierPhase::Idle,
            preroll_budget_ms,
        }
    }

    /// Initiates an unpause sequence on the server.
    pub fn prepare_unpause(
        &mut self,
        t_server_now_ms: u64,
        max_rtt_ms: u64,
        tau_unpause_ms: u64,
    ) -> u64 {
        let target_server_ms =
            compute_target_unpause_server(t_server_now_ms, max_rtt_ms, self.preroll_budget_ms);
        self.phase = BarrierPhase::Preparing {
            target_server_ms,
            tau_unpause_ms,
        };
        target_server_ms
    }

    /// Transitions client to ready state upon completing pre-roll decoding.
    pub fn client_ready(&mut self, client_id: u64) {
        self.phase = BarrierPhase::Ready { client_id };
    }

    /// Commits the unpause barrier on the server when all peers are ready.
    pub fn commit(&mut self, start_time_server_ms: u64) {
        self.phase = BarrierPhase::Committed {
            start_time_server_ms,
        };
    }

    /// Returns current barrier phase.
    pub fn phase(&self) -> BarrierPhase {
        self.phase
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_barrier_unpause_timing_and_margins() {
        let max_rtt = 400u64;
        let preroll_budget = 200u64;
        let t_server_now = 0u64;

        // Target: 0 + 1.5 * 400 + 200 + 100 = 900ms
        let target_server = compute_target_unpause_server(t_server_now, max_rtt, preroll_budget);
        assert_eq!(target_server, 900);

        // Verify across various client pre-roll durations (0 to 200ms)
        for preroll in [0, 50, 100, 150, 200] {
            let t_client_ready_sent = 200 + preroll;
            let t_server_receives_ready = t_client_ready_sent + 200;
            let t_client_receives_commit = t_server_receives_ready + 200;

            let margin = (target_server as i64) - (t_client_receives_commit as i64);
            assert!(
                margin >= 100,
                "Commit arrival margin must be >= 100ms, got {}",
                margin
            );
        }
    }
}
