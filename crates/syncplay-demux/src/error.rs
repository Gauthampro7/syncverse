//! Strongly typed error enums for container demuxing.

use thiserror::Error;

/// Error variants encountered during EBML VINT decoding.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum VintError {
    /// The input buffer was truncated before all octets of the VINT could be read.
    #[error("unexpected EOF: needed {needed} octets, available {available}")]
    UnexpectedEof {
        /// Number of bytes required.
        needed: usize,
        /// Number of bytes actually available.
        available: usize,
    },

    /// The first octet was 0x00 or leading zeros exceed 8.
    #[error("first octet cannot be 0x00")]
    InvalidZeroPrefix,

    /// The Element ID has all data bits set to 1, which is reserved by RFC 8794 Section 4.1.
    #[error("element ID has all data bits set to 1 (reserved)")]
    ReservedElementId,
}

/// Error variants encountered during container demuxing.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum DemuxError {
    /// Unexpected end-of-file while parsing container structures.
    #[error("unexpected EOF: needed {needed} bytes, available {available}")]
    UnexpectedEof {
        /// Number of bytes needed.
        needed: usize,
        /// Number of bytes available.
        available: usize,
    },

    /// Malformed box header or unsupported box geometry.
    #[error("malformed box: {0}")]
    MalformedBox(String),

    /// Corrupted sample table mapping in ISOBMFF container.
    #[error("corrupt sample table: {0}")]
    CorruptSampleTable(String),

    /// VINT decoding error in EBML stream.
    #[error("EBML VINT error: {0}")]
    Vint(#[from] VintError),

    /// Integer arithmetic overflow occurred during timestamp or offset calculation.
    #[error("integer overflow occurred")]
    IntegerOverflow,

    /// Invalid container header or unrecognized magic signature.
    #[error("invalid container header: {0}")]
    InvalidHeader(String),

    /// Underlying I/O error occurred during stream reading.
    #[error("I/O error: {0}")]
    Io(String),

    /// Requested track or seek point not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// Corrupted block framing or invalid lacing.
    #[error("corrupted block framing: {0}")]
    CorruptedBlock(String),
}
