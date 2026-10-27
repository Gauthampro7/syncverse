//! Error types for master clock and ring buffer.

use thiserror::Error;

/// Error returned when attempting to push into a full ring buffer.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[error("ring buffer capacity full")]
pub struct BufferFullError<T> {
    /// The item that failed to be pushed into the ring buffer.
    pub item: T,
}

impl<T> BufferFullError<T> {
    /// Constructs a new `BufferFullError` holding the unpushed item.
    pub const fn new(item: T) -> Self {
        Self { item }
    }
}

/// Error variants encountered during clock and presentation operations.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ClockError {
    /// Invalid capacity specified for data structure.
    #[error("invalid capacity: {0}")]
    InvalidCapacity(String),

    /// Clock rate or anchor specification error.
    #[error("clock error: {0}")]
    InvalidRate(String),

    /// Audio pacing or buffer error.
    #[error("audio pacing error: {0}")]
    AudioPacing(String),

    /// Codec error during frame construction.
    #[error("codec error: {0}")]
    Codec(#[from] syncverse_codecs::CodecError),
}
