//! 10 Hz timeline scrubber rate-limiting debouncer.

/// Debounces high-frequency mouse drag events (60-144 Hz) into 10 Hz rate-limited seek triggers.
#[derive(Debug, Clone)]
pub struct ScrubberDebouncer {
    debounce_interval_ms: f64,
    last_seek_time_ms: f64,
    visual_updates: usize,
    intermediate_seeks: Vec<f64>,
}

impl Default for ScrubberDebouncer {
    fn default() -> Self {
        Self::new(100.0) // 100ms interval = 10 Hz
    }
}

impl ScrubberDebouncer {
    /// Constructs a new `ScrubberDebouncer` with the given rate-limiting interval in milliseconds.
    pub fn new(debounce_interval_ms: f64) -> Self {
        Self {
            debounce_interval_ms,
            last_seek_time_ms: -debounce_interval_ms,
            visual_updates: 0,
            intermediate_seeks: Vec::new(),
        }
    }

    /// Handles a high-frequency mouse drag move event.
    ///
    /// Updates the visual thumb immediately and dispatches an intermediate seek
    /// if at least `debounce_interval_ms` has elapsed since the last seek.
    pub fn on_drag_move(&mut self, t_ms: f64) -> Option<f64> {
        self.visual_updates += 1;
        if (t_ms - self.last_seek_time_ms) >= self.debounce_interval_ms {
            self.last_seek_time_ms = t_ms;
            self.intermediate_seeks.push(t_ms);
            Some(t_ms)
        } else {
            None
        }
    }

    /// Handles mouse release (drag complete), immediately emitting the final authoritative seek.
    pub fn on_drag_release(&mut self, t_ms: f64) -> f64 {
        t_ms
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

    #[test]
    fn test_scrubber_debouncing_120hz_drag() {
        let mut debouncer = ScrubberDebouncer::new(100.0);
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

        // Mouse released at t = 505.0ms
        let release_seek = debouncer.on_drag_release(505.0);

        assert_eq!(intermediate_commands.len(), 5);
        assert_eq!(debouncer.visual_updates_count(), 61);
        assert_eq!(release_seek, 505.0);
    }
}
