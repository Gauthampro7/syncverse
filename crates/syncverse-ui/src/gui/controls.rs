//! Modern sleek dark desktop playback controls bar, interactive seekbar, volume slider, HUD, and speed selector.

use std::time::Instant;
use egui::{Color32, Context, FontId, Layout, RichText, Stroke, Ui, Vec2};
use crate::debouncer::SeekDebouncer;
use crate::gui::ab_loop::AbRepeatState;
use crate::gui::aspect_ratio::AspectRatioOverride;
use crate::gui::dialogs::{AudioConfigDialog, SubtitleConfigDialog, VideoConfigDialog};
use crate::overlay::format_timecode_hud;
use crate::snapshot::PlayerUiSnapshot;

/// Supported playback speeds in the speed selector.
pub const SUPPORTED_SPEEDS: [f64; 8] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

/// Minimum controls bar width threshold in logical pixels below which secondary combo boxes
/// (playback speed and aspect ratio) are collapsed to prevent visual overflow on compact viewports.
pub const COMPACT_LAYOUT_THRESHOLD_PX: f32 = 700.0;

/// Minimum controls bar width threshold in logical pixels below which secondary buttons and chapters
/// are collapsed to preserve core transport and HUD clarity on narrow or split viewports.
pub const ULTRA_COMPACT_LAYOUT_THRESHOLD_PX: f32 = 560.0;

/// State of audio volume and mute controls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VolumeControlState {
    /// Audio gain [0.0, 2.0], where > 1.0 enables soft-knee over-amplification boost.
    pub volume: f32,
    /// Whether audio output is currently muted.
    pub is_muted: bool,
    /// Cached volume prior to muting, used for volume restoration upon unmuting.
    pub unmuted_volume: f32,
}

impl Default for VolumeControlState {
    fn default() -> Self {
        Self {
            volume: 1.0,
            is_muted: false,
            unmuted_volume: 1.0,
        }
    }
}

impl VolumeControlState {
    /// Maximum allowable audio gain multiplier (2.0 = 200% volume boost).
    pub const MAX_VOLUME: f32 = 2.0;

    /// Constructs a new `VolumeControlState` with initial volume clamped to [0.0, MAX_VOLUME].
    pub fn new(initial_volume: f32) -> Self {
        let v = initial_volume.clamp(0.0, Self::MAX_VOLUME);
        Self {
            volume: v,
            is_muted: v == 0.0,
            unmuted_volume: if v > 0.0 { v } else { 0.5 },
        }
    }

    /// Computes the effective audio volume to send to audio sink (0.0 if muted, else volume).
    pub fn effective_volume(&self) -> f32 {
        if self.is_muted {
            0.0
        } else {
            self.volume
        }
    }

    /// Updates volume from user slider adjustment, automatically unmuting if positive.
    pub fn set_volume(&mut self, vol: f32) {
        let v = vol.clamp(0.0, Self::MAX_VOLUME);
        self.volume = v;
        if v > 0.0 {
            self.is_muted = false;
            self.unmuted_volume = v;
        } else {
            self.is_muted = true;
        }
    }

    /// Toggles mute state, restoring previous non-zero volume upon unmuting.
    pub fn toggle_mute(&mut self) {
        self.is_muted = !self.is_muted;
        if !self.is_muted && self.volume == 0.0 {
            self.volume = if self.unmuted_volume > 0.0 {
                self.unmuted_volume
            } else {
                0.5
            };
        }
    }

    /// Formats the current volume as an integer percentage string: `"80%"`.
    pub fn formatted_percentage(&self) -> String {
        format!("{}%", (self.effective_volume() * 100.0).round() as u32)
    }

    /// Returns the appropriate volume icon glyph based on state.
    pub fn glyph(&self) -> &'static str {
        if self.is_muted || self.volume == 0.0 {
            "🔇"
        } else if self.volume < 0.5 {
            "🔉"
        } else {
            "🔊"
        }
    }
}

/// Comprehensive state for bottom docked playback controls bar.
#[derive(Debug)]
pub struct ControlsBarState {
    /// 10 Hz rate-limiting seek debouncer.
    pub debouncer: SeekDebouncer,
    /// Volume and mute controller.
    pub volume: VolumeControlState,
    /// Currently selected playback speed multiplier.
    pub speed: f64,
    /// Active aspect ratio override preset.
    pub aspect_ratio: AspectRatioOverride,
    /// Active drag scrub position in milliseconds (if user is actively dragging the seekbar).
    pub drag_pts_ms: Option<u64>,
    /// Pending seek target in milliseconds and emission instant to avoid rubber-band jitter before snapshot updates.
    pub pending_seek_ms: Option<(u64, Instant)>,
    /// Hover preview timestamp in milliseconds (if mouse is hovering over seekbar).
    pub hover_pts_ms: Option<u64>,
    /// Timestamp of last user mouse activity (for fullscreen auto-hide).
    pub last_mouse_activity: Instant,
    /// Whether fullscreen mode is active.
    pub is_fullscreen: bool,
    /// Auto-hide timeout duration (default 2.5s).
    pub autohide_delay_secs: f64,
    /// Audio configuration modal dialog.
    pub audio_dialog: AudioConfigDialog,
    /// Subtitle configuration modal dialog.
    pub subtitle_dialog: SubtitleConfigDialog,
    /// Video configuration modal dialog.
    pub video_dialog: VideoConfigDialog,
    /// Discovered container chapter markers.
    pub chapters: Vec<syncverse_demux::MediaChapter>,
    /// Active A-B repeat playback looping state.
    pub ab_repeat: AbRepeatState,
}

impl Default for ControlsBarState {
    fn default() -> Self {
        Self {
            debouncer: SeekDebouncer::default(),
            volume: VolumeControlState::default(),
            speed: 1.0,
            aspect_ratio: AspectRatioOverride::Original,
            drag_pts_ms: None,
            pending_seek_ms: None,
            hover_pts_ms: None,
            last_mouse_activity: Instant::now(),
            is_fullscreen: false,
            autohide_delay_secs: 2.5,
            audio_dialog: AudioConfigDialog::default(),
            subtitle_dialog: SubtitleConfigDialog::default(),
            video_dialog: VideoConfigDialog::default(),
            chapters: Vec::new(),
            ab_repeat: AbRepeatState::default(),
        }
    }
}

impl ControlsBarState {
    /// Constructs a new `ControlsBarState`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records user interaction to reset fullscreen auto-hide timer.
    pub fn on_user_activity(&mut self, now: Instant) {
        self.last_mouse_activity = now;
    }

    /// Returns whether playback controls bar should be visible.
    ///
    /// In windowed mode: always visible.
    /// In fullscreen mode: visible only if recent mouse activity occurred within `autohide_delay_secs`.
    pub fn is_visible(&self, now: Instant) -> bool {
        if !self.is_fullscreen {
            true
        } else {
            now.saturating_duration_since(self.last_mouse_activity).as_secs_f64() < self.autohide_delay_secs
        }
    }

    /// Formats single timestamp in milliseconds to `"HH:MM:SS"`.
    pub fn format_timestamp(ms: u64) -> String {
        let sec = ms / 1000;
        let h = sec / 3600;
        let m = (sec % 3600) / 60;
        let s = sec % 60;
        format!("{:02}:{:02}:{:02}", h, m, s)
    }

    /// Opens the audio configuration dialog, synchronizing volume and mute state.
    pub fn open_audio_dialog(&mut self) {
        self.audio_dialog.volume = self.volume.volume;
        self.audio_dialog.is_muted = self.volume.is_muted;
        self.audio_dialog.open();
    }

    /// Opens the subtitle configuration dialog.
    pub fn open_subtitle_dialog(&mut self) {
        self.subtitle_dialog.open();
    }

    /// Opens the video configuration dialog, synchronizing aspect ratio and speed.
    pub fn open_video_dialog(&mut self) {
        self.video_dialog.speed = self.speed;
        self.video_dialog.aspect_ratio = self.aspect_ratio;
        self.video_dialog.open();
    }

    /// Returns true if the controls bar width is below `COMPACT_LAYOUT_THRESHOLD_PX` (< 700px).
    pub fn is_compact_layout(width: f32) -> bool {
        width < COMPACT_LAYOUT_THRESHOLD_PX
    }

    /// Returns true if the controls bar width is below `ULTRA_COMPACT_LAYOUT_THRESHOLD_PX` (< 560px).
    pub fn is_ultra_compact_layout(width: f32) -> bool {
        width < ULTRA_COMPACT_LAYOUT_THRESHOLD_PX
    }

    /// Evaluates the effective presentation timestamp in milliseconds, taking into account
    /// active dragging and pending seek smoothing with a 400ms grace period.
    pub fn effective_position_ms(&mut self, snapshot_ms: u64, now: Instant) -> u64 {
        if let Some(drag_pts) = self.drag_pts_ms {
            drag_pts
        } else if let Some((seek_target, seek_time)) = self.pending_seek_ms {
            if now.saturating_duration_since(seek_time) < std::time::Duration::from_millis(400)
                && snapshot_ms.abs_diff(seek_target) > 500
            {
                seek_target
            } else {
                self.pending_seek_ms = None;
                snapshot_ms
            }
        } else {
            snapshot_ms
        }
    }

    /// Renders any active modal dialogs (Audio, Video, Subtitle) to the UI context.
    pub fn show_dialogs(&mut self, ctx: &Context) {
        self.audio_dialog.show(ctx);
        self.subtitle_dialog.show(ctx);
        self.video_dialog.show(ctx);
    }
}

/// Actions emitted by the playback controls bar to the application coordinator.
#[derive(Debug, Clone, PartialEq)]
pub enum ControlsBarAction {
    /// Toggle Play/Pause state.
    TogglePause,
    /// Stop playback and return to PTS 0.
    Stop,
    /// Seek to target timestamp in milliseconds.
    Seek(u64),
    /// Set audio volume [0.0, 2.0].
    SetVolume(f32),
    /// Toggle audio mute.
    ToggleMute,
    /// Set playback speed ratio.
    SetSpeed(f64),
    /// Set aspect ratio override.
    SetAspectRatio(AspectRatioOverride),
    /// Cycle to next aspect ratio preset.
    CycleAspectRatio,
    /// Toggle watch-party sidebar open/closed.
    ToggleSidebar,
    /// Toggle fullscreen mode (F11).
    ToggleFullscreen,
    /// Open audio configuration modal dialog.
    OpenAudioSettings,
    /// Open subtitle configuration modal dialog.
    OpenSubtitleSettings,
    /// Open video configuration modal dialog.
    OpenVideoSettings,
    /// Jump forward to the start of the next chapter.
    NextChapter,
    /// Jump backward to the start of the current or previous chapter.
    PreviousChapter,
    /// Capture a lossless PNG screenshot of the current video frame.
    TakeScreenshot,
    /// Advance A-B repeat playback looping state machine.
    CycleAbRepeat,
    /// Clear A-B repeat playback looping immediately.
    ClearAbRepeat,
    /// Toggle playlist queue dialog window.
    TogglePlaylist,
    /// Toggle technical media and codec statistics dialog window.
    ToggleMediaInfo,
    /// Jump to previous track in playlist/folder.
    PreviousTrack,
    /// Jump to next track in playlist/folder.
    NextTrack,
}

/// Modern sleek dark aesthetics palette for player controls bar.
pub mod style {
    use egui::Color32;

    /// Controls bar container background (#141417).
    pub const BAR_BACKGROUND: Color32 = Color32::from_rgb(20, 20, 23);
    /// Top border separator (#27272A).
    pub const BAR_BORDER: Color32 = Color32::from_rgb(39, 39, 42);

    /// Seekbar track background (#3F3F46).
    pub const SEEKBAR_TRACK: Color32 = Color32::from_rgb(63, 63, 70);
    /// Seekbar played progress fill (#38BDF8 Sky Blue).
    pub const SEEKBAR_PLAYED: Color32 = Color32::from_rgb(56, 189, 248);
    /// Seekbar seekhead thumb (#FFFFFF).
    pub const SEEKBAR_THUMB: Color32 = Color32::from_rgb(255, 255, 255);
    /// Seekbar seekhead outer glow.
    pub const SEEKBAR_THUMB_GLOW: Color32 = Color32::from_rgba_premultiplied(56, 189, 248, 120);

    /// Button background normal (#27272A).
    pub const BUTTON_BG: Color32 = Color32::from_rgb(39, 39, 42);

    /// Play button accent background (#0EA5E9).
    pub const PLAY_ACCENT_BG: Color32 = Color32::from_rgb(14, 165, 233);

    /// Primary foreground text (#F4F4F5).
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(244, 244, 245);
    /// Secondary muted text (#A1A1AA).
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(161, 161, 170);

    /// Tooltip background (#18181B with 95% opacity).
    pub const TOOLTIP_BG: Color32 = Color32::from_rgba_premultiplied(24, 24, 27, 245);
    /// Tooltip border stroke (#52525B).
    pub const TOOLTIP_BORDER: Color32 = Color32::from_rgb(82, 82, 91);
}

/// Renderer for bottom docked playback controls bar.
pub struct PlaybackControlsBar;

impl PlaybackControlsBar {
    /// Renders the complete bottom docked controls bar into the given `egui::Ui`.
    pub fn show(
        ui: &mut Ui,
        snapshot: &PlayerUiSnapshot,
        state: &mut ControlsBarState,
        now: Instant,
    ) -> Vec<ControlsBarAction> {
        let mut actions = Vec::new();

        // 1. Container frame with sleek dark styling
        egui::Frame::new()
            .fill(style::BAR_BACKGROUND)
            .stroke(Stroke::new(1.0, style::BAR_BORDER))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    let snapshot_ms = snapshot.media_position_pts / 1_000_000;
                    let current_ms = state.effective_position_ms(snapshot_ms, now);

                    // Top: Interactive Seekbar spanning available width
                    actions.extend(Self::render_seekbar(ui, snapshot, state, current_ms, now));

                    ui.add_space(6.0);

                    // Bottom: Transport buttons, Timecode HUD, Volume, Speed, and Toggles
                    actions.extend(Self::render_controls_row(ui, snapshot, state, current_ms));
                });
            });

        actions
    }

    /// Renders the interactive seekbar track with hover preview tooltip and debounced scrubbing.
    fn render_seekbar(
        ui: &mut Ui,
        snapshot: &PlayerUiSnapshot,
        state: &mut ControlsBarState,
        current_ms: u64,
        now: Instant,
    ) -> Vec<ControlsBarAction> {
        let mut actions = Vec::new();

        let duration_ms = snapshot.media_duration_pts / 1_000_000;

        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 16.0),
            egui::Sense::click_and_drag(),
        );

        let is_active = response.hovered() || response.dragged();
        let track_height = if is_active { 8.0 } else { 4.0 };
        let y_center = rect.center().y;

        let progress_fraction = if duration_ms > 0 {
            (current_ms as f64 / duration_ms as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let painter = ui.painter();

        // Background track line
        painter.line_segment(
            [
                egui::pos2(rect.min.x, y_center),
                egui::pos2(rect.max.x, y_center),
            ],
            Stroke::new(track_height, style::SEEKBAR_TRACK),
        );

        // Played progress line
        let played_x = rect.min.x + (progress_fraction as f32 * rect.width());
        if played_x > rect.min.x {
            painter.line_segment(
                [
                    egui::pos2(rect.min.x, y_center),
                    egui::pos2(played_x, y_center),
                ],
                Stroke::new(track_height, style::SEEKBAR_PLAYED),
            );
        }

        // Chapter notch markers
        if duration_ms > 0 {
            for chapter in &state.chapters {
                if chapter.start_ms > 0 && chapter.start_ms < duration_ms {
                    let ch_fraction = (chapter.start_ms as f64 / duration_ms as f64).clamp(0.0, 1.0);
                    let ch_x = rect.min.x + (ch_fraction as f32 * rect.width());
                    painter.line_segment(
                        [
                            egui::pos2(ch_x, y_center - (track_height * 0.85)),
                            egui::pos2(ch_x, y_center + (track_height * 0.85)),
                        ],
                        Stroke::new(1.5, egui::Color32::from_rgb(250, 204, 21)),
                    );
                }
            }

            // A-B Repeat segment and marker ticks
            if let Some(a_ms) = state.ab_repeat.point_a() {
                let a_frac = (a_ms as f64 / duration_ms as f64).clamp(0.0, 1.0);
                let a_x = rect.min.x + (a_frac as f32 * rect.width());

                // Looping span and Point B marker if active
                if let Some(b_ms) = state.ab_repeat.point_b() {
                    let b_frac = (b_ms as f64 / duration_ms as f64).clamp(0.0, 1.0);
                    let b_x = rect.min.x + (b_frac as f32 * rect.width());
                    let min_x = a_x.min(b_x);
                    let max_x = a_x.max(b_x);

                    painter.line_segment(
                        [egui::pos2(min_x, y_center), egui::pos2(max_x, y_center)],
                        Stroke::new(track_height * 1.5, Color32::from_rgb(168, 85, 247)),
                    );

                    painter.line_segment(
                        [
                            egui::pos2(b_x, y_center - (track_height * 1.3)),
                            egui::pos2(b_x, y_center + (track_height * 1.3)),
                        ],
                        Stroke::new(2.5, Color32::from_rgb(236, 72, 153)),
                    );
                }

                // Point A marker tick
                painter.line_segment(
                    [
                        egui::pos2(a_x, y_center - (track_height * 1.3)),
                        egui::pos2(a_x, y_center + (track_height * 1.3)),
                    ],
                    Stroke::new(2.5, Color32::from_rgb(74, 222, 128)),
                );
            }
        }

        // Seekhead thumb rendering
        if is_active {
            painter.circle_filled(
                egui::pos2(played_x, y_center),
                8.0,
                style::SEEKBAR_THUMB_GLOW,
            );
            painter.circle_filled(
                egui::pos2(played_x, y_center),
                5.5,
                style::SEEKBAR_THUMB,
            );
        } else {
            painter.circle_filled(
                egui::pos2(played_x, y_center),
                4.0,
                style::SEEKBAR_THUMB,
            );
        }

        // Hover tooltip preview (HH:MM:SS with Chapter title) & mouse wheel seek
        if response.hovered() && duration_ms > 0 {
            // Mouse wheel scroll to seek (±5s) - ignored if actively dragging to prevent conflicts
            let is_dragging = response.dragged() || state.drag_pts_ms.is_some();
            if !is_dragging {
                let scroll_y = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll_y.abs() >= 2.0 {
                    state.on_user_activity(now);
                    let step_ms: i64 = if scroll_y > 0.0 { 5_000 } else { -5_000 };
                    let target_pts = (current_ms as i64 + step_ms).clamp(0, duration_ms as i64) as u64;
                    state.pending_seek_ms = Some((target_pts, now));
                    actions.push(ControlsBarAction::Seek(target_pts));
                }
            }

            if let Some(mouse_pos) = response.hover_pos() {
                let rel_x = ((mouse_pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                let hover_pts = (rel_x as f64 * duration_ms as f64).round() as u64;
                state.hover_pts_ms = Some(hover_pts);

                response.show_tooltip_ui(|ui| {
                    ui.style_mut().visuals.window_fill = style::TOOLTIP_BG;
                    ui.style_mut().visuals.window_stroke =
                        Stroke::new(1.0, style::TOOLTIP_BORDER);

                    let active_chapter = state
                        .chapters
                        .iter()
                        .find(|c| hover_pts >= c.start_ms && hover_pts <= c.end_ms);
                    let tooltip_text = if let Some(ch) = active_chapter {
                        format!(
                            "{} — 📌 {}",
                            ControlsBarState::format_timestamp(hover_pts),
                            ch.title
                        )
                    } else {
                        ControlsBarState::format_timestamp(hover_pts)
                    };

                    ui.add(
                        egui::Label::new(
                            RichText::new(tooltip_text)
                                .monospace()
                                .color(style::TEXT_PRIMARY)
                                .size(12.0),
                        ),
                    );
                });
            }
        } else {
            state.hover_pts_ms = None;
        }

        // Drag scrubbing (10 Hz debounced)
        if response.dragged() && duration_ms > 0 {
            state.on_user_activity(now);
            if let Some(mouse_pos) = response.interact_pointer_pos() {
                let rel_x = ((mouse_pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                let target_pts = (rel_x as f64 * duration_ms as f64).round() as u64;
                state.drag_pts_ms = Some(target_pts);

                if let Some(debounced_pts) = state.debouncer.on_drag_instant(target_pts, now) {
                    actions.push(ControlsBarAction::Seek(debounced_pts));
                }
            }
        }

        // Drag release (Authoritative final seek)
        if response.drag_stopped() && duration_ms > 0 {
            state.on_user_activity(now);
            if let Some(drag_pts) = state.drag_pts_ms.take() {
                let final_pts = state.debouncer.on_release_instant(drag_pts, now);
                state.pending_seek_ms = Some((final_pts, now));
                actions.push(ControlsBarAction::Seek(final_pts));
            }
        } else if response.clicked() && duration_ms > 0 {
            // Click-to-seek (guarded by else-if so it does not trigger on drag release)
            state.on_user_activity(now);
            if let Some(mouse_pos) = response.interact_pointer_pos() {
                let rel_x = ((mouse_pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                let target_pts = (rel_x as f64 * duration_ms as f64).round() as u64;
                state.pending_seek_ms = Some((target_pts, now));
                actions.push(ControlsBarAction::Seek(target_pts));
            }
        }

        actions
    }

    /// Renders the horizontal controls row: buttons, timecode HUD, volume, speed selector.
    fn render_controls_row(
        ui: &mut Ui,
        snapshot: &PlayerUiSnapshot,
        state: &mut ControlsBarState,
        current_ms: u64,
    ) -> Vec<ControlsBarAction> {
        let mut actions = Vec::new();
        let bar_width = ui.available_width();
        let is_compact = ControlsBarState::is_compact_layout(bar_width);
        let is_ultra_compact = ControlsBarState::is_ultra_compact_layout(bar_width);

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);

            // Play / Pause Button (Bit 0: 1 = Playing, 0 = Paused/Stopped)
            let is_playing = (snapshot.playback_state & 1) != 0;
            let play_label = if is_playing { "⏸" } else { "▶" };
            let play_btn = egui::Button::new(
                RichText::new(play_label)
                    .size(16.0)
                    .color(style::TEXT_PRIMARY),
            )
            .min_size(Vec2::new(36.0, 32.0))
            .fill(if is_playing {
                style::BUTTON_BG
            } else {
                style::PLAY_ACCENT_BG
            })
            .corner_radius(6.0);

            if ui
                .add(play_btn)
                .on_hover_text(if is_playing { "Pause (Space)" } else { "Play (Space)" })
                .clicked()
            {
                actions.push(ControlsBarAction::TogglePause);
            }

            // Stop Button
            let stop_btn = egui::Button::new(
                RichText::new("⏹")
                    .size(14.0)
                    .color(style::TEXT_PRIMARY),
            )
            .min_size(Vec2::new(32.0, 32.0))
            .fill(style::BUTTON_BG)
            .corner_radius(6.0);

            if ui
                .add(stop_btn)
                .on_hover_text("Stop Playback (returns to 00:00:00)")
                .clicked()
            {
                actions.push(ControlsBarAction::Stop);
            }

            // Track / Chapter & A-B Looping Buttons (collapsed on narrow viewports < 560px)
            if !is_ultra_compact {
                // Previous Track Button (P)
                let prev_trk_btn = egui::Button::new(
                    RichText::new("⏮")
                        .size(13.0)
                        .color(style::TEXT_PRIMARY),
                )
                .min_size(Vec2::new(26.0, 32.0))
                .fill(style::BUTTON_BG)
                .corner_radius(6.0);

                if ui
                    .add(prev_trk_btn)
                    .on_hover_text("Previous File / Track (P)")
                    .clicked()
                {
                    actions.push(ControlsBarAction::PreviousTrack);
                }

                // Next Track Button (N)
                let next_trk_btn = egui::Button::new(
                    RichText::new("⏭")
                        .size(13.0)
                        .color(style::TEXT_PRIMARY),
                )
                .min_size(Vec2::new(26.0, 32.0))
                .fill(style::BUTTON_BG)
                .corner_radius(6.0);

                if ui
                    .add(next_trk_btn)
                    .on_hover_text("Next File / Track (N)")
                    .clicked()
                {
                    actions.push(ControlsBarAction::NextTrack);
                }

                // A-B Repeat Looping Button
                let (ab_label, ab_color) = match state.ab_repeat {
                    AbRepeatState::Disabled => ("🔁 A-B", style::TEXT_SECONDARY),
                    AbRepeatState::PointASet(_) => ("A➔...", Color32::from_rgb(74, 222, 128)),
                    AbRepeatState::Active { .. } => ("A⇄B", Color32::from_rgb(168, 85, 247)),
                };
                let ab_btn = egui::Button::new(
                    RichText::new(ab_label)
                        .size(11.0)
                        .strong()
                        .color(ab_color),
                )
                .min_size(Vec2::new(36.0, 32.0))
                .fill(style::BUTTON_BG)
                .corner_radius(6.0);

                let resp = ui
                    .add(ab_btn)
                    .on_hover_text(format!("{} (Left-click: cycle, Right-click: clear)", state.ab_repeat.status_text()));
                if resp.clicked() {
                    actions.push(ControlsBarAction::CycleAbRepeat);
                } else if resp.secondary_clicked() {
                    actions.push(ControlsBarAction::ClearAbRepeat);
                }
            }

            // Timecode HUD: Monospace HH:MM:SS / HH:MM:SS (or single HH:MM:SS on narrowest viewports)
            let duration_ms = snapshot.media_duration_pts / 1_000_000;
            let timecode_str = if bar_width < 450.0 {
                ControlsBarState::format_timestamp(current_ms)
            } else {
                format_timecode_hud(current_ms, duration_ms)
            };

            ui.add(
                egui::Label::new(
                    RichText::new(timecode_str)
                        .monospace()
                        .size(13.0)
                        .color(style::TEXT_PRIMARY),
                ),
            );

            // Right-aligned settings group
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                // Fullscreen Toggle Button (F11 / F)
                let fs_label = if state.is_fullscreen { "🗗" } else { "⛶" };
                if ui
                    .button(RichText::new(fs_label).size(14.0))
                    .on_hover_text("Toggle Fullscreen (F / F11 / Double-Click)")
                    .clicked()
                {
                    actions.push(ControlsBarAction::ToggleFullscreen);
                }

                // Lossless Screenshot Button (📸) (hidden on narrow viewports < 560px)
                if !is_ultra_compact
                    && ui
                        .button(RichText::new("📸").size(14.0))
                        .on_hover_text("Take Lossless Screenshot (F8 / Ctrl+S)")
                        .clicked()
                {
                    actions.push(ControlsBarAction::TakeScreenshot);
                }

                // Watch-Party Sidebar Toggle
                if ui
                    .button(RichText::new("💬").size(14.0))
                    .on_hover_text("Toggle Watch-Party Chat & Member Panel")
                    .clicked()
                {
                    actions.push(ControlsBarAction::ToggleSidebar);
                }

                // Playlist Queue Button (🎶)
                if ui
                    .button(RichText::new("🎶").size(14.0))
                    .on_hover_text("Playlist Queue (Ctrl+L)")
                    .clicked()
                {
                    actions.push(ControlsBarAction::TogglePlaylist);
                }

                // Media Info & Diagnostics Button (ℹ️)
                if !is_ultra_compact
                    && ui
                        .button(RichText::new("ℹ️").size(14.0))
                        .on_hover_text("Media Information & Codec Statistics (Ctrl+I)")
                        .clicked()
                {
                    actions.push(ControlsBarAction::ToggleMediaInfo);
                }

                ui.separator();

                // Video Configuration Quick-Access Button (hidden on narrow viewports < 560px)
                if !is_ultra_compact {
                    let video_active = state.video_dialog.is_open;
                    let video_btn = egui::Button::new(
                        RichText::new("🎬")
                            .size(13.0)
                            .color(if video_active { style::SEEKBAR_PLAYED } else { style::TEXT_PRIMARY }),
                    )
                    .min_size(Vec2::new(28.0, 28.0))
                    .fill(if video_active { style::BUTTON_BG } else { Color32::TRANSPARENT });

                    if ui
                        .add(video_btn)
                        .on_hover_text("Video Settings (Aspect Ratio, Playback Speed, Stream Info)")
                        .clicked()
                    {
                        state.video_dialog.is_open = !state.video_dialog.is_open;
                        if state.video_dialog.is_open {
                            state.video_dialog.speed = state.speed;
                            state.video_dialog.aspect_ratio = state.aspect_ratio;
                        }
                        actions.push(ControlsBarAction::OpenVideoSettings);
                    }

                    // Subtitle Configuration Quick-Access Button
                    let sub_active = state.subtitle_dialog.is_open;
                    let sub_btn = egui::Button::new(
                        RichText::new("CC")
                            .strong()
                            .size(12.0)
                            .color(if sub_active { style::SEEKBAR_PLAYED } else { style::TEXT_PRIMARY }),
                    )
                    .min_size(Vec2::new(28.0, 28.0))
                    .fill(if sub_active { style::BUTTON_BG } else { Color32::TRANSPARENT });

                    if ui
                        .add(sub_btn)
                        .on_hover_text("Subtitle Settings (Tracks, External Subtitles, Sync Offset)")
                        .clicked()
                    {
                        state.subtitle_dialog.is_open = !state.subtitle_dialog.is_open;
                        actions.push(ControlsBarAction::OpenSubtitleSettings);
                    }
                }

                // Audio Configuration Quick-Access Button
                let audio_active = state.audio_dialog.is_open;
                let audio_btn = egui::Button::new(
                    RichText::new("🎚")
                        .size(13.0)
                        .color(if audio_active { style::SEEKBAR_PLAYED } else { style::TEXT_PRIMARY }),
                )
                .min_size(Vec2::new(28.0, 28.0))
                .fill(if audio_active { style::BUTTON_BG } else { Color32::TRANSPARENT });

                if ui
                    .add(audio_btn)
                    .on_hover_text("Audio Settings (Track Selection, Volume, Sync Delay)")
                    .clicked()
                {
                    state.audio_dialog.is_open = !state.audio_dialog.is_open;
                    if state.audio_dialog.is_open {
                        state.audio_dialog.volume = state.volume.volume;
                        state.audio_dialog.is_muted = state.volume.is_muted;
                    }
                    actions.push(ControlsBarAction::OpenAudioSettings);
                }

                if !is_compact {
                    ui.separator();

                    // Aspect Ratio Selector (Original, 16:9, 4:3, 21:9, 1:1, Fill/Stretch)
                    let aspect_label = format!("📐 {}", state.aspect_ratio.display_name());
                    egui::ComboBox::from_id_salt("aspect_ratio_selector")
                        .selected_text(
                            RichText::new(aspect_label)
                                .size(12.0)
                                .color(style::TEXT_PRIMARY),
                        )
                        .width(88.0)
                        .show_ui(ui, |ui| {
                            for &ar in &AspectRatioOverride::ALL {
                                let is_selected = state.aspect_ratio == ar;
                                if ui.selectable_label(is_selected, ar.display_name()).clicked() {
                                    state.aspect_ratio = ar;
                                    actions.push(ControlsBarAction::SetAspectRatio(ar));
                                }
                            }
                        });

                    ui.separator();

                    // Playback Speed Selector (0.25x, 0.5x, 0.75x, 1.0x, 1.25x, 1.5x, 1.75x, 2.0x)
                    let speed_label = format!("{:.2}x", state.speed);
                    egui::ComboBox::from_id_salt("playback_speed_selector")
                        .selected_text(
                            RichText::new(speed_label)
                                .monospace()
                                .size(12.0)
                                .color(style::TEXT_PRIMARY),
                        )
                        .width(68.0)
                        .show_ui(ui, |ui| {
                            for &spd in &SUPPORTED_SPEEDS {
                                let label = format!("{:.2}x", spd);
                                let is_selected = (state.speed - spd).abs() < 0.01;
                                if ui.selectable_label(is_selected, label).clicked() {
                                    state.speed = spd;
                                    actions.push(ControlsBarAction::SetSpeed(spd));
                                }
                            }
                        });
                }

                ui.separator();
                if !is_ultra_compact {
                    // Volume percentage text (amber highlight when boosted > 100%)
                    let vol_color = if state.volume.volume > 1.0 {
                        Color32::from_rgb(251, 146, 60)
                    } else {
                        style::TEXT_SECONDARY
                    };
                    ui.add(
                        egui::Label::new(
                            RichText::new(state.volume.formatted_percentage())
                                .monospace()
                                .size(12.0)
                                .color(vol_color),
                        ),
                    );
                }

                // Volume Slider (0.0 to 2.0 with soft-knee saturation, compact width on narrow viewports)
                let mut vol = state.volume.volume;
                let slider = egui::Slider::new(&mut vol, 0.0..=VolumeControlState::MAX_VOLUME)
                    .show_value(false)
                    .trailing_fill(true);
                let slider_width = if is_ultra_compact { 50.0 } else { 80.0 };
                let slider_resp = ui.add_sized(Vec2::new(slider_width, 16.0), slider);
                if slider_resp.changed() {
                    state.volume.set_volume(vol);
                    actions.push(ControlsBarAction::SetVolume(state.volume.effective_volume()));
                } else if slider_resp.hovered() {
                    let scroll_y = ui.input(|i| i.smooth_scroll_delta.y);
                    if scroll_y >= 2.0 {
                        let new_vol = (state.volume.volume + 0.05).min(VolumeControlState::MAX_VOLUME);
                        state.volume.set_volume(new_vol);
                        actions.push(ControlsBarAction::SetVolume(state.volume.effective_volume()));
                    } else if scroll_y <= -2.0 {
                        let new_vol = (state.volume.volume - 0.05).max(0.0);
                        state.volume.set_volume(new_vol);
                        actions.push(ControlsBarAction::SetVolume(state.volume.effective_volume()));
                    }
                }

                // Mute Toggle Button ('M')
                let mute_btn = egui::Button::new(
                    RichText::new(state.volume.glyph()).font(FontId::proportional(14.0)),
                )
                .min_size(Vec2::new(28.0, 28.0))
                .fill(Color32::TRANSPARENT);

                if ui.add(mute_btn).clicked() {
                    state.volume.toggle_mute();
                    actions.push(ControlsBarAction::ToggleMute);
                }
            });
        });

        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_volume_control_state_transitions() {
        let mut state = VolumeControlState::new(0.8);
        assert_eq!(state.volume, 0.8);
        assert!(!state.is_muted);
        assert_eq!(state.effective_volume(), 0.8);
        assert_eq!(state.formatted_percentage(), "80%");
        assert_eq!(state.glyph(), "🔊");

        // Toggle mute
        state.toggle_mute();
        assert!(state.is_muted);
        assert_eq!(state.effective_volume(), 0.0);
        assert_eq!(state.formatted_percentage(), "0%");
        assert_eq!(state.glyph(), "🔇");

        // Toggle unmute restores 0.8
        state.toggle_mute();
        assert!(!state.is_muted);
        assert_eq!(state.effective_volume(), 0.8);
        assert_eq!(state.formatted_percentage(), "80%");

        // Set volume to 0.3
        state.set_volume(0.3);
        assert_eq!(state.effective_volume(), 0.3);
        assert_eq!(state.glyph(), "🔉");

        // Setting to 0.0 mutes
        state.set_volume(0.0);
        assert!(state.is_muted);
        assert_eq!(state.glyph(), "🔇");
    }

    #[test]
    fn test_controls_bar_autohide_state_machine() {
        let mut state = ControlsBarState::new();
        let t0 = Instant::now();

        // In windowed mode: always visible
        state.is_fullscreen = false;
        assert!(state.is_visible(t0 + Duration::from_secs(10)));

        // In fullscreen mode: visible within 2.5s, hidden after 2.5s
        state.is_fullscreen = true;
        state.on_user_activity(t0);
        assert!(state.is_visible(t0 + Duration::from_millis(1000)));
        assert!(state.is_visible(t0 + Duration::from_millis(2490)));
        assert!(!state.is_visible(t0 + Duration::from_millis(2600)));

        // User activity revives visibility
        state.on_user_activity(t0 + Duration::from_millis(3000));
        assert!(state.is_visible(t0 + Duration::from_millis(3100)));
    }

    #[test]
    fn test_supported_speeds_inventory() {
        assert_eq!(SUPPORTED_SPEEDS.len(), 8);
        assert_eq!(
            SUPPORTED_SPEEDS,
            [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0]
        );
        for window in SUPPORTED_SPEEDS.windows(2) {
            assert!(window[0] < window[1], "Speeds must be in strictly ascending order");
        }
    }

    #[test]
    fn test_controls_bar_aspect_ratio_state() {
        let mut state = ControlsBarState::new();
        assert_eq!(state.aspect_ratio, AspectRatioOverride::Original);

        state.aspect_ratio = AspectRatioOverride::Ratio16_9;
        assert_eq!(state.aspect_ratio, AspectRatioOverride::Ratio16_9);

        state.aspect_ratio = AspectRatioOverride::FillStretch;
        assert_eq!(state.aspect_ratio, AspectRatioOverride::FillStretch);
    }

    #[test]
    fn test_controls_bar_dialog_interactions() {
        let mut state = ControlsBarState::new();
        assert!(!state.audio_dialog.is_open);
        assert!(!state.subtitle_dialog.is_open);
        assert!(!state.video_dialog.is_open);

        state.open_audio_dialog();
        assert!(state.audio_dialog.is_open);
        assert_eq!(state.audio_dialog.volume, state.volume.volume);

        state.open_subtitle_dialog();
        assert!(state.subtitle_dialog.is_open);

        state.open_video_dialog();
        assert!(state.video_dialog.is_open);
        assert_eq!(state.video_dialog.speed, state.speed);
        assert_eq!(state.video_dialog.aspect_ratio, state.aspect_ratio);
    }

    #[test]
    fn test_responsive_compact_layout_guard() {
        assert_eq!(COMPACT_LAYOUT_THRESHOLD_PX, 700.0);
        assert!(ControlsBarState::is_compact_layout(699.9));
        assert!(ControlsBarState::is_compact_layout(500.0));
        assert!(!ControlsBarState::is_compact_layout(700.0));
        assert!(!ControlsBarState::is_compact_layout(1280.0));
    }

    #[test]
    fn test_responsive_ultra_compact_layout_guard() {
        assert_eq!(ULTRA_COMPACT_LAYOUT_THRESHOLD_PX, 560.0);
        assert!(ControlsBarState::is_ultra_compact_layout(360.0));
        assert!(ControlsBarState::is_ultra_compact_layout(480.0));
        assert!(ControlsBarState::is_ultra_compact_layout(559.9));
        assert!(!ControlsBarState::is_ultra_compact_layout(560.0));
        assert!(!ControlsBarState::is_ultra_compact_layout(640.0));
        assert!(!ControlsBarState::is_ultra_compact_layout(1920.0));
    }

    #[test]
    fn test_pending_seek_smoothing_lifecycle() {
        let mut state = ControlsBarState::new();
        assert_eq!(state.pending_seek_ms, None);

        let now = Instant::now();
        state.pending_seek_ms = Some((45_000, now));
        assert_eq!(state.pending_seek_ms, Some((45_000, now)));

        // Clearing pending seek resets smoothly
        state.pending_seek_ms = None;
        assert_eq!(state.pending_seek_ms, None);
    }

    #[test]
    fn test_effective_position_ms_lifecycle() {
        let mut state = ControlsBarState::new();
        let now = Instant::now();

        // 1. Without dragging or pending seek: returns snapshot position
        assert_eq!(state.effective_position_ms(10_000, now), 10_000);

        // 2. While actively dragging: returns drag PTS irrespective of snapshot
        state.drag_pts_ms = Some(25_000);
        assert_eq!(state.effective_position_ms(10_000, now), 25_000);
        state.drag_pts_ms = None;

        // 3. With pending seek within 400ms grace period: returns seek target if snapshot lags (> 500ms diff)
        state.pending_seek_ms = Some((30_000, now));
        assert_eq!(state.effective_position_ms(10_000, now), 30_000);
        assert!(state.pending_seek_ms.is_some());

        // 4. When snapshot catches up (within 500ms of seek target): resets pending seek and returns snapshot
        assert_eq!(state.effective_position_ms(29_800, now), 29_800);
        assert_eq!(state.pending_seek_ms, None);

        // 5. When grace period expires (> 400ms): clears pending seek and returns snapshot
        state.pending_seek_ms = Some((50_000, now - Duration::from_millis(500)));
        assert_eq!(state.effective_position_ms(12_000, now), 12_000);
        assert_eq!(state.pending_seek_ms, None);
    }

    #[test]
    fn test_controls_clear_ab_repeat_action() {
        let action = ControlsBarAction::ClearAbRepeat;
        assert!(matches!(action, ControlsBarAction::ClearAbRepeat));
    }
}

