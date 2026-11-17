//! A-B repeat playback looping state machine, markers, and segment management.

use crate::gui::controls::ControlsBarState;

/// State of A-B repeat playback looping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AbRepeatState {
    /// A-B looping is disabled (normal linear playback).
    #[default]
    Disabled,
    /// Point A has been marked at presentation timestamp PTS in milliseconds; awaiting Point B.
    PointASet(u64),
    /// Both Point A and Point B are marked; presentation loops continuously within [a_ms, b_ms].
    Active {
        /// Start of looping segment in milliseconds.
        a_ms: u64,
        /// End of looping segment in milliseconds.
        b_ms: u64,
    },
}

impl AbRepeatState {
    /// Human-readable status label for HUD notifications, OSD banners, and tooltips.
    pub fn status_text(&self) -> String {
        match self {
            Self::Disabled => "A-B Loop: Off".to_string(),
            Self::PointASet(a) => format!("A-B Loop: Set A ({})", ControlsBarState::format_timestamp(*a)),
            Self::Active { a_ms, b_ms } => {
                let dur_s = (*b_ms as f64 - *a_ms as f64) / 1000.0;
                format!(
                    "A-B Loop: {} ➔ {} ({:.1}s)",
                    ControlsBarState::format_timestamp(*a_ms),
                    ControlsBarState::format_timestamp(*b_ms),
                    dur_s
                )
            }
        }
    }

    /// Advances the A-B repeat state machine cyclically at `current_pts_ms`.
    ///
    /// State transitions:
    /// - `Disabled` ➔ `PointASet(current_pts_ms)`
    /// - `PointASet(a)` ➔ `Active { a_ms: min(a, current), b_ms: max(a, current) }`
    /// - `Active { .. }` ➔ `Disabled`
    pub fn cycle(&self, current_pts_ms: u64) -> Self {
        match self {
            Self::Disabled => Self::PointASet(current_pts_ms),
            Self::PointASet(a) => {
                let a_val = *a;
                if current_pts_ms > a_val + 100 {
                    Self::Active {
                        a_ms: a_val,
                        b_ms: current_pts_ms,
                    }
                } else if a_val > current_pts_ms + 100 {
                    Self::Active {
                        a_ms: current_pts_ms,
                        b_ms: a_val,
                    }
                } else {
                    // Current PTS is identical or too close to Point A; advance Point B by 1.0 second
                    Self::Active {
                        a_ms: a_val,
                        b_ms: a_val + 1000,
                    }
                }
            }
            Self::Active { .. } => Self::Disabled,
        }
    }

    /// Sets Point A explicitly at the given presentation timestamp in milliseconds.
    pub fn set_point_a(&mut self, pts_ms: u64) {
        *self = match *self {
            Self::Active { b_ms, .. } if b_ms > pts_ms + 100 => Self::Active {
                a_ms: pts_ms,
                b_ms,
            },
            _ => Self::PointASet(pts_ms),
        };
    }

    /// Sets Point B explicitly at the given presentation timestamp in milliseconds.
    pub fn set_point_b(&mut self, pts_ms: u64) {
        *self = match *self {
            Self::PointASet(a_ms) | Self::Active { a_ms, .. } => {
                if pts_ms > a_ms + 100 {
                    Self::Active {
                        a_ms,
                        b_ms: pts_ms,
                    }
                } else if a_ms > pts_ms + 100 {
                    Self::Active {
                        a_ms: pts_ms,
                        b_ms: a_ms,
                    }
                } else {
                    Self::Active {
                        a_ms,
                        b_ms: a_ms + 1000,
                    }
                }
            }
            Self::Disabled => Self::PointASet(pts_ms),
        };
    }

    /// Nudges Point A by `delta_ms` milliseconds, ensuring bounds validity.
    pub fn nudge_point_a(&mut self, delta_ms: i64) {
        if let Self::Active { a_ms, b_ms } = *self {
            let new_a = (a_ms as i64 + delta_ms).max(0) as u64;
            if new_a + 100 < b_ms {
                *self = Self::Active { a_ms: new_a, b_ms };
            }
        } else if let Self::PointASet(a) = *self {
            let new_a = (a as i64 + delta_ms).max(0) as u64;
            *self = Self::PointASet(new_a);
        }
    }

    /// Nudges Point B by `delta_ms` milliseconds, ensuring bounds validity.
    pub fn nudge_point_b(&mut self, delta_ms: i64, max_duration_ms: u64) {
        if let Self::Active { a_ms, b_ms } = *self {
            let mut new_b = (b_ms as i64 + delta_ms).max(0) as u64;
            if max_duration_ms > 0 {
                new_b = new_b.min(max_duration_ms);
            }
            if new_b > a_ms + 100 {
                *self = Self::Active { a_ms, b_ms: new_b };
            }
        }
    }

    /// Clears the A-B repeat state back to `Disabled`.
    pub fn clear(&mut self) {
        *self = Self::Disabled;
    }

    /// Returns `true` if A-B repeat looping is actively enforcing playback bounds.
    #[inline]
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active { .. })
    }

    /// Returns duration of active A-B loop segment in milliseconds, or `None` if inactive.
    pub fn duration_ms(&self) -> Option<u64> {
        match self {
            Self::Active { a_ms, b_ms } => Some(b_ms.saturating_sub(*a_ms)),
            _ => None,
        }
    }

    /// Returns Point A in milliseconds if set.
    pub fn point_a(&self) -> Option<u64> {
        match self {
            Self::PointASet(a) => Some(*a),
            Self::Active { a_ms, .. } => Some(*a_ms),
            Self::Disabled => None,
        }
    }

    /// Returns Point B in milliseconds if set and active.
    pub fn point_b(&self) -> Option<u64> {
        match self {
            Self::Active { b_ms, .. } => Some(*b_ms),
            _ => None,
        }
    }

    /// Evaluates whether the given `current_pts_ms` has reached or exceeded Point B,
    /// returning `Some(target_a_ms)` to seek back to if a loop restart is required.
    pub fn evaluate_loop(&self, current_pts_ms: u64) -> Option<u64> {
        if let Self::Active { a_ms, b_ms } = *self {
            if current_pts_ms >= b_ms {
                return Some(a_ms);
            }
        }
        None
    }
}

/// Role of the local peer with respect to a watch-party room (for A-B loop gating).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopRoomRole {
    /// Not in a room.
    Solo,
    /// Hosting a room.
    Host,
    /// Connected to a host.
    Client,
}

/// What the A-B loop is allowed to do in the current room role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbLoopPolicy {
    /// Loop seeks are purely local.
    Local,
    /// Loop seeks become room-wide broadcast seeks (everyone loops with the host).
    HostBroadcast,
    /// Loop is suppressed: a client seek is forwarded to the host (`PeerControlAction::Seek`) and
    /// would yank the whole room, while the PLL fights any purely local jump.
    Disabled,
}

/// HUD text shown when a non-host tries to start or run an A-B loop inside a room.
pub const AB_LOOP_CLIENT_DISABLED_HUD: &str = "A-B repeat is host-only while in a watch party";

/// Decides the A-B loop policy for `role`.
///
/// Behaviour recorded from `playback_runtime.rs` (`PlayerCommand::Seek` arm): the loop seek is a
/// normal `Seek`. As Host it calls `set_media_state` + `broadcast_room_state` (the whole room
/// follows, so the loop works but re-broadcasts on every iteration); as Client it sends
/// `PeerControlAction::Seek` to the host, so one viewer's loop would move everybody. Hence
/// clients are gated.
pub fn ab_loop_policy(role: LoopRoomRole) -> AbLoopPolicy {
    match role {
        LoopRoomRole::Solo => AbLoopPolicy::Local,
        LoopRoomRole::Host => AbLoopPolicy::HostBroadcast,
        LoopRoomRole::Client => AbLoopPolicy::Disabled,
    }
}

impl AbRepeatState {
    /// Role-aware loop evaluation: like [`AbRepeatState::evaluate_loop`] but returns `None`
    /// whenever `role` is gated by [`ab_loop_policy`].
    pub fn evaluate_loop_for_role(&self, current_pts_ms: u64, role: LoopRoomRole) -> Option<u64> {
        if ab_loop_policy(role) == AbLoopPolicy::Disabled {
            return None;
        }
        self.evaluate_loop(current_pts_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ab_loop_room_policy_decision() {
        let a = AbRepeatState::Active { a_ms: 1000, b_ms: 2000 };
        assert_eq!(a.evaluate_loop_for_role(2500, LoopRoomRole::Solo), Some(1000));
        assert_eq!(a.evaluate_loop_for_role(2500, LoopRoomRole::Host), Some(1000));
        assert_eq!(a.evaluate_loop_for_role(2500, LoopRoomRole::Client), None);
        assert_eq!(ab_loop_policy(LoopRoomRole::Client), AbLoopPolicy::Disabled);
    }

    #[test]
    fn test_ab_repeat_lifecycle_and_cycling() {
        let mut state = AbRepeatState::default();
        assert_eq!(state, AbRepeatState::Disabled);
        assert!(!state.is_active());
        assert_eq!(state.status_text(), "A-B Loop: Off");
        assert_eq!(state.point_a(), None);
        assert_eq!(state.point_b(), None);

        // Press 1: Set Point A at 10,000 ms
        state = state.cycle(10_000);
        assert_eq!(state, AbRepeatState::PointASet(10_000));
        assert!(!state.is_active());
        assert_eq!(state.point_a(), Some(10_000));
        assert_eq!(state.point_b(), None);
        assert!(state.status_text().contains("Set A"));

        // Press 2: Set Point B at 25,000 ms -> Active loop
        state = state.cycle(25_000);
        assert_eq!(
            state,
            AbRepeatState::Active {
                a_ms: 10_000,
                b_ms: 25_000,
            }
        );
        assert!(state.is_active());
        assert_eq!(state.point_a(), Some(10_000));
        assert_eq!(state.point_b(), Some(25_000));
        assert_eq!(state.duration_ms(), Some(15_000));
        assert!(state.status_text().contains("15.0s"));

        // Evaluate loop:
        // Before B: No seek
        assert_eq!(state.evaluate_loop(15_000), None);
        // At or past B: Seek to A
        assert_eq!(state.evaluate_loop(25_000), Some(10_000));
        assert_eq!(state.evaluate_loop(26_000), Some(10_000));

        // Press 3: Reset -> Disabled
        state = state.cycle(18_000);
        assert_eq!(state, AbRepeatState::Disabled);
        assert!(!state.is_active());
        assert_eq!(state.duration_ms(), None);
    }

    #[test]
    fn test_ab_repeat_explicit_setters_and_clear() {
        let mut state = AbRepeatState::Disabled;
        state.set_point_a(5000);
        assert_eq!(state, AbRepeatState::PointASet(5000));

        state.set_point_b(12000);
        assert_eq!(state, AbRepeatState::Active { a_ms: 5000, b_ms: 12000 });
        assert_eq!(state.duration_ms(), Some(7000));

        state.clear();
        assert_eq!(state, AbRepeatState::Disabled);
        assert_eq!(state.duration_ms(), None);
    }

    #[test]
    fn test_ab_repeat_duration_and_nudging() {
        let mut state = AbRepeatState::Active { a_ms: 2000, b_ms: 8000 };
        assert_eq!(state.duration_ms(), Some(6000));

        // Nudge Point A forward by 500ms -> [2500, 8000]
        state.nudge_point_a(500);
        assert_eq!(state.point_a(), Some(2500));
        assert_eq!(state.duration_ms(), Some(5500));

        // Nudge Point A backward by 1000ms -> [1500, 8000]
        state.nudge_point_a(-1000);
        assert_eq!(state.point_a(), Some(1500));

        // Nudge Point B forward by 1000ms with max limit 10000 -> [1500, 9000]
        state.nudge_point_b(1000, 10000);
        assert_eq!(state.point_b(), Some(9000));

        // Nudge Point B past max limit clamps to 10000
        state.nudge_point_b(3000, 10000);
        assert_eq!(state.point_b(), Some(10000));
    }
}
