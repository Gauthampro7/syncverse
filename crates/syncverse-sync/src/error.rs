//! Strongly typed error enums for network synchronization.

use thiserror::Error;

/// Reason an NTP probe sample was rejected by the RFC 5905 filter.
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlierReason {
    /// Sample dispersion or offset exceeded the filter threshold.
    #[error("dispersion exceeded outlier threshold")]
    ExcessiveDispersion,
    /// Sample round-trip time was implausible or non-positive.
    #[error("implausible sample RTT")]
    ImplausibleRtt,
}

/// Reason a two-phase barrier unpause failed.
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarrierFailureKind {
    /// Timed out waiting for all peers to report pre-roll ready.
    #[error("timed out waiting for peer pre-roll readiness")]
    PrepareTimeout,
    /// Target unpause timestamp elapsed before commit could be dispatched.
    #[error("target unpause deadline expired before commit")]
    DeadlineExpired,
    /// Barrier was cancelled by a pause or seek.
    #[error("barrier cancelled")]
    Cancelled,
}

/// Specific cryptographic or room-authentication failure.
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoErrorKind {
    /// Failed to construct ChaCha20-Poly1305 AEAD key.
    #[error("failed to create ChaCha20-Poly1305 unbound key")]
    KeyCreationFailed,
    /// Nonce uniqueness assumption failed.
    #[error("nonce uniqueness assumption failed")]
    NonceUniquenessFailed,
    /// ChaCha20-Poly1305 sealing failed.
    #[error("ChaCha20-Poly1305 encryption failed")]
    EncryptionFailed,
    /// Ciphertext shorter than 16-byte Poly1305 authentication tag.
    #[error("ciphertext too short for Poly1305 tag")]
    CiphertextTooShort,
    /// Poly1305 authentication tag check failed (wrong room password or tampered datagram).
    #[error("ChaCha20-Poly1305 authentication failed: invalid tag or corrupted ciphertext")]
    AuthenticationFailed,
    /// Encrypted packet received on an unencrypted transport.
    #[error("received encrypted packet but room password is not configured")]
    MissingRoomKey,
    /// Plaintext packet received on an encrypted transport.
    #[error("unencrypted packet rejected on encrypted room session")]
    PlaintextRejected,
}

/// Error variants encountered during network clock synchronization.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum SyncError {
    /// Insufficient probe samples in window to compute filtered offset.
    #[error("insufficient NTP probe samples in window")]
    InsufficientSamples,

    /// Outlier sample rejected by RFC 5905 filter.
    #[error("NTP probe sample rejected as outlier: {0}")]
    OutlierRejected(#[from] OutlierReason),

    /// Desynchronization phase error exceeded hard seek threshold.
    #[error("phase error {0} ms exceeded hard seek threshold")]
    HardSeekRequired(f64),

    /// Barrier unpause protocol timeout or pre-roll failure.
    #[error("barrier unpause error: {0}")]
    BarrierFailure(#[from] BarrierFailureKind),

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

    /// An enumerated wire field carried a value outside its defined range.
    #[error("unknown value {value} for wire field {field}")]
    UnknownEnumValue {
        /// Name of the offending field.
        field: &'static str,
        /// Raw value found on the wire.
        value: u8,
    },

    /// A wire string field was not valid UTF-8.
    #[error("invalid UTF-8 in wire field {field} after {valid_up_to} valid bytes")]
    InvalidUtf8 {
        /// Name of the offending field.
        field: &'static str,
        /// Length of the valid UTF-8 prefix.
        valid_up_to: usize,
    },

    /// Underlying socket or I/O error.
    #[error("network I/O error ({kind:?}, os_code={os_code:?})")]
    Io {
        /// Categorized standard library I/O error kind.
        kind: std::io::ErrorKind,
        /// Optional raw OS error code.
        os_code: Option<i32>,
    },

    /// Cryptographic or authentication error.
    #[error("cryptographic error: {0}")]
    Crypto(#[from] CryptoErrorKind),
}

impl From<std::io::Error> for SyncError {
    fn from(err: std::io::Error) -> Self {
        SyncError::Io {
            kind: err.kind(),
            os_code: err.raw_os_error(),
        }
    }
}
