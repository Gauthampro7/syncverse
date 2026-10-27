//! Unified audio packet decoder and payload unpacker.

use crate::audio::opus::unpack_opus_packet;
use crate::audio::pcm::{unpack_pcm, PcmFormat};
use crate::audio::vorbis::{unpack_vorbis_audio_packet, VorbisIdentificationHeader};
use crate::error::CodecError;
use crate::frame::AudioFramePCM;

/// Supported audio codec types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioCodec {
    /// Uncompressed PCM format.
    Pcm(PcmFormat),
    /// Opus interactive audio packet.
    Opus,
    /// Vorbis audio stream with identification header.
    Vorbis(VorbisIdentificationHeader),
}

/// Unified audio packet decoder producing `AudioFramePCM` frames.
#[derive(Debug, Clone)]
pub struct AudioDecoder {
    codec: AudioCodec,
    sample_rate: u32,
    channels: u16,
}

impl AudioDecoder {
    /// Constructs a new `AudioDecoder`.
    pub fn new(codec: AudioCodec, sample_rate: u32, channels: u16) -> Self {
        Self {
            codec,
            sample_rate,
            channels,
        }
    }

    /// Decodes an audio packet into an `AudioFramePCM`.
    pub fn decode_packet(&mut self, data: &[u8], pts_ms: u64) -> Result<AudioFramePCM, CodecError> {
        match &self.codec {
            AudioCodec::Pcm(format) => {
                unpack_pcm(data, *format, self.channels, self.sample_rate, pts_ms)
            }
            AudioCodec::Opus => unpack_opus_packet(data, self.sample_rate, pts_ms),
            AudioCodec::Vorbis(header) => unpack_vorbis_audio_packet(data, header, pts_ms),
        }
    }

    /// Returns the active audio sampling rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Returns the channel count.
    pub fn channels(&self) -> u16 {
        self.channels
    }
}
