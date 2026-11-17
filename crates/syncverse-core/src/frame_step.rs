//! Exact frame-step planning from presented PTS history and stream frame rate.
//!
//! The planner is a pure function: it never touches the decoder or the clock.
//! The runtime keeps a ring of the last [`PTS_HISTORY_LEN`] presented PTS values
//! (milliseconds, ascending) in a [`PtsHistory`] and asks [`plan_step`] what to do.
//!
//! # Rules
//!
//! * **Forward** — present exactly the next decoded frame while paused
//!   ([`StepPlan::PresentNextDecoded`]). The plan also carries a
//!   `fallback_target_ms` (`last + frame duration`) to seek to only when the
//!   decoder has no next frame ready (e.g. right after a seek).
//! * **Backward** — exact-seek to the previous presented frame
//!   ([`StepPlan::SeekExact`]). If the history has no earlier frame, the
//!   target is `current - frame duration`, clamped at 0. At PTS 0 the step is
//!   a [`StepPlan::NoOp`].
//! * **Seek-landing convention** — computed (non-history) targets assume the
//!   runtime lands on the *first frame with PTS >= target*. They are biased by
//!   one millisecond (`current + dur - 1`, `current - dur - 1`) so ms rounding
//!   of 23.976/29.97 fps can never skip or repeat a frame. Targets taken from
//!   the history are exact frame PTS values and are safe under any convention.
//! * **Frame duration** — observed median PTS delta (handles VFR), otherwise
//!   `1000 / fps`, otherwise [`FALLBACK_FRAME_MS`] (40 ms) when nothing is known.
//! * **Watch party** — stepping is **disabled for party clients**
//!   ([`StepError::DisabledInPartyClient`]): a local step would be fought by
//!   the PLL, and a host-side step is a broadcast seek. Hosts and solo
//!   playback may step.

use thiserror::Error;

/// Number of most-recent presented PTS values the runtime should retain.
pub const PTS_HISTORY_LEN: usize = 32;

/// Frame duration used only when neither history nor fps is usable (40 ms = 25 fps).
pub const FALLBACK_FRAME_MS: u64 = 40;

/// HUD text shown when a step is refused inside a watch-party client.
pub const CLIENT_STEP_DISABLED_HUD: &str = "Frame step is disabled while connected to a host";

/// Role of this process in a watch party.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepRole {
    /// Not in a room.
    Solo,
    /// Hosting a room: a step is a broadcast seek.
    Host,
    /// Connected to a host: stepping is refused.
    Client,
}

/// Reasons a step request is refused.
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepError {
    /// Party clients may not step locally (the PLL would fight it).
    #[error("frame step is disabled inside a watch-party client")]
    DisabledInPartyClient,
}

/// What the runtime should do for one step request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepPlan {
    /// Present the next decoded frame; if none is ready, exact-seek to `fallback_target_ms`.
    PresentNextDecoded {
        /// Seek target (ms) when the decoder has no next frame buffered.
        fallback_target_ms: u64,
    },
    /// Exact (frame-accurate) seek to `target_ms`, staying paused.
    SeekExact {
        /// Target presentation time in milliseconds.
        target_ms: u64,
    },
    /// Nothing to do (e.g. stepping back at PTS 0 or forward at the end).
    NoOp,
}

/// Converts a frame rate to a frame duration in ms (rounded, at least 1), or `None` if unusable.
pub fn frame_duration_ms(fps: f64) -> Option<u64> {
    if fps.is_finite() && fps > 0.5 && fps < 1000.0 {
        Some(((1000.0 / fps).round() as u64).max(1))
    } else {
        None
    }
}

/// Median of strictly positive consecutive PTS deltas, or `None` with fewer than two distinct PTS.
fn median_delta(history: &[u64]) -> Option<u64> {
    let mut deltas: Vec<u64> = history
        .windows(2)
        .filter_map(|w| w[1].checked_sub(w[0]).filter(|d| *d > 0))
        .collect();
    if deltas.is_empty() {
        return None;
    }
    deltas.sort_unstable();
    Some(deltas[deltas.len() / 2])
}

/// Best estimate of the current frame duration in ms.
///
/// Preference: observed history (handles VFR and 23.976 drift) → `fps` → 40 ms.
pub fn estimate_frame_ms(pts_history: &[u64], fps: f64) -> u64 {
    median_delta(pts_history)
        .or_else(|| frame_duration_ms(fps))
        .unwrap_or(FALLBACK_FRAME_MS)
}

/// Plans one frame step. See the module docs for the rules.
///
/// `pts_history` is ascending presented PTS in ms; the last element is the
/// frame currently on screen. An empty history is treated as "at PTS 0".
pub fn plan_step(pts_history: &[u64], fps: f64, forward: bool) -> StepPlan {
    let current = pts_history.last().copied().unwrap_or(0);
    let dur = estimate_frame_ms(pts_history, fps);
    if forward {
        return StepPlan::PresentNextDecoded {
            fallback_target_ms: current.saturating_add(dur.saturating_sub(1).max(1)),
        };
    }
    if current == 0 {
        return StepPlan::NoOp;
    }
    let prev = pts_history.iter().rev().find(|p| **p < current).copied();
    // Only trust a history neighbour that is plausibly the adjacent frame.
    let target = match prev {
        Some(p) if current - p <= dur.saturating_mul(4).max(FALLBACK_FRAME_MS) => p,
        _ => current.saturating_sub(dur.saturating_add(1)),
    };
    StepPlan::SeekExact { target_ms: target }
}

/// Role-aware planner: refuses to step in a party client, otherwise defers to [`plan_step`].
///
/// `duration_ms`, when known, clamps forward fallbacks to the media length
/// (a step at the end becomes [`StepPlan::NoOp`]).
pub fn plan_step_for_role(
    role: StepRole,
    pts_history: &[u64],
    fps: f64,
    forward: bool,
    duration_ms: Option<u64>,
) -> Result<StepPlan, StepError> {
    if role == StepRole::Client {
        return Err(StepError::DisabledInPartyClient);
    }
    let plan = plan_step(pts_history, fps, forward);
    Ok(match (plan, duration_ms) {
        (StepPlan::PresentNextDecoded { fallback_target_ms }, Some(d)) => {
            if pts_history.last().copied().unwrap_or(0) >= d {
                StepPlan::NoOp
            } else {
                StepPlan::PresentNextDecoded { fallback_target_ms: fallback_target_ms.min(d) }
            }
        }
        (p, _) => p,
    })
}

/// Ring of the last [`PTS_HISTORY_LEN`] presented PTS values (ms), kept ascending.
#[derive(Debug, Clone, Default)]
pub struct PtsHistory {
    buf: Vec<u64>,
}

impl PtsHistory {
    /// Creates an empty history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a presented frame. A PTS lower than the last one (a seek or
    /// backward step) truncates newer entries so the history stays ascending.
    pub fn push(&mut self, pts_ms: u64) {
        while self.buf.last().is_some_and(|l| *l > pts_ms) {
            self.buf.pop();
        }
        if self.buf.last() != Some(&pts_ms) {
            self.buf.push(pts_ms);
        }
        if self.buf.len() > PTS_HISTORY_LEN {
            self.buf.remove(0);
        }
    }

    /// Clears the history (call after a non-step seek or file change).
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Ascending slice of retained PTS values, suitable for [`plan_step`].
    pub fn as_slice(&self) -> &[u64] {
        &self.buf
    }
}
