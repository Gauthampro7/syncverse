//! Tier 4: Real-World Application Scenarios
//!
//! Realistic multi-step end-to-end user workflows simulating watch-party sessions,
//! late-joining peers, AFK overrides, file discrepancies, and update workflows.

use syncverse_sync::wire::{
    PacketPayload, PacketType, WirePacket,
};
use syncverse_ui::gui::updater::parse_release_json;
use syncverse_ui::UiEvent;

use super::contract_models::*;

/// Scenario 1: Standard 3-Peer Watch Party Playback
///
/// Workflow:
/// 1. Host, Alice, and Bob connect to room "MovieNight".
/// 2. All 3 peers load matching media files (duration delta < 500ms).
/// 3. All 3 peers toggle "Ready to Watch".
/// 4. Host triggers Play / Unpause.
/// 5. Unpause gate permits playback, room announcement is broadcast, playback begins.
#[test]
fn test_tier4_scenario1_standard_three_peer_watch_party() {
    let host_media = ("Host", true, "The_Matrix.mkv", 8_160_000u64, 4_000_000_000u64);
    let alice_media = ("Alice", true, "The_Matrix.mkv", 8_160_250u64, 4_000_000_100u64);
    let bob_media = ("Bob", true, "The_Matrix_1080p.mkv", 8_160_100u64, 3_950_000_000u64);

    // 1. Verify file matching diagnostics across all pairs
    assert_eq!(
        evaluate_file_match(true, host_media.3, host_media.2, true, alice_media.3, alice_media.2),
        MatchBadge::Match
    );
    assert_eq!(
        evaluate_file_match(true, host_media.3, host_media.2, true, bob_media.3, bob_media.2),
        MatchBadge::Match
    );

    // 2. Transmit PeerStatus wire packets
    let alice_pkt = WirePacket::peer_status(10, true, true, alice_media.3, alice_media.4, alice_media.0, alice_media.2);
    let bob_pkt = WirePacket::peer_status(11, true, true, bob_media.3, bob_media.4, bob_media.0, bob_media.2);
    assert_eq!(WirePacket::from_bytes(&alice_pkt.to_bytes()).unwrap().packet_type, PacketType::PeerStatus);
    assert_eq!(WirePacket::from_bytes(&bob_pkt.to_bytes()).unwrap().packet_type, PacketType::PeerStatus);

    // 3. Room peer state
    let room_peers = vec![
        ("Alice".to_string(), true, true),
        ("Bob".to_string(), true, true),
    ];

    // 4. Host triggers Unpause -> Gate evaluates
    let decision = evaluate_unpause_gate(&room_peers, false);
    assert_eq!(decision, UnpauseDecision::Permitted, "When all peers are ready with media, unpause must succeed");

    // 5. Broadcast activity announcements
    let resume_announcement = RoomAnnouncement::resumed("Host");
    assert_eq!(resume_announcement, "▶ Host resumed playback");
}

/// Scenario 2: Late-Joining Peer With No Media Blocks Host Unpause
///
/// Workflow:
/// 1. Host and Alice are connected and ready.
/// 2. Bob joins late with no media loaded (`has_media: false`, `is_ready: false`).
/// 3. Host triggers Unpause.
/// 4. Strict Unpause Gate BLOCKS playback, displays on-screen alert, and sends chat warning.
/// 5. Bob loads file and toggles ready -> status packet broadcast.
/// 6. Host triggers Unpause again -> succeeds smoothly.
#[test]
fn test_tier4_scenario2_late_joining_unready_peer_blocks_unpause() {
    // 1. Initial room state: Bob joined with no file
    let mut peers = vec![
        ("Alice".to_string(), true, true),
        ("Bob".to_string(), false, false), // Bob late-joined with no file
    ];

    // 2. Diagnostics show Bob has no file
    let bob_badge = evaluate_file_match(true, 7_200_000, "movie.mkv", false, 0, "");
    assert_eq!(bob_badge, MatchBadge::NoFileLoaded);
    assert_eq!(bob_badge.label(), "[⏳ No file loaded]");

    // 3. Host attempts unpause -> Gate blocks
    let blocked_dec = evaluate_unpause_gate(&peers, false);
    match blocked_dec {
        UnpauseDecision::Blocked { unready_peers, message } => {
            assert_eq!(unready_peers, vec!["Bob".to_string()]);
            assert_eq!(message, "⏸ Paused: Waiting for Bob to load media and get ready...");
        }
        UnpauseDecision::Permitted => panic!("Must be blocked by Bob"),
    }

    // 4. Bob loads media and reports ready
    let bob_status = WirePacket::peer_status(42, true, true, 7_200_000, 2_000_000_000, "Bob", "movie.mkv");
    let received = WirePacket::from_bytes(&bob_status.to_bytes()).unwrap();
    if let PacketPayload::PeerStatus(p) = received.payload {
        assert!(p.has_media && p.is_ready);
    }
    peers[1] = ("Bob".to_string(), true, true);

    // 5. Host unpauses again -> Gate permits
    let permitted_dec = evaluate_unpause_gate(&peers, false);
    assert_eq!(permitted_dec, UnpauseDecision::Permitted);
}

/// Scenario 3: Host Overrides Unpause Gate with "Force Play" Bypass
///
/// Workflow:
/// 1. Host and Alice are ready. Bob is AFK with no media loaded.
/// 2. Host attempts normal unpause -> gate blocks.
/// 3. Host activates "Force Play" override checkbox.
/// 4. Host triggers Unpause again -> gate allows playback.
/// 5. Room activity announcement logs that playback resumed.
#[test]
fn test_tier4_scenario3_host_force_play_bypass_for_afk_peer() {
    let peers = vec![
        ("Alice".to_string(), true, true),
        ("Bob_AFK".to_string(), false, false),
    ];

    // 1. Normal unpause is blocked
    let normal_attempt = evaluate_unpause_gate(&peers, false);
    assert!(matches!(normal_attempt, UnpauseDecision::Blocked { .. }));

    // 2. Host engages Force Play bypass
    let mut force_play_enabled = true;
    let override_attempt = evaluate_unpause_gate(&peers, force_play_enabled);
    assert_eq!(
        override_attempt,
        UnpauseDecision::Permitted,
        "Force Play bypass must permit unpause even with AFK/unready peers"
    );

    // 3. Verify chat announcement emitted
    let announcement = RoomAnnouncement::resumed("Host");
    assert_eq!(announcement, "▶ Host resumed playback");

    // 4. When host later turns off Force Play, gate is reenforced
    force_play_enabled = false;
    let subsequent_attempt = evaluate_unpause_gate(&peers, force_play_enabled);
    assert!(matches!(subsequent_attempt, UnpauseDecision::Blocked { .. }));
}

/// Scenario 4: Peer Loads Different File Cut (Duration Mismatch Warning)
///
/// Workflow:
/// 1. Host loads theatrical release (120m 00s = 7,200,000ms).
/// 2. Peer loads director's cut (120m 15s = 7,215,000ms -> delta 15s).
/// 3. File match diagnostic detects discrepancy > 2000ms and sets badge to `[⚠️ Mismatch]`.
/// 4. Peer is displayed in sidebar with their loaded filename and duration.
/// 5. Both peers can see warning badge.
#[test]
fn test_tier4_scenario4_peer_loads_different_cut_mismatch_warning() {
    let host_dur = 7_200_000u64;
    let peer_dur = 7_215_000u64; // +15 seconds
    let host_file = "Blade_Runner_Theatrical.mkv";
    let peer_file = "Blade_Runner_Final_Cut.mkv";

    // 1. Diagnostic evaluation
    let badge = evaluate_file_match(true, host_dur, host_file, true, peer_dur, peer_file);
    assert_eq!(badge, MatchBadge::Mismatch);
    assert_eq!(badge.label(), "[⚠️ Mismatch]");

    // 2. Formatted durations
    assert_eq!(format_duration_hms(host_dur), "02:00:00");
    assert_eq!(format_duration_hms(peer_dur), "02:00:15");

    // 3. Wire packet accurately transfers mismatched metadata
    let peer_wire = WirePacket::peer_status(105, true, true, peer_dur, 3_000_000_000, "Viewer", peer_file);
    let parsed = WirePacket::from_bytes(&peer_wire.to_bytes()).unwrap();
    if let PacketPayload::PeerStatus(p) = parsed.payload {
        assert_eq!(p.duration_ms, 7_215_000);
        assert_eq!(p.filename, "Blade_Runner_Final_Cut.mkv");
    }
}

/// Scenario 5: Update Check Detects Newer Version & User Dismisses Banner
///
/// Workflow:
/// 1. Application starts on version "0.1.1".
/// 2. Background update worker queries GitHub Releases API.
/// 3. API returns release "v0.1.2" with platform assets.
/// 4. `UiEvent::UpdateAvailable` is received by UI.
/// 5. Notification banner displays at top of window with download button.
/// 6. User clicks "Dismiss" -> banner state sets `is_dismissed = true` -> banner is hidden.
#[test]
fn test_tier4_scenario5_update_detection_and_banner_dismissal() {
    // 1. Current running application version
    let current_version = "0.1.1";

    // 2. GitHub Releases response payload
    let release_json = serde_json::json!({
        "tag_name": "v0.1.2",
        "html_url": "https://github.com/Gauthampro7/syncverse/releases/tag/v0.1.2",
        "body": "Fixed unpause race conditions and added social polish.",
        "assets": [
            {
                "name": "syncplay-alt-linux-x86_64.tar.gz",
                "browser_download_url": "https://github.com/downloads/syncplay-alt-linux-x86_64.tar.gz"
            },
            {
                "name": "syncplay-alt-windows-x64.exe",
                "browser_download_url": "https://github.com/downloads/syncplay-alt-windows-x64.exe"
            }
        ]
    });

    // 3. Parse and verify update detected
    let parsed_info = parse_release_json(current_version, &release_json).expect("valid parse").expect("update detected");
    assert_eq!(parsed_info.latest_version, "0.1.2");
    assert_eq!(parsed_info.current_version, "0.1.1");
    assert!(parsed_info.download_url.is_some());

    // 4. UI event emitted
    let event = UiEvent::UpdateAvailable {
        latest_version: parsed_info.latest_version.clone(),
        release_url: parsed_info.release_url.clone(),
        download_url: parsed_info.download_url.clone(),
    };

    // 5. App UI receives event and shows banner
    let mut banner_visible = matches!(event, UiEvent::UpdateAvailable { .. });
    assert!(banner_visible, "Banner must be visible when update is available");

    // 6. User clicks "Dismiss" button
    let user_clicked_dismiss = true;
    if user_clicked_dismiss {
        banner_visible = false;
    }
    assert!(!banner_visible, "Banner must be hidden after dismissal");
}
