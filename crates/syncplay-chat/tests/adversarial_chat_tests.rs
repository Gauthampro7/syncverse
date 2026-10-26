//! Comprehensive adversarial, fuzzing, and stress integration tests for `syncplay-chat`.

use syncplay_chat::{
    format_sync_status_report, ChatError, ChatNetworkPacket, ChatRecord, ChatRecoveryScanner,
    ChatStore, DispatchAction, SlashCommandDispatcher,
    SyncDiagnostics, UNANCHORED_MEDIA_PTS,
};

#[test]
fn test_adversarial_slash_commands_and_dispatch() {
    let mut dispatcher = SlashCommandDispatcher::new();

    // 1. /seek timestamp formats
    // HH:MM:SS
    assert_eq!(
        dispatcher.dispatch("/seek 01:23:45", None).unwrap(),
        DispatchAction::Seek { target_pts_ms: 5025000 }
    );
    // HH:MM:SS.mmm
    assert_eq!(
        dispatcher.dispatch("/seek 00:01:23.456", None).unwrap(),
        DispatchAction::Seek { target_pts_ms: 83456 }
    );
    // MM:SS
    assert_eq!(
        dispatcher.dispatch("/seek 14:30", None).unwrap(),
        DispatchAction::Seek { target_pts_ms: 870000 }
    );
    // MM:SS.mmm
    assert_eq!(
        dispatcher.dispatch("/seek 05:20.500", None).unwrap(),
        DispatchAction::Seek { target_pts_ms: 320500 }
    );
    // Raw seconds
    assert_eq!(
        dispatcher.dispatch("/seek 123.45s", None).unwrap(),
        DispatchAction::Seek { target_pts_ms: 123450 }
    );
    // Relative seek +30s
    assert_eq!(
        dispatcher.dispatch("/seek +30s", Some(100000)).unwrap(),
        DispatchAction::Seek { target_pts_ms: 130000 }
    );
    // Relative seek -15s
    assert_eq!(
        dispatcher.dispatch("/seek -15s", Some(100000)).unwrap(),
        DispatchAction::Seek { target_pts_ms: 85000 }
    );
    // Underflow clamping
    assert_eq!(
        dispatcher.dispatch("/seek -200s", Some(10000)).unwrap(),
        DispatchAction::Seek { target_pts_ms: 0 }
    );

    // 2. /bookmark with various label formats
    let act_bm1 = dispatcher.dispatch("/bookmark", Some(55000)).unwrap();
    match act_bm1 {
        DispatchAction::Bookmark(b) => {
            assert_eq!(b.label, None);
            assert_eq!(b.pts_ms, 55000);
        }
        _ => panic!("Expected Bookmark"),
    }

    let act_bm2 = dispatcher.dispatch("/bookmark \"Climax Scene\"", Some(72000)).unwrap();
    match act_bm2 {
        DispatchAction::Bookmark(b) => {
            assert_eq!(b.label.as_deref(), Some("Climax Scene"));
            assert_eq!(b.pts_ms, 72000);
        }
        _ => panic!("Expected Bookmark"),
    }

    // 3. /clip with multi-word title and validation
    let act_clip = dispatcher.dispatch("/clip 01:10 02:40 Epic Duel Sequence", None).unwrap();
    match act_clip {
        DispatchAction::Clip(c) => {
            assert_eq!(c.start_pts_ms, 70000);
            assert_eq!(c.end_pts_ms, 160000);
            assert_eq!(c.title.as_deref(), Some("Epic Duel Sequence"));
        }
        _ => panic!("Expected Clip"),
    }

    // Start > end must return error
    assert!(dispatcher.dispatch("/clip 02:00 01:00 Reversed", None).is_err());

    // 4. /poll pipe and quote notation
    let act_poll1 = dispatcher
        .dispatch("/poll Favorite Genre? | Cyberpunk | Sci-Fi | Fantasy", None)
        .unwrap();
    match act_poll1 {
        DispatchAction::Poll(p) => {
            assert_eq!(p.question, "Favorite Genre?");
            assert_eq!(p.options, vec!["Cyberpunk", "Sci-Fi", "Fantasy"]);
            assert_eq!(p.votes, vec![0, 0, 0]);
        }
        _ => panic!("Expected Poll"),
    }

    // Voting
    dispatcher.vote_poll(1).unwrap();
    dispatcher.vote_poll(1).unwrap();
    dispatcher.vote_poll(0).unwrap();
    assert_eq!(dispatcher.active_poll().unwrap().votes, vec![1, 2, 0]);
    assert!(dispatcher.vote_poll(5).is_err()); // Out of bounds

    // /poll with quotes
    let act_poll2 = dispatcher
        .dispatch(r#"/poll "Best OS?" "Fedora Linux" "OpenBSD""#, None)
        .unwrap();
    match act_poll2 {
        DispatchAction::Poll(p) => {
            assert_eq!(p.question, "Best OS?");
            assert_eq!(p.options, vec!["Fedora Linux", "OpenBSD"]);
        }
        _ => panic!("Expected Poll"),
    }

    // 5. /sync-status diagnostics
    let diag = SyncDiagnostics {
        rtt_ms: 280.4,
        clock_offset_ms: 3.12,
        speed_multiplier: 1.0018,
        peer_count: 4,
        current_pts_ms: 84000,
        is_paused: false,
        pll_zone: "MicroSpeed".to_string(),
    };
    dispatcher.set_sync_diagnostics(diag.clone());

    let act_sync = dispatcher.dispatch("/sync-status", Some(84000)).unwrap();
    match act_sync {
        DispatchAction::SyncStatus { report, diagnostics } => {
            assert_eq!(diagnostics, diag);
            assert!(report.contains("280.40 ms"));
            assert!(report.contains("+3.12 ms"));
            assert!(report.contains("1.0018x"));
            assert!(report.contains("4"));
        }
        _ => panic!("Expected SyncStatus"),
    }

    // Diagnostics report formatter standalone
    let report_str = format_sync_status_report(&diag);
    assert!(report_str.contains("Syncplay-Alt Sync Status"));

    // 6. Invalid command checks
    assert!(matches!(dispatcher.dispatch("not a slash", None), Err(ChatError::InvalidCommand(_))));
    assert!(matches!(dispatcher.dispatch("/unknown_cmd", None), Err(ChatError::InvalidCommand(_))));
    assert!(matches!(dispatcher.dispatch("/seek", None), Err(ChatError::InvalidCommand(_))));
    assert!(matches!(dispatcher.dispatch("/clip 10s", None), Err(ChatError::InvalidCommand(_))));
    assert!(matches!(dispatcher.dispatch("/poll SingleArgOnly", None), Err(ChatError::InvalidCommand(_))));
}

#[test]
fn test_adversarial_false_sync_fuzzing() {
    let sender_id = [0x42; 16];
    let mut log = Vec::new();
    log.extend_from_slice(&syncplay_chat::make_file_header(0));

    // Create 15 valid records
    let mut valid_records = Vec::new();
    for i in 0..15 {
        let rec = ChatRecord::new_text(
            1000 + (i as u64) * 10,
            (i as u64) * 3000,
            sender_id,
            format!("Sender_{:02}", i),
            &format!("Genuine message payload #{}", i),
        );
        valid_records.push(rec);
    }

    // Layout:
    // [Header: 16B]
    // [Record 0..3]
    for r in &valid_records[0..3] {
        log.extend_from_slice(&r.serialize_record());
    }

    // Adversarial Injection 1: False sync word 0xAA55 with oversized total_len (100MB)
    log.extend_from_slice(&[0xAA, 0x55, 0x00, 0x00, 0x00, 0x08]); // 0x08000000 = 134 MB

    // [Record 3..6]
    for r in &valid_records[3..6] {
        log.extend_from_slice(&r.serialize_record());
    }

    // Adversarial Injection 2: False sync word 0xAA55 with undersized total_len (12 bytes < 48)
    log.extend_from_slice(&[0xAA, 0x55, 0x0C, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04]);

    // [Record 6..9]
    for r in &valid_records[6..9] {
        log.extend_from_slice(&r.serialize_record());
    }

    // Adversarial Injection 3: Corrupted record with CRC bit-flip
    let mut bad_rec = valid_records[9].serialize_record();
    bad_rec[20] ^= 0xFF; // Corrupt payload body
    log.extend_from_slice(&bad_rec);

    // [Record 10..14]
    for r in &valid_records[10..14] {
        log.extend_from_slice(&r.serialize_record());
    }

    // Adversarial Injection 4: False sync embedded inside payload
    let sneaky_payload = b"Attacker \xAA\x55\xFF\xFF\xFF\x7F embedded sync word attack";
    let sneaky_rec = ChatRecord {
        record_type: 1,
        flags: 0,
        wall_timestamp_utc_ms: 2000,
        media_timestamp_ms: 45000,
        sender_id,
        sender_name: "SneakyUser".into(),
        payload: sneaky_payload.to_vec(),
    };
    log.extend_from_slice(&sneaky_rec.serialize_record());

    // Final valid record: Record 14
    let rec14 = valid_records[14].serialize_record();
    // Append torn write at EOF (truncate trailing 12 bytes of rec14)
    let torn_rec14 = &rec14[..rec14.len() - 12];
    log.extend_from_slice(torn_rec14);

    // Run recovery scanner
    let scanner = ChatRecoveryScanner::new(&log);
    let (recovered, stats) = scanner.recover().unwrap();

    // Expected valid records recovered:
    // Records 0..3 (3) + Records 3..6 (3) + Records 6..9 (3) + Records 10..14 (4) + Sneaky (1) = 14 records
    // Record 9 was corrupted (skipped)
    // Record 14 was truncated at EOF (torn write detected)
    assert_eq!(recovered.len(), 14, "Expected 14 valid records recovered");
    assert_eq!(stats.torn_writes, 1, "Expected torn write detected at EOF");
    assert!(stats.corrupted_chunks >= 3, "Expected at least 3 corrupted chunks detected");

    // Spot check recovered content matches exactly
    assert_eq!(recovered[0].sender_name, "Sender_00");
    assert_eq!(recovered[2].sender_name, "Sender_02");
    assert_eq!(recovered[3].sender_name, "Sender_03");
    assert_eq!(recovered[6].sender_name, "Sender_06");
    assert_eq!(recovered[9].sender_name, "Sender_10");
    assert_eq!(recovered[13].sender_name, "SneakyUser");
    assert_eq!(recovered[13].payload, sneaky_payload.to_vec());
}

#[test]
fn test_adversarial_cjk_and_multilingual_fts() {
    let mut store = ChatStore::new_in_memory();
    let sid = [0x55; 16];

    let r1 = ChatRecord::new_text(
        1000, 10000, sid, "Alice".into(),
        "这是一段非常精彩的吉他独奏 #rock @alice",
    );
    let r2 = ChatRecord::new_text(
        1010, 15000, sid, "Bob".into(),
        "我喜欢猫和狗",
    );
    let r3 = ChatRecord::new_text(
        1020, 20000, sid, "Charlie".into(),
        "東京タワーの夜景は本当に素晴らしいです！ #tokyo",
    );
    let r4 = ChatRecord::new_text(
        1030, 25000, sid, "Dave".into(),
        "한국어 자막과 함께 영화를 감상하세요.",
    );
    let r5 = ChatRecord::new_text(
        1040, 30000, sid, "Elif".into(),
        "İSTANBUL büyük bir şehirdir. #travel @elif",
    );
    let r6 = ChatRecord::new_text(
        1050, UNANCHORED_MEDIA_PTS, sid, "Frank".into(),
        "The quick brown fox jumps over the lazy dog #wildlife",
    );

    store.append(&r1).unwrap();
    store.append(&r2).unwrap();
    store.append(&r3).unwrap();
    store.append(&r4).unwrap();
    store.append(&r5).unwrap();
    store.append(&r6).unwrap();

    // 1. CJK single ideograph exemption test
    let res_cat = store.search("猫");
    assert_eq!(res_cat.len(), 1);
    assert_eq!(res_cat[0].record.sender_name, "Bob");

    let res_dog_cjk = store.search("狗");
    assert_eq!(res_dog_cjk.len(), 1);
    assert_eq!(res_dog_cjk[0].record.sender_name, "Bob");

    // 2. CJK bigram test
    let res_guitar = store.search("吉他");
    assert_eq!(res_guitar.len(), 1);
    assert_eq!(res_guitar[0].record.sender_name, "Alice");

    let res_solo = store.search("独奏");
    assert_eq!(res_solo.len(), 1);
    assert_eq!(res_solo[0].record.sender_name, "Alice");

    // 3. Multi-term cross-language Boolean AND
    let res_multi = store.search("吉他 #rock");
    assert_eq!(res_multi.len(), 1);
    assert_eq!(res_multi[0].record.sender_name, "Alice");

    // Negative Boolean AND (no message contains both)
    let res_none = store.search("吉他 #tokyo");
    assert!(res_none.is_empty());

    // 4. Japanese Kanji / Katakana search
    let res_night_view = store.search("夜景");
    assert_eq!(res_night_view.len(), 1);
    assert_eq!(res_night_view[0].record.sender_name, "Charlie");

    // 5. Korean text token search
    let res_korean = store.search("자막과");
    assert_eq!(res_korean.len(), 1);
    assert_eq!(res_korean[0].record.sender_name, "Dave");

    // 6. Turkish dotted I case folding
    let res_tr = store.search("istanbul");
    assert_eq!(res_tr.len(), 1);
    assert_eq!(res_tr[0].record.sender_name, "Elif");

    // 7. Hashtag & Mention search
    let res_tag = store.search("#tokyo");
    assert_eq!(res_tag.len(), 1);
    assert_eq!(res_tag[0].record.sender_name, "Charlie");

    let res_mention = store.search("@elif");
    assert_eq!(res_mention.len(), 1);
    assert_eq!(res_mention[0].record.sender_name, "Elif");

    // 8. English stop words removal (stop words like 'the' are filtered out)
    assert!(store.search("the").is_empty());
    let res_fox = store.search("fox");
    assert_eq!(res_fox.len(), 1);
    assert_eq!(res_fox[0].record.sender_name, "Frank");

    // 9. Timeline spatial query: window around 15,000 with past=6000, future=6000 -> [9000, 21000]
    // Should include r1 (10000), r2 (15000), r3 (20000)
    let window_records = store.query_timeline_range(15000, 6000, 6000);
    assert_eq!(window_records.len(), 3);
    assert_eq!(window_records[0].sender_name, "Alice");
    assert_eq!(window_records[1].sender_name, "Bob");
    assert_eq!(window_records[2].sender_name, "Charlie");

    // Unanchored lobby message (r6) must NOT appear in timeline overlay query
    let all_timeline = store.query_timeline_range(50000, 50000, 50000);
    assert!(!all_timeline.iter().any(|r| r.sender_name == "Frank"));
}

#[test]
fn test_network_packet_roundtrip_and_corruption() {
    let sid = [0xAA; 16];
    let record = ChatRecord::new_text(
        1_700_000_123_456,
        45_000,
        sid,
        "NetworkPeer".into(),
        "UDP sync watch-party message #live",
    );

    // 1. Direct ChatRecord network serialization roundtrip
    let net_bytes = record.to_network_bytes();
    assert_eq!(&net_bytes[0..2], &[0xAA, 0x55]);
    let parsed_record = ChatRecord::from_network_bytes(&net_bytes).unwrap();
    assert_eq!(record, parsed_record);

    // 2. ChatNetworkPacket roundtrip with sequence number
    let packet = ChatNetworkPacket::new(101, record.clone());
    let pkt_bytes = packet.to_bytes();
    assert_eq!(pkt_bytes.len(), 4 + net_bytes.len());

    let parsed_pkt = ChatNetworkPacket::from_bytes(&pkt_bytes).unwrap();
    assert_eq!(parsed_pkt.sequence, 101);
    assert_eq!(parsed_pkt.record, record);

    // 3. Corruption tests
    // Corrupt sequence header (short packet)
    assert!(ChatNetworkPacket::from_bytes(&pkt_bytes[..20]).is_err());

    // Corrupt CRC32 in record body
    let mut corrupt_crc = pkt_bytes.clone();
    let last = corrupt_crc.len() - 1;
    corrupt_crc[last] ^= 0x01;
    assert!(matches!(
        ChatNetworkPacket::from_bytes(&corrupt_crc),
        Err(ChatError::CrcMismatch)
    ));

    // Corrupt sync word
    let mut corrupt_sync = pkt_bytes.clone();
    corrupt_sync[4] = 0x00;
    assert!(matches!(
        ChatNetworkPacket::from_bytes(&corrupt_sync),
        Err(ChatError::MalformedRecord(_))
    ));
}

#[test]
fn test_file_backed_chat_store_persistence_and_recovery() {
    let temp_dir = std::env::temp_dir();
    let log_path = temp_dir.join(format!("syncplay_test_store_{}.bin", std::process::id()));

    // Clean up if existed
    let _ = std::fs::remove_file(&log_path);

    // 1. Open new store and write records
    let sid = [0x99; 16];
    {
        let mut store = ChatStore::open(&log_path).expect("failed to open chat store");
        assert_eq!(store.record_count(), 0);

        let r1 = ChatRecord::new_text(1000, 5000, sid, "UserA".into(), "First message");
        let r2 = ChatRecord::new_text(1010, 10000, sid, "UserB".into(), "Second message #awesome");
        let r3 = ChatRecord::new_text(1020, 15000, sid, "UserC".into(), "Third message #awesome");

        store.append(&r1).unwrap();
        store.append(&r2).unwrap();
        store.append(&r3).unwrap();
        assert_eq!(store.record_count(), 3);

        let search_res = store.search("#awesome");
        assert_eq!(search_res.len(), 2);
    }

    // 2. Re-open store from the same file: verifies crash recovery & index re-population
    {
        let mut store = ChatStore::open(&log_path).expect("failed to re-open store");
        assert_eq!(store.record_count(), 3);
        assert_eq!(store.records()[0].sender_name, "UserA");
        assert_eq!(store.records()[1].sender_name, "UserB");
        assert_eq!(store.records()[2].sender_name, "UserC");

        let stats = store.recovery_stats().expect("should have recovery stats");
        assert_eq!(stats.valid_records, 3);
        assert_eq!(stats.torn_writes, 0);

        // Append 2 more records
        let r4 = ChatRecord::new_text(1030, 20000, sid, "UserD".into(), "Fourth message");
        let r5 = ChatRecord::new_text(1040, 25000, sid, "UserE".into(), "Fifth message #awesome");

        store.append(&r4).unwrap();
        store.append(&r5).unwrap();
        assert_eq!(store.record_count(), 5);

        // Search across re-opened + newly appended records
        let search_awesome = store.search("#awesome");
        assert_eq!(search_awesome.len(), 3);

        // Export history
        let md = store.export_transcript_markdown();
        assert!(md.contains("UserA"));
        assert!(md.contains("UserE"));
        assert!(md.contains("[00:00:25.000] <UserE>: Fifth message #awesome"));

        let json = store.export_history_json();
        assert!(json.contains("\"sender_name\": \"UserA\""));
        assert!(json.contains("\"sender_name\": \"UserE\""));
    }

    // Clean up
    let _ = std::fs::remove_file(&log_path);
}
