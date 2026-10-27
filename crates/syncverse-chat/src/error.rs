//! Strongly typed error enums for smart chat and binary storage.

use thiserror::Error;

/// Error variants encountered during chat log storage, parsing, and search.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ChatError {
    /// Corrupt file header or invalid magic signature.
    #[error("invalid header magic: expected SPCH")]
    InvalidHeaderMagic,

    /// Header CRC32 checksum mismatch.
    #[error("header CRC32 mismatch")]
    HeaderCrcMismatch,

    /// Record framing or length error.
    #[error("malformed record: {0}")]
    MalformedRecord(String),

    /// Checksum verification failure for a candidate record.
    #[error("CRC32 mismatch in record body")]
    CrcMismatch,

    /// Slash-command syntax or argument parsing error.
    #[error("invalid slash command: {0}")]
    InvalidCommand(String),

    /// Underlying I/O error occurred during file operations.
    #[error("I/O error: {0}")]
    Io(String),
}
