//! Pure-Rust audio packet demuxing and payload unpacking (PCM, Opus, Vorbis).

pub mod decoder;
pub mod opus;
pub mod pcm;
pub mod resampler;
pub mod universal;
pub mod vorbis;

pub use decoder::{AudioCodec, AudioDecoder};
pub use opus::{unpack_opus_packet, OpusBandwidth, OpusMode, OpusToc};
pub use pcm::{unpack_pcm, PcmFormat};
pub use resampler::LinearResampler;
pub use universal::{UniversalAudioDecoder, UniversalAudioError};
pub use vorbis::{unpack_vorbis_audio_packet, VorbisIdentificationHeader};


