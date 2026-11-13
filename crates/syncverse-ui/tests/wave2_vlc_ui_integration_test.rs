//! Comprehensive Wave 2 headless integration test suite for `SyncverseApp`, `VideoViewport`,
//! subtitle timing/bitmap rendering, VLC hotkeys, and multi-resolution / multi-DPI layout.

#![deny(warnings)]

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{vec2, Context, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect};
use syncverse_clock::{MasterClock, PlaybackState, SpscRingBuffer};
use syncverse_codecs::VideoFrameRGBA;
use syncverse_ui::command::PlayerCommand;
use syncverse_ui::event::{SubtitleBitmapCue, TrackSubtitleEvent, UiEvent};
use syncverse_ui::gui::app::SyncverseApp;
use syncverse_ui::gui::aspect_ratio::ZoomMode;
use syncverse_ui::gui::dialogs::AudioTrackItem;
use syncverse_ui::gui::subtitle_overlay::{SubtitleCue, SubtitleTrack};
use syncverse_ui::gui::viewport::VideoViewport;
use syncverse_ui::snapshot::{
    PlayerUiSnapshot, TripleBufferSnapshotExchange, PLAYBACK_STATE_PAUSED, PLAYBACK_STATE_PLAYING,
};

struct HeadlessHarness {
    app: SyncverseApp,
    exchange: Arc<TripleBufferSnapshotExchange>,
    video_ring: Arc<SpscRingBuffer<VideoFrameRGBA, 32>>,
    _clock: Arc<MasterClock>,
    command_rx: mpsc::Receiver<PlayerCommand>,
    event_tx: mpsc::Sender<UiEvent>,
}

impl HeadlessHarness {
    fn new() -> Self {
        let exchange = Arc::new(TripleBufferSnapshotExchange::new());
        let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
        let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let app = SyncverseApp::with_headless_subsystems(
            Arc::clone(&exchange),
            Arc::clone(&video_ring),
            Arc::clone(&clock),
            command_tx,
            event_rx,
        );
        Self {
            app,
            exchange,
            video_ring,
            _clock: clock,
            command_rx,
            event_tx,
        }
    }

    fn drain_commands(&self) -> Vec<PlayerCommand> {
        let mut cmds = Vec::new();
        while let Ok(cmd) = self.command_rx.try_recv() {
            cmds.push(cmd);
        }
        cmds
    }
}

fn key_press_event(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers,
    }
}

struct CapturedFrame {
    shapes: Vec<egui::epaint::ClippedShape>,
    textures_set_len: usize,
    repaint_delay: Duration,
    sidebar_rect: Option<Rect>,
    controls_rect: Option<Rect>,
    top_bar_rect: Option<Rect>,
}

fn capture_full_output(
    mut output: egui::FullOutput,
    sidebar_rect: Option<Rect>,
    controls_rect: Option<Rect>,
    top_bar_rect: Option<Rect>,
) -> CapturedFrame {
    let textures_set_len = output.textures_delta.set.len();
    let repaint_delay = output
        .viewport_output
        .values()
        .next()
        .map(|v| v.repaint_delay)
        .unwrap_or(Duration::MAX);
    // Clear unapplied texture deltas before dropping FullOutput in headless mode.
    output.textures_delta.clear();
    CapturedFrame {
        shapes: output.shapes,
        textures_set_len,
        repaint_delay,
        sidebar_rect,
        controls_rect,
        top_bar_rect,
    }
}

fn find_text_rect_in_shapes(shapes: &[egui::epaint::ClippedShape], needle: &str) -> Option<Rect> {
    fn search_shape(shape: &egui::Shape, needle: &str, exact: bool) -> Option<Rect> {
        match shape {
            egui::Shape::Text(text_shape) => {
                let txt = text_shape.galley.text();
                let matched = if exact {
                    txt == needle
                } else {
                    txt.contains(needle)
                };
                if matched {
                    Some(text_shape.visual_bounding_rect())
                } else {
                    None
                }
            }
            egui::Shape::Vec(vec) => {
                for s in vec {
                    if let Some(r) = search_shape(s, needle, exact) {
                        return Some(r);
                    }
                }
                None
            }
            _ => None,
        }
    }

    for exact in [true, false] {
        for clipped in shapes {
            if let Some(r) = search_shape(&clipped.shape, needle, exact) {
                return Some(r);
            }
        }
    }
    None
}

fn run_app_frame_with_rects(
    ctx: &Context,
    app: &mut SyncverseApp,
    width: f32,
    height: f32,
    dpi: f32,
    mut events: Vec<Event>,
    now: Instant,
) -> CapturedFrame {
    ctx.set_pixels_per_point(dpi);
    let mut active_modifiers = None;
    for ev in &events {
        if let Event::Key { modifiers, .. } = ev {
            active_modifiers = Some(*modifiers);
        }
    }
    if let Some(mods) = active_modifiers {
        events.insert(0, Event::ModifiersChanged(mods));
    }
    let raw_input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, height))),
        events,
        ..Default::default()
    };
    let mut sidebar_rect = None;
    let mut controls_rect = None;
    let mut top_bar_rect = None;
    let output = ctx.run_ui(raw_input, |ui| {
        let base_id = ui.scope_id();
        app.render_ui_at(ui, now);
        sidebar_rect = egui::PanelState::load(
            ui.ctx(),
            base_id.with_salt(egui::IdSalt::new("watch_party_sidebar")),
        )
        .map(|s| s.outer_rect);
        controls_rect = egui::PanelState::load(
            ui.ctx(),
            base_id.with_salt(egui::IdSalt::new("playback_controls_bar")),
        )
        .map(|s| s.outer_rect);
        top_bar_rect = egui::PanelState::load(
            ui.ctx(),
            base_id.with_salt(egui::IdSalt::new("top_menu_bar")),
        )
        .map(|s| s.outer_rect);
    });
    capture_full_output(output, sidebar_rect, controls_rect, top_bar_rect)
}

fn run_app_frame(
    ctx: &Context,
    app: &mut SyncverseApp,
    width: f32,
    height: f32,
    dpi: f32,
    events: Vec<Event>,
    now: Instant,
) -> CapturedFrame {
    run_app_frame_with_rects(ctx, app, width, height, dpi, events, now)
}

fn click_pos(
    ctx: &Context,
    app: &mut SyncverseApp,
    width: f32,
    height: f32,
    pos: Pos2,
    now: &mut Instant,
) -> CapturedFrame {
    let _ = run_app_frame(
        ctx,
        app,
        width,
        height,
        1.0,
        vec![Event::PointerMoved(pos)],
        *now,
    );
    *now += Duration::from_millis(20);
    let _ = run_app_frame(
        ctx,
        app,
        width,
        height,
        1.0,
        vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }],
        *now,
    );
    *now += Duration::from_millis(20);
    let _ = run_app_frame(
        ctx,
        app,
        width,
        height,
        1.0,
        vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
        *now,
    );
    *now += Duration::from_millis(20);
    let out = run_app_frame(ctx, app, width, height, 1.0, Vec::new(), *now);
    *now += Duration::from_millis(20);
    out
}

#[test]
fn test_multi_resolution_and_dpi_layout_panel_docking_and_truncation() {
    let resolutions = [
        (480.0_f32, 320.0_f32),
        (640.0, 360.0),
        (930.0, 720.0),
        (1280.0, 720.0),
        (1920.0, 1080.0),
    ];
    let dpis = [1.0_f32, 2.0_f32];
    let long_filename = "extremely_long_movie_release_filename_2026_2160p_uhd_bluray_remux_hdr10_x265_truehd_atmos_7_1_syncverse_edition.mkv";

    for &(w, h) in &resolutions {
        for &dpi in &dpis {
            let mut harn = HeadlessHarness::new();
            harn.app
                .set_current_file(Some(PathBuf::from(format!("/media/{}", long_filename))));
            harn.app.sidebar_state_mut().is_expanded = true;

            // Populate long chat entries and long peer filename to stress sidebar clipping.
            for idx in 0..8 {
                harn.app.sidebar_state_mut().pending_local_records.push(
                    syncverse_chat::ChatRecord::new_text(
                        idx as u64 * 1000,
                        idx as u64 * 1000,
                        [idx as u8; 16],
                        format!("VeryLongPeerUsernameNumber{}", idx),
                        &format!(
                            "Long chat message #{} testing vertical scroll area bounds without overflowing footer input bar!",
                            idx
                        ),
                    ),
                );
            }

            harn.exchange.publish(PlayerUiSnapshot {
                media_position_pts: 65_000_000_000,
                media_duration_pts: 3_600_000_000_000,
                playback_state: PLAYBACK_STATE_PLAYING,
                peer_count: 2,
                peers_ready: 2,
                ..Default::default()
            });

            let mut frame = VideoFrameRGBA::new(320, 180, 65_000);
            frame.generation = 0;
            let _ = harn.video_ring.try_push(frame);

            let ctx = Context::default();
            let now = Instant::now();
            // Run 3 frames so egui panel sizing converges cleanly.
            let mut last_res = None;
            for step in 0..3 {
                last_res = Some(run_app_frame_with_rects(
                    &ctx,
                    &mut harn.app,
                    w,
                    h,
                    dpi,
                    Vec::new(),
                    now + Duration::from_millis(step * 16),
                ));
            }
            let frame_out = last_res.expect("frame output");
            assert!(
                !frame_out.shapes.is_empty(),
                "Expected rendered shapes at {}x{} @ {}x DPI",
                w,
                h,
                dpi
            );

            let sidebar_rect = frame_out.sidebar_rect.unwrap_or_else(|| {
                panic!(
                    "watch_party_sidebar PanelState missing at {}x{} @ {}x",
                    w, h, dpi
                )
            });
            let controls_rect = frame_out.controls_rect.unwrap_or_else(|| {
                panic!(
                    "playback_controls_bar PanelState missing at {}x{} @ {}x",
                    w, h, dpi
                )
            });

            // Sidebar is docked before controls bar, so controls bar ends at sidebar's left edge.
            assert!(
                controls_rect.right() <= sidebar_rect.left() + 1.0,
                "Bottom controls bar ({:?}) overlapped right sidebar ({:?}) at {}x{} @ {}x",
                controls_rect,
                sidebar_rect,
                w,
                h,
                dpi
            );

            // Sidebar spans down to the bottom of the screen.
            assert!(
                sidebar_rect.bottom() >= h - 1.5,
                "Sidebar ({:?}) did not span full height {} at {}x{} @ {}x",
                sidebar_rect,
                h,
                w,
                h,
                dpi
            );

            // Sidebar must stay within sane bounds so viewport isn't crushed.
            assert!(
                sidebar_rect.left() > 0.0 && sidebar_rect.width() <= w * 0.5 + 1.0,
                "Sidebar width ({:?}) unreasonable at {}x{} @ {}x",
                sidebar_rect,
                w,
                h,
                dpi
            );

            // Top menu bar must not wrap vertically or spill outside screen width.
            if let Some(top_bar_rect) = frame_out.top_bar_rect {
                assert!(
                    top_bar_rect.height() < 52.0,
                    "Top menu bar wrapped too tall ({:?}) at {}x{} @ {}x",
                    top_bar_rect,
                    w,
                    h,
                    dpi
                );
                assert!(
                    top_bar_rect.right() <= w + 1.0,
                    "Top menu bar overflowed width ({:?}) at {}x{} @ {}x",
                    top_bar_rect,
                    w,
                    h,
                    dpi
                );
            }

            // Video viewport rect (if present) must not overlap sidebar or controls bar.
            if let Some(vrect) = harn.app.viewport().last_video_rect() {
                assert!(
                    vrect.right() <= sidebar_rect.left() + 1.0,
                    "Video rect ({:?}) overlapped sidebar ({:?}) at {}x{}",
                    vrect,
                    sidebar_rect,
                    w,
                    h
                );
                assert!(
                    vrect.bottom() <= controls_rect.top() + 1.0,
                    "Video rect ({:?}) overlapped controls bar ({:?}) at {}x{}",
                    vrect,
                    controls_rect,
                    w,
                    h
                );
            }
        }
    }
}

#[test]
fn test_viewport_epoch_filtering_no_clock_mutation_fit_click_and_idle_repaint() {
    let ctx = Context::default();
    let now = Instant::now();

    // 1. Epoch filtering: stale generation=0 discarded after seek_to (generation=1).
    {
        let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Playing));
        let ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
        let mut vp = VideoViewport::new();

        clock.seek_to(5000.0, now);
        assert_eq!(clock.seek_generation(), 1);

        // Stale frames from generation 0.
        let _ = ring.try_push(VideoFrameRGBA::new(64, 64, 1000).with_generation(0));
        let _ = ring.try_push(VideoFrameRGBA::new(64, 64, 5000).with_generation(0));
        // Fresh frame from generation 1.
        let _ = ring.try_push(VideoFrameRGBA::new(128, 72, 5000).with_generation(1));

        let out1 = capture_full_output(
            ctx.run_ui(RawInput::default(), |ui| {
                vp.update_playback(ui.ctx(), &ring, &clock, now);
            }),
            None,
            None,
            None,
        );
        let _ = out1;

        assert_eq!(
            clock.seek_generation(),
            1,
            "update_playback must never mutate clock.seek_generation()"
        );
        let captured = vp
            .capture_frame()
            .expect("Expected generation-1 frame to be presented");
        assert_eq!(captured.generation, 1);
        assert_eq!(captured.width, 128);
        assert_eq!(captured.height, 72);
        assert_eq!(captured.pts_ms, 5000);
    }

    // 2. First frame presentation must NOT mutate clock via seek_to.
    {
        let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
        let ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
        let mut vp = VideoViewport::new();

        let _ = ring.try_push(VideoFrameRGBA::new(160, 90, 2500).with_generation(0));
        let _out = capture_full_output(
            ctx.run_ui(RawInput::default(), |ui| {
                vp.update_playback(ui.ctx(), &ring, &clock, now);
            }),
            None,
            None,
            None,
        );

        assert_eq!(clock.seek_generation(), 0);
        assert_eq!(clock.current_pts(now), 0.0);
        assert_eq!(vp.capture_frame().map(|f| f.pts_ms), Some(2500));

        // 3. Paused with empty ring must NOT schedule busy 8ms/16ms timer repaints
        // (after egui's initial context frames converge).
        for _ in 0..3 {
            let _ = capture_full_output(
                ctx.run_ui(RawInput::default(), |ui| {
                    vp.update_playback(ui.ctx(), &ring, &clock, now);
                }),
                None,
                None,
                None,
            );
        }
        let idle_out = capture_full_output(
            ctx.run_ui(RawInput::default(), |ui| {
                vp.update_playback(ui.ctx(), &ring, &clock, now);
            }),
            None,
            None,
            None,
        );
        assert!(
            idle_out.repaint_delay >= Duration::from_secs(1),
            "Expected idle paused viewport not to schedule busy repaint, got {:?}",
            idle_out.repaint_delay
        );
    }

    // 4. ZoomMode::Fit must not swallow single-clicks (even with small pointer jitter).
    {
        let fit_ctx = Context::default();
        let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
        let ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
        let mut vp = VideoViewport::new();
        vp.set_zoom_mode(ZoomMode::Fit);
        let _ = ring.try_push(VideoFrameRGBA::new(320, 180, 0).with_generation(0));

        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0));
        // Frame 1: upload frame & establish viewport rect.
        let _out = capture_full_output(
            fit_ctx.run_ui(
                RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    vp.update_playback(ui.ctx(), &ring, &clock, now);
                    let _ = vp.show_with_context_menu(ui, |_| {});
                },
            ),
            None,
            None,
            None,
        );

        // Frame 2: pointer down at (640, 360) with tiny jitter + pointer up at (642, 361).
        let _out = capture_full_output(
            fit_ctx.run_ui(
                RawInput {
                    screen_rect: Some(screen),
                    events: vec![
                        Event::PointerMoved(Pos2::new(640.0, 360.0)),
                        Event::PointerButton {
                            pos: Pos2::new(640.0, 360.0),
                            button: PointerButton::Primary,
                            pressed: true,
                            modifiers: Modifiers::NONE,
                        },
                        Event::PointerMoved(Pos2::new(642.0, 361.0)),
                        Event::PointerButton {
                            pos: Pos2::new(642.0, 361.0),
                            button: PointerButton::Primary,
                            pressed: false,
                            modifiers: Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ui| {
                    let _ = vp.show_with_context_menu(ui, |_| {});
                },
            ),
            None,
            None,
            None,
        );

        // Wait past 250ms double-click disambiguation threshold used inside show_with_context_menu.
        std::thread::sleep(Duration::from_millis(270));

        let mut action_after = None;
        let _out = capture_full_output(
            fit_ctx.run_ui(
                RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    action_after = Some(vp.show_with_context_menu(ui, |_| {}));
                },
            ),
            None,
            None,
            None,
        );
        assert!(
            action_after.expect("action").toggle_play_pause,
            "ZoomMode::Fit should trigger play/pause click rather than swallowing as pan drag"
        );
    }
}

#[test]
fn test_subtitle_millisecond_accuracy_dedup_clear_active_and_bitmap_rendering() {
    let mut harn = HeadlessHarness::new();
    let ctx = Context::default();
    let now = Instant::now();

    // 1. Millisecond accuracy: cue at [5_000_000 ms, 5_005_000 ms] must NOT match pts = 5 ms.
    let far_cue = SubtitleCue::new(5_000_000, 5_005_000, "Far future cue");
    harn.app.add_subtitle_track(SubtitleTrack::new(
        0,
        "Track 0",
        Some("eng".to_string()),
        vec![far_cue],
    ));
    harn.app.select_subtitle_track(Some(0));
    assert_eq!(
        harn.app.current_active_subtitle(5),
        None,
        "5ms must not match a cue at 5,000,000ms"
    );
    assert_eq!(
        harn.app.current_active_subtitle(5_000_000),
        Some("Far future cue".to_string())
    );

    // 2. UiEvent::SubtitleCueDemuxed deduplication & empty-text ClearActive sentinel truncation.
    harn.event_tx
        .send(UiEvent::SubtitleDataDiscovered(vec![SubtitleTrack::new(
            0,
            "Demuxed ASS",
            Some("eng".to_string()),
            vec![],
        )]))
        .unwrap();
    harn.event_tx
        .send(UiEvent::SubtitleTrackSelected {
            track_index: Some(0),
            name: "Demuxed ASS".to_string(),
        })
        .unwrap();
    harn.event_tx
        .send(UiEvent::SubtitleCueDemuxed(SubtitleCue::new(
            1000, 5000, "Hello",
        )))
        .unwrap();
    // Duplicate cue from backward seek replay.
    harn.event_tx
        .send(UiEvent::SubtitleCueDemuxed(SubtitleCue::new(
            1000, 5000, "Hello",
        )))
        .unwrap();
    // Empty-text ClearActive sentinel at 3000ms.
    harn.event_tx
        .send(UiEvent::SubtitleCueDemuxed(SubtitleCue::new(
            3000, 3000, "",
        )))
        .unwrap();

    // Publish 5ms snapshot (5_000_000 ns) so far_cue wouldn't render in UI either, then process events.
    harn.exchange.publish(PlayerUiSnapshot {
        media_position_pts: 5_000_000,
        media_duration_pts: 60_000_000_000,
        playback_state: PLAYBACK_STATE_PAUSED,
        ..Default::default()
    });
    let _ = run_app_frame(&ctx, &mut harn.app, 1280.0, 720.0, 1.0, Vec::new(), now);

    assert_eq!(
        harn.app.current_active_subtitle(2000),
        Some("Hello".to_string()),
        "Duplicate demuxed cue should be deduplicated (not repeated)"
    );
    assert_eq!(
        harn.app.current_active_subtitle(3500),
        None,
        "Empty-text ClearActive sentinel at 3000ms should truncate active cue ending"
    );

    // 3. BitmapCue + ClearActive + rendering inside video_rect.
    let _ = harn
        .video_ring
        .try_push(VideoFrameRGBA::new(1280, 720, 2000));
    harn.exchange.publish(PlayerUiSnapshot {
        media_position_pts: 2_000_000_000, // 2000 ms
        media_duration_pts: 60_000_000_000,
        playback_state: PLAYBACK_STATE_PAUSED,
        ..Default::default()
    });
    harn.app.apply_track_subtitle_event(
        TrackSubtitleEvent::BitmapCue {
            track_index: 0,
            cue: SubtitleBitmapCue {
                start_ms: 1500,
                end_ms: 4000,
                x: 100,
                y: 500,
                width: 200,
                height: 40,
                canvas_width: 1280,
                canvas_height: 720,
                rgba: Arc::from(vec![255u8; 200 * 40 * 4]),
            },
        },
        now,
    );
    assert!(harn.app.current_active_bitmap_subtitle(2000).is_some());

    let out_with_bmp = run_app_frame(&ctx, &mut harn.app, 1280.0, 720.0, 1.0, Vec::new(), now);
    let vrect = harn
        .app
        .viewport()
        .last_video_rect()
        .expect("video_rect should be populated");
    // Expect at least 2 uploaded textures (video frame + bitmap subtitle overlay).
    assert!(
        out_with_bmp.textures_set_len >= 2,
        "Expected both video and bitmap subtitle textures to be uploaded, got {} sets",
        out_with_bmp.textures_set_len
    );
    assert!(vrect.width() > 0.0 && vrect.height() > 0.0);

    // ClearActive truncates bitmap cue at 2500ms.
    harn.app.apply_track_subtitle_event(
        TrackSubtitleEvent::ClearActive {
            track_index: 0,
            at_ms: 2500,
        },
        now,
    );
    assert!(harn.app.current_active_bitmap_subtitle(2000).is_some());
    assert!(harn.app.current_active_bitmap_subtitle(2600).is_none());

    harn.exchange.publish(PlayerUiSnapshot {
        media_position_pts: 2_600_000_000, // 2600 ms
        media_duration_pts: 60_000_000_000,
        playback_state: PLAYBACK_STATE_PAUSED,
        ..Default::default()
    });
    let out_cleared = run_app_frame(
        &ctx,
        &mut harn.app,
        1280.0,
        720.0,
        1.0,
        Vec::new(),
        now + Duration::from_millis(16),
    );
    // No new texture upload for bitmap cue once cleared.
    assert_eq!(out_cleared.textures_set_len, 0);
}

#[test]
fn test_audio_stream_id_menu_subtitle_menu_escape_hierarchy_and_vlc_hotkeys() {
    let mut harn = HeadlessHarness::new();
    let ctx = Context::default();
    let mut now = Instant::now();

    // Populate audio tracks with non-sequential stream IDs (menu index 0 -> stream id 3).
    harn.event_tx
        .send(UiEvent::TracksDiscovered {
            audio_tracks: vec![
                "Japanese Surround".to_string(),
                "Director Commentary".to_string(),
            ],
            subtitle_tracks: vec!["English Dialogue".to_string()],
        })
        .unwrap();
    harn.event_tx
        .send(UiEvent::AudioDataDiscovered(vec![
            AudioTrackItem::new(3, "Japanese Surround", "aac"),
            AudioTrackItem::new(7, "Director Commentary", "opus"),
        ]))
        .unwrap();
    harn.event_tx
        .send(UiEvent::SubtitleDataDiscovered(vec![SubtitleTrack::new(
            0,
            "English Dialogue",
            Some("eng".to_string()),
            vec![SubtitleCue::new(0, 5000, "Line 1")],
        )]))
        .unwrap();

    // Prime event_rx processing & render initial top menu bar.
    let out1 = run_app_frame(&ctx, &mut harn.app, 1280.0, 720.0, 1.0, Vec::new(), now);
    now += Duration::from_millis(20);

    // 1. Click Top Menu -> Audio -> Audio Track -> "Japanese Surround" (index 0, stream id 3).
    let audio_menu_pos = find_text_rect_in_shapes(&out1.shapes, "Audio")
        .expect("Top menu 'Audio' button should render")
        .center();
    let out2 = click_pos(&ctx, &mut harn.app, 1280.0, 720.0, audio_menu_pos, &mut now);

    let audio_track_sub = find_text_rect_in_shapes(&out2.shapes, "Audio Track")
        .expect("'Audio Track' submenu should appear")
        .center();
    let out3 = click_pos(&ctx, &mut harn.app, 1280.0, 720.0, audio_track_sub, &mut now);

    let track_zero_btn = find_text_rect_in_shapes(&out3.shapes, "Japanese Surround")
        .expect("'Japanese Surround' audio track item should appear")
        .center();
    let _ = click_pos(&ctx, &mut harn.app, 1280.0, 720.0, track_zero_btn, &mut now);

    let cmds = harn.drain_commands();
    assert!(
        cmds.contains(&PlayerCommand::SelectAudioTrack(3)),
        "Selecting track at menu index 0 must dispatch SelectAudioTrack(3), got {:?}",
        cmds
    );

    // 2. Click Top Menu -> Subtitle -> Subtitle Track -> "English Dialogue".
    let sub_menu_pos = find_text_rect_in_shapes(&out1.shapes, "Subtitle")
        .expect("Top menu 'Subtitle' button should render")
        .center();
    let out_sub1 = click_pos(&ctx, &mut harn.app, 1280.0, 720.0, sub_menu_pos, &mut now);

    let sub_track_menu = find_text_rect_in_shapes(&out_sub1.shapes, "Subtitle Track")
        .expect("'Subtitle Track' submenu should appear")
        .center();
    let out_sub2 = click_pos(&ctx, &mut harn.app, 1280.0, 720.0, sub_track_menu, &mut now);

    let sub_item_pos = find_text_rect_in_shapes(&out_sub2.shapes, "English Dialogue")
        .expect("'English Dialogue' subtitle item should appear")
        .center();
    let _ = click_pos(&ctx, &mut harn.app, 1280.0, 720.0, sub_item_pos, &mut now);

    let sub_cmds = harn.drain_commands();
    assert!(
        sub_cmds.contains(&PlayerCommand::SelectSubtitleTrack(Some(0))),
        "Selecting track in Top Menu must dispatch SelectSubtitleTrack(Some(0)), got {:?}",
        sub_cmds
    );
    assert_eq!(harn.app.subtitle_track_index(), Some(0));

    // 3. Escape hierarchy: dialogs close before exiting fullscreen.
    harn.app.controls_state_mut().is_fullscreen = true;
    harn.app.effects_dialog_mut().is_open = true;
    let _ = run_app_frame(
        &ctx,
        &mut harn.app,
        1280.0,
        720.0,
        1.0,
        vec![key_press_event(Key::Escape, Modifiers::NONE)],
        now,
    );
    now += Duration::from_millis(20);
    assert!(!harn.app.effects_dialog().is_open);
    assert!(
        harn.app.controls_state().is_fullscreen,
        "Escape closing effects_dialog must preserve is_fullscreen"
    );

    harn.app.jump_time_dialog_mut().is_open = true;
    let _ = run_app_frame(
        &ctx,
        &mut harn.app,
        1280.0,
        720.0,
        1.0,
        vec![key_press_event(Key::Escape, Modifiers::NONE)],
        now,
    );
    now += Duration::from_millis(20);
    assert!(!harn.app.jump_time_dialog().is_open);
    assert!(
        harn.app.controls_state().is_fullscreen,
        "Escape closing jump_time_dialog must preserve is_fullscreen"
    );

    // Escape with no modals open exits fullscreen.
    let _ = run_app_frame(
        &ctx,
        &mut harn.app,
        1280.0,
        720.0,
        1.0,
        vec![key_press_event(Key::Escape, Modifiers::NONE)],
        now,
    );
    now += Duration::from_millis(20);
    assert!(!harn.app.controls_state().is_fullscreen);

    // 4. VLC hotkeys: F (fullscreen), J/K/Shift+J (audio delay), G/H/Shift+G (subtitle delay).
    let _ = run_app_frame(
        &ctx,
        &mut harn.app,
        1280.0,
        720.0,
        1.0,
        vec![key_press_event(Key::F, Modifiers::NONE)],
        now,
    );
    now += Duration::from_millis(20);
    assert!(
        harn.app.controls_state().is_fullscreen,
        "Pressing F should toggle fullscreen on"
    );

    let _ = run_app_frame(
        &ctx,
        &mut harn.app,
        1280.0,
        720.0,
        1.0,
        vec![key_press_event(Key::F, Modifiers::NONE)],
        now,
    );
    now += Duration::from_millis(20);
    assert!(
        !harn.app.controls_state().is_fullscreen,
        "Pressing F again should toggle fullscreen off"
    );

    let _ = harn.drain_commands();

    // Audio delay: J (-50), K (+50 -> 0), K (+50), Shift+J (reset -> 0).
    for key_ev in [
        key_press_event(Key::J, Modifiers::NONE),
        key_press_event(Key::K, Modifiers::NONE),
        key_press_event(Key::K, Modifiers::NONE),
        key_press_event(Key::J, Modifiers::SHIFT),
    ] {
        let _ = run_app_frame(&ctx, &mut harn.app, 1280.0, 720.0, 1.0, vec![key_ev], now);
        now += Duration::from_millis(20);
    }
    assert_eq!(harn.app.audio_delay_ms(), 0);
    let audio_delay_cmds = harn.drain_commands();
    assert_eq!(
        audio_delay_cmds,
        vec![
            PlayerCommand::AdjustAudioDelay(-50),
            PlayerCommand::AdjustAudioDelay(50),
            PlayerCommand::AdjustAudioDelay(50),
            PlayerCommand::SetAudioDelay(0),
        ]
    );

    // Subtitle delay: G (-50), H (+50 -> 0), H (+50), Shift+G (reset -> 0).
    for key_ev in [
        key_press_event(Key::G, Modifiers::NONE),
        key_press_event(Key::H, Modifiers::NONE),
        key_press_event(Key::H, Modifiers::NONE),
        key_press_event(Key::G, Modifiers::SHIFT),
    ] {
        let _ = run_app_frame(&ctx, &mut harn.app, 1280.0, 720.0, 1.0, vec![key_ev], now);
        now += Duration::from_millis(20);
    }
    assert_eq!(harn.app.subtitle_delay_ms(), 0);
    let sub_delay_cmds = harn.drain_commands();
    assert_eq!(
        sub_delay_cmds,
        vec![
            PlayerCommand::AdjustSubtitleDelay(-50),
            PlayerCommand::AdjustSubtitleDelay(50),
            PlayerCommand::AdjustSubtitleDelay(50),
            PlayerCommand::SetSubtitleDelay(0),
        ]
    );
}

#[test]
fn test_pause_preserves_lookahead_and_paused_seek_same_pts_uploads() {
    let ctx = Context::default();
    let now = Instant::now();

    // Part (a): While playing at gen 0, pausing preserves all future lookahead frames.
    {
        let clock = Arc::new(MasterClock::new(1000.0, 1.0, PlaybackState::Playing));
        let ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
        let mut vp = VideoViewport::new();

        let _ = ring.try_push(VideoFrameRGBA::new(64, 64, 1000).with_generation(0));
        let _ = ring.try_push(VideoFrameRGBA::new(64, 64, 1040).with_generation(0));
        let _ = ring.try_push(VideoFrameRGBA::new(64, 64, 1080).with_generation(0));
        let _ = ring.try_push(VideoFrameRGBA::new(64, 64, 1120).with_generation(0));

        // Frame 1 (Playing): uploads pts=1000.
        let _ = capture_full_output(
            ctx.run_ui(RawInput::default(), |ui| {
                vp.update_playback(ui.ctx(), &ring, &clock, now);
            }),
            None,
            None,
            None,
        );
        // Frame 2 (Playing): pops pts=1040, sees Wait(40ms), stores it in pending_frame;
        // pts=1080 and pts=1120 remain untouched in ring (ring.len() == 2).
        let _ = capture_full_output(
            ctx.run_ui(RawInput::default(), |ui| {
                vp.update_playback(ui.ctx(), &ring, &clock, now);
            }),
            None,
            None,
            None,
        );
        assert_eq!(vp.capture_frame().map(|f| f.pts_ms), Some(1000));
        assert_eq!(ring.len(), 2);

        // Pause clock and run multiple paused frames: must NOT drain pending_frame or ring.
        clock.set_state(now, PlaybackState::Paused);
        for _ in 0..3 {
            let _ = capture_full_output(
                ctx.run_ui(RawInput::default(), |ui| {
                    vp.update_playback(ui.ctx(), &ring, &clock, now);
                }),
                None,
                None,
                None,
            );
        }
        assert_eq!(
            vp.capture_frame().map(|f| f.pts_ms),
            Some(1000),
            "Pausing must not jump ahead to future buffered frames"
        );
        assert_eq!(
            ring.len(),
            2,
            "Future gen=0 frames in video_ring must remain intact while paused"
        );

        // Unpause at `resume_start` (clock is still at 1000ms), then advance 40ms of playback:
        // pending_frame (pts=1040) is presented immediately in order.
        let resume_start = now + Duration::from_millis(40);
        clock.set_state(resume_start, PlaybackState::Playing);
        let present_time = resume_start + Duration::from_millis(40);
        let _ = capture_full_output(
            ctx.run_ui(RawInput::default(), |ui| {
                vp.update_playback(ui.ctx(), &ring, &clock, present_time);
            }),
            None,
            None,
            None,
        );
        assert_eq!(vp.capture_frame().map(|f| f.pts_ms), Some(1040));
        assert_eq!(ring.len(), 2);
    }

    // Part (b): Paused seek back to 0ms (same PTS as before, new generation=1) discards stale
    // gen=0 frames, uploads gen=1 pts=0ms, and preserves gen=1 pts=40ms in video_ring.
    {
        let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
        let ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
        let mut vp = VideoViewport::new();

        // Initial paused frame at pts=0, gen=0 (32x32).
        let _ = ring.try_push(VideoFrameRGBA::new(32, 32, 0).with_generation(0));
        let _ = capture_full_output(
            ctx.run_ui(RawInput::default(), |ui| {
                vp.update_playback(ui.ctx(), &ring, &clock, now);
            }),
            None,
            None,
            None,
        );
        assert_eq!(vp.capture_frame().map(|f| (f.pts_ms, f.generation, f.width)), Some((0, 0, 32)));

        // Queue leftover stale gen=0 frames, seek while paused -> gen=1, then push two gen=1 frames.
        let _ = ring.try_push(VideoFrameRGBA::new(32, 32, 1040).with_generation(0));
        let _ = ring.try_push(VideoFrameRGBA::new(32, 32, 1080).with_generation(0));
        clock.seek_to(0.0, now);
        assert_eq!(clock.seek_generation(), 1);

        let _ = ring.try_push(VideoFrameRGBA::new(128, 72, 0).with_generation(1));
        let _ = ring.try_push(VideoFrameRGBA::new(128, 72, 40).with_generation(1));

        let _ = capture_full_output(
            ctx.run_ui(RawInput::default(), |ui| {
                vp.update_playback(ui.ctx(), &ring, &clock, now);
            }),
            None,
            None,
            None,
        );

        let presented = vp
            .capture_frame()
            .expect("Expected gen=1 pts=0ms frame to upload despite previous pts=0ms");
        assert_eq!(presented.generation, 1);
        assert_eq!(presented.pts_ms, 0);
        assert_eq!(presented.width, 128);
        assert_eq!(presented.height, 72);
        assert_eq!(
            ring.len(),
            1,
            "Second gen=1 frame (pts=40ms) must remain queued in video_ring"
        );
        assert_eq!(ring.try_pop().map(|f| (f.pts_ms, f.generation)), Some((40, 1)));
    }
}
