//! Zero-allocation audio DSP primitives: peak/RMS metering, decibel/linear conversion,
//! feed-forward dynamic range compression with soft-knee limiter, and constant-power stereo panning.

use crate::audio::soft_clip_sample;

/// Floor limit for decibel calculations corresponding to -120 dBFS.
pub const DB_FLOOR: f32 = -120.0;

/// Linear threshold corresponding to `DB_FLOOR` ($10^{-6}$).
pub const LINEAR_FLOOR: f32 = 0.000001;

/// Calculates the peak absolute amplitude across an audio sample slice.
///
/// Ignores non-finite and `NaN` samples. Returns `0.0` if the slice is empty.
pub fn calculate_peak(samples: &[f32]) -> f32 {
    let mut peak: f32 = 0.0;
    for &s in samples {
        if s.is_finite() {
            let abs = s.abs();
            if abs > peak {
                peak = abs;
            }
        }
    }
    peak
}

/// Calculates the root-mean-square (RMS) loudness across an audio sample slice.
///
/// Returns `0.0` if the slice is empty.
pub fn calculate_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sum_sq: f64 = 0.0;
    let mut count: usize = 0;

    for &s in samples {
        if s.is_finite() {
            sum_sq += (s as f64) * (s as f64);
            count += 1;
        }
    }

    if count == 0 {
        0.0
    } else {
        (sum_sq / count as f64).sqrt() as f32
    }
}

/// Converts linear amplitude to decibels full scale (dBFS): $20 \cdot \log_{10}(\max(linear, 10^{-6}))$.
///
/// Clamped at a floor of `-120.0` dB. Non-finite or non-positive inputs return `DB_FLOOR`.
#[inline]
pub fn linear_to_db(linear: f32) -> f32 {
    if !linear.is_finite() || linear <= LINEAR_FLOOR {
        DB_FLOOR
    } else {
        (20.0 * linear.log10()).max(DB_FLOOR)
    }
}

/// Converts decibels full scale (dBFS) to linear amplitude: $10^{\text{db} / 20}$.
///
/// Inputs at or below `DB_FLOOR` return `0.0`.
#[inline]
pub fn db_to_linear(db: f32) -> f32 {
    if !db.is_finite() || db <= DB_FLOOR {
        0.0
    } else {
        10.0_f32.powf(db / 20.0)
    }
}

/// In-place gain amplification with soft-knee saturation protection.
///
/// Multiplies each sample by `gain_linear` and runs it through `soft_clip_sample`
/// to eliminate digital clipping buzz.
pub fn apply_gain(samples: &mut [f32], gain_linear: f32) {
    if !gain_linear.is_finite() || gain_linear <= 0.0 {
        samples.fill(0.0);
        return;
    }
    for s in samples.iter_mut() {
        *s = soft_clip_sample(*s * gain_linear);
    }
}

/// Feed-forward audio dynamic range compressor / peak limiter with soft-knee saturation.
///
/// Protects against excessive dynamic range spikes and whispers/explosions disparity
/// without allocating heap memory during runtime execution.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioDynamicsCompressor {
    threshold_db: f32,
    ratio: f32,
    attack_coeff: f32,
    release_coeff: f32,
    makeup_gain_linear: f32,
    sample_rate: u32,
    envelope_db: f32,
}

impl AudioDynamicsCompressor {
    /// Constructs a new dynamic range compressor with specified acoustic parameters.
    ///
    /// - `threshold_db`: Decibel level above which compression activates (e.g. `-14.0` dB).
    /// - `ratio`: Compression ratio $\ge 1.0$ (e.g. `4.0` for 4:1 compression).
    /// - `attack_ms`: Attack response time in milliseconds (e.g. `10.0` ms).
    /// - `release_ms`: Release recovery time in milliseconds (e.g. `100.0` ms).
    /// - `makeup_gain_db`: Post-compression amplification in decibels (e.g. `2.0` dB).
    /// - `sample_rate`: Audio sampling rate in Hertz (e.g. `48000`).
    pub fn new(
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        makeup_gain_db: f32,
        sample_rate: u32,
    ) -> Self {
        let sr = sample_rate.max(8000) as f32;
        let attack_sec = (attack_ms.max(0.1)) / 1000.0;
        let release_sec = (release_ms.max(1.0)) / 1000.0;

        let attack_coeff = (-1.0 / (attack_sec * sr)).exp();
        let release_coeff = (-1.0 / (release_sec * sr)).exp();

        Self {
            threshold_db: threshold_db.clamp(-60.0, 0.0),
            ratio: ratio.max(1.0),
            attack_coeff,
            release_coeff,
            makeup_gain_linear: db_to_linear(makeup_gain_db),
            sample_rate,
            envelope_db: DB_FLOOR,
        }
    }

    /// Resets the internal envelope follower state to silence (`DB_FLOOR`).
    pub fn reset(&mut self) {
        self.envelope_db = DB_FLOOR;
    }

    /// Processes an in-place slice of audio samples with zero memory allocations.
    ///
    /// Updates the dynamic detector envelope, computes gain reduction, applies makeup gain,
    /// and soft-clips output peaks.
    pub fn process(&mut self, samples: &mut [f32]) {
        let slope = 1.0 - (1.0 / self.ratio);

        for s in samples.iter_mut() {
            let abs_val = s.abs();
            let input_db = linear_to_db(abs_val);

            // Ballistics envelope smoothing
            let coeff = if input_db > self.envelope_db {
                self.attack_coeff
            } else {
                self.release_coeff
            };
            self.envelope_db = coeff * self.envelope_db + (1.0 - coeff) * input_db;

            // Compute gain reduction in decibels
            let gain_reduction_db = if self.envelope_db > self.threshold_db {
                -slope * (self.envelope_db - self.threshold_db)
            } else {
                0.0
            };

            let gain_linear = db_to_linear(gain_reduction_db) * self.makeup_gain_linear;
            *s = soft_clip_sample(*s * gain_linear);
        }
    }
}

/// Constant-power stereo panning operator conforming to the $3\text{ dB}$ panning law.
///
/// Ensures perceptual volume remains identical regardless of left/right pan position ($L^2 + R^2 = 1.0$).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioStereoPan;

impl AudioStereoPan {
    /// Applies constant-power panning to interleaved stereo audio samples (`[L, R, L, R, ...]`).
    ///
    /// - `pan`: Position in range `[-1.0, 1.0]`, where `-1.0` is full left, `0.0` is center,
    ///   and `1.0` is full right.
    pub fn pan_interleaved(samples: &mut [f32], pan: f32) {
        let pan = if pan.is_finite() { pan.clamp(-1.0, 1.0) } else { 0.0 };
        // Normalized angle in [0, pi/2]
        let angle = (pan + 1.0) * (std::f32::consts::FRAC_PI_4);
        let left_gain = angle.cos();
        let right_gain = angle.sin();

        let (chunks, remainder) = samples.as_chunks_mut::<2>();
        for [left_sample, right_sample] in chunks {
            let left = if left_sample.is_finite() { *left_sample * left_gain } else { 0.0 };
            let right = if right_sample.is_finite() { *right_sample * right_gain } else { 0.0 };
            *left_sample = left;
            *right_sample = right;
        }

        for rem in remainder {
            if !rem.is_finite() {
                *rem = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peak_and_rms_calculations() {
        let samples = [0.0, 0.5, -0.8, 0.2];
        assert!((calculate_peak(&samples) - 0.8).abs() < 1e-6);

        // RMS of [1.0, -1.0, 1.0, -1.0] is 1.0
        let unit_samples = [1.0, -1.0, 1.0, -1.0];
        assert!((calculate_rms(&unit_samples) - 1.0).abs() < 1e-6);

        // Empty slice returns 0.0
        assert_eq!(calculate_peak(&[]), 0.0);
        assert_eq!(calculate_rms(&[]), 0.0);
    }

    #[test]
    fn test_db_linear_roundtrip() {
        let test_dbs = [0.0, -6.0, -12.0, -20.0, -60.0, -100.0];
        for &db in &test_dbs {
            let linear = db_to_linear(db);
            let roundtrip = linear_to_db(linear);
            assert!((roundtrip - db).abs() < 0.05, "failed for db {}", db);
        }

        assert_eq!(linear_to_db(0.0), DB_FLOOR);
        assert_eq!(db_to_linear(DB_FLOOR), 0.0);
    }

    #[test]
    fn test_compressor_gain_reduction() {
        let mut comp = AudioDynamicsCompressor::new(-10.0, 4.0, 1.0, 20.0, 0.0, 48000);
        let mut loud_burst = vec![1.0; 480]; // 10ms of 0 dBFS signal (exceeding -10 dB threshold)

        comp.process(&mut loud_burst);

        // The loud burst should have been compressed below 1.0
        let final_peak = calculate_peak(&loud_burst[240..]);
        assert!(final_peak < 0.8, "peak should be compressed: {}", final_peak);
    }

    #[test]
    fn test_constant_power_stereo_panning() {
        let mut samples = vec![1.0, 1.0];
        // Center pan (0.0): left_gain = cos(pi/4) = 0.7071, right_gain = sin(pi/4) = 0.7071
        AudioStereoPan::pan_interleaved(&mut samples, 0.0);

        let power = samples[0] * samples[0] + samples[1] * samples[1];
        assert!((power - 1.0).abs() < 1e-4, "Power should be 1.0: {}", power);

        // Full left (-1.0): left_gain = cos(0) = 1.0, right_gain = sin(0) = 0.0
        let mut samples_left = vec![1.0, 1.0];
        AudioStereoPan::pan_interleaved(&mut samples_left, -1.0);
        assert!((samples_left[0] - 1.0).abs() < 1e-4);
        assert!(samples_left[1].abs() < 1e-4);

        // Full right (1.0): left_gain = cos(pi/2) = 0.0, right_gain = sin(pi/2) = 1.0
        let mut samples_right = vec![1.0, 1.0];
        AudioStereoPan::pan_interleaved(&mut samples_right, 1.0);
        assert!(samples_right[0].abs() < 1e-4);
        assert!((samples_right[1] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_constant_power_stereo_panning_adversarial() {
        let mut samples = vec![f32::NAN, f32::INFINITY, -f32::INFINITY, 0.5, f32::NAN];
        AudioStereoPan::pan_interleaved(&mut samples, 0.5);

        assert_eq!(samples[0], 0.0);
        assert_eq!(samples[1], 0.0);
        assert_eq!(samples[2], 0.0);
        assert!(samples[3].is_finite());
        assert_eq!(samples[4], 0.0);
    }
}
