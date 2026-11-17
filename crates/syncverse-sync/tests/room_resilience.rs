//! Room resilience acceptance tests: handshake, heartbeat, host-loss detection, rejoin,
//! Bye, id allocation, v0.4 compatibility and garbage tolerance.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use syncverse_sync::client::{
    ClientConnectionState as St, CloseReason, ConnectFailReason, ConnectionEvent, ResilienceConfig,
};
use syncverse_sync::wire::{ByeReason, FLAG_ENCRYPTED, PacketPayload, WirePacket};
use syncverse_sync::{ClientNode, HostServer, PeerControlAction, RoomCryptoSession};

const LO: &str = "127.0.0.1:0";

fn pump_until<F>(
    host: &mut HostServer,
    clients: &mut [&mut ClientNode],
    timeout: Duration,
    mut cond: F,
) -> bool
where
    F: FnMut(&mut HostServer, &mut [&mut ClientNode]) -> bool,
{
    let end = Instant::now() + timeout;
    loop {
        while let Ok(Some(_)) = host.poll_and_process() {}
        let _ = host.flush_welcome_to_new_peers();
        for c in clients.iter_mut() {
            while let Ok(Some(_)) = c.poll_and_process() {}
        }
        if cond(host, clients) {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn pump_clients_until<F>(clients: &mut [&mut ClientNode], timeout: Duration, mut cond: F) -> bool
where
    F: FnMut(&mut [&mut ClientNode]) -> bool,
{
    let end = Instant::now() + timeout;
    loop {
        for c in clients.iter_mut() {
            while let Ok(Some(_)) = c.poll_and_process() {}
        }
        if cond(clients) {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn fast_host() -> HostServer {
    HostServer::bind(LO)
        .unwrap()
        .with_resilience(ResilienceConfig::fast())
}

fn fast_client(host_addr: SocketAddr, id: u32) -> ClientNode {
    ClientNode::bind(LO, host_addr, id)
        .unwrap()
        .with_resilience(ResilienceConfig::fast())
}

fn connected(host: &mut HostServer, client: &mut ClientNode) {
    client.begin_join("alice");
    let ok = pump_until(host, &mut [client], Duration::from_secs(2), |_, c| {
        c[0].is_connected()
    });
    assert!(ok, "client must reach Connected, state {:?}", client.connection_state());
}

struct Xorshift(u64);
impl Xorshift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn chance(&mut self, p: f64) -> bool {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64 <= p
    }
}

#[test]
fn hello_ack_required_for_connected() {
    let dead = UdpSocket::bind(LO).unwrap();
    let dead_addr = dead.local_addr().unwrap();
    drop(dead);
    let mut client = fast_client(dead_addr, 5);
    client.begin_join("alice");
    let mut ever_connected = false;
    let end = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < end {
        let _ = client.poll_and_process();
        ever_connected |= client.is_connected();
        if matches!(client.connection_state(), St::Failed(_)) {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(!ever_connected, "must never report Connected without HelloAck");
    assert_eq!(client.connection_state(), St::Failed(ConnectFailReason::Timeout));
}

#[test]
fn handshake_assigns_id_and_reports_connected() {
    let mut host = fast_host();
    let mut client = fast_client(host.local_addr().unwrap(), 4242);
    connected(&mut host, &mut client);
    assert!(!client.is_legacy_host());
    assert_eq!(host.clients().len(), 1);
    let session = host.clients().values().next().unwrap();
    assert_eq!(session.username, "alice");
    assert_eq!(session.client_id, client.client_id());
    let events = client.poll_connection_events();
    assert!(events.contains(&ConnectionEvent::StateChanged(St::Connecting)));
    assert!(events.contains(&ConnectionEvent::StateChanged(St::Connected)));
}

#[test]
fn wrong_password_reports_auth_failure() {
    let mut host = fast_host().with_crypto(Some(Arc::new(
        RoomCryptoSession::from_password("room", "right").unwrap(),
    )));
    let mut client = fast_client(host.local_addr().unwrap(), 1).with_crypto(Some(Arc::new(
        RoomCryptoSession::from_password("room", "wrong").unwrap(),
    )));
    client.begin_join("mallory");
    pump_until(&mut host, &mut [&mut client], Duration::from_secs(2), |_, c| {
        matches!(c[0].connection_state(), St::Failed(_))
    });
    assert_eq!(client.connection_state(), St::Failed(ConnectFailReason::AuthFailed));
    assert!(host.clients().is_empty(), "failed auth must not register a peer");
}

#[test]
fn plaintext_client_vs_encrypted_host_reports_auth_failure_and_vice_versa() {
    let crypto = || Some(Arc::new(RoomCryptoSession::from_password("r", "p").unwrap()));
    // plaintext client -> encrypted host
    let mut host = fast_host().with_crypto(crypto());
    let mut client = fast_client(host.local_addr().unwrap(), 1);
    client.begin_join("x");
    pump_until(&mut host, &mut [&mut client], Duration::from_secs(2), |_, c| {
        matches!(c[0].connection_state(), St::Failed(_))
    });
    assert_eq!(client.connection_state(), St::Failed(ConnectFailReason::AuthFailed));
    // encrypted client -> plaintext host
    let mut host = fast_host();
    let mut client = fast_client(host.local_addr().unwrap(), 1).with_crypto(crypto());
    client.begin_join("x");
    pump_until(&mut host, &mut [&mut client], Duration::from_secs(2), |_, c| {
        matches!(c[0].connection_state(), St::Failed(_))
    });
    assert_eq!(client.connection_state(), St::Failed(ConnectFailReason::AuthFailed));
}

#[test]
fn matching_password_connects_and_hello_is_inside_aead_envelope() {
    // 1) Sniff the Hello of an encrypted client: it must be encrypted, username not in clear.
    let sniffer = UdpSocket::bind(LO).unwrap();
    sniffer.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
    let mut probe = fast_client(sniffer.local_addr().unwrap(), 9).with_crypto(Some(Arc::new(
        RoomCryptoSession::from_password("room", "pw").unwrap(),
    )));
    probe.begin_join("SECRET-USERNAME-XYZ");
    let mut buf = [0u8; 2048];
    let (n, _) = sniffer.recv_from(&mut buf).expect("hello datagram");
    assert_eq!(buf[4], 0x10, "packet type is Hello");
    assert_ne!(buf[5] & FLAG_ENCRYPTED, 0, "Hello must carry the encrypted flag");
    let hay = &buf[..n];
    assert!(
        !hay.windows(19).any(|w| w == b"SECRET-USERNAME-XYZ"),
        "username must not appear in clear"
    );
    // 2) Matching passwords connect.
    let crypto = || Some(Arc::new(RoomCryptoSession::from_password("room", "pw").unwrap()));
    let mut host = fast_host().with_crypto(crypto());
    let mut client = fast_client(host.local_addr().unwrap(), 1).with_crypto(crypto());
    connected(&mut host, &mut client);
}

#[test]
fn host_killed_client_enters_reconnecting_within_bound() {
    let cfg = ResilienceConfig::fast();
    let mut host = fast_host();
    let mut client = fast_client(host.local_addr().unwrap(), 1);
    connected(&mut host, &mut client);
    // Let some heartbeats flow, then kill the host without any Bye.
    pump_until(&mut host, &mut [&mut client], Duration::from_millis(250), |_, _| false);
    drop(host);
    let killed = Instant::now();
    let ok = pump_clients_until(&mut [&mut client], Duration::from_secs(2), |c| c[0].is_link_stale());
    let detect = killed.elapsed();
    assert!(ok, "never entered Reconnecting");
    assert!(
        detect <= cfg.silence_timeout + Duration::from_millis(100),
        "detected after {detect:?}"
    );
    assert!(matches!(client.connection_state(), St::Reconnecting { attempt: 1 }));
    // Playback continues locally: PLL holds the nominal rate and never seeks.
    assert!(!client.should_snap_immediately(5000, true, true));
}

#[test]
fn host_restart_same_port_client_rejoins_and_restores_state() {
    let mut host = fast_host();
    let addr = host.local_addr().unwrap();
    let mut client = fast_client(addr, 77);
    connected(&mut host, &mut client);
    client
        .send_peer_status_with_hash(true, true, 90_000, 1234, "alice", "movie.mkv", [7u8; 32])
        .unwrap();
    pump_until(&mut host, &mut [&mut client], Duration::from_millis(300), |h, _| {
        h.clients().values().next().is_some_and(|c| c.is_user_ready)
    });
    let old_epoch = host.host_epoch();
    drop(host);
    pump_clients_until(&mut [&mut client], Duration::from_secs(2), |c| c[0].is_link_stale());
    assert!(client.is_link_stale());

    let mut host2 = HostServer::bind(addr)
        .expect("rebind same port")
        .with_resilience(ResilienceConfig::fast());
    assert_ne!(host2.host_epoch(), old_epoch);
    let ok = pump_until(&mut host2, &mut [&mut client], Duration::from_secs(5), |_, c| {
        c[0].is_connected()
    });
    assert!(ok, "client must rejoin, state {:?}", client.connection_state());
    assert!(client
        .poll_connection_events()
        .contains(&ConnectionEvent::Rejoined { host_restarted: true }));
    // Ready flag + username restored on the new host.
    let restored = pump_until(&mut host2, &mut [&mut client], Duration::from_secs(1), |h, _| {
        h.clients()
            .values()
            .next()
            .is_some_and(|c| c.is_user_ready && c.has_media && c.username == "alice")
    });
    assert!(restored, "status not restored: {:?}", host2.peer_statuses());
    assert_eq!(host2.clients().values().next().unwrap().client_id, 77);

    // Barrier unpause works afterwards.
    host2.set_media_state(1000, 1.0, true);
    let target = host2.prepare_barrier_unpause(50).unwrap();
    let ready = pump_until(&mut host2, &mut [&mut client], Duration::from_secs(2), |h, _| {
        h.are_all_clients_ready()
    });
    assert!(ready, "barrier never became ready after rejoin");
    host2.commit_barrier_unpause(target).unwrap();
    let committed = pump_until(&mut host2, &mut [&mut client], Duration::from_secs(1), |_, c| {
        c[0].scheduled_unpause_local_ms().is_some()
    });
    assert!(committed);
}

#[test]
fn reconnect_during_barrier_prepare_does_not_deadlock() {
    let mut host = fast_host();
    let mut client = fast_client(host.local_addr().unwrap(), 3);
    connected(&mut host, &mut client);
    // Cut the client off (drop everything inbound) until it declares the host lost.
    let blackhole = Arc::new(AtomicU64::new(1));
    let bh = Arc::clone(&blackhole);
    client.set_datagram_interceptor(Some(Box::new(move |_, _| bh.load(Ordering::Relaxed) == 1)));
    pump_until(&mut host, &mut [&mut client], Duration::from_secs(2), |_, c| c[0].is_link_stale());
    assert!(client.is_link_stale());
    // Barrier enters Prepare while the client is away.
    host.set_media_state(0, 1.0, true);
    host.prepare_barrier_unpause(50).unwrap();
    assert!(!host.are_all_clients_ready());
    blackhole.store(0, Ordering::Relaxed);
    let ok = pump_until(&mut host, &mut [&mut client], Duration::from_secs(3), |h, c| {
        c[0].is_connected() && h.are_all_clients_ready()
    });
    assert!(ok, "rejoin during Prepare must complete the barrier");
}

#[test]
fn packet_loss_30pct_handshake_and_heartbeat_survive() {
    // Default (production) timeouts on purpose: no spurious Reconnecting at 30 % loss.
    let mut host = HostServer::bind(LO).unwrap();
    let mut client = ClientNode::bind(LO, host.local_addr().unwrap(), 1).unwrap();
    let rng_h = Arc::new(std::sync::Mutex::new(Xorshift(0x1234_5678_9ABC_DEF1)));
    let rng_c = Arc::new(std::sync::Mutex::new(Xorshift(0x0FED_CBA9_8765_4321)));
    host.set_datagram_interceptor(Some(Box::new(move |_, _| rng_h.lock().unwrap().chance(0.30))));
    client.set_datagram_interceptor(Some(Box::new(move |_, _| rng_c.lock().unwrap().chance(0.30))));
    client.begin_join("lossy");
    let mut saw_reconnecting = false;
    let mut connected_at = None;
    let start = Instant::now();
    pump_until(&mut host, &mut [&mut client], Duration::from_secs(11), |_, c| {
        if connected_at.is_none() && c[0].is_connected() {
            connected_at = Some(start.elapsed());
        }
        saw_reconnecting |= matches!(c[0].connection_state(), St::Reconnecting { .. });
        false
    });
    let connected_at = connected_at.expect("handshake must survive 30% loss");
    assert!(connected_at < Duration::from_secs(5), "took {connected_at:?}");
    assert!(!client.is_legacy_host(), "must not degrade to legacy under loss");
    assert!(!saw_reconnecting, "spurious Reconnecting under 30% loss");
    assert!(client.is_connected());
    assert_eq!(host.clients().len(), 1);
}

#[test]
fn heartbeat_keeps_client_registered_against_prune() {
    let mut host = fast_host();
    let mut client = fast_client(host.local_addr().unwrap(), 1);
    connected(&mut host, &mut client);
    let mut pruned = Vec::new();
    pump_until(&mut host, &mut [&mut client], Duration::from_millis(2500), |h, _| {
        pruned.extend(h.prune_stale());
        false
    });
    assert!(pruned.is_empty(), "heartbeating client was pruned");
    assert_eq!(host.clients().len(), 1);
}

#[test]
fn bye_removes_client_immediately() {
    let mut host = fast_host();
    let mut a = fast_client(host.local_addr().unwrap(), 1);
    let mut b = fast_client(host.local_addr().unwrap(), 2);
    a.begin_join("a");
    b.begin_join("b");
    assert!(pump_until(&mut host, &mut [&mut a, &mut b], Duration::from_secs(2), |_, c| {
        c[0].is_connected() && c[1].is_connected()
    }));
    b.send_peer_status(true, false, 1, 1, "b", "f.mkv").unwrap();
    pump_until(&mut host, &mut [&mut a, &mut b], Duration::from_millis(200), |_, c| {
        !c[0].peer_statuses().is_empty()
    });
    let b_id = b.client_id();
    b.leave();
    assert_eq!(b.connection_state(), St::Closed(CloseReason::LocalLeave));
    let t = Instant::now();
    let gone = pump_until(&mut host, &mut [&mut a], Duration::from_millis(300), |h, _| {
        h.clients().len() == 1
    });
    assert!(gone && t.elapsed() < Duration::from_millis(100), "Bye must remove at once");
    let departed = host.drain_departed_peers();
    assert_eq!(departed.len(), 1);
    assert_eq!(departed[0].reason, ByeReason::Leave);
    assert_eq!(departed[0].client_id, b_id);
    // Remaining peers learn that b left and drop it from their list.
    pump_until(&mut host, &mut [&mut a], Duration::from_millis(200), |_, c| {
        !c[0].peer_statuses().contains_key(&b_id)
    });
    assert!(!a.peer_statuses().contains_key(&b_id));
    assert!(a.poll_departed_peers().contains(&b_id));
    // A late in-flight packet from the departed peer must not resurrect it.
    let _ = b.send_ntp_probe();
    pump_until(&mut host, &mut [&mut a], Duration::from_millis(100), |_, _| false);
    assert_eq!(host.clients().len(), 1);
}

#[test]
fn host_close_notifies_clients() {
    let mut host = fast_host();
    let mut a = fast_client(host.local_addr().unwrap(), 1);
    let mut b = fast_client(host.local_addr().unwrap(), 2);
    a.begin_join("a");
    b.begin_join("b");
    assert!(pump_until(&mut host, &mut [&mut a, &mut b], Duration::from_secs(2), |_, c| {
        c[0].is_connected() && c[1].is_connected()
    }));
    host.close_room();
    let t = Instant::now();
    let ok = pump_clients_until(&mut [&mut a, &mut b], Duration::from_millis(300), |c| {
        c.iter().all(|x| x.connection_state() == St::Closed(CloseReason::HostClosed))
    });
    assert!(ok, "states {:?} {:?}", a.connection_state(), b.connection_state());
    assert!(t.elapsed() < Duration::from_millis(150), "must not wait for silence timeout");
}

#[test]
fn host_full_is_reported() {
    let mut host = fast_host();
    host.set_max_clients(1);
    let mut a = fast_client(host.local_addr().unwrap(), 1);
    let mut b = fast_client(host.local_addr().unwrap(), 2);
    connected(&mut host, &mut a);
    b.begin_join("b");
    pump_until(&mut host, &mut [&mut a, &mut b], Duration::from_secs(2), |_, c| {
        matches!(c[1].connection_state(), St::Failed(_))
    });
    assert_eq!(b.connection_state(), St::Failed(ConnectFailReason::HostFull));
}

/// Sends one legacy v0.4-style packet (no handshake) from a raw socket.
fn raw_send(sock: &UdpSocket, host: SocketAddr, pkt: &WirePacket) {
    sock.send_to(&pkt.to_bytes(), host).unwrap();
}

#[test]
fn client_ids_never_collide_after_prune() {
    let mut host = HostServer::bind(LO).unwrap();
    let addr = host.local_addr().unwrap();
    let a = UdpSocket::bind(LO).unwrap();
    let b = UdpSocket::bind(LO).unwrap();
    let c = UdpSocket::bind(LO).unwrap();
    let drain = |host: &mut HostServer| {
        thread::sleep(Duration::from_millis(15));
        while let Ok(Some(_)) = host.poll_and_process() {}
    };
    raw_send(&a, addr, &WirePacket::ntp_request(1, 1));
    drain(&mut host);
    raw_send(&b, addr, &WirePacket::ntp_request(1, 1));
    drain(&mut host);
    thread::sleep(Duration::from_millis(120));
    raw_send(&b, addr, &WirePacket::ntp_request(2, 2));
    drain(&mut host);
    let pruned = host.prune_stale_clients(Duration::from_millis(100));
    assert_eq!(pruned.len(), 1, "only a is stale");
    raw_send(&c, addr, &WirePacket::ntp_request(1, 1));
    drain(&mut host);
    let mut ids: Vec<u32> = host.clients().values().map(|s| s.client_id).collect();
    assert_eq!(ids.len(), 2);
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 2, "client ids collided: {:?}", host.clients().values().map(|s| s.client_id).collect::<Vec<_>>());
}

#[test]
fn legacy_v04_client_still_joins() {
    let mut host = HostServer::bind(LO).unwrap();
    host.set_media_state(5000, 1.0, true);
    let addr = host.local_addr().unwrap();
    let raw = UdpSocket::bind(LO).unwrap();
    raw.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
    // Captured v0.4 join sequence: peer status, NTP probe, snapshot request, chat.
    raw_send(&raw, addr, &WirePacket::peer_status_with_hash(4711, true, false, 1000, 10, "old", "a.mkv", [1; 32]));
    raw_send(&raw, addr, &WirePacket::ntp_request(1, 42));
    raw_send(&raw, addr, &WirePacket::peer_control(1, 4711, PeerControlAction::RequestSyncSnapshot, 0, 1.0));
    raw_send(&raw, addr, &WirePacket::chat_message(1, 1, 1, b"hello".to_vec()));
    thread::sleep(Duration::from_millis(30));
    while let Ok(Some(_)) = host.poll_and_process() {}
    assert_eq!(host.clients().len(), 1, "legacy client implicitly registered");
    assert_eq!(host.clients().values().next().unwrap().username, "old");
    let mut got_room_state = false;
    let mut got_ntp = false;
    let mut buf = [0u8; 2048];
    while let Ok((n, _)) = raw.recv_from(&mut buf) {
        match WirePacket::from_bytes(&buf[..n]).map(|p| p.payload) {
            Ok(PacketPayload::RoomStateBroadcast(rs)) => {
                got_room_state = true;
                assert_eq!(rs.ref_media_pts_ms, 5000);
            }
            Ok(PacketPayload::NtpProbeResponse(_)) => got_ntp = true,
            Ok(PacketPayload::HelloAck(_)) => panic!("legacy peer must not receive handshake traffic"),
            _ => {}
        }
        if got_room_state && got_ntp {
            break;
        }
    }
    assert!(got_room_state && got_ntp);
}

#[test]
fn v05_client_falls_back_to_implicit_join_against_legacy_host() {
    // A v0.4 host: answers NTP probes, cannot decode (and so ignores) Hello/Heartbeat.
    let sock = UdpSocket::bind(LO).unwrap();
    sock.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
    let addr = sock.local_addr().unwrap();
    let stop = Arc::new(AtomicU64::new(0));
    let stop2 = Arc::clone(&stop);
    let host = thread::spawn(move || {
        let mut buf = [0u8; 2048];
        while stop2.load(Ordering::Relaxed) == 0 {
            if let Ok((n, from)) = sock.recv_from(&mut buf) {
                if buf[4] == 0x01 {
                    if let Ok(WirePacket { payload: PacketPayload::NtpProbeRequest(r), .. }) = WirePacket::from_bytes(&buf[..n]) {
                        let resp = WirePacket::ntp_response(r.probe_id, r.t1_client_send_ns, 1, 2);
                        let _ = sock.send_to(&resp.to_bytes(), from);
                    }
                }
            }
        }
    });
    let mut client = fast_client(addr, 1);
    client.begin_join("new");
    let ok = pump_clients_until(&mut [&mut client], Duration::from_secs(2), |c| c[0].is_connected());
    stop.store(1, Ordering::Relaxed);
    host.join().unwrap();
    assert!(ok, "state {:?}", client.connection_state());
    assert!(client.is_legacy_host());
    assert!(client.poll_connection_events().contains(&ConnectionEvent::LegacyHostFallback));
}

#[test]
fn garbage_datagrams_do_not_register_or_stall() {
    let mut host = HostServer::bind(LO).unwrap();
    let addr = host.local_addr().unwrap();
    let mut client = ClientNode::bind(LO, addr, 1).unwrap();
    let rogue = UdpSocket::bind(LO).unwrap();
    let mut rng = Xorshift(0xDEAD_BEEF_CAFE_F00D);
    let start = Instant::now();
    let mut sent = 0u32;
    while sent < 10_000 {
        let len = (rng.next() % 300) as usize;
        let mut d: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        if sent.is_multiple_of(7) && d.len() >= 12 {
            d[..4].copy_from_slice(b"SPNT");
        }
        if sent.is_multiple_of(50) {
            d = b"SVPUNCH1\x00\x01".to_vec();
        }
        rogue.send_to(&d, addr).unwrap();
        sent += 1;
        if sent.is_multiple_of(64) {
            while let Ok(Some(_)) = host.poll_and_process() {}
        }
    }
    assert!(start.elapsed() < Duration::from_secs(5));
    while let Ok(Some(_)) = host.poll_and_process() {}
    assert!(host.clients().is_empty(), "garbage registered a client");
    assert!(host.dropped_datagrams() > 1000, "dropped {}", host.dropped_datagrams());
    // A valid packet afterwards is processed within one poll loop.
    client.send_ntp_probe().unwrap();
    thread::sleep(Duration::from_millis(10));
    let mut got = None;
    for _ in 0..1 {
        while let Ok(r) = host.poll_and_process() {
            if r.is_some() {
                got = r;
            } else {
                break;
            }
        }
    }
    assert!(got.is_some(), "valid packet must be processed in one drain loop");
    assert_eq!(host.clients().len(), 1);
}

#[test]
fn new_wire_packets_roundtrip_plain_and_encrypted() {
    let crypto = RoomCryptoSession::from_password("r", "p").unwrap();
    let packets = [
        WirePacket::hello(0xDEADBEEF, 12, "Ünïcode ☃"),
        WirePacket::hello_ack(5, syncverse_sync::wire::HelloStatus::HostFull, 9, 77),
        WirePacket::bye(ByeReason::HostClosed, 3),
        WirePacket::heartbeat(99, true),
    ];
    for p in packets {
        assert_eq!(WirePacket::from_bytes(&p.to_bytes()).unwrap(), p);
        let enc = p.to_bytes_encrypted(&crypto).unwrap();
        assert_eq!(WirePacket::from_encrypted_bytes(&enc, Some(&crypto)).unwrap(), p);
    }
}
