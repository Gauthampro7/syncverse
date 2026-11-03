//! Pure-Rust audio packet demuxing and payload unpacking (PCM, Opus, Vorbis).

pub mod decoder;
pub mod dsp;
pub mod opus;
pub mod pcm;
pub mod resampler;
pub mod universal;
pub mod vorbis;

pub use decoder::{AudioCodec, AudioDecoder};
pub use dsp::{
    apply_gain, calculate_peak, calculate_rms, db_to_linear, linear_to_db, AudioDynamicsCompressor,
    AudioStereoPan, DB_FLOOR, LINEAR_FLOOR,
};
pub use opus::{unpack_opus_packet, OpusBandwidth, OpusMode, OpusToc};
pub use pcm::{unpack_pcm, PcmFormat};
pub use resampler::LinearResampler;
pub use universal::{UniversalAudioDecoder, UniversalAudioError};
pub use vorbis::{unpack_vorbis_audio_packet, VorbisIdentificationHeader};

/// Smooth, zero-allocation soft-knee saturation curve for audio samples exceeding +/-0.85.
///
/// Samples within `[-0.85, 0.85]` undergo zero modification (linear pass-through).
/// Samples exceeding `+/-0.85` are smoothly compressed using hyperbolic tangent (`tanh`)
/// towards `+/-1.0`, eliminating harsh digital clipping buzz on loud or multichannel downmixed audio.
/// Any `NaN` inputs are cleanly silenced to `0.0` to guard against floating-point corruption.
#[inline(always)]
pub fn soft_clip_sample(s: f32) -> f32 {
    if s.is_nan() {
        return 0.0;
    }
    if s.abs() <= 0.85 {
        s
    } else if s > 0.85 {
        0.85 + 0.15 * ((s - 0.85) / 0.15).tanh()
    } else {
        -0.85 - 0.15 * ((-s - 0.85) / 0.15).tanh()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soft_clip_linear_region() {
        // Exact passthrough for |s| <= 0.85
        assert_eq!(soft_clip_sample(0.0), 0.0);
        assert_eq!(soft_clip_sample(0.5), 0.5);
        assert_eq!(soft_clip_sample(-0.5), -0.5);
        assert_eq!(soft_clip_sample(0.85), 0.85);
        assert_eq!(soft_clip_sample(-0.85), -0.85);
    }

    #[test]
    fn test_soft_clip_saturation_and_bounds() {
        let val_pos = soft_clip_sample(1.0);
        assert!(val_pos > 0.85 && val_pos < 1.0);

        let val_neg = soft_clip_sample(-1.0);
        assert!(val_neg < -0.85 && val_neg > -1.0);

        // Extreme peaks must remain bounded within [-1.0, 1.0]
        let extreme_pos = soft_clip_sample(10.0);
        assert!((0.99..=1.0).contains(&extreme_pos));

        let extreme_neg = soft_clip_sample(-10.0);
        assert!((-1.0..=-0.99).contains(&extreme_neg));
    }

    #[test]
    fn test_soft_clip_symmetry_and_monotonicity() {
        for i in 0..200 {
            let s = (i as f32) * 0.02 - 2.0; // [-2.0, 2.0]
            let clipped = soft_clip_sample(s);
            let clipped_neg = soft_clip_sample(-s);
            assert!((clipped + clipped_neg).abs() < 1e-6);

            let s_next = s + 0.01;
            assert!(soft_clip_sample(s_next) >= clipped);
        }
    }

    #[test]
    fn test_soft_clip_nan_silencing() {
        assert_eq!(soft_clip_sample(f32::NAN), 0.0);
    }
}



