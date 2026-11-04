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
            Self::Active { a_ms, b_ms } => format!(
                "A-B Loop: {} ➔ {} (Looping)",
                ControlsBarState::format_timestamp(*a_ms),
                ControlsBarState::format_timestamp(*b_ms)
            ),
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

    /// Clears the A-B repeat state back to `Disabled`.
    pub fn clear(&mut self) {
        *self = Self::Disabled;
    }

    /// Returns `true` if A-B repeat looping is actively enforcing playback bounds.
    #[inline]
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active { .. })
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(state.status_text().contains("Looping"));

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
    }

    #[test]
    fn test_ab_repeat_explicit_setters_and_clear() {
        let mut state = AbRepeatState::Disabled;
        state.set_point_a(5000);
        assert_eq!(state, AbRepeatState::PointASet(5000));

        state.set_point_b(12000);
        assert_eq!(state, AbRepeatState::Active { a_ms: 5000, b_ms: 12000 });

        state.clear();
        assert_eq!(state, AbRepeatState::Disabled);
    }
}
