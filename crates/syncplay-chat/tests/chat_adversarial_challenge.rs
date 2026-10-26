//! Exhaustive Empirical Adversarial Challenge Suite for `syncplay-chat` (Milestone 4).
//!
//! Stress Tests:
//! 1. False-sync resilience, high-density corruption noise, mid-stream truncation,
//!    byte-by-byte (+1) forward scanning, and authoritative EOF torn-write truncation.
//! 2. Multi-lingual Unicode CJK full-text search, Turkish dotted I, German Eszett,
//!    single ideograph indexing exemption, hashtags, mentions, and tombstone exclusion.
//! 3. BTreeMap spatial timeline window queries with O(log N + K) complexity, underflow,
//!    overflow saturation, duplicate timestamps, and unanchored record exclusion.
//! 4. Slash command parser & dispatcher fuzzing: malformed timestamps, extreme values,
//!    reversed clip intervals, multi-delimiter poll parsing, voting bounds, and sync status.
//! 5. Disk-backed ChatStore crash resilience: persistence, corruption injection, recovery,
//!    append after recovery, and transcript export.

use syncplay_chat::{
    format_sync_status_report, make_file_header, ChatError, ChatRecord,
    ChatRecoveryScanner, ChatStore, DispatchAction, InvertedIndexFts, SlashCommandDispatcher,
    SyncDiagnostics, UNANCHORED_MEDIA_PTS,
};

#[test]
fn test_false_sync_high_density_noise_and_midstream_corruption() {
    let sender_id = [0x77; 16];
    let mut log = Vec::new();
    log.extend_from_slice(&make_file_header(0));

    // Create 20 baseline valid records
    let mut valid_records = Vec::new();
    for i in 0..20 {
        let rec = ChatRecord::new_text(
            1_700_000_000_000 + i * 100,
            i * 2000,
            sender_id,
            format!("Tester_{:02}", i),
            &format!("Valid message body payload #{:02}", i),
        );
        valid_records.push(rec);
    }

    // --- Block 1: Records 0..4 (clean) ---
    for r in &valid_records[0..4] {
        log.extend_from_slice(&r.serialize_record());
    }

    // --- Noise Injection A: Sequence of false sync words ---
    // [0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55]
    log.extend_from_slice(&[0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55]);

    // --- Noise Injection B: Candidate with total_len = 0 (< 48) ---
    log.extend_from_slice(&[0xAA, 0x55, 0x00, 0x00, 0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF]);

    // --- Noise Injection C: Candidate with total_len = 47 (< 48) ---
    log.extend_from_slice(&[0xAA, 0x55, 0x2F, 0x00, 0x00, 0x00]);

    // --- Noise Injection D: Candidate claiming 4GB length (0xFFFFFFFF) ---
    log.extend_from_slice(&[0xAA, 0x55, 0xFF, 0xFF, 0xFF, 0xFF, 0x11, 0x22, 0x33, 0x44]);

    // --- Block 2: Records 4..8 (clean) ---
    for r in &valid_records[4..8] {
        log.extend_from_slice(&r.serialize_record());
    }

    // --- Noise Injection E: Candidate with valid length (60) but corrupt CRC32 ---
    let mut fake_rec = valid_records[8].serialize_record();
    let crc_idx = fake_rec.len() - 2;
    fake_rec[crc_idx] ^= 0xAA; // Bit-flip in CRC
    log.extend_from_slice(&fake_rec);

    // --- Noise Injection F: Candidate with body corrupt (CRC mismatch) ---
    let mut bad_body_rec = valid_records[9].serialize_record();
    bad_body_rec[15] ^= 0x55; // Bit-flip in body
    log.extend_from_slice(&bad_body_rec);

    // --- Block 3: Records 10..14 (clean) ---
    for r in &valid_records[10..14] {
        log.extend_from_slice(&r.serialize_record());
    }

    // --- Noise Injection G: Mid-stream truncated record ---
    // Take record 14, write only 25 bytes out of its full size, followed immediately by valid records!
    let full_r14 = valid_records[14].serialize_record();
    log.extend_from_slice(&full_r14[..25]);

    // --- Block 4: Records 15..19 (clean) ---
    for r in &valid_records[15..19] {
        log.extend_from_slice(&r.serialize_record());
    }

    // --- Noise Injection H: Authoritative trailing incomplete write at EOF ---
    // Take record 19, truncate trailing 8 bytes (so header + partial body exists at EOF)
    let full_r19 = valid_records[19].serialize_record();
    log.extend_from_slice(&full_r19[..full_r19.len() - 8]);

    // Execute ChatRecoveryScanner
    let scanner = ChatRecoveryScanner::new(&log);
    let (recovered, stats) = scanner.recover().expect("recovery should succeed");

    // Expected valid records recovered:
    // Block 1: 0..4 (4 records: 0, 1, 2, 3)
    // Block 2: 4..8 (4 records: 4, 5, 6, 7)
    // Block 3: 10..14 (4 records: 10, 11, 12, 13)
    // Block 4: 15..19 (4 records: 15, 16, 17, 18)
    // Total = 16 valid records recovered!
    // (Records 8, 9, 14, 19 were corrupted or truncated)
    assert_eq!(recovered.len(), 16, "Should recover exactly 16 valid records");
    assert_eq!(stats.torn_writes, 1, "Authoritative EOF torn write must be flagged");
    assert!(
        stats.corrupted_chunks >= 6,
        "Should flag at least 6 corrupted chunks/false-syncs, got {}",
        stats.corrupted_chunks
    );

    // Verify ordering and content fidelity
    assert_eq!(recovered[0].sender_name, "Tester_00");
    assert_eq!(recovered[3].sender_name, "Tester_03");
    assert_eq!(recovered[4].sender_name, "Tester_04");
    assert_eq!(recovered[7].sender_name, "Tester_07");
    assert_eq!(recovered[8].sender_name, "Tester_10");
    assert_eq!(recovered[11].sender_name, "Tester_13");
    assert_eq!(recovered[12].sender_name, "Tester_15");
    assert_eq!(recovered[15].sender_name, "Tester_18");
}

#[test]
fn test_recovery_scanner_header_and_short_file_edge_cases() {
    // 1. File shorter than 16 bytes
    let empty_buf = [];
    let (recs, stats) = ChatRecoveryScanner::new(&empty_buf).recover().unwrap();
    assert_eq!(recs.len(), 0);
    assert_eq!(stats.torn_writes, 0);

    let five_bytes = [1, 2, 3, 4, 5];
    let (recs5, stats5) = ChatRecoveryScanner::new(&five_bytes).recover().unwrap();
    assert_eq!(recs5.len(), 0);
    assert_eq!(stats5.torn_writes, 0);

    // 2. Exactly 16-byte valid header with 0 records
    let hdr = make_file_header(0);
    let (recs_hdr, stats_hdr) = ChatRecoveryScanner::new(&hdr).recover().unwrap();
    assert_eq!(recs_hdr.len(), 0);
    assert_eq!(stats_hdr.torn_writes, 0);

    // 3. Invalid magic
    let mut bad_magic = hdr;
    bad_magic[0] = b'X';
    assert!(matches!(
        ChatRecoveryScanner::new(&bad_magic).recover(),
        Err(ChatError::InvalidHeaderMagic)
    ));

    // 4. Header CRC mismatch
    let mut bad_crc = hdr;
    bad_crc[12] ^= 0xFF;
    assert!(matches!(
        ChatRecoveryScanner::new(&bad_crc).recover(),
        Err(ChatError::HeaderCrcMismatch)
    ));

    // 5. Header + trailing unaligned bytes at EOF (e.g. 1, 2, 5 bytes)
    let mut hdr_plus_1 = hdr.to_vec();
    hdr_plus_1.push(0xAA);
    let (_, stats_p1) = ChatRecoveryScanner::new(&hdr_plus_1).recover().unwrap();
    assert_eq!(stats_p1.torn_writes, 1);

    let mut hdr_plus_sync = hdr.to_vec();
    hdr_plus_sync.extend_from_slice(&[0xAA, 0x55]);
    let (_, stats_sync) = ChatRecoveryScanner::new(&hdr_plus_sync).recover().unwrap();
    assert_eq!(stats_sync.torn_writes, 1);
}

#[test]
fn test_cjk_multilingual_unicode_complex_queries() {
    let mut index = InvertedIndexFts::new();
    let sid = [0x33; 16];

    // Document 1: Chinese
    index.insert(ChatRecord::new_text(
        1000, 10000, sid, "Li".into(),
        "自然语言处理与机器学习的结合非常精彩 #AI #NLP @li",
    ));

    // Document 2: Japanese Kanji + Kana
    index.insert(ChatRecord::new_text(
        1010, 20000, sid, "Ken".into(),
        "桜が満開の季節に、新幹線で東京へ行きました！ #travel #sakura @ken",
    ));

    // Document 3: Korean
    index.insert(ChatRecord::new_text(
        1020, 30000, sid, "Minho".into(),
        "넷플릭스 오리지널 드라마 자막이 정말 훌륭합니다. #drama @minho",
    ));

    // Document 4: Turkish with dotted and dotless I
    index.insert(ChatRecord::new_text(
        1030, 40000, sid, "Ahmet".into(),
        "İSTANBUL ve İZMİR şehirlerinde harika bir tatil yaptık. #tatil @ahmet",
    ));

    // Document 5: German with Eszett and umlauts
    index.insert(ChatRecord::new_text(
        1040, 50000, sid, "Hans".into(),
        "Schöne Grüße aus der Königsstraße in München! #deutschland @hans",
    ));

    // Document 6: Single CJK characters
    index.insert(ChatRecord::new_text(
        1050, 60000, sid, "Yuki".into(),
        "山 水 火 木 金 土 日",
    ));

    // Document 7: Tombstoned (deleted) record
    let mut tombstone = ChatRecord::new_text(
        1060, 25000, sid, "Ghost".into(),
        "Deleted message containing 桜 and 机器学习 #secret",
    );
    tombstone.flags |= 1; // Mark tombstone bit
    index.insert(tombstone);

    // 1. Single CJK Ideograph Exemption
    for ideograph in &["山", "水", "火", "木", "金", "土", "日"] {
        let res = index.search(ideograph);
        assert_eq!(res.len(), 1, "Failed for ideograph {}", ideograph);
        assert_eq!(res[0].sender_name, "Yuki");
    }

    // 2. CJK Bigrams
    let res_cjk_bigram = index.search("自然");
    assert_eq!(res_cjk_bigram.len(), 1);
    assert_eq!(res_cjk_bigram[0].sender_name, "Li");

    let res_cjk_bigram2 = index.search("新幹線");
    assert_eq!(res_cjk_bigram2.len(), 1);
    assert_eq!(res_cjk_bigram2[0].sender_name, "Ken");

    // 3. Korean tokens
    let res_kr = index.search("드라마");
    assert_eq!(res_kr.len(), 1);
    assert_eq!(res_kr[0].sender_name, "Minho");

    // 4. Turkish Case Folding (both uppercase and lowercase queries)
    let res_tr_lower = index.search("istanbul");
    assert_eq!(res_tr_lower.len(), 1);
    assert_eq!(res_tr_lower[0].sender_name, "Ahmet");

    let res_tr_upper = index.search("İSTANBUL");
    assert_eq!(res_tr_upper.len(), 1);
    assert_eq!(res_tr_upper[0].sender_name, "Ahmet");

    let res_tr_izmir = index.search("izmir");
    assert_eq!(res_tr_izmir.len(), 1);
    assert_eq!(res_tr_izmir[0].sender_name, "Ahmet");

    // 5. German tokens
    let res_de = index.search("münchen");
    assert_eq!(res_de.len(), 1);
    assert_eq!(res_de[0].sender_name, "Hans");

    // 6. Cross-language Boolean AND
    let res_and = index.search("桜 #travel");
    assert_eq!(res_and.len(), 1);
    assert_eq!(res_and[0].sender_name, "Ken");

    let res_and_mention = index.search("机器学习 @li");
    assert_eq!(res_and_mention.len(), 1);
    assert_eq!(res_and_mention[0].sender_name, "Li");

    // 7. Negative Boolean AND (no intersection)
    assert!(index.search("桜 넷플릭스").is_empty());
    assert!(index.search("İSTANBUL #deutschland").is_empty());

    // 8. Tombstone exclusion: tombstoned message containing "机器学习" or "桜" must NOT appear
    let res_machine = index.search("机器学习");
    assert_eq!(res_machine.len(), 1);
    assert_eq!(res_machine[0].sender_name, "Li"); // Ghost excluded!

    // 9. Edge-case queries: empty, whitespace, pure punctuation, pure stop words
    assert!(index.search("").is_empty());
    assert!(index.search("   \t\n  ").is_empty());
    assert!(index.search("!@#$%^&*()").is_empty());
    assert!(index.search("the is at which on and").is_empty());
}

#[test]
fn test_spatial_timeline_window_queries_btree() {
    let mut index = InvertedIndexFts::new();
    let sid = [0x11; 16];

    // Insert 10 records at various media timestamps
    index.insert(ChatRecord::new_text(100, 0, sid, "T0".into(), "Intro at 0s"));
    index.insert(ChatRecord::new_text(101, 5_000, sid, "T5".into(), "At 5s"));
    // Multiple records at exact same millisecond: 10_000
    index.insert(ChatRecord::new_text(102, 10_000, sid, "T10_A".into(), "At 10s A"));
    index.insert(ChatRecord::new_text(103, 10_000, sid, "T10_B".into(), "At 10s B"));
    index.insert(ChatRecord::new_text(104, 15_000, sid, "T15".into(), "At 15s"));
    index.insert(ChatRecord::new_text(105, 20_000, sid, "T20".into(), "At 20s"));
    index.insert(ChatRecord::new_text(106, 60_000, sid, "T60".into(), "At 60s"));
    index.insert(ChatRecord::new_text(107, 86_400_000, sid, "T24h".into(), "At 24h"));

    // Unanchored lobby record (media_pts = u64::MAX)
    index.insert(ChatRecord::new_text(
        108, UNANCHORED_MEDIA_PTS, sid, "Lobby".into(), "Lobby chat",
    ));

    // Tombstoned record at 10_000
    let mut tombstone = ChatRecord::new_text(
        109, 10_000, sid, "Tomb".into(), "Deleted at 10s",
    );
    tombstone.flags |= 1;
    index.insert(tombstone);

    // 1. Exact point query: window=0 at 10_000 -> returns T10_A and T10_B (excludes Tomb)
    let pt_10 = index.query_timeline_window(10_000, 0, 0);
    assert_eq!(pt_10.len(), 2);
    assert_eq!(pt_10[0].sender_name, "T10_A");
    assert_eq!(pt_10[1].sender_name, "T10_B");

    // 2. Symmetric window: around 10_000 with window 5_000 -> [5_000, 15_000]
    // Expect: T5, T10_A, T10_B, T15 (4 records)
    let win_10 = index.query_timeline_window(10_000, 5_000, 5_000);
    assert_eq!(win_10.len(), 4);
    assert_eq!(win_10[0].sender_name, "T5");
    assert_eq!(win_10[3].sender_name, "T15");

    // 3. Underflow saturation: current_pts = 1_000, past = 5_000 -> start clamps to 0
    let win_underflow = index.query_timeline_window(1_000, 5_000, 4_000); // [0, 5_000]
    assert_eq!(win_underflow.len(), 2);
    assert_eq!(win_underflow[0].sender_name, "T0");
    assert_eq!(win_underflow[1].sender_name, "T5");

    // 4. Far future / MAX saturation: current_pts = u64::MAX - 500, future = 1_000
    let win_overflow = index.query_timeline_window(u64::MAX - 500, 0, 1_000);
    // Unanchored records (u64::MAX) must NOT be returned!
    assert!(win_overflow.iter().all(|r| r.sender_name != "Lobby"));

    // 5. Query empty gap: [25_000, 55_000]
    let win_empty = index.query_timeline_window(40_000, 10_000, 10_000);
    assert!(win_empty.is_empty());
}

#[test]
fn test_slash_commands_and_dispatcher_adversarial() {
    let mut dispatcher = SlashCommandDispatcher::new();

    // 1. /seek timestamp formats and edge cases
    // Valid formats
    assert_eq!(
        dispatcher.dispatch("/seek 02:30:15", None).unwrap(),
        DispatchAction::Seek { target_pts_ms: 9015000 }
    );
    assert_eq!(
        dispatcher.dispatch("/seek 45.75s", None).unwrap(),
        DispatchAction::Seek { target_pts_ms: 45750 }
    );
    assert_eq!(
        dispatcher.dispatch("/seek 100", None).unwrap(),
        DispatchAction::Seek { target_pts_ms: 100000 }
    );

    // Relative seeks
    assert_eq!(
        dispatcher.dispatch("/seek +45s", Some(50000)).unwrap(),
        DispatchAction::Seek { target_pts_ms: 95000 }
    );
    assert_eq!(
        dispatcher.dispatch("/seek -20s", Some(50000)).unwrap(),
        DispatchAction::Seek { target_pts_ms: 30000 }
    );
    // Relative underflow clamps to 0
    assert_eq!(
        dispatcher.dispatch("/seek -100s", Some(50000)).unwrap(),
        DispatchAction::Seek { target_pts_ms: 0 }
    );

    // Malformed seeks
    assert!(matches!(
        dispatcher.dispatch("/seek", None),
        Err(ChatError::InvalidCommand(_))
    ));
    assert!(matches!(
        dispatcher.dispatch("/seek invalid_non_numeric", None),
        Err(ChatError::InvalidCommand(_))
    ));
    assert!(matches!(
        dispatcher.dispatch("/seek 12:34:56:78", None),
        Err(ChatError::InvalidCommand(_))
    ));
    assert!(matches!(
        dispatcher.dispatch("/seek 12:abc", None),
        Err(ChatError::InvalidCommand(_))
    ));

    // 2. /bookmark with various label formats
    let bm_empty = dispatcher.dispatch("/bookmark", Some(12345)).unwrap();
    match bm_empty {
        DispatchAction::Bookmark(b) => {
            assert_eq!(b.label, None);
            assert_eq!(b.pts_ms, 12345);
        }
        _ => panic!("Expected Bookmark"),
    }

    let bm_quoted = dispatcher.dispatch("/bookmark \"Climax Battle\"", Some(88888)).unwrap();
    match bm_quoted {
        DispatchAction::Bookmark(b) => {
            assert_eq!(b.label.as_deref(), Some("Climax Battle"));
            assert_eq!(b.pts_ms, 88888);
        }
        _ => panic!("Expected Bookmark"),
    }

    let bm_multiword = dispatcher.dispatch("/bookmark Epic Unquoted Sequence", Some(99999)).unwrap();
    match bm_multiword {
        DispatchAction::Bookmark(b) => {
            assert_eq!(b.label.as_deref(), Some("Epic Unquoted Sequence"));
        }
        _ => panic!("Expected Bookmark"),
    }

    // 3. /clip validation
    let clip_valid = dispatcher.dispatch("/clip 00:30 01:45 Highlight Reel", None).unwrap();
    match clip_valid {
        DispatchAction::Clip(c) => {
            assert_eq!(c.start_pts_ms, 30000);
            assert_eq!(c.end_pts_ms, 105000);
            assert_eq!(c.title.as_deref(), Some("Highlight Reel"));
        }
        _ => panic!("Expected Clip"),
    }

    // Zero-duration clip (start == end) is valid
    assert!(dispatcher.dispatch("/clip 10s 10s Instant Marker", None).is_ok());

    // Inverted clip (start > end) must error
    assert!(matches!(
        dispatcher.dispatch("/clip 02:00 01:00 Inverted Interval", None),
        Err(ChatError::InvalidCommand(_))
    ));

    // Missing arguments
    assert!(dispatcher.dispatch("/clip", None).is_err());
    assert!(dispatcher.dispatch("/clip 10s", None).is_err());

    // 4. /poll pipe and quote parsing & voting
    // Pipe notation
    let poll_action = dispatcher
        .dispatch("/poll Best Anime? | Steins;Gate | FMA:B | Attack on Titan", None)
        .unwrap();
    match poll_action {
        DispatchAction::Poll(p) => {
            assert_eq!(p.question, "Best Anime?");
            assert_eq!(p.options.len(), 3);
            assert_eq!(p.votes, vec![0, 0, 0]);
        }
        _ => panic!("Expected Poll"),
    }

    // Vote on options
    dispatcher.vote_poll(0).unwrap();
    dispatcher.vote_poll(0).unwrap();
    dispatcher.vote_poll(2).unwrap();
    let poll = dispatcher.active_poll().unwrap();
    assert_eq!(poll.votes, vec![2, 0, 1]);

    // Vote out of bounds
    assert!(dispatcher.vote_poll(3).is_err());
    assert!(dispatcher.vote_poll(99).is_err());

    // Quote notation
    let poll_quotes = dispatcher
        .dispatch(r#"/poll "Favorite Editor?" "Neovim" "Helix" "VSCode""#, None)
        .unwrap();
    match poll_quotes {
        DispatchAction::Poll(p) => {
            assert_eq!(p.question, "Favorite Editor?");
            assert_eq!(p.options, vec!["Neovim", "Helix", "VSCode"]);
        }
        _ => panic!("Expected Poll"),
    }

    // Malformed polls
    assert!(dispatcher.dispatch("/poll SingleOptionOnly", None).is_err());
    assert!(dispatcher.dispatch(r#"/poll "Only Question""#, None).is_err());

    // 5. /sync-status diagnostics reporting
    let diag = SyncDiagnostics {
        rtt_ms: 312.45,
        clock_offset_ms: -3.85,
        speed_multiplier: 1.0021,
        peer_count: 5,
        current_pts_ms: 125000,
        is_paused: false,
        pll_zone: "MicroSpeed".to_string(),
    };
    dispatcher.set_sync_diagnostics(diag.clone());

    let sync_action = dispatcher.dispatch("/sync-status", Some(125000)).unwrap();
    match sync_action {
        DispatchAction::SyncStatus { report, diagnostics } => {
            assert_eq!(diagnostics, diag);
            assert!(report.contains("312.45 ms"));
            assert!(report.contains("-3.85 ms"));
            assert!(report.contains("1.0021x"));
            assert!(report.contains("MicroSpeed"));
            assert!(report.contains("5"));
            assert!(report.contains("PLAYING"));
        }
        _ => panic!("Expected SyncStatus"),
    }

    // Standalone report formatter
    let report_output = format_sync_status_report(&diag);
    assert!(report_output.contains("=== Syncplay-Alt Sync Status ==="));
}

#[test]
fn test_disk_store_fuzz_crash_and_append() {
    let temp_dir = std::env::temp_dir();
    let log_path = temp_dir.join(format!("syncplay_adversarial_store_{}.bin", std::process::id()));
    let _ = std::fs::remove_file(&log_path);

    let sid = [0xBB; 16];

    // Step 1: Create store, append 5 valid records
    {
        let mut store = ChatStore::open(&log_path).expect("open failed");
        for i in 0..5 {
            let rec = ChatRecord::new_text(
                1000 + i * 10,
                i * 10000,
                sid,
                format!("User_{}", i),
                &format!("Message number {} #active", i),
            );
            store.append(&rec).unwrap();
        }
        assert_eq!(store.record_count(), 5);
    }

    // Step 2: Inject mid-file corruption and a trailing torn write directly to the file bytes
    {
        use std::fs::OpenOptions;
        use std::io::{Read, Write};

        let mut data = Vec::new();
        {
            let mut f = OpenOptions::new().read(true).open(&log_path).unwrap();
            f.read_to_end(&mut data).unwrap();
        }

        // Corrupt a byte in record 2 (offset 174..253)
        data[174 + 30] ^= 0xEE;

        // Append an incomplete torn record at EOF
        data.extend_from_slice(&[0xAA, 0x55, 0x40, 0x00, 0x00, 0x00, 0x01, 0x02]);

        let mut f = OpenOptions::new().write(true).truncate(true).open(&log_path).unwrap();
        f.write_all(&data).unwrap();
        f.flush().unwrap();
    }

    // Step 3: Re-open store: verifies automated crash recovery, index re-population
    {
        let mut store = ChatStore::open(&log_path).expect("re-open failed");
        let stats = store.recovery_stats().expect("should have recovery stats");

        // Record 2 was corrupted, torn record at EOF was truncated
        // Should recover 4 valid records (0, 1, 3, 4)
        assert_eq!(stats.valid_records, 4);
        assert_eq!(stats.torn_writes, 1);
        assert!(stats.corrupted_chunks >= 1);
        assert_eq!(store.record_count(), 4);

        // Append 2 more records to the recovered store
        let r5 = ChatRecord::new_text(2000, 50000, sid, "User_5".into(), "New message 5 #active");
        let r6 = ChatRecord::new_text(2010, 60000, sid, "User_6".into(), "New message 6 #active");
        store.append(&r5).unwrap();
        store.append(&r6).unwrap();
        assert_eq!(store.record_count(), 6);

        // Search across recovered + newly appended records
        let res = store.search("#active");
        assert_eq!(res.len(), 6);

        // Export markdown and json
        let md = store.export_transcript_markdown();
        assert!(md.contains("User_0"));
        assert!(md.contains("User_5"));

        let json = store.export_history_json();
        assert!(json.contains("User_0"));
        assert!(json.contains("User_6"));
    }

    // Clean up
    let _ = std::fs::remove_file(&log_path);
}
