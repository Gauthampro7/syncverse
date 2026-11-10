//! Zero-allocation 10-band graphic equalizer with classic VLC presets, biquad peaking filters, and pre-amp gain.

use crate::audio::dsp::db_to_linear;
use crate::audio::soft_clip_sample;

/// Standard ISO/VLC 10-band center frequencies in Hertz.
pub const BAND_FREQUENCIES: [f32; 10] = [
    60.0, 170.0, 310.0, 600.0, 1000.0, 3000.0, 6000.0, 12000.0, 14000.0, 16000.0,
];

/// Second-order IIR biquad filter in Direct Form I implementation for a stereo channel pair.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiquadFilter {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    // Left channel delay state
    x1_l: f32,
    x2_l: f32,
    y1_l: f32,
    y2_l: f32,
    // Right channel delay state
    x1_r: f32,
    x2_r: f32,
    y1_r: f32,
    y2_r: f32,
}

impl Default for BiquadFilter {
    fn default() -> Self {
        Self::identity()
    }
}

impl BiquadFilter {
    /// Constructs a bypass/identity biquad filter ($y[n] = x[n]$).
    pub fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1_l: 0.0,
            x2_l: 0.0,
            y1_l: 0.0,
            y2_l: 0.0,
            x1_r: 0.0,
            x2_r: 0.0,
            y1_r: 0.0,
            y2_r: 0.0,
        }
    }

    /// Computes peaking equalizer filter coefficients conforming to Robert Bristow-Johnson Audio EQ Cookbook.
    ///
    /// - `sample_rate`: Sampling rate in Hz (e.g. 48000.0).
    /// - `freq`: Center frequency in Hz.
    /// - `q`: Filter quality factor (typically ~1.414 for ~1 octave bandwidth).
    /// - `gain_db`: Boost or cut in decibels [-20.0, 20.0].
    pub fn peaking_eq(sample_rate: f32, freq: f32, q: f32, gain_db: f32) -> Self {
        if gain_db.abs() < 0.02 {
            return Self::identity();
        }

        let sr = sample_rate.max(8000.0);
        let nyquist = sr * 0.495;
        let f0 = freq.clamp(20.0, nyquist);
        let q = q.max(0.1);

        let a = 10.0_f32.powf(gain_db / 40.0);
        let omega = 2.0 * std::f32::consts::PI * f0 / sr;
        let sin_omega = omega.sin();
        let cos_omega = omega.cos();
        let alpha = sin_omega / (2.0 * q);

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_omega;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_omega;
        let a2 = 1.0 - alpha / a;

        let inv_a0 = 1.0 / a0;

        Self {
            b0: b0 * inv_a0,
            b1: b1 * inv_a0,
            b2: b2 * inv_a0,
            a1: a1 * inv_a0,
            a2: a2 * inv_a0,
            x1_l: 0.0,
            x2_l: 0.0,
            y1_l: 0.0,
            y2_l: 0.0,
            x1_r: 0.0,
            x2_r: 0.0,
            y1_r: 0.0,
            y2_r: 0.0,
        }
    }

    /// Resets the internal delay state variables to silence (0.0).
    pub fn reset(&mut self) {
        self.x1_l = 0.0;
        self.x2_l = 0.0;
        self.y1_l = 0.0;
        self.y2_l = 0.0;
        self.x1_r = 0.0;
        self.x2_r = 0.0;
        self.y1_r = 0.0;
        self.y2_r = 0.0;
    }

    /// Processes a single stereo sample pair in Direct Form I.
    #[inline(always)]
    pub fn process_stereo(&mut self, left: &mut f32, right: &mut f32) {
        let xl = if left.is_finite() { *left } else { 0.0 };
        let yl = self.b0 * xl + self.b1 * self.x1_l + self.b2 * self.x2_l
            - self.a1 * self.y1_l
            - self.a2 * self.y2_l;

        self.x2_l = self.x1_l;
        self.x1_l = xl;
        self.y2_l = self.y1_l;
        self.y1_l = if yl.is_finite() { yl } else { 0.0 };
        *left = self.y1_l;

        let xr = if right.is_finite() { *right } else { 0.0 };
        let yr = self.b0 * xr + self.b1 * self.x1_r + self.b2 * self.x2_r
            - self.a1 * self.y1_r
            - self.a2 * self.y2_r;

        self.x2_r = self.x1_r;
        self.x1_r = xr;
        self.y2_r = self.y1_r;
        self.y1_r = if yr.is_finite() { yr } else { 0.0 };
        *right = self.y1_r;
    }
}

/// Classic VLC audio equalizer preset profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EqualizerPreset {
    /// Completely flat (0 dB across all bands).
    #[default]
    Flat,
    /// Classical acoustic profile.
    Classical,
    /// Club profile.
    Club,
    /// Dance profile with boosted lows.
    Dance,
    /// Full Bass boost.
    FullBass,
    /// Full Bass and Treble boost.
    FullBassTreble,
    /// Full Treble boost.
    FullTreble,
    /// Headphone acoustic compensation.
    Headphone,
    /// Live performance atmosphere.
    Live,
    /// Party loud profile.
    Party,
    /// Pop music curve.
    Pop,
    /// Reggae curve.
    Reggae,
    /// Rock music curve.
    Rock,
    /// Techno electronic curve.
    Techno,
    /// Dialogue / vocal clarity enhancer.
    Vocal,
}

impl EqualizerPreset {
    /// Returns the human-readable display name of the preset.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Flat => "Flat",
            Self::Classical => "Classical",
            Self::Club => "Club",
            Self::Dance => "Dance",
            Self::FullBass => "Full Bass",
            Self::FullBassTreble => "Full Bass & Treble",
            Self::FullTreble => "Full Treble",
            Self::Headphone => "Headphones",
            Self::Live => "Live",
            Self::Party => "Party",
            Self::Pop => "Pop",
            Self::Reggae => "Reggae",
            Self::Rock => "Rock",
            Self::Techno => "Techno",
            Self::Vocal => "Vocal / Dialogue",
        }
    }

    /// Returns the decibel gains for the 10 bands corresponding to this preset.
    pub fn band_gains(&self) -> [f32; 10] {
        match self {
            Self::Flat => [0.0; 10],
            Self::Classical => [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -2.0, -4.0, -4.0, -6.0],
            Self::Club => [0.0, 0.0, 2.0, 3.0, 3.0, 3.0, 2.0, 0.0, 0.0, 0.0],
            Self::Dance => [6.0, 4.0, 2.0, 0.0, 0.0, -3.0, -4.0, -4.0, 0.0, 0.0],
            Self::FullBass => [7.0, 6.0, 5.0, 3.0, 1.0, -2.0, -5.0, -7.0, -8.0, -9.0],
            Self::FullBassTreble => [6.0, 4.0, 0.0, -4.0, -3.0, 1.0, 6.0, 7.0, 8.0, 8.0],
            Self::FullTreble => [-7.0, -7.0, -7.0, -3.0, 1.0, 6.0, 9.0, 10.0, 10.0, 10.0],
            Self::Headphone => [3.0, 6.0, 3.0, -2.0, -1.0, 1.0, 3.0, 6.0, 8.0, 9.0],
            Self::Live => [-3.0, 0.0, 2.0, 3.0, 4.0, 4.0, 3.0, 2.0, 2.0, 2.0],
            Self::Party => [4.0, 4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 4.0, 4.0],
            Self::Pop => [-1.0, 1.0, 3.0, 4.0, 3.0, -1.0, -2.0, -2.0, -1.0, -1.0],
            Self::Reggae => [0.0, 0.0, -1.0, -4.0, 0.0, 4.0, 4.0, 2.0, 0.0, 0.0],
            Self::Rock => [5.0, 3.0, -3.0, -5.0, -2.0, 2.0, 5.0, 7.0, 7.0, 7.0],
            Self::Techno => [5.0, 4.0, 0.0, -3.0, -3.0, 0.0, 5.0, 6.0, 6.0, 5.0],
            Self::Vocal => [-4.0, -3.0, -1.0, 2.0, 5.0, 5.0, 4.0, 2.0, 0.0, -2.0],
        }
    }

    /// Slice of all available preset variants for dropdowns and iteration.
    pub const ALL: &'static [EqualizerPreset] = &[
        EqualizerPreset::Flat,
        EqualizerPreset::Classical,
        EqualizerPreset::Club,
        EqualizerPreset::Dance,
        EqualizerPreset::FullBass,
        EqualizerPreset::FullBassTreble,
        EqualizerPreset::FullTreble,
        EqualizerPreset::Headphone,
        EqualizerPreset::Live,
        EqualizerPreset::Party,
        EqualizerPreset::Pop,
        EqualizerPreset::Reggae,
        EqualizerPreset::Rock,
        EqualizerPreset::Techno,
        EqualizerPreset::Vocal,
    ];
}

/// Configuration settings for the 10-band audio equalizer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioEqualizerConfig {
    /// Whether equalizer filtering is enabled.
    pub enabled: bool,
    /// Pre-amp gain in decibels [-20.0, 20.0]. Default 0.0 dB.
    pub pre_amp_db: f32,
    /// Decibel gain per frequency band [-20.0, 20.0]. Default 0.0 dB.
    pub bands: [f32; 10],
}

impl Default for AudioEqualizerConfig {
    fn default() -> Self {
        Self::flat()
    }
}

impl AudioEqualizerConfig {
    /// Constructs a flat (0 dB) equalizer configuration.
    pub fn flat() -> Self {
        Self {
            enabled: false,
            pre_amp_db: 0.0,
            bands: [0.0; 10],
        }
    }

    /// Constructs an equalizer configuration loaded from a preset.
    pub fn from_preset(preset: EqualizerPreset) -> Self {
        Self {
            enabled: true,
            pre_amp_db: 0.0,
            bands: preset.band_gains(),
        }
    }

    /// Returns `true` if all band gains and pre-amp are within 0.05 dB of 0.0.
    pub fn is_flat(&self) -> bool {
        if self.pre_amp_db.abs() >= 0.05 {
            return false;
        }
        for &b in &self.bands {
            if b.abs() >= 0.05 {
                return false;
            }
        }
        true
    }
}

/// 10-band IIR biquad graphic equalizer processing interleaved 32-bit float stereo audio.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioEqualizer10Band {
    sample_rate: u32,
    config: AudioEqualizerConfig,
    filters: [BiquadFilter; 10],
    pre_amp_linear: f32,
    active_band_indices: Vec<usize>,
}

impl Default for AudioEqualizer10Band {
    fn default() -> Self {
        Self::new(48000, AudioEqualizerConfig::default())
    }
}

impl AudioEqualizer10Band {
    /// Constructs a new `AudioEqualizer10Band` for the specified sample rate and configuration.
    pub fn new(sample_rate: u32, config: AudioEqualizerConfig) -> Self {
        let mut eq = Self {
            sample_rate: sample_rate.max(8000),
            config,
            filters: [BiquadFilter::identity(); 10],
            pre_amp_linear: 1.0,
            active_band_indices: Vec::with_capacity(10),
        };
        eq.recalculate();
        eq
    }

    /// Returns the active equalizer configuration.
    pub fn config(&self) -> &AudioEqualizerConfig {
        &self.config
    }

    /// Returns `true` if the equalizer is enabled and actively modifying audio.
    pub fn is_active(&self) -> bool {
        self.config.enabled && (!self.active_band_indices.is_empty() || (self.pre_amp_linear - 1.0).abs() >= 0.005)
    }

    /// Updates the equalizer configuration and recalculates filter coefficients.
    pub fn set_config(&mut self, config: AudioEqualizerConfig) {
        self.config = config;
        self.recalculate();
    }

    /// Resets the internal state of all biquad filters to silence.
    pub fn reset(&mut self) {
        for f in &mut self.filters {
            f.reset();
        }
    }

    /// Recalculates filter coefficients based on current configuration and sample rate.
    fn recalculate(&mut self) {
        self.pre_amp_linear = db_to_linear(self.config.pre_amp_db.clamp(-20.0, 20.0));
        self.active_band_indices.clear();

        let sr = self.sample_rate as f32;
        for (i, &freq) in BAND_FREQUENCIES.iter().enumerate() {
            let gain = self.config.bands[i].clamp(-20.0, 20.0);
            if gain.abs() >= 0.05 {
                self.filters[i] = BiquadFilter::peaking_eq(sr, freq, 1.414, gain);
                self.active_band_indices.push(i);
            } else {
                self.filters[i] = BiquadFilter::identity();
            }
        }
    }

    /// Processes interleaved stereo audio samples (`[L, R, L, R, ...]`) in-place.
    ///
    /// Multiplies by pre-amp gain, chains active biquad filters, and applies `soft_clip_sample`
    /// to eliminate digital clipping distortion.
    pub fn process_interleaved(&mut self, samples: &mut [f32]) {
        if !self.is_active() || samples.is_empty() {
            return;
        }

        let pre_amp = self.pre_amp_linear;
        let (chunks, remainder) = samples.as_chunks_mut::<2>();

        for [left, right] in chunks {
            *left *= pre_amp;
            *right *= pre_amp;

            for &idx in &self.active_band_indices {
                self.filters[idx].process_stereo(left, right);
            }

            *left = soft_clip_sample(*left);
            *right = soft_clip_sample(*right);
        }

        for rem in remainder {
            *rem = soft_clip_sample(*rem * pre_amp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_biquad_identity_and_reset() {
        let mut biquad = BiquadFilter::identity();
        let mut l = 0.5;
        let mut r = -0.5;
        biquad.process_stereo(&mut l, &mut r);
        assert!((l - 0.5).abs() < 1e-6);
        assert!((r - (-0.5)).abs() < 1e-6);

        biquad.reset();
        assert_eq!(biquad.x1_l, 0.0);
        assert_eq!(biquad.y1_l, 0.0);
    }

    #[test]
    fn test_equalizer_flat_bypass() {
        let mut eq = AudioEqualizer10Band::default();
        assert!(!eq.is_active());

        let mut samples = vec![0.5, -0.5, 0.25, -0.25];
        let original = samples.clone();
        eq.process_interleaved(&mut samples);
        assert_eq!(samples, original);
    }

    #[test]
    fn test_equalizer_boost_and_cut() {
        let mut cfg = AudioEqualizerConfig::flat();
        cfg.enabled = true;
        cfg.bands[4] = 6.0; // Boost 1000 Hz by 6 dB
        let mut eq = AudioEqualizer10Band::new(48000, cfg);
        assert!(eq.is_active());

        // Generate 1000 Hz sine wave (4800 samples = 100ms)
        let mut sine_1khz = Vec::new();
        for n in 0..4800 {
            let t = n as f32 / 48000.0;
            let val = (2.0 * std::f32::consts::PI * 1000.0 * t).sin() * 0.2;
            sine_1khz.push(val);
            sine_1khz.push(val);
        }

        let input_peak = crate::audio::calculate_peak(&sine_1khz);
        eq.process_interleaved(&mut sine_1khz);
        // Measure steady state peak (second half of signal)
        let output_peak = crate::audio::calculate_peak(&sine_1khz[2400..]);

        // Output peak should be noticeably larger than input due to +6 dB boost (~2x)
        assert!(output_peak > input_peak * 1.5, "Expected boost: input {}, output {}", input_peak, output_peak);
    }

    #[test]
    fn test_equalizer_presets_and_names() {
        for preset in EqualizerPreset::ALL {
            assert!(!preset.name().is_empty());
            let cfg = AudioEqualizerConfig::from_preset(*preset);
            assert!(cfg.enabled);
            assert_eq!(cfg.bands, preset.band_gains());
        }
    }

    #[test]
    fn test_equalizer_nan_resilience() {
        let mut cfg = AudioEqualizerConfig::flat();
        cfg.enabled = true;
        cfg.bands[4] = 6.0; // 1kHz boost
        let mut eq = AudioEqualizer10Band::new(48000, cfg);

        let mut samples = vec![f32::NAN, 1.0, f32::INFINITY, -1.0];
        eq.process_interleaved(&mut samples);

        assert!(samples[0].is_finite());
        assert!(samples[1].is_finite());
        assert!(samples[2].is_finite());
        assert!(samples[3].is_finite());
    }
}
