//! Audio clock pacing and zero-allocation buffer underrun protection.
//!
//! Synchronizes audio presentation with `MasterClock` monotonic time and protects against
//! buffer underrun thread stalls by transferring pre-allocated silence frames over the SPSC ring buffer.

#![deny(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use syncplay_codecs::AudioFramePCM;

use crate::error::{BufferFullError, ClockError};
use crate::master_clock::MasterClock;
use crate::spsc::SpscRingBuffer;

/// Action evaluated by `AudioPacer` for scheduling an audio frame against `MasterClock`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioTimingAction {
    /// Frame is on time with master presentation clock (|delta| <= 20ms).
    PlayNow,
    /// Frame is early (delta > 20ms); caller should wait or buffer before presenting.
    Wait(Duration),
    /// Frame is late (delta < -20ms); caller should render immediately or skip if severely late.
    Late(Duration),
}

/// Statistics and health metrics tracked by `AudioPacer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioPacerStats {
    /// Number of valid non-silence frames successfully processed.
    pub valid_frames: usize,
    /// Number of buffer underrun occurrences detected.
    pub underruns: usize,
    /// Number of silence frames injected to prevent audio thread starvation.
    pub silence_frames_injected: usize,
}

/// Pacer coordinating audio presentation with `MasterClock` and protecting against buffer underruns.
///
/// Features:
/// - Paces audio frames against `MasterClock` with tight timing classification (`AudioTimingAction`).
/// - Pre-allocated silence scratch buffer transferred over `SpscRingBuffer<AudioFramePCM, 64>` to eliminate
///   audio thread stalls and zero pop/glitch on underrun.
/// - Smooth 2ms linear ramp-in on the first valid frame when recovering from an underrun.
/// - Zero heap allocation during steady-state buffer filling (`fill_samples`).
pub struct AudioPacer {
    sample_rate: u32,
    channels: u16,
    frame_size_samples: usize,
    silence_template: AudioFramePCM,
    underrun_count: AtomicUsize,
    valid_frames_count: AtomicUsize,
    silence_injected_count: AtomicUsize,
    last_pts_ms: AtomicU64,
    in_underrun: AtomicBool,
}

impl AudioPacer {
    /// Creates a new `AudioPacer` with pre-allocated silence scratch buffer.
    pub fn new(
        sample_rate: u32,
        channels: u16,
        frame_size_samples: usize,
    ) -> Result<Self, ClockError> {
        if sample_rate == 0 || channels == 0 || frame_size_samples == 0 {
            return Err(ClockError::AudioPacing(
                "sample_rate, channels, and frame_size_samples must be > 0".into(),
            ));
        }

        let total_samples = frame_size_samples * (channels as usize);
        let silence_samples = vec![0.0f32; total_samples];
        let silence_template = AudioFramePCM::new(sample_rate, channels, 0, silence_samples)
            .map_err(ClockError::Codec)?;

        Ok(Self {
            sample_rate,
            channels,
            frame_size_samples,
            silence_template,
            underrun_count: AtomicUsize::new(0),
            valid_frames_count: AtomicUsize::new(0),
            silence_injected_count: AtomicUsize::new(0),
            last_pts_ms: AtomicU64::new(0),
            in_underrun: AtomicBool::new(false),
        })
    }

    /// Creates a default `AudioPacer` configured for 48 kHz stereo audio with 1024-sample frames.
    pub fn default_48k_stereo() -> Result<Self, ClockError> {
        Self::new(48_000, 2, 1024)
    }

    /// Returns the target audio sample rate in Hz.
    #[inline]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Returns the number of audio channels.
    #[inline]
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Returns the frame size in samples per channel.
    #[inline]
    pub fn frame_size_samples(&self) -> usize {
        self.frame_size_samples
    }

    /// Returns the total number of audio samples per frame across all channels.
    #[inline]
    pub fn total_samples_per_frame(&self) -> usize {
        self.frame_size_samples * (self.channels as usize)
    }

    /// Returns the cumulative pacer performance statistics.
    pub fn stats(&self) -> AudioPacerStats {
        AudioPacerStats {
            valid_frames: self.valid_frames_count.load(Ordering::Relaxed),
            underruns: self.underrun_count.load(Ordering::Relaxed),
            silence_frames_injected: self.silence_injected_count.load(Ordering::Relaxed),
        }
    }

    /// Returns the number of buffer underruns detected.
    #[inline]
    pub fn underruns(&self) -> usize {
        self.underrun_count.load(Ordering::Relaxed)
    }

    /// Returns the number of valid non-silence frames processed.
    #[inline]
    pub fn valid_frames(&self) -> usize {
        self.valid_frames_count.load(Ordering::Relaxed)
    }

    /// Returns the number of silence frames injected.
    #[inline]
    pub fn silence_injected(&self) -> usize {
        self.silence_injected_count.load(Ordering::Relaxed)
    }

    /// Returns whether the pacer is currently in an underrun state.
    #[inline]
    pub fn is_in_underrun(&self) -> bool {
        self.in_underrun.load(Ordering::Relaxed)
    }

    /// Returns the presentation timestamp (PTS) of the last presented frame in milliseconds.
    #[inline]
    pub fn last_pts_ms(&self) -> u64 {
        self.last_pts_ms.load(Ordering::Relaxed)
    }

    /// Evaluates the timing action for an audio frame against the `MasterClock`.
    ///
    /// Decision Windows:
    /// - Delta > +20ms: Wait(delta - 20ms)
    /// - -20ms <= Delta <= +20ms: PlayNow
    /// - Delta < -20ms: Late(-delta)
    pub fn evaluate_timing(
        &self,
        frame_pts_ms: u64,
        master_clock: &MasterClock,
        now: Instant,
    ) -> AudioTimingAction {
        let master_pts = master_clock.current_pts(now);
        let delta_ms = (frame_pts_ms as f64) - master_pts;

        if delta_ms > 20.0 {
            let wait_ms = delta_ms - 20.0;
            AudioTimingAction::Wait(Duration::from_secs_f64(wait_ms / 1000.0))
        } else if delta_ms >= -20.0 {
            AudioTimingAction::PlayNow
        } else {
            let late_ms = -delta_ms;
            AudioTimingAction::Late(Duration::from_secs_f64(late_ms / 1000.0))
        }
    }

    /// Pulls the next available frame from the SPSC ring buffer.
    ///
    /// If the buffer is empty (underrun), increments the underrun counter and returns a pre-allocated
    /// silence scratch frame anchored to `MasterClock` presentation time.
    /// If recovering from an underrun, applies a smooth 2ms linear ramp-in to prevent pops/clicks.
    pub fn pull_frame(
        &self,
        spsc: &SpscRingBuffer<AudioFramePCM, 64>,
        master_clock: &MasterClock,
        now: Instant,
    ) -> AudioFramePCM {
        match spsc.try_pop() {
            Some(mut frame) => {
                self.valid_frames_count.fetch_add(1, Ordering::Relaxed);
                if self.in_underrun.swap(false, Ordering::AcqRel) {
                    self.apply_ramp_in(&mut frame.samples);
                }
                self.last_pts_ms.store(frame.pts_ms, Ordering::Relaxed);
                frame
            }
            None => {
                self.underrun_count.fetch_add(1, Ordering::Relaxed);
                self.silence_injected_count.fetch_add(1, Ordering::Relaxed);
                self.in_underrun.store(true, Ordering::Release);

                let current_pts = master_clock.current_pts(now).max(0.0) as u64;
                self.last_pts_ms.store(current_pts, Ordering::Relaxed);

                let mut silence = self.silence_template.clone();
                silence.pts_ms = current_pts;
                silence
            }
        }
    }

    /// Fills an output buffer directly from the SPSC ring buffer with zero allocations.
    ///
    /// If the SPSC buffer is empty, writes zeros from the silence scratch buffer and records an underrun.
    /// Returns the number of samples written into `output`.
    pub fn fill_samples(
        &self,
        spsc: &SpscRingBuffer<AudioFramePCM, 64>,
        master_clock: &MasterClock,
        now: Instant,
        output: &mut Vec<f32>,
    ) -> usize {
        output.clear();

        match spsc.try_pop() {
            Some(mut frame) => {
                self.valid_frames_count.fetch_add(1, Ordering::Relaxed);
                if self.in_underrun.swap(false, Ordering::AcqRel) {
                    self.apply_ramp_in(&mut frame.samples);
                }
                self.last_pts_ms.store(frame.pts_ms, Ordering::Relaxed);
                output.extend_from_slice(&frame.samples);
            }
            None => {
                self.underrun_count.fetch_add(1, Ordering::Relaxed);
                self.silence_injected_count.fetch_add(1, Ordering::Relaxed);
                self.in_underrun.store(true, Ordering::Release);

                let current_pts = master_clock.current_pts(now).max(0.0) as u64;
                self.last_pts_ms.store(current_pts, Ordering::Relaxed);
                output.extend_from_slice(&self.silence_template.samples);
            }
        }

        output.len()
    }

    /// Transfers a pre-allocated silence frame over the SPSC ring buffer on the producer side.
    ///
    /// Used by decode threads to prime the buffer during pre-roll or avoid underrun during seeks.
    pub fn inject_silence_frame(
        &self,
        spsc: &SpscRingBuffer<AudioFramePCM, 64>,
        pts_ms: u64,
    ) -> Result<(), BufferFullError<AudioFramePCM>> {
        let mut silence = self.silence_template.clone();
        silence.pts_ms = pts_ms;
        spsc.try_push(silence)?;
        self.silence_injected_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Resets all counters and clears the underrun state (e.g. after a hard seek).
    pub fn reset(&self) {
        self.underrun_count.store(0, Ordering::Relaxed);
        self.valid_frames_count.store(0, Ordering::Relaxed);
        self.silence_injected_count.store(0, Ordering::Relaxed);
        self.last_pts_ms.store(0, Ordering::Relaxed);
        self.in_underrun.store(false, Ordering::Release);
    }

    /// Applies a 2ms linear ramp-in on the leading samples of a frame to prevent pops when recovering from silence.
    fn apply_ramp_in(&self, samples: &mut [f32]) {
        let ch = self.channels as usize;
        // 2ms ramp duration in frames
        let ramp_frames = ((self.sample_rate as usize * 2) / 1000).max(1);
        let total_frames = samples.len() / ch;
        let effective_ramp = ramp_frames.min(total_frames);

        for f in 0..effective_ramp {
            let gain = (f as f32) / (effective_ramp as f32);
            let frame_start = f * ch;
            for c in 0..ch {
                samples[frame_start + c] *= gain;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::master_clock::PlaybackState;

    #[test]
    fn test_audio_pacer_timing_decisions() {
        let pacer = AudioPacer::default_48k_stereo().unwrap();
        let clock = MasterClock::new(1000.0, 1.0, PlaybackState::Playing);
        let now = Instant::now();

        // 1. On-time frame (delta = 5ms -> PlayNow)
        assert_eq!(
            pacer.evaluate_timing(1005, &clock, now),
            AudioTimingAction::PlayNow
        );

        // 2. Early frame (delta = +50ms -> Wait(30ms))
        match pacer.evaluate_timing(1050, &clock, now) {
            AudioTimingAction::Wait(d) => {
                let ms = d.as_secs_f64() * 1000.0;
                assert!((ms - 30.0).abs() < 1e-2);
            }
            other => panic!("Expected Wait, got {:?}", other),
        }

        // 3. Late frame (delta = -40ms -> Late(40ms))
        match pacer.evaluate_timing(960, &clock, now) {
            AudioTimingAction::Late(d) => {
                let ms = d.as_secs_f64() * 1000.0;
                assert!((ms - 40.0).abs() < 1e-2);
            }
            other => panic!("Expected Late, got {:?}", other),
        }
    }

    #[test]
    fn test_underrun_detection_and_silence_injection() {
        let pacer = AudioPacer::default_48k_stereo().unwrap();
        let spsc = SpscRingBuffer::<AudioFramePCM, 64>::new();
        let clock = MasterClock::new(5000.0, 1.0, PlaybackState::Playing);
        let now = Instant::now();

        assert_eq!(pacer.underruns(), 0);
        assert!(!pacer.is_in_underrun());

        // Buffer is empty -> underrun triggers, returning silence frame
        let frame = pacer.pull_frame(&spsc, &clock, now);
        assert_eq!(pacer.underruns(), 1);
        assert!(pacer.is_in_underrun());
        assert_eq!(frame.sample_rate, 48000);
        assert_eq!(frame.channels, 2);
        assert_eq!(frame.pts_ms, 5000);
        assert!(frame.samples.iter().all(|&s| s == 0.0));

        // Now push a valid frame into SPSC
        let valid_samples = vec![0.5f32; 1024 * 2];
        let valid_frame = AudioFramePCM::new(48000, 2, 5021, valid_samples).unwrap();
        spsc.try_push(valid_frame).unwrap();

        // Pull frame: should recover from underrun and apply ramp-in
        let recovered = pacer.pull_frame(&spsc, &clock, now);
        assert!(!pacer.is_in_underrun());
        assert_eq!(pacer.valid_frames(), 1);
        assert_eq!(recovered.pts_ms, 5021);

        // First sample should be ramped up from 0.0 towards 0.5
        assert_eq!(recovered.samples[0], 0.0);
        // Mid-ramp should be scaled
        assert!(recovered.samples[100] > 0.0 && recovered.samples[100] < 0.5);
        // Beyond ramp should be full volume 0.5
        assert!((recovered.samples[500] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn test_zero_allocation_fill_samples() {
        let pacer = AudioPacer::default_48k_stereo().unwrap();
        let spsc = SpscRingBuffer::<AudioFramePCM, 64>::new();
        let clock = MasterClock::new(0.0, 1.0, PlaybackState::Playing);
        let now = Instant::now();

        let mut output = Vec::with_capacity(2048);

        // Fill with silence on empty SPSC
        let count = pacer.fill_samples(&spsc, &clock, now, &mut output);
        assert_eq!(count, 2048);
        assert_eq!(output.len(), 2048);
        assert!(output.iter().all(|&s| s == 0.0));
        assert_eq!(output.capacity(), 2048);
    }

    #[test]
    fn test_producer_silence_injection() {
        let pacer = AudioPacer::default_48k_stereo().unwrap();
        let spsc = SpscRingBuffer::<AudioFramePCM, 64>::new();

        pacer.inject_silence_frame(&spsc, 1234).unwrap();
        assert_eq!(pacer.silence_injected(), 1);

        let popped = spsc.try_pop().unwrap();
        assert_eq!(popped.pts_ms, 1234);
        assert!(popped.samples.iter().all(|&s| s == 0.0));
    }
}
