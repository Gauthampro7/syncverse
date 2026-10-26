//! 10 Hz timeline seek rate-limiting debouncer.
//!
//! Decouples high-frequency mouse drag events (60–144 Hz) into 10 Hz rate-limited seek triggers
//! while ensuring the visual scrub thumb responds with zero latency and mouse release
//! emits an authoritative final seek immediately.

use std::time::Instant;

/// Rate-limiting debouncer for interactive seekbar timeline scrubbing.
///
/// Prevents flooding media demuxing and WAN network peers with high-frequency seek requests
/// during mouse drag-scrubbing, bounding emissions to at most 10 Hz (every >= 100ms).
#[derive(Debug, Clone)]
pub struct SeekDebouncer {
    debounce_interval_ms: f64,
    last_seek_time_ms: f64,
    last_seek_instant: Option<Instant>,
    visual_updates: usize,
    intermediate_seeks: Vec<f64>,
    is_dragging: bool,
}

impl Default for SeekDebouncer {
    fn default() -> Self {
        Self::new(100.0) // 100ms interval = 10 Hz rate limit
    }
}

impl SeekDebouncer {
    /// Constructs a new `SeekDebouncer` with the given rate-limiting interval in milliseconds.
    pub fn new(debounce_interval_ms: f64) -> Self {
        Self {
            debounce_interval_ms,
            last_seek_time_ms: -debounce_interval_ms,
            last_seek_instant: None,
            visual_updates: 0,
            intermediate_seeks: Vec::new(),
            is_dragging: false,
        }
    }

    /// Handles a simulated high-frequency mouse drag move event at timestamp `t_ms`.
    ///
    /// Updates the visual thumb count immediately and dispatches an intermediate seek
    /// if at least `debounce_interval_ms` has elapsed since the last seek.
    pub fn on_drag_move(&mut self, t_ms: f64) -> Option<f64> {
        self.visual_updates += 1;
        self.is_dragging = true;
        if (t_ms - self.last_seek_time_ms) >= self.debounce_interval_ms {
            self.last_seek_time_ms = t_ms;
            self.intermediate_seeks.push(t_ms);
            Some(t_ms)
        } else {
            None
        }
    }

    /// Handles simulated mouse release (drag complete), immediately emitting the final authoritative seek.
    pub fn on_drag_release(&mut self, t_ms: f64) -> f64 {
        self.is_dragging = false;
        t_ms
    }

    /// Handles a real-time mouse drag move event targeting `pts_ms` at wall-clock `now`.
    ///
    /// Dispatches an intermediate seek if at least `debounce_interval_ms` has elapsed
    /// since the last emitted seek instant.
    pub fn on_drag_instant(&mut self, pts_ms: u64, now: Instant) -> Option<u64> {
        self.visual_updates += 1;
        self.is_dragging = true;

        let should_seek = match self.last_seek_instant {
            None => true,
            Some(last) => {
                let elapsed_ms = (now - last).as_secs_f64() * 1000.0;
                elapsed_ms >= self.debounce_interval_ms
            }
        };

        if should_seek {
            self.last_seek_instant = Some(now);
            self.last_seek_time_ms = pts_ms as f64;
            self.intermediate_seeks.push(pts_ms as f64);
            Some(pts_ms)
        } else {
            None
        }
    }

    /// Handles a real-time mouse release event targeting `pts_ms` at wall-clock `now`.
    ///
    /// Immediately resets drag state and emits the authoritative seek PTS.
    pub fn on_release_instant(&mut self, pts_ms: u64, now: Instant) -> u64 {
        self.is_dragging = false;
        self.last_seek_instant = Some(now);
        self.last_seek_time_ms = pts_ms as f64;
        pts_ms
    }

    /// Returns whether a drag operation is currently active.
    pub fn is_dragging(&self) -> bool {
        self.is_dragging
    }

    /// Resets the debouncer state.
    pub fn reset(&mut self) {
        self.last_seek_time_ms = -self.debounce_interval_ms;
        self.last_seek_instant = None;
        self.visual_updates = 0;
        self.intermediate_seeks.clear();
        self.is_dragging = false;
    }

    /// Returns total number of visual thumb updates.
    pub fn visual_updates_count(&self) -> usize {
        self.visual_updates
    }

    /// Returns list of timestamps where intermediate seek commands were emitted.
    pub fn intermediate_seeks(&self) -> &[f64] {
        &self.intermediate_seeks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_seek_debouncer_simulation_120hz_drag() {
        let mut debouncer = SeekDebouncer::new(100.0);
        let total_drag_ms = 500.0;
        let event_interval_ms = 1000.0 / 120.0; // 8.333ms

        let mut intermediate_commands = Vec::new();
        let mut t = 0.0;
        while t <= total_drag_ms {
            if let Some(seek_t) = debouncer.on_drag_move(t) {
                intermediate_commands.push(seek_t);
            }
            t += event_interval_ms;
        }

        let release_seek = debouncer.on_drag_release(505.0);

        assert_eq!(intermediate_commands.len(), 5);
        assert_eq!(debouncer.visual_updates_count(), 61);
        assert_eq!(release_seek, 505.0);
    }

    #[test]
    fn test_seek_debouncer_instant_rate_limiting() {
        let mut debouncer = SeekDebouncer::new(100.0);
        let t0 = Instant::now();

        // First seek emits immediately
        let s1 = debouncer.on_drag_instant(1000, t0);
        assert_eq!(s1, Some(1000));

        // Rapid seeks within 100ms are suppressed
        let s2 = debouncer.on_drag_instant(1500, t0 + Duration::from_millis(30));
        assert_eq!(s2, None);
        let s3 = debouncer.on_drag_instant(2000, t0 + Duration::from_millis(80));
        assert_eq!(s3, None);

        // Seek after 100ms emits
        let s4 = debouncer.on_drag_instant(2500, t0 + Duration::from_millis(105));
        assert_eq!(s4, Some(2500));

        // Release emits authoritative final seek immediately regardless of elapsed interval
        let s_final = debouncer.on_release_instant(3000, t0 + Duration::from_millis(120));
        assert_eq!(s_final, 3000);
    }
}
