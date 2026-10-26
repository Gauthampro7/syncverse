//! Uncompressed PCM audio packet demuxing and payload unpacking.

use crate::error::CodecError;
use crate::frame::AudioFramePCM;

/// Audio PCM sample format and endianness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcmFormat {
    /// 8-bit unsigned integer PCM (offset binary: 128 = 0.0).
    U8,
    /// 16-bit signed integer little-endian PCM.
    I16Le,
    /// 16-bit signed integer big-endian PCM.
    I16Be,
    /// 24-bit signed integer little-endian PCM.
    I24Le,
    /// 24-bit signed integer big-endian PCM.
    I24Be,
    /// 32-bit signed integer little-endian PCM.
    I32Le,
    /// 32-bit signed integer big-endian PCM.
    I32Be,
    /// 32-bit IEEE 754 floating-point little-endian PCM.
    F32Le,
    /// 32-bit IEEE 754 floating-point big-endian PCM.
    F32Be,
}

impl PcmFormat {
    /// Returns the number of bytes per single audio sample for this format.
    #[inline]
    pub const fn bytes_per_sample(&self) -> usize {
        match self {
            Self::U8 => 1,
            Self::I16Le | Self::I16Be => 2,
            Self::I24Le | Self::I24Be => 3,
            Self::I32Le | Self::I32Be | Self::F32Le | Self::F32Be => 4,
        }
    }
}

/// Unpacks raw interleaved PCM audio bytes into a normalized `AudioFramePCM` in range `[-1.0, 1.0]`.
pub fn unpack_pcm(
    data: &[u8],
    format: PcmFormat,
    channels: u16,
    sample_rate: u32,
    pts_ms: u64,
) -> Result<AudioFramePCM, CodecError> {
    if channels == 0 || sample_rate == 0 {
        return Err(CodecError::InvalidAudio(
            "channels and sample_rate must be > 0".into(),
        ));
    }

    let bps = format.bytes_per_sample();
    let frame_bytes = bps * (channels as usize);
    let num_frames = data.len() / frame_bytes;
    let total_samples = num_frames * (channels as usize);

    let mut samples = Vec::with_capacity(total_samples);

    match format {
        PcmFormat::U8 => {
            for &byte in &data[..total_samples] {
                let s = ((byte as f32) - 128.0) / 128.0;
                samples.push(s.clamp(-1.0, 1.0));
            }
        }
        PcmFormat::I16Le => {
            let (chunks, _) = data[..total_samples * 2].as_chunks::<2>();
            for chunk in chunks {
                let val = i16::from_le_bytes(*chunk);
                let s = (val as f32) / 32768.0;
                samples.push(s.clamp(-1.0, 1.0));
            }
        }
        PcmFormat::I16Be => {
            let (chunks, _) = data[..total_samples * 2].as_chunks::<2>();
            for chunk in chunks {
                let val = i16::from_be_bytes(*chunk);
                let s = (val as f32) / 32768.0;
                samples.push(s.clamp(-1.0, 1.0));
            }
        }
        PcmFormat::I24Le => {
            let (chunks, _) = data[..total_samples * 3].as_chunks::<3>();
            for chunk in chunks {
                let b0 = chunk[0] as u32;
                let b1 = chunk[1] as u32;
                let b2 = chunk[2] as u32;
                let raw = b0 | (b1 << 8) | (b2 << 16);
                let signed = if (raw & 0x800000) != 0 {
                    (raw | 0xFF000000) as i32
                } else {
                    raw as i32
                };
                let s = (signed as f32) / 8388608.0;
                samples.push(s.clamp(-1.0, 1.0));
            }
        }
        PcmFormat::I24Be => {
            let (chunks, _) = data[..total_samples * 3].as_chunks::<3>();
            for chunk in chunks {
                let b0 = chunk[0] as u32;
                let b1 = chunk[1] as u32;
                let b2 = chunk[2] as u32;
                let raw = (b0 << 16) | (b1 << 8) | b2;
                let signed = if (raw & 0x800000) != 0 {
                    (raw | 0xFF000000) as i32
                } else {
                    raw as i32
                };
                let s = (signed as f32) / 8388608.0;
                samples.push(s.clamp(-1.0, 1.0));
            }
        }
        PcmFormat::I32Le => {
            let (chunks, _) = data[..total_samples * 4].as_chunks::<4>();
            for chunk in chunks {
                let val = i32::from_le_bytes(*chunk);
                let s = ((val as f64) / 2147483648.0) as f32;
                samples.push(s.clamp(-1.0, 1.0));
            }
        }
        PcmFormat::I32Be => {
            let (chunks, _) = data[..total_samples * 4].as_chunks::<4>();
            for chunk in chunks {
                let val = i32::from_be_bytes(*chunk);
                let s = ((val as f64) / 2147483648.0) as f32;
                samples.push(s.clamp(-1.0, 1.0));
            }
        }
        PcmFormat::F32Le => {
            let (chunks, _) = data[..total_samples * 4].as_chunks::<4>();
            for chunk in chunks {
                let val = f32::from_le_bytes(*chunk);
                if val.is_nan() {
                    return Err(CodecError::InvalidAudio("encountered NaN float sample".into()));
                }
                samples.push(val.clamp(-1.0, 1.0));
            }
        }
        PcmFormat::F32Be => {
            let (chunks, _) = data[..total_samples * 4].as_chunks::<4>();
            for chunk in chunks {
                let val = f32::from_be_bytes(*chunk);
                if val.is_nan() {
                    return Err(CodecError::InvalidAudio("encountered NaN float sample".into()));
                }
                samples.push(val.clamp(-1.0, 1.0));
            }
        }
    }

    AudioFramePCM::new(sample_rate, channels, pts_ms, samples)
}
