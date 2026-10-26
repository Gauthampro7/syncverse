//! Pure-Rust linear audio resampler with fractional phase accumulation and speed steering.
//!
//! Provides seamless sample rate conversion (e.g. 44.1 kHz <-> 48.0 kHz) and continuous
//! micro-speed steering ratio $r \in [0.96, 1.04]$ with zero heap allocations in the hot loop.

use std::sync::Mutex;
use crate::error::CodecError;
use crate::frame::AudioFramePCM;

/// Internal state for linear resampling across chunk boundaries.
#[derive(Debug)]
struct ResamplerState {
    phase: f64,
    speed_ratio: f64,
    prev_frame: Vec<f32>,
    has_history: bool,
}

/// Pure-Rust linear resampler with fractional phase accumulation and speed steering.
///
/// Features:
/// - Mathematically continuous linear interpolation with phase tracking across chunk boundaries.
/// - Dynamic micro-speed steering ratio $r \in [0.96, 1.04]$ for network clock synchronization.
/// - Zero heap allocation during steady-state processing (`resample_into`).
/// - Multi-channel interleaved audio support (mono, stereo, 5.1, etc.).
/// - Thread-safe `Send` + `Sync` interface.
#[derive(Debug)]
pub struct LinearResampler {
    in_rate: u32,
    out_rate: u32,
    channels: u16,
    state: Mutex<ResamplerState>,
}

impl LinearResampler {
    /// Creates a new `LinearResampler` with default speed ratio 1.0.
    pub fn new(in_rate: u32, out_rate: u32, channels: u16) -> Result<Self, CodecError> {
        Self::with_speed_ratio(in_rate, out_rate, channels, 1.0)
    }

    /// Creates a new `LinearResampler` with a specified initial speed ratio.
    pub fn with_speed_ratio(
        in_rate: u32,
        out_rate: u32,
        channels: u16,
        speed_ratio: f64,
    ) -> Result<Self, CodecError> {
        if in_rate == 0 || out_rate == 0 || channels == 0 {
            return Err(CodecError::InvalidAudio(
                "Sample rates and channel count must be > 0".into(),
            ));
        }
        if speed_ratio <= 0.0 || speed_ratio.is_nan() || speed_ratio.is_infinite() {
            return Err(CodecError::InvalidAudio(
                "Speed ratio must be a positive finite number".into(),
            ));
        }

        Ok(Self {
            in_rate,
            out_rate,
            channels,
            state: Mutex::new(ResamplerState {
                phase: 0.0,
                speed_ratio,
                prev_frame: vec![0.0; channels as usize],
                has_history: false,
            }),
        })
    }

    /// Returns the input sample rate in Hz.
    #[inline]
    pub fn in_rate(&self) -> u32 {
        self.in_rate
    }

    /// Returns the output sample rate in Hz.
    #[inline]
    pub fn out_rate(&self) -> u32 {
        self.out_rate
    }

    /// Returns the number of audio channels.
    #[inline]
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Returns the current playback speed ratio.
    pub fn speed_ratio(&self) -> f64 {
        let state = self.state.lock().expect("resampler lock poisoned");
        state.speed_ratio
    }

    /// Dynamically updates the micro-speed steering ratio (typically $r \in [0.96, 1.04]$).
    pub fn set_speed_ratio(&self, speed_ratio: f64) -> Result<(), CodecError> {
        if speed_ratio <= 0.0 || speed_ratio.is_nan() || speed_ratio.is_infinite() {
            return Err(CodecError::InvalidAudio(
                "Speed ratio must be a positive finite number".into(),
            ));
        }
        let mut state = self.state.lock().expect("resampler lock poisoned");
        state.speed_ratio = speed_ratio;
        Ok(())
    }

    /// Resets the internal fractional phase accumulator and boundary history buffer.
    pub fn reset(&self) {
        let mut state = self.state.lock().expect("resampler lock poisoned");
        state.phase = 0.0;
        state.has_history = false;
        state.prev_frame.fill(0.0);
    }

    /// Resamples interleaved PCM audio samples into a pre-allocated output vector.
    ///
    /// If `output` has sufficient capacity, zero heap allocations occur in this call.
    /// Returns the number of samples written into `output`.
    pub fn resample_into(&self, input: &[f32], output: &mut Vec<f32>) -> Result<usize, CodecError> {
        output.clear();
        if input.is_empty() {
            return Ok(0);
        }

        let ch = self.channels as usize;
        if !input.len().is_multiple_of(ch) {
            return Err(CodecError::InvalidAudio(format!(
                "Input sample count {} is not a multiple of channels {}",
                input.len(),
                ch
            )));
        }

        let num_in_frames = input.len() / ch;
        let mut state = self.state.lock().expect("resampler lock poisoned");
        let speed_ratio = state.speed_ratio;
        let step = ((self.in_rate as f64) * speed_ratio) / (self.out_rate as f64);

        let estimated_out_frames = ((num_in_frames as f64) / step).ceil() as usize + 8;
        output.reserve(estimated_out_frames * ch);

        let mut t = state.phase;

        if !state.has_history {
            // First chunk: coordinate 0 is input[0], coordinate 1 is input[1], etc.
            while t < (num_in_frames as f64) {
                let k = t.floor() as usize;
                let alpha = (t - (k as f64)) as f32;

                if k + 1 < num_in_frames {
                    let idx0 = k * ch;
                    let idx1 = (k + 1) * ch;
                    for c in 0..ch {
                        let s0 = input[idx0 + c];
                        let s1 = input[idx1 + c];
                        let s = (1.0 - alpha) * s0 + alpha * s1;
                        output.push(s.clamp(-1.0, 1.0));
                    }
                    t += step;
                } else if k == num_in_frames - 1 && alpha < 1e-9 {
                    let idx0 = k * ch;
                    for c in 0..ch {
                        output.push(input[idx0 + c].clamp(-1.0, 1.0));
                    }
                    t += step;
                } else {
                    break;
                }
            }

            let last_frame_start = (num_in_frames - 1) * ch;
            state.prev_frame.copy_from_slice(&input[last_frame_start..last_frame_start + ch]);
            state.has_history = true;
            state.phase = t - ((num_in_frames - 1) as f64);
        } else {
            // Subsequent chunks: coordinate 0 is prev_frame, coordinate 1 is input[0], etc.
            while t < ((num_in_frames + 1) as f64) {
                let k = t.floor() as usize;
                let alpha = (t - (k as f64)) as f32;

                if k == 0 {
                    // Between prev_frame and input[0]
                    for (&s0, &s1) in state.prev_frame[..ch].iter().zip(&input[..ch]) {
                        let s = (1.0 - alpha) * s0 + alpha * s1;
                        output.push(s.clamp(-1.0, 1.0));
                    }
                    t += step;
                } else if k < num_in_frames {
                    // Between input[k - 1] and input[k]
                    let idx0 = (k - 1) * ch;
                    let idx1 = k * ch;
                    for c in 0..ch {
                        let s0 = input[idx0 + c];
                        let s1 = input[idx1 + c];
                        let s = (1.0 - alpha) * s0 + alpha * s1;
                        output.push(s.clamp(-1.0, 1.0));
                    }
                    t += step;
                } else if k == num_in_frames && alpha < 1e-9 {
                    let idx0 = (num_in_frames - 1) * ch;
                    for c in 0..ch {
                        output.push(input[idx0 + c].clamp(-1.0, 1.0));
                    }
                    t += step;
                } else {
                    break;
                }
            }

            let last_frame_start = (num_in_frames - 1) * ch;
            state.prev_frame.copy_from_slice(&input[last_frame_start..last_frame_start + ch]);
            state.phase = t - (num_in_frames as f64);
        }

        Ok(output.len())
    }

    /// Resamples interleaved PCM audio samples, returning a newly allocated `Vec<f32>`.
    pub fn resample(&self, input: &[f32]) -> Result<Vec<f32>, CodecError> {
        let mut out = Vec::new();
        self.resample_into(input, &mut out)?;
        Ok(out)
    }

    /// Resamples an `AudioFramePCM` into a new `AudioFramePCM` with the resampler's output rate.
    pub fn resample_frame(&self, frame: &AudioFramePCM) -> Result<AudioFramePCM, CodecError> {
        if frame.sample_rate != self.in_rate {
            return Err(CodecError::InvalidAudio(format!(
                "Frame sample rate {} does not match resampler in_rate {}",
                frame.sample_rate, self.in_rate
            )));
        }
        if frame.channels != self.channels {
            return Err(CodecError::InvalidAudio(format!(
                "Frame channel count {} does not match resampler channels {}",
                frame.channels, self.channels
            )));
        }

        let resampled_samples = self.resample(&frame.samples)?;
        AudioFramePCM::new(self.out_rate, self.channels, frame.pts_ms, resampled_samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resampler_identity() {
        let resampler = LinearResampler::new(48000, 48000, 2).unwrap();
        let input = vec![0.0, 0.5, -0.5, 0.25, -0.25, 0.0];
        let output = resampler.resample(&input).unwrap();
        assert_eq!(output.len(), input.len());
        for (i, (&a, &b)) in input.iter().zip(output.iter()).enumerate() {
            assert!(
                (a - b).abs() < 1e-4,
                "Sample {} differs: in={}, out={}",
                i,
                a,
                b
            );
        }
    }

    #[test]
    fn test_resampler_phase_continuity_across_chunks() {
        let in_rate = 44100;
        let out_rate = 48000;
        let channels = 1;

        // Generate a 440 Hz test tone for 100ms (4410 samples)
        let total_samples = 4410;
        let mut full_input = Vec::with_capacity(total_samples);
        for i in 0..total_samples {
            let sample = (2.0 * std::f32::consts::PI * 440.0 * (i as f32) / (in_rate as f32)).sin();
            full_input.push(sample);
        }

        // Resample in one big chunk
        let resampler_single = LinearResampler::new(in_rate, out_rate, channels).unwrap();
        let out_single = resampler_single.resample(&full_input).unwrap();

        // Resample in three separate chunks
        let resampler_split = LinearResampler::new(in_rate, out_rate, channels).unwrap();
        let mut out_split = Vec::new();
        let mut chunk_out = Vec::new();

        let c1 = &full_input[0..1000];
        let c2 = &full_input[1000..2500];
        let c3 = &full_input[2500..];

        resampler_split.resample_into(c1, &mut chunk_out).unwrap();
        out_split.extend_from_slice(&chunk_out);

        resampler_split.resample_into(c2, &mut chunk_out).unwrap();
        out_split.extend_from_slice(&chunk_out);

        resampler_split.resample_into(c3, &mut chunk_out).unwrap();
        out_split.extend_from_slice(&chunk_out);

        assert_eq!(out_single.len(), out_split.len());
        for (i, (&s1, &s2)) in out_single.iter().zip(out_split.iter()).enumerate() {
            assert!(
                (s1 - s2).abs() < 1e-4,
                "Discontinuity at sample {}: single={}, split={}",
                i,
                s1,
                s2
            );
        }
    }

    #[test]
    fn test_speed_ratio_micro_steering() {
        let resampler = LinearResampler::with_speed_ratio(48000, 48000, 2, 1.0).unwrap();
        assert_eq!(resampler.speed_ratio(), 1.0);

        // Steering to 1.04x speed
        resampler.set_speed_ratio(1.04).unwrap();
        assert_eq!(resampler.speed_ratio(), 1.04);

        let input = vec![0.1; 4800 * 2]; // 100ms of stereo audio
        let output = resampler.resample(&input).unwrap();
        // At 1.04x speed, step is larger (48000 * 1.04 / 48000 = 1.04)
        // so input audio is consumed 1.04x faster, generating fewer output frames (4800 / 1.04 = ~4615)
        let out_frames = output.len() / 2;
        let expected_frames = (4800.0f64 / 1.04).round() as usize;
        assert!(
            (out_frames as isize - expected_frames as isize).abs() <= 2,
            "Expected ~{} frames, got {}",
            expected_frames,
            out_frames
        );

        // Steering to 0.96x speed: step is smaller (0.96), generating more output frames (4800 / 0.96 = 5000)
        resampler.reset();
        resampler.set_speed_ratio(0.96).unwrap();
        let output_slow = resampler.resample(&input).unwrap();
        let out_frames_slow = output_slow.len() / 2;
        let expected_frames_slow = (4800.0f64 / 0.96).round() as usize;
        assert!(
            (out_frames_slow as isize - expected_frames_slow as isize).abs() <= 2,
            "Expected ~{} frames, got {}",
            expected_frames_slow,
            out_frames_slow
        );

        // Invariant: r = 1.04 must generate fewer output frames than r = 0.96 from the same input buffer
        assert!(
            out_frames < out_frames_slow,
            "Faster speed (1.04x) must generate fewer output frames ({}) than slower speed (0.96x, {})",
            out_frames,
            out_frames_slow
        );
    }

    #[test]
    fn test_zero_allocation_resample_into() {
        let resampler = LinearResampler::new(44100, 48000, 2).unwrap();
        let input = vec![0.2; 1024 * 2];
        let mut output = Vec::with_capacity(2048 * 2);

        // First run reserves if needed
        resampler.resample_into(&input, &mut output).unwrap();
        let cap_before = output.capacity();

        // Second run uses pre-allocated capacity
        resampler.resample_into(&input, &mut output).unwrap();
        let cap_after = output.capacity();

        assert_eq!(cap_before, cap_after, "Capacity must not grow on identical chunks");
    }
}
