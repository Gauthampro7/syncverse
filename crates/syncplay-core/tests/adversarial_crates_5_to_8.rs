//! Adversarial Empirical Stress Test Suite for Crates 5 to 8:
//! - syncplay-sync
//! - syncplay-chat
//! - syncplay-ui
//! - syncplay-core

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use syncplay_chat::{
    make_file_header, ChatRecord, ChatRecoveryScanner, InvertedIndexFts,
    ParsedSlashCommand, SlashCommandParser,
};
use syncplay_core::{SubsystemMetrics, TelemetryAggregator};
use syncplay_sync::{
    compute_target_unpause_server, BarrierPhase, BarrierStateMachine,
    PllSpeedController, PllZone, ProbeSample, Rfc5905Filter,
};
use syncplay_ui::{
    BoundedChatOverlay, FontAtlasAlpha8, ScrubberDebouncer,
    TripleBufferSnapshotExchange,
};

// ============================================================================
// 1. HIGH-RTT NETWORK & NTP FILTER STRESS TEST
// ============================================================================
#[test]
fn test_high_rtt_network_ntp_filter_stress() {
    let mut filter = Rfc5905Filter::new(8, 5_000_000.0); // 8 samples, 5ms floor

    // Seed 8 samples around 300ms RTT (±30ms jitter), true offset = +50ms (50,000,000 ns)
    let jitter_offsets_ms = [-25.0, 15.0, -10.0, 20.0, -5.0, 30.0, -30.0, 5.0];
    for (i, &jitter) in jitter_offsets_ms.iter().enumerate() {
        let rtt_ms = 300.0 + jitter; // 270ms to 330ms
        let half_rtt_ns = ((rtt_ms / 2.0) * 1_000_000.0) as u64;
        let t1 = (i as u64) * 1_000_000_000;
        let t2 = t1 + half_rtt_ns + 50_000_000;
        let t3 = t2 + 5_000_000; // 5ms server turnaround
        let t4 = t3 + half_rtt_ns - 50_000_000;

        let s = ProbeSample::new(t1, t2, t3, t4);
        assert!(filter.process_sample(s));
    }

    assert_eq!(filter.sample_count(), 8);
    // Minimum RTT in window is 300 - 30 = 270ms
    let min_offset = filter.get_filtered_offset().unwrap();
    assert!(
        (min_offset - 50_000_000.0).abs() < 1e-3,
        "Filtered offset must match true offset"
    );

    // Verify dispersion floor: even if all samples have identical offset, D_effective >= 5ms
    let d_eff = filter.get_effective_dispersion();
    assert!(
        d_eff >= 5_000_000.0,
        "Effective dispersion must be >= 5ms floor, got {}",
        d_eff
    );

    // Asymmetric Delay Spike: Total RTT = 300ms, but forward = 250ms, return = 50ms
    // Error in offset: +100ms (|150ms - 50ms| = 100ms > 3 * 5ms = 15ms) -> MUST BE REJECTED
    let asym = ProbeSample::new(
        10_000_000_000,
        10_300_000_000, // forward 300ms
        10_305_000_000,
        10_305_000_000, // return 0ms
    );
    assert!(
        !filter.process_sample(asym),
        "Asymmetric delay spike must be rejected"
    );

    // Legitimate +10ms drift (offset becomes 60ms): delta = 10ms <= 3 * 5ms = 15ms -> MUST BE ACCEPTED
    let step = ProbeSample::new(
        11_000_000_000,
        11_210_000_000, // t2 = t1 + 150ms + 60ms
        11_215_000_000, // t3 = t2 + 5ms
        11_305_000_000, // t4 = t3 + 150ms - 60ms
    );
    assert_eq!(step.offset, 60_000_000.0);
    assert_eq!(step.rtt, 300_000_000.0);
    assert!(
        filter.process_sample(step),
        "Legitimate +10ms step must be accepted under D_effective floor"
    );

    // Route Reroute Watchdog Trip: Network reroutes, RTT jumps to 700ms (> 2.0 * median)
    let mut rejected = 0;
    let mut accepted = 0;
    for k in 0..50 {
        let s = ProbeSample::new(
            (12 + k) * 1_000_000_000,
            (12 + k) * 1_000_000_000 + 350_000_000,
            (12 + k) * 1_000_000_000 + 355_000_000,
            (12 + k) * 1_000_000_000 + 705_000_000, // RTT = 700ms
        );
        if filter.process_sample(s) {
            accepted += 1;
        } else {
            rejected += 1;
        }
    }
    assert!(
        rejected <= 8,
        "Watchdog must trip within 8 rejections, got {}",
        rejected
    );
    assert!(
        accepted >= 42,
        "Post-watchdog probes must be accepted, got {}",
        accepted
    );
}

// ============================================================================
// 2. PI PLL DYNAMICS, DEADZONE, OVERSHOOT & ANTI-WINDUP
// ============================================================================
#[test]
fn test_pi_pll_convergence_positive_and_negative_drift() {
    let mut controller = PllSpeedController::new();
    let dt = 0.2; // 200ms

    // Test A: Initial error +450ms
    let mut e = 0.450f64;
    let mut time_entered_deadband: Option<f64> = None;
    let mut overshoot = false;

    let steps = (25.0 / dt) as usize;
    for step in 0..steps {
        let t = (step as f64) * dt;
        let target_ms = 100_000 + (e * 1000.0) as u64;
        let actual_ms = 100_000;
        let corr = controller.update(target_ms, actual_ms, dt);

        if e.abs() < 0.015 && time_entered_deadband.is_none() {
            time_entered_deadband = Some(t);
        }

        // e[k+1] = e[k] - (r - 1.0) * dt
        let delta_phase = (corr.speed_multiplier - 1.0) * dt;
        let e_next = e - delta_phase;

        if e_next < -0.015 && e >= -0.015 {
            overshoot = true;
        }
        e = e_next;
    }

    assert!(time_entered_deadband.is_some());
    let t_entered = time_entered_deadband.unwrap();
    assert!(
        t_entered <= 20.0,
        "Must enter deadband in <= 20.0s, entered at {}s",
        t_entered
    );
    assert!(!overshoot, "Zero overshoot past -15ms allowed");

    // Test B: Negative initial error -450ms (actual is ahead of target)
    controller.reset();
    let mut e_neg = -0.450f64;
    let mut t_entered_neg: Option<f64> = None;
    let mut overshoot_pos = false;

    for step in 0..steps {
        let t = (step as f64) * dt;
        let target_ms = 100_000;
        let actual_ms = 100_000 + (-e_neg * 1000.0) as u64;
        let corr = controller.update(target_ms, actual_ms, dt);

        if e_neg.abs() < 0.015 && t_entered_neg.is_none() {
            t_entered_neg = Some(t);
        }

        let delta_phase = (corr.speed_multiplier - 1.0) * dt;
        let e_next = e_neg - delta_phase;

        if e_next > 0.015 && e_neg <= 0.015 {
            overshoot_pos = true;
        }
        e_neg = e_next;
    }

    assert!(t_entered_neg.is_some());
    let t_neg = t_entered_neg.unwrap();
    assert!(
        t_neg <= 20.0,
        "Negative drift must enter deadband in <= 20.0s, entered at {}s",
        t_neg
    );
    assert!(!overshoot_pos, "Zero positive overshoot past +15ms allowed");

    // Test C: Boundary behavior: +1490ms (Zone 2) vs +1510ms (Zone 3)
    controller.reset();
    let corr_zone2 = controller.update(101_490, 100_000, dt);
    assert_eq!(corr_zone2.zone, PllZone::MicroSpeed);
    assert_eq!(corr_zone2.speed_multiplier, 1.04);
    assert!(corr_zone2.hard_seek_target.is_none());

    let corr_zone3 = controller.update(101_510, 100_000, dt);
    assert_eq!(corr_zone3.zone, PllZone::HardSeek);
    assert_eq!(corr_zone3.hard_seek_target, Some(101_510));
}

// ============================================================================
// 3. BARRIER UNPAUSE TIMING & MULTI-PEER ALIGNMENT
// ============================================================================
#[test]
fn test_barrier_unpause_multi_peer_alignment() {
    // 3 peers with asymmetric network conditions:
    // Peer 1: RTT = 400ms, pre-roll = 200ms
    // Peer 2: RTT = 100ms, pre-roll = 50ms
    // Peer 3: RTT = 280ms, pre-roll = 120ms
    let max_rtt = 400u64;
    let preroll_budget = 200u64;
    let t_server_now = 1000u64;

    let target_server = compute_target_unpause_server(t_server_now, max_rtt, preroll_budget);
    // 1000 + 1.5 * 400 + 200 + 100 = 1900ms
    assert_eq!(target_server, 1900);

    let peers = [
        (1u64, 400u64, 200u64), // (id, rtt, preroll)
        (2u64, 100u64, 50u64),
        (3u64, 280u64, 120u64),
    ];

    for &(id, rtt, preroll) in &peers {
        let one_way = rtt / 2;
        // Server sends prepare at t = 1000
        let t_client_prepare_recv = 1000 + one_way;
        // Client executes pre-roll
        let t_client_ready_sent = t_client_prepare_recv + preroll;
        // Server receives ready
        let t_server_ready_recv = t_client_ready_sent + one_way;
        // Server broadcasts commit
        let t_client_commit_recv = t_server_ready_recv + one_way;

        let margin = (target_server as i64) - (t_client_commit_recv as i64);
        assert!(
            margin >= 100,
            "Peer {} commit arrival margin {}ms < 100ms",
            id,
            margin
        );
    }

    // State machine check
    let mut bsm = BarrierStateMachine::new(preroll_budget);
    let target = bsm.prepare_unpause(t_server_now, max_rtt, 50_000);
    assert_eq!(target, 1900);
    bsm.client_ready(1);
    bsm.client_ready(2);
    bsm.client_ready(3);
    bsm.commit(1900);
    assert_eq!(
        bsm.phase(),
        BarrierPhase::Committed {
            start_time_server_ms: 1900
        }
    );
}

// ============================================================================
// 4. SMART CHAT CRASH RECOVERY & FALSE-SYNC RESILIENCE
// ============================================================================
#[test]
fn test_smart_chat_crash_recovery_adversarial() {
    let sender_id = [42u8; 16];
    let mut valid_records = Vec::new();

    // 10 valid records
    for i in 0..10 {
        valid_records.push(ChatRecord::new_text(
            1000 + i * 20,
            i * 4000,
            sender_id,
            format!("User{}", i),
            &format!("Message content {}", i),
        ));
    }

    let mut log_bytes = Vec::new();
    log_bytes.extend_from_slice(&make_file_header(0));

    // Append records 0..3
    for r in &valid_records[0..4] {
        log_bytes.extend_from_slice(&r.serialize_record());
    }

    // Injected Corruption 1: Mid-stream false sync word [0xAA, 0x55] with enormous length (65535)
    log_bytes.extend_from_slice(&[0xAA, 0x55, 0xFF, 0xFF, 0x00, 0x00]);
    log_bytes.extend_from_slice(b"random corrupted junk payload inside database");

    // Append records 4..7
    for r in &valid_records[4..8] {
        log_bytes.extend_from_slice(&r.serialize_record());
    }

    // Injected Corruption 2: Record with CRC bit-flip
    let mut rec_flipped = valid_records[8].serialize_record();
    let last_byte = rec_flipped.len() - 1;
    rec_flipped[last_byte] ^= 0xAA; // Corrupt CRC
    log_bytes.extend_from_slice(&rec_flipped);

    // Append record 9
    log_bytes.extend_from_slice(&valid_records[9].serialize_record());

    // Injected Corruption 3: Torn write at EOF (truncate trailing record by 7 bytes)
    let torn_rec = valid_records[0].serialize_record();
    log_bytes.extend_from_slice(&torn_rec[..torn_rec.len() - 7]);

    // Run recovery scanner
    let scanner = ChatRecoveryScanner::new(&log_bytes);
    let (recovered, stats) = scanner.recover().unwrap();

    // Out of 10 valid records + 1 flipped + 1 torn:
    // Should recover exactly 9 records (0..3, 4..7, 9)
    assert_eq!(
        recovered.len(),
        9,
        "Must recover all 9 uncorrupted records despite false sync and bitflips"
    );
    assert_eq!(stats.torn_writes, 1, "Torn write at EOF must be reported");
    assert!(
        stats.corrupted_chunks >= 2,
        "Corrupted chunks must be detected"
    );
    assert_eq!(recovered[0].sender_name, "User0");
    assert_eq!(recovered[3].sender_name, "User3");
    assert_eq!(recovered[4].sender_name, "User4");
    assert_eq!(recovered[8].sender_name, "User9");
}

// ============================================================================
// 5. INVERTED INDEX FTS & SLASH COMMANDS
// ============================================================================
#[test]
fn test_inverted_index_fts_and_slash_commands() {
    let mut fts = InvertedIndexFts::new();
    let sid = [7u8; 16];

    // CJK and Western records
    fts.insert(ChatRecord::new_text(
        1000, 5000, sid, "Alice".into(),
        "Check out that awesome guitar solo at 14:30! #riff @alice",
    ));
    fts.insert(ChatRecord::new_text(
        1001, 10000, sid, "Bob".into(),
        "这是一段非常精彩的吉他独奏",
    ));
    fts.insert(ChatRecord::new_text(
        1002, 15000, sid, "Charlie".into(),
        "猫 和 狗 在 房间 里 玩耍",
    ));

    // Test English tokenization
    let en_results = fts.search("awesome guitar");
    assert_eq!(en_results.len(), 1);
    assert_eq!(en_results[0].sender_name, "Alice");

    // Test CJK bigram search: "吉他"
    let cjk_results = fts.search("吉他");
    assert_eq!(cjk_results.len(), 1);
    assert_eq!(cjk_results[0].sender_name, "Bob");

    // Test CJK single ideograph search: "猫"
    let single_cjk_results = fts.search("猫");
    assert_eq!(single_cjk_results.len(), 1);
    assert_eq!(single_cjk_results[0].sender_name, "Charlie");

    // Test hashtag & mention
    let tag_results = fts.search("#riff");
    assert_eq!(tag_results.len(), 1);
    let mention_results = fts.search("@alice");
    assert_eq!(mention_results.len(), 1);

    // Test Slash Commands
    let cmd_seek = SlashCommandParser::parse("/seek 01:23:45.500", None).unwrap();
    assert_eq!(
        cmd_seek,
        ParsedSlashCommand::Seek {
            target_pts_ms: 5025500
        }
    );

    let cmd_rel_seek = SlashCommandParser::parse("/seek +15s", Some(50_000)).unwrap();
    assert_eq!(
        cmd_rel_seek,
        ParsedSlashCommand::Seek {
            target_pts_ms: 65000
        }
    );

    let cmd_poll = SlashCommandParser::parse(
        r#"/poll "Which is best?" "Rust" "C++" "Python""#,
        None,
    )
    .unwrap();
    match cmd_poll {
        ParsedSlashCommand::Poll { question, options } => {
            assert_eq!(question, "Which is best?");
            assert_eq!(options, vec!["Rust", "C++", "Python"]);
        }
        _ => panic!("Expected Poll command"),
    }
}

// ============================================================================
// 6. SCRUBBER DEBOUNCER 120HZ DYNAMICS
// ============================================================================
#[test]
fn test_scrubber_debouncer_120hz_drag() {
    let mut debouncer = ScrubberDebouncer::new(100.0); // 10 Hz rate limit
    let total_drag_ms = 500.0;
    let event_interval_ms = 1000.0 / 120.0; // 8.333ms

    let mut intermediate_commands = Vec::new();
    let mut t = 0.0;
    while t <= total_drag_ms {
        if let Some(seek_t) = debouncer.on_drag_move(t) {
            intermediate_commands.push(seek_t);
        }
        t += event_interval_ms;
    }

    let release_seek = debouncer.on_drag_release(505.0);

    assert_eq!(
        intermediate_commands.len(),
        5,
        "Must emit exactly 5 intermediate seeks during 500ms drag at 10Hz"
    );
    assert_eq!(
        debouncer.visual_updates_count(),
        61,
        "Must update visual thumb on all 61 mouse events"
    );
    assert_eq!(
        release_seek, 505.0,
        "Must emit 6th authoritative seek on release"
    );
}

// ============================================================================
// 7. UI MEMORY BUDGET & BURST CAPPING
// ============================================================================
#[test]
fn test_ui_memory_and_burst_bounding() {
    let mut overlay = BoundedChatOverlay::new(5);
    for i in 0..1000 {
        overlay.on_message(format!("Spam reaction #{}", i), (i as f64) * 0.001);
    }

    assert!(overlay.visible_bubbles().len() <= 5);
    assert_eq!(overlay.dropped_count(), 995);

    // Texture footprint
    let atlas_2048 = FontAtlasAlpha8::new(2048, 2048);
    assert!((atlas_2048.memory_mb() - 4.0).abs() < 1e-4);

    let hud_fb_mb = 8.5;
    let snapshots_mb = 1.0;
    let total_ui_mb = atlas_2048.memory_mb() + hud_fb_mb + snapshots_mb;
    assert!(
        total_ui_mb <= 15.0,
        "Total UI RAM footprint {}MB must not exceed 15MB budget",
        total_ui_mb
    );
}

// ============================================================================
// 8. TELEMETRY AGGREGATOR & SPSC TRIPLE BUFFER CONCURRENCY
// ============================================================================
#[test]
fn test_telemetry_aggregator_and_triple_buffer_stress() {
    use std::sync::Barrier;

    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let ex_producer = Arc::clone(&exchange);

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_cons = Arc::clone(&stop_flag);

    let barrier = Arc::new(Barrier::new(2));
    let barrier_cons = Arc::clone(&barrier);

    let total_iterations = 30_000usize;

    let producer = thread::spawn(move || {
        barrier.wait();
        let aggregator = TelemetryAggregator::new(&ex_producer);
        for seq in 0..total_iterations {
            let metrics = SubsystemMetrics {
                monotonic_timestamp: seq as u64,
                media_position_pts: (seq * 1000) as u64,
                media_duration_pts: 120_000 * 1_000_000,
                playback_state: 1,
                network_rtt_nanos: 250_000_000,
                clock_offset_nanos: 50_000_000,
                phase_error_nanos: 10_000_000,
                speed_ratio: 1.0,
                peer_count: 3,
                peers_ready: 3,
                room_sequence: seq as u64,
            };
            aggregator.publish_snapshot(&metrics);
        }
        stop_flag.store(true, Ordering::Release);
    });

    let consumer = thread::spawn(move || {
        let mut read_count = 0usize;
        let mut inconsistencies = 0usize;

        barrier_cons.wait();

        loop {
            let snap = exchange.poll_latest_snapshot();
            read_count += 1;
            if !snap.is_internally_consistent() {
                inconsistencies += 1;
            }
            if stop_flag_cons.load(Ordering::Acquire) {
                break;
            }
        }

        let final_snap = exchange.poll_latest_snapshot();
        read_count += 1;
        if !final_snap.is_internally_consistent() {
            inconsistencies += 1;
        }

        (read_count, inconsistencies)
    });

    producer.join().unwrap();
    let (reads, bad) = consumer.join().unwrap();
    assert!(reads > 0);
    assert_eq!(
        bad, 0,
        "Observed {} torn snapshots under concurrent stress",
        bad
    );
}
