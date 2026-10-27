//! Tier 2: Boundary & Corner Cases
//!
//! Tests boundary conditions, extreme values, and error-handling edge cases
//! across wire packets, file matching tolerances, SemVer comparison, unpause gating,
//! and network rate-limiting.

use syncverse_sync::wire::{
    PacketPayload, WirePacket,
};
use syncverse_ui::gui::updater::{
    is_newer_version, normalize_semver_tag, parse_release_json, UpdateError,
};

use super::contract_models::*;

// ==============================================================================
// Domain 1: Wire Protocol Data Extremes & String Boundaries
// ==============================================================================

#[test]
fn test_tier2_bva_wire_empty_strings() {
    let pkt = WirePacket::peer_status(0, false, false, 0, 0, "", "");
    let bytes = pkt.to_bytes();
    let decoded = WirePacket::from_bytes(&bytes).expect("parse empty strings packet");
    if let PacketPayload::PeerStatus(p) = decoded.payload {
        assert_eq!(p.username, "");
        assert_eq!(p.filename, "");
        assert_eq!(p.duration_ms, 0);
        assert_eq!(p.file_size_bytes, 0);
    } else {
        panic!("Expected PeerStatus");
    }
}

#[test]
fn test_tier2_bva_wire_unicode_500_char_filename() {
    // 500+ characters of multi-byte UTF-8 emojis and kanji
    let long_unicode = "🍿🎬✨🚀日本語тест_long_name_".repeat(25);
    assert!(long_unicode.chars().count() >= 500);

    let pkt = WirePacket::peer_status(999, true, true, 123_456, 10_000, "UserUnicode", &long_unicode);
    let bytes = pkt.to_bytes();
    let decoded = WirePacket::from_bytes(&bytes).expect("parse long unicode filename");
    if let PacketPayload::PeerStatus(p) = decoded.payload {
        assert_eq!(p.filename, long_unicode);
    } else {
        panic!("Expected PeerStatus");
    }
}

#[test]
fn test_tier2_bva_wire_zero_duration_and_size() {
    let pkt = WirePacket::peer_status(1, true, true, 0, 0, "U", "zero.mp4");
    let bytes = pkt.to_bytes();
    let decoded = WirePacket::from_bytes(&bytes).unwrap();
    if let PacketPayload::PeerStatus(p) = decoded.payload {
        assert_eq!(p.duration_ms, 0);
        assert_eq!(p.file_size_bytes, 0);
    }
}

#[test]
fn test_tier2_bva_wire_u64_max_duration_and_size() {
    let pkt = WirePacket::peer_status(u32::MAX, true, true, u64::MAX, u64::MAX, "MaxUser", "infinite.mkv");
    let bytes = pkt.to_bytes();
    let decoded = WirePacket::from_bytes(&bytes).expect("parse u64::MAX fields");
    if let PacketPayload::PeerStatus(p) = decoded.payload {
        assert_eq!(p.client_id, u32::MAX);
        assert_eq!(p.duration_ms, u64::MAX);
        assert_eq!(p.file_size_bytes, u64::MAX);
    }
}

#[test]
fn test_tier2_bva_wire_tampered_single_bit_payload() {
    let pkt = WirePacket::peer_status(12, true, true, 50_000, 20_000, "Alice", "test.mp4");
    let mut bytes = pkt.to_bytes();
    // Invert the very last bit
    let len = bytes.len();
    bytes[len - 1] ^= 0x01;
    let res = WirePacket::from_bytes(&bytes);
    assert!(res.is_err(), "Single bit tampering must fail CRC32 verification");
}

// ==============================================================================
// Domain 2: Exact ±2000ms Match Tolerance Boundaries
// ==============================================================================

#[test]
fn test_tier2_bva_match_exact_zero_delta() {
    let badge = evaluate_file_match(true, 100_000, "f.mkv", true, 100_000, "f.mkv");
    assert_eq!(badge, MatchBadge::Match, "Delta = 0ms must be Match");
}

#[test]
fn test_tier2_bva_match_positive_boundary_1999ms() {
    let badge = evaluate_file_match(true, 100_000, "f.mkv", true, 101_999, "f.mkv");
    assert_eq!(badge, MatchBadge::Match, "Delta = +1999ms must be Match");
}

#[test]
fn test_tier2_bva_match_positive_boundary_2000ms() {
    let badge = evaluate_file_match(true, 100_000, "f.mkv", true, 102_000, "f.mkv");
    assert_eq!(badge, MatchBadge::Match, "Delta = +2000ms (exact boundary) must be Match");
}

#[test]
fn test_tier2_bva_match_positive_boundary_2001ms() {
    let badge = evaluate_file_match(true, 100_000, "f.mkv", true, 102_001, "f.mkv");
    assert_eq!(badge, MatchBadge::Mismatch, "Delta = +2001ms must be Mismatch");
}

#[test]
fn test_tier2_bva_match_negative_boundary_2000ms() {
    let badge = evaluate_file_match(true, 102_000, "f.mkv", true, 100_000, "f.mkv");
    assert_eq!(badge, MatchBadge::Match, "Delta = -2000ms (exact boundary) must be Match");
}

#[test]
fn test_tier2_bva_match_negative_boundary_2001ms() {
    let badge = evaluate_file_match(true, 102_001, "f.mkv", true, 100_000, "f.mkv");
    assert_eq!(badge, MatchBadge::Mismatch, "Delta = -2001ms must be Mismatch");
}

// ==============================================================================
// Domain 3: SemVer Boundary Value Analysis & Invalid Formats
// ==============================================================================

#[test]
fn test_tier2_bva_semver_v_prefix_normalization_equality() {
    let v1 = normalize_semver_tag("v0.1.1").unwrap();
    let v2 = normalize_semver_tag("0.1.1").unwrap();
    let v3 = normalize_semver_tag("  V0.1.1  ").unwrap();
    assert_eq!(v1, v2);
    assert_eq!(v2, v3);
}

#[test]
fn test_tier2_bva_semver_prerelease_comparison() {
    // In SemVer 2.0.0: normal version > prerelease version with same major/minor/patch
    // 0.1.2 > 0.1.2-alpha.1
    assert!(is_newer_version("0.1.2-alpha.1", "0.1.2").unwrap());
    // 0.1.2-beta > 0.1.2-alpha
    assert!(is_newer_version("0.1.2-alpha", "0.1.2-beta").unwrap());
}

#[test]
fn test_tier2_bva_semver_empty_and_whitespace_tags() {
    assert!(normalize_semver_tag("").is_err());
    assert!(normalize_semver_tag("   ").is_err());
    assert!(normalize_semver_tag("v").is_err());
}

#[test]
fn test_tier2_bva_semver_invalid_string_rejection() {
    assert!(is_newer_version("0.1.1", "beta_version").is_err());
    assert!(is_newer_version("0.1.1", "1.2.3.4").is_err());
    assert!(is_newer_version("0.1.1", "v1").is_err());
}

#[test]
fn test_tier2_bva_semver_extreme_version_numbers() {
    assert!(is_newer_version("9999.9999.9999", "10000.0.0").unwrap());
}

// ==============================================================================
// Domain 4: Unpause Gate Peer Cardinality & Transition Boundaries
// ==============================================================================

#[test]
fn test_tier2_bva_unpause_solo_host_zero_peers() {
    // 0 remote peers (solo watch session)
    let peers: Vec<(String, bool, bool)> = Vec::new();
    let dec = evaluate_unpause_gate(&peers, false);
    assert_eq!(dec, UnpauseDecision::Permitted, "Solo player with 0 peers must unpause cleanly");
}

#[test]
fn test_tier2_bva_unpause_single_ready_peer() {
    let peers = vec![("Alice".to_string(), true, true)];
    let dec = evaluate_unpause_gate(&peers, false);
    assert_eq!(dec, UnpauseDecision::Permitted);
}

#[test]
fn test_tier2_bva_unpause_single_unready_peer() {
    let peers = vec![("Alice".to_string(), true, false)];
    let dec = evaluate_unpause_gate(&peers, false);
    assert!(matches!(dec, UnpauseDecision::Blocked { .. }));
}

#[test]
fn test_tier2_bva_unpause_large_room_one_unready_blocks() {
    let peers = vec![
        ("P1".to_string(), true, true),
        ("P2".to_string(), true, true),
        ("P3".to_string(), true, true),
        ("P4".to_string(), true, true),
        ("P5".to_string(), true, false), // Only P5 is not ready
    ];
    let dec = evaluate_unpause_gate(&peers, false);
    if let UnpauseDecision::Blocked { unready_peers, .. } = dec {
        assert_eq!(unready_peers, vec!["P5".to_string()]);
    } else {
        panic!("Single unready peer in 5-peer room must block unpause");
    }
}

#[test]
fn test_tier2_bva_unpause_force_play_toggle_transition() {
    let peers = vec![("Bob".to_string(), false, false)];

    // 1. Initial attempt without force play -> Blocked
    assert!(matches!(evaluate_unpause_gate(&peers, false), UnpauseDecision::Blocked { .. }));

    // 2. Host engages force play -> Permitted
    assert_eq!(evaluate_unpause_gate(&peers, true), UnpauseDecision::Permitted);

    // 3. Host disengages force play -> Blocked again
    assert!(matches!(evaluate_unpause_gate(&peers, false), UnpauseDecision::Blocked { .. }));
}

// ==============================================================================
// Domain 5: Update Checker Network & Error Boundaries
// ==============================================================================

#[test]
fn test_tier2_bva_update_rate_limit_parsing() {
    let err = UpdateError::RateLimited {
        reset_time: "1730000000".to_string(),
    };
    let display = format!("{}", err);
    assert!(display.contains("GitHub API rate limit exceeded"));
    assert!(display.contains("1730000000"));
}

#[test]
fn test_tier2_bva_update_timeout_error() {
    let err = UpdateError::Timeout(5);
    let display = format!("{}", err);
    assert!(display.contains("timed out after 5 seconds"));
}

#[test]
fn test_tier2_bva_update_http_status_codes() {
    let err_404 = UpdateError::HttpStatus {
        status: 404,
        message: "Not Found".to_string(),
    };
    assert_eq!(err_404.to_string(), "GitHub API returned HTTP status 404: Not Found");

    let err_429 = UpdateError::RateLimited {
        reset_time: "in 60 seconds".to_string(),
    };
    assert!(err_429.to_string().contains("rate limit"));
}

#[test]
fn test_tier2_bva_update_malformed_json_types() {
    // 'tag_name' is an integer instead of a string
    let bad_json = serde_json::json!({
        "tag_name": 12345,
        "html_url": "https://example.com"
    });
    let res = parse_release_json("0.1.1", &bad_json);
    assert!(matches!(res, Err(UpdateError::ParseError(_))));
}

#[test]
fn test_tier2_bva_update_empty_json_object() {
    let empty_json = serde_json::json!({});
    let res = parse_release_json("0.1.1", &empty_json);
    assert!(matches!(res, Err(UpdateError::ParseError(_))));
}
