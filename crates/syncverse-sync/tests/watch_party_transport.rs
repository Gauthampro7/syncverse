//! Integration tests for real-time watch-party transport, NTP exchanges, PLL steering, and barrier unpause.

use std::thread;
use std::time::Duration;

use syncverse_sync::{
    compute_target_unpause_server, ClientNode, HostServer, PllZone, UdpSyncTransport, WirePacket,
};

#[test]
fn test_live_watch_party_transport_integration() {
    // 1. Initialize Host and two Clients on loopback
    let mut host = HostServer::bind("127.0.0.1:0").expect("host bind");
    let host_addr = host.local_addr().expect("host local_addr");

    let mut client1 = ClientNode::bind("127.0.0.1:0", host_addr, 101).expect("client1 bind");
    let addr1 = client1.local_addr().expect("client1 local_addr");

    let mut client2 = ClientNode::bind("127.0.0.1:0", host_addr, 102).expect("client2 bind");
    let addr2 = client2.local_addr().expect("client2 local_addr");

    // Register clients on host with simulated transatlantic RTTs
    // Client 1: 300ms RTT, Client 2: 120ms RTT -> max RTT = 300ms
    host.register_client(addr1, 101, 300);
    host.register_client(addr2, 102, 120);
    assert_eq!(host.max_rtt_ms(), 300);

    // 2. Real NTP probe exchanges between Client 1 and Host
    for _ in 0..8 {
        let _ = client1.send_ntp_probe().expect("send probe");
        thread::sleep(Duration::from_millis(5));

        // Host processes probe and replies
        for _ in 0..10 {
            if host.poll_and_process().unwrap().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }

        // Client 1 processes response
        for _ in 0..10 {
            if client1.poll_and_process().unwrap().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    assert!(
        client1.filtered_offset_nanos().is_some(),
        "Client 1 must have filtered clock offset after 8 probes"
    );
    let offset_ms = client1.filtered_offset_nanos().unwrap() / 1_000_000.0;
    // On loopback, offset should be negligible (< 10ms)
    assert!(
        offset_ms.abs() < 10.0,
        "Loopback clock offset should be near 0, got {:.2}ms",
        offset_ms
    );

    // 3. Room State Broadcast and PLL Micro-Speed Steering
    host.set_media_state(10_000, 1.0, false);
    let sent = host.broadcast_room_state().expect("broadcast");
    assert_eq!(sent, 2, "Broadcast should reach both clients");

    // Client 1 polls room state
    let mut got_room = false;
    for _ in 0..20 {
        if let Some(pkt) = client1.poll_and_process().unwrap() {
            if matches!(pkt.packet_type, syncverse_sync::PacketType::RoomStateBroadcast) {
                got_room = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(got_room, "Client 1 must receive room state");

    // Simulate Client 1 lagging by 300ms (local PTS = 9,700ms vs target ~10,000ms)
    client1.set_local_pts_ms(9_700);
    let corr = client1.update_pll(0.2).expect("PLL correction");
    assert_eq!(corr.zone, PllZone::MicroSpeed, "Lag of 300ms must trigger Zone 2 MicroSpeed");
    assert!(
        corr.speed_multiplier > 1.00,
        "Lagging client must increase speed, got {:.4}x",
        corr.speed_multiplier
    );
    assert!(
        corr.speed_multiplier <= 1.04,
        "Speed multiplier must not exceed 1.04x ceiling"
    );
    assert!(
        corr.hard_seek_target.is_none(),
        "MicroSpeed must not issue hard seek"
    );

    // 4. Two-Phase Barrier Unpause Synchronization
    host.set_media_state(25_000, 1.0, true); // paused
    let preroll_budget_ms = 200u32;
    let t_server_now = host.current_server_wall_ms();
    let expected_target = compute_target_unpause_server(t_server_now, 300, preroll_budget_ms as u64);

    let scheduled_target = host
        .prepare_barrier_unpause(preroll_budget_ms)
        .expect("prepare barrier");
    assert_eq!(
        scheduled_target, expected_target,
        "Target schedule must obey T_target = t_now + 1.5*max(RTT) + preroll + 100ms"
    );

    // Clients process BarrierPrepare and reply with BarrierReady
    let mut c1_ready = false;
    for _ in 0..20 {
        if let Some(pkt) = client1.poll_and_process().unwrap() {
            if matches!(pkt.packet_type, syncverse_sync::PacketType::BarrierPrepare) {
                c1_ready = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(c1_ready, "Client 1 must process BarrierPrepare");
    assert!(client1.scheduled_unpause_local_ms().is_some());

    let mut c2_ready = false;
    for _ in 0..20 {
        if let Some(pkt) = client2.poll_and_process().unwrap() {
            if matches!(pkt.packet_type, syncverse_sync::PacketType::BarrierPrepare) {
                c2_ready = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(c2_ready, "Client 2 must process BarrierPrepare");
    assert!(client2.scheduled_unpause_local_ms().is_some());

    // Host processes BarrierReady from both clients
    for _ in 0..20 {
        let _ = host.poll_and_process();
        if host.are_all_clients_ready() {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(
        host.are_all_clients_ready(),
        "All registered clients must report ready"
    );

    // Host commits unpause
    let commit_sent = host
        .commit_barrier_unpause(scheduled_target)
        .expect("commit barrier");
    assert_eq!(commit_sent, 2);

    // Verify cross-peer scheduled local target alignment
    let c1_target = client1.scheduled_unpause_local_ms().unwrap();
    let c2_target = client2.scheduled_unpause_local_ms().unwrap();
    let target_diff = (c1_target as i64 - c2_target as i64).abs();
    assert!(
        target_diff < 10,
        "Cross-peer scheduled unpause alignment must be < 10ms, got {}ms",
        target_diff
    );

    // 5. Seek / Command Event
    let seek_pkt = WirePacket::seek_command(42, 60_000);
    let _ = client1
        .send_ntp_probe(); // make sure client transport is alive
    // Client sends seek to host
    let t1_sock = UdpSyncTransport::bind("127.0.0.1:0").unwrap();
    t1_sock.send_packet(&seek_pkt, host_addr).unwrap();

    let mut host_got_seek = false;
    for _ in 0..20 {
        if let Some((pkt, _)) = host.poll_and_process().unwrap() {
            if matches!(pkt.packet_type, syncverse_sync::PacketType::SeekCommand) {
                host_got_seek = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(host_got_seek, "Host must receive seek command");
    assert_eq!(host.ref_media_pts_ms(), 60_000);
}

#[test]
fn test_multi_peer_live_socket_simulation_diverse_preroll_and_margins() {
    let mut host = HostServer::bind("127.0.0.1:0").expect("host bind");
    let host_addr = host.local_addr().expect("host local_addr");

    let client_rtts: [(u32, u64); 5] = [
        (201, 400), // High WAN link
        (202, 280), // Transatlantic link
        (203, 150), // Cross-country link
        (204, 75),  // Regional link
        (205, 20),  // Metro / LAN link
    ];

    let mut clients: Vec<ClientNode> = client_rtts
        .iter()
        .map(|&(id, _)| ClientNode::bind("127.0.0.1:0", host_addr, id).expect("client bind"))
        .collect();

    for (i, &(_, rtt)) in client_rtts.iter().enumerate() {
        let addr = clients[i].local_addr().expect("client addr");
        host.register_client(addr, client_rtts[i].0, rtt);
    }

    assert_eq!(host.max_rtt_ms(), 400, "Max RTT across peers must be 400ms");

    // Test across diverse pre-roll budgets: 0ms, 25ms, 50ms, 100ms, 150ms, 200ms
    let preroll_budgets = [0u32, 25, 50, 100, 150, 200];

    for &preroll_budget in &preroll_budgets {
        let t_server_now = host.current_server_wall_ms();
        let expected_target =
            compute_target_unpause_server(t_server_now, 400, preroll_budget as u64);

        let scheduled_target = host
            .prepare_barrier_unpause(preroll_budget)
            .expect("prepare barrier unpause");
        assert_eq!(
            scheduled_target, expected_target,
            "Target schedule must equal t_server_now + 1.5*max(RTT) + preroll_budget + 100"
        );

        // All 5 clients poll BarrierPrepare and reply with BarrierReady
        for client in &mut clients {
            let mut got_prepare = false;
            for _ in 0..50 {
                if let Ok(Some(pkt)) = client.poll_and_process() {
                    if matches!(pkt.packet_type, syncverse_sync::PacketType::BarrierPrepare) {
                        got_prepare = true;
                        break;
                    }
                }
                thread::sleep(Duration::from_millis(1));
            }
            assert!(
                got_prepare,
                "Client {} must receive BarrierPrepare for preroll {}ms",
                client.client_id(),
                preroll_budget
            );
            assert!(
                client.scheduled_unpause_local_ms().is_some(),
                "Client {} must have scheduled unpause armed",
                client.client_id()
            );
        }

        // Host polls until all 5 clients are ready
        for _ in 0..100 {
            let _ = host.poll_and_process();
            if host.are_all_clients_ready() {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        assert!(
            host.are_all_clients_ready(),
            "All 5 clients must report ready for preroll {}ms",
            preroll_budget
        );

        // Host commits unpause
        let commits_sent = host
            .commit_barrier_unpause(scheduled_target)
            .expect("commit barrier");
        assert_eq!(commits_sent, 5, "Commit must be sent to all 5 clients");

        // Verify that across all clients, commit arrival margin is strictly >= 100ms
        for client in &mut clients {
            let mut got_commit = false;
            for _ in 0..50 {
                if let Ok(Some(pkt)) = client.poll_and_process() {
                    if matches!(pkt.packet_type, syncverse_sync::PacketType::BarrierPrepare) {
                        got_commit = true;
                        break;
                    }
                }
                thread::sleep(Duration::from_millis(1));
            }
            assert!(
                got_commit,
                "Client {} must receive Commit for preroll {}ms",
                client.client_id(),
                preroll_budget
            );

            let arrival_time = client.current_local_wall_ms();
            let scheduled_target = client.scheduled_unpause_local_ms().unwrap();
            let margin = (scheduled_target as i64) - (arrival_time as i64);

            assert!(
                margin >= 100,
                "Client {} commit arrival margin for preroll {}ms must be >= 100ms, got {}ms",
                client.client_id(),
                preroll_budget,
                margin
            );
        }
    }
}

#[test]
fn test_cross_peer_playback_start_alignment() {
    let mut host = HostServer::bind("127.0.0.1:0").expect("host bind");
    let host_addr = host.local_addr().expect("host local_addr");

    // 3 clients with different synthetic clock offsets:
    // Client 1: 0ms offset
    // Client 2: +40ms offset
    // Client 3: +80ms offset
    let mut client1 = ClientNode::bind("127.0.0.1:0", host_addr, 301).expect("c1");
    let mut client2 = ClientNode::bind("127.0.0.1:0", host_addr, 302)
        .expect("c2")
        .with_clock_offset(40_000_000);
    let mut client3 = ClientNode::bind("127.0.0.1:0", host_addr, 303)
        .expect("c3")
        .with_clock_offset(80_000_000);

    host.register_client(client1.local_addr().unwrap(), 301, 20);
    host.register_client(client2.local_addr().unwrap(), 302, 20);
    host.register_client(client3.local_addr().unwrap(), 303, 20);

    // Run NTP probe exchanges for all 3 clients to let them accurately estimate clock offsets
    for _ in 0..10 {
        let _ = client1.send_ntp_probe();
        let _ = client2.send_ntp_probe();
        let _ = client3.send_ntp_probe();
        thread::sleep(Duration::from_millis(2));

        for _ in 0..15 {
            let _ = host.poll_and_process();
        }

        for _ in 0..15 {
            let _ = client1.poll_and_process();
            let _ = client2.poll_and_process();
            let _ = client3.poll_and_process();
        }
    }

    assert!(client1.filtered_offset_nanos().is_some());
    assert!(client2.filtered_offset_nanos().is_some());
    assert!(client3.filtered_offset_nanos().is_some());

    let c1_theta_ms = client1.filtered_offset_nanos().unwrap() / 1e6;
    let c2_theta_ms = client2.filtered_offset_nanos().unwrap() / 1e6;
    let c3_theta_ms = client3.filtered_offset_nanos().unwrap() / 1e6;

    // Offsets: theta = t_server - t_client
    // c1: ~0ms, c2: ~-40ms, c3: ~-80ms (within 5ms)
    assert!(
        c1_theta_ms.abs() < 5.0,
        "c1 theta ~0ms, got {:.2}",
        c1_theta_ms
    );
    assert!(
        (c2_theta_ms - (-40.0)).abs() < 5.0,
        "c2 theta ~-40ms, got {:.2}",
        c2_theta_ms
    );
    assert!(
        (c3_theta_ms - (-80.0)).abs() < 5.0,
        "c3 theta ~-80ms, got {:.2}",
        c3_theta_ms
    );

    // Host prepares barrier unpause
    let scheduled_target = host.prepare_barrier_unpause(0).expect("prepare");

    // All clients process prepare and send ready
    for _ in 0..20 {
        let _ = client1.poll_and_process();
        let _ = client2.poll_and_process();
        let _ = client3.poll_and_process();
    }

    for _ in 0..20 {
        let _ = host.poll_and_process();
    }
    assert!(host.are_all_clients_ready());

    // Host commits
    host.commit_barrier_unpause(scheduled_target)
        .expect("commit");

    for _ in 0..20 {
        let _ = client1.poll_and_process();
        let _ = client2.poll_and_process();
        let _ = client3.poll_and_process();
    }

    // Now monitor physical start time: when does each client trigger unpause?
    let mut c1_trigger_host_ms = None;
    let mut c2_trigger_host_ms = None;
    let mut c3_trigger_host_ms = None;

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        let now_host = host.current_server_wall_ms();
        if c1_trigger_host_ms.is_none() && client1.check_scheduled_unpause() {
            c1_trigger_host_ms = Some(now_host);
        }
        if c2_trigger_host_ms.is_none() && client2.check_scheduled_unpause() {
            c2_trigger_host_ms = Some(now_host);
        }
        if c3_trigger_host_ms.is_none() && client3.check_scheduled_unpause() {
            c3_trigger_host_ms = Some(now_host);
        }

        if c1_trigger_host_ms.is_some()
            && c2_trigger_host_ms.is_some()
            && c3_trigger_host_ms.is_some()
        {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }

    assert!(c1_trigger_host_ms.is_some(), "Client 1 must unpause");
    assert!(c2_trigger_host_ms.is_some(), "Client 2 must unpause");
    assert!(c3_trigger_host_ms.is_some(), "Client 3 must unpause");

    let t1 = c1_trigger_host_ms.unwrap();
    let t2 = c2_trigger_host_ms.unwrap();
    let t3 = c3_trigger_host_ms.unwrap();

    let max_t = t1.max(t2).max(t3);
    let min_t = t1.min(t2).min(t3);
    let alignment_diff = max_t - min_t;

    assert!(
        alignment_diff < 10,
        "Cross-peer playback start alignment must be strictly < 10ms, got {}ms (t1={}, t2={}, t3={})",
        alignment_diff,
        t1,
        t2,
        t3
    );
}

#[test]
fn test_corrupted_datagram_rejection_without_panicking() {
    let mut host = HostServer::bind("127.0.0.1:0").expect("host bind");
    let host_addr = host.local_addr().expect("host local_addr");

    let mut client = ClientNode::bind("127.0.0.1:0", host_addr, 401).expect("client bind");
    let client_addr = client.local_addr().expect("client local_addr");

    let raw_sock = std::net::UdpSocket::bind("127.0.0.1:0").expect("raw sock bind");

    let valid_pkt = WirePacket::ntp_request(100, 1_000_000_000);
    let valid_bytes = valid_pkt.to_bytes();

    // 1. Magic corruption
    let mut bad_magic = valid_bytes.clone();
    bad_magic[0] = b'X';
    raw_sock.send_to(&bad_magic, host_addr).expect("send");
    thread::sleep(Duration::from_millis(2));
    let err = host
        .poll_and_process()
        .expect_err("should reject invalid magic");
    assert!(matches!(err, syncverse_sync::SyncError::InvalidMagic(_)));

    // 2. Truncated header (0 to 11 bytes)
    for len in 0..12 {
        raw_sock.send_to(&valid_bytes[..len], host_addr).expect("send");
        thread::sleep(Duration::from_millis(1));
        let err = host
            .poll_and_process()
            .expect_err("should reject truncated header");
        assert!(matches!(err, syncverse_sync::SyncError::PacketTooShort(_)));
    }

    // 3. Unknown packet type
    for &bad_type in &[0x00u8, 0x07, 0x42, 0xAA, 0xFF] {
        let mut bad_type_bytes = valid_bytes.clone();
        bad_type_bytes[4] = bad_type;
        raw_sock
            .send_to(&bad_type_bytes, host_addr)
            .expect("send");
        thread::sleep(Duration::from_millis(1));
        let err = host
            .poll_and_process()
            .expect_err("should reject unknown packet type");
        assert!(matches!(
            err,
            syncverse_sync::SyncError::UnknownPacketType(_)
        ));
    }

    // 4. Truncated payload
    let mut truncated_payload = valid_bytes.clone();
    truncated_payload.truncate(valid_bytes.len() - 3);
    raw_sock
        .send_to(&truncated_payload, host_addr)
        .expect("send");
    thread::sleep(Duration::from_millis(1));
    let err = host
        .poll_and_process()
        .expect_err("should reject truncated payload");
    assert!(matches!(
        err,
        syncverse_sync::SyncError::PayloadTruncated { .. }
    ));

    // 5. CRC32 tampering
    let mut bad_crc = valid_bytes.clone();
    bad_crc[8] ^= 0xFF;
    raw_sock.send_to(&bad_crc, host_addr).expect("send");
    thread::sleep(Duration::from_millis(1));
    let err = host
        .poll_and_process()
        .expect_err("should reject CRC mismatch");
    assert!(matches!(err, syncverse_sync::SyncError::CrcMismatch { .. }));

    // 6. Payload single-bit tampering (all 96 bits of 12-byte payload)
    for byte_idx in 12..valid_bytes.len() {
        for bit in 0..8 {
            let mut flipped = valid_bytes.clone();
            flipped[byte_idx] ^= 1 << bit;
            raw_sock.send_to(&flipped, host_addr).expect("send");
            thread::sleep(Duration::from_millis(1));
            let err = host
                .poll_and_process()
                .expect_err("should reject tampered payload");
            assert!(matches!(err, syncverse_sync::SyncError::CrcMismatch { .. }));
        }
    }

    // 7. Fuzzing with 200 random byte datagrams of various lengths
    let mut lfsr: u32 = 0x12345678;
    for _ in 0..200 {
        lfsr = lfsr.wrapping_mul(1664525).wrapping_add(1013904223);
        let len = ((lfsr >> 16) % 300) as usize;
        let mut fuzz_bytes = vec![0u8; len];
        for b in &mut fuzz_bytes {
            lfsr = lfsr.wrapping_mul(1664525).wrapping_add(1013904223);
            *b = (lfsr >> 24) as u8;
        }

        raw_sock.send_to(&fuzz_bytes, host_addr).expect("send fuzz");
        raw_sock
            .send_to(&fuzz_bytes, client_addr)
            .expect("send fuzz to client");
        thread::sleep(Duration::from_micros(500));

        let _ = host.poll_and_process();
        let _ = client.poll_and_process();
    }

    // 8. Normal operation recovery: ensure sockets remain healthy and unpoisoned
    client
        .send_ntp_probe()
        .expect("send ntp probe after fuzzing");
    thread::sleep(Duration::from_millis(2));

    let mut host_processed = false;
    for _ in 0..20 {
        if let Ok(Some((pkt, sender))) = host.poll_and_process() {
            if matches!(pkt.packet_type, syncverse_sync::PacketType::NtpProbeRequest) {
                assert_eq!(sender, client_addr);
                host_processed = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(
        host_processed,
        "Host must cleanly process legitimate packet after attacks"
    );

    let mut client_processed = false;
    for _ in 0..20 {
        if let Ok(Some(pkt)) = client.poll_and_process() {
            if matches!(pkt.packet_type, syncverse_sync::PacketType::NtpProbeResponse) {
                client_processed = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(
        client_processed,
        "Client must cleanly process response after attacks"
    );
}

#[test]
fn test_barrier_unpause_pre_roll_boundary_stress() {
    let rtt_values = [10u64, 50, 100, 200, 300, 400, 600, 1000];
    let preroll_budgets = [0u64, 10, 20, 50, 100, 150, 200];
    let t_now = 10_000u64;

    // 1. Verify standard symmetric latency model (alpha = 0.5, d_fwd = d_ret = 0.5 * RTT)
    // Under this standard network model, margin must be strictly >= 100ms for ALL combinations
    for &max_rtt in &rtt_values {
        for &budget in &preroll_budgets {
            let target_server = compute_target_unpause_server(t_now, max_rtt, budget);
            assert_eq!(
                target_server,
                t_now + (1.5 * (max_rtt as f64)) as u64 + budget + 100
            );

            for &actual_preroll in &[0u64, budget / 2, budget] {
                // Symmetric transit: 0.5 * max_rtt each way
                let d_fwd = max_rtt / 2;
                let d_ret = max_rtt - d_fwd;

                let t_server_receives_ready = t_now + d_fwd + actual_preroll + d_ret;
                let t_client_receives_commit = t_server_receives_ready + d_fwd;

                let margin = (target_server as i64) - (t_client_receives_commit as i64);

                assert!(
                    margin >= 100,
                    "Symmetric invariant violated: max_rtt={}, budget={}, actual={} -> margin={}",
                    max_rtt,
                    budget,
                    actual_preroll,
                    margin
                );
            }
        }
    }

    // 2. Adversarial Analysis of Asymmetric Latency:
    // If forward transit is slower than return (alpha > 0.5), total transit is RTT + d_fwd.
    // The margin is reduced by (alpha - 0.5) * RTT.
    // Verify that for all realistic WAN links (RTT <= 400ms) with mild asymmetry (alpha <= 0.6)
    // and full preroll budget (200ms) with fast decode (actual <= 50ms), margin remains >= 100ms.
    for &max_rtt in &[100u64, 200, 300, 400] {
        let budget = 200u64;
        let actual_preroll = 50u64;
        let alpha = 0.6; // 60% forward, 40% return
        let d_fwd = (alpha * (max_rtt as f64)) as u64;
        let d_ret = max_rtt - d_fwd;

        let target_server = compute_target_unpause_server(t_now, max_rtt, budget);
        let t_server_receives_ready = t_now + d_fwd + actual_preroll + d_ret;
        let t_client_receives_commit = t_server_receives_ready + d_fwd;
        let margin = (target_server as i64) - (t_client_receives_commit as i64);

        assert!(
            margin >= 100,
            "Mild asymmetry invariant violated: max_rtt={}, margin={}",
            max_rtt,
            margin
        );
    }
}

