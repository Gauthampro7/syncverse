//! NAT mapping classification and UDP hole punching built on top of [`crate::stun`].
//!
//! # What this does
//!
//! * [`NatMux`] sits on the room's UDP socket. It recognises STUN responses and `SVPUNCH1`
//!   datagrams, consumes them, and routes them to whoever is waiting. Everything else is left
//!   to the normal room packet parser. It is plugged in through the transport's datagram
//!   interceptor ([`NatMux::interceptor`]); for a socket nobody else reads, call
//!   [`NatMux::spawn_reader`].
//! * [`NatMux::detect`] asks two STUN servers from the same socket and classifies the mapping:
//!   the same public port from both servers is [`NatMapping::EndpointIndependent`] (hole
//!   punching can work), different ports is [`NatMapping::Symmetric`] (it cannot; use UPnP,
//!   NAT-PMP or manual port-forwarding).
//! * [`NatMux::punch`] is the hole puncher. Both peers call it with the other side's public
//!   endpoint at about the same time. Each sends `SVPUNCH1` probes every 50 ms for at most
//!   5 s and answers probes from the peer; it returns [`PunchOutcome::Established`] once an
//!   acknowledgement of its own probe arrives from the peer address.
//! * [`NatMux::start_keepalive`] refreshes the NAT mapping every 15 s.
//!
//! # Limits
//!
//! Punching needs both sides behind endpoint-independent mappings. If both sides are behind
//! symmetric NAT or CGNAT it fails and a relay would be needed; Syncverse ships none.
//! Behaviour on real routers is not verified by the in-tree tests, which use loopback
//! simulators of port-restricted NAT.
//!
//! # Wire format of punch datagrams
//!
//! `"SVPUNCH1"` (8 bytes) | kind (1 byte: 0 probe, 1 ack, 2 keep-alive) | 8-byte value.
//! For a probe the value is the sender's random nonce, for an ack it echoes the nonce of the
//! probe being acknowledged, for a keep-alive it is zero. Probes are answered only for peers
//! the caller has allowed (by calling [`NatMux::punch`] or [`NatMux::allow_peer`]), so a
//! random host on the Internet gets no reply from the room socket.

use std::collections::{HashMap, HashSet};
use std::io;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ring::rand::{SecureRandom, SystemRandom};

use crate::stun::{
    decode_binding_response, encode_binding_request, looks_like_stun, peek_transaction_id,
    resolve_server, StunConfig, StunError, TransactionId,
};

/// Magic prefix of punch datagrams.
pub const PUNCH_MAGIC: &[u8; 8] = b"SVPUNCH1";

/// Length of every punch datagram: magic + kind + 8-byte value.
pub const PUNCH_DATAGRAM_LEN: usize = 17;

/// Interval between mapping keep-alive datagrams.
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

const KIND_PROBE: u8 = 0;
const KIND_ACK: u8 = 1;
const KIND_KEEPALIVE: u8 = 2;

/// How the NAT maps one local socket towards different destinations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatMapping {
    /// Not determined (no or only one STUN answer).
    Unknown,
    /// The socket already has the public address: no NAT in between.
    Open,
    /// Same public endpoint towards every server: hole punching is possible.
    EndpointIndependent,
    /// Public endpoint depends on the destination: hole punching will not work.
    Symmetric,
}

impl NatMapping {
    /// Whether UDP hole punching can be expected to work with this mapping on our side.
    pub fn punchable(self) -> bool {
        matches!(self, NatMapping::Open | NatMapping::EndpointIndependent)
    }

    /// One-line explanation for the UI.
    pub fn describe(self) -> &'static str {
        match self {
            NatMapping::Unknown => "NAT type unknown",
            NatMapping::Open => "No NAT: this machine is directly reachable",
            NatMapping::EndpointIndependent => {
                "Cone NAT: hole punching should work if the other side is also a cone NAT"
            }
            NatMapping::Symmetric => {
                "Symmetric NAT: hole punching will not work, use UPnP or port-forwarding"
            }
        }
    }
}

/// Coarse failure class of a STUN probe, matching the UI's error categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeFailure {
    /// No server answered.
    NoResponse,
    /// A server answered with something unusable.
    BadResponse,
    /// Local socket or name-resolution failure.
    Socket,
}

/// Maps a [`StunError`] onto the coarse [`ProbeFailure`] classes.
pub fn probe_failure(err: &StunError) -> ProbeFailure {
    match err {
        StunError::Timeout => ProbeFailure::NoResponse,
        StunError::Resolve | StunError::Random | StunError::Io(_) => ProbeFailure::Socket,
        _ => ProbeFailure::BadResponse,
    }
}

/// Result of [`NatMux::detect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NatReport {
    /// Mapping classification.
    pub mapping: NatMapping,
    /// Public endpoint as seen by the first answering server, if any.
    pub public_addr: Option<SocketAddr>,
    /// Public endpoint as seen by the second server, if it answered.
    pub second_addr: Option<SocketAddr>,
    /// First error seen, if the report is incomplete.
    pub error: Option<StunError>,
}

/// Classifies two mapped endpoints observed for one local socket.
///
/// `local` is the socket's own address when known; equal to the mapped address means no NAT.
pub fn classify_mapping(
    local: Option<SocketAddr>,
    first: SocketAddr,
    second: SocketAddr,
) -> NatMapping {
    if first.ip() != second.ip() || first.port() != second.port() {
        return NatMapping::Symmetric;
    }
    if local == Some(first) {
        NatMapping::Open
    } else {
        NatMapping::EndpointIndependent
    }
}

/// Timing of the hole puncher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PunchConfig {
    /// Gap between probes (default 50 ms).
    pub interval: Duration,
    /// Give up after this long (default 5 s).
    pub timeout: Duration,
}

impl Default for PunchConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_millis(50),
            timeout: Duration::from_secs(5),
        }
    }
}

/// Outcome of [`NatMux::punch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PunchOutcome {
    /// The peer acknowledged one of our probes: packets flow both ways to this address.
    Established(SocketAddr),
    /// No acknowledgement within the time limit.
    Timeout,
}

struct PunchWait {
    nonce: u64,
    tx: Sender<()>,
}

struct StunWait {
    server: SocketAddr,
    tx: Sender<Result<SocketAddr, StunError>>,
}

#[derive(Default)]
struct MuxState {
    stun: HashMap<TransactionId, StunWait>,
    punch: HashMap<SocketAddr, PunchWait>,
    allowed: HashSet<SocketAddr>,
}

struct MuxInner {
    socket: Arc<UdpSocket>,
    state: Mutex<MuxState>,
    consumed: AtomicU64,
}

impl MuxInner {
    fn state(&self) -> MutexGuard<'_, MuxState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Demultiplexer for STUN and punch datagrams on one UDP socket. Cheap to clone.
#[derive(Clone)]
pub struct NatMux {
    inner: Arc<MuxInner>,
}

fn random_u64() -> u64 {
    let mut b = [0u8; 8];
    // A failing system RNG is practically impossible; a zero nonce would only weaken matching.
    let _ = SystemRandom::new().fill(&mut b);
    u64::from_be_bytes(b)
}

fn punch_datagram(kind: u8, value: u64) -> [u8; PUNCH_DATAGRAM_LEN] {
    let mut out = [0u8; PUNCH_DATAGRAM_LEN];
    out[..8].copy_from_slice(PUNCH_MAGIC);
    out[8] = kind;
    out[9..].copy_from_slice(&value.to_be_bytes());
    out
}

/// The keep-alive datagram (consumed by the peer's [`NatMux`], ignored by the wire parser).
///
/// Exposed so the room heartbeat can double as mapping keep-alive if the caller prefers.
pub fn keepalive_datagram() -> [u8; PUNCH_DATAGRAM_LEN] {
    punch_datagram(KIND_KEEPALIVE, 0)
}

/// Whether a datagram carries the `SVPUNCH1` magic.
pub fn looks_like_punch(data: &[u8]) -> bool {
    data.len() >= PUNCH_MAGIC.len() && &data[..8] == PUNCH_MAGIC
}

impl NatMux {
    /// Wraps a socket. The socket is shared, not owned exclusively.
    pub fn new(socket: Arc<UdpSocket>) -> Self {
        Self {
            inner: Arc::new(MuxInner {
                socket,
                state: Mutex::new(MuxState::default()),
                consumed: AtomicU64::new(0),
            }),
        }
    }

    /// Wraps a duplicate handle of `socket` (same underlying socket, same NAT mapping).
    pub fn from_socket(socket: &UdpSocket) -> io::Result<Self> {
        Ok(Self::new(Arc::new(socket.try_clone()?)))
    }

    /// Number of STUN and punch datagrams consumed so far.
    pub fn consumed_count(&self) -> u64 {
        self.inner.consumed.load(Ordering::Relaxed)
    }

    /// Builds the closure for the transport's datagram interceptor
    /// (`FnMut(&[u8], SocketAddr) -> bool`, true = consumed, not parsed as a room packet).
    pub fn interceptor(&self) -> impl FnMut(&[u8], SocketAddr) -> bool + Send + 'static {
        let mux = self.clone();
        move |data, from| mux.handle_datagram(data, from)
    }

    /// Offers one received datagram to the mux. Returns true if it was a STUN or punch
    /// datagram (consumed, whether or not anybody was waiting for it).
    pub fn handle_datagram(&self, data: &[u8], from: SocketAddr) -> bool {
        if looks_like_punch(data) {
            self.inner.consumed.fetch_add(1, Ordering::Relaxed);
            self.handle_punch(data, from);
            return true;
        }
        if looks_like_stun(data) {
            self.inner.consumed.fetch_add(1, Ordering::Relaxed);
            self.handle_stun(data, from);
            return true;
        }
        false
    }

    fn handle_stun(&self, data: &[u8], from: SocketAddr) {
        let Some(txid) = peek_transaction_id(data) else {
            return;
        };
        let state = self.inner.state();
        if let Some(wait) = state.stun.get(&txid) {
            if wait.server == from {
                let _ = wait.tx.send(decode_binding_response(data, Some(&txid)));
            }
        }
    }

    fn handle_punch(&self, data: &[u8], from: SocketAddr) {
        if data.len() != PUNCH_DATAGRAM_LEN {
            return;
        }
        let kind = data[8];
        let mut value = [0u8; 8];
        value.copy_from_slice(&data[9..]);
        let value = u64::from_be_bytes(value);
        let state = self.inner.state();
        if !state.allowed.contains(&from) {
            return;
        }
        match kind {
            KIND_PROBE => {
                // Reply from the same socket so the ack leaves through the punched mapping.
                let _ = self
                    .inner
                    .socket
                    .send_to(&punch_datagram(KIND_ACK, value), from);
            }
            KIND_ACK => {
                if let Some(wait) = state.punch.get(&from) {
                    if wait.nonce == value {
                        let _ = wait.tx.send(());
                    }
                }
            }
            _ => {}
        }
    }

    /// Lets the mux answer punch probes coming from `peer` (also done by [`NatMux::punch`]).
    pub fn allow_peer(&self, peer: SocketAddr) {
        self.inner.state().allowed.insert(peer);
    }

    /// Stops answering probes from `peer`.
    pub fn forget_peer(&self, peer: SocketAddr) {
        self.inner.state().allowed.remove(&peer);
    }

    /// Sends one Binding request to `server` from the room socket and waits for the answer.
    ///
    /// Contacts `server`: the caller must only call this on a user action.
    pub fn stun_binding(
        &self,
        server: SocketAddr,
        cfg: &StunConfig,
    ) -> Result<SocketAddr, StunError> {
        let txid = TransactionId::random()?;
        let (tx, rx) = mpsc::channel();
        self.inner
            .state()
            .stun
            .insert(txid, StunWait { server, tx });
        let request = encode_binding_request(&txid);
        let mut result = Err(StunError::Timeout);
        for _ in 0..cfg.attempts.max(1) {
            if let Err(e) = self.inner.socket.send_to(&request, server) {
                // ICMP errors of earlier datagrams can surface here; retry on those.
                if e.kind() != io::ErrorKind::ConnectionRefused {
                    result = Err(e.into());
                    break;
                }
            }
            match rx.recv_timeout(cfg.rto) {
                Ok(answer) => {
                    result = answer;
                    break;
                }
                Err(_) => continue,
            }
        }
        self.inner.state().stun.remove(&txid);
        result
    }

    /// Resolves STUN server names and runs [`NatMux::detect`]. Contacts the servers.
    pub fn detect_named(&self, servers: &[&str], cfg: &StunConfig) -> NatReport {
        let local = self
            .inner
            .socket
            .local_addr()
            .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)));
        let mut addrs = Vec::new();
        let mut first_err = None;
        for s in servers {
            match resolve_server(s, &local) {
                Ok(a) => addrs.push(a),
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }
        let mut report = self.detect(&addrs, cfg);
        if report.error.is_none() {
            report.error = first_err;
        }
        report
    }

    /// Queries up to two STUN servers from this socket and classifies the mapping.
    ///
    /// Contacts the servers: the caller must only call this on a user action.
    pub fn detect(&self, servers: &[SocketAddr], cfg: &StunConfig) -> NatReport {
        let mut answers: Vec<SocketAddr> = Vec::new();
        let mut error = None;
        for server in servers.iter().take(2) {
            match self.stun_binding(*server, cfg) {
                Ok(a) => answers.push(a),
                Err(e) => {
                    error.get_or_insert(e);
                }
            }
        }
        let local = self.local_endpoint(servers.first().copied());
        match answers.as_slice() {
            [a, b] => NatReport {
                mapping: classify_mapping(local, *a, *b),
                public_addr: Some(*a),
                second_addr: Some(*b),
                error: None,
            },
            [a] => NatReport {
                mapping: NatMapping::Unknown,
                public_addr: Some(*a),
                second_addr: None,
                error,
            },
            _ => NatReport {
                mapping: NatMapping::Unknown,
                public_addr: None,
                second_addr: None,
                error: error.or(Some(StunError::Timeout)),
            },
        }
    }

    /// Best guess of the socket's own endpoint with a concrete IP (to detect "no NAT").
    fn local_endpoint(&self, toward: Option<SocketAddr>) -> Option<SocketAddr> {
        let local = self.inner.socket.local_addr().ok()?;
        if !local.ip().is_unspecified() {
            return Some(local);
        }
        let target = toward?;
        let probe = UdpSocket::bind(if target.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        })
        .ok()?;
        // connect() on UDP only selects a route; no packet is sent.
        probe.connect(target).ok()?;
        let ip: IpAddr = probe.local_addr().ok()?.ip();
        Some(SocketAddr::new(ip, local.port()))
    }

    /// Punches towards `peer`; both sides must call this with each other's public endpoint.
    ///
    /// Blocks up to `cfg.timeout`. Probes go out every `cfg.interval`. Afterwards the peer
    /// stays allowed so late probes from the other side are still answered; call
    /// [`NatMux::forget_peer`] to stop that.
    pub fn punch(&self, peer: SocketAddr, cfg: &PunchConfig) -> PunchOutcome {
        let nonce = random_u64();
        let (tx, rx) = mpsc::channel();
        {
            let mut state = self.inner.state();
            state.allowed.insert(peer);
            state.punch.insert(peer, PunchWait { nonce, tx });
        }
        let probe = punch_datagram(KIND_PROBE, nonce);
        let deadline = Instant::now() + cfg.timeout;
        let mut outcome = PunchOutcome::Timeout;
        while Instant::now() < deadline {
            // Send errors (e.g. ICMP unreachable echoes) are expected while the peer is not ready.
            let _ = self.inner.socket.send_to(&probe, peer);
            if rx.recv_timeout(cfg.interval).is_ok() {
                outcome = PunchOutcome::Established(peer);
                break;
            }
        }
        self.inner.state().punch.remove(&peer);
        outcome
    }

    /// Starts a thread that sends a keep-alive datagram to `peer` every `interval`
    /// (use [`KEEPALIVE_INTERVAL`] normally). Dropping the handle stops it.
    pub fn start_keepalive(&self, peer: SocketAddr, interval: Duration) -> KeepAlive {
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let stop_thread = Arc::clone(&stop);
        let socket = Arc::clone(&self.inner.socket);
        let handle = std::thread::Builder::new()
            .name("syncverse-nat-keepalive".to_string())
            .spawn(move || {
                let (flag, cv) = &*stop_thread;
                let mut stopped = flag.lock().unwrap_or_else(|e| e.into_inner());
                while !*stopped {
                    let _ = socket.send_to(&keepalive_datagram(), peer);
                    let (guard, _) = cv
                        .wait_timeout(stopped, interval)
                        .unwrap_or_else(|e| e.into_inner());
                    stopped = guard;
                }
            })
            .ok();
        KeepAlive { stop, handle }
    }

    /// Starts a thread that reads the socket and feeds the mux. For sockets with no other
    /// reader (standalone STUN checks, tests). Do not use on a room socket the transport
    /// already reads: use [`NatMux::interceptor`] there.
    pub fn spawn_reader(&self) -> io::Result<ReaderGuard> {
        self.inner
            .socket
            .set_read_timeout(Some(Duration::from_millis(20)))?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let mux = self.clone();
        let handle = std::thread::Builder::new()
            .name("syncverse-nat-reader".to_string())
            .spawn(move || {
                let mut buf = [0u8; 1500];
                while !stop_thread.load(Ordering::Relaxed) {
                    if let Ok((n, from)) = mux.inner.socket.recv_from(&mut buf) {
                        mux.handle_datagram(&buf[..n], from);
                    }
                }
            })?;
        Ok(ReaderGuard {
            stop,
            handle: Some(handle),
        })
    }
}

/// Keep-alive sender; stops when dropped.
pub struct KeepAlive {
    stop: Arc<(Mutex<bool>, Condvar)>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for KeepAlive {
    fn drop(&mut self) {
        let (flag, cv) = &*self.stop;
        *flag.lock().unwrap_or_else(|e| e.into_inner()) = true;
        cv.notify_all();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Reader thread started by [`NatMux::spawn_reader`]; stops when dropped.
pub struct ReaderGuard {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for ReaderGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sa(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn classification_rules() {
        let a = sa("198.51.100.1:5000");
        assert_eq!(
            classify_mapping(None, a, a),
            NatMapping::EndpointIndependent
        );
        assert_eq!(
            classify_mapping(None, a, sa("198.51.100.1:5001")),
            NatMapping::Symmetric
        );
        assert_eq!(classify_mapping(Some(a), a, a), NatMapping::Open);
        assert!(NatMapping::EndpointIndependent.punchable());
        assert!(!NatMapping::Symmetric.punchable());
        assert!(!NatMapping::Unknown.punchable());
    }

    #[test]
    fn punch_datagram_shape() {
        let d = punch_datagram(KIND_PROBE, 42);
        assert!(looks_like_punch(&d));
        assert_eq!(d.len(), PUNCH_DATAGRAM_LEN);
        assert!(!looks_like_stun(&d));
    }
}
