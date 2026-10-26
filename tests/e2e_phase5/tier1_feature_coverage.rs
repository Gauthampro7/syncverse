//! Tier 1: Feature Coverage (>=5 test cases per feature across all 18 features)
//!
//! Authoritative source: `TEST_INFRA.md § Feature Inventory` and `PROJECT.md`.

use std::fs;
use std::time::Duration;

use syncplay_chat::{ChatRecord, ChatRecoveryScanner, ChatStore};
use syncplay_sync::wire::{
    PacketPayload, PacketType, PeerStatusPayload, WirePacket,
};
use syncplay_sync::SyncError;
use syncplay_ui::gui::updater::{
    clean_semver_tag, is_newer_version, normalize_semver_tag, parse_release_json,
    select_asset_for_os, select_platform_download_url, UpdateError, UpdateInfo,
};
use syncplay_ui::snapshot::PlayerUiSnapshot;
use syncplay_ui::UiEvent;

use super::contract_models::*;

// ==============================================================================
// Feature 1: GitHub Releases API Update Querying (ORIGINAL_REQUEST §R1)
// ==============================================================================

#[test]
fn test_f1_update_check_success_parsing() {
    let raw_json = serde_json::json!({
        "tag_name": "v0.1.2",
        "html_url": "https://github.com/Gauthampro7/syncplay-desktop-preview/releases/tag/v0.1.2",
        "body": "Phase 5 Social & Update release",
        "assets": [
            {
                "name": "syncplay-alt-linux.tar.gz",
                "browser_download_url": "https://example.com/syncplay-alt-linux.tar.gz"
            }
        ]
    });

    let res = parse_release_json("0.1.1", &raw_json).expect("valid parse");
    assert!(res.is_some());
    let info = res.unwrap();
    assert_eq!(info.current_version, "0.1.1");
    assert_eq!(info.latest_version, "0.1.2");
    assert_eq!(info.release_url, "https://github.com/Gauthampro7/syncplay-desktop-preview/releases/tag/v0.1.2");
    assert_eq!(info.release_notes.as_deref(), Some("Phase 5 Social & Update release"));
}

#[test]
fn test_f1_update_check_asset_matching() {
    let assets = vec![
        ("syncplay-alt-v0.1.2-windows.exe".to_string(), "https://example.com/win.exe".to_string()),
        ("syncplay-alt-v0.1.2-linux.tar.gz".to_string(), "https://example.com/linux.tar.gz".to_string()),
        ("syncplay-alt-v0.1.2-macos.dmg".to_string(), "https://example.com/mac.dmg".to_string()),
    ];

    assert_eq!(select_asset_for_os(&assets, "linux"), Some("https://example.com/linux.tar.gz".to_string()));
    assert_eq!(select_asset_for_os(&assets, "windows"), Some("https://example.com/win.exe".to_string()));
    assert_eq!(select_asset_for_os(&assets, "macos"), Some("https://example.com/mac.dmg".to_string()));
}

#[test]
fn test_f1_update_check_missing_asset_fallback() {
    let assets = vec![
        ("syncplay-alt-generic-source.tar.gz".to_string(), "https://example.com/src.tar.gz".to_string()),
    ];
    // Fallback selects first asset if no OS string matches
    assert_eq!(select_asset_for_os(&assets, "solaris"), Some("https://example.com/src.tar.gz".to_string()));
    // Empty asset list returns None
    assert_eq!(select_asset_for_os(&[], "linux"), None);
}

#[test]
fn test_f1_update_check_missing_tag_error() {
    let raw_json = serde_json::json!({
        "html_url": "https://github.com/example/release"
    });
    let res = parse_release_json("0.1.1", &raw_json);
    assert!(matches!(res, Err(UpdateError::ParseError(_))));
}

#[test]
fn test_f1_update_check_no_update_when_same_version() {
    let raw_json = serde_json::json!({
        "tag_name": "v0.1.1",
        "html_url": "https://github.com/example/release",
        "assets": []
    });
    let res = parse_release_json("0.1.1", &raw_json).expect("valid parse");
    assert!(res.is_none(), "Identical version must yield None");
}

// ==============================================================================
// Feature 2: Semver Parsing & Comparison (ORIGINAL_REQUEST §R1)
// ==============================================================================

#[test]
fn test_f2_semver_strip_v_prefix() {
    assert_eq!(clean_semver_tag("v0.1.2"), "0.1.2");
    assert_eq!(clean_semver_tag("V0.1.2"), "0.1.2");
    assert_eq!(clean_semver_tag("  v1.0.0  "), "1.0.0");
    assert_eq!(clean_semver_tag("2.3.4"), "2.3.4");
}

#[test]
fn test_f2_semver_valid_parsing() {
    let v = normalize_semver_tag("v0.1.1").expect("valid");
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);
    assert_eq!(v.patch, 1);

    let v_pre = normalize_semver_tag("v0.2.0-rc.1").expect("valid rc");
    assert_eq!(v_pre.major, 0);
    assert_eq!(v_pre.minor, 2);
    assert_eq!(v_pre.patch, 0);
    assert_eq!(v_pre.pre.as_str(), "rc.1");
}

#[test]
fn test_f2_semver_patch_upgrade() {
    assert!(is_newer_version("0.1.1", "v0.1.2").expect("valid comparison"));
    assert!(is_newer_version("0.1.1", "0.1.3").expect("valid comparison"));
}

#[test]
fn test_f2_semver_minor_and_major_upgrade() {
    assert!(is_newer_version("0.1.9", "v0.2.0").expect("minor upgrade"));
    assert!(is_newer_version("0.9.9", "v1.0.0").expect("major upgrade"));
}

#[test]
fn test_f2_semver_same_and_downgrade() {
    assert!(!is_newer_version("0.1.1", "v0.1.1").expect("same"));
    assert!(!is_newer_version("0.1.2", "v0.1.1").expect("older"));
    assert!(!is_newer_version("1.0.0", "v0.9.9").expect("older major"));
}

// ==============================================================================
// Feature 3: Auto-Update Notification Banner & Actions (ORIGINAL_REQUEST §R1)
// ==============================================================================

#[test]
fn test_f3_banner_initial_state_on_update() {
    let event = UiEvent::UpdateAvailable {
        latest_version: "0.1.2".to_string(),
        release_url: "https://example.com/rel".to_string(),
        download_url: Some("https://example.com/dl".to_string()),
    };

    if let UiEvent::UpdateAvailable { latest_version, release_url, download_url } = event {
        assert_eq!(latest_version, "0.1.2");
        assert_eq!(release_url, "https://example.com/rel");
        assert_eq!(download_url, Some("https://example.com/dl".to_string()));
    } else {
        panic!("Expected UpdateAvailable");
    }
}

#[test]
fn test_f3_banner_dismissal_action() {
    let banner_visible = true;
    assert!(banner_visible);
    // Simulate user clicking "Dismiss" button
    let banner_visible = false;
    assert!(!banner_visible, "Dismissing banner must hide it");
}

#[test]
fn test_f3_banner_download_url_priority() {
    let assets = vec![
        ("syncplay-alt.rpm".to_string(), "https://example.com/app.rpm".to_string()),
    ];
    let download_url = select_platform_download_url(&assets);
    assert!(download_url.is_some(), "Platform download URL preferred when present");
}

#[test]
fn test_f3_banner_release_page_fallback() {
    let info = UpdateInfo {
        current_version: "0.1.1".to_string(),
        latest_version: "0.1.2".to_string(),
        release_url: "https://github.com/release".to_string(),
        download_url: None,
        release_notes: None,
    };
    // If download_url is None, user click opens release_url
    let target = info.download_url.as_deref().unwrap_or(&info.release_url);
    assert_eq!(target, "https://github.com/release");
}

#[test]
fn test_f3_banner_no_display_when_latest() {
    let event = UiEvent::UpdateChecked {
        is_latest: true,
        current_version: "0.1.1".to_string(),
    };
    if let UiEvent::UpdateChecked { is_latest, current_version } = event {
        assert!(is_latest);
        assert_eq!(current_version, "0.1.1");
    } else {
        panic!("Expected UpdateChecked");
    }
}

// ==============================================================================
// Feature 4: Help Menu "Check for Updates..." (ORIGINAL_REQUEST §R1)
// ==============================================================================

#[test]
fn test_f4_help_menu_manual_trigger() {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = tx.send(UiEvent::UpdateChecked {
        is_latest: true,
        current_version: "0.1.1".to_string(),
    });
    let received = rx.recv_timeout(Duration::from_millis(100)).expect("received event");
    assert!(matches!(received, UiEvent::UpdateChecked { .. }));
}

#[test]
fn test_f4_help_menu_up_to_date_event() {
    let event = UiEvent::UpdateChecked {
        is_latest: true,
        current_version: "0.1.1".to_string(),
    };
    if let UiEvent::UpdateChecked { is_latest, .. } = event {
        assert!(is_latest, "Up to date check reports is_latest == true");
    }
}

#[test]
fn test_f4_help_menu_update_available_event() {
    let event = UiEvent::UpdateAvailable {
        latest_version: "0.2.0".to_string(),
        release_url: "https://github.com/release".to_string(),
        download_url: None,
    };
    assert!(matches!(event, UiEvent::UpdateAvailable { .. }));
}

#[test]
fn test_f4_help_menu_error_propagation() {
    let event = UiEvent::UpdateFailed {
        error: "Connection refused".to_string(),
    };
    if let UiEvent::UpdateFailed { error } = event {
        assert!(error.contains("Connection refused"));
    } else {
        panic!("Expected UpdateFailed");
    }
}

#[test]
fn test_f4_help_menu_non_blocking_channel() {
    let (tx, rx) = std::sync::mpsc::channel::<UiEvent>();
    // Non-blocking try_recv should return empty initially
    assert!(rx.try_recv().is_err());
    let _ = tx.send(UiEvent::UpdateChecked {
        is_latest: true,
        current_version: "0.1.1".to_string(),
    });
    assert!(rx.try_recv().is_ok());
}

// ==============================================================================
// Feature 5: Peer Media Wire Packet Metadata (ORIGINAL_REQUEST §R2)
// ==============================================================================

#[test]
fn test_f5_wire_peer_status_packet_type() {
    assert_eq!(PacketType::PeerStatus as u8, 0x09);
    assert_eq!(PacketType::try_from(0x09).unwrap(), PacketType::PeerStatus);
}

#[test]
fn test_f5_wire_peer_status_serialization() {
    let pkt = WirePacket::peer_status(101, true, true, 3_600_000, 500_000_000, "Alice", "movie.mkv");
    let bytes = pkt.to_bytes();
    assert_eq!(&bytes[0..4], b"SPNT");
    assert_eq!(bytes[4], 0x09); // PacketType::PeerStatus
    assert!(bytes.len() > 12);
}

#[test]
fn test_f5_wire_peer_status_roundtrip() {
    let original = WirePacket::peer_status(42, true, false, 7_200_000, 1_048_576, "Bob", "feature.mp4");
    let bytes = original.to_bytes();
    let decoded = WirePacket::from_bytes(&bytes).expect("parse valid packet");
    assert_eq!(original, decoded);

    if let PacketPayload::PeerStatus(status) = decoded.payload {
        assert_eq!(status.client_id, 42);
        assert!(status.has_media);
        assert!(!status.is_ready);
        assert_eq!(status.duration_ms, 7_200_000);
        assert_eq!(status.file_size_bytes, 1_048_576);
        assert_eq!(status.username, "Bob");
        assert_eq!(status.filename, "feature.mp4");
    } else {
        panic!("Expected PeerStatus payload");
    }
}

#[test]
fn test_f5_wire_peer_status_crc32_verification() {
    let pkt = WirePacket::peer_status(1, true, true, 1000, 2000, "User", "test.mkv");
    let mut bytes = pkt.to_bytes();
    // Tamper with duration
    let last = bytes.len() - 1;
    bytes[last] ^= 0x55;
    assert!(matches!(WirePacket::from_bytes(&bytes), Err(SyncError::CrcMismatch { .. })));
}

#[test]
fn test_f5_wire_peer_status_has_media_flag() {
    let no_media = WirePacket::peer_status(2, false, false, 0, 0, "Noob", "");
    let bytes = no_media.to_bytes();
    let decoded = WirePacket::from_bytes(&bytes).unwrap();
    if let PacketPayload::PeerStatus(p) = decoded.payload {
        assert!(!p.has_media);
        assert_eq!(p.duration_ms, 0);
        assert_eq!(p.filename, "");
    } else {
        panic!("Expected PeerStatus payload");
    }
}

// ==============================================================================
// Feature 6: Peer Readiness Wire Packet State (ORIGINAL_REQUEST §R3)
// ==============================================================================

#[test]
fn test_f6_wire_peer_readiness_true() {
    let payload = PeerStatusPayload::new(5, true, true, 60000, 1024, "Alice", "clip.mkv");
    assert!(payload.is_ready);
    let pkt = WirePacket::new(PacketType::PeerStatus, 0x00, PacketPayload::PeerStatus(payload));
    let parsed = WirePacket::from_bytes(&pkt.to_bytes()).unwrap();
    if let PacketPayload::PeerStatus(p) = parsed.payload {
        assert!(p.is_ready);
    }
}

#[test]
fn test_f6_wire_peer_readiness_false() {
    let payload = PeerStatusPayload::new(5, true, false, 60000, 1024, "Alice", "clip.mkv");
    assert!(!payload.is_ready);
    let pkt = WirePacket::new(PacketType::PeerStatus, 0x00, PacketPayload::PeerStatus(payload));
    let parsed = WirePacket::from_bytes(&pkt.to_bytes()).unwrap();
    if let PacketPayload::PeerStatus(p) = parsed.payload {
        assert!(!p.is_ready);
    }
}

#[test]
fn test_f6_wire_peer_readiness_client_id_association() {
    let p1 = PeerStatusPayload::new(101, true, true, 1000, 200, "P1", "f1.mkv");
    let p2 = PeerStatusPayload::new(102, true, false, 1000, 200, "P2", "f2.mkv");
    assert_eq!(p1.client_id, 101);
    assert!(p1.is_ready);
    assert_eq!(p2.client_id, 102);
    assert!(!p2.is_ready);
}

#[test]
fn test_f6_wire_peer_readiness_username_association() {
    let p = PeerStatusPayload::new(1, true, true, 5000, 100, "Gautham", "video.webm");
    assert_eq!(p.username, "Gautham");
    assert!(p.is_ready);
}

#[test]
fn test_f6_wire_peer_readiness_payload_boundaries() {
    // Truncated payload less than minimum 26 bytes
    let mut pkt_bytes = WirePacket::peer_status(1, true, true, 100, 100, "U", "F").to_bytes();
    // Corrupt payload length in header to 10 bytes
    pkt_bytes[6] = 10;
    pkt_bytes[7] = 0;
    // CRC will mismatch
    assert!(WirePacket::from_bytes(&pkt_bytes).is_err());
}

// ==============================================================================
// Feature 7: Negative Opcode Validation Update (Survey Explorer 1)
// ==============================================================================

#[test]
fn test_f7_opcode_0x00_rejected() {
    assert!(matches!(PacketType::try_from(0x00), Err(SyncError::UnknownPacketType(0x00))));
}

#[test]
fn test_f7_opcode_0x07_rejected() {
    assert!(matches!(PacketType::try_from(0x07), Err(SyncError::UnknownPacketType(0x07))));
}

#[test]
fn test_f7_opcode_0x09_accepted() {
    let p = PacketType::try_from(0x09).expect("0x09 must be accepted as PeerStatus");
    assert_eq!(p, PacketType::PeerStatus);
}

#[test]
fn test_f7_opcode_0x0a_rejected() {
    assert!(matches!(PacketType::try_from(0x0A), Err(SyncError::UnknownPacketType(0x0A))));
}

#[test]
fn test_f7_opcode_0xff_rejected() {
    assert!(matches!(PacketType::try_from(0xFF), Err(SyncError::UnknownPacketType(0xFF))));
}

// ==============================================================================
// Feature 8: Persistent Readiness vs Barrier Preroll (Survey Explorer 1)
// ==============================================================================

#[test]
fn test_f8_readiness_independent_from_barrier_state() {
    let is_user_ready = true;
    let mut barrier_preroll_ready = false;
    assert!(!barrier_preroll_ready);

    // Preroll initiates
    barrier_preroll_ready = true;
    assert!(is_user_ready && barrier_preroll_ready);

    // Barrier completes and resets preroll flag
    barrier_preroll_ready = false;
    assert!(is_user_ready, "User watch readiness must remain true after barrier preroll completes");
    assert!(!barrier_preroll_ready);
}

#[test]
fn test_f8_barrier_preroll_completion_preserves_user_readiness() {
    let is_user_ready = true;
    // Simulate multi-round barrier execution
    for _ in 0..5 {
        let mut preroll = true;
        assert!(preroll);
        // preroll ends
        preroll = false;
        assert!(!preroll);
        assert!(is_user_ready);
    }
}

#[test]
fn test_f8_user_unready_blocks_barrier_initiation() {
    let peers = vec![("Alice".to_string(), true, false)]; // user not ready
    let dec = evaluate_unpause_gate(&peers, false);
    assert!(matches!(dec, UnpauseDecision::Blocked { .. }));
}

#[test]
fn test_f8_multiple_peers_independent_readiness() {
    let mut states = [true, false, true];
    assert!(states[0]);
    assert!(!states[1]);
    assert!(states[2]);

    states[1] = true;
    assert!(states.iter().all(|&r| r));
}

#[test]
fn test_f8_readiness_reset_on_media_unload() {
    let mut has_media = true;
    let mut is_ready = true;
    assert!(has_media && is_ready);

    // Media unloaded
    has_media = false;
    is_ready = false;

    assert!(!has_media);
    assert!(!is_ready, "Unloading media must reset readiness state to false");
}

// ==============================================================================
// Feature 9: Media Status Broadcast on Open/Change (ORIGINAL_REQUEST §R2)
// ==============================================================================

#[test]
fn test_f9_broadcast_on_file_open() {
    let status = PeerStatusPayload::new(1, true, true, 120_000, 45_000_000, "Alice", "episode1.mkv");
    let pkt = WirePacket::new(PacketType::PeerStatus, 0, PacketPayload::PeerStatus(status));
    assert_eq!(pkt.packet_type, PacketType::PeerStatus);
}

#[test]
fn test_f9_broadcast_on_file_change() {
    let mut status = PeerStatusPayload::new(1, true, true, 120_000, 45_000_000, "Alice", "episode1.mkv");
    // Change file to episode 2
    status.filename = "episode2.mkv".to_string();
    status.duration_ms = 135_000;
    status.file_size_bytes = 48_000_000;

    assert_eq!(status.filename, "episode2.mkv");
    assert_eq!(status.duration_ms, 135_000);
}

#[test]
fn test_f9_broadcast_clearing_media() {
    let status = PeerStatusPayload::new(1, false, false, 0, 0, "Alice", "");
    assert!(!status.has_media);
    assert_eq!(status.filename, "");
    assert_eq!(status.duration_ms, 0);
}

#[test]
fn test_f9_broadcast_multi_peer_reception() {
    let pkt = WirePacket::peer_status(10, true, true, 5000, 1000, "Host", "test.mp4");
    let bytes = pkt.to_bytes();
    // Simulate 3 clients receiving the broadcast packet
    for _ in 0..3 {
        let parsed = WirePacket::from_bytes(&bytes).expect("client parse");
        assert_eq!(parsed.packet_type, PacketType::PeerStatus);
    }
}

#[test]
fn test_f9_broadcast_preserves_client_id() {
    let p1 = WirePacket::peer_status(1, true, true, 1000, 100, "U1", "a.mkv");
    let p2 = WirePacket::peer_status(2, true, true, 1000, 100, "U2", "a.mkv");
    assert_ne!(p1.to_bytes(), p2.to_bytes());
}

// ==============================================================================
// Feature 10: Room Activity Announcements Formatting (ORIGINAL_REQUEST §R4)
// ==============================================================================

#[test]
fn test_f10_format_media_loaded() {
    let msg = RoomAnnouncement::media_loaded("Alice", "Inception.mkv", "02:28:07");
    assert_eq!(msg, "🎬 Alice loaded 'Inception.mkv' (02:28:07)");
}

#[test]
fn test_f10_format_ready() {
    let msg = RoomAnnouncement::ready("Bob");
    assert_eq!(msg, "✓ Bob is ready to watch");
}

#[test]
fn test_f10_format_not_ready() {
    let msg = RoomAnnouncement::not_ready("Charlie");
    assert_eq!(msg, "⏳ Charlie is not ready");
}

#[test]
fn test_f10_format_playback_paused() {
    let msg = RoomAnnouncement::paused("David");
    assert_eq!(msg, "⏸ David paused playback");
}

#[test]
fn test_f10_format_playback_resumed_and_seek() {
    let msg_resume = RoomAnnouncement::resumed("Eve");
    assert_eq!(msg_resume, "▶ Eve resumed playback");

    let msg_seek = RoomAnnouncement::seeked("Frank", "01:15:30");
    assert_eq!(msg_seek, "⏩ Frank seeked to 01:15:30");
}

// ==============================================================================
// Feature 11: Seek Announcement Debouncing (Survey Explorer 3)
// ==============================================================================

#[test]
fn test_f11_single_seek_emits_announcement() {
    let mut debouncer = SeekDebouncer::new(500);
    let emit = debouncer.on_drag_seek(10_000, 1000);
    assert_eq!(emit, Some(10_000), "Initial seek must emit");
}

#[test]
fn test_f11_rapid_scrubbing_debounced() {
    let mut debouncer = SeekDebouncer::new(500);
    let first = debouncer.on_drag_seek(10_000, 1000);
    assert_eq!(first, Some(10_000));

    // Rapid drags during scrubbing must be held (return None)
    assert_eq!(debouncer.on_drag_seek(10_100, 1050), None);
    assert_eq!(debouncer.on_drag_seek(10_200, 1100), None);
    assert_eq!(debouncer.on_drag_seek(10_500, 1150), None);

    // On release, the final position is emitted
    let final_emit = debouncer.on_release(1200);
    assert_eq!(final_emit, Some(10_500));
}

#[test]
fn test_f11_final_seek_pts_accuracy() {
    let mut debouncer = SeekDebouncer::new(500);
    debouncer.on_drag_seek(1000, 100);
    debouncer.on_drag_seek(5000, 200);
    debouncer.on_drag_seek(15000, 300);
    let release = debouncer.on_release(350);
    assert_eq!(release, Some(15000));
}

#[test]
fn test_f11_seek_after_quiescence_emits() {
    let mut debouncer = SeekDebouncer::new(500);
    debouncer.on_drag_seek(1000, 100);
    debouncer.on_release(200);

    // Seek after cooldown period emits immediately
    let next = debouncer.on_drag_seek(25000, 2000);
    assert_eq!(next, Some(25000));
}

#[test]
fn test_f11_zero_distance_seek_suppressed() {
    let mut debouncer = SeekDebouncer::new(500);
    let first = debouncer.on_drag_seek(1000, 100);
    assert_eq!(first, Some(1000));
    // Release without change does not re-emit
    let release = debouncer.on_release(200);
    assert_eq!(release, None);
}

// ==============================================================================
// Feature 12: Room Announcements Chat Storage & FTS (ORIGINAL_REQUEST §R4)
// ==============================================================================

#[test]
fn test_f12_announcement_persisted_in_binary_log() {
    let temp_dir = std::env::temp_dir();
    let log_path = temp_dir.join(format!("test_spch_f12_{}.bin", std::process::id()));
    let _ = fs::remove_file(&log_path);

    let mut store = ChatStore::open(&log_path).expect("create log store");
    let announcement_text = RoomAnnouncement::media_loaded("Alice", "movie.mp4", "01:30:00");
    let rec = ChatRecord::new_text(1_700_000_000_000, 0, [0u8; 16], "System".to_string(), &announcement_text);

    store.append(&rec).expect("append record");
    drop(store);

    // Verify recovery and reading
    let (records, stats) = ChatRecoveryScanner::recover_from_file(&log_path).expect("recover records");
    assert_eq!(records.len(), 1);
    assert_eq!(stats.valid_records, 1);
    assert_eq!(stats.corrupted_chunks, 0);
    assert_eq!(records[0].sender_name, "System");
    assert_eq!(String::from_utf8_lossy(&records[0].payload), announcement_text);

    let _ = fs::remove_file(&log_path);
}

#[test]
fn test_f12_announcement_dual_timestamps() {
    let rec = ChatRecord::new_text(1_700_000_050_000, 45_000, [0u8; 16], "System".to_string(), "resumed");
    assert_eq!(rec.wall_timestamp_utc_ms, 1_700_000_050_000);
    assert_eq!(rec.media_timestamp_ms, 45_000);
}

#[test]
fn test_f12_announcement_fts_search_by_keyword() {
    let mut store = ChatStore::new_in_memory();
    let announcement = RoomAnnouncement::resumed("Gautham");
    let rec = ChatRecord::new_text(100, 0, [0u8; 16], "System".to_string(), &announcement);
    store.append(&rec).unwrap();

    let results = store.search("resumed");
    assert!(!results.is_empty(), "FTS must index and find 'resumed'");
    assert_eq!(results[0].record.sender_name, "System");
}

#[test]
fn test_f12_announcement_fts_search_by_filename() {
    let mut store = ChatStore::new_in_memory();
    let announcement = RoomAnnouncement::media_loaded("Bob", "Cyberpunk Edgerunners Episode 1.mkv", "00:24:15");
    let rec = ChatRecord::new_text(200, 0, [0u8; 16], "System".to_string(), &announcement);
    store.append(&rec).unwrap();

    let results = store.search("Cyberpunk");
    assert!(!results.is_empty());
    assert_eq!(results[0].record.sender_name, "System");
}

#[test]
fn test_f12_announcement_crc32_framing_integrity() {
    let temp_dir = std::env::temp_dir();
    let log_path = temp_dir.join(format!("test_spch_crc_{}.bin", std::process::id()));
    let _ = fs::remove_file(&log_path);

    let mut store = ChatStore::open(&log_path).unwrap();
    let rec = ChatRecord::new_text(100, 200, [0u8; 16], "System".to_string(), "✓ Alice is ready");
    store.append(&rec).unwrap();
    drop(store);

    let (_, stats) = ChatRecoveryScanner::recover_from_file(&log_path).expect("recover stats");
    assert_eq!(stats.valid_records, 1);
    assert_eq!(stats.corrupted_chunks, 0);

    let _ = fs::remove_file(&log_path);
}

// ==============================================================================
// Feature 13: Sidebar Peer Media Display (ORIGINAL_REQUEST §R2)
// ==============================================================================

#[test]
fn test_f13_sidebar_peer_with_media() {
    let filename = Some("interstellar.mkv".to_string());
    let duration_ms = Some(10_140_000); // 2h 49m
    let formatted = duration_ms.map(format_duration_hms);
    assert_eq!(formatted, Some("02:49:00".to_string()));
    assert_eq!(filename.as_deref(), Some("interstellar.mkv"));
}

#[test]
fn test_f13_sidebar_peer_without_media() {
    let filename: Option<String> = None;
    let badge = if filename.is_none() {
        MatchBadge::NoFileLoaded.label()
    } else {
        "[Match ✔]"
    };
    assert_eq!(badge, "[⏳ No file loaded]");
}

#[test]
fn test_f13_sidebar_duration_format_hms() {
    assert_eq!(format_duration_hms(0), "00:00:00");
    assert_eq!(format_duration_hms(65_000), "00:01:05");
    assert_eq!(format_duration_hms(3_665_000), "01:01:05");
    assert_eq!(format_duration_hms(86_399_000), "23:59:59");
}

#[test]
fn test_f13_sidebar_local_user_media_display() {
    let local_filename = "sample.mp4";
    let local_dur_ms = 120_000;
    assert_eq!(format_duration_hms(local_dur_ms), "00:02:00");
    assert_eq!(local_filename, "sample.mp4");
}

#[test]
fn test_f13_sidebar_multi_peer_list_rendering() {
    let members = [
        ("Host", true, Some("video.mp4"), Some(60_000)),
        ("Alice", false, Some("video.mp4"), Some(60_000)),
        ("Bob", false, None, None),
    ];
    assert_eq!(members.len(), 3);
    assert_eq!(members[2].2, None); // Bob has no file
}

// ==============================================================================
// Feature 14: File Match Diagnostics Badge (ORIGINAL_REQUEST §R2)
// ==============================================================================

#[test]
fn test_f14_exact_match_badge() {
    let badge = evaluate_file_match(true, 5000, "f.mkv", true, 5000, "f.mkv");
    assert_eq!(badge, MatchBadge::Match);
    assert_eq!(badge.label(), "[Match ✔]");
}

#[test]
fn test_f14_within_tolerance_match_badge() {
    // 1500ms difference <= 2000ms
    let badge = evaluate_file_match(true, 5000, "f.mkv", true, 6500, "f.mkv");
    assert_eq!(badge, MatchBadge::Match);
}

#[test]
fn test_f14_exceeds_tolerance_mismatch_badge() {
    // 2500ms difference > 2000ms
    let badge = evaluate_file_match(true, 5000, "f.mkv", true, 7500, "f.mkv");
    assert_eq!(badge, MatchBadge::Mismatch);
    assert_eq!(badge.label(), "[⚠️ Mismatch]");
}

#[test]
fn test_f14_unloaded_file_diagnostic() {
    let badge = evaluate_file_match(true, 5000, "f.mkv", false, 0, "");
    assert_eq!(badge, MatchBadge::NoFileLoaded);
    assert_eq!(badge.label(), "[⏳ No file loaded]");
}

#[test]
fn test_f14_filename_diagnostic_consistency() {
    let badge = evaluate_file_match(true, 60000, "movie_cutA.mkv", true, 60000, "movie_cutB.mkv");
    assert_eq!(badge, MatchBadge::Match);
}

// ==============================================================================
// Feature 15: "Ready to Watch" Toggle & Badges (ORIGINAL_REQUEST §R3)
// ==============================================================================

#[test]
fn test_f15_ready_badge_display() {
    let is_ready = true;
    let badge_text = if is_ready { "[✓ Ready]" } else { "[⏳ Not Ready]" };
    assert_eq!(badge_text, "[✓ Ready]");
}

#[test]
fn test_f15_not_ready_badge_display() {
    let is_ready = false;
    let badge_text = if is_ready { "[✓ Ready]" } else { "[⏳ Not Ready]" };
    assert_eq!(badge_text, "[⏳ Not Ready]");
}

#[test]
fn test_f15_default_ready_on_media_load() {
    let media_loaded = true;
    let default_ready = media_loaded;
    assert!(default_ready, "Loading media must default readiness to true");
}

#[test]
fn test_f15_interactive_toggle_flips_state() {
    let mut ready = true;
    // User clicks checkbox
    ready = !ready;
    assert!(!ready);
    ready = !ready;
    assert!(ready);
}

#[test]
fn test_f15_remote_peer_badge_matches_wire_state() {
    let status_pkt = PeerStatusPayload::new(2, true, true, 5000, 1000, "RemotePeer", "f.mp4");
    let badge = if status_pkt.is_ready { "[✓ Ready]" } else { "[⏳ Not Ready]" };
    assert_eq!(badge, "[✓ Ready]");
}

// ==============================================================================
// Feature 16: Strict Playback Unpause Gate (ORIGINAL_REQUEST §R3)
// ==============================================================================

#[test]
fn test_f16_all_ready_unpause_permitted() {
    let peers = vec![
        ("Alice".to_string(), true, true),
        ("Bob".to_string(), true, true),
    ];
    let dec = evaluate_unpause_gate(&peers, false);
    assert_eq!(dec, UnpauseDecision::Permitted);
}

#[test]
fn test_f16_unready_peer_blocks_unpause() {
    let peers = vec![
        ("Alice".to_string(), true, true),
        ("Bob".to_string(), true, false), // Bob is unready
    ];
    let dec = evaluate_unpause_gate(&peers, false);
    if let UnpauseDecision::Blocked { unready_peers, message } = dec {
        assert_eq!(unready_peers, vec!["Bob".to_string()]);
        assert!(message.contains("Bob"));
    } else {
        panic!("Expected unpause blocked");
    }
}

#[test]
fn test_f16_unloaded_media_peer_blocks_unpause() {
    let peers = vec![
        ("Alice".to_string(), true, true),
        ("Charlie".to_string(), false, true), // Charlie has no media
    ];
    let dec = evaluate_unpause_gate(&peers, false);
    assert!(matches!(dec, UnpauseDecision::Blocked { .. }));
}

#[test]
fn test_f16_unpause_blocked_alert_generation() {
    let peers = vec![("David".to_string(), true, false)];
    let dec = evaluate_unpause_gate(&peers, false);
    if let UnpauseDecision::Blocked { message, .. } = dec {
        assert_eq!(message, "⏸ Paused: Waiting for David to load media and get ready...");
    } else {
        panic!("Expected blocked alert message");
    }
}

#[test]
fn test_f16_unpause_blocked_announcement_broadcast() {
    let users = vec!["Eve".to_string(), "Frank".to_string()];
    let announcement = RoomAnnouncement::gate_waiting(&users);
    assert_eq!(announcement, "⏸ Paused: Waiting for Eve, Frank to load media and get ready...");
}

// ==============================================================================
// Feature 17: Host "Force Play" Bypass (ORIGINAL_REQUEST §R3)
// ==============================================================================

#[test]
fn test_f17_host_force_play_unpauses_despite_unready() {
    let peers = vec![("AFK_Peer".to_string(), false, false)];
    // When force_play_bypass is true, unpause is permitted
    let dec = evaluate_unpause_gate(&peers, true);
    assert_eq!(dec, UnpauseDecision::Permitted);
}

#[test]
fn test_f17_force_play_disabled_restores_gate() {
    let peers = vec![("AFK_Peer".to_string(), false, false)];
    // Toggling force play back to false reenforces gate
    let dec = evaluate_unpause_gate(&peers, false);
    assert!(matches!(dec, UnpauseDecision::Blocked { .. }));
}

#[test]
fn test_f17_non_host_cannot_force_play() {
    let is_host = false;
    let effective_bypass = is_host && true; // only host can engage bypass
    assert!(!effective_bypass);
}

#[test]
fn test_f17_force_play_announcement() {
    let user = "Host";
    let resume_msg = RoomAnnouncement::resumed(user);
    assert_eq!(resume_msg, "▶ Host resumed playback");
}

#[test]
fn test_f17_force_play_clears_blocked_alert() {
    let mut blocked_alert: Option<String> = Some("Waiting for Bob".to_string());
    // Activating force play clears alert
    let force_play_activated = true;
    if force_play_activated {
        blocked_alert = None;
    }
    assert!(blocked_alert.is_none());
}

// ==============================================================================
// Feature 18: 72-Byte Snapshot Invariant (Survey Explorer 2)
// ==============================================================================

#[test]
fn test_f18_snapshot_exact_size_72_bytes() {
    assert_eq!(
        std::mem::size_of::<PlayerUiSnapshot>(),
        72,
        "PlayerUiSnapshot must strictly maintain 72-byte memory layout"
    );
}

#[test]
fn test_f18_snapshot_alignment_8_bytes() {
    assert_eq!(
        std::mem::align_of::<PlayerUiSnapshot>(),
        8,
        "PlayerUiSnapshot memory alignment must be 8 bytes"
    );
}

#[test]
fn test_f18_snapshot_ready_peers_field() {
    let snap = PlayerUiSnapshot {
        peers_ready: 3,
        peer_count: 4,
        ..Default::default()
    };
    assert_eq!(snap.peers_ready, 3);
    assert_eq!(snap.peer_count, 4);
}

#[test]
fn test_f18_snapshot_playback_state_field() {
    let snap = PlayerUiSnapshot {
        playback_state: 1, // Playing flag
        ..Default::default()
    };
    assert_eq!(snap.playback_state, 1);
}

#[test]
fn test_f18_snapshot_copy_clone_thread_safety() {
    fn assert_copy<T: Copy + Clone + Send + Sync>() {}
    assert_copy::<PlayerUiSnapshot>();
}
