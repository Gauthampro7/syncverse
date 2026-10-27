//! Common error types across syncplay-alt crates.

use thiserror::Error;

/// Core error enumeration for byte manipulation, reader streams, and foundational operations.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum CommonError {
    /// Unexpected end-of-file or slice boundary reached.
    #[error("unexpected EOF: needed {needed} bytes, but only {available} bytes available")]
    UnexpectedEof {
        /// Number of bytes required.
        needed: usize,
        /// Number of bytes actually available.
        available: usize,
    },

    /// Invalid data or framing detected.
    #[error("invalid data: {0}")]
    InvalidData(String),

    /// Requested offset or index is out of bounds.
    #[error("out of bounds: requested offset {requested}, but maximum is {max}")]
    OutOfBounds {
        /// Requested byte offset.
        requested: usize,
        /// Maximum valid index.
        max: usize,
    },
}
