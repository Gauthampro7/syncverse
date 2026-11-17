//! Loopback tests for STUN decoding, Binding exchange, NAT classification and hole punching.
//!
//! No real network and no third-party server is contacted. The NAT simulator below models
//! endpoint-independent (or symmetric) mapping with port-restricted filtering; it says nothing
//! about the behaviour of real routers.

use std::collections::HashSet;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use syncverse_sync::{
    binding_request, classify_mapping, decode_binding_response, encode_binding_request,
    encode_binding_success, looks_like_stun, HostServer, NatMapping, NatMux, PunchConfig,
    PunchOutcome, StunConfig, StunError, TransactionId,
};

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.split_whitespace().collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

const RFC_TXID: [u8; 12] = [
    0xb7, 0xe7, 0xa7, 0x01, 0xbc, 0x34, 0xd6, 0x86, 0xfa, 0x87, 0xdf, 0xae,
];

/// RFC 5769 section 2.2 layout. The MESSAGE-INTEGRITY value is an arbitrary placeholder: the
/// decoder skips that attribute, so its bytes do not matter here.
fn rfc5769_v4() -> Vec<u8> {
    hex("0101003c 2112a442 b7e7a701bc34d686fa87dfae \
         8022000b 7465737420766563746f7220 \
         0020 0008 0001 a147 e112a643 \
         0008 0014 000102030405060708090a0b0c0d0e0f10111213 \
         8028 0004 c07d4c96")
}

fn rfc5769_v6() -> Vec<u8> {
    hex("010100 48 2112a442 b7e7a701bc34d686fa87dfae \
         8022000b 7465737420766563746f7220 \
         0020 0014 0002 a147 0113a9faa5d3f179bc25f4b5bed2b9d9 \
         0008 0014 000102030405060708090a0b0c0d0e0f10111213 \
         8028 0004 c8fb0b4c")
}

#[test]
fn rfc5769_vectors_decode() {
    let txid = TransactionId(RFC_TXID);
    let v4 = decode_binding_response(&rfc5769_v4(), Some(&txid)).unwrap();
    assert_eq!(v4, "192.0.2.1:32853".parse::<SocketAddr>().unwrap());
    let v6 = decode_binding_response(&rfc5769_v6(), Some(&txid)).unwrap();
    assert_eq!(
        v6,
        "[2001:db8:1234:5678:11:2233:4455:6677]:32853"
            .parse::<SocketAddr>()
            .unwrap()
    );
    // Without an expected id the same message decodes too.
    assert_eq!(decode_binding_response(&rfc5769_v4(), None), Ok(v4));
}

#[test]
fn malformed_responses_are_rejected_with_typed_errors() {
    let txid = TransactionId(RFC_TXID);
    let good = rfc5769_v4();

    let mut bad_cookie = good.clone();
    bad_cookie[4] ^= 0xFF;
    assert_eq!(
        decode_binding_response(&bad_cookie, Some(&txid)),
        Err(StunError::BadMagicCookie)
    );

    let other = TransactionId([9; 12]);
    assert_eq!(
        decode_binding_response(&good, Some(&other)),
        Err(StunError::TransactionMismatch)
    );

    assert_eq!(
        decode_binding_response(&good[..10], Some(&txid)),
        Err(StunError::TooShort)
    );
    // Datagram cut inside the attributes: header length no longer matches.
    assert_eq!(
        decode_binding_response(&good[..good.len() - 4], Some(&txid)),
        Err(StunError::LengthMismatch)
    );

    // Consistent header length but an attribute that claims more bytes than exist.
    let mut trunc = hex("0101000c 2112a442 b7e7a701bc34d686fa87dfae");
    trunc.extend(hex("0020 0014 0001 a147 e112a643"));
    assert_eq!(
        decode_binding_response(&trunc, Some(&txid)),
        Err(StunError::TruncatedAttribute)
    );

    // Not STUN at all (top bits set).
    let mut not_stun = good.clone();
    not_stun[0] = 0x80;
    assert_eq!(
        decode_binding_response(&not_stun, Some(&txid)),
        Err(StunError::NotStun)
    );

    // Unknown family.
    let mut fam = good.clone();
    fam[41] = 0x07; // family byte of the XOR-MAPPED-ADDRESS value (20 header + 16 SOFTWARE + 4 + 1)
    assert_eq!(
        decode_binding_response(&fam, Some(&txid)),
        Err(StunError::BadAddressFamily(7))
    );

    // Request type is not a response.
    let req = encode_binding_request(&txid);
    assert_eq!(
        decode_binding_response(&req, Some(&txid)),
        Err(StunError::UnexpectedMessageType(1))
    );

    // Response without any address.
    let empty = hex("01010000 2112a442 b7e7a701bc34d686fa87dfae");
    assert_eq!(
        decode_binding_response(&empty, Some(&txid)),
        Err(StunError::NoMappedAddress)
    );

    // Error response 420.
    let err = hex("01110008 2112a442 b7e7a701bc34d686fa87dfae 0009 0004 0000 0414");
    assert_eq!(
        decode_binding_response(&err, Some(&txid)),
        Err(StunError::ErrorResponse { code: 420 })
    );
}

/// In-test STUN server. Answers with the reflexive address, or `fixed` when given; drops the
/// first `drop_first` requests.
struct FakeStun {
    addr: SocketAddr,
    seen: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl FakeStun {
    fn start(drop_first: usize, fixed: Option<SocketAddr>) -> Self {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        let addr = sock.local_addr().unwrap();
        let seen = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (seen2, stop2) = (seen.clone(), stop.clone());
        let handle = thread::spawn(move || {
            let mut buf = [0u8; 1500];
            while !stop2.load(Ordering::Relaxed) {
                let Ok((n, from)) = sock.recv_from(&mut buf) else {
                    continue;
                };
                if !looks_like_stun(&buf[..n]) {
                    continue;
                }
                let k = seen2.fetch_add(1, Ordering::SeqCst);
                if k < drop_first {
                    continue;
                }
                let mut id = [0u8; 12];
                id.copy_from_slice(&buf[8..20]);
                let reply = encode_binding_success(&TransactionId(id), fixed.unwrap_or(from));
                let _ = sock.send_to(&reply, from);
            }
        });
        Self {
            addr,
            seen,
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for FakeStun {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn fast_stun() -> StunConfig {
    StunConfig {
        rto: Duration::from_millis(100),
        attempts: 4,
    }
}

#[test]
fn binding_roundtrip_against_in_test_server() {
    let server = FakeStun::start(0, None);
    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mapped = binding_request(&client, server.addr, &fast_stun()).unwrap();
    assert_eq!(mapped, client.local_addr().unwrap());
    assert_eq!(server.seen.load(Ordering::SeqCst), 1);
    // Read timeout of the caller's socket is restored.
    assert_eq!(client.read_timeout().unwrap(), None);
}

#[test]
fn binding_retries_after_dropped_requests_and_times_out() {
    let lossy = FakeStun::start(2, None);
    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mapped = binding_request(&client, lossy.addr, &fast_stun()).unwrap();
    assert_eq!(mapped, client.local_addr().unwrap());
    assert_eq!(
        lossy.seen.load(Ordering::SeqCst),
        3,
        "two dropped + one answered"
    );

    let dead = FakeStun::start(usize::MAX, None);
    let start = Instant::now();
    assert_eq!(
        binding_request(&client, dead.addr, &fast_stun()),
        Err(StunError::Timeout)
    );
    assert_eq!(dead.seen.load(Ordering::SeqCst), 4, "four transmissions");
    assert!(start.elapsed() >= Duration::from_millis(390));
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn nat_classification_two_servers() {
    let a: SocketAddr = "198.51.100.7:40001".parse().unwrap();
    let same = (FakeStun::start(0, Some(a)), FakeStun::start(0, Some(a)));
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mux = NatMux::from_socket(&sock).unwrap();
    let _reader = mux.spawn_reader().unwrap();
    let report = mux.detect(&[same.0.addr, same.1.addr], &fast_stun());
    assert_eq!(report.mapping, NatMapping::EndpointIndependent);
    assert_eq!(report.public_addr, Some(a));
    assert!(report.mapping.punchable());
    assert!(report.error.is_none());

    let b: SocketAddr = "198.51.100.7:40002".parse().unwrap();
    let diff = (FakeStun::start(0, Some(a)), FakeStun::start(0, Some(b)));
    let report = mux.detect(&[diff.0.addr, diff.1.addr], &fast_stun());
    assert_eq!(report.mapping, NatMapping::Symmetric);
    assert!(!report.mapping.punchable());
    assert_eq!(report.second_addr, Some(b));

    // One server silent: Unknown, but the address is still reported along with the error.
    let dead = FakeStun::start(usize::MAX, None);
    let report = mux.detect(&[same.0.addr, dead.addr], &fast_stun());
    assert_eq!(report.mapping, NatMapping::Unknown);
    assert_eq!(report.public_addr, Some(a));
    assert_eq!(report.error, Some(StunError::Timeout));

    // No NAT: mapped address equals the socket's own address.
    let own = sock.local_addr().unwrap();
    assert_eq!(classify_mapping(Some(own), own, own), NatMapping::Open);
}

// ---------------------------------------------------------------------------------------------
// NAT simulator
// ---------------------------------------------------------------------------------------------

/// One simulated NAT in front of one application socket. The application sends to
/// `inside_addr()` (standing for "the Internet"); the NAT forwards from its outside socket(s) to
/// `remote_public`, remembering the destination for port-restricted filtering. Inbound datagrams
/// are forwarded to the application only from endpoints it has sent to.
struct SimNat {
    inside: Arc<UdpSocket>,
    outside_adv: Arc<UdpSocket>,
    remote_public: Arc<Mutex<Option<SocketAddr>>>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl SimNat {
    fn new(symmetric: bool) -> Self {
        let inside = Arc::new(UdpSocket::bind("127.0.0.1:0").unwrap());
        let outside_adv = Arc::new(UdpSocket::bind("127.0.0.1:0").unwrap());
        for s in [&inside, &outside_adv] {
            s.set_read_timeout(Some(Duration::from_millis(10))).unwrap();
        }
        let remote_public = Arc::new(Mutex::new(None::<SocketAddr>));
        let allowed = Arc::new(Mutex::new(HashSet::<SocketAddr>::new()));
        let app_addr = Arc::new(Mutex::new(None::<SocketAddr>));
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();

        // Outside socket used for sending: the advertised one (cone) or a fresh one (symmetric).
        let out_send = if symmetric {
            let s = Arc::new(UdpSocket::bind("127.0.0.1:0").unwrap());
            s.set_read_timeout(Some(Duration::from_millis(10))).unwrap();
            s
        } else {
            outside_adv.clone()
        };

        {
            let (inside, out_send, remote, allowed, app, stop) = (
                inside.clone(),
                out_send.clone(),
                remote_public.clone(),
                allowed.clone(),
                app_addr.clone(),
                stop.clone(),
            );
            threads.push(thread::spawn(move || {
                let mut buf = [0u8; 1500];
                while !stop.load(Ordering::Relaxed) {
                    if let Ok((n, from)) = inside.recv_from(&mut buf) {
                        *app.lock().unwrap() = Some(from);
                        let dest = *remote.lock().unwrap();
                        if let Some(dest) = dest {
                            allowed.lock().unwrap().insert(dest);
                            let _ = out_send.send_to(&buf[..n], dest);
                        }
                    }
                }
            }));
        }
        let mut listeners = vec![outside_adv.clone()];
        if symmetric {
            listeners.push(out_send.clone());
        }
        for out in listeners {
            let (inside, allowed, app, stop) = (
                inside.clone(),
                allowed.clone(),
                app_addr.clone(),
                stop.clone(),
            );
            threads.push(thread::spawn(move || {
                let mut buf = [0u8; 1500];
                while !stop.load(Ordering::Relaxed) {
                    if let Ok((n, src)) = out.recv_from(&mut buf) {
                        if allowed.lock().unwrap().contains(&src) {
                            if let Some(app) = *app.lock().unwrap() {
                                let _ = inside.send_to(&buf[..n], app);
                            }
                        }
                    }
                }
            }));
        }
        Self {
            inside,
            outside_adv,
            remote_public,
            stop,
            threads,
        }
    }

    fn inside_addr(&self) -> SocketAddr {
        self.inside.local_addr().unwrap()
    }

    /// The endpoint this NAT would advertise to the other side (STUN would report it).
    fn advertised(&self) -> SocketAddr {
        self.outside_adv.local_addr().unwrap()
    }
}

impl Drop for SimNat {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

struct Peer {
    mux: NatMux,
    _reader: syncverse_sync::ReaderGuard,
    _sock: UdpSocket,
}

fn peer() -> Peer {
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    // The simulator learns the application address from the first datagram; connecting is not
    // needed, but each app talks only to its NAT's inside address.
    let mux = NatMux::from_socket(&sock).unwrap();
    let reader = mux.spawn_reader().unwrap();
    Peer {
        mux,
        _reader: reader,
        _sock: sock,
    }
}

fn pair(sym_a: bool) -> (SimNat, SimNat) {
    let a = SimNat::new(sym_a);
    let b = SimNat::new(false);
    *a.remote_public.lock().unwrap() = Some(b.advertised());
    *b.remote_public.lock().unwrap() = Some(a.advertised());
    (a, b)
}

fn cfg(timeout_ms: u64) -> PunchConfig {
    PunchConfig {
        interval: Duration::from_millis(50),
        timeout: Duration::from_millis(timeout_ms),
    }
}

#[test]
fn punch_through_simulated_port_restricted_nats() {
    // Both sides punch: handshake completes on both.
    let (nat_a, nat_b) = pair(false);
    let (pa, pb) = (peer(), peer());
    let (ma, mb) = (pa.mux.clone(), pb.mux.clone());
    let (ia, ib) = (nat_a.inside_addr(), nat_b.inside_addr());
    let ta = thread::spawn(move || ma.punch(ia, &cfg(3000)));
    let tb = thread::spawn(move || mb.punch(ib, &cfg(3000)));
    assert_eq!(ta.join().unwrap(), PunchOutcome::Established(ia));
    assert_eq!(tb.join().unwrap(), PunchOutcome::Established(ib));
    drop((pa, pb, nat_a, nat_b));

    // Negative control: B stays silent, so B's NAT never opens towards A.
    let (nat_a, nat_b) = pair(false);
    let (pa, pb) = (peer(), peer());
    let start = Instant::now();
    assert_eq!(
        pa.mux.punch(nat_a.inside_addr(), &cfg(600)),
        PunchOutcome::Timeout
    );
    assert!(start.elapsed() < Duration::from_millis(1500));
    drop((pa, pb, nat_a, nat_b));
}

#[test]
fn punch_fails_within_bound_when_one_side_is_symmetric() {
    let (nat_a, nat_b) = pair(true);
    let (pa, pb) = (peer(), peer());
    let (ma, mb) = (pa.mux.clone(), pb.mux.clone());
    let (ia, ib) = (nat_a.inside_addr(), nat_b.inside_addr());
    let start = Instant::now();
    let ta = thread::spawn(move || ma.punch(ia, &cfg(700)));
    let tb = thread::spawn(move || mb.punch(ib, &cfg(700)));
    assert_eq!(ta.join().unwrap(), PunchOutcome::Timeout);
    assert_eq!(tb.join().unwrap(), PunchOutcome::Timeout);
    assert!(start.elapsed() < Duration::from_millis(1800));
}

#[test]
fn punch_default_config_matches_spec() {
    let c = PunchConfig::default();
    assert_eq!(c.interval, Duration::from_millis(50));
    assert_eq!(c.timeout, Duration::from_secs(5));
    let s = StunConfig::default();
    assert_eq!(s.rto, Duration::from_millis(500));
    assert_eq!(s.attempts, 4);
}

#[test]
fn probes_from_unknown_peers_get_no_answer() {
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mux = NatMux::from_socket(&sock).unwrap();
    let _reader = mux.spawn_reader().unwrap();
    let stranger = UdpSocket::bind("127.0.0.1:0").unwrap();
    stranger
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    let mut probe = b"SVPUNCH1".to_vec();
    probe.push(0);
    probe.extend_from_slice(&7u64.to_be_bytes());
    stranger
        .send_to(&probe, sock.local_addr().unwrap())
        .unwrap();
    let mut buf = [0u8; 64];
    assert!(
        stranger.recv_from(&mut buf).is_err(),
        "no reply to strangers"
    );

    mux.allow_peer(stranger.local_addr().unwrap());
    stranger
        .send_to(&probe, sock.local_addr().unwrap())
        .unwrap();
    let (n, _) = stranger
        .recv_from(&mut buf)
        .expect("allowed peer gets an ack");
    assert_eq!(&buf[..9], b"SVPUNCH1\x01");
    assert_eq!(&buf[9..n], &7u64.to_be_bytes());
}

#[test]
fn keepalive_sends_periodically_and_stops_on_drop() {
    let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let mux = NatMux::from_socket(&sender).unwrap();
    let ka = mux.start_keepalive(receiver.local_addr().unwrap(), Duration::from_millis(40));
    let mut buf = [0u8; 64];
    let rx_mux = NatMux::from_socket(&receiver).unwrap();
    let mut got = 0;
    while got < 3 {
        let (n, from) = receiver.recv_from(&mut buf).expect("keep-alive datagram");
        assert!(
            rx_mux.handle_datagram(&buf[..n], from),
            "consumed by the mux"
        );
        got += 1;
    }
    drop(ka);
    receiver
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    while receiver.recv_from(&mut buf).is_ok() {}
    assert!(receiver.recv_from(&mut buf).is_err(), "stopped after drop");
}

// ---------------------------------------------------------------------------------------------
// Integration with the room socket (transport seam)
// ---------------------------------------------------------------------------------------------

#[test]
fn punch_datagrams_never_reach_wire_parser() {
    let mut host = HostServer::bind("127.0.0.1:0").unwrap();
    let mux = NatMux::from_socket(host.socket()).unwrap();
    host.set_datagram_interceptor(Some(Box::new(mux.interceptor())));
    let host_addr = host.local_addr().unwrap();

    let flooder = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut intercepted_expected = 0u64;
    let mut garbage = 0u64;
    let txid = TransactionId([3; 12]);
    for i in 0..200u32 {
        match i % 4 {
            0 => {
                let mut d = b"SVPUNCH1".to_vec();
                d.push((i % 3) as u8);
                d.extend_from_slice(&u64::from(i).to_be_bytes());
                flooder.send_to(&d, host_addr).unwrap();
                intercepted_expected += 1;
            }
            1 => {
                flooder
                    .send_to(&encode_binding_success(&txid, host_addr), host_addr)
                    .unwrap();
                intercepted_expected += 1;
            }
            2 => {
                // Truncated punch datagram: still consumed, never parsed as a room packet.
                flooder.send_to(b"SVPUNCH1\x00", host_addr).unwrap();
                intercepted_expected += 1;
            }
            _ => {
                // Pseudo-random garbage that is neither STUN nor punch.
                let mut x = i.wrapping_mul(2654435761) | 0x8000_0000;
                let d: Vec<u8> = (0..(5 + i % 40))
                    .map(|_| {
                        x ^= x << 13;
                        x ^= x >> 17;
                        x ^= x << 5;
                        (x >> 3) as u8 | 0x80
                    })
                    .collect();
                flooder.send_to(&d, host_addr).unwrap();
                garbage += 1;
            }
        }
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline
        && host.intercepted_datagrams() + host.dropped_datagrams() < intercepted_expected + garbage
    {
        assert!(matches!(host.poll_and_process(), Ok(None)));
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(host.intercepted_datagrams(), intercepted_expected);
    assert_eq!(host.dropped_datagrams(), garbage);
    assert!(host.clients().is_empty(), "no client may be registered");
    assert_eq!(mux.consumed_count(), intercepted_expected);
}

#[test]
fn stun_and_punch_work_on_the_room_socket() {
    let mut host = HostServer::bind("127.0.0.1:0").unwrap();
    let host_addr = host.local_addr().unwrap();
    let mux = NatMux::from_socket(host.socket()).unwrap();
    host.set_datagram_interceptor(Some(Box::new(mux.interceptor())));

    // STUN from the room socket: the server sees the room port as source.
    let s1 = FakeStun::start(0, None);
    let s2 = FakeStun::start(0, None);
    let (m, a, b) = (mux.clone(), s1.addr, s2.addr);
    let t = thread::spawn(move || m.detect(&[a, b], &fast_stun()));
    while !t.is_finished() {
        let _ = host.poll_and_process();
        thread::sleep(Duration::from_millis(2));
    }
    let report = t.join().unwrap();
    assert_eq!(report.public_addr, Some(host_addr));
    assert_eq!(
        report.mapping,
        NatMapping::Open,
        "loopback: mapped == local"
    );

    // Punch with a plain peer (direct path stands in for an opened mapping).
    let guest = peer();
    let guest_addr = guest._sock.local_addr().unwrap();
    let (m, g) = (mux.clone(), guest_addr);
    let hp = thread::spawn(move || m.punch(g, &cfg(3000)));
    let gm = guest.mux.clone();
    let gp = thread::spawn(move || gm.punch(host_addr, &cfg(3000)));
    while !hp.is_finished() || !gp.is_finished() {
        let _ = host.poll_and_process();
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(hp.join().unwrap(), PunchOutcome::Established(guest_addr));
    assert_eq!(gp.join().unwrap(), PunchOutcome::Established(host_addr));
    assert!(host.clients().is_empty());
}
