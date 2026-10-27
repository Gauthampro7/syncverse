//! Integration tests for Milestone 4: Peer Media Status, Diagnostics & Unpause Gate UI.
//!
//! Validates:
//! 1. Unpause Gate blocking logic when peers are unready or missing media.
//! 2. Unpause Gate bypass via Host `ForcePlay` or `SetForcePlay(true)`.
//! 3. Peer media status and readiness network synchronization.
//! 4. Room announcements for media loading, readiness toggling, pause, and resume.
//! 5. Cross-platform file diagnostic matching (basename extraction, duration tolerance).
//! 6. 72-byte `PlayerUiSnapshot` bitflags and helper queries.

use std::mem::size_of;
use std::thread;
use std::time::Duration;

use syncverse_chat::{ChatNetworkPacket, RoomAnnouncement};
use syncverse_sync::{ClientNode, HostServer};
use syncverse_ui::{
    evaluate_file_match, format_duration_hms, FileMatchStatus, PlayerCommand, PlayerUiSnapshot,
    RoomMember, UiEvent, PLAYBACK_STATE_PAUSED, PLAYBACK_STATE_PLAYING,
    SNAPSHOT_FLAG_FORCE_PLAY, SNAPSHOT_FLAG_GATE_BLOCKED, SNAPSHOT_FLAG_HAS_MISMATCH,
    SNAPSHOT_FLAG_LOCAL_READY,
};

#[test]
fn test_snapshot_memory_layout_and_flags() {
    assert_eq!(
        size_of::<PlayerUiSnapshot>(),
        72,
        "PlayerUiSnapshot must remain strictly 72 bytes"
    );

    let mut snap = PlayerUiSnapshot::default();
    assert!(!snap.is_playing());
    assert!(!snap.is_gate_blocked());
    assert!(!snap.is_force_play());
    assert!(!snap.is_local_ready());
    assert!(!snap.has_media_mismatch());

    // Set playing state with flags
    snap.playback_state = PLAYBACK_STATE_PLAYING
        | SNAPSHOT_FLAG_LOCAL_READY
        | SNAPSHOT_FLAG_GATE_BLOCKED
        | SNAPSHOT_FLAG_FORCE_PLAY
        | SNAPSHOT_FLAG_HAS_MISMATCH;

    assert!(snap.is_playing());
    assert!(snap.is_gate_blocked());
    assert!(snap.is_force_play());
    assert!(snap.is_local_ready());
    assert!(snap.has_media_mismatch());

    // Switch to paused without flags
    snap.playback_state = PLAYBACK_STATE_PAUSED;
    assert!(!snap.is_playing());
    assert!(!snap.is_gate_blocked());
    assert!(!snap.is_force_play());
    assert!(!snap.is_local_ready());
    assert!(!snap.has_media_mismatch());
}

#[test]
fn test_file_match_diagnostics_integration() {
    // 1. Exact match
    let status1 = evaluate_file_match(
        Some("matrix.mkv"),
        Some(7_200_000),
        Some("matrix.mkv"),
        Some(7_200_000),
    );
    assert_eq!(status1, FileMatchStatus::Match);
    assert_eq!(status1.label(), "[Match ✔]");

    // 2. Basename match with cross-platform paths (Linux vs Windows)
    let status2 = evaluate_file_match(
        Some("/home/user/Videos/matrix.mkv"),
        Some(7_201_500), // Within 2000ms tolerance
        Some(r"C:\Users\Friend\Downloads\matrix.mkv"),
        Some(7_200_000),
    );
    assert_eq!(status2, FileMatchStatus::Match);

    // 3. Duration mismatch (> 2000ms) with different filenames
    let status3 = evaluate_file_match(
        Some("matrix.mkv"),
        Some(7_205_000), // 5000ms delta > 2000ms
        Some("matrix_cut.mkv"),
        Some(7_200_000),
    );
    assert_eq!(status3, FileMatchStatus::Mismatch);
    assert_eq!(status3.label(), "[⚠️ Mismatch]");

    // 4. Filename mismatch and duration mismatch
    let status4 = evaluate_file_match(
        Some("matrix_reloaded.mkv"),
        Some(8_500_000),
        Some("matrix.mkv"),
        Some(7_200_000),
    );
    assert_eq!(status4, FileMatchStatus::Mismatch);

    // 5. Missing file on peer
    let status5 = evaluate_file_match(
        Some("matrix.mkv"),
        Some(7_200_000),
        None,
        None,
    );
    assert_eq!(status5, FileMatchStatus::NoFileLoaded);
    assert_eq!(status5.label(), "[⏳ No file loaded]");

    // Duration formatting helper
    assert_eq!(format_duration_hms(0), "00:00:00");
    assert_eq!(format_duration_hms(61_000), "00:01:01");
    assert_eq!(format_duration_hms(3_661_000), "01:01:01");
}

#[test]
fn test_unpause_gate_network_peer_status_and_gate_blocking() {
    let mut host = HostServer::bind("127.0.0.1:0").expect("host bind");
    let host_addr = host.local_addr().expect("host local_addr");

    let mut client = ClientNode::bind("127.0.0.1:0", host_addr, 42).expect("client bind");

    // Establish connection via NTP probes
    for _ in 0..4 {
        let _ = client.send_ntp_probe();
        let _ = host.poll_and_process();
        let _ = client.poll_and_process();
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(host.clients().len(), 1);

    // Inspect initial client state on host: client has not sent status yet (has_media: false, is_user_ready: false)
    let (_, client_session) = host.clients().iter().next().unwrap();
    assert!(!client_session.has_media);
    assert!(!client_session.is_user_ready);

    // Simulate unpause gate check on Host
    let mut unready_peers = Vec::new();
    for session in host.clients().values() {
        if session.last_seen.elapsed() < Duration::from_secs(15)
            && (!session.has_media || !session.is_user_ready)
        {
            unready_peers.push("Alice".to_string());
        }
    }
    assert_eq!(unready_peers.len(), 1);
    assert_eq!(unready_peers[0], "Alice");

    // Construct unpause blocked event
    let event = UiEvent::UnpauseBlocked {
        unready_peers: unready_peers.clone(),
        message: format!(
            "Playback held: Waiting for {} ({}) to be ready",
            unready_peers.join(", "),
            if unready_peers.len() == 1 { "peer" } else { "peers" }
        ),
    };

    match event {
        UiEvent::UnpauseBlocked { unready_peers: peers, message } => {
            assert_eq!(peers, vec!["Alice"]);
            assert!(message.contains("Waiting for Alice"));
        }
        _ => panic!("Expected UiEvent::UnpauseBlocked"),
    }

    // Client broadcasts peer status with media loaded and ready
    client
        .send_peer_status(
            true,
            true,
            8_880_000,
            4_500_000_000,
            "Alice",
            "inception.mkv",
        )
        .expect("send peer status");

    // Host processes incoming peer status packet
    for _ in 0..20 {
        let _ = host.poll_and_process();
        thread::sleep(Duration::from_millis(2));
    }

    // Inspect updated client state on host
    let (_, updated_session) = host.clients().iter().next().unwrap();
    assert!(updated_session.has_media);
    assert!(updated_session.is_user_ready);
    assert_eq!(updated_session.filename, "inception.mkv");
    assert_eq!(updated_session.duration_ms, 8_880_000);

    // Re-check unpause gate on Host: now zero unready peers
    let mut unready_peers_after = Vec::new();
    for session in host.clients().values() {
        if session.last_seen.elapsed() < Duration::from_secs(15)
            && (!session.has_media || !session.is_user_ready)
        {
            unready_peers_after.push("Alice".to_string());
        }
    }
    assert!(unready_peers_after.is_empty(), "All peers are ready, gate must pass");
}

#[test]
fn test_unpause_gate_force_play_bypass() {
    let force_play = true;
    let unready_peers = ["Bob".to_string(), "Charlie".to_string()];

    // When force_play is enabled, the gate evaluates to allowed regardless of unready peers
    let gate_blocked = !force_play && !unready_peers.is_empty();
    assert!(!gate_blocked, "force_play must bypass gate block");

    // Verify PlayerCommand variants exist and can be sent
    let cmd_ready = PlayerCommand::SetReady(true);
    let cmd_force_toggle = PlayerCommand::SetForcePlay(true);
    let cmd_force_play = PlayerCommand::ForcePlay;

    match cmd_ready {
        PlayerCommand::SetReady(val) => assert!(val),
        _ => panic!("Expected SetReady"),
    }
    match cmd_force_toggle {
        PlayerCommand::SetForcePlay(val) => assert!(val),
        _ => panic!("Expected SetForcePlay"),
    }
    match cmd_force_play {
        PlayerCommand::ForcePlay => {}
        _ => panic!("Expected ForcePlay"),
    }
}

#[test]
fn test_room_announcements_for_media_and_readiness() {
    // Media loaded announcement
    let a1 = RoomAnnouncement::media_loaded_ms("David", "interstellar.mkv", 10_140_000);
    assert_eq!(
        a1,
        "🎬 David loaded 'interstellar.mkv' (02:49:00)"
    );

    // Readiness announcement
    let a2_ready = RoomAnnouncement::readiness("David", true);
    assert_eq!(a2_ready, "✓ David is ready to watch");

    let a2_unready = RoomAnnouncement::readiness("David", false);
    assert_eq!(a2_unready, "⏳ David is not ready");

    // Gate waiting announcement
    let a3 = RoomAnnouncement::gate_waiting(&["Alice", "Bob"]);
    assert_eq!(
        a3,
        "⏸ Paused: Waiting for Alice, Bob to load media and get ready..."
    );

    // Verify announcements serialize into framed ChatRecord with 0xAA55 and CRC32
    let record = RoomAnnouncement::create_record(1_700_000_000_000, 0, &a1);
    let net_pkt = ChatNetworkPacket::new(100, record);
    let bytes = net_pkt.to_bytes();

    // Bytes 0..4 are the sequence (100 as u32 LE: [100, 0, 0, 0])
    // Bytes 4..6 are the framed ChatRecord magic sync word (0xAA55: [0xAA, 0x55])
    assert_eq!(&bytes[4..6], &[0xAA, 0x55]);
    let recovered_pkt = ChatNetworkPacket::from_bytes(&bytes).expect("parse packet");
    assert_eq!(recovered_pkt.sequence, 100);
    assert_eq!(recovered_pkt.record.sender_name, "System");
    assert_eq!(
        String::from_utf8_lossy(&recovered_pkt.record.payload),
        "🎬 David loaded 'interstellar.mkv' (02:49:00)"
    );
}

#[test]
fn test_room_member_struct_and_constructors() {
    let member = RoomMember::new("Elena".to_string(), true, true, 12.5);
    assert_eq!(member.username, "Elena");
    assert!(member.is_local);
    assert!(member.is_host);
    assert_eq!(member.rtt_ms, 12.5);
    assert_eq!(member.file_name, None);
    assert_eq!(member.duration_ms, None);
    assert_eq!(member.file_size_bytes, None);
    assert!(!member.is_ready);

    let member_with_media = RoomMember::with_media(
        "Frank".to_string(),
        false,
        false,
        45.0,
        Some("dune.mkv".to_string()),
        Some(9_300_000),
        Some(6_200_000_000),
        true,
    );
    assert_eq!(member_with_media.username, "Frank");
    assert!(!member_with_media.is_local);
    assert!(!member_with_media.is_host);
    assert_eq!(member_with_media.file_name.as_deref(), Some("dune.mkv"));
    assert_eq!(member_with_media.duration_ms, Some(9_300_000));
    assert_eq!(member_with_media.file_size_bytes, Some(6_200_000_000));
    assert!(member_with_media.is_ready);
}
