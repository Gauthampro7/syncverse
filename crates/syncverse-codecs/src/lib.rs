//! Pure-Rust video and audio bitstream decoders, frame representations, and Q8 color converter.

#![deny(warnings)]
#![deny(missing_docs)]

pub mod audio;
pub mod color;
pub mod error;
pub mod frame;
pub mod h264;
pub mod universal_video;

pub use audio::{
    unpack_opus_packet, unpack_pcm, unpack_vorbis_audio_packet, AudioCodec, AudioDecoder,
    LinearResampler, OpusBandwidth, OpusMode, OpusToc, PcmFormat, UniversalAudioDecoder,
    UniversalAudioError, VorbisIdentificationHeader,
};
pub use color::{
    clamp_u8, convert_rgba_to_terminal, convert_yuv420p_to_rgba, convert_yuv420p_to_terminal,
    yuv_to_rgb_bt601, yuv_to_rgb_bt709, Bt601Converter, Bt709Converter, ColorSpaceConverter,
    ColorStandard, TerminalColorCell, TerminalTrueColorBuffer,
};
pub use error::CodecError;
pub use frame::{AlignedBuffer, AudioFramePCM, VideoFrameRGBA, VideoFrameYUV420p};
pub use h264::{
    extract_nal_units, parse_annex_b, parse_avcc, unescape_rbsp, AnnexBScanner, H264Decoder,
    NalHeader, NalUnitType, Pps, SliceHeader, SliceType, Sps,
};
pub use universal_video::UniversalVideoDecoder;

