//! Empirical integration test suite validating VLC-grade features:
//! - M3U loading with `#EXTINF`, `file://` URIs, and percent-encoding
//! - Drag-and-drop directory scanning and file categorization
//! - A-B repeat precision nudging and duration computation
//! - Viewport zoom pan bounding and geometry clamping
//! - Single-item playlist repeat-all behavior on EOF

use std::path::PathBuf;
use syncverse_ui::gui::ab_loop::AbRepeatState;
use syncverse_ui::gui::aspect_ratio::ZoomMode;
use syncverse_ui::gui::drag_drop::{is_supported_media, is_supported_playlist, is_supported_subtitle};
use syncverse_ui::gui::playlist::{decode_percent_encoded, PlaylistManager, PlaylistRepeatMode};
use syncverse_ui::gui::viewport::VideoViewport;

#[test]
fn test_m3u_file_uri_and_extinf_full_flow() {
    let dir = std::env::temp_dir().join(format!("syncverse_m3u_e2e_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let m3u_path = dir.join("full_test.m3u8");

    let m3u_body = "\
#EXTM3U
#EXTINF:354,Dire Straits - Sultans of Swing
file:///media/Music/Dire%20Straits%20-%20Sultans%20of%20Swing.flac
#EXTINF:120,Sample Track 2
local_track.mp3
#EXTINF:90,Special%20Title%20With%20Symbols
file://localhost/var/media/track3.ogg
";
    std::fs::write(&m3u_path, m3u_body).expect("write m3u must succeed");

    let mut mgr = PlaylistManager::new();
    let loaded = mgr.load_from_m3u(&m3u_path).expect("load m3u must succeed");
    assert_eq!(loaded, 3);
    assert_eq!(mgr.items.len(), 3);

    // Track 1
    assert_eq!(mgr.items[0].filename, "Dire Straits - Sultans of Swing");
    assert_eq!(mgr.items[0].path, PathBuf::from("/media/Music/Dire Straits - Sultans of Swing.flac"));

    // Track 2 (relative)
    assert_eq!(mgr.items[1].filename, "Sample Track 2");
    assert_eq!(mgr.items[1].path, dir.join("local_track.mp3"));

    // Track 3 (file://localhost)
    assert_eq!(mgr.items[2].filename, "Special%20Title%20With%20Symbols");
    assert_eq!(mgr.items[2].path, PathBuf::from("/var/media/track3.ogg"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_percent_decoding_edge_cases() {
    assert_eq!(decode_percent_encoded("test%20file.mp4"), "test file.mp4");
    assert_eq!(decode_percent_encoded("a%2Fb%2Fc.mkv"), "a/b/c.mkv");
    assert_eq!(decode_percent_encoded("no_escapes.wav"), "no_escapes.wav");
    assert_eq!(decode_percent_encoded("incomplete%2"), "incomplete%2");
    assert_eq!(decode_percent_encoded("invalid%ZZhex"), "invalid%ZZhex");
}

#[test]
fn test_ab_repeat_precision_nudging_limits() {
    let mut state = AbRepeatState::Active { a_ms: 10_000, b_ms: 20_000 };
    assert_eq!(state.duration_ms(), Some(10_000));

    // Nudge A forward
    state.nudge_point_a(100);
    assert_eq!(state.point_a(), Some(10_100));
    assert_eq!(state.duration_ms(), Some(9_900));

    // Nudge A backward
    state.nudge_point_a(-200);
    assert_eq!(state.point_a(), Some(9_900));
    assert_eq!(state.duration_ms(), Some(10_100));

    // Nudge A cannot cross or equal B - 100ms
    state.nudge_point_a(15_000); // 9900 + 15000 = 24900 > 20000 - 100
    assert_eq!(state.point_a(), Some(9_900)); // unchanged

    // Nudge B forward with max duration 30000
    state.nudge_point_b(500, 30_000);
    assert_eq!(state.point_b(), Some(20_500));

    // Nudge B past max duration clamps to max
    state.nudge_point_b(20_000, 30_000);
    assert_eq!(state.point_b(), Some(30_000));

    // Nudge B cannot fall below A + 100ms
    state.nudge_point_b(-25_000, 30_000); // 30000 - 25000 = 5000 < 9900 + 100
    assert_eq!(state.point_b(), Some(30_000)); // unchanged
}

#[test]
fn test_viewport_pan_bounding_across_zoom_modes() {
    let mut viewport = VideoViewport::new();
    viewport.set_video_dimensions(1920, 1080);

    let available = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1920.0, 1080.0));

    // In Fit mode: pan is always zero regardless of pan_offset
    viewport.set_zoom_mode(ZoomMode::Fit);
    viewport.set_pan_offset(egui::vec2(500.0, 300.0));
    let rect_fit = viewport.compute_video_rect(available).unwrap();
    assert_eq!(rect_fit.min, egui::pos2(0.0, 0.0));

    // In Double mode (2:1): video is 3840 x 2160. Max pan is +/-960 x and +/-540 y
    viewport.set_zoom_mode(ZoomMode::Double);
    viewport.set_pan_offset(egui::vec2(2000.0, 2000.0));
    let rect_double = viewport.compute_video_rect(available).unwrap();
    // Centered min is -960. Clamped pan.x is +960. -960 + 960 = 0.0
    // Centered min is -540. Clamped pan.y is +540. -540 + 540 = 0.0
    assert_eq!(rect_double.min, egui::pos2(0.0, 0.0));
}

#[test]
fn test_playlist_single_track_repeat_all_eof_advancement() {
    let mut mgr = PlaylistManager::new();
    mgr.add_file(PathBuf::from("/media/single_video.mkv"));
    mgr.current_index = Some(0);

    // Repeat Off: next_track returns None
    mgr.repeat_mode = PlaylistRepeatMode::Off;
    assert_eq!(mgr.next_track(), None);

    // Repeat All: next_track loops back to index 0
    mgr.repeat_mode = PlaylistRepeatMode::All;
    let next = mgr.next_track();
    assert_eq!(next, Some(PathBuf::from("/media/single_video.mkv")));
    assert_eq!(mgr.current_index, Some(0));

    // Repeat Track: next_track repeats current track
    mgr.repeat_mode = PlaylistRepeatMode::Track;
    assert_eq!(mgr.next_track(), Some(PathBuf::from("/media/single_video.mkv")));
}

#[test]
fn test_drag_drop_extension_matching() {
    assert!(is_supported_media(std::path::Path::new("/tmp/test.webm")));
    assert!(is_supported_media(std::path::Path::new("/tmp/test.mkv")));
    assert!(is_supported_media(std::path::Path::new("/tmp/test.mp4")));
    assert!(is_supported_media(std::path::Path::new("/tmp/test.opus")));
    assert!(is_supported_playlist(std::path::Path::new("/tmp/test.m3u")));
    assert!(is_supported_playlist(std::path::Path::new("/tmp/test.m3u8")));
    assert!(is_supported_subtitle(std::path::Path::new("/tmp/test.srt")));
    assert!(is_supported_subtitle(std::path::Path::new("/tmp/test.ass")));
    assert!(!is_supported_media(std::path::Path::new("/tmp/test.exe")));
}

#[test]
fn test_ab_repeat_point_a_set_nudging_and_loop_evaluation() {
    let mut state = AbRepeatState::PointASet(15_000);
    assert_eq!(state.point_a(), Some(15_000));
    assert_eq!(state.point_b(), None);
    assert!(!state.is_active());

    // Nudging Point A while in PointASet state
    state.nudge_point_a(250);
    assert_eq!(state.point_a(), Some(15_250));

    state.nudge_point_a(-500);
    assert_eq!(state.point_a(), Some(14_750));

    // Nudging Point A negative cannot fall below 0
    state.nudge_point_a(-20_000);
    assert_eq!(state.point_a(), Some(0));

    // Set Point B -> Active state
    state.set_point_b(30_000);
    assert!(state.is_active());
    assert_eq!(state.point_a(), Some(0));
    assert_eq!(state.point_b(), Some(30_000));

    // Evaluate loop restarts at or past Point B
    assert_eq!(state.evaluate_loop(29_999), None);
    assert_eq!(state.evaluate_loop(30_000), Some(0));
    assert_eq!(state.evaluate_loop(35_000), Some(0));
}

#[test]
fn test_aspect_ratio_presets_complete_synchronization() {
    use syncverse_ui::gui::app::ASPECT_RATIO_PRESETS;
    use syncverse_ui::gui::aspect_ratio::AspectRatioOverride;

    assert_eq!(ASPECT_RATIO_PRESETS.len(), AspectRatioOverride::ALL.len());
    for (i, &preset) in AspectRatioOverride::ALL.iter().enumerate() {
        assert_eq!(ASPECT_RATIO_PRESETS[i], preset.display_name());
    }
}

#[test]
fn test_m3u_windows_drive_letter_decoding() {
    let dir = std::env::temp_dir().join(format!("syncverse_m3u_win_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let m3u_path = dir.join("win_test.m3u");

    let m3u_body = "\
#EXTM3U
#EXTINF:200,Windows Track
file:///C:/Users/Music/My%20Song%20File.mp3
";
    std::fs::write(&m3u_path, m3u_body).expect("write win m3u must succeed");

    let mut mgr = PlaylistManager::new();
    let loaded = mgr.load_from_m3u(&m3u_path).expect("load win m3u must succeed");
    assert_eq!(loaded, 1);
    assert_eq!(mgr.items[0].filename, "Windows Track");
    assert_eq!(mgr.items[0].path, PathBuf::from("C:/Users/Music/My Song File.mp3"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_media_info_report_includes_file_size_and_boost() {
    use syncverse_ui::gui::aspect_ratio::{AspectRatioOverride, ZoomMode};
    use syncverse_ui::gui::dialogs::VideoStreamMetadata;
    use syncverse_ui::gui::media_info::{MediaInfoContext, MediaInfoDialog};
    use syncverse_ui::snapshot::PlayerUiSnapshot;

    let dir = std::env::temp_dir().join(format!("syncverse_diag_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let test_file = dir.join("sample.mp4");
    std::fs::write(&test_file, vec![0u8; 2048]).unwrap();

    let meta = VideoStreamMetadata {
        width: Some(1920),
        height: Some(1080),
        codec: Some("h264".to_string()),
        frame_rate: Some(60.0),
        bit_depth: Some(8),
        pixel_format: Some("yuv420p".to_string()),
        timescale: Some(1000),
    };
    let snap = PlayerUiSnapshot {
        media_position_pts: 10_000_000_000,
        media_duration_pts: 60_000_000_000,
        network_rtt_nanos: 15_000_000,
        clock_offset_nanos: 2_500_000,
        phase_error_nanos: 500_000,
        ..Default::default()
    };
    let ctx = MediaInfoContext {
        current_file: Some(&test_file),
        video_meta: &meta,
        audio_tracks: &[],
        active_audio_track: None,
        audio_delay_ms: 50,
        subtitle_delay_ms: -100,
        aspect_override: AspectRatioOverride::Ratio16_9,
        zoom_mode: ZoomMode::Fit,
        volume: 1.5, // 150% Boost
        snapshot: &snap,
    };

    let report = MediaInfoDialog::build_diagnostics_report(&ctx);
    assert!(report.contains("File Size:"));
    assert!(report.contains("2.0 KB (2048 bytes)"));
    assert!(report.contains("Volume:** 150% [Boost]"));
    assert!(report.contains("16:9"));
    assert!(report.contains("Audio Delay:** +50 ms"));
    assert!(report.contains("Subtitle Delay:** -100 ms"));

    let _ = std::fs::remove_dir_all(&dir);
}

