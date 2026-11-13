//! Master monotonic clock, continuous PTS re-anchoring via SeqLock, and presentation scheduling.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Operational playback state of the media player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    /// Actively presenting media at the current speed rate.
    Playing,
    /// Playback paused at the current presentation time.
    Paused,
    /// Discontinuous hard seek in progress.
    Seeking,
    /// Pre-roll decode or network buffer fill in progress.
    Buffering,
}

/// Operational playback state of the clock (alias for `PlaybackState`).
pub type ClockState = PlaybackState;

/// Action to be taken by the presentation scheduler for a candidate video frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationAction {
    /// Frame is early (delta > +10ms); wait for the specified duration before displaying.
    Wait(Duration),
    /// Frame is on-time (-15ms <= delta <= +10ms); submit immediately to the compositor.
    PresentNow,
    /// Frame is slightly late (-100ms < delta < -15ms); render immediately and record metric.
    PresentNowLate,
    /// Frame is severely late (delta <= -100ms); drop immediately without rendering.
    DropLateFrame,
}

struct ClockStateSnapshot {
    anchor_pts: f64,
    anchor_wall: Instant,
    speed: f64,
    state: PlaybackState,
}

/// Master Monotonic Clock providing lock-free optimistic snapshot reads via SeqLock.
///
/// Guarantees continuous $C^0$ presentation timestamp progression during rate shifts:
/// `lim_{e -> 0} |CurrentPTS(t + e) - CurrentPTS(t - e)| = 0`.
pub struct MasterClock {
    seq: AtomicUsize,
    writer_lock: Mutex<()>,
    state: std::cell::UnsafeCell<ClockStateSnapshot>,
    seek_generation: AtomicU64,
    audio_latency_ns: AtomicU64,
}

// SAFETY: Synchronization is guaranteed by the SeqLock protocol: readers only observe
// complete, consistent snapshots when `seq` is even and unchanged; writers are serialized by `writer_lock`.
unsafe impl Send for MasterClock {}
unsafe impl Sync for MasterClock {}

impl MasterClock {
    /// Creates a new `MasterClock` anchored at the specified initial PTS and instant.
    pub fn new(initial_pts_ms: f64, initial_speed: f64, initial_state: PlaybackState) -> Self {
        Self {
            seq: AtomicUsize::new(0),
            writer_lock: Mutex::new(()),
            state: std::cell::UnsafeCell::new(ClockStateSnapshot {
                anchor_pts: initial_pts_ms,
                anchor_wall: Instant::now(),
                speed: initial_speed,
                state: initial_state,
            }),
            seek_generation: AtomicU64::new(0),
            audio_latency_ns: AtomicU64::new(0),
        }
    }

    /// Returns the monotonically increasing seek/epoch generation counter.
    ///
    /// Video and audio consumers compare frame generations against this epoch to
    /// discard stale pre-seek frames immediately without lock contention.
    #[inline]
    pub fn seek_generation(&self) -> u64 {
        self.seek_generation.load(Ordering::Acquire)
    }

    /// Atomically increments the seek generation epoch and returns the new value.
    #[inline]
    pub fn bump_seek_generation(&self) -> u64 {
        self.seek_generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Updates the reported hardware audio output latency in milliseconds.
    ///
    /// Video presentation pacing subtracts this latency so visual frames align
    /// with when audio samples actually reach the physical output device.
    #[inline]
    pub fn set_audio_latency_ms(&self, latency_ms: u64) {
        self.audio_latency_ns
            .store(latency_ms.saturating_mul(1_000_000), Ordering::Release);
    }

    /// Returns the currently configured hardware audio output latency in milliseconds.
    #[inline]
    pub fn audio_latency_ms(&self) -> u64 {
        self.audio_latency_ns.load(Ordering::Acquire) / 1_000_000
    }

    /// Returns the current media presentation timestamp in whole milliseconds at `now`.
    #[inline]
    pub fn current_pts_ms(&self, now: Instant) -> u64 {
        self.current_pts(now).max(0.0).round() as u64
    }

    /// Returns the latency-compensated video presentation target timestamp in milliseconds at `now`.
    ///
    /// Subtracts the active hardware audio output buffer latency from `current_pts_ms(now)`
    /// so video frames wait for queued audio to drain to the speaker.
    #[inline]
    pub fn effective_video_pts_ms(&self, now: Instant) -> u64 {
        self.current_pts_ms(now)
            .saturating_sub(self.audio_latency_ms())
    }

    /// Returns the current media presentation timestamp in milliseconds at wall instant `now`.
    pub fn current_pts(&self, now: Instant) -> f64 {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 & 1 != 0 {
                // Writer in progress, spin and retry
                std::hint::spin_loop();
                continue;
            }

            // SAFETY: s1 was observed even, read fields optimistically
            let (anchor_pts, anchor_wall, speed, state) = unsafe {
                let snap = &*self.state.get();
                (snap.anchor_pts, snap.anchor_wall, snap.speed, snap.state)
            };

            let s2 = self.seq.load(Ordering::Acquire);
            if s1 == s2 && (s2 & 1 == 0) {
                // Clean, torn-free snapshot acquired
                return match state {
                    PlaybackState::Playing => {
                        let elapsed_secs = if now >= anchor_wall {
                            now.duration_since(anchor_wall).as_secs_f64()
                        } else {
                            0.0
                        };
                        anchor_pts + elapsed_secs * speed * 1000.0
                    }
                    _ => anchor_pts,
                };
            }
            std::hint::spin_loop();
        }
    }

    /// Returns the current playback state of the clock.
    pub fn state(&self) -> ClockState {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }

            // SAFETY: s1 was observed even, read state optimistically.
            let state = unsafe { (*self.state.get()).state };

            let s2 = self.seq.load(Ordering::Acquire);
            if s1 == s2 && (s2 & 1 == 0) {
                return state;
            }
            std::hint::spin_loop();
        }
    }

    /// Returns whether the clock is currently in the `PlaybackState::Playing` state.
    pub fn is_playing(&self) -> bool {
        self.state() == PlaybackState::Playing
    }

    /// Returns the current playback speed ratio.
    pub fn speed(&self) -> f64 {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }

            // SAFETY: s1 was observed even, read speed optimistically.
            let speed = unsafe { (*self.state.get()).speed };

            let s2 = self.seq.load(Ordering::Acquire);
            if s1 == s2 && (s2 & 1 == 0) {
                return speed;
            }
            std::hint::spin_loop();
        }
    }

    /// Re-anchors the timeline to a new playback speed at wall instant `now`.
    ///
    /// Preserves mathematical $C^0$ continuity with zero discontinuous jumps.
    pub fn reanchor(&self, now: Instant, new_speed: f64) {
        let _guard = self.writer_lock.lock().unwrap();

        // 1. Compute exact current PTS at transition instant
        let curr_pts = self.current_pts(now);

        // 2. Begin atomic SeqLock publication (odd sequence)
        let s = self.seq.load(Ordering::Relaxed);
        self.seq.store(s.wrapping_add(1), Ordering::Release);

        // SAFETY: Writer holds writer_lock, has exclusive write access
        unsafe {
            let snap = &mut *self.state.get();
            snap.anchor_pts = curr_pts;
            snap.anchor_wall = now;
            snap.speed = new_speed;
        }

        // 3. Complete atomic publication (even sequence)
        self.seq.store(s.wrapping_add(2), Ordering::Release);
    }

    /// Updates playback state (e.g. Pause, Play, Seek).
    pub fn set_state(&self, now: Instant, new_state: PlaybackState) {
        let _guard = self.writer_lock.lock().unwrap();
        let curr_pts = self.current_pts(now);

        let s = self.seq.load(Ordering::Relaxed);
        self.seq.store(s.wrapping_add(1), Ordering::Release);

        // SAFETY: Writer holds writer_lock, ensuring exclusive mutable access to the internal state.
        unsafe {
            let snap = &mut *self.state.get();
            snap.anchor_pts = curr_pts;
            snap.anchor_wall = now;
            snap.state = new_state;
        }

        self.seq.store(s.wrapping_add(2), Ordering::Release);
    }

    /// Hard seeks presentation to a target timestamp and increments the seek generation epoch.
    pub fn seek_to(&self, target_pts_ms: f64, now: Instant) {
        let _guard = self.writer_lock.lock().unwrap();
        let s = self.seq.load(Ordering::Relaxed);
        self.seq.store(s.wrapping_add(1), Ordering::Release);

        // SAFETY: Writer holds writer_lock, ensuring exclusive mutable access to the internal state.
        unsafe {
            let snap = &mut *self.state.get();
            snap.anchor_pts = target_pts_ms;
            snap.anchor_wall = now;
        }

        self.seek_generation.fetch_add(1, Ordering::AcqRel);
        self.seq.store(s.wrapping_add(2), Ordering::Release);
    }

    /// Evaluates the presentation scheduling action for a frame with timestamp `frame_pts_ms`.
    ///
    /// Accounts for the configured hardware audio output latency (`audio_latency_ms`) so
    /// visual frame presentation tracks when audio actually emerges from the output device.
    ///
    /// Decision Window:
    /// - Early (Delta > +10ms): Wait(Duration)
    /// - On-Time (-15ms <= Delta <= +10ms): PresentNow
    /// - Slightly Late (-100ms < Delta < -15ms): PresentNowLate
    /// - Severely Late (Delta <= -100ms): DropLateFrame
    pub fn evaluate_presentation_timing(
        &self,
        frame_pts_ms: u64,
        now: Instant,
    ) -> PresentationAction {
        let audio_latency_ms = (self.audio_latency_ns.load(Ordering::Acquire) as f64) / 1_000_000.0;
        let master_pts = self.current_pts(now) - audio_latency_ms;
        let delta_ms = (frame_pts_ms as f64) - master_pts;

        if delta_ms > 10.0 {
            let wait_ms = (delta_ms - 10.0).max(0.0);
            PresentationAction::Wait(Duration::from_secs_f64(wait_ms / 1000.0))
        } else if delta_ms >= -15.0 {
            PresentationAction::PresentNow
        } else if delta_ms > -100.0 {
            PresentationAction::PresentNowLate
        } else {
            PresentationAction::DropLateFrame
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_master_clock_c0_continuity() {
        let clock = MasterClock::new(10000.0, 1.0, PlaybackState::Playing);
        let t0 = Instant::now();

        let pts_before = clock.current_pts(t0);
        assert!((pts_before - 10000.0).abs() < 1e-3);

        // Reanchor speed to 1.04x
        clock.reanchor(t0, 1.04);
        let pts_after = clock.current_pts(t0);
        // C0 continuity: zero discontinuous jump at t0
        assert!(
            (pts_after - pts_before).abs() < 1e-4,
            "Reanchoring must maintain C0 continuity"
        );
    }

    #[test]
    fn test_presentation_decision_window() {
        let clock = MasterClock::new(20000.0, 1.0, PlaybackState::Paused);
        let now = Instant::now();

        // Frame A: PTS = 20,050ms (Delta = +50ms -> Wait(40ms))
        match clock.evaluate_presentation_timing(20050, now) {
            PresentationAction::Wait(d) => {
                let wait_ms = d.as_secs_f64() * 1000.0;
                assert!((wait_ms - 40.0).abs() < 1e-2);
            }
            other => panic!("Expected Wait, got {:?}", other),
        }

        // Frame B: PTS = 19,995ms (Delta = -5ms -> PresentNow)
        assert_eq!(
            clock.evaluate_presentation_timing(19995, now),
            PresentationAction::PresentNow
        );

        // Frame C: PTS = 19,950ms (Delta = -50ms -> PresentNowLate)
        assert_eq!(
            clock.evaluate_presentation_timing(19950, now),
            PresentationAction::PresentNowLate
        );

        // Frame D: PTS = 19,850ms (Delta = -150ms -> DropLateFrame)
        assert_eq!(
            clock.evaluate_presentation_timing(19850, now),
            PresentationAction::DropLateFrame
        );
    }

    #[test]
    fn test_master_clock_state_and_speed_getters() {
        let clock = MasterClock::new(0.0, 1.0, PlaybackState::Paused);
        assert_eq!(clock.state(), PlaybackState::Paused);
        assert!(!clock.is_playing());
        assert!((clock.speed() - 1.0).abs() < 1e-6);

        let now = Instant::now();
        clock.set_state(now, PlaybackState::Playing);
        assert_eq!(clock.state(), PlaybackState::Playing);
        assert!(clock.is_playing());

        clock.reanchor(now, 1.04);
        assert!((clock.speed() - 1.04).abs() < 1e-6);
    }

    #[test]
    fn test_seek_generation_and_audio_latency_compensation() {
        let clock = MasterClock::new(1000.0, 1.0, PlaybackState::Paused);
        let now = Instant::now();

        assert_eq!(clock.seek_generation(), 0);
        assert_eq!(clock.audio_latency_ms(), 0);
        assert_eq!(clock.current_pts_ms(now), 1000);
        assert_eq!(clock.effective_video_pts_ms(now), 1000);

        // Explicit bump
        assert_eq!(clock.bump_seek_generation(), 1);
        assert_eq!(clock.seek_generation(), 1);

        // seek_to also increments seek_generation
        clock.seek_to(5000.0, now);
        assert_eq!(clock.seek_generation(), 2);
        assert_eq!(clock.current_pts_ms(now), 5000);

        // Set 45ms hardware audio latency -> effective video PTS lags master by 45ms
        clock.set_audio_latency_ms(45);
        assert_eq!(clock.audio_latency_ms(), 45);
        assert_eq!(clock.effective_video_pts_ms(now), 4955);

        // Frame at 5000ms is 45ms ahead of latency-compensated video clock (4955ms) -> Wait(35ms)
        match clock.evaluate_presentation_timing(5000, now) {
            PresentationAction::Wait(d) => {
                let wait_ms = d.as_secs_f64() * 1000.0;
                assert!((wait_ms - 35.0).abs() < 1e-2);
            }
            other => panic!("Expected Wait(35ms), got {:?}", other),
        }

        // Saturating sub when current_pts < audio_latency_ms
        clock.seek_to(20.0, now);
        assert_eq!(clock.effective_video_pts_ms(now), 0);
    }
}
