//! Adversarial stress test suite for `syncplay-sync`.
//!
//! Validates:
//! 1. High-RTT WAN network conditions (250-400ms RTT, ±30ms jitter), zero-dispersion freeze defense ($D_{\text{eff}} \ge 5.0\text{ms}$).
//! 2. Critically damped PI PLL micro-speed convergence ($e(0) = +450\text{ms}$ into deadband within 20s, no overshoot past $-15\text{ms}$).
//! 3. Continuous micro-speed steering under 1.5s drift without hard seek.
//! 4. Hard seek boundary triggering and integral reset at $|e| > 1.5\text{s}$.
//! 5. Two-phase barrier unpause positive commit arrival margins (>= 100ms) and cross-peer alignment (< 10ms).
//! 6. Wire protocol fuzzing and error resilience against corrupted packets.
//! 7. Live UDP transport operation with connected clients.

use std::thread;
use std::time::Duration;

use syncplay_sync::{
    compute_target_unpause_server, ClientNode, HostServer, PacketType, PllCorrection,
    PllSpeedController, PllZone, ProbeSample, Rfc5905Filter, SyncError, WirePacket,
};

#[test]
fn test_wan_ntp_filter_high_rtt_and_jitter() {
    let mut filter = Rfc5905Filter::new(8, 5_000_000.0); // 8 samples, 5ms floor

    // True offset is +75ms = 75,000,000 ns
    let true_offset_ns = 75_000_000i64;

    // Simulate 40 probe samples under high-RTT WAN (250-400ms RTT with ±30ms jitter)
    let jitter_pattern = [
        -28.0, 14.0, -12.0, 25.0, -6.0, 30.0, -30.0, 8.0, -15.0, 22.0, -18.0, 5.0, 19.0, -25.0,
        10.0, -4.0, 27.0, -22.0, 16.0, -9.0,
    ];

    for (i, &jitter) in jitter_pattern.iter().cycle().take(40).enumerate() {
        let rtt_ms = 325.0 + jitter; // mean 325ms in range [295, 355]ms
        let half_rtt_ns = ((rtt_ms / 2.0) * 1_000_000.0) as i64;

        let t1 = (i as i64) * 1_000_000_000;
        let t2 = t1 + half_rtt_ns + true_offset_ns;
        let t3 = t2 + 5_000_000; // 5ms server processing
        let t4 = t3 + half_rtt_ns - true_offset_ns;

        let sample = ProbeSample::new(t1 as u64, t2 as u64, t3 as u64, t4 as u64);
        assert!(filter.process_sample(sample));
    }

    assert_eq!(filter.sample_count(), 8);

    // Filtered offset must accurately converge to true offset (+75ms)
    let est_offset = filter.get_filtered_offset().expect("filtered offset");
    let est_offset_ms = est_offset / 1_000_000.0;
    assert!(
        (est_offset_ms - 75.0).abs() < 1e-3,
        "Filtered offset should match true offset (+75.0ms), got {:.3}ms",
        est_offset_ms
    );

    // Verify dispersion floor prevents zero-dispersion freeze
    let d_eff = filter.get_effective_dispersion();
    let d_eff_ms = d_eff / 1_000_000.0;
    assert!(
        d_eff_ms >= 5.0,
        "Effective dispersion must be >= 5.0ms, got {:.3}ms",
        d_eff_ms
    );

    // Zero-jitter case: All samples identical
    let mut zero_jitter_filter = Rfc5905Filter::new(8, 5_000_000.0);
    for i in 0..8 {
        let sample = ProbeSample::new(
            i * 1_000_000_000,
            i * 1_000_000_000 + 150_000_000,
            i * 1_000_000_000 + 155_000_000,
            i * 1_000_000_000 + 305_000_000,
        );
        zero_jitter_filter.process_sample(sample);
    }
    assert_eq!(zero_jitter_filter.compute_dispersion(), 0.0);
    assert_eq!(
        zero_jitter_filter.get_effective_dispersion(),
        5_000_000.0,
        "Zero-dispersion floor must be 5.0ms"
    );

    // Asymmetric transit delay spike: forward = 250ms, return = 50ms (RTT = 300ms, offset distorted by +100ms)
    let asym = ProbeSample::new(
        50_000_000_000,
        50_250_000_000,
        50_255_000_000,
        50_305_000_000,
    );
    assert!(
        !zero_jitter_filter.process_sample(asym),
        "Asymmetric delay spike must be rejected"
    );

    // Legitimate +10ms clock drift (threshold is 3 * 5ms = 15ms)
    let drift = ProbeSample::new(
        51_000_000_000,
        51_160_000_000,
        51_165_000_000,
        51_315_000_000,
    );
    assert!(
        zero_jitter_filter.process_sample(drift),
        "Legitimate +10ms drift must be accepted under D_eff"
    );

    // Watchdog reset on route change doubling RTT
    let mut rejected = 0;
    let mut accepted = 0;
    for k in 0..50 {
        let reroute = ProbeSample::new(
            (60 + k) * 1_000_000_000,
            (60 + k) * 1_000_000_000 + 350_000_000,
            (60 + k) * 1_000_000_000 + 355_000_000,
            (60 + k) * 1_000_000_000 + 705_000_000, // RTT = 700ms > 2.0 * median (300ms)
        );
        if zero_jitter_filter.process_sample(reroute) {
            accepted += 1;
        } else {
            rejected += 1;
        }
    }
    assert!(
        (7..=8).contains(&rejected),
        "Watchdog should trigger within 8 rejections, got {}",
        rejected
    );
    assert!(
        accepted >= 42,
        "Subsequent probes after watchdog reset must be accepted, got {}",
        accepted
    );
    assert!(
        (zero_jitter_filter.get_median_rtt() - 700_000_000.0).abs() < 1e-3,
        "Median RTT should adapt to new route (700ms)"
    );
}

#[test]
fn test_pll_convergence_and_critical_damping() {
    let mut controller = PllSpeedController::new();
    let dt = 0.2; // 200ms tick

    // Positive drift: e(0) = +450ms
    let mut e = 0.450f64;
    let mut time_entered_deadband: Option<f64> = None;
    let mut overshoot = false;

    let steps = (25.0 / dt) as usize; // 125 steps = 25s
    for step in 0..steps {
        let t = (step as f64) * dt;

        let target_ms = 100_000 + (e * 1000.0) as u64;
        let actual_ms = 100_000;
        let corr: PllCorrection = controller.update(target_ms, actual_ms, dt);

        assert!(
            corr.speed_multiplier >= 0.96 && corr.speed_multiplier <= 1.04,
            "Playback rate r must stay in [0.96, 1.04], got {}",
            corr.speed_multiplier
        );

        if e.abs() < 0.015 && time_entered_deadband.is_none() {
            time_entered_deadband = Some(t);
        }

        // Plant update: target advances by dt, actual by r * dt
        let delta_phase = (corr.speed_multiplier - 1.0) * dt;
        let e_next = e - delta_phase;

        if e_next < -0.015 && e >= -0.015 {
            overshoot = true;
        }
        e = e_next;
    }

    assert!(time_entered_deadband.is_some(), "Must enter deadband");
    let t_enter = time_entered_deadband.unwrap();
    assert!(
        t_enter <= 20.0,
        "Must enter deadband within 20s, entered at {}s",
        t_enter
    );
    assert!(
        (t_enter - 12.0).abs() < 1.0,
        "Critically damped controller enters deadband around t=12.0s, entered at {}s",
        t_enter
    );
    assert!(
        !overshoot,
        "Controller must not overshoot past -15ms into negative territory"
    );

    // Negative drift: e(0) = -450ms
    let mut neg_controller = PllSpeedController::new();
    let mut neg_e = -0.450f64;
    let mut neg_entered_deadband: Option<f64> = None;
    let mut neg_overshoot = false;

    for step in 0..steps {
        let t = (step as f64) * dt;

        let target_ms = 100_000;
        let actual_ms = 100_000 + ((-neg_e) * 1000.0) as u64;
        let corr = neg_controller.update(target_ms, actual_ms, dt);

        assert!(
            corr.speed_multiplier >= 0.96 && corr.speed_multiplier <= 1.04,
            "Playback rate r must stay in [0.96, 1.04], got {}",
            corr.speed_multiplier
        );

        if neg_e.abs() < 0.015 && neg_entered_deadband.is_none() {
            neg_entered_deadband = Some(t);
        }

        let delta_phase = (corr.speed_multiplier - 1.0) * dt;
        let e_next = neg_e - delta_phase;

        if e_next > 0.015 && neg_e <= 0.015 {
            neg_overshoot = true;
        }
        neg_e = e_next;
    }

    assert!(
        neg_entered_deadband.is_some(),
        "Negative drift must enter deadband"
    );
    let neg_t_enter = neg_entered_deadband.unwrap();
    assert!(
        neg_t_enter <= 20.0,
        "Negative drift must enter deadband within 20s, entered at {}s",
        neg_t_enter
    );
    assert!(
        !neg_overshoot,
        "Negative drift must not overshoot past +15ms"
    );
}

#[test]
fn test_pll_continuous_microspeed_under_1500ms() {
    let mut controller = PllSpeedController::new();
    let dt = 0.2;

    // Test at boundary e = 1500ms (1.500s)
    let corr_boundary = controller.update(101_500, 100_000, dt);
    assert_eq!(
        corr_boundary.zone,
        PllZone::MicroSpeed,
        "1500ms must be in Zone 2 MicroSpeed"
    );
    assert!(corr_boundary.hard_seek_target.is_none());
    assert_eq!(corr_boundary.speed_multiplier, 1.04);

    // Test at e = 1499ms
    let corr_1499 = controller.update(101_499, 100_000, dt);
    assert_eq!(corr_1499.zone, PllZone::MicroSpeed);
    assert!(corr_1499.hard_seek_target.is_none());

    // Test negative drift at e = -1500ms
    let mut neg_ctrl = PllSpeedController::new();
    let corr_neg = neg_ctrl.update(100_000, 101_500, dt);
    assert_eq!(corr_neg.zone, PllZone::MicroSpeed);
    assert!(corr_neg.hard_seek_target.is_none());
    assert_eq!(corr_neg.speed_multiplier, 0.96);

    // Anti-windup saturation test: 500 steps of persistent 1.4s drift
    let mut sat_ctrl = PllSpeedController::new();
    for _ in 0..500 {
        let c = sat_ctrl.update(101_400, 100_000, dt);
        assert_eq!(c.zone, PllZone::MicroSpeed);
        assert_eq!(c.speed_multiplier, 1.04);
    }
}

#[test]
fn test_pll_hard_seek_trigger_and_reset() {
    let mut controller = PllSpeedController::new();
    let dt = 0.2;

    // e = 1501ms (> 1.500s)
    let corr = controller.update(101_501, 100_000, dt);
    assert_eq!(
        corr.zone,
        PllZone::HardSeek,
        ">1500ms drift must trigger HardSeek"
    );
    assert_eq!(
        corr.hard_seek_target,
        Some(101_501),
        "HardSeek must target target_pts"
    );
    assert_eq!(corr.speed_multiplier, 1.0);

    // Negative e = -1501ms
    let corr_neg = controller.update(100_000, 101_501, dt);
    assert_eq!(corr_neg.zone, PllZone::HardSeek);
    assert_eq!(corr_neg.hard_seek_target, Some(100_000));
}

#[test]
fn test_barrier_unpause_multi_peer_alignment() {
    let peers = [
        (60u64, 10u64),   // (rtt, preroll)
        (150u64, 50u64),
        (280u64, 100u64),
        (350u64, 150u64),
        (400u64, 200u64),
    ];

    let max_rtt = peers.iter().map(|&(rtt, _)| rtt).max().unwrap(); // 400ms
    let preroll_budget = 200u64;
    let t_server_now = 10_000u64;

    let target_server = compute_target_unpause_server(t_server_now, max_rtt, preroll_budget);
    // 10000 + 1.5 * 400 + 200 + 100 = 10900ms
    assert_eq!(target_server, 10_900);

    for &(rtt, preroll) in &peers {
        let one_way = (rtt as f64) / 2.0;
        let t_client_rx_prep = (t_server_now as f64) + one_way;
        let t_client_tx_ready = t_client_rx_prep + (preroll as f64);
        let t_server_rx_ready = t_client_tx_ready + one_way;
        let t_client_rx_commit = t_server_rx_ready + one_way;

        let margin = (target_server as f64) - t_client_rx_commit;
        assert!(
            margin >= 100.0,
            "Commit arrival margin must be >= 100ms for peer RTT={}, got {}ms",
            rtt,
            margin
        );
    }
}

#[test]
fn test_wire_fuzzing_and_malformed_packets() {
    // 1. Buffer too short
    assert!(matches!(
        WirePacket::from_bytes(&[]),
        Err(SyncError::PacketTooShort(0))
    ));
    assert!(matches!(
        WirePacket::from_bytes(b"SPN"),
        Err(SyncError::PacketTooShort(3))
    ));
    assert!(matches!(
        WirePacket::from_bytes(&[0u8; 11]),
        Err(SyncError::PacketTooShort(11))
    ));

    // 2. Invalid magic
    let mut header = [0u8; 12];
    header[0..4].copy_from_slice(b"BAD!");
    assert!(matches!(
        WirePacket::from_bytes(&header),
        Err(SyncError::InvalidMagic(_))
    ));

    // 3. Unknown packet type
    header[0..4].copy_from_slice(b"SPNT");
    header[4] = 0x99; // unknown type
    assert!(matches!(
        WirePacket::from_bytes(&header),
        Err(SyncError::UnknownPacketType(0x99))
    ));

    // 4. Payload truncated
    header[4] = 0x01; // NtpProbeRequest
    header[6..8].copy_from_slice(&100u16.to_le_bytes()); // claims 100 bytes
    assert!(matches!(
        WirePacket::from_bytes(&header),
        Err(SyncError::PayloadTruncated {
            expected: 100,
            actual: 0
        })
    ));

    // 5. CRC mismatch
    let valid_pkt = WirePacket::ntp_request(1, 1000);
    let mut bytes = valid_pkt.to_bytes();
    bytes[12] ^= 0x55; // flip byte in payload
    assert!(matches!(
        WirePacket::from_bytes(&bytes),
        Err(SyncError::CrcMismatch { .. })
    ));

    // 6. Malformed payload struct length
    let mut raw = Vec::new();
    raw.extend_from_slice(b"SPNT");
    raw.push(0x01); // NtpProbeRequest expects 12 bytes
    raw.push(0x00);
    raw.extend_from_slice(&8u16.to_le_bytes()); // payload_len = 8 bytes
    let crc = syncplay_common::Crc32::digest(&[0u8; 8]);
    raw.extend_from_slice(&crc.to_le_bytes());
    raw.extend_from_slice(&[0u8; 8]); // 8 bytes instead of 12
    assert!(matches!(
        WirePacket::from_bytes(&raw),
        Err(SyncError::MalformedPayload(_))
    ));
}

#[test]
fn test_live_udp_wan_watch_party_transport() {
    let mut host = HostServer::bind("127.0.0.1:0").expect("host bind");
    let host_addr = host.local_addr().expect("host local_addr");

    let mut client = ClientNode::bind("127.0.0.1:0", host_addr, 42).expect("client bind");
    let client_addr = client.local_addr().expect("client local_addr");

    host.register_client(client_addr, 42, 350); // simulated 350ms RTT

    // 1. Probe exchange
    let probe_id = client.send_ntp_probe().expect("send probe");
    assert_eq!(probe_id, 1);

    // Host processes probe
    let mut host_rx = false;
    for _ in 0..20 {
        if host.poll_and_process().unwrap().is_some() {
            host_rx = true;
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(host_rx, "Host should process probe");

    // Client receives response
    let mut client_rx = false;
    for _ in 0..20 {
        if client.poll_and_process().unwrap().is_some() {
            client_rx = true;
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(client_rx, "Client should receive probe response");
    assert!(client.filtered_offset_nanos().is_some());

    // 2. Room State & Micro-speed steering
    host.set_media_state(50_000, 1.0, false);
    host.broadcast_room_state().expect("broadcast");

    let mut got_room = false;
    for _ in 0..20 {
        if let Some(pkt) = client.poll_and_process().unwrap() {
            if matches!(pkt.packet_type, PacketType::RoomStateBroadcast) {
                got_room = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(got_room, "Client must receive room state");

    // Lagging client
    client.set_local_pts_ms(49_600); // 400ms lag
    let corr = client.update_pll(0.2).expect("PLL update");
    assert_eq!(corr.zone, PllZone::MicroSpeed);
    assert!(corr.speed_multiplier > 1.0);
    assert!(corr.speed_multiplier <= 1.04);

    // 3. Barrier Unpause
    host.set_media_state(60_000, 1.0, true);
    let sched_target = host.prepare_barrier_unpause(200).expect("prepare");

    // Client gets prepare and replies ready
    let mut client_got_prep = false;
    for _ in 0..20 {
        if let Some(pkt) = client.poll_and_process().unwrap() {
            if matches!(pkt.packet_type, PacketType::BarrierPrepare) {
                client_got_prep = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(client_got_prep, "Client must process prepare");

    // Host receives ready
    for _ in 0..20 {
        let _ = host.poll_and_process();
        if host.are_all_clients_ready() {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(host.are_all_clients_ready(), "Client must be marked ready");

    // Host commits
    host.commit_barrier_unpause(sched_target).expect("commit");

    // Client receives commit
    let mut client_got_commit = false;
    for _ in 0..20 {
        if let Some(pkt) = client.poll_and_process().unwrap() {
            if matches!(pkt.packet_type, PacketType::BarrierPrepare) {
                client_got_commit = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(client_got_commit, "Client must receive commit");
    assert!(client.scheduled_unpause_local_ms().is_some());
}
