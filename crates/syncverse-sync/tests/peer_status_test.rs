//! Integration test suite for PeerStatus wire protocol, state tracking, and relay.
//!
//! Validates:
//! 1. Wire serialization and deserialization round-trips for `PeerStatusPayload`.
//! 2. Path sanitization ensuring directory trees are never leaked across the wire.
//! 3. Adversarial / malformed packet fuzzing for `PeerStatus` (truncated buffers, invalid UTF-8, CRC corruptions).
//! 4. Multi-peer watch party status exchange, host session tracking, and relay between clients.
//! 5. Strict independence between transient `barrier_preroll_ready` and persistent `is_user_ready`.

use std::thread;
use std::time::Duration;

use syncverse_common::Crc32;
use syncverse_sync::{
    ClientNode, HostServer, PacketPayload, PacketType, PeerStatusPayload, SyncError, WirePacket,
};

#[test]
fn test_peer_status_wire_roundtrips() {
    // 1. Standard payload
    let p1 = PeerStatusPayload::new(
        1,
        true,
        true,
        5_400_000,
        1_073_741_824,
        "Alice",
        "movie.mp4",
    );
    let pkt1 = WirePacket::peer_status(
        p1.client_id,
        p1.has_media,
        p1.is_ready,
        p1.duration_ms,
        p1.file_size_bytes,
        &p1.username,
        &p1.filename,
    );
    let bytes1 = pkt1.to_bytes();
    assert_eq!(&bytes1[0..4], b"SPNT");
    assert_eq!(bytes1[4], PacketType::PeerStatus as u8);
    let parsed1 = WirePacket::from_bytes(&bytes1).expect("parse standard payload");
    assert_eq!(pkt1, parsed1);

    // 2. Empty strings and zero values
    let p2 = PeerStatusPayload::new(0, false, false, 0, 0, "", "");
    let pkt2 = WirePacket::peer_status(
        p2.client_id,
        p2.has_media,
        p2.is_ready,
        p2.duration_ms,
        p2.file_size_bytes,
        &p2.username,
        &p2.filename,
    );
    let bytes2 = pkt2.to_bytes();
    assert_eq!(bytes2.len(), 12 + 26);
    let parsed2 = WirePacket::from_bytes(&bytes2).expect("parse empty payload");
    assert_eq!(pkt2, parsed2);

    // 3. Unicode and boundary values
    let p3 = PeerStatusPayload::new(
        u32::MAX,
        true,
        false,
        u64::MAX,
        u64::MAX,
        "オーディエンス_1 🍿",
        "すずめの戸締まり_2022_1080p.mkv",
    );
    let pkt3 = WirePacket::peer_status(
        p3.client_id,
        p3.has_media,
        p3.is_ready,
        p3.duration_ms,
        p3.file_size_bytes,
        &p3.username,
        &p3.filename,
    );
    let bytes3 = pkt3.to_bytes();
    let parsed3 = WirePacket::from_bytes(&bytes3).expect("parse unicode payload");
    assert_eq!(pkt3, parsed3);

    // Verify parsed payload fields exactly
    if let PacketPayload::PeerStatus(status) = parsed3.payload {
        assert_eq!(status.client_id, u32::MAX);
        assert!(status.has_media);
        assert!(!status.is_ready);
        assert_eq!(status.duration_ms, u64::MAX);
        assert_eq!(status.file_size_bytes, u64::MAX);
        assert_eq!(status.username, "オーディエンス_1 🍿");
        assert_eq!(status.filename, "すずめの戸締まり_2022_1080p.mkv");
    } else {
        panic!("expected PeerStatus payload");
    }
}

#[test]
fn test_peer_status_path_sanitization() {
    let mut host = HostServer::bind("127.0.0.1:0").expect("host bind");
    let host_addr = host.local_addr().unwrap();

    let mut client = ClientNode::bind("127.0.0.1:0", host_addr, 42).expect("client bind");

    // Client passes a full UNIX path
    client
        .send_peer_status(
            true,
            true,
            60_000,
            10_000_000,
            "Gautham",
            "/home/gautham/Videos/WatchParty/secret_movie.mp4",
        )
        .expect("send peer status");

    // Host processes incoming packet
    let mut received_pkt = None;
    for _ in 0..50 {
        if let Ok(Some((pkt, _))) = host.poll_and_process() {
            if matches!(pkt.payload, PacketPayload::PeerStatus(_)) {
                received_pkt = Some(pkt);
                break;
            }
        }
        thread::sleep(Duration::from_millis(2));
    }

    let pkt = received_pkt.expect("host should have received peer status");
    if let PacketPayload::PeerStatus(status) = pkt.payload {
        assert_eq!(status.filename, "secret_movie.mp4");
        assert!(!status.filename.contains('/'));
        assert!(!status.filename.contains("home"));
    } else {
        panic!("expected PeerStatus payload");
    }
}

#[test]
fn test_peer_status_adversarial_rejections() {
    let valid_pkt = WirePacket::peer_status(
        1,
        true,
        true,
        100_000,
        500_000_000,
        "Adversary",
        "video.mkv",
    );
    let valid_bytes = valid_pkt.to_bytes();

    // 1. Truncated payload (< 26 bytes header)
    {
        let mut malformed = valid_bytes.clone();
        malformed[6] = 20; // set declared payload_len to 20
        malformed[7] = 0;
        let new_crc = Crc32::digest(&malformed[12..12 + 20]);
        malformed[8..12].copy_from_slice(&new_crc.to_le_bytes());
        malformed.truncate(12 + 20);

        let res = WirePacket::from_bytes(&malformed);
        assert!(matches!(res, Err(SyncError::MalformedPayload(_))));
    }

    // 2. Declared username length extends past payload boundary
    {
        let mut malformed = valid_bytes.clone();
        // Offset 12 + 22 = username_len LE
        malformed[12 + 22] = 0xFF;
        malformed[12 + 23] = 0x00;
        let payload_len = u16::from_le_bytes([malformed[6], malformed[7]]) as usize;
        let new_crc = Crc32::digest(&malformed[12..12 + payload_len]);
        malformed[8..12].copy_from_slice(&new_crc.to_le_bytes());

        let res = WirePacket::from_bytes(&malformed);
        assert!(matches!(res, Err(SyncError::MalformedPayload(_))));
    }

    // 3. Invalid UTF-8 in username
    {
        let mut malformed = valid_bytes.clone();
        // First byte of username is at offset 12 + 24
        malformed[12 + 24] = 0xFF; // invalid UTF-8 start byte
        let payload_len = u16::from_le_bytes([malformed[6], malformed[7]]) as usize;
        let new_crc = Crc32::digest(&malformed[12..12 + payload_len]);
        malformed[8..12].copy_from_slice(&new_crc.to_le_bytes());

        let res = WirePacket::from_bytes(&malformed);
        assert!(matches!(res, Err(SyncError::MalformedPayload(_))));
    }

    // 4. Declared filename length mismatch
    {
        let mut malformed = valid_bytes.clone();
        // Username is "Adversary" (9 bytes).
        // Filename len is at 12 + 24 + 9 = 45.
        malformed[45] = 0x50; // declared filename 80 bytes, but actual is 9 ("video.mkv")
        malformed[46] = 0x00;
        let payload_len = u16::from_le_bytes([malformed[6], malformed[7]]) as usize;
        let new_crc = Crc32::digest(&malformed[12..12 + payload_len]);
        malformed[8..12].copy_from_slice(&new_crc.to_le_bytes());

        let res = WirePacket::from_bytes(&malformed);
        assert!(matches!(res, Err(SyncError::MalformedPayload(_))));
    }

    // 5. CRC corruption
    {
        let mut corrupted = valid_bytes.clone();
        let last_idx = corrupted.len() - 1;
        corrupted[last_idx] ^= 0x55;
        let res = WirePacket::from_bytes(&corrupted);
        assert!(matches!(res, Err(SyncError::CrcMismatch { .. })));
    }
}

#[test]
fn test_host_and_multi_client_peer_status_exchange() {
    let mut host = HostServer::bind("127.0.0.1:0").expect("host bind");
    let host_addr = host.local_addr().unwrap();

    let mut client1 = ClientNode::bind("127.0.0.1:0", host_addr, 101).expect("client1 bind");
    let mut client2 = ClientNode::bind("127.0.0.1:0", host_addr, 102).expect("client2 bind");

    // Register both clients via NTP probes
    let _ = client1.send_ntp_probe().expect("send probe 1");
    let _ = client2.send_ntp_probe().expect("send probe 2");

    for _ in 0..50 {
        let _ = host.poll_and_process();
        if host.clients().len() >= 2 {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(host.clients().len(), 2);

    // Initial state: neither client has media or is ready
    assert!(!host.are_all_users_ready());
    let unready = host.get_unready_clients();
    assert_eq!(unready.len(), 2);

    // Client 1 sends peer status: has media, but NOT ready
    client1
        .send_peer_status(
            true,
            false,
            7_200_000,
            1_500_000_000,
            "Alice",
            "dune_part_two.mkv",
        )
        .expect("client1 send status");

    // Host processes status: updates client 1 and relays to client 2
    for _ in 0..50 {
        let _ = host.poll_and_process();
        let statuses = host.peer_statuses();
        if statuses.iter().any(|s| s.client_id == 101 && s.has_media) {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }

    let statuses = host.peer_statuses();
    let c1_status = statuses.iter().find(|s| s.client_id == 101).unwrap();
    assert!(c1_status.has_media);
    assert!(!c1_status.is_ready);
    assert_eq!(c1_status.filename, "dune_part_two.mkv");
    assert_eq!(c1_status.username, "Alice");

    // Client 2 polls and receives relayed status of Client 1
    for _ in 0..50 {
        let _ = client2.poll_and_process();
        if client2.get_peer_status(101).is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }

    let relayed_c1 = client2.get_peer_status(101).expect("client 2 should receive client 1 status");
    assert_eq!(relayed_c1.client_id, 101);
    assert_eq!(relayed_c1.username, "Alice");
    assert_eq!(relayed_c1.filename, "dune_part_two.mkv");
    assert_eq!(relayed_c1.duration_ms, 7_200_000);
    assert!(relayed_c1.has_media);
    assert!(!relayed_c1.is_ready);

    // Client 1 marks itself ready
    client1
        .send_peer_status(
            true,
            true,
            7_200_000,
            1_500_000_000,
            "Alice",
            "dune_part_two.mkv",
        )
        .expect("client1 send ready");

    for _ in 0..50 {
        let _ = host.poll_and_process();
        if host.peer_statuses().iter().any(|s| s.client_id == 101 && s.is_ready) {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }

    // Now Client 1 is ready, but Client 2 is still unready
    assert!(!host.are_all_users_ready());
    let unready = host.get_unready_clients();
    assert_eq!(unready.len(), 1);
    // Client 2 has not sent PeerStatus yet, so its ID is the initial registration ID (2)
    assert_eq!(unready[0].0, 2);

    // Client 2 sends peer status: has media and is ready
    client2
        .send_peer_status(
            true,
            true,
            7_200_000,
            1_500_000_000,
            "Bob",
            "dune_part_two.mkv",
        )
        .expect("client2 send ready");

    for _ in 0..50 {
        let _ = host.poll_and_process();
        if host.are_all_users_ready() {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }

    // Now all clients have media and are ready!
    assert!(host.are_all_users_ready());
    assert!(host.get_unready_clients().is_empty());

    // Host broadcasts its own peer status to all room members
    let host_status = PeerStatusPayload::new(
        0,
        true,
        true,
        7_200_000,
        1_500_000_000,
        "HostAdmin",
        "dune_part_two.mkv",
    );
    let sent_count = host
        .broadcast_peer_status(&host_status)
        .expect("broadcast host status");
    assert_eq!(sent_count, 2);

    // Both clients receive host's status
    for _ in 0..50 {
        let _ = client1.poll_and_process();
        let _ = client2.poll_and_process();
        if client1.get_peer_status(0).is_some() && client2.get_peer_status(0).is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(client1.get_peer_status(0).unwrap().username, "HostAdmin");
    assert_eq!(client2.get_peer_status(0).unwrap().username, "HostAdmin");

    // Test poll_peer_statuses drain
    let drained1 = client1.poll_peer_statuses();
    assert!(!drained1.is_empty());
    assert!(client1.poll_peer_statuses().is_empty());
}

#[test]
fn test_barrier_unpause_independence_from_user_readiness() {
    let mut host = HostServer::bind("127.0.0.1:0").expect("host bind");
    let host_addr = host.local_addr().unwrap();

    let mut client = ClientNode::bind("127.0.0.1:0", host_addr, 5).expect("client bind");

    // Client sends PeerStatus marking user ready
    client
        .send_peer_status(true, true, 120_000, 100_000_000, "Carol", "video.mp4")
        .expect("send peer status");

    for _ in 0..50 {
        let _ = host.poll_and_process();
        if host.are_all_users_ready() {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }

    assert!(host.are_all_users_ready(), "User should be marked ready");

    // Now host initiates a two-phase barrier unpause
    let target_ms = host.prepare_barrier_unpause(200).expect("prepare barrier");
    assert!(target_ms > 0);

    // Barrier preroll readiness was reset
    assert!(!host.are_all_clients_ready(), "Barrier preroll ready must be false");

    // BUT user readiness must STILL be true!
    assert!(
        host.are_all_users_ready(),
        "User readiness must NOT be cleared by barrier unpause"
    );

    // Client processes barrier prepare and automatically sends BarrierReady
    for _ in 0..50 {
        let _ = client.poll_and_process();
        let _ = host.poll_and_process();
        if host.are_all_clients_ready() {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }

    assert!(
        host.are_all_clients_ready(),
        "Barrier preroll readiness must be achieved"
    );
    assert!(
        host.are_all_users_ready(),
        "User readiness must remain true"
    );
}
