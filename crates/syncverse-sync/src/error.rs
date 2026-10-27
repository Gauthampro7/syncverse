//! Strongly typed error enums for network synchronization.

use thiserror::Error;

/// Error variants encountered during network clock synchronization.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum SyncError {
    /// Insufficient probe samples in window to compute filtered offset.
    #[error("insufficient NTP probe samples in window")]
    InsufficientSamples,

    /// Outlier sample rejected by RFC 5905 filter.
    #[error("NTP probe sample rejected as outlier: {0}")]
    OutlierRejected(String),

    /// Desynchronization phase error exceeded hard seek threshold.
    #[error("phase error {0} ms exceeded hard seek threshold")]
    HardSeekRequired(f64),

    /// Barrier unpause protocol timeout or pre-roll failure.
    #[error("barrier unpause error: {0}")]
    BarrierFailure(String),

    /// Packet header is shorter than 12 bytes.
    #[error("packet buffer too short: {0} bytes (expected at least 12)")]
    PacketTooShort(usize),

    /// Magic bytes did not match SPNT.
    #[error("invalid wire magic: {0:?} (expected b\"SPNT\")")]
    InvalidMagic([u8; 4]),

    /// Packet payload was truncated.
    #[error("payload truncated: expected {expected} bytes, found {actual}")]
    PayloadTruncated {
        /// Expected payload length in bytes.
        expected: usize,
        /// Actual payload bytes available.
        actual: usize,
    },

    /// Packet payload CRC32 checksum mismatch.
    #[error("CRC32 mismatch: expected 0x{expected:08X}, got 0x{actual:08X}")]
    CrcMismatch {
        /// Expected CRC32 checksum from header.
        expected: u32,
        /// Computed CRC32 checksum from payload.
        actual: u32,
    },

    /// Unrecognized packet type byte.
    #[error("unknown packet type: 0x{0:02X}")]
    UnknownPacketType(u8),

    /// Malformed packet payload data.
    #[error("malformed packet payload: {0}")]
    MalformedPayload(String),

    /// Underlying socket or I/O error.
    #[error("network I/O error: {0}")]
    Io(String),
}

impl From<std::io::Error> for SyncError {
    fn from(err: std::io::Error) -> Self {
        SyncError::Io(err.to_string())
    }
}
