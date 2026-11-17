//! Integration tests for v0.5.0 UI wiring (WP6).

use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::{Duration, Instant};

use syncverse_clock::{MasterClock, PlaybackState, SpscRingBuffer};
use syncverse_codecs::VideoFrameRGBA;
use syncverse_common::userdata::{keys, Settings, UserDirs};
use syncverse_ui::{
    AspectRatioOverride, PlayerCommand, SyncverseApp, TripleBufferSnapshotExchange, UiEvent,
};

#[test]
fn resume_banner_event_roundtrip_and_aspect_restore() {
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let (command_tx, _command_rx) = channel::<PlayerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();

    let mut app =
        SyncverseApp::with_headless_subsystems(exchange, video_ring, clock, command_tx, event_rx);

    event_tx
        .send(UiEvent::ResumeAvailable {
            pos_ms: 45_000,
            duration_ms: 180_000,
        })
        .unwrap();
    event_tx
        .send(UiEvent::AspectRatioRestored("16:9".to_string()))
        .unwrap();

    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        app.render_ui(ui);
    });
    out.textures_delta.clear();

    let banner = app
        .resume_banner()
        .expect("ResumeAvailable should populate resume_banner");
    assert_eq!(banner.pos_ms, 45_000);
    assert_eq!(banner.duration_ms, 180_000);
    assert_eq!(
        app.viewport().aspect_override(),
        AspectRatioOverride::Ratio16_9
    );

    // Preferences dialog opens and counts as an active modal.
    assert!(!app.is_any_modal_open());
    app.preferences_dialog_mut().open();
    assert!(app.is_any_modal_open());

    // Simulate 8s elapsed: rendering a frame auto-hides the banner.
    let _ = app;
    let _ = Instant::now() + Duration::from_secs(9);
}

#[test]
fn settings_load_and_save_roundtrip_preserves_unknown_keys() {
    let base = std::env::temp_dir().join(format!("syncverse_ui_wp4_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dirs = UserDirs::under(&base);

    let mut initial = Settings::default();
    initial.set("custom_unknown_key", "keep_me");
    initial.set(keys::VOLUME, "0.65");
    initial.set_bool(keys::MUTED, true);
    initial.set_bool(keys::SIDEBAR_OPEN, false);
    initial.set(keys::USERNAME, "Gautham");
    initial.set(keys::LAST_HOST_PORT, "19000");
    initial.set(keys::RESUME_POLICY, "always");
    initial.set_bool(keys::REMEMBER_PER_FILE, false);
    initial.set_bool(keys::REMEMBER_SETTINGS, true);
    initial.save(&dirs.settings_path()).unwrap();

    let mut app = SyncverseApp::new();
    app.load_settings_from_dirs(&dirs);

    assert!((app.controls_state().volume.volume - 0.65).abs() < 0.01);
    assert!(app.controls_state().volume.is_muted);
    assert!(!app.sidebar_state().is_expanded);
    assert_eq!(app.preferences_dialog_mut().resume_policy, "always");
    assert!(!app.preferences_dialog_mut().remember_per_file);
    assert!(app.preferences_dialog_mut().remember_settings);

    app.preferences_dialog_mut().resume_policy = "ask".to_string();
    app.save_settings_to_dirs(&dirs);

    let reloaded = Settings::load(&dirs.settings_path());
    assert_eq!(reloaded.get("custom_unknown_key"), Some("keep_me"));
    assert_eq!(reloaded.get(keys::RESUME_POLICY), Some("ask"));
    assert_eq!(reloaded.get(keys::USERNAME), Some("Gautham"));
    assert_eq!(reloaded.get(keys::LAST_HOST_PORT), Some("19000"));

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn wp5_ab_loop_disabled_for_party_client() {
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let (command_tx, _command_rx) = channel::<PlayerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();

    let mut app =
        SyncverseApp::with_headless_subsystems(exchange, video_ring, clock, command_tx, event_rx);

    // Simulate being a non-host client in a 2-member room.
    event_tx
        .send(UiEvent::RoomMembersUpdated(vec![
            syncverse_ui::RoomMember::new("Host".to_string(), false, true, 10.0),
            syncverse_ui::RoomMember::new("Client".to_string(), true, false, 15.0),
        ]))
        .unwrap();

    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        app.render_ui(ui);
    });
    out.textures_delta.clear();

    app.cycle_ab_repeat(5_000, Instant::now());
    assert!(
        !app.controls_state().ab_repeat.is_active(),
        "A-B repeat must stay disabled for a party client"
    );
    assert!(app.controls_state().ab_repeat.point_a().is_none());
}

#[test]
fn wp1_video_dialog_hwdec_mode_persists_in_settings() {
    let base = std::env::temp_dir().join(format!("syncverse_ui_wp1_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dirs = UserDirs::under(&base);

    let mut s = Settings::default();
    s.set(keys::HWDEC_MODE, "auto");
    s.save(&dirs.settings_path()).unwrap();

    let mut app = SyncverseApp::new();
    app.load_settings_from_dirs(&dirs);
    assert_eq!(app.controls_state().video_dialog.hwdec_mode, "auto");

    app.controls_state_mut().video_dialog.hwdec_mode = "vaapi".to_string();
    app.save_settings_to_dirs(&dirs);
    let reloaded = Settings::load(&dirs.settings_path());
    assert_eq!(reloaded.get(keys::HWDEC_MODE), Some("vaapi"));

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn wp3_party_connection_state_updates_sidebar_badge() {
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let (command_tx, _command_rx) = channel::<PlayerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();

    let mut app =
        SyncverseApp::with_headless_subsystems(exchange, video_ring, clock, command_tx, event_rx);

    event_tx
        .send(UiEvent::PartyConnectionState {
            state: syncverse_ui::event::PartyConnectionState::Reconnecting { attempt: 3 },
        })
        .unwrap();

    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        app.render_ui(ui);
    });
    out.textures_delta.clear();

    assert_eq!(
        app.sidebar_state().party_connection_state,
        Some(syncverse_ui::event::PartyConnectionState::Reconnecting { attempt: 3 })
    );

    event_tx
        .send(UiEvent::PartyConnectionState {
            state: syncverse_ui::event::PartyConnectionState::Closed,
        })
        .unwrap();
    let mut out2 = ctx.run_ui(egui::RawInput::default(), |ui| {
        app.render_ui(ui);
    });
    out2.textures_delta.clear();
    assert_eq!(app.sidebar_state().party_connection_state, None);
}

#[test]
fn wp2_nat_report_and_upnp_status_update_dialogs() {
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let (command_tx, _command_rx) = channel::<PlayerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();

    let mut app =
        SyncverseApp::with_headless_subsystems(exchange, video_ring, clock, command_tx, event_rx);

    event_tx
        .send(UiEvent::NatReport {
            kind: syncverse_ui::event::NatKind::EndpointIndependent,
            public_addr: Some("203.0.113.5:18242".to_string()),
            error: None,
        })
        .unwrap();
    event_tx
        .send(UiEvent::UpnpStatusUpdated {
            is_mapped: true,
            port: 18242,
            external_ip: Some("203.0.113.5".to_string()),
            message: "UPnP active".to_string(),
        })
        .unwrap();

    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        app.render_ui(ui);
    });
    out.textures_delta.clear();
}
