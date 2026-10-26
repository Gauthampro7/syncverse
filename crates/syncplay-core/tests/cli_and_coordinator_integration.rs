//! Comprehensive integration tests for syncplay-core:
//! - Container inspection across synthetic formats
//! - Presentation pipeline execution with Q8 conversion and SPSC transfer
//! - Real UDP watch party synchronization (NTP probes, PI PLL, barrier unpause)
//! - Smart chat binary log persistence, FTS search, and slash commands
//! - Telemetry aggregator triple-buffered atomic snapshots

use std::time::Instant;

use syncplay_chat::{
    export_transcript_markdown, ChatNetworkPacket, ChatRecord, ChatStore,
    SlashCommandParser, UNANCHORED_MEDIA_PTS,
};
use syncplay_clock::PlaybackState;
use syncplay_core::{
    normalize_target_path, PartyRole, PlayerCoordinator, SubsystemMetrics,
    TelemetryAggregator,
};
use syncplay_sync::{BarrierPhase, ClientNode, HostServer};
use syncplay_ui::{PlayerCommand, TripleBufferSnapshotExchange};

// ============================================================================
// 1. CONTAINER INSPECTION INTEGRATION TESTS
// ============================================================================

#[test]
fn test_inspect_all_synthetic_container_targets() {
    // 1A. Synthetic 48-byte MKV
    let ebml_info = PlayerCoordinator::inspect("synthetic-mkv")
        .expect("inspect synthetic-mkv should succeed");
    assert_eq!(ebml_info.format, "Matroska/EBML");
    assert_eq!(ebml_info.timescale, 1_000_000); // 1ms scale
    assert_eq!(ebml_info.tracks.len(), 1);
    assert_eq!(ebml_info.tracks[0].codec_id, "V_VP9");

    // 1B. Synthetic 132-byte MP4
    let mp4_info = PlayerCoordinator::inspect("synthetic-mp4")
        .expect("inspect synthetic-mp4 should succeed");
    assert_eq!(mp4_info.format, "ISOBMFF/MP4");
    assert_eq!(mp4_info.timescale, 1000);
    assert_eq!(mp4_info.tracks.len(), 1);
    assert_eq!(mp4_info.tracks[0].codec_id, "avc1");

    // 1C. Synthetic Multi-Track Full MKV
    let full_mkv = PlayerCoordinator::inspect("synthetic-full-mkv")
        .expect("inspect synthetic-full-mkv should succeed");
    assert_eq!(full_mkv.format, "Matroska/EBML");
    assert_eq!(full_mkv.tracks.len(), 2);
    assert_eq!(full_mkv.duration_ms, Some(1000));
    assert_eq!(full_mkv.keyframe_count, 2);
    assert_eq!(full_mkv.keyframe_entries.len(), 2);
    assert!(full_mkv.keyframe_entries[0].contains("PTS=0ms"));
    assert!(full_mkv.keyframe_entries[1].contains("PTS=500ms"));

    // 1D. Synthetic Multi-Track Full MP4
    let full_mp4 = PlayerCoordinator::inspect("synthetic-full-mp4")
        .expect("inspect synthetic-full-mp4 should succeed");
    assert_eq!(full_mp4.format, "ISOBMFF/MP4");
    assert_eq!(full_mp4.tracks.len(), 2);
    assert_eq!(full_mp4.duration_ms, Some(1000));
    assert_eq!(full_mp4.keyframe_count, 1);
    assert_eq!(full_mp4.keyframe_entries.len(), 2);

    // 1E. Error handling for non-existent target
    let err = PlayerCoordinator::inspect("non_existent_file_xyz.mkv");
    assert!(err.is_err(), "Non-existent file must return error");
}

// ============================================================================
// 2. PRESENTATION PIPELINE INTEGRATION TESTS
// ============================================================================

#[test]
fn test_presentation_pipeline_synthetic_mkv_and_mp4() {
    let mut coordinator = PlayerCoordinator::new();

    // 2A. Run pipeline on MKV
    let snapshots_mkv = coordinator
        .run_presentation_pipeline("synthetic-full-mkv", 6)
        .expect("pipeline on synthetic-full-mkv should succeed");

    assert_eq!(snapshots_mkv.len(), 7); // 6 ticks + 1 post-seek
    for (i, snap) in snapshots_mkv.iter().enumerate() {
        assert!(snap.is_internally_consistent(), "Snapshot {} must be internally consistent", i);
        assert_eq!(snap.playback_state, 1, "Must be in PLAYING state");
        assert_eq!(snap.media_duration_pts, 1000 * 1_000_000);
    }
    // Verify post-seek snapshot jumps to 15,000ms
    let post_seek_mkv = snapshots_mkv.last().unwrap();
    assert_eq!(post_seek_mkv.media_position_pts, 15000 * 1_000_000);

    // 2B. Run pipeline on MP4
    let mut coord_mp4 = PlayerCoordinator::new();
    let snapshots_mp4 = coord_mp4
        .run_presentation_pipeline("synthetic-full-mp4", 4)
        .expect("pipeline on synthetic-full-mp4 should succeed");

    assert_eq!(snapshots_mp4.len(), 5); // 4 ticks + 1 post-seek
    for (i, snap) in snapshots_mp4.iter().enumerate() {
        assert!(snap.is_internally_consistent(), "Snapshot {} must be internally consistent", i);
        assert_eq!(snap.playback_state, 1, "Must be in PLAYING state");
    }
}

#[test]
fn test_coordinator_interactive_commands() {
    let mut coordinator = PlayerCoordinator::new();
    let now = Instant::now();

    // Initial state: Paused
    assert_eq!(coordinator.clock.state(), PlaybackState::Paused);
    let snap0 = coordinator.tick(now);
    assert_eq!(snap0.playback_state, 0);

    // Command: TogglePause -> Playing
    coordinator
        .handle_command(PlayerCommand::TogglePause, now)
        .expect("toggle pause should succeed");
    assert_eq!(coordinator.clock.state(), PlaybackState::Playing);
    let snap1 = coordinator.tick(now);
    assert_eq!(snap1.playback_state, 1);

    // Command: Seek to 30,000ms
    coordinator
        .handle_command(PlayerCommand::Seek(30_000), now)
        .expect("seek command should succeed");
    let snap2 = coordinator.tick(now);
    assert_eq!(snap2.media_position_pts, 30_000 * 1_000_000);

    // Command: SetSpeed to 1.025x
    coordinator
        .handle_command(PlayerCommand::SetSpeed(1.025), now)
        .expect("set speed should succeed");
    assert!((coordinator.clock.speed() - 1.025).abs() < 1e-4);

    // Command: TogglePause -> Paused
    coordinator
        .handle_command(PlayerCommand::TogglePause, now)
        .expect("toggle pause should succeed");
    assert_eq!(coordinator.clock.state(), PlaybackState::Paused);
}

// ============================================================================
// 3. WATCH PARTY NETWORK SYNCHRONIZATION OVER REAL UDP
// ============================================================================

#[test]
fn test_watch_party_network_sync_e2e() {
    // Bind host on dynamic port
    let mut host = HostServer::bind("127.0.0.1:0").expect("host bind failed");
    let host_addr = host.local_addr().expect("host local_addr failed");

    // Bind client node
    let mut client = ClientNode::bind("127.0.0.1:0", host_addr, 1)
        .expect("client bind failed");

    host.set_media_state(25_000, 1.0, true);

    // 1. Exchange 8 NTP probes
    for i in 1..=8 {
        let probe_id = client.send_ntp_probe().expect("send NTP probe failed");
        assert_eq!(probe_id, i);

        let host_packet = host.poll_and_process().expect("host poll failed");
        assert!(host_packet.is_some(), "Host must receive probe #{}", i);

        let client_packet = client.poll_and_process().expect("client poll failed");
        assert!(client_packet.is_some(), "Client must receive probe response #{}", i);
    }

    // Loopback median RTT should be small (< 50ms)
    assert!(client.median_rtt_ms() < 50);

    // 2. Broadcast room state from host
    let broadcast_count = host.broadcast_room_state().expect("broadcast failed");
    assert_eq!(broadcast_count, 1, "Must broadcast to 1 client");

    let client_room_packet = client.poll_and_process().expect("client poll room state failed");
    assert!(client_room_packet.is_some());
    let room_state = client.latest_room_state().expect("room state must be present");
    assert_eq!(room_state.tau_ref_media_ms, 25_000);
    assert!(room_state.is_paused);

    // 3. Update PI PLL
    host.set_media_state(25_000, 1.0, false);
    host.broadcast_room_state().expect("broadcast unpaused failed");
    client.poll_and_process().expect("poll unpaused failed");

    let corr = client.update_pll(0.2);
    assert!(corr.is_some(), "PLL must produce correction when playing");

    // 4. Two-phase barrier unpause
    let target_unpause_server_ms = host.prepare_barrier_unpause(200)
        .expect("prepare barrier unpause failed");

    // Client receives prepare and replies with ready
    let prep_recv = client.poll_and_process().expect("client poll prepare failed");
    assert!(prep_recv.is_some());
    assert!(client.scheduled_unpause_local_ms().is_some());

    // Host receives client ready
    let ready_recv = host.poll_and_process().expect("host poll ready failed");
    assert!(ready_recv.is_some());
    assert!(host.are_all_clients_ready(), "All clients must be ready");

    // Host commits unpause
    let commit_sent = host.commit_barrier_unpause(target_unpause_server_ms)
        .expect("commit unpause failed");
    assert_eq!(commit_sent, 1);
    assert!(matches!(host.barrier_phase(), BarrierPhase::Committed { .. }));

    // Client receives commit
    let commit_recv = client.poll_and_process().expect("client poll commit failed");
    assert!(commit_recv.is_some());

    // 5. Chat network packet relay
    let test_record = ChatRecord::new_text(
        1_700_000_000_000,
        25_000,
        [7; 16],
        "WatchPartyHost".to_string(),
        "Let the movie begin!",
    );
    let net_pkt = ChatNetworkPacket::new(42, test_record);
    let wire_bytes = net_pkt.to_bytes();

    let tx_sock = std::net::UdpSocket::bind("127.0.0.1:0").expect("tx bind failed");
    let rx_sock = std::net::UdpSocket::bind("127.0.0.1:0").expect("rx bind failed");
    let rx_addr = rx_sock.local_addr().expect("rx local_addr failed");

    tx_sock.send_to(&wire_bytes, rx_addr).expect("chat send failed");
    let mut rx_buf = [0u8; 1024];
    let (n, _) = rx_sock.recv_from(&mut rx_buf).expect("chat recv failed");
    let parsed_net = ChatNetworkPacket::from_bytes(&rx_buf[..n]).expect("chat parse failed");
    assert_eq!(parsed_net.sequence, 42);
    assert_eq!(parsed_net.record.sender_name, "WatchPartyHost");
}

#[test]
fn test_coordinator_party_demo_execution() {
    let mut coordinator = PlayerCoordinator::new();
    coordinator
        .party(PartyRole::Demo)
        .expect("party demo role must succeed");
}

// ============================================================================
// 4. SMART CHAT PERSISTENCE, FTS, AND COMMANDS INTEGRATION
// ============================================================================

#[test]
fn test_chat_store_e2e_persistence_fts_and_commands() {
    let test_log = "test_integration_chat_e2e.bin";
    let _ = std::fs::remove_file(test_log);

    // 4A. Open store and append records
    let mut store = ChatStore::open(test_log).expect("open chat store failed");

    let rec1 = ChatRecord::new_text(
        1_700_000_000_000,
        15_000,
        [1; 16],
        "Alice".to_string(),
        "Check out this opening scene! #intro #cinematography",
    );
    let rec2 = ChatRecord::new_text(
        1_700_000_005_000,
        25_000,
        [2; 16],
        "Bob".to_string(),
        "这首吉他背景音乐太棒了 (This guitar background music is great!)",
    );
    let rec3 = ChatRecord::new_text(
        1_700_000_010_000,
        UNANCHORED_MEDIA_PTS,
        [3; 16],
        "Charlie".to_string(),
        "Hey everyone, welcome to the stream! #welcome",
    );

    let off1 = store.append(&rec1).expect("append rec1 failed");
    let off2 = store.append(&rec2).expect("append rec2 failed");
    let off3 = store.append(&rec3).expect("append rec3 failed");

    assert!(off2 > off1);
    assert!(off3 > off2);

    // 4B. Full-Text Search Queries
    // English word query
    let results_scene = store.search("scene");
    assert_eq!(results_scene.len(), 1);
    assert_eq!(results_scene[0].record.sender_name, "Alice");

    // Hashtag query
    let results_hashtag = store.search("#intro");
    assert_eq!(results_hashtag.len(), 1);
    assert_eq!(results_hashtag[0].record.sender_name, "Alice");

    // CJK character query (unigram / bigram)
    let results_cjk = store.search("吉他");
    assert_eq!(results_cjk.len(), 1);
    assert_eq!(results_cjk[0].record.sender_name, "Bob");

    // Single ideograph query
    let results_single_cjk = store.search("首");
    assert_eq!(results_single_cjk.len(), 1);

    // 4C. Slash Command Parsing
    let parsed_seek = SlashCommandParser::parse("/seek 00:45.000", None)
        .expect("parse /seek should succeed");
    assert!(matches!(parsed_seek, syncplay_chat::ParsedSlashCommand::Seek { target_pts_ms: 45_000 }));

    let parsed_bookmark = SlashCommandParser::parse("/bookmark Favorite Scene", Some(15_000))
        .expect("parse /bookmark should succeed");
    assert!(matches!(parsed_bookmark, syncplay_chat::ParsedSlashCommand::Bookmark { pts_ms: 15_000, .. }));

    // 4D. Transcript Markdown Export
    let transcript = export_transcript_markdown(&[rec1, rec2, rec3]);
    assert!(transcript.contains("Syncplay-Alt Room Chat Transcript"));
    assert!(transcript.contains("<Alice>: Check out this opening scene!"));
    assert!(transcript.contains("<Bob>: 这首吉他背景音乐太棒了"));
    assert!(transcript.contains("[--:--:--.---] <Charlie>: Hey everyone"));

    // 4E. Crash Recovery Verification: reopen file and verify all 3 records recovered
    drop(store);
    let recovered_store = ChatStore::open(test_log).expect("reopening chat store failed");
    let recovered_scene = recovered_store.search("scene");
    assert_eq!(recovered_scene.len(), 1);
    assert_eq!(recovered_scene[0].record.sender_name, "Alice");

    // Clean up
    let _ = std::fs::remove_file(test_log);
}

// ============================================================================
// 5. TELEMETRY AGGREGATOR & UI TRIPLE-BUFFER INTEGRATION
// ============================================================================

#[test]
fn test_telemetry_aggregator_snapshot_pipeline() {
    let exchange = TripleBufferSnapshotExchange::new();
    let aggregator = TelemetryAggregator::new(&exchange);

    for tick in 1..=50 {
        let ts_nanos = tick * 16_666_667;
        let metrics = SubsystemMetrics {
            monotonic_timestamp: ts_nanos,
            media_position_pts: tick * 20_000_000,
            media_duration_pts: 120_000_000_000,
            playback_state: 1, // Playing
            network_rtt_nanos: 320_000_000,
            clock_offset_nanos: 45_000_000,
            phase_error_nanos: 12_000_000,
            speed_ratio: 1.02,
            peer_count: 5,
            peers_ready: 5,
            room_sequence: ts_nanos,
        };

        aggregator.publish_snapshot(&metrics);
        let snapshot = exchange.poll_latest_snapshot();

        assert!(snapshot.is_internally_consistent());
        assert_eq!(snapshot.monotonic_timestamp, ts_nanos);
        assert_eq!(snapshot.media_position_pts, tick * 20_000_000);
        assert_eq!(snapshot.playback_state, 1);
        assert_eq!(snapshot.peer_count, 5);
        assert_eq!(snapshot.room_sequence, ts_nanos);
    }
}

// ============================================================================
// 6. INTERACTIVE PLAYER & DESKTOP LAUNCHER INTEGRATION
// ============================================================================

#[test]
fn test_interactive_player_presentation_loop_bounded_ticks() {
    use syncplay_core::InteractivePlayer;

    let mut player = InteractivePlayer::new("synthetic-full-mkv").with_max_ticks(Some(12));
    assert_eq!(player.target_fps, 60);
    assert_eq!(player.max_ticks, Some(12));
    assert!(player.media_info.is_some());

    let res = player.run_presentation_loop();
    assert!(res.is_ok(), "InteractivePlayer presentation loop should exit cleanly on max_ticks");
    assert!(player.coordinator.is_playing());
}

#[test]
fn test_interactive_player_keyboard_commands_and_slash_dispatch() {
    use syncplay_core::{InteractivePlayer, PlayerInputMode};
    use syncplay_ui::KeyCommand;

    let mut player = InteractivePlayer::new("synthetic-full-mkv");
    let now = Instant::now();

    // 1. Key commands in normal mode
    // Pause / Resume
    assert_eq!(player.coordinator.clock.state(), PlaybackState::Paused);
    let quit = player.handle_key_command(KeyCommand::TogglePause, now);
    assert!(!quit);
    assert_eq!(player.coordinator.clock.state(), PlaybackState::Playing);

    // Keyframe Seek (+5000ms snaps to keyframe or bounds)
    player.handle_key_command(KeyCommand::SeekRelative(5000), now);
    assert!(player.status_message.is_some());

    // Fine Seek (+1000ms)
    player.handle_key_command(KeyCommand::SeekRelative(1000), now);
    assert!(player.status_message.as_ref().unwrap().0.contains("Fine Jump"));

    // Speed shift
    player.handle_key_command(KeyCommand::ShiftSpeed(1), now);
    assert!((player.coordinator.speed() - 1.1).abs() < 1e-4);

    // Audio controls
    player.handle_key_command(KeyCommand::ToggleMute, now);
    assert!(player.is_muted);
    player.handle_key_command(KeyCommand::AdjustVolume(0.05), now);
    assert!(!player.is_muted); // Volume up unmutes

    // HUD toggles
    assert!(player.show_chat);
    player.handle_key_command(KeyCommand::ToggleChatOverlay, now);
    assert!(!player.show_chat);

    assert!(!player.show_sync_hud);
    player.handle_key_command(KeyCommand::ToggleSyncHud, now);
    assert!(player.show_sync_hud);

    // 2. CommandBar mode and slash commands
    player.handle_key_command(KeyCommand::OpenCommandPrompt, now);
    assert_eq!(player.input_mode, PlayerInputMode::CommandBar);
    assert_eq!(player.command_buffer, "/");

    // Input: "seek 00:15\n"
    player.handle_command_bar_input(b"seek 00:15\n", now);
    assert_eq!(player.input_mode, PlayerInputMode::Normal);
    assert!(player.command_buffer.is_empty());
    assert!(player.status_message.as_ref().unwrap().0.contains("[SEEK]"));

    // Input: "/bookmark EpicScene\n"
    player.input_mode = PlayerInputMode::CommandBar;
    player.handle_command_bar_input(b"/bookmark EpicScene\n", now);
    assert_eq!(player.input_mode, PlayerInputMode::Normal);
    assert!(player.status_message.as_ref().unwrap().0.contains("EpicScene"));

    // Input: "/sync-status\n"
    player.input_mode = PlayerInputMode::CommandBar;
    player.handle_command_bar_input(b"/sync-status\n", now);
    assert!(player.show_sync_hud);

    // Quit command
    let quit = player.handle_key_command(KeyCommand::Quit, now);
    assert!(quit, "Quit key command must return true");
}

#[test]
fn test_uri_target_normalization_and_coordinator_demuxing() {
    use std::fs;
    use syncplay_core::InteractivePlayer;
    use syncplay_demux::build_synthetic_full_mkv;

    let temp_dir = std::env::temp_dir();
    let file_plain = temp_dir.join("syncplay_test_uri_plain.mkv");
    let file_space = temp_dir.join("syncplay test space video.mkv");

    let mkv_bytes = build_synthetic_full_mkv();
    fs::write(&file_plain, &mkv_bytes).expect("write plain temp file");
    fs::write(&file_space, &mkv_bytes).expect("write space temp file");

    let raw_plain_path = file_plain.to_str().unwrap();
    let raw_space_path = file_space.to_str().unwrap();

    // 1. Raw path inspection baseline
    let base_info = PlayerCoordinator::inspect(raw_plain_path)
        .expect("inspect raw path should succeed");
    assert_eq!(base_info.format, "Matroska/EBML");

    // 2. URI path: file:///...
    let uri_plain = format!("file://{}", raw_plain_path);
    let uri_info = PlayerCoordinator::inspect(&uri_plain)
        .expect("inspect file:// URI should succeed");
    assert_eq!(uri_info, base_info);

    // 3. URI path with localhost: file://localhost/...
    let localhost_uri = format!("file://localhost{}", raw_plain_path);
    let localhost_info = PlayerCoordinator::inspect(&localhost_uri)
        .expect("inspect file://localhost URI should succeed");
    assert_eq!(localhost_info, base_info);

    // 4. URI path with single slash: file:/...
    let single_slash_uri = format!("file:{}", raw_plain_path);
    let single_info = PlayerCoordinator::inspect(&single_slash_uri)
        .expect("inspect file: URI should succeed");
    assert_eq!(single_info, base_info);

    // 5. Percent-encoded URI path with spaces: file:///.../syncplay%20test%20space%20video.mkv
    let encoded_space_path = raw_space_path.replace(' ', "%20");
    let uri_space = format!("file://{}", encoded_space_path);
    assert_eq!(normalize_target_path(&uri_space), raw_space_path);
    let space_info = PlayerCoordinator::inspect(&uri_space)
        .expect("inspect percent-encoded space URI should succeed");
    assert_eq!(space_info.format, "Matroska/EBML");

    // 6. Presentation pipeline execution via URI
    let mut coordinator = PlayerCoordinator::new();
    let snapshots = coordinator
        .run_presentation_pipeline(&uri_space, 3)
        .expect("run_presentation_pipeline with URI should succeed");
    assert_eq!(snapshots.len(), 4); // 3 ticks + 1 post-seek snapshot

    // 7. Interactive player initialization with URI
    let mut player = InteractivePlayer::new(&uri_space).with_max_ticks(Some(2));
    assert!(player.media_info.is_some());
    assert_eq!(player.target, raw_space_path);
    let loop_res = player.run_presentation_loop();
    assert!(loop_res.is_ok());

    // Clean up
    let _ = fs::remove_file(file_plain);
    let _ = fs::remove_file(file_space);
}

#[test]
fn test_interactive_player_multibyte_utf8_title_handling() {
    use syncplay_core::InteractivePlayer;

    let target_uri = "file:///tmp/%E6%9D%B1%E4%BA%AC_%C3%A9l%C3%A9phant.mkv";
    let normalized = normalize_target_path(target_uri);
    assert_eq!(normalized, "/tmp/東京_éléphant.mkv");

    let mut player = InteractivePlayer::new(target_uri).with_max_ticks(Some(2));
    assert_eq!(player.target, "/tmp/東京_éléphant.mkv");
    // Verify that running presentation loop doesn't panic on character boundary
    let res = player.run_presentation_loop();
    assert!(res.is_ok());
}
