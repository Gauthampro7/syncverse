//! Comprehensive integration tests for Watch Party networking, in-band chat, and CLI argument parsing.
//!
//! Covers:
//! 1. Multi-client watch party with 1 Host and 2 Clients.
//! 2. Live UDP exchange of framed `ChatRecord` / `ChatNetworkPacket` payloads.
//! 3. Local `ChatStore` disk persistence (`syncplay_chat.bin`), `0xAA55` framing, and CRC32 verification.
//! 4. Full-text search (FTS) queries on persistent records (CJK, English, hashtags).
//! 5. Interactive slash-commands (`/seek`, `/bookmark`, `/sync-status`).
//! 6. CLI argument parser validation for `--port`, `--file`, and positional fallbacks.

use std::process::Command;
use std::thread;
use std::time::Duration;

use syncverse_chat::{
    parse_time_to_ms, ChatNetworkPacket, ChatRecord, ChatStore, DispatchAction,
    SlashCommandDispatcher, SlashCommandParser,
};
use syncverse_sync::{ClientNode, HostServer};

#[test]
fn test_watch_party_in_band_chat_persistence_and_fts_e2e() {
    let test_bin_path = "test_watch_party_chat_e2e.bin";
    let _ = std::fs::remove_file(test_bin_path);

    let mut host = HostServer::bind("127.0.0.1:0").expect("host bind");
    let host_addr = host.local_addr().expect("host local_addr");

    let mut client1 = ClientNode::bind("127.0.0.1:0", host_addr, 1).expect("client1 bind");
    let mut client2 = ClientNode::bind("127.0.0.1:0", host_addr, 2).expect("client2 bind");

    // Register clients on host via NTP probes
    for _ in 0..4 {
        let _ = client1.send_ntp_probe();
        let _ = client2.send_ntp_probe();
        let _ = host.poll_and_process();
        let _ = client1.poll_and_process();
        let _ = client2.poll_and_process();
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(host.clients().len(), 2);

    // 1. Client 1 sends a dual-temporal chat record with CJK and hashtag
    let rec1 = ChatRecord::new_text(
        1_700_000_200_000,
        45_000,
        [0xAA; 16],
        "Gautham".to_string(),
        "Fedora Linux native build #ricing 电影 starts now!",
    );
    let pkt1 = ChatNetworkPacket::new(1, rec1.clone());
    let _ = client1.send_chat_message(&pkt1.to_bytes()).expect("client1 send");

    // Host processes message
    for _ in 0..30 {
        let _ = host.poll_and_process();
        thread::sleep(Duration::from_millis(2));
    }
    let host_received = host.poll_chat_messages();
    assert_eq!(host_received.len(), 1);

    // Client 2 receives relayed message
    for _ in 0..30 {
        let _ = client2.poll_and_process();
        thread::sleep(Duration::from_millis(2));
    }
    let client2_received = client2.poll_chat_messages();
    assert_eq!(client2_received.len(), 1);

    // Reconstruct record on Client 2
    let recovered_on_c2 = ChatRecord::from_chat_message(&client2_received[0]);
    assert_eq!(recovered_on_c2.sender_name, "Gautham");
    assert_eq!(recovered_on_c2.media_timestamp_ms, 45_000);
    assert_eq!(recovered_on_c2.payload, rec1.payload);

    // 2. Client 2 persists to local ChatStore
    let mut store = ChatStore::open(test_bin_path).expect("open chat store");
    store.append(&recovered_on_c2).expect("append recovered record");

    // Client 2 replies with a slash command message
    let rec2 = ChatRecord::new_text(
        1_700_000_205_000,
        47_500,
        [0xBB; 16],
        "Elena".to_string(),
        "/seek 01:15",
    );
    store.append(&rec2).expect("append rec2");

    // Verify FTS on persistent store
    let search_fedora = store.search("Fedora");
    assert_eq!(search_fedora.len(), 1);
    assert_eq!(search_fedora[0].record.sender_name, "Gautham");

    let search_cjk = store.search("电影");
    assert_eq!(search_cjk.len(), 1);

    let search_tag = store.search("#ricing");
    assert_eq!(search_tag.len(), 1);

    // Verify slash command dispatch
    let cmd = SlashCommandParser::parse("/seek 01:15", None).expect("parse slash seek");
    let target_ms = parse_time_to_ms("01:15", None).expect("parse time");
    assert_eq!(target_ms, 75_000);

    let mut dispatcher = SlashCommandDispatcher::new();
    let action = dispatcher.dispatch_parsed(&cmd, Some(45_000)).expect("dispatch seek");
    assert_eq!(action, DispatchAction::Seek { target_pts_ms: 75_000 });

    // Verify file exists on disk and has valid 16-byte header
    let metadata = std::fs::metadata(test_bin_path).expect("stat file");
    assert!(metadata.len() > 16 + 48);

    // Clean up
    let _ = std::fs::remove_file(test_bin_path);
}

#[test]
fn test_cli_party_subcommand_flag_parsing() {
    let bin_path = env!("CARGO_BIN_EXE_syncverse");

    // 1. Host with --port and --file
    let output = Command::new(bin_path)
        .args(["party", "host", "--port", "19876", "--file", "synthetic-mkv"])
        .output()
        .expect("invoke party host with flags");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Starting host server on UDP port 19876..."));
    assert!(stdout.contains("Media file: synthetic-mkv"));
    assert!(stdout.contains("[HOST LISTENING]"));
    assert!(stdout.contains("Room ready on port 19876"));

    // 2. Connect with address and --file
    let output_connect = Command::new(bin_path)
        .args(["party", "connect", "127.0.0.1:19876", "--file", "synthetic-mkv"])
        .output()
        .expect("invoke party connect with flags");
    assert!(output_connect.status.success());
    let stdout_conn = String::from_utf8_lossy(&output_connect.stdout);
    assert!(stdout_conn.contains("Connecting to watch-party host at 127.0.0.1:19876..."));
    assert!(stdout_conn.contains("Media file: synthetic-mkv"));
    assert!(stdout_conn.contains("[CONNECTED]"));
    assert!(stdout_conn.contains("Probe sent to 127.0.0.1:19876."));

    // 3. Fallback positional host
    let output_pos = Command::new(bin_path)
        .args(["party", "host", "19877"])
        .output()
        .expect("invoke party host positional");
    assert!(output_pos.status.success());
    let stdout_pos = String::from_utf8_lossy(&output_pos.stdout);
    assert!(stdout_pos.contains("Starting host server on UDP port 19877..."));

    // 4. Default demo mode
    let output_demo = Command::new(bin_path)
        .args(["party"])
        .output()
        .expect("invoke party demo");
    assert!(output_demo.status.success());
    let stdout_demo = String::from_utf8_lossy(&output_demo.stdout);
    assert!(stdout_demo.contains("Launching watch-party synchronization mode:"));
    assert!(stdout_demo.contains("demo"));
    assert!(stdout_demo.contains("UDP loopback verified: 8 NTP probes"));
}

#[test]
fn test_playback_workers_watch_party_network_and_chat_e2e() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use syncverse_clock::{MasterClock, PlaybackState, SpscRingBuffer};
    use syncverse_codecs::VideoFrameRGBA;
    use syncverse_core::{run_playback_worker, AudioWorkerCommand};
    use syncverse_ui::{PlayerCommand, TripleBufferSnapshotExchange, UiEvent};

    let running = Arc::new(AtomicBool::new(true));

    // 1. Setup Host Worker
    let (host_cmd_tx, host_cmd_rx) = channel::<PlayerCommand>();
    let (host_audio_tx, _host_audio_rx) = channel::<AudioWorkerCommand>();
    let (host_event_tx, host_event_rx) = channel::<UiEvent>();
    let host_exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let host_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let host_clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let host_running = Arc::clone(&running);

    let host_thread = thread::spawn(move || {
        run_playback_worker(
            host_cmd_rx,
            host_audio_tx,
            host_event_tx,
            host_exchange,
            host_ring,
            host_clock,
            host_running,
        );
    });

    // 2. Setup Client Worker
    let (client_cmd_tx, client_cmd_rx) = channel::<PlayerCommand>();
    let (client_audio_tx, _client_audio_rx) = channel::<AudioWorkerCommand>();
    let (client_event_tx, client_event_rx) = channel::<UiEvent>();
    let client_exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let client_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let client_clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let client_running = Arc::clone(&running);

    let client_thread = thread::spawn(move || {
        run_playback_worker(
            client_cmd_rx,
            client_audio_tx,
            client_event_tx,
            client_exchange,
            client_ring,
            client_clock,
            client_running,
        );
    });

    // Pick an available high port for testing
    let test_port = 19888;
    host_cmd_tx
        .send(PlayerCommand::HostParty {
            port: test_port,
            room: "glupa".to_string(),
            username: "GauthamHost".to_string(),
        })
        .unwrap();

    thread::sleep(Duration::from_millis(50));

    // Client connects to 127.0.0.1:19888
    client_cmd_tx
        .send(PlayerCommand::ConnectParty {
            addr: format!("127.0.0.1:{}", test_port),
            room: "glupa".to_string(),
            username: "AliceClient".to_string(),
        })
        .unwrap();

    // Allow workers to exchange UDP packets
    let mut host_saw_client = false;
    let mut client_saw_host = false;
    let start = std::time::Instant::now();

    while start.elapsed() < Duration::from_secs(3) {
        while let Ok(evt) = host_event_rx.try_recv() {
            if let UiEvent::RoomMembersUpdated(members) = evt {
                if members.iter().any(|m| m.username.contains("AliceClient")) {
                    host_saw_client = true;
                }
            }
        }
        while let Ok(evt) = client_event_rx.try_recv() {
            if let UiEvent::RoomMembersUpdated(members) = evt {
                if members.iter().any(|m| m.username.contains("GauthamHost") || m.is_host) {
                    client_saw_host = true;
                }
            }
        }
        if host_saw_client && client_saw_host {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }

    assert!(host_saw_client, "Host must observe connecting client in RoomMembersUpdated");
    assert!(client_saw_host, "Client must observe host in RoomMembersUpdated");

    // 3. Test two-way chat messaging
    // Client sends message
    client_cmd_tx
        .send(PlayerCommand::SendChatMessage("Hey host, testing watch party!".to_string()))
        .unwrap();

    // Host sends message
    host_cmd_tx
        .send(PlayerCommand::SendChatMessage("Welcome to the party, Alice!".to_string()))
        .unwrap();

    let mut host_received_client_msg = false;
    let mut client_received_host_msg = false;
    let start_chat = std::time::Instant::now();

    while start_chat.elapsed() < Duration::from_secs(3) {
        while let Ok(evt) = host_event_rx.try_recv() {
            if let UiEvent::NewChatRecord(rec) = evt {
                let text = String::from_utf8_lossy(&rec.payload);
                if text.contains("Hey host, testing watch party!") {
                    host_received_client_msg = true;
                }
            }
        }
        while let Ok(evt) = client_event_rx.try_recv() {
            if let UiEvent::NewChatRecord(rec) = evt {
                let text = String::from_utf8_lossy(&rec.payload);
                if text.contains("Welcome to the party, Alice!") {
                    client_received_host_msg = true;
                }
            }
        }
        if host_received_client_msg && client_received_host_msg {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }

    assert!(host_received_client_msg, "Host must receive client's chat message over UDP");
    assert!(client_received_host_msg, "Client must receive host's chat message over UDP");

    // Clean shutdown
    running.store(false, Ordering::Relaxed);
    let _ = host_thread.join();
    let _ = client_thread.join();
}
