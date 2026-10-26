//! Adversarial Empirical Challenge Suite: syncplay-core (Phase 2 Milestone 5)
//!
//! Stress tests:
//! 1. PlayerCoordinator Rapid Seek Storm (10,000 seeks across extreme dynamic range: 0ms .. 24h)
//! 2. PlayerCoordinator Rapid TogglePause & Speed Steering (10,000 state transitions)
//! 3. Multi-Threaded Concurrent Command Dispatch & Snapshot Polling Race
//! 4. TelemetryAggregator High-Throughput Triple-Buffer Concurrency Stress (100,000 snapshots)
//! 5. Presentation Pipeline Demuxing & Color Conversion under Extended Ticks (100 ticks)
//! 6. Watch-Party UDP Resilience under Malformed Datagram Injection
//! 7. ChatStore Disk Recovery under Mid-Stream Noise & False Sync Injection

#![deny(warnings)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use syncplay_chat::{ChatRecord, ChatStore};
use syncplay_clock::PlaybackState;
use syncplay_core::{PlayerCoordinator, SubsystemMetrics, TelemetryAggregator};
use syncplay_sync::{ClientNode, HostServer};
use syncplay_ui::{PlayerCommand, TripleBufferSnapshotExchange};

// ============================================================================
// 1. RAPID SEEK STORM & EXTREME BOUNDARY PTS STRESS
// ============================================================================

#[test]
fn test_coordinator_rapid_seek_storm_and_boundaries() {
    let mut coordinator = PlayerCoordinator::new();
    let start = Instant::now();

    // Verify initial state
    assert_eq!(coordinator.clock.state(), PlaybackState::Paused);
    coordinator
        .handle_command(PlayerCommand::TogglePause, start)
        .expect("TogglePause should succeed");
    assert_eq!(coordinator.clock.state(), PlaybackState::Playing);

    let seek_targets_ms = [
        0u64,
        1,
        15_000,
        86_400_000,          // 24 hours
        86_400_000 * 7,      // 7 days
        500,
        100_000,
        0,
        12_345_678,
        3_600_000,           // 1 hour
    ];

    let mut current_instant = start;

    // 1A. Execute 10,000 rapid seeks with pseudo-random targets across 24h dynamic range
    for i in 0..10_000 {
        current_instant += Duration::from_micros(100);
        let target = seek_targets_ms[i % seek_targets_ms.len()] + (i as u64 % 1000);

        coordinator
            .handle_command(PlayerCommand::Seek(target), current_instant)
            .expect("seek must succeed");

        let curr_pts = coordinator.clock.current_pts(current_instant);
        assert!(
            (curr_pts - target as f64).abs() < 1e-2,
            "Seek target {} ms did not match current_pts {}",
            target,
            curr_pts
        );

        let snap = coordinator.tick(current_instant);
        assert!(snap.is_internally_consistent());
        assert_eq!(snap.playback_state, 1);
        assert_eq!(snap.media_position_pts, (target as f64 * 1_000_000.0) as u64);
    }
}

// ============================================================================
// 2. RAPID TOGGLE PAUSE & SPEED STEERING TRANSITION STORM
// ============================================================================

#[test]
fn test_coordinator_rapid_toggle_pause_and_speed_transitions() {
    let mut coordinator = PlayerCoordinator::new();
    let mut current_instant = Instant::now();

    let speeds = [0.96, 0.98, 1.00, 1.02, 1.04, 1.00];

    // 10,000 alternating state transitions and speed updates
    for i in 0..10_000 {
        current_instant += Duration::from_micros(50);

        // Toggle state
        coordinator
            .handle_command(PlayerCommand::TogglePause, current_instant)
            .expect("toggle pause must succeed");

        let expected_state = if i % 2 == 0 {
            PlaybackState::Playing
        } else {
            PlaybackState::Paused
        };
        assert_eq!(coordinator.clock.state(), expected_state);

        // Set speed
        let speed = speeds[i % speeds.len()];
        coordinator
            .handle_command(PlayerCommand::SetSpeed(speed), current_instant)
            .expect("set speed must succeed");
        assert!((coordinator.clock.speed() - speed).abs() < 1e-4);

        let snap = coordinator.tick(current_instant);
        assert!(snap.is_internally_consistent());
        let expected_play_val = if expected_state == PlaybackState::Playing { 1 } else { 0 };
        assert_eq!(snap.playback_state, expected_play_val);
    }
}

// ============================================================================
// 3. TELEMETRY AGGREGATOR HIGH-THROUGHPUT CONCURRENCY STRESS (100,000 SNAPSHOTS)
// ============================================================================

#[test]
fn test_telemetry_aggregator_high_throughput_concurrency_stress() {
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let running = Arc::new(AtomicBool::new(true));
    let published_count = Arc::new(AtomicU64::new(0));

    let ex_pub = Arc::clone(&exchange);
    let run_pub = Arc::clone(&running);
    let pub_counter = Arc::clone(&published_count);

    // Publisher thread: produces 100,000 snapshots at high speed
    let publisher = thread::spawn(move || {
        let aggregator = TelemetryAggregator::new(&ex_pub);
        for seq in 1..=100_000 {
            let ts = seq * 1_000;
            let metrics = SubsystemMetrics {
                monotonic_timestamp: ts,
                media_position_pts: seq * 10_000,
                media_duration_pts: 1_000_000_000,
                playback_state: (seq % 4),
                network_rtt_nanos: 250_000_000,
                clock_offset_nanos: 15_000_000,
                phase_error_nanos: -5_000_000,
                speed_ratio: 1.015,
                peer_count: 3,
                peers_ready: 3,
                room_sequence: ts,
            };
            aggregator.publish_snapshot(&metrics);
            pub_counter.store(seq, Ordering::Release);
        }
        run_pub.store(false, Ordering::Release);
    });

    // Consumer / UI thread: continuously polls and validates snapshot consistency
    let ex_sub = Arc::clone(&exchange);
    let run_sub = Arc::clone(&running);
    let mut total_polls = 0u64;
    let mut last_seq = 0u64;

    while run_sub.load(Ordering::Acquire) || last_seq < 100_000 * 1_000 {
        let snap = ex_sub.poll_latest_snapshot();
        assert!(
            snap.is_internally_consistent(),
            "Snapshot at seq {} was not internally consistent",
            snap.room_sequence
        );
        // Sequence numbers must be monotonically non-decreasing
        assert!(
            snap.room_sequence >= last_seq,
            "Torn read or sequence regression: last {} vs current {}",
            last_seq,
            snap.room_sequence
        );
        last_seq = snap.room_sequence;
        total_polls += 1;
        if total_polls > 2_000_000 {
            break;
        }
    }

    publisher.join().expect("publisher thread should join cleanly");
    assert!(total_polls > 0);
    assert_eq!(last_seq, 100_000 * 1_000);
}

// ============================================================================
// 4. PRESENTATION PIPELINE UNDER EXTENDED TICKS & SEEK SEQUENCES
// ============================================================================

#[test]
fn test_coordinator_extended_presentation_ticks() {
    let mut coordinator = PlayerCoordinator::new();

    // 100 ticks on synthetic-full-mkv
    let snapshots = coordinator
        .run_presentation_pipeline("synthetic-full-mkv", 100)
        .expect("extended pipeline run should succeed");

    assert_eq!(snapshots.len(), 101); // 100 ticks + 1 post-seek
    let mut prev_time = 0u64;

    for (i, snap) in snapshots.iter().take(100).enumerate() {
        assert!(snap.is_internally_consistent());
        assert_eq!(snap.playback_state, 1);
        assert_eq!(snap.media_duration_pts, 1000 * 1_000_000);
        if i > 0 {
            assert!(
                snap.media_position_pts >= prev_time,
                "PTS must advance monotonically during playback tick {}: prev {} vs curr {}",
                i,
                prev_time,
                snap.media_position_pts
            );
        }
        prev_time = snap.media_position_pts;
    }

    // Post-seek snapshot
    let post_seek = snapshots.last().unwrap();
    assert_eq!(post_seek.media_position_pts, 15000 * 1_000_000);
}

// ============================================================================
// 5. WATCH-PARTY UDP TRANSPORT MALFORMED & CORRUPTED DATAGRAM RESILIENCE
// ============================================================================

#[test]
fn test_watch_party_udp_corrupted_packet_injection() {
    let mut host = HostServer::bind("127.0.0.1:0").expect("host bind failed");
    let host_addr = host.local_addr().expect("host addr failed");
    let mut client = ClientNode::bind("127.0.0.1:0", host_addr, 1).expect("client bind failed");

    // Establish initial sync
    let _ = client.send_ntp_probe().expect("send probe failed");
    let host_pkt = host.poll_and_process().expect("host poll failed");
    assert!(host_pkt.is_some());
    let client_pkt = client.poll_and_process().expect("client poll failed");
    assert!(client_pkt.is_some());

    // Inject corrupted UDP packets directly to host
    let rogue_sender = std::net::UdpSocket::bind("127.0.0.1:0").expect("rogue bind failed");

    let garbage_payloads: [&[u8]; 5] = [
        b"X",                               // 1 byte (too short)
        b"SPNT\x00\x00\x00\x00",            // 8 bytes (truncated)
        b"BADM\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00", // Bad magic
        &[0xFF; 64],                        // Random noise
        b"SPNT\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\xDE\xAD\xBE\xEF", // CRC mismatch
    ];

    for payload in &garbage_payloads {
        rogue_sender.send_to(payload, host_addr).expect("send garbage failed");
        // Host must return strongly-typed SyncError or ignore, without panicking
        let result = host.poll_and_process();
        assert!(result.is_err(), "Corrupted packet must be flagged with SyncError");
    }

    // Verify host and client remain fully operational after rogue attack
    let probe_id2 = client.send_ntp_probe().expect("second probe failed");
    assert_eq!(probe_id2, 2);
    let host_ok = host.poll_and_process().expect("host recovery failed");
    assert!(host_ok.is_some());
    let client_ok = client.poll_and_process().expect("client recovery failed");
    assert!(client_ok.is_some());
}

// ============================================================================
// 6. CHAT STORE DISK RECOVERY UNDER NOISE INJECTION & CRASH RESILIENCE
// ============================================================================

#[test]
fn test_chat_store_disk_noise_injection_and_recovery() {
    let test_log = "test_adversarial_core_chat.bin";
    let _ = std::fs::remove_file(test_log);

    // Step 1: Write 5 valid records
    {
        let mut store = ChatStore::open(test_log).expect("open failed");
        for i in 1..=5 {
            let msg = format!("Message number {} with #tag{}", i, i);
            let rec = ChatRecord::new_text(
                1_700_000_000_000 + i * 1000,
                i * 5000,
                [i as u8; 16],
                format!("User{}", i),
                &msg,
            );
            store.append(&rec).expect("append failed");
        }
        assert_eq!(store.search("#tag1").len(), 1);
        assert_eq!(store.search("#tag5").len(), 1);
        assert_eq!(store.search("Message").len(), 5);
    }

    // Step 2: Inject adversarial noise into the middle and end of the binary log
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(test_log)
            .expect("reopen file failed");

        // False sync word followed by invalid length
        f.write_all(&[0xAA, 0x55, 0x00, 0x00, 0x00, 0x00]).expect("noise 1 failed");
        // Random garbage bytes
        f.write_all(&[0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0]).expect("noise 2 failed");
        // False sync word with truncated body
        f.write_all(&[0xAA, 0x55, 0x30, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03]).expect("noise 3 failed");
    }

    // Step 3: Reopen ChatStore and verify crash scanner salvages valid records
    {
        let store = ChatStore::open(test_log).expect("reopen after corruption failed");
        assert_eq!(store.search("#tag1").len(), 1);
        assert_eq!(store.search("#tag5").len(), 1);
        assert_eq!(store.search("Message").len(), 5);
    }

    // Step 4: Append new record after corrupted section
    {
        let mut store = ChatStore::open(test_log).expect("reopen for append failed");
        let rec6 = ChatRecord::new_text(
            1_700_000_010_000,
            30_000,
            [6; 16],
            "User6".to_string(),
            "Message after recovered corruption #resilience",
        );
        store.append(&rec6).expect("append after corruption failed");

        assert_eq!(store.search("#resilience").len(), 1);
        assert_eq!(store.search("#tag1").len(), 1);
        assert_eq!(store.search("Message").len(), 6);
    }

    let _ = std::fs::remove_file(test_log);
}
