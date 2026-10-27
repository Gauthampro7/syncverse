//! Error types for codec representations and color conversion.

use thiserror::Error;

/// Error variants encountered during codec operations.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// Dimension mismatch between source and destination frames.
    #[error("dimension mismatch: source is {src_w}x{src_h}, destination is {dst_w}x{dst_h}")]
    DimensionMismatch {
        /// Source width.
        src_w: u32,
        /// Source height.
        src_h: u32,
        /// Destination width.
        dst_w: u32,
        /// Destination height.
        dst_h: u32,
    },

    /// Audio frame channel count or sample rate error.
    #[error("invalid audio parameters: {0}")]
    InvalidAudio(String),

    /// Buffer capacity is insufficient for the requested operation.
    #[error("buffer too small: required {required} bytes, available {available} bytes")]
    BufferTooSmall {
        /// Bytes required.
        required: usize,
        /// Bytes available.
        available: usize,
    },

    /// Invalid or corrupted NAL unit syntax.
    #[error("invalid NAL unit: {0}")]
    InvalidNal(String),

    /// Invalid or unsupported Sequence Parameter Set (SPS).
    #[error("invalid SPS: {0}")]
    InvalidSps(String),

    /// Invalid or unsupported Picture Parameter Set (PPS).
    #[error("invalid PPS: {0}")]
    InvalidPps(String),

    /// Invalid or unsupported slice header.
    #[error("invalid slice: {0}")]
    InvalidSlice(String),

    /// Decoder initialization or operational error from FFmpeg backend.
    #[error("decoder error: {0}")]
    Decoder(String),

    /// Unsupported codec format or configuration.
    #[error("unsupported codec: {0}")]
    UnsupportedCodec(String),

    /// Underlying bitstream or common reader error.
    #[error("common error: {0}")]
    Common(#[from] syncverse_common::error::CommonError),
}

impl From<ffmpeg_next::Error> for CodecError {
    fn from(err: ffmpeg_next::Error) -> Self {
        CodecError::Decoder(err.to_string())
    }
}

