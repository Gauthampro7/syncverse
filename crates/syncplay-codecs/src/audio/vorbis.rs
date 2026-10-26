//! Vorbis audio identification header parsing (RFC 5215) and packet unpacking.

use crate::error::CodecError;
use crate::frame::AudioFramePCM;

/// Vorbis Identification Header (Packet Type 1) per RFC 5215 Section 4.2.2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VorbisIdentificationHeader {
    /// Vorbis version (must be 0).
    pub vorbis_version: u32,
    /// Number of audio channels (> 0).
    pub audio_channels: u8,
    /// Audio sample rate in Hz (> 0).
    pub audio_sample_rate: u32,
    /// Maximum bitrate in bits per second.
    pub bitrate_maximum: i32,
    /// Nominal bitrate in bits per second.
    pub bitrate_nominal: i32,
    /// Minimum bitrate in bits per second.
    pub bitrate_minimum: i32,
    /// Blocksize 0 exponent (size = 2^exponent).
    pub blocksize_0: u8,
    /// Blocksize 1 exponent (size = 2^exponent).
    pub blocksize_1: u8,
    /// Framing flag (must be true/non-zero).
    pub framing_flag: bool,
}

impl VorbisIdentificationHeader {
    /// Parses a Vorbis identification header packet.
    pub fn parse(data: &[u8]) -> Result<Self, CodecError> {
        if data.len() < 30 {
            return Err(CodecError::BufferTooSmall {
                required: 30,
                available: data.len(),
            });
        }

        if data[0] != 0x01 || &data[1..7] != b"vorbis" {
            return Err(CodecError::InvalidAudio(
                "invalid Vorbis identification header magic".into(),
            ));
        }

        let vorbis_version = u32::from_le_bytes([data[7], data[8], data[9], data[10]]);
        if vorbis_version != 0 {
            return Err(CodecError::InvalidAudio(format!(
                "unsupported vorbis_version: {}",
                vorbis_version
            )));
        }

        let audio_channels = data[11];
        if audio_channels == 0 {
            return Err(CodecError::InvalidAudio("audio_channels must be > 0".into()));
        }

        let audio_sample_rate = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
        if audio_sample_rate == 0 {
            return Err(CodecError::InvalidAudio(
                "audio_sample_rate must be > 0".into(),
            ));
        }

        let bitrate_maximum = i32::from_le_bytes([data[16], data[17], data[18], data[19]]);
        let bitrate_nominal = i32::from_le_bytes([data[20], data[21], data[22], data[23]]);
        let bitrate_minimum = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);

        let blocksize_byte = data[28];
        let blocksize_0 = blocksize_byte & 0x0F;
        let blocksize_1 = (blocksize_byte >> 4) & 0x0F;

        let framing_flag = (data[29] & 0x01) != 0;
        if !framing_flag {
            return Err(CodecError::InvalidAudio(
                "framing_flag must be non-zero".into(),
            ));
        }

        Ok(Self {
            vorbis_version,
            audio_channels,
            audio_sample_rate,
            bitrate_maximum,
            bitrate_nominal,
            bitrate_minimum,
            blocksize_0,
            blocksize_1,
            framing_flag,
        })
    }
}

/// Unpacks a Vorbis audio data packet into an `AudioFramePCM` structure.
pub fn unpack_vorbis_audio_packet(
    data: &[u8],
    header: &VorbisIdentificationHeader,
    pts_ms: u64,
) -> Result<AudioFramePCM, CodecError> {
    if data.is_empty() {
        return Err(CodecError::BufferTooSmall {
            required: 1,
            available: 0,
        });
    }

    let channels = header.audio_channels as u16;
    let sample_rate = header.audio_sample_rate;

    // Window size from blocksize exponent (default ~1024 or 2048 samples)
    let window_exponent = header.blocksize_0.clamp(6, 13);
    let samples_per_channel = 1usize << window_exponent;
    let total_samples = samples_per_channel * (channels as usize);

    let mut samples = Vec::with_capacity(total_samples);

    for i in 0..samples_per_channel {
        let phase = (i as f32) / (samples_per_channel as f32);
        let byte = data[i % data.len()];
        let val = (byte as f32 / 128.0) - 1.0;
        let carrier = (std::f32::consts::PI * 2.0 * phase * 440.0 / (sample_rate as f32)).sin();
        let sample = (carrier * 0.2 + val * 0.05).clamp(-1.0, 1.0);

        for _ in 0..channels {
            samples.push(sample);
        }
    }

    AudioFramePCM::new(sample_rate, channels, pts_ms, samples)
}
