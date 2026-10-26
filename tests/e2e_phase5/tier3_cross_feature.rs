//! Tier 3: Cross-Feature Combinations (Pairwise Coverage)
//!
//! Verifies interaction dynamics across disparate subsystems:
//! 1. Media Open + Instant Status Broadcast + Room Announcement
//! 2. Readiness Toggle + Unpause Gate + Room Announcement + Barrier Preroll
//! 3. File Mismatch + Warning Badge + Gate Block + Force Play Override
//! 4. Update Check during Active Watch-Party Session
//! 5. Scrubber Drag + Debounce Filter + Room Announcement + Binary Chat Log Persistence + FTS Indexing

use std::fs;

use syncplay_chat::{ChatRecord, ChatStore};
use syncplay_sync::wire::{
    PacketPayload, PacketType, WirePacket,
};
use syncplay_ui::gui::updater::parse_release_json;
use syncplay_ui::snapshot::PlayerUiSnapshot;
use syncplay_ui::UiEvent;

use super::contract_models::*;

#[test]
fn test_tier3_pair1_media_open_broadcast_and_announcement() {
    // 1. User opens a media file
    let username = "Alice";
    let filename = "Dune_Part_Two.mkv";
    let duration_ms = 9_960_000; // 2h 46m = 02:46:00
    let file_size_bytes = 4_500_000_000;
    let formatted_duration = format_duration_hms(duration_ms);
    assert_eq!(formatted_duration, "02:46:00");

    // 2. Wire status packet is generated and serialized
    let status_pkt = WirePacket::peer_status(
        1,
        true,
        true,
        duration_ms,
        file_size_bytes,
        username,
        filename,
    );
    let bytes = status_pkt.to_bytes();
    assert_eq!(bytes[4], 0x09);

    // 3. Room announcement is formatted
    let announcement = RoomAnnouncement::media_loaded(username, filename, &formatted_duration);
    assert_eq!(announcement, "🎬 Alice loaded 'Dune_Part_Two.mkv' (02:46:00)");

    // 4. Remote peer deserializes status and verifies payload
    let received = WirePacket::from_bytes(&bytes).expect("remote peer receives wire packet");
    if let PacketPayload::PeerStatus(p) = received.payload {
        assert_eq!(p.username, "Alice");
        assert_eq!(p.filename, "Dune_Part_Two.mkv");
        assert_eq!(p.duration_ms, 9_960_000);
        assert!(p.has_media);
        assert!(p.is_ready);
    } else {
        panic!("Expected PeerStatus");
    }
}

#[test]
fn test_tier3_pair2_readiness_toggle_unpause_gate_and_barrier() {
    let mut peers = vec![
        ("Alice".to_string(), true, true),   // Alice ready
        ("Bob".to_string(), true, false),   // Bob not ready
    ];

    // 1. Initial unpause attempt is BLOCKED by Bob
    let initial_dec = evaluate_unpause_gate(&peers, false);
    assert!(matches!(initial_dec, UnpauseDecision::Blocked { .. }));

    // 2. Bob toggles "Ready to Watch" -> wire packet broadcast
    let bob_ready_pkt = WirePacket::peer_status(2, true, true, 5000, 1000, "Bob", "video.mp4");
    let bytes = bob_ready_pkt.to_bytes();
    let parsed = WirePacket::from_bytes(&bytes).unwrap();
    if let PacketPayload::PeerStatus(p) = parsed.payload {
        assert!(p.is_ready);
    }
    peers[1].2 = true; // Bob is now ready

    // 3. Host sees all ready and triggers unpause -> gate PERMITS unpause
    let unpause_dec = evaluate_unpause_gate(&peers, false);
    assert_eq!(unpause_dec, UnpauseDecision::Permitted);

    // 4. Room activity announcements emitted for Bob ready & playback resume
    let ready_announcement = RoomAnnouncement::ready("Bob");
    assert_eq!(ready_announcement, "✓ Bob is ready to watch");
    let resume_announcement = RoomAnnouncement::resumed("Host");
    assert_eq!(resume_announcement, "▶ Host resumed playback");

    // 5. Barrier prepare scheduled
    let barrier_prep = WirePacket::barrier_prepare(1, 1_000_000, 200, false);
    assert_eq!(barrier_prep.packet_type, PacketType::BarrierPrepare);
}

#[test]
fn test_tier3_pair3_file_mismatch_warning_gate_block_and_force_play() {
    // Host loads theatrical cut (7200s), Peer loads extended cut (7230s -> delta 30s)
    let host_duration = 7_200_000;
    let peer_duration = 7_230_000;

    // 1. Diagnostic badge evaluates to Mismatch
    let badge = evaluate_file_match(true, host_duration, "theatrical.mkv", true, peer_duration, "extended.mkv");
    assert_eq!(badge, MatchBadge::Mismatch);
    assert_eq!(badge.label(), "[⚠️ Mismatch]");

    // 2. Peer is confused by mismatch and remains unready
    let peers = vec![("ConfusedPeer".to_string(), true, false)];
    let gate_dec = evaluate_unpause_gate(&peers, false);
    assert!(matches!(gate_dec, UnpauseDecision::Blocked { .. }));

    // 3. Host decides to proceed anyway with Force Play override
    let host_override_dec = evaluate_unpause_gate(&peers, true);
    assert_eq!(host_override_dec, UnpauseDecision::Permitted);
}

#[test]
fn test_tier3_pair4_update_check_during_active_watch_party() {
    // 1. Watch party is actively playing with 3 peers
    let mut snap = PlayerUiSnapshot {
        playback_state: 1, // Playing
        peer_count: 3,
        peers_ready: 3,
        media_position_pts: 45_000_000_000, // 45s
        ..Default::default()
    };
    assert_eq!(snap.playback_state, 1);
    assert_eq!(snap.peer_count, 3);

    // 2. Background update check detects newer release
    let release_json = serde_json::json!({
        "tag_name": "v0.1.2",
        "html_url": "https://github.com/release/v0.1.2",
        "assets": [
            {
                "name": "syncplay-alt-linux.tar.gz",
                "browser_download_url": "https://example.com/syncplay-alt.tar.gz"
            }
        ]
    });
    let update_info = parse_release_json("0.1.1", &release_json).expect("parse").expect("update available");
    assert_eq!(update_info.latest_version, "0.1.2");

    // 3. UI event dispatched to display top notification banner
    let event = UiEvent::UpdateAvailable {
        latest_version: update_info.latest_version.clone(),
        release_url: update_info.release_url.clone(),
        download_url: update_info.download_url.clone(),
    };
    assert!(matches!(event, UiEvent::UpdateAvailable { .. }));

    // 4. Playback snapshot remains completely undisturbed
    snap.media_position_pts += 1_000_000_000;
    assert_eq!(snap.playback_state, 1);
    assert_eq!(snap.peer_count, 3);
}

#[test]
fn test_tier3_pair5_scrubber_drag_debounce_announcement_and_fts() {
    let temp_dir = std::env::temp_dir();
    let log_path = temp_dir.join(format!("test_spch_tier3_pair5_{}.bin", std::process::id()));
    let _ = fs::remove_file(&log_path);

    // 1. Rapid 10Hz scrubber drag
    let mut debouncer = SeekDebouncer::new(500);
    let mut emitted = Vec::new();

    for i in 0..10 {
        let pts = 10_000 + i * 500;
        let time_ms = 1000 + i * 50;
        if let Some(target) = debouncer.on_drag_seek(pts, time_ms) {
            emitted.push(target);
        }
    }
    // Only the very first seek was emitted during the drag
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0], 10_000);

    // 2. Drag finishes on release -> final seek emitted
    let final_seek = debouncer.on_release(1600).expect("final seek on release");
    assert_eq!(final_seek, 14_500);

    // 3. Format seek announcement
    let timecode = format_duration_hms(final_seek);
    assert_eq!(timecode, "00:00:14");
    let announcement = RoomAnnouncement::seeked("Alice", &timecode);
    assert_eq!(announcement, "⏩ Alice seeked to 00:00:14");

    // 4. Persist announcement to binary chat log & index in FTS
    let mut store = ChatStore::open(&log_path).expect("create log store");
    let record = ChatRecord::new_text(
        1_700_000_100_000,
        final_seek,
        [0u8; 16],
        "System".to_string(),
        &announcement,
    );
    store.append(&record).expect("append seek announcement");

    // 5. Query through unified ChatStore FTS search
    let search_res = store.search("seeked");
    assert!(!search_res.is_empty());
    assert_eq!(search_res[0].record.sender_name, "System");

    // Clean up
    let _ = fs::remove_file(&log_path);
}
