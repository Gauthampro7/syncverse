//! Minimal RFC 8489 STUN Binding client used to discover the public UDP endpoint.
//!
//! Only the Binding method is implemented: a 20-byte request without attributes, and a response
//! decoder for `XOR-MAPPED-ADDRESS` (IPv4 and IPv6) with the legacy `MAPPED-ADDRESS` as fallback.
//! `MESSAGE-INTEGRITY` and `FINGERPRINT` attributes in responses are skipped, not verified
//! (public servers do not require authentication for Binding, and a forged response can only
//! mislead the displayed address, never the room traffic).
//!
//! # Privacy
//!
//! Nothing in this module runs by itself. A third-party STUN server is contacted only when the
//! caller invokes [`binding_request`] (or the helpers in [`crate::nat`]), which the UI does only
//! after the user asks for "Detect public address". [`DEFAULT_STUN_SERVERS`] is just a list of
//! defaults the caller may use; the caller can pass any other servers.
//!
//! # Socket choice
//!
//! A NAT gives a different public port to each local socket. To learn the endpoint a peer must
//! use for the room, run the request on the room's own UDP socket.
//! [`binding_request`] reads from the socket it is given, so use it only when nobody else is
//! reading that socket; otherwise use [`crate::nat::NatMux`], which receives STUN responses
//! through the transport's datagram interceptor.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

use ring::rand::{SecureRandom, SystemRandom};

/// STUN magic cookie (RFC 8489 section 5).
pub const STUN_MAGIC_COOKIE: u32 = 0x2112_A442;

/// Length of the fixed STUN header in bytes.
pub const STUN_HEADER_LEN: usize = 20;

/// Default STUN servers (two, so that mapping behaviour can be classified).
///
/// These are only used when the user explicitly asks to detect the public address.
pub const DEFAULT_STUN_SERVERS: [&str; 2] = ["stun.l.google.com:19302", "stun.cloudflare.com:3478"];

const MSG_BINDING_REQUEST: u16 = 0x0001;
const MSG_BINDING_SUCCESS: u16 = 0x0101;
const MSG_BINDING_ERROR: u16 = 0x0111;
const ATTR_MAPPED_ADDRESS: u16 = 0x0001;
const ATTR_ERROR_CODE: u16 = 0x0009;
const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;
const FAMILY_V4: u8 = 0x01;
const FAMILY_V6: u8 = 0x02;

/// Errors from STUN encoding, decoding and network exchange.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum StunError {
    /// Datagram is shorter than the 20-byte header.
    #[error("STUN message shorter than the 20-byte header")]
    TooShort,
    /// The two most significant bits of the type are not zero, so this is not STUN.
    #[error("not a STUN message (leading bits are not zero)")]
    NotStun,
    /// Magic cookie field does not hold `0x2112A442`.
    #[error("STUN magic cookie mismatch")]
    BadMagicCookie,
    /// Header length field disagrees with the datagram size or is not a multiple of four.
    #[error("STUN length field does not match datagram size")]
    LengthMismatch,
    /// An attribute header or value runs past the end of the message.
    #[error("STUN attribute is truncated")]
    TruncatedAttribute,
    /// Message type is not a Binding success or error response.
    #[error("unexpected STUN message type {0:#06x}")]
    UnexpectedMessageType(u16),
    /// Response transaction id differs from the request.
    #[error("STUN transaction id mismatch")]
    TransactionMismatch,
    /// Response carried no (X)MAPPED-ADDRESS attribute.
    #[error("STUN response has no mapped address")]
    NoMappedAddress,
    /// Address attribute has an unknown family byte.
    #[error("unknown STUN address family {0:#04x}")]
    BadAddressFamily(u8),
    /// Address attribute length does not fit its family.
    #[error("STUN address attribute has wrong length")]
    BadAttributeLength,
    /// Server answered with a Binding error response.
    #[error("STUN server returned error {code}")]
    ErrorResponse {
        /// Error code (class * 100 + number), 0 if the attribute was absent.
        code: u16,
    },
    /// No valid response arrived within the retry budget.
    #[error("no response from STUN server")]
    Timeout,
    /// The server name did not resolve to a socket address of a usable family.
    #[error("STUN server name did not resolve")]
    Resolve,
    /// The operating system random generator failed.
    #[error("system random generator failed")]
    Random,
    /// Socket operation failed.
    #[error("socket error ({0:?})")]
    Io(io::ErrorKind),
}

impl From<io::Error> for StunError {
    fn from(e: io::Error) -> Self {
        StunError::Io(e.kind())
    }
}

/// A 96-bit STUN transaction id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransactionId(pub [u8; 12]);

impl TransactionId {
    /// Draws a fresh id from the operating system CSPRNG (`ring::rand::SystemRandom`).
    pub fn random() -> Result<Self, StunError> {
        let mut id = [0u8; 12];
        SystemRandom::new()
            .fill(&mut id)
            .map_err(|_| StunError::Random)?;
        Ok(Self(id))
    }
}

/// Retransmission policy for a Binding request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StunConfig {
    /// Time to wait for a response after each transmission (RFC 8489 RTO, default 500 ms).
    pub rto: Duration,
    /// Number of transmissions before giving up (default 4).
    pub attempts: u32,
}

impl Default for StunConfig {
    fn default() -> Self {
        Self {
            rto: Duration::from_millis(500),
            attempts: 4,
        }
    }
}

/// Encodes a Binding request (20 bytes, no attributes) for `txid`.
pub fn encode_binding_request(txid: &TransactionId) -> [u8; STUN_HEADER_LEN] {
    let mut out = [0u8; STUN_HEADER_LEN];
    out[0..2].copy_from_slice(&MSG_BINDING_REQUEST.to_be_bytes());
    // length stays 0
    out[4..8].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
    out[8..20].copy_from_slice(&txid.0);
    out
}

/// Cheap check used by datagram interceptors: does this datagram look like STUN?
///
/// True when it is at least a full header, the top two bits are zero and the magic cookie
/// matches. Room packets are encrypted and fail this check with probability 1 - 2^-34.
pub fn looks_like_stun(data: &[u8]) -> bool {
    data.len() >= STUN_HEADER_LEN
        && data[0] & 0xC0 == 0
        && data[4..8] == STUN_MAGIC_COOKIE.to_be_bytes()
}

/// Returns the transaction id of a datagram that [`looks_like_stun`].
pub fn peek_transaction_id(data: &[u8]) -> Option<TransactionId> {
    if !looks_like_stun(data) {
        return None;
    }
    let mut id = [0u8; 12];
    id.copy_from_slice(&data[8..20]);
    Some(TransactionId(id))
}

/// Decodes a Binding response and returns the mapped (public) socket address.
///
/// If `expected` is given, a different transaction id is rejected with
/// [`StunError::TransactionMismatch`]. `XOR-MAPPED-ADDRESS` is preferred; the legacy
/// `MAPPED-ADDRESS` is accepted when the former is missing. Unknown attributes are skipped.
pub fn decode_binding_response(
    data: &[u8],
    expected: Option<&TransactionId>,
) -> Result<SocketAddr, StunError> {
    if data.len() < STUN_HEADER_LEN {
        return Err(StunError::TooShort);
    }
    if data[0] & 0xC0 != 0 {
        return Err(StunError::NotStun);
    }
    if data[4..8] != STUN_MAGIC_COOKIE.to_be_bytes() {
        return Err(StunError::BadMagicCookie);
    }
    let msg_type = u16::from_be_bytes([data[0], data[1]]);
    let len = usize::from(u16::from_be_bytes([data[2], data[3]]));
    if len % 4 != 0 || STUN_HEADER_LEN + len != data.len() {
        return Err(StunError::LengthMismatch);
    }
    let mut txid = [0u8; 12];
    txid.copy_from_slice(&data[8..20]);
    if let Some(exp) = expected {
        if exp.0 != txid {
            return Err(StunError::TransactionMismatch);
        }
    }
    if msg_type != MSG_BINDING_SUCCESS && msg_type != MSG_BINDING_ERROR {
        return Err(StunError::UnexpectedMessageType(msg_type));
    }

    let mut xor_addr: Option<SocketAddr> = None;
    let mut plain_addr: Option<SocketAddr> = None;
    let mut error_code: Option<u16> = None;
    let mut pos = STUN_HEADER_LEN;
    while pos < data.len() {
        if pos + 4 > data.len() {
            return Err(StunError::TruncatedAttribute);
        }
        let attr_type = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let attr_len = usize::from(u16::from_be_bytes([data[pos + 2], data[pos + 3]]));
        let value_start = pos + 4;
        let value_end = value_start + attr_len;
        if value_end > data.len() {
            return Err(StunError::TruncatedAttribute);
        }
        let value = &data[value_start..value_end];
        match attr_type {
            ATTR_XOR_MAPPED_ADDRESS => xor_addr = Some(decode_address(value, Some(&txid))?),
            ATTR_MAPPED_ADDRESS => plain_addr = Some(decode_address(value, None)?),
            ATTR_ERROR_CODE if value.len() >= 4 => {
                error_code = Some(u16::from(value[2] & 0x07) * 100 + u16::from(value[3]));
            }
            _ => {}
        }
        // Values are padded to a multiple of four bytes.
        pos = value_start + attr_len.div_ceil(4) * 4;
    }

    if msg_type == MSG_BINDING_ERROR {
        return Err(StunError::ErrorResponse {
            code: error_code.unwrap_or(0),
        });
    }
    xor_addr.or(plain_addr).ok_or(StunError::NoMappedAddress)
}

/// Decodes one (XOR-)MAPPED-ADDRESS value; `xor_txid` is `Some` for the XOR variant.
fn decode_address(value: &[u8], xor_txid: Option<&[u8; 12]>) -> Result<SocketAddr, StunError> {
    if value.len() < 4 {
        return Err(StunError::BadAttributeLength);
    }
    let family = value[1];
    let mut port = u16::from_be_bytes([value[2], value[3]]);
    if xor_txid.is_some() {
        port ^= (STUN_MAGIC_COOKIE >> 16) as u16;
    }
    let cookie = STUN_MAGIC_COOKIE.to_be_bytes();
    match family {
        FAMILY_V4 => {
            if value.len() != 8 {
                return Err(StunError::BadAttributeLength);
            }
            let mut ip = [value[4], value[5], value[6], value[7]];
            if xor_txid.is_some() {
                for (b, c) in ip.iter_mut().zip(cookie.iter()) {
                    *b ^= c;
                }
            }
            Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::from(ip)), port))
        }
        FAMILY_V6 => {
            if value.len() != 20 {
                return Err(StunError::BadAttributeLength);
            }
            let mut ip = [0u8; 16];
            ip.copy_from_slice(&value[4..20]);
            if let Some(txid) = xor_txid {
                let mut mask = [0u8; 16];
                mask[..4].copy_from_slice(&cookie);
                mask[4..].copy_from_slice(txid);
                for (b, m) in ip.iter_mut().zip(mask.iter()) {
                    *b ^= m;
                }
            }
            Ok(SocketAddr::new(IpAddr::V6(Ipv6Addr::from(ip)), port))
        }
        other => Err(StunError::BadAddressFamily(other)),
    }
}

/// Encodes a Binding success response carrying `XOR-MAPPED-ADDRESS`.
///
/// Used by in-tree test servers and simulators; real servers are third-party.
pub fn encode_binding_success(txid: &TransactionId, mapped: SocketAddr) -> Vec<u8> {
    let mut attr = vec![
        0u8,
        if mapped.is_ipv4() {
            FAMILY_V4
        } else {
            FAMILY_V6
        },
    ];
    attr.extend_from_slice(&(mapped.port() ^ (STUN_MAGIC_COOKIE >> 16) as u16).to_be_bytes());
    let cookie = STUN_MAGIC_COOKIE.to_be_bytes();
    match mapped.ip() {
        IpAddr::V4(v4) => {
            for (b, c) in v4.octets().iter().zip(cookie.iter()) {
                attr.push(b ^ c);
            }
        }
        IpAddr::V6(v6) => {
            let mut mask = [0u8; 16];
            mask[..4].copy_from_slice(&cookie);
            mask[4..].copy_from_slice(&txid.0);
            for (b, m) in v6.octets().iter().zip(mask.iter()) {
                attr.push(b ^ m);
            }
        }
    }
    let mut out = Vec::with_capacity(STUN_HEADER_LEN + 4 + attr.len());
    out.extend_from_slice(&MSG_BINDING_SUCCESS.to_be_bytes());
    out.extend_from_slice(&((4 + attr.len()) as u16).to_be_bytes());
    out.extend_from_slice(&cookie);
    out.extend_from_slice(&txid.0);
    out.extend_from_slice(&ATTR_XOR_MAPPED_ADDRESS.to_be_bytes());
    out.extend_from_slice(&(attr.len() as u16).to_be_bytes());
    out.extend_from_slice(&attr);
    out
}

/// Resolves a `host:port` STUN server name to an address of the same family as `local`
/// (an IPv4-bound socket cannot reach an IPv6 server and vice versa).
pub fn resolve_server(server: &str, local: &SocketAddr) -> Result<SocketAddr, StunError> {
    server
        .to_socket_addrs()
        .map_err(|_| StunError::Resolve)?
        .find(|a| a.is_ipv4() == local.is_ipv4())
        .ok_or(StunError::Resolve)
}

/// Sends a Binding request from `socket` to `server` and returns the mapped address.
///
/// Retransmits every `cfg.rto` up to `cfg.attempts` times (default 500 ms x 4). Datagrams from
/// other sources, non-STUN datagrams and responses with a foreign transaction id are ignored
/// while waiting (they are consumed, so do not use this on a socket another thread reads).
/// The socket's read timeout is restored before returning.
///
/// # Errors
/// [`StunError::Timeout`] when no valid response arrives; [`StunError::ErrorResponse`] when the
/// server rejects the request; [`StunError::Io`] for socket failures.
pub fn binding_request(
    socket: &UdpSocket,
    server: SocketAddr,
    cfg: &StunConfig,
) -> Result<SocketAddr, StunError> {
    let txid = TransactionId::random()?;
    let request = encode_binding_request(&txid);
    let previous = socket.read_timeout()?;
    let result = binding_exchange(socket, server, cfg, &txid, &request);
    // Best effort: failing to restore must not mask the real result.
    let _ = socket.set_read_timeout(previous);
    result
}

fn binding_exchange(
    socket: &UdpSocket,
    server: SocketAddr,
    cfg: &StunConfig,
    txid: &TransactionId,
    request: &[u8],
) -> Result<SocketAddr, StunError> {
    let mut buf = [0u8; 1500];
    for _ in 0..cfg.attempts.max(1) {
        socket.send_to(request, server)?;
        let deadline = Instant::now() + cfg.rto;
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            socket.set_read_timeout(Some((deadline - now).max(Duration::from_millis(1))))?;
            match socket.recv_from(&mut buf) {
                Ok((n, from)) => {
                    if from != server {
                        continue;
                    }
                    match decode_binding_response(&buf[..n], Some(txid)) {
                        Ok(addr) => return Ok(addr),
                        Err(e @ StunError::ErrorResponse { .. }) => return Err(e),
                        Err(_) => continue,
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    break
                }
                // Linux reports ICMP port-unreachable of an earlier send as ConnectionRefused.
                Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => continue,
                Err(e) => return Err(e.into()),
            }
        }
    }
    Err(StunError::Timeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_v4_and_v6() {
        let txid = TransactionId([7; 12]);
        for addr in ["203.0.113.9:40000", "[2001:db8::5]:1234"] {
            let a: SocketAddr = addr.parse().unwrap();
            let msg = encode_binding_success(&txid, a);
            assert_eq!(decode_binding_response(&msg, Some(&txid)), Ok(a));
        }
    }

    #[test]
    fn request_is_20_bytes_with_cookie() {
        let txid = TransactionId::random().unwrap();
        let r = encode_binding_request(&txid);
        assert!(looks_like_stun(&r));
        assert_eq!(peek_transaction_id(&r), Some(txid));
    }

    #[test]
    fn random_ids_differ() {
        assert_ne!(
            TransactionId::random().unwrap(),
            TransactionId::random().unwrap()
        );
    }
}
