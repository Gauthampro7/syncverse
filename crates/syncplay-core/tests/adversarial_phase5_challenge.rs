//! Empirical adversarial challenge verification suite for Phase 5.
//!
//! Stress-tests:
//! 1. Wire protocol deserialization (`PeerStatusPayload`): corrupted lengths, multi-megabyte strings, invalid UTF-8, truncated packets, CRC32 corruptions, fuzz mutations.
//! 2. File match diagnostics at boundaries: exactly 2000ms, 2001ms, 1999ms, Windows vs Linux paths.
//! 3. SemVer parsing & comparison against edge tags: pre-releases, build metadata, high numbers, garbage strings.
//! 4. Unpause gate state transitions & concurrency: multi-peer joining, leaving, unreadying, toggling force play concurrently.

#![deny(warnings)]

use std::thread;
use std::time::Duration;

use syncplay_chat::RoomAnnouncement;
use syncplay_common::Crc32;
use syncplay_sync::wire::{
    PacketType, WirePacket, FLAG_NONE, HEADER_LEN, WIRE_MAGIC,
};
use syncplay_sync::{ClientNode, HostServer, SyncError};
use syncplay_ui::gui::sidebar::{evaluate_file_match, extract_basename, FileMatchStatus};
use syncplay_ui::gui::updater::{
    clean_semver_tag, is_newer_version, normalize_semver_tag, parse_release_json,
    select_asset_for_os, UpdateError,
};
use syncplay_ui::{
    PlayerUiSnapshot, SNAPSHOT_FLAG_FORCE_PLAY, SNAPSHOT_FLAG_GATE_BLOCKED,
    SNAPSHOT_FLAG_HAS_MISMATCH, SNAPSHOT_FLAG_LOCAL_READY,
};

// ============================================================================
// 1. WIRE PROTOCOL ADVERSARIAL CHALLENGES (PeerStatusPayload & SPNT Framing)
// ============================================================================

#[test]
fn test_wire_deserialization_truncated_packets() {
    let valid_packet = WirePacket::peer_status(1, true, true, 5000, 1024, "Alice", "movie.mkv");
    let valid_bytes = valid_packet.to_bytes();
    assert!(valid_bytes.len() > HEADER_LEN);

    // Truncate at every single byte offset from 0 to full length - 1
    for len in 0..valid_bytes.len() {
        let truncated = &valid_bytes[..len];
        let res = WirePacket::from_bytes(truncated);
        assert!(
            res.is_err(),
            "Truncated packet of len {} should be rejected",
            len
        );
        match res.unwrap_err() {
            SyncError::PacketTooShort(_) => {
                assert!(len < HEADER_LEN);
            }
            SyncError::PayloadTruncated { expected, actual } => {
                assert!(len >= HEADER_LEN);
                assert!(actual < expected);
            }
            other => panic!("Unexpected error on truncation: {:?}", other),
        }
    }
}

#[test]
fn test_wire_deserialization_corrupted_magic_and_opcodes() {
    let packet = WirePacket::peer_status(42, true, false, 120_000, 500_000, "Bob", "clip.mp4");
    let bytes = packet.to_bytes();

    // 1. Mutate magic bytes
    for i in 0..4 {
        let mut corrupted = bytes.clone();
        corrupted[i] ^= 0xFF;
        let res = WirePacket::from_bytes(&corrupted);
        match res {
            Err(SyncError::InvalidMagic(m)) => {
                assert_ne!(m, WIRE_MAGIC);
            }
            other => panic!("Expected InvalidMagic error, got: {:?}", other),
        }
    }

    // 2. Corrupt packet opcode (byte 4) to invalid / unassigned opcodes
    let invalid_opcodes = [0x00, 0x07, 0x0A, 0x0B, 0x7F, 0xFF];
    for &op in &invalid_opcodes {
        let mut corrupted = bytes.clone();
        corrupted[4] = op;
        // Recompute CRC so it passes CRC check and hits opcode parsing
        let payload_len = u16::from_le_bytes([corrupted[6], corrupted[7]]) as usize;
        let crc = Crc32::digest(&corrupted[HEADER_LEN..HEADER_LEN + payload_len]);
        corrupted[8..12].copy_from_slice(&crc.to_le_bytes());

        let res = WirePacket::from_bytes(&corrupted);
        match res {
            Err(SyncError::UnknownPacketType(val)) => {
                assert_eq!(val, op);
            }
            other => panic!("Expected UnknownPacketType({}), got {:?}", op, other),
        }
    }
}

#[test]
fn test_wire_deserialization_crc32_corruption_exhaustive() {
    let packet = WirePacket::peer_status(
        99,
        true,
        true,
        9_000_000,
        10_000_000,
        "Charlie",
        "presentation.webm",
    );
    let original = packet.to_bytes();

    // Flip every single bit in the payload area
    for byte_idx in HEADER_LEN..original.len() {
        for bit_idx in 0..8 {
            let mut corrupted = original.clone();
            corrupted[byte_idx] ^= 1 << bit_idx;

            let res = WirePacket::from_bytes(&corrupted);
            assert!(
                res.is_err(),
                "Flipped bit at byte {} bit {} MUST fail verification",
                byte_idx,
                bit_idx
            );
            match res.unwrap_err() {
                SyncError::CrcMismatch { expected, actual } => {
                    assert_ne!(expected, actual);
                }
                other => panic!("Expected CrcMismatch, got {:?}", other),
            }
        }
    }
}

#[test]
fn test_wire_deserialization_corrupted_string_lengths() {
    let packet = WirePacket::peer_status(1, true, true, 1000, 2000, "Alice", "video.mp4");
    let bytes = packet.to_bytes();

    // In PeerStatus, offset 12 + 22 = 34 has username_len (u16 LE)
    // 1. Declare username_len = 65535 when payload is small
    let mut corrupted = bytes.clone();
    corrupted[12 + 22..12 + 24].copy_from_slice(&65535u16.to_le_bytes());
    // Update CRC
    let payload_len = u16::from_le_bytes([corrupted[6], corrupted[7]]) as usize;
    let crc = Crc32::digest(&corrupted[HEADER_LEN..HEADER_LEN + payload_len]);
    corrupted[8..12].copy_from_slice(&crc.to_le_bytes());

    let res = WirePacket::from_bytes(&corrupted);
    assert!(res.is_err());
    match res.unwrap_err() {
        SyncError::MalformedPayload(msg) => {
            assert!(msg.contains("username length") && msg.contains("exceeds buffer"));
        }
        other => panic!("Expected MalformedPayload on username overflow, got {:?}", other),
    }

    // 2. Declare filename_len = 65535
    let mut corrupted_fn = bytes.clone();
    let fn_offset = 12 + 24 + 5; // 5 is len of "Alice"
    corrupted_fn[fn_offset..fn_offset + 2].copy_from_slice(&65535u16.to_le_bytes());
    let crc_fn = Crc32::digest(&corrupted_fn[HEADER_LEN..HEADER_LEN + payload_len]);
    corrupted_fn[8..12].copy_from_slice(&crc_fn.to_le_bytes());

    let res_fn = WirePacket::from_bytes(&corrupted_fn);
    assert!(res_fn.is_err());
    match res_fn.unwrap_err() {
        SyncError::MalformedPayload(msg) => {
            assert!(msg.contains("declared payload length mismatch"));
        }
        other => panic!("Expected MalformedPayload on filename mismatch, got {:?}", other),
    }
}

#[test]
fn test_wire_deserialization_invalid_utf8() {
    let packet = WirePacket::peer_status(1, true, true, 1000, 2000, "Alice", "video.mp4");
    let bytes = packet.to_bytes();

    // 1. Corrupt username with invalid UTF-8 bytes [0xFF, 0xFE]
    let mut corrupted_user = bytes.clone();
    corrupted_user[12 + 24] = 0xFF;
    corrupted_user[12 + 25] = 0xFE;
    let payload_len = u16::from_le_bytes([corrupted_user[6], corrupted_user[7]]) as usize;
    let crc = Crc32::digest(&corrupted_user[HEADER_LEN..HEADER_LEN + payload_len]);
    corrupted_user[8..12].copy_from_slice(&crc.to_le_bytes());

    let res = WirePacket::from_bytes(&corrupted_user);
    assert!(res.is_err());
    match res.unwrap_err() {
        SyncError::MalformedPayload(msg) => {
            assert!(msg.contains("invalid UTF-8 username"));
        }
        other => panic!("Expected invalid UTF-8 username error, got {:?}", other),
    }

    // 2. Corrupt filename with incomplete 3-byte UTF-8 sequence
    let mut corrupted_file = bytes.clone();
    let fn_bytes_offset = 12 + 24 + 5 + 2;
    corrupted_file[fn_bytes_offset] = 0xE2; // First byte of 3-byte sequence without continuation
    corrupted_file[fn_bytes_offset + 1] = 0x28; // ASCII '(' not valid continuation (0x80..0xBF)
    let crc2 = Crc32::digest(&corrupted_file[HEADER_LEN..HEADER_LEN + payload_len]);
    corrupted_file[8..12].copy_from_slice(&crc2.to_le_bytes());

    let res2 = WirePacket::from_bytes(&corrupted_file);
    assert!(res2.is_err());
    match res2.unwrap_err() {
        SyncError::MalformedPayload(msg) => {
            assert!(msg.contains("invalid UTF-8 filename"));
        }
        other => panic!("Expected invalid UTF-8 filename error, got {:?}", other),
    }
}

#[test]
fn test_wire_deserialization_multi_megabyte_and_fuzz_buffers() {
    // 1. Multi-megabyte buffer filled with pseudo-random noise
    let mut huge_buffer = vec![0xAA; 2 * 1024 * 1024]; // 2 MB
    huge_buffer[0..4].copy_from_slice(&WIRE_MAGIC);
    huge_buffer[4] = PacketType::PeerStatus as u8;
    huge_buffer[5] = FLAG_NONE;
    // Set payload_len to max u16
    huge_buffer[6..8].copy_from_slice(&65535u16.to_le_bytes());
    let crc = Crc32::digest(&huge_buffer[HEADER_LEN..HEADER_LEN + 65535]);
    huge_buffer[8..12].copy_from_slice(&crc.to_le_bytes());

    // Deserialization must safely reject malformed payload inside the 65535 bytes without panic
    let res = WirePacket::from_bytes(&huge_buffer);
    assert!(res.is_err(), "Random noise payload must not successfully parse");

    // 2. Pseudo-random fuzzing (5,000 iterations)
    let mut seed = 0x12345678u64;
    for _ in 0..5_000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let len = (seed % 1024) as usize;
        let mut fuzz_bytes = vec![0u8; len];
        for b in &mut fuzz_bytes {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            *b = (seed >> 32) as u8;
        }

        // Must NEVER panic under any input
        let _ = WirePacket::from_bytes(&fuzz_bytes);
    }
}

// ============================================================================
// 2. FILE MATCH DIAGNOSTICS AT BOUNDARIES (±2000ms & Cross-Platform Paths)
// ============================================================================

#[test]
fn test_file_match_boundary_tolerance_exact_values() {
    // Case 1: Exactly 0ms delta -> Match
    assert_eq!(
        evaluate_file_match(Some("a.mkv"), Some(10_000), Some("b.mkv"), Some(10_000)),
        FileMatchStatus::Match
    );

    // Case 2: Exactly +1999ms delta -> Match
    assert_eq!(
        evaluate_file_match(Some("a.mkv"), Some(10_000), Some("b.mkv"), Some(11_999)),
        FileMatchStatus::Match
    );

    // Case 3: Exactly -1999ms delta -> Match
    assert_eq!(
        evaluate_file_match(Some("a.mkv"), Some(11_999), Some("b.mkv"), Some(10_000)),
        FileMatchStatus::Match
    );

    // Case 4: Exactly +2000ms delta (boundary inclusive) -> Match
    assert_eq!(
        evaluate_file_match(Some("a.mkv"), Some(10_000), Some("b.mkv"), Some(12_000)),
        FileMatchStatus::Match
    );

    // Case 5: Exactly -2000ms delta (boundary inclusive) -> Match
    assert_eq!(
        evaluate_file_match(Some("a.mkv"), Some(12_000), Some("b.mkv"), Some(10_000)),
        FileMatchStatus::Match
    );

    // Case 6: Exactly +2001ms delta (boundary excursion) -> Mismatch
    assert_eq!(
        evaluate_file_match(Some("a.mkv"), Some(10_000), Some("b.mkv"), Some(12_001)),
        FileMatchStatus::Mismatch
    );

    // Case 7: Exactly -2001ms delta (boundary excursion) -> Mismatch
    assert_eq!(
        evaluate_file_match(Some("a.mkv"), Some(12_001), Some("b.mkv"), Some(10_000)),
        FileMatchStatus::Mismatch
    );
}

#[test]
fn test_file_match_cross_platform_path_basenames() {
    // 1. Linux vs Windows separators
    assert_eq!(
        extract_basename(r"C:\Users\Gautham\Videos\Interstellar.2014.1080p.mkv"),
        "Interstellar.2014.1080p.mkv"
    );
    assert_eq!(
        extract_basename("/home/gautham/Movies/Interstellar.2014.1080p.mkv"),
        "Interstellar.2014.1080p.mkv"
    );

    // 2. Cross-platform match when duration differs by 10 minutes
    assert_eq!(
        evaluate_file_match(
            Some(r"C:\Movies\Anime\Episode01.mkv"),
            Some(1_440_000),
            Some("/var/media/Episode01.mkv"),
            Some(1_440_000 + 600_000), // +10m delta, but identical filename
        ),
        FileMatchStatus::Match
    );

    // 3. Mixed forward/backward slashes
    assert_eq!(
        extract_basename("C:/Games/Steam/steamapps/common/Video\\trailer.mp4"),
        "trailer.mp4"
    );

    // 4. Case-insensitivity for Windows-style files
    assert_eq!(
        evaluate_file_match(
            Some("MOVIE.MKV"),
            Some(100_000),
            Some("movie.mkv"),
            Some(500_000), // > 2000ms delta, but case-insensitive match
        ),
        FileMatchStatus::Match
    );

    // 5. Deep Windows UNC path
    assert_eq!(
        extract_basename(r"\\?\UNC\server\share\directory\movie_final.mkv"),
        "movie_final.mkv"
    );

    // 6. Whitespace and empty file names
    assert_eq!(
        evaluate_file_match(Some(""), Some(1000), Some("a.mkv"), Some(1000)),
        FileMatchStatus::Mismatch
    );
    assert_eq!(
        evaluate_file_match(Some("   "), Some(1000), Some("a.mkv"), Some(1000)),
        FileMatchStatus::Mismatch
    );
    assert_eq!(
        evaluate_file_match(Some("a.mkv"), Some(1000), None, None),
        FileMatchStatus::NoFileLoaded
    );
    assert_eq!(
        evaluate_file_match(Some("a.mkv"), Some(1000), Some(""), None),
        FileMatchStatus::NoFileLoaded
    );
    assert_eq!(
        evaluate_file_match(Some("a.mkv"), Some(1000), Some("   "), None),
        FileMatchStatus::NoFileLoaded
    );
    // 7. Duration integer boundaries and overflow resilience
    // When duration is u64::MAX (e.g. 18.4 quintillion ms) and peer duration is 0,
    // abs_diff must correctly evaluate difference without signed integer wrapping.
    let extreme_match_1 = evaluate_file_match(
        Some("movie_a.mkv"),
        Some(u64::MAX),
        Some("movie_b.mkv"),
        Some(0),
    );
    assert_eq!(extreme_match_1, FileMatchStatus::Mismatch);

    // 8. Overflow safety: loc = 0, peer = 1u64 << 63
    // Handled safely without subtraction overflow panic, returning Mismatch.
    let extreme_match_2 = evaluate_file_match(
        Some("movie_a.mkv"),
        Some(0),
        Some("movie_b.mkv"),
        Some(1u64 << 63),
    );
    assert_eq!(extreme_match_2, FileMatchStatus::Mismatch);
}

// ============================================================================
// 3. UPDATER SEMVER PARSING & COMPARISON AGAINST EDGE TAGS
// ============================================================================

#[test]
fn test_semver_parsing_and_normalization_edges() {
    // 1. Tag prefix stripping
    assert_eq!(clean_semver_tag("v1.2.3"), "1.2.3");
    assert_eq!(clean_semver_tag("V1.2.3"), "1.2.3");
    assert_eq!(clean_semver_tag("  v1.2.3  "), "1.2.3");
    assert_eq!(clean_semver_tag("1.2.3"), "1.2.3");

    // 2. Normalization to semver::Version
    let v = normalize_semver_tag("v0.5.10").expect("Valid tag");
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 5);
    assert_eq!(v.patch, 10);

    // 3. Pre-release versions
    assert!(is_newer_version("0.1.0", "v0.1.1-beta.1").expect("Valid comparison"));
    assert!(!is_newer_version("0.1.1", "v0.1.1-beta.1").expect("0.1.1 is newer than beta"));
    assert!(is_newer_version("0.1.1-alpha", "v0.1.1-beta").expect("beta > alpha"));
    assert!(is_newer_version("0.1.1-rc.1", "v0.1.1-rc.2").expect("rc.2 > rc.1"));

    // 4. Build metadata
    // In Rust's `semver::Version`, `Ord` is derived across all fields including `build`.
    // Therefore, `1.0.0+build.1` is ordered greater than `1.0.0` (empty build metadata),
    // and `1.0.0+build.2` is ordered greater than `1.0.0+build.1`.
    assert!(is_newer_version("1.0.0", "v1.0.0+build.1").expect("Build metadata comparison"));
    assert!(is_newer_version("1.0.0+build.1", "v1.0.0+build.2").expect("Build metadata comparison"));
    assert!(is_newer_version("1.0.0+build.1", "v1.0.1+build.2").expect("Patch increment"));

    // 5. Extremely large version integers
    assert!(is_newer_version("0.1.0", "v999999.888888.777777").expect("High numbers"));
    assert!(is_newer_version("1.0.0", "v18446744073709551614.0.0").expect("Near u64::MAX"));

    // 6. Garbage & malformed tags (must fail with clean error, never panic)
    let invalid_tags = [
        "",
        "   ",
        "v",
        "latest",
        "nightly",
        "v..",
        "vv1.0.0",
        "release-1.0.0",
        "1.0",       // Missing patch version
        "v1.0",      // Missing patch version
        "1.0.0.0",   // Too many components
        "v-1.0.0",   // Negative number
        "v01.0.0",   // Leading zero in major (invalid in SemVer 2.0.0)
        "1.0.0-01",  // Leading zero in numeric prerelease
    ];
    for &tag in &invalid_tags {
        let res = normalize_semver_tag(tag);
        assert!(res.is_err(), "Tag '{}' should fail SemVer parsing", tag);
    }
}

#[test]
fn test_updater_release_json_parsing_and_asset_selection() {
    let current_ver = "0.1.0";

    // 1. Valid newer release JSON
    let json_newer = serde_json::json!({
        "tag_name": "v0.2.0",
        "html_url": "https://github.com/Gauthampro7/syncplay-desktop-preview/releases/tag/v0.2.0",
        "body": "Major stability release with peer status",
        "assets": [
            {
                "name": "syncplay-alt-windows.exe",
                "browser_download_url": "https://github.com/.../syncplay-alt-windows.exe"
            },
            {
                "name": "syncplay-alt-linux.tar.gz",
                "browser_download_url": "https://github.com/.../syncplay-alt-linux.tar.gz"
            }
        ]
    });

    let info = parse_release_json(current_ver, &json_newer)
        .expect("Parse should succeed")
        .expect("Update should be available");
    assert_eq!(info.latest_version, "0.2.0");
    assert_eq!(info.current_version, "0.1.0");

    // 2. Same version or older (no update)
    let json_same = serde_json::json!({
        "tag_name": "v0.1.0",
        "html_url": "https://github.com/.../v0.1.0"
    });
    assert!(parse_release_json(current_ver, &json_same).unwrap().is_none());

    let json_older = serde_json::json!({
        "tag_name": "v0.0.9",
        "html_url": "https://github.com/.../v0.0.9"
    });
    assert!(parse_release_json(current_ver, &json_older).unwrap().is_none());

    // 3. Malformed JSON missing tag_name
    let json_no_tag = serde_json::json!({
        "html_url": "https://github.com/..."
    });
    let err = parse_release_json(current_ver, &json_no_tag).unwrap_err();
    match err {
        UpdateError::ParseError(msg) => assert!(msg.contains("Missing 'tag_name'")),
        other => panic!("Expected ParseError, got {:?}", other),
    }

    // 4. Asset selection across target OS
    let assets = vec![
        ("syncplay-v0.2.0-setup.exe".to_string(), "http://win".to_string()),
        ("syncplay-v0.2.0-x86_64.AppImage".to_string(), "http://linux".to_string()),
        ("syncplay-v0.2.0.dmg".to_string(), "http://mac".to_string()),
    ];

    assert_eq!(select_asset_for_os(&assets, "windows"), Some("http://win".to_string()));
    assert_eq!(select_asset_for_os(&assets, "linux"), Some("http://linux".to_string()));
    assert_eq!(select_asset_for_os(&assets, "macos"), Some("http://mac".to_string()));
    assert_eq!(select_asset_for_os(&assets, "freebsd"), Some("http://win".to_string())); // Fallback to first
}

// ============================================================================
// 4. UNPAUSE GATE STATE TRANSITIONS & MULTI-PEER CONCURRENCY
// ============================================================================

#[test]
fn test_unpause_gate_state_transitions_comprehensive() {
    // Model the exact unpause gate rules from playback_runtime.rs lines 410-486:
    // Rule: In watch party, if local or any remote peer has (!has_media || !is_ready),
    // and !force_play, playback is BLOCKED.

    struct PeerState {
        name: String,
        has_media: bool,
        is_ready: bool,
    }

    fn check_gate(
        in_party: bool,
        local_has_media: bool,
        local_ready: bool,
        remote_peers: &[PeerState],
        force_play: bool,
    ) -> (bool /* allowed */, Vec<String> /* unready_names */) {
        if !in_party {
            return (true, Vec::new());
        }

        let mut unready = Vec::new();
        if !local_has_media || !local_ready {
            unready.push("Local".to_string());
        }
        for p in remote_peers {
            if !p.has_media || !p.is_ready {
                unready.push(p.name.clone());
            }
        }

        let allowed = unready.is_empty() || force_play;
        (allowed, unready)
    }

    // Scenario 1: Solo playback (not in watch party) -> Always allowed
    let (allowed, unready) = check_gate(false, false, false, &[], false);
    assert!(allowed);
    assert!(unready.is_empty());

    // Scenario 2: Watch party, local unready, remote ready -> Blocked
    let remotes = vec![PeerState {
        name: "Alice".to_string(),
        has_media: true,
        is_ready: true,
    }];
    let (allowed, unready) = check_gate(true, true, false, &remotes, false);
    assert!(!allowed);
    assert_eq!(unready, vec!["Local"]);

    // Scenario 3: Watch party, local ready, Alice unready -> Blocked
    let remotes = vec![PeerState {
        name: "Alice".to_string(),
        has_media: true,
        is_ready: false,
    }];
    let (allowed, unready) = check_gate(true, true, true, &remotes, false);
    assert!(!allowed);
    assert_eq!(unready, vec!["Alice"]);

    // Scenario 4: Watch party, Bob has no media loaded -> Blocked
    let remotes = vec![
        PeerState {
            name: "Alice".to_string(),
            has_media: true,
            is_ready: true,
        },
        PeerState {
            name: "Bob".to_string(),
            has_media: false,
            is_ready: true,
        },
    ];
    let (allowed, unready) = check_gate(true, true, true, &remotes, false);
    assert!(!allowed);
    assert_eq!(unready, vec!["Bob"]);

    // Scenario 5: Force play override active -> Allowed despite unready Bob
    let (allowed, unready) = check_gate(true, true, true, &remotes, true);
    assert!(allowed);
    assert_eq!(unready, vec!["Bob"]); // unready reported, but allowed == true

    // Scenario 6: All peers ready with media -> Allowed
    let remotes = vec![
        PeerState {
            name: "Alice".to_string(),
            has_media: true,
            is_ready: true,
        },
        PeerState {
            name: "Bob".to_string(),
            has_media: true,
            is_ready: true,
        },
    ];
    let (allowed, unready) = check_gate(true, true, true, &remotes, false);
    assert!(allowed);
    assert!(unready.is_empty());
}

#[test]
fn test_unpause_gate_concurrent_network_simulation() {
    // Spin up real HostServer and multiple ClientNodes on localhost UDP ports
    let mut host = HostServer::bind("127.0.0.1:0").expect("Host bind");
    let host_addr = host.local_addr().expect("Host local addr");

    let mut client1 = ClientNode::bind("127.0.0.1:0", host_addr, 101).expect("Client 1 bind");
    let mut client2 = ClientNode::bind("127.0.0.1:0", host_addr, 102).expect("Client 2 bind");
    let mut client3 = ClientNode::bind("127.0.0.1:0", host_addr, 103).expect("Client 3 bind");

    // Client 1 sends status: has_media = true, is_ready = true
    client1
        .send_peer_status(true, true, 7_200_000, 10_000_000, "Alice", "matrix.mkv")
        .expect("Send status");

    // Client 2 sends status: has_media = true, is_ready = false (Not Ready)
    client2
        .send_peer_status(true, false, 7_200_000, 10_000_000, "Bob", "matrix.mkv")
        .expect("Send status");

    // Client 3 sends status: has_media = false, is_ready = false (No media)
    client3
        .send_peer_status(false, false, 0, 0, "Charlie", "")
        .expect("Send status");

    // Host processes all incoming datagrams
    let mut processed_count = 0;
    for _ in 0..20 {
        while let Ok(Some(_)) = host.poll_and_process() {
            processed_count += 1;
        }
        if processed_count >= 3 {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert!(processed_count >= 3, "Host must process all 3 client statuses");

    // Verify host client session state
    let clients = host.clients();
    assert_eq!(clients.len(), 3);

    let mut unready_peers = Vec::new();
    for client in clients.values() {
        if !client.has_media || !client.is_user_ready {
            unready_peers.push(client.username.clone());
        }
    }
    unready_peers.sort();
    assert_eq!(unready_peers, vec!["Bob", "Charlie"]);

    // Test Gate Announcement formatting
    let msg = RoomAnnouncement::gate_waiting(&unready_peers);
    assert_eq!(
        msg,
        "⏸ Paused: Waiting for Bob, Charlie to load media and get ready..."
    );

    // Now Client 2 becomes ready
    client2
        .send_peer_status(true, true, 7_200_000, 10_000_000, "Bob", "matrix.mkv")
        .expect("Send status");

    // Client 3 loads media and becomes ready
    client3
        .send_peer_status(true, true, 7_200_000, 10_000_000, "Charlie", "matrix.mkv")
        .expect("Send status");

    for _ in 0..20 {
        while let Ok(Some(_)) = host.poll_and_process() {}
        thread::sleep(Duration::from_millis(5));
    }

    let clients = host.clients();
    let unready_after: Vec<_> = clients
        .values()
        .filter(|c| !c.has_media || !c.is_user_ready)
        .collect();
    assert!(
        unready_after.is_empty(),
        "All peers are now ready with media; gate should permit unpause"
    );
}

#[test]
fn test_snapshot_atomic_invariants_and_bitflags() {
    let mut snap = PlayerUiSnapshot {
        monotonic_timestamp: 12345678,
        media_position_pts: 5_000_000_000,
        media_duration_pts: 7_200_000_000_000,
        playback_state: 0,
        network_rtt_nanos: 25_000_000,
        clock_offset_nanos: -150_000,
        phase_error_nanos: 1_200_000,
        pll_speed_ratio_q16: 65536,
        peer_count: 5,
        peers_ready: 4,
        room_sequence: 42,
    };

    assert_eq!(std::mem::size_of::<PlayerUiSnapshot>(), 72);
    assert_eq!(std::mem::align_of::<PlayerUiSnapshot>(), 8);

    // Test flag toggling
    snap.playback_state |= SNAPSHOT_FLAG_LOCAL_READY;
    assert!(snap.is_local_ready());

    snap.playback_state |= SNAPSHOT_FLAG_GATE_BLOCKED;
    assert!(snap.is_gate_blocked());

    snap.playback_state |= SNAPSHOT_FLAG_FORCE_PLAY;
    assert!(snap.is_force_play());

    snap.playback_state |= SNAPSHOT_FLAG_HAS_MISMATCH;
    assert!(snap.has_media_mismatch());

    // Clear flags
    snap.playback_state &= !SNAPSHOT_FLAG_GATE_BLOCKED;
    assert!(!snap.is_gate_blocked());
    assert!(snap.is_force_play());
    assert!(snap.is_local_ready());
    assert!(snap.has_media_mismatch());
}
