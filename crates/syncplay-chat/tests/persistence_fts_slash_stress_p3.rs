//! Empirical Adversarial Challenge Suite: Persistence, FTS Concurrency & Slash Command Stress.
//!
//! Evaluates:
//! 1. `syncplay_chat.bin` persistence & recovery under crash/corruption scenarios:
//!    - Header magic & CRC32 verification.
//!    - Mid-payload truncation and EOF torn writes.
//!    - Forward-scanning false-sync recovery around `0xAA55` markers (oversized, undersized, corrupted CRC).
//!    - Embedded `0xAA55` in valid message byte payload.
//! 2. Inverted Index FTS under rapid concurrent inserts:
//!    - Concurrent multi-threaded inserts (8 threads inserting hundreds of records).
//!    - CJK unigrams and overlapping bigrams.
//!    - Unicode emojis (multi-byte code points).
//!    - Punctuation stripping with `#` and `@` preservation.
//!    - Single CJK ideograph indexing exemption vs Western word length bounding.
//!    - Multi-term Boolean AND queries across CJK, hashtags, Western words with case-folding.
//! 3. Slash command parsing & fuzzing:
//!    - `/seek 01:23:45`, `/seek +15s`, `/seek -10s`, `/seek -9999s` underflow clamping.
//!    - `/bookmark`, `/clip` (with reversed interval error), `/poll` (quotes & pipes), `/sync-status`.
//!    - Fuzzing malformed/invalid inputs, empty slash commands, long buffers, null characters.

use std::sync::{Arc, Mutex};
use std::thread;

use syncplay_chat::{
    make_file_header, ChatError, ChatRecord, ChatRecoveryScanner, ChatStore,
    DispatchAction, InvertedIndexFts, SlashCommandDispatcher,
};

#[test]
fn test_persistence_header_and_short_file_scenarios() {
    // 1. Empty buffer
    let (records, stats) = ChatRecoveryScanner::new(&[]).recover().unwrap();
    assert!(records.is_empty());
    assert_eq!(stats.valid_records, 0);

    // 2. Buffer less than 16 bytes
    let (records, stats) = ChatRecoveryScanner::new(&[0x53, 0x50, 0x43, 0x48, 0x01, 0x00]).recover().unwrap();
    assert!(records.is_empty());
    assert_eq!(stats.valid_records, 0);

    // 3. Exactly 16-byte valid header
    let valid_hdr = make_file_header(0);
    let (records, stats) = ChatRecoveryScanner::new(&valid_hdr).recover().unwrap();
    assert!(records.is_empty());
    assert_eq!(stats.valid_records, 0);
    assert_eq!(stats.torn_writes, 0);

    // 4. Invalid magic
    let mut bad_magic = valid_hdr;
    bad_magic[0] = b'X';
    assert!(matches!(
        ChatRecoveryScanner::new(&bad_magic).recover(),
        Err(ChatError::InvalidHeaderMagic)
    ));

    // 5. Corrupt header CRC
    let mut bad_crc = valid_hdr;
    bad_crc[15] ^= 0xFF;
    assert!(matches!(
        ChatRecoveryScanner::new(&bad_crc).recover(),
        Err(ChatError::HeaderCrcMismatch)
    ));
}

#[test]
fn test_persistence_crashes_and_false_sync_forward_recovery() {
    let mut data = Vec::new();
    data.extend_from_slice(&make_file_header(0));

    let sender = [0x42; 16];

    // Create 5 valid baseline records
    let mut expected_valid = Vec::new();
    for i in 0..5 {
        let rec = ChatRecord::new_text(
            1_700_000_000_000 + i * 1_000,
            i * 5_000,
            sender,
            format!("User{}", i),
            &format!("Message number #{} with 🍿 emoji", i),
        );
        expected_valid.push(rec);
    }

    // Write Record 0 & 1 (clean)
    data.extend_from_slice(&expected_valid[0].serialize_record());
    data.extend_from_slice(&expected_valid[1].serialize_record());

    // Inject Crash Scenario 1: False sync word [0xAA, 0x55] with length 0 (< 48)
    data.extend_from_slice(&[0xAA, 0x55, 0x00, 0x00, 0x00, 0x00, 0xDE, 0xAD]);

    // Inject Crash Scenario 2: False sync word [0xAA, 0x55] with length 47 (< 48)
    data.extend_from_slice(&[0xAA, 0x55, 0x2F, 0x00, 0x00, 0x00]);

    // Inject Crash Scenario 3: False sync word with length > remaining file bytes (0x00010000 = 65536 bytes)
    data.extend_from_slice(&[0xAA, 0x55, 0x00, 0x00, 0x01, 0x00, 0x01, 0x02, 0x03, 0x04]);

    // Inject Crash Scenario 4: Multiple overlapping false sync bytes: [0xAA, 0xAA, 0x55, 0xAA, 0x55]
    data.extend_from_slice(&[0xAA, 0xAA, 0x55, 0xAA, 0x55]);

    // Write Record 2 (clean)
    data.extend_from_slice(&expected_valid[2].serialize_record());

    // Inject Crash Scenario 5: Valid-looking candidate record with bit-flipped body CRC
    let mut corrupted_rec = expected_valid[3].serialize_record();
    let crc_pos = corrupted_rec.len() - 2;
    corrupted_rec[crc_pos] ^= 0x55;
    data.extend_from_slice(&corrupted_rec);

    // Inject Crash Scenario 6: Embedded 0xAA, 0x55 inside payload of a valid record!
    let mut embedded_payload = b"Embedded binary markers: ".to_vec();
    embedded_payload.extend_from_slice(&[0xAA, 0x55]);
    embedded_payload.extend_from_slice(b" inside byte payload! #resilient");
    let embedded_sync_rec = ChatRecord {
        record_type: 1,
        flags: 0,
        wall_timestamp_utc_ms: 1_700_000_050_000,
        media_timestamp_ms: 25_000,
        sender_id: sender,
        sender_name: "Gautham".to_string(),
        payload: embedded_payload,
    };
    data.extend_from_slice(&embedded_sync_rec.serialize_record());

    // Write Record 4 (clean)
    data.extend_from_slice(&expected_valid[4].serialize_record());

    // Inject Crash Scenario 7: EOF torn write (authoritative incomplete trailing write)
    // Take a serialized record and truncate it mid-body
    let full_rec = expected_valid[3].serialize_record();
    data.extend_from_slice(&full_rec[..30]); // partial write of 30 bytes

    // Recover using scanner
    let (recovered, stats) = ChatRecoveryScanner::new(&data).recover().expect("recovery succeeds");

    // We expect: Record 0, Record 1, Record 2, Embedded Sync Rec, Record 4 = 5 valid records
    assert_eq!(recovered.len(), 5);
    assert_eq!(stats.valid_records, 5);
    assert!(stats.corrupted_chunks > 0, "Corrupted chunks must be detected");
    assert_eq!(stats.torn_writes, 1, "EOF torn write must be detected exactly once");

    // Verify payloads
    assert_eq!(recovered[0].payload, expected_valid[0].payload);
    assert_eq!(recovered[1].payload, expected_valid[1].payload);
    assert_eq!(recovered[2].payload, expected_valid[2].payload);
    assert_eq!(recovered[3].payload, embedded_sync_rec.payload);
    assert_eq!(recovered[4].payload, expected_valid[4].payload);
}

#[test]
fn test_disk_chat_store_crash_recovery_and_append() {
    let temp_path = "test_persistence_crash_store_tmp.bin";
    let _ = std::fs::remove_file(temp_path);

    let sender = [0x11; 16];

    // 1. Create store and append 3 records
    {
        let mut store = ChatStore::open(temp_path).expect("open new store");
        for i in 0..3 {
            let r = ChatRecord::new_text(
                1_700_000_100_000 + i * 1000,
                i * 10_000,
                sender,
                format!("Peer{}", i),
                &format!("Persisted message #{}", i),
            );
            store.append(&r).expect("append record");
        }
    }

    // 2. Corrupt file by appending garbage and partial torn write at EOF
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(temp_path)
            .expect("open for corruption");
        // False sync followed by partial write
        f.write_all(&[0xAA, 0x55, 0x10, 0x00, 0x00, 0x00, 0xFF, 0xFE, 0xFD])
            .expect("append corruption");
    }

    // 3. Re-open store with crash recovery
    {
        let mut store = ChatStore::open(temp_path).expect("re-open after crash");
        assert_eq!(store.records().len(), 3);

        // Append a 4th record after recovery
        let r4 = ChatRecord::new_text(
            1_700_000_105_000,
            30_000,
            sender,
            "Peer3".to_string(),
            "Post-recovery appended message",
        );
        store.append(&r4).expect("append after recovery");
        assert_eq!(store.records().len(), 4);

        // Search must find both pre-recovery and post-recovery records
        let hits_pre = store.search("Persisted");
        assert_eq!(hits_pre.len(), 3);
        let hits_post = store.search("Post-recovery");
        assert_eq!(hits_post.len(), 1);
    }

    // 4. Clean up
    let _ = std::fs::remove_file(temp_path);
}

#[test]
fn test_inverted_index_fts_cjk_emojis_and_boolean_and() {
    let mut index = InvertedIndexFts::new();
    let sender = [0x55; 16];

    // Record 1: CJK Chinese sentence with hashtags
    let r1 = ChatRecord::new_text(
        1_700_000_000_000,
        10_000,
        sender,
        "Gautham".to_string(),
        "这是一段精彩的吉他独奏 #ricing #fedora",
    );
    index.insert(r1);

    // Record 2: CJK Japanese with emojis
    let r2 = ChatRecord::new_text(
        1_700_000_001_000,
        15_000,
        sender,
        "Alice".to_string(),
        "映画鑑賞会を始めましょう！🍿🎬 独奏も最高！",
    );
    index.insert(r2);

    // Record 3: English with Turkish dotted I and mentions
    let r3 = ChatRecord::new_text(
        1_700_000_002_000,
        20_000,
        sender,
        "Bob".to_string(),
        "Connecting from İSTANBUL watch-party! @gautham check this out",
    );
    index.insert(r3);

    // Record 4: Single CJK ideographs vs single Western character
    let r4 = ChatRecord::new_text(
        1_700_000_003_000,
        25_000,
        sender,
        "Charlie".to_string(),
        "猫 狗 a b c #pets",
    );
    index.insert(r4);

    // 1. CJK Unigram searches
    assert_eq!(index.search("吉").len(), 1);
    assert_eq!(index.search("他").len(), 1);
    assert_eq!(index.search("奏").len(), 2); // In r1 ("独奏") and r2 ("独奏")

    // 2. CJK Bigram searches
    assert_eq!(index.search("吉他").len(), 1);
    assert_eq!(index.search("独奏").len(), 2);
    assert_eq!(index.search("映画").len(), 1);

    // 3. Single CJK ideograph exemption
    assert_eq!(index.search("猫").len(), 1);
    assert_eq!(index.search("狗").len(), 1);

    // Single Western characters 'a', 'b', 'c' should NOT be indexed (minimum 2 chars)
    assert_eq!(index.search("a").len(), 0);

    // 4. Hashtags & Mentions
    assert_eq!(index.search("#ricing").len(), 1);
    assert_eq!(index.search("#fedora").len(), 1);
    assert_eq!(index.search("@gautham").len(), 1);

    // 5. Turkish dotted I case folding
    assert_eq!(index.search("istanbul").len(), 1);
    assert_eq!(index.search("İSTANBUL").len(), 1);

    // 6. Multi-term Boolean AND queries
    // Both terms must match in the same record
    let and_match = index.search("独奏 #ricing");
    assert_eq!(and_match.len(), 1);
    assert_eq!(and_match[0].sender_name, "Gautham");

    // "独奏" matches r1 and r2, but "映画" only matches r2
    let and_match2 = index.search("独奏 映画");
    assert_eq!(and_match2.len(), 1);
    assert_eq!(and_match2[0].sender_name, "Alice");

    // One term absent from all -> yields empty
    assert_eq!(index.search("独奏 nonexistent_word").len(), 0);

    // 7. Unicode Emojis in query: does not panic
    let _ = index.search("🍿");
    let _ = index.search("🎬 🍿 🔥");
}

#[test]
fn test_rapid_concurrent_inserts_and_fts_queries() {
    let store = Arc::new(Mutex::new(ChatStore::new_in_memory()));
    let num_threads = 8;
    let records_per_thread = 50;

    let mut handles = Vec::new();

    for t in 0..num_threads {
        let store_clone = Arc::clone(&store);
        let handle = thread::spawn(move || {
            let sender = [(t as u8); 16];
            for i in 0..records_per_thread {
                let rec = ChatRecord::new_text(
                    1_700_000_000_000 + (t as u64) * 10000 + (i as u64),
                    (i as u64) * 1000,
                    sender,
                    format!("Worker_{}", t),
                    &format!(
                        "Concurrent message [t{}-i{}] with CJK 派对 and #tag_{}",
                        t, i, t % 3
                    ),
                );
                store_clone.lock().unwrap().append(&rec).unwrap();
            }
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().unwrap();
    }

    let final_store = store.lock().unwrap();
    assert_eq!(final_store.records().len(), num_threads * records_per_thread);

    // Search for CJK bigram across all concurrent records
    let hits_cjk = final_store.search("派对");
    assert_eq!(hits_cjk.len(), num_threads * records_per_thread);

    // Search for hashtag partition
    let hits_tag0 = final_store.search("#tag_0");
    assert!(!hits_tag0.is_empty());

    // Multi-term Boolean AND search
    let hits_multi = final_store.search("派对 #tag_0");
    assert_eq!(hits_multi.len(), hits_tag0.len());
}

#[test]
fn test_slash_command_dispatcher_and_fuzzing() {
    let mut dispatcher = SlashCommandDispatcher::new();

    // 1. /seek formats
    assert_eq!(
        dispatcher.dispatch("/seek 01:23:45", None).unwrap(),
        DispatchAction::Seek { target_pts_ms: 5025000 }
    );
    assert_eq!(
        dispatcher.dispatch("/seek 14:30", None).unwrap(),
        DispatchAction::Seek { target_pts_ms: 870000 }
    );
    assert_eq!(
        dispatcher.dispatch("/seek 123.4s", None).unwrap(),
        DispatchAction::Seek { target_pts_ms: 123400 }
    );
    assert_eq!(
        dispatcher.dispatch("/seek +15s", Some(10000)).unwrap(),
        DispatchAction::Seek { target_pts_ms: 25000 }
    );
    assert_eq!(
        dispatcher.dispatch("/seek -10s", Some(10000)).unwrap(),
        DispatchAction::Seek { target_pts_ms: 0 }
    );
    assert_eq!(
        dispatcher.dispatch("/seek -9999s", Some(10000)).unwrap(),
        DispatchAction::Seek { target_pts_ms: 0 } // Clamped underflow
    );

    // 2. /bookmark formats
    let act_bm = dispatcher.dispatch("/bookmark", Some(12345)).unwrap();
    match act_bm {
        DispatchAction::Bookmark(b) => {
            assert_eq!(b.label, None);
            assert_eq!(b.pts_ms, 12345);
        }
        _ => panic!("Expected bookmark"),
    }

    let act_bm_lbl = dispatcher.dispatch("/bookmark Climax Battle", Some(67890)).unwrap();
    match act_bm_lbl {
        DispatchAction::Bookmark(b) => {
            assert_eq!(b.label.as_deref(), Some("Climax Battle"));
            assert_eq!(b.pts_ms, 67890);
        }
        _ => panic!("Expected bookmark with label"),
    }

    // 3. /clip formats & validation
    let act_clip = dispatcher.dispatch("/clip 00:10 01:20 Favorite Scene", None).unwrap();
    match act_clip {
        DispatchAction::Clip(c) => {
            assert_eq!(c.start_pts_ms, 10000);
            assert_eq!(c.end_pts_ms, 80000);
            assert_eq!(c.title.as_deref(), Some("Favorite Scene"));
        }
        _ => panic!("Expected clip"),
    }

    // Reversed interval must error
    assert!(dispatcher.dispatch("/clip 02:00 01:00 Reversed", None).is_err());

    // 4. /poll formats
    let act_poll_quote = dispatcher
        .dispatch("/poll \"Best Editor\" \"Vim\" \"Helix\" \"VSCode\"", None)
        .unwrap();
    match act_poll_quote {
        DispatchAction::Poll(p) => {
            assert_eq!(p.question, "Best Editor");
            assert_eq!(p.options, vec!["Vim", "Helix", "VSCode"]);
        }
        _ => panic!("Expected poll quote"),
    }

    let act_poll_pipe = dispatcher
        .dispatch("/poll Best Shell? | fish | zsh | bash", None)
        .unwrap();
    match act_poll_pipe {
        DispatchAction::Poll(p) => {
            assert_eq!(p.question, "Best Shell?");
            assert_eq!(p.options, vec!["fish", "zsh", "bash"]);
        }
        _ => panic!("Expected poll pipe"),
    }

    // /poll without options must error
    assert!(dispatcher.dispatch("/poll \"Only Question\"", None).is_err());
    assert!(dispatcher.dispatch("/poll Only Question |", None).is_err());

    // 5. /sync-status
    let act_status = dispatcher.dispatch("/sync-status", None).unwrap();
    match act_status {
        DispatchAction::SyncStatus { report, diagnostics } => {
            assert!(report.contains("Syncplay-Alt Sync Status"));
            assert_eq!(diagnostics.current_pts_ms, 0);
        }
        _ => panic!("Expected SyncStatus action"),
    }

    // 6. Fuzzing invalid/malformed commands
    let fuzz_inputs = [
        "",
        "not a command",
        "/",
        "//",
        "///",
        "/   ",
        "/unknown_cmd",
        "/seek",
        "/seek abc",
        "/seek 12:34:56:78",
        "/seek ++10s",
        "/clip",
        "/clip 10",
        "/clip invalid 20",
        "/poll",
        "/poll \"\"",
        "/poll | | |",
        "   /seek   00:45   ",
    ];

    for input in fuzz_inputs {
        if input.trim() == "   /seek   00:45   ".trim() {
            assert!(dispatcher.dispatch(input, None).is_ok());
        } else {
            let res = dispatcher.dispatch(input, None);
            // Must return Err cleanly without panic
            assert!(res.is_err(), "Input '{}' should have errored, got {:?}", input, res);
        }
    }

    // Fuzz with large 10KB string
    let huge_str = format!("/seek {}", "9".repeat(10_000));
    assert!(dispatcher.dispatch(&huge_str, None).is_err());

    // Fuzz with null bytes
    assert!(dispatcher.dispatch("/seek \0\0\0", None).is_err());
}
