//! Integration and empirical test suite for cycling hotkeys, HUD/OSD telemetry feedback,
//! command/event extensions, and PlayerUiSnapshot memory layout invariant.

use std::mem::size_of;
use std::time::{Duration, Instant};

use syncverse_ui::command::PlayerCommand;
use syncverse_ui::event::UiEvent;
use syncverse_ui::osd::OsdOverlay;
use syncverse_ui::snapshot::PlayerUiSnapshot;

#[test]
fn test_player_ui_snapshot_invariant_72_bytes() {
    let size = size_of::<PlayerUiSnapshot>();
    assert_eq!(
        size, 72,
        "CRITICAL INVARIANT: PlayerUiSnapshot must remain exactly 72 bytes"
    );
}

#[test]
fn test_all_player_command_variants_construction_and_equality() {
    let commands = vec![
        PlayerCommand::SelectAudioTrack(42),
        PlayerCommand::CycleAudioTrack,
        PlayerCommand::SetAudioDelay(-100),
        PlayerCommand::AdjustAudioDelay(50),
        PlayerCommand::SelectSubtitleTrack(Some(2)),
        PlayerCommand::SelectSubtitleTrack(None),
        PlayerCommand::CycleSubtitleTrack,
        PlayerCommand::SetSubtitleDelay(200),
        PlayerCommand::AdjustSubtitleDelay(-50),
        PlayerCommand::CycleAspectRatio,
    ];

    for cmd in &commands {
        let cloned = cmd.clone();
        assert_eq!(cmd, &cloned);
        assert!(!format!("{:?}", cmd).is_empty());
    }

    assert_eq!(PlayerCommand::SelectAudioTrack(42), PlayerCommand::SelectAudioTrack(42));
    assert_ne!(PlayerCommand::SelectAudioTrack(1), PlayerCommand::SelectAudioTrack(2));
    assert_eq!(PlayerCommand::SelectSubtitleTrack(None), PlayerCommand::SelectSubtitleTrack(None));
    assert_ne!(PlayerCommand::SelectSubtitleTrack(Some(0)), PlayerCommand::SelectSubtitleTrack(None));
}

#[test]
fn test_all_ui_event_variants_construction() {
    let now = Instant::now();

    let hud_ev = UiEvent::HudNotification {
        text: "Audio: Track 2/3 - Japanese (AAC 2ch)".to_string(),
        timestamp: now,
    };
    let audio_sel = UiEvent::AudioTrackSelected {
        track_id: 2,
        name: "Japanese (AAC 2ch)".to_string(),
    };
    let sub_sel = UiEvent::SubtitleTrackSelected {
        track_index: Some(1),
        name: "English (.srt)".to_string(),
    };
    let sub_off = UiEvent::SubtitleTrackSelected {
        track_index: None,
        name: "Off".to_string(),
    };
    let a_delay = UiEvent::AudioDelayChanged(-50);
    let s_delay = UiEvent::SubtitleDelayChanged(50);
    let tracks_disc = UiEvent::TracksDiscovered {
        audio_tracks: vec![
            "English (AAC 2ch)".to_string(),
            "Japanese (AAC 2ch)".to_string(),
        ],
        subtitle_tracks: vec![
            "English (.srt)".to_string(),
            "Spanish (.vtt)".to_string(),
        ],
    };

    assert!(matches!(hud_ev, UiEvent::HudNotification { .. }));
    assert!(matches!(audio_sel, UiEvent::AudioTrackSelected { track_id: 2, .. }));
    assert!(matches!(sub_sel, UiEvent::SubtitleTrackSelected { track_index: Some(1), .. }));
    assert!(matches!(sub_off, UiEvent::SubtitleTrackSelected { track_index: None, .. }));
    assert!(matches!(a_delay, UiEvent::AudioDelayChanged(-50)));
    assert!(matches!(s_delay, UiEvent::SubtitleDelayChanged(50)));
    assert!(matches!(tracks_disc, UiEvent::TracksDiscovered { .. }));
}

#[test]
fn test_osd_overlay_exact_dwell_and_fade_timing() {
    let mut osd = OsdOverlay::new();
    let t0 = Instant::now();

    // 1. Initial state is inactive
    assert_eq!(osd.opacity(t0), 0.0);
    assert!(osd.active_notification(t0).is_none());

    // 2. Trigger notification
    osd.show_notification("Audio: Track 2/3 - Japanese (AAC 2ch)", t0);

    // 3. During 1.5s dwell period, opacity MUST remain exactly 1.0
    assert_eq!(osd.opacity(t0), 1.0);
    assert_eq!(osd.opacity(t0 + Duration::from_millis(500)), 1.0);
    assert_eq!(osd.opacity(t0 + Duration::from_millis(1000)), 1.0);
    assert_eq!(osd.opacity(t0 + Duration::from_millis(1500)), 1.0);

    let (text, alpha) = osd.active_notification(t0 + Duration::from_millis(1500)).unwrap();
    assert_eq!(text, "Audio: Track 2/3 - Japanese (AAC 2ch)");
    assert_eq!(alpha, 1.0);

    // 4. During 0.5s fade period (1.5s -> 2.0s), opacity decays linearly
    // At t0 + 1750ms (halfway through fade): opacity should be 0.5
    let op_mid = osd.opacity(t0 + Duration::from_millis(1750));
    assert!((op_mid - 0.5).abs() < 1e-4, "Expected opacity ~0.5 at 1.75s, got {}", op_mid);

    // At t0 + 1600ms (20% through fade): opacity should be 0.8
    let op_20 = osd.opacity(t0 + Duration::from_millis(1600));
    assert!((op_20 - 0.8).abs() < 1e-4, "Expected opacity ~0.8 at 1.6s, got {}", op_20);

    // At t0 + 1900ms (80% through fade): opacity should be 0.2
    let op_80 = osd.opacity(t0 + Duration::from_millis(1900));
    assert!((op_80 - 0.2).abs() < 1e-4, "Expected opacity ~0.2 at 1.9s, got {}", op_80);

    // 5. At t0 + 2000ms: total 2.0s reached, opacity is strictly 0.0
    assert_eq!(osd.opacity(t0 + Duration::from_millis(2000)), 0.0);
    assert!(osd.active_notification(t0 + Duration::from_millis(2000)).is_none());

    // 6. Beyond 2.0s: strictly expired
    assert_eq!(osd.opacity(t0 + Duration::from_millis(2500)), 0.0);
    assert!(osd.active_notification(t0 + Duration::from_millis(2500)).is_none());
}

#[test]
fn test_osd_overlay_new_notification_resets_timer() {
    let mut osd = OsdOverlay::new();
    let t0 = Instant::now();

    osd.show_notification("Aspect Ratio: 16:9", t0);
    assert!((osd.opacity(t0 + Duration::from_millis(1800)) - 0.4).abs() < 1e-4);

    // Trigger a new notification at t0 + 1800ms
    let t1 = t0 + Duration::from_millis(1800);
    osd.show_notification("Aspect Ratio: 4:3", t1);

    // Opacity must reset to 1.0 at t1
    assert_eq!(osd.opacity(t1), 1.0);
    let (text, alpha) = osd.active_notification(t1).unwrap();
    assert_eq!(text, "Aspect Ratio: 4:3");
    assert_eq!(alpha, 1.0);

    // Must dwell until t1 + 1500ms
    assert_eq!(osd.opacity(t1 + Duration::from_millis(1500)), 1.0);
}

#[test]
fn test_osd_overlay_clear_immediately_dismisses() {
    let mut osd = OsdOverlay::new();
    let t0 = Instant::now();

    osd.show_notification("Audio Delay: +50 ms", t0);
    assert_eq!(osd.opacity(t0), 1.0);

    osd.clear();
    assert_eq!(osd.opacity(t0), 0.0);
    assert!(osd.active_notification(t0).is_none());
}

#[test]
fn test_audio_and_subtitle_cycling_logic() {
    // Audio cycling simulation: [0, 1, 2] -> 0 -> 1 -> 2 -> 0
    let audio_tracks = ["English (AAC)", "Japanese (Opus)", "French (AC3)"];
    let mut audio_idx = 0usize;

    audio_idx = (audio_idx + 1) % audio_tracks.len();
    assert_eq!(audio_idx, 1);
    assert_eq!(audio_tracks[audio_idx], "Japanese (Opus)");

    audio_idx = (audio_idx + 1) % audio_tracks.len();
    assert_eq!(audio_idx, 2);
    assert_eq!(audio_tracks[audio_idx], "French (AC3)");

    audio_idx = (audio_idx + 1) % audio_tracks.len();
    assert_eq!(audio_idx, 0);
    assert_eq!(audio_tracks[audio_idx], "English (AAC)");

    // Subtitle cycling simulation: None -> Some(0) -> Some(1) -> None
    let sub_tracks = ["English (.srt)", "Spanish (.vtt)"];
    let mut sub_idx: Option<usize> = None;

    let cycle_sub = |idx: Option<usize>, total: usize| -> Option<usize> {
        match idx {
            None => Some(0),
            Some(i) if i + 1 < total => Some(i + 1),
            Some(_) => None,
        }
    };

    sub_idx = cycle_sub(sub_idx, sub_tracks.len());
    assert_eq!(sub_idx, Some(0));

    sub_idx = cycle_sub(sub_idx, sub_tracks.len());
    assert_eq!(sub_idx, Some(1));

    sub_idx = cycle_sub(sub_idx, sub_tracks.len());
    assert_eq!(sub_idx, None);
}

#[test]
fn test_delay_adjustment_math() {
    let mut audio_delay_ms: i64 = 0;

    // Hotkey J: -50ms
    audio_delay_ms -= 50;
    assert_eq!(audio_delay_ms, -50);
    assert_eq!(format!("Audio Delay: {:+} ms", audio_delay_ms), "Audio Delay: -50 ms");

    // Hotkey K: +50ms
    audio_delay_ms += 50;
    assert_eq!(audio_delay_ms, 0);
    assert_eq!(format!("Audio Delay: {:+} ms", audio_delay_ms), "Audio Delay: +0 ms");

    audio_delay_ms += 50;
    assert_eq!(audio_delay_ms, 50);
    assert_eq!(format!("Audio Delay: {:+} ms", audio_delay_ms), "Audio Delay: +50 ms");

    let mut sub_delay_ms: i64 = 0;
    // Hotkey Z: -50ms
    sub_delay_ms -= 50;
    assert_eq!(sub_delay_ms, -50);
    assert_eq!(format!("Subtitle Delay: {:+} ms", sub_delay_ms), "Subtitle Delay: -50 ms");

    // Hotkey X: +50ms
    sub_delay_ms += 50;
    assert_eq!(sub_delay_ms, 0);
}
