//! Main desktop graphical player application and eframe lifecycle coordinator.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

use egui::{Align2, Color32, Context, CornerRadius, Stroke, ViewportBuilder, ViewportCommand};
use syncverse_chat::ChatRecord;
use syncverse_clock::{MasterClock, PlaybackState, SpscRingBuffer};
use syncverse_codecs::VideoFrameRGBA;

use crate::command::PlayerCommand;
use crate::event::UiEvent;
use crate::gui::controls::{ControlsBarAction, ControlsBarState, PlaybackControlsBar};
use crate::gui::dialogs::{AboutDialog, ConnectPartyDialog, HostPartyDialog, ShortcutsDialog};
use crate::gui::drag_drop::{DragDropHandler, DroppedFileAction};
use crate::gui::file_dialog::FileDialogManager;
use crate::gui::recent_files::RecentFilesManager;
use crate::gui::sidebar::SidebarState;
use crate::gui::subtitle_overlay::{
    compute_effective_pts, find_active_cue_text, SubtitleCue, SubtitleOverlay, SubtitleTrack,
};
use crate::gui::viewport::VideoViewport;
use crate::osd::OsdOverlay;
use crate::snapshot::TripleBufferSnapshotExchange;

/// Preset aspect ratio modes for sequential cycling.
pub const ASPECT_RATIO_PRESETS: [&str; 6] = [
    "Original (Fit)",
    "16:9",
    "4:3",
    "21:9",
    "1:1",
    "Fill / Stretch",
];

/// Information for rendering the update notification banner.
#[derive(Debug, Clone)]
pub struct UpdateBannerInfo {
    /// Latest remote version string.
    pub latest_version: String,
    /// Web URL to the GitHub release page.
    pub release_url: String,
    /// Direct download URL for the target installer or binary.
    pub download_url: Option<String>,
}

/// Information for rendering the unpause gate blocked on-screen alert banner.
#[derive(Debug, Clone)]
pub struct UnpauseBlockedBanner {
    /// List of unready usernames.
    pub unready_peers: Vec<String>,
    /// Alert announcement message.
    pub message: String,
    /// Alert emission instant.
    pub timestamp: Instant,
}

/// Desktop application state implementing `eframe::App`.
pub struct SyncplayApp {
    viewport: VideoViewport,
    controls_state: ControlsBarState,
    sidebar_state: SidebarState,
    file_dialog: FileDialogManager,
    recent_files: RecentFilesManager,
    host_dialog: HostPartyDialog,
    connect_dialog: ConnectPartyDialog,
    shortcuts_dialog: ShortcutsDialog,
    about_dialog: AboutDialog,
    exchange: Arc<TripleBufferSnapshotExchange>,
    video_ring: Arc<SpscRingBuffer<VideoFrameRGBA, 32>>,
    clock: Arc<MasterClock>,
    command_tx: Sender<PlayerCommand>,
    event_rx: Receiver<UiEvent>,
    local_event_tx: Sender<UiEvent>,
    local_event_rx: Receiver<UiEvent>,
    chat_records: Vec<ChatRecord>,
    current_file: Option<PathBuf>,
    update_info: Option<UpdateBannerInfo>,
    is_update_banner_dismissed: bool,
    update_status_toast: Option<(String, Instant)>,
    unpause_blocked_banner: Option<UnpauseBlockedBanner>,
    osd_overlay: OsdOverlay,
    audio_delay_ms: i64,
    subtitle_delay_ms: i64,
    audio_track_index: usize,
    audio_tracks: Vec<String>,
    subtitle_track_index: Option<usize>,
    subtitle_tracks: Vec<String>,
    subtitle_overlay: SubtitleOverlay,
    active_subtitle_cues: Vec<SubtitleCue>,
    subtitle_track_data: Vec<SubtitleTrack>,
    aspect_ratio_index: usize,
}

impl Default for SyncplayApp {
    fn default() -> Self {
        let exchange = Arc::new(TripleBufferSnapshotExchange::new());
        let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
        let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
        let (command_tx, _command_rx) = channel::<PlayerCommand>();
        let (_event_tx, event_rx) = channel::<UiEvent>();
        let (local_event_tx, local_event_rx) = channel::<UiEvent>();

        Self {
            viewport: VideoViewport::new(),
            controls_state: ControlsBarState::default(),
            sidebar_state: SidebarState::default(),
            file_dialog: FileDialogManager::new(),
            recent_files: RecentFilesManager::load(),
            host_dialog: HostPartyDialog::default(),
            connect_dialog: ConnectPartyDialog::default(),
            shortcuts_dialog: ShortcutsDialog::default(),
            about_dialog: AboutDialog::default(),
            exchange,
            video_ring,
            clock,
            command_tx,
            event_rx,
            local_event_tx,
            local_event_rx,
            chat_records: Vec::new(),
            current_file: None,
            update_info: None,
            is_update_banner_dismissed: false,
            update_status_toast: None,
            unpause_blocked_banner: None,
            osd_overlay: OsdOverlay::new(),
            audio_delay_ms: 0,
            subtitle_delay_ms: 0,
            audio_track_index: 0,
            audio_tracks: Vec::new(),
            subtitle_track_index: None,
            subtitle_tracks: Vec::new(),
            subtitle_overlay: SubtitleOverlay::new(),
            active_subtitle_cues: Vec::new(),
            subtitle_track_data: Vec::new(),
            aspect_ratio_index: 0,
        }
    }
}

impl SyncplayApp {
    /// Constructs a new standalone or headless `SyncplayApp` instance with default channels.
    pub fn new() -> Self {
        Self::default()
    }

    /// Constructs a `SyncplayApp` with explicit presentation subsystems and eframe context.
    pub fn with_subsystems(
        _cc: &eframe::CreationContext<'_>,
        exchange: Arc<TripleBufferSnapshotExchange>,
        video_ring: Arc<SpscRingBuffer<VideoFrameRGBA, 32>>,
        clock: Arc<MasterClock>,
        command_tx: Sender<PlayerCommand>,
        event_rx: Receiver<UiEvent>,
        initial_file: Option<PathBuf>,
    ) -> Self {
        let (local_event_tx, local_event_rx) = channel::<UiEvent>();

        // Trigger asynchronous background update check on startup
        crate::gui::updater::spawn_update_check(
            env!("CARGO_PKG_VERSION"),
            local_event_tx.clone(),
            _cc.egui_ctx.clone(),
        );

        let mut app = Self {
            viewport: VideoViewport::new(),
            controls_state: ControlsBarState::default(),
            sidebar_state: SidebarState::default(),
            file_dialog: FileDialogManager::new(),
            recent_files: RecentFilesManager::load(),
            host_dialog: HostPartyDialog::default(),
            connect_dialog: ConnectPartyDialog::default(),
            shortcuts_dialog: ShortcutsDialog::default(),
            about_dialog: AboutDialog::default(),
            exchange,
            video_ring,
            clock,
            command_tx,
            event_rx,
            local_event_tx,
            local_event_rx,
            chat_records: Vec::new(),
            current_file: None,
            update_info: None,
            is_update_banner_dismissed: false,
            update_status_toast: None,
            unpause_blocked_banner: None,
            osd_overlay: OsdOverlay::new(),
            audio_delay_ms: 0,
            subtitle_delay_ms: 0,
            audio_track_index: 0,
            audio_tracks: Vec::new(),
            subtitle_track_index: None,
            subtitle_tracks: Vec::new(),
            subtitle_overlay: SubtitleOverlay::new(),
            active_subtitle_cues: Vec::new(),
            subtitle_track_data: Vec::new(),
            aspect_ratio_index: 0,
        };

        if let Some(path) = initial_file {
            app.load_file(path);
        }

        app
    }

    /// Returns a reference to the active OSD HUD overlay.
    pub fn osd_overlay(&self) -> &OsdOverlay {
        &self.osd_overlay
    }

    /// Returns a mutable reference to the active OSD HUD overlay.
    pub fn osd_overlay_mut(&mut self) -> &mut OsdOverlay {
        &mut self.osd_overlay
    }

    /// Returns the current presentation audio delay in milliseconds.
    pub fn audio_delay_ms(&self) -> i64 {
        self.audio_delay_ms
    }

    /// Returns the current presentation subtitle delay in milliseconds.
    pub fn subtitle_delay_ms(&self) -> i64 {
        self.subtitle_delay_ms
    }

    /// Returns the currently active audio track index.
    pub fn audio_track_index(&self) -> usize {
        self.audio_track_index
    }

    /// Returns the currently active subtitle track index (`None` if disabled/off).
    pub fn subtitle_track_index(&self) -> Option<usize> {
        self.subtitle_track_index
    }

    /// Returns the slice of all discovered and loaded subtitle track labels.
    pub fn subtitle_tracks(&self) -> &[String] {
        &self.subtitle_tracks
    }

    /// Selects the active subtitle track by index (`None` for Off/Disabled).
    pub fn select_subtitle_track(&mut self, track_idx: Option<usize>) {
        self.subtitle_track_index = track_idx;
        self.controls_state.subtitle_dialog.select_track(track_idx);
        if let Some(idx) = track_idx {
            if idx < self.subtitle_track_data.len() {
                self.active_subtitle_cues = self.subtitle_track_data[idx].cues.clone();
            }
        } else {
            self.active_subtitle_cues.clear();
        }
        let _ = self.command_tx.send(PlayerCommand::SelectSubtitleTrack(track_idx));
    }

    /// Returns the current aspect ratio preset label.
    pub fn aspect_ratio_preset(&self) -> &'static str {
        ASPECT_RATIO_PRESETS[self.aspect_ratio_index % ASPECT_RATIO_PRESETS.len()]
    }

    /// Shows a transient OSD HUD notification banner.
    pub fn show_hud_notification(&mut self, text: impl Into<String>, timestamp: Instant) {
        self.osd_overlay.show_notification(text, timestamp);
    }

    /// Returns a reference to the subtitle overlay component.
    pub fn subtitle_overlay(&self) -> &SubtitleOverlay {
        &self.subtitle_overlay
    }

    /// Returns a mutable reference to the subtitle overlay component.
    pub fn subtitle_overlay_mut(&mut self) -> &mut SubtitleOverlay {
        &mut self.subtitle_overlay
    }

    /// Returns the currently active subtitle cues slice.
    pub fn active_subtitle_cues(&self) -> &[SubtitleCue] {
        if let Some(idx) = self.subtitle_track_index {
            if idx < self.subtitle_track_data.len() {
                return &self.subtitle_track_data[idx].cues;
            }
        }
        &self.active_subtitle_cues
    }

    /// Sets the active subtitle cues directly for the currently active track.
    pub fn set_active_subtitle_cues(&mut self, cues: Vec<SubtitleCue>) {
        self.active_subtitle_cues = cues;
    }

    /// Appends a new subtitle track with its cues and title.
    pub fn add_subtitle_track(&mut self, track: SubtitleTrack) {
        if !self.subtitle_tracks.contains(&track.title) {
            self.subtitle_tracks.push(track.title.clone());
        }
        self.subtitle_track_data.push(track);
    }

    /// Evaluates the currently active subtitle text based on current playback PTS and subtitle delay.
    ///
    /// Supports both nanosecond timestamps (as emitted in `PlayerUiSnapshot`) and millisecond
    /// timestamps (for direct presentation queries and test harnesses).
    pub fn current_active_subtitle(&self, media_position_pts: u64) -> Option<String> {
        let track_idx = self.subtitle_track_index?;
        let effective_pts_ms = compute_effective_pts(media_position_pts, self.subtitle_delay_ms);

        // 1. Check pre-parsed track cues if available
        if track_idx < self.subtitle_track_data.len() {
            if let Some(text) = self.subtitle_track_data[track_idx].active_text_at(effective_pts_ms) {
                return Some(text);
            }
        }

        // 2. Check dynamically demuxed / active streaming cues
        if let Some(text) = find_active_cue_text(&self.active_subtitle_cues, effective_pts_ms) {
            return Some(text);
        }

        // 3. Fallback for callers supplying PTS directly in milliseconds
        let direct_ms = media_position_pts as i64 + self.subtitle_delay_ms;
        if track_idx < self.subtitle_track_data.len() {
            if let Some(text) = self.subtitle_track_data[track_idx].active_text_at(direct_ms) {
                return Some(text);
            }
        }
        if let Some(text) = find_active_cue_text(&self.active_subtitle_cues, direct_ms) {
            return Some(text);
        }

        None
    }

    /// Loads and opens a media file.
    pub fn load_file(&mut self, path: PathBuf) {
        self.viewport.reset();
        self.recent_files.add_file(&path);
        let _ = self.command_tx.send(PlayerCommand::OpenFile(path.clone()));
        self.current_file = Some(path.clone());
        self.sidebar_state.local_ready = true;
        self.subtitle_track_index = None;
        self.subtitle_tracks.clear();
        self.subtitle_track_data.clear();
        self.active_subtitle_cues.clear();
        self.subtitle_delay_ms = 0;

        let external = syncverse_demux::subtitles::scan_external_subtitles(&path);
        for ext_track in external {
            let label = ext_track.display_label();
            let ui_cues: Vec<crate::gui::subtitle_overlay::SubtitleCue> = ext_track
                .cues
                .into_iter()
                .map(|c| crate::gui::subtitle_overlay::SubtitleCue {
                    start_ms: c.start.as_millis() as i64,
                    end_ms: c.end.as_millis() as i64,
                    text: c.text,
                })
                .collect();
            let track = crate::gui::subtitle_overlay::SubtitleTrack::new(
                ext_track.id,
                label.clone(),
                ext_track.language,
                ui_cues,
            );
            self.subtitle_tracks.push(label);
            self.subtitle_track_data.push(track);
        }
        let dialog_items: Vec<crate::gui::dialogs::SubtitleTrackItem> = self
            .subtitle_track_data
            .iter()
            .enumerate()
            .map(|(i, t)| crate::gui::dialogs::SubtitleTrackItem {
                id: i,
                title: t.title.clone(),
                language: t.language.clone(),
                format: "SUB".to_string(),
                is_external: true,
                file_path: None,
            })
            .collect();
        self.controls_state.subtitle_dialog.set_tracks(dialog_items);
    }

    /// Loads an external subtitle file (.srt, .vtt, .ass), parses its cues, registers it as active,
    /// and dispatches `PlayerCommand::LoadExternalSubtitle` to the core playback runtime.
    pub fn load_external_subtitle_file(&mut self, path: PathBuf, now: Instant) {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let parse_res = match ext.as_str() {
            "srt" => std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|c| syncverse_demux::subtitles::parse_srt(&c).map_err(|e| e.to_string())),
            "vtt" => std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|c| syncverse_demux::subtitles::parse_vtt(&c).map_err(|e| e.to_string())),
            "ass" | "ssa" => std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|c| syncverse_demux::subtitles::parse_ass(&c).map_err(|e| e.to_string())),
            other => Err(format!("Unsupported subtitle format: .{other}")),
        };

        match parse_res {
            Ok(parsed_cues) => {
                let filename = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("External Subtitle")
                    .to_string();

                let ui_cues: Vec<crate::gui::subtitle_overlay::SubtitleCue> = parsed_cues
                    .into_iter()
                    .map(|c| crate::gui::subtitle_overlay::SubtitleCue {
                        start_ms: c.start.as_millis() as i64,
                        end_ms: c.end.as_millis() as i64,
                        text: c.text,
                    })
                    .collect();

                let new_track_id = self.subtitle_track_data.len();
                let track = crate::gui::subtitle_overlay::SubtitleTrack::new(
                    new_track_id,
                    filename.clone(),
                    None,
                    ui_cues.clone(),
                );

                self.subtitle_tracks.push(filename.clone());
                self.subtitle_track_data.push(track);
                self.subtitle_track_index = Some(new_track_id);
                self.active_subtitle_cues = ui_cues;

                let dialog_items: Vec<crate::gui::dialogs::SubtitleTrackItem> = self
                    .subtitle_track_data
                    .iter()
                    .enumerate()
                    .map(|(i, t)| crate::gui::dialogs::SubtitleTrackItem {
                        id: i,
                        title: t.title.clone(),
                        language: t.language.clone(),
                        format: ext.to_uppercase(),
                        is_external: true,
                        file_path: Some(path.clone()),
                    })
                    .collect();
                self.controls_state.subtitle_dialog.set_tracks(dialog_items);
                self.controls_state.subtitle_dialog.select_track(Some(new_track_id));

                let _ = self.command_tx.send(PlayerCommand::LoadExternalSubtitle(path));

                self.osd_overlay.show_notification(
                    format!("Loaded Subtitles: {filename}"),
                    now,
                );
            }
            Err(err) => {
                eprintln!("[ERROR] Failed to load external subtitle: {err}");
                self.osd_overlay.show_notification(
                    format!("Subtitle Error: {err}"),
                    now,
                );
            }
        }
    }

    /// Appends a new chat record to the local in-memory view.
    pub fn add_chat_record(&mut self, record: ChatRecord) {
        self.chat_records.push(record);
    }
}

impl eframe::App for SyncplayApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx: Context = ui.ctx().clone();
        let now = Instant::now();

        // 0. Drain events from playback runtime and local UI workers
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }
        while let Ok(event) = self.local_event_rx.try_recv() {
            events.push(event);
        }

        for event in events {
            match event {
                UiEvent::NewChatRecord(record) => {
                    self.chat_records.push(record);
                    self.sidebar_state.autoscroll = true;
                }
                UiEvent::RoomMembersUpdated(members) => {
                    if let Some(local) = members.iter().find(|m| m.is_local) {
                        self.sidebar_state.local_ready = local.is_ready;
                    }
                    self.sidebar_state.members = members;
                }
                UiEvent::UnpauseBlocked {
                    unready_peers,
                    message,
                } => {
                    self.unpause_blocked_banner = Some(UnpauseBlockedBanner {
                        unready_peers,
                        message,
                        timestamp: now,
                    });
                }
                UiEvent::UpdateAvailable {
                    latest_version,
                    release_url,
                    download_url,
                } => {
                    self.update_info = Some(UpdateBannerInfo {
                        latest_version,
                        release_url,
                        download_url,
                    });
                    self.is_update_banner_dismissed = false;
                    self.update_status_toast = None;
                }
                UiEvent::UpdateChecked {
                    is_latest,
                    current_version,
                } => {
                    if is_latest {
                        self.update_status_toast = Some((
                            format!("Syncverse v{} is up to date!", current_version),
                            now,
                        ));
                    }
                }
                UiEvent::UpdateFailed { error } => {
                    self.update_status_toast = Some((
                        format!("Update check failed: {}", error),
                        now,
                    ));
                }
                UiEvent::HudNotification { text, timestamp } => {
                    self.osd_overlay.show_notification(text, timestamp);
                }
                UiEvent::AudioTrackSelected { track_id: _, name } => {
                    if let Some(pos) = self.audio_tracks.iter().position(|t| t == &name) {
                        self.audio_track_index = pos;
                    }
                    self.osd_overlay.show_notification(format!("Audio: {}", name), now);
                }
                UiEvent::SubtitleTrackSelected { track_index, name } => {
                    self.subtitle_track_index = track_index;
                    self.controls_state.subtitle_dialog.select_track(track_index);
                    if let Some(idx) = track_index {
                        if idx < self.subtitle_track_data.len() {
                            self.active_subtitle_cues = self.subtitle_track_data[idx].cues.clone();
                        }
                    } else {
                        self.active_subtitle_cues.clear();
                    }
                    let msg = if name == "Off" || track_index.is_none() {
                        "Subtitles: Off".to_string()
                    } else {
                        format!("Subtitles: {}", name)
                    };
                    self.osd_overlay.show_notification(msg, now);
                }
                UiEvent::AudioDelayChanged(ms) => {
                    self.audio_delay_ms = ms;
                    self.osd_overlay.show_notification(format!("Audio Delay: {:+} ms", ms), now);
                }
                UiEvent::SubtitleDelayChanged(ms) => {
                    self.subtitle_delay_ms = ms;
                    self.osd_overlay.show_notification(format!("Subtitle Delay: {:+} ms", ms), now);
                }
                UiEvent::TracksDiscovered {
                    audio_tracks,
                    subtitle_tracks,
                } => {
                    self.audio_tracks = audio_tracks;
                    if self.subtitle_tracks.is_empty() {
                        self.subtitle_tracks = subtitle_tracks;
                    }
                }
                UiEvent::SubtitleDataDiscovered(tracks) => {
                    self.subtitle_track_data = tracks;
                    self.subtitle_tracks = self.subtitle_track_data.iter().map(|t| t.title.clone()).collect();
                    let dialog_items: Vec<crate::gui::dialogs::SubtitleTrackItem> = self
                        .subtitle_track_data
                        .iter()
                        .enumerate()
                        .map(|(i, t)| crate::gui::dialogs::SubtitleTrackItem {
                            id: i,
                            title: t.title.clone(),
                            language: t.language.clone(),
                            format: "SUB".to_string(),
                            is_external: true,
                            file_path: None,
                        })
                        .collect();
                    self.controls_state.subtitle_dialog.set_tracks(dialog_items);
                }
                UiEvent::SubtitleCueDemuxed(cue) => {
                    if let Some(idx) = self.subtitle_track_index {
                        if idx < self.subtitle_track_data.len() {
                            self.subtitle_track_data[idx].add_cue(cue.clone());
                        }
                    }
                    self.active_subtitle_cues.push(cue);
                }
            }
        }

        // 1. Check window close request
        if ctx.input(|i| i.viewport().close_requested()) {
            let _ = self.command_tx.send(PlayerCommand::Stop);
        }

        // 2. Poll latest atomic UI snapshot from telemetry aggregator
        let snapshot = self.exchange.poll_latest_snapshot();

        // 3. User activity tracking for fullscreen auto-hide
        if ctx.input(|i| i.pointer.is_moving() || i.pointer.any_click()) {
            self.controls_state.on_user_activity(now);
        }

        // 4. Global keyboard shortcuts (suppressed if typing in text edit or modal dialog open)
        let has_text_focus = ctx.memory(|m| m.focused().is_some());
        if !has_text_focus && !self.host_dialog.is_open && !self.connect_dialog.is_open {
            if ctx.input(|i| i.key_pressed(egui::Key::F11)) {
                self.controls_state.is_fullscreen = !self.controls_state.is_fullscreen;
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(self.controls_state.is_fullscreen));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.controls_state.is_fullscreen {
                self.controls_state.is_fullscreen = false;
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Tab)) {
                self.sidebar_state.toggle();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
                self.shortcuts_dialog.open();
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::O)) {
                self.file_dialog.open_file_dialog();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                let _ = self.command_tx.send(PlayerCommand::TogglePause);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                let offset_ms = if ctx.input(|i| i.modifiers.shift) { -30_000 } else { -5_000 };
                let current_ms = snapshot.media_position_pts as i64 / 1_000_000;
                let target = (current_ms + offset_ms).max(0) as u64;
                self.viewport.clear_pending();
                let _ = self.command_tx.send(PlayerCommand::Seek(target));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                let offset_ms = if ctx.input(|i| i.modifiers.shift) { 30_000 } else { 5_000 };
                let current_ms = snapshot.media_position_pts as i64 / 1_000_000;
                let dur_ms = snapshot.media_duration_pts / 1_000_000;
                let target = ((current_ms + offset_ms).max(0) as u64).min(dur_ms);
                self.viewport.clear_pending();
                let _ = self.command_tx.send(PlayerCommand::Seek(target));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                let new_vol = (self.controls_state.volume.volume + 0.05).min(1.0);
                self.controls_state.volume.set_volume(new_vol);
                let _ = self.command_tx.send(PlayerCommand::SetVolume(new_vol));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                let new_vol = (self.controls_state.volume.volume - 0.05).max(0.0);
                self.controls_state.volume.set_volume(new_vol);
                let _ = self.command_tx.send(PlayerCommand::SetVolume(new_vol));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::M)) {
                self.controls_state.volume.toggle_mute();
                let _ = self.command_tx.send(PlayerCommand::ToggleMute);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::S)) {
                let _ = self.command_tx.send(PlayerCommand::Stop);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::OpenBracket)) {
                let spd = (self.controls_state.speed - 0.25).max(0.25);
                self.controls_state.speed = spd;
                let _ = self.command_tx.send(PlayerCommand::SetSpeed(spd));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::CloseBracket)) {
                let spd = (self.controls_state.speed + 0.25).min(3.0);
                self.controls_state.speed = spd;
                let _ = self.command_tx.send(PlayerCommand::SetSpeed(spd));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Backspace)) {
                self.controls_state.speed = 1.0;
                let _ = self.command_tx.send(PlayerCommand::SetSpeed(1.0));
            }
            // Audio track cycling (B)
            if ctx.input(|i| i.key_pressed(egui::Key::B)) {
                let _ = self.command_tx.send(PlayerCommand::CycleAudioTrack);
                let notif = if !self.audio_tracks.is_empty() {
                    self.audio_track_index = (self.audio_track_index + 1) % self.audio_tracks.len();
                    format!(
                        "Audio: Track {}/{} - {}",
                        self.audio_track_index + 1,
                        self.audio_tracks.len(),
                        self.audio_tracks[self.audio_track_index]
                    )
                } else {
                    "Audio: Track 1/1 - Default".to_string()
                };
                self.osd_overlay.show_notification(notif, now);
            }
            // Subtitle track cycling (V or C)
            if ctx.input(|i| i.key_pressed(egui::Key::V) || i.key_pressed(egui::Key::C)) {
                let _ = self.command_tx.send(PlayerCommand::CycleSubtitleTrack);
                let notif = if !self.subtitle_tracks.is_empty() {
                    let next_idx = match self.subtitle_track_index {
                        None => Some(0),
                        Some(idx) if idx + 1 < self.subtitle_tracks.len() => Some(idx + 1),
                        Some(_) => None,
                    };
                    self.subtitle_track_index = next_idx;
                    self.controls_state.subtitle_dialog.select_track(next_idx);
                    match next_idx {
                        Some(idx) => {
                            if idx < self.subtitle_track_data.len() {
                                self.active_subtitle_cues = self.subtitle_track_data[idx].cues.clone();
                            }
                            format!(
                                "Subtitles: [{}/{}] {}",
                                idx + 1,
                                self.subtitle_tracks.len(),
                                self.subtitle_tracks[idx]
                            )
                        }
                        None => {
                            self.active_subtitle_cues.clear();
                            "Subtitles: Off".to_string()
                        }
                    }
                } else {
                    self.active_subtitle_cues.clear();
                    "Subtitles: Off".to_string()
                };
                self.osd_overlay.show_notification(notif, now);
            }
            // Aspect ratio cycling (A)
            if ctx.input(|i| i.key_pressed(egui::Key::A)) {
                let _ = self.command_tx.send(PlayerCommand::CycleAspectRatio);
                self.aspect_ratio_index = (self.aspect_ratio_index + 1) % ASPECT_RATIO_PRESETS.len();
                let notif = format!("Aspect Ratio: {}", ASPECT_RATIO_PRESETS[self.aspect_ratio_index]);
                self.osd_overlay.show_notification(notif, now);
            }
            // Audio delay adjustment (J: -50ms, K: +50ms)
            if ctx.input(|i| i.key_pressed(egui::Key::J)) {
                self.audio_delay_ms -= 50;
                let _ = self.command_tx.send(PlayerCommand::AdjustAudioDelay(-50));
                self.osd_overlay.show_notification(format!("Audio Delay: {:+} ms", self.audio_delay_ms), now);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::K)) {
                self.audio_delay_ms += 50;
                let _ = self.command_tx.send(PlayerCommand::AdjustAudioDelay(50));
                self.osd_overlay.show_notification(format!("Audio Delay: {:+} ms", self.audio_delay_ms), now);
            }
            // Subtitle delay adjustment (Z: -50ms, X: +50ms)
            if ctx.input(|i| i.key_pressed(egui::Key::Z)) {
                self.subtitle_delay_ms -= 50;
                let _ = self.command_tx.send(PlayerCommand::AdjustSubtitleDelay(-50));
                self.osd_overlay.show_notification(format!("Subtitle Delay: {:+} ms", self.subtitle_delay_ms), now);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::X)) {
                self.subtitle_delay_ms += 50;
                let _ = self.command_tx.send(PlayerCommand::AdjustSubtitleDelay(50));
                self.osd_overlay.show_notification(format!("Subtitle Delay: {:+} ms", self.subtitle_delay_ms), now);
            }
        }

        // 5. Poll background native file picker & drag-and-drop
        if let Some(picked_file) = self.file_dialog.poll_selected_file() {
            self.load_file(picked_file);
        }
        if let Some(action) = DragDropHandler::handle_dropped_file(&ctx) {
            match action {
                DroppedFileAction::OpenMedia(path) => {
                    self.load_file(path);
                }
                DroppedFileAction::LoadSubtitle(path) => {
                    self.load_external_subtitle_file(path, now);
                }
            }
        }

        // 6. Handle sidebar dialog requests
        if self.sidebar_state.request_open_host_dialog {
            self.sidebar_state.request_open_host_dialog = false;
            self.host_dialog.open();
        }
        if self.sidebar_state.request_open_connect_dialog {
            self.sidebar_state.request_open_connect_dialog = false;
            self.connect_dialog.open();
        }

        // 7. Update video frames from SPSC ring against presentation clock
        self.viewport.update_playback(&ctx, &self.video_ring, &self.clock, now);

        // 8a. Top Update Notification Banner (rendered when an update is available and not dismissed)
        if self.controls_state.is_visible(now) {
            if let Some(ref update) = self.update_info {
                if !self.is_update_banner_dismissed {
                    egui::Panel::top("update_notification_banner")
                        .frame(
                            egui::Frame::new()
                                .fill(egui::Color32::from_rgb(15, 23, 42))
                                .inner_margin(egui::Margin::symmetric(14, 8))
                                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(30, 41, 59))),
                        )
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("🚀 Update Available: v{}", update.latest_version))
                                        .strong()
                                        .color(egui::Color32::from_rgb(56, 189, 248)),
                                );
                                ui.label(
                                    egui::RichText::new(format!("(current: v{})", env!("CARGO_PKG_VERSION")))
                                        .small()
                                        .color(egui::Color32::from_rgb(148, 163, 184)),
                                );

                                ui.add_space(8.0);

                                if ui.button(egui::RichText::new("📥 Download").strong().color(egui::Color32::WHITE)).clicked() {
                                    let target = update.download_url.as_deref().unwrap_or(&update.release_url);
                                    let _ = crate::gui::updater::open_browser_url(target);
                                    ui.ctx().open_url(egui::OpenUrl::new_tab(target));
                                }

                                if ui.button("Release Page").clicked() {
                                    let _ = crate::gui::updater::open_browser_url(&update.release_url);
                                    ui.ctx().open_url(egui::OpenUrl::new_tab(&update.release_url));
                                }

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("✖ Dismiss").clicked() {
                                        self.is_update_banner_dismissed = true;
                                    }
                                });
                            });
                        });
                }
            } else if let Some((ref msg, timestamp)) = self.update_status_toast {
                if now.duration_since(timestamp).as_secs() < 6 {
                    let mut dismiss_toast = false;
                    let toast_msg = msg.clone();
                    egui::Panel::top("update_status_toast")
                        .frame(
                            egui::Frame::new()
                                .fill(egui::Color32::from_rgb(30, 41, 59))
                                .inner_margin(egui::Margin::symmetric(14, 6)),
                        )
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(&toast_msg).color(egui::Color32::from_rgb(226, 232, 240)),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("✖").clicked() {
                                        dismiss_toast = true;
                                    }
                                });
                            });
                        });
                    if dismiss_toast {
                        self.update_status_toast = None;
                    }
                }
            }
        }

        // 8. Top Menu Bar (hidden in fullscreen if controls auto-hidden)
        if self.controls_state.is_visible(now) {
            egui::Panel::top("top_menu_bar").show(ui, |ui| {
                egui::menu::MenuBar::new().ui(ui, |ui| {
                    // --- FILE MENU ---
                    ui.menu_button("File", |ui| {
                        if ui.button("Open File... (Ctrl+O)").clicked() {
                            ui.close();
                            self.file_dialog.open_file_dialog();
                        }

                        ui.menu_button("Recent Files", |ui| {
                            if self.recent_files.recent_paths.is_empty() {
                                ui.label(egui::RichText::new("No recent files").italics().color(egui::Color32::GRAY));
                            } else {
                                let mut to_open = None;
                                for p in &self.recent_files.recent_paths {
                                    let filename = p.file_name().and_then(|n| n.to_str()).unwrap_or("Media File");
                                    if ui.button(filename).on_hover_text(p.to_string_lossy()).clicked() {
                                        to_open = Some(p.clone());
                                        ui.close();
                                    }
                                }
                                if let Some(p) = to_open {
                                    self.load_file(p);
                                }
                                ui.separator();
                                if ui.button("Clear History").clicked() {
                                    self.recent_files.clear();
                                    ui.close();
                                }
                            }
                        });

                        ui.separator();

                        if ui.button("Close Media").clicked() {
                            ui.close();
                            self.viewport.reset();
                            self.current_file = None;
                            let _ = self.command_tx.send(PlayerCommand::Stop);
                        }

                        if ui.button("Exit (Ctrl+Q)").clicked() {
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                    });

                    // --- PLAYBACK MENU ---
                    ui.menu_button("Playback", |ui| {
                        let play_pause_label = if self.clock.is_playing() { "Pause (Space)" } else { "Play (Space)" };
                        if ui.button(play_pause_label).clicked() {
                            ui.close();
                            let _ = self.command_tx.send(PlayerCommand::TogglePause);
                        }
                        if ui.button("Stop (S)").clicked() {
                            ui.close();
                            let _ = self.command_tx.send(PlayerCommand::Stop);
                        }

                        ui.separator();

                        if ui.button("Seek -5s (←)").clicked() {
                            ui.close();
                            let current_ms = snapshot.media_position_pts as i64 / 1_000_000;
                            let target = (current_ms - 5_000).max(0) as u64;
                            self.viewport.clear_pending();
                            let _ = self.command_tx.send(PlayerCommand::Seek(target));
                        }
                        if ui.button("Seek +5s (→)").clicked() {
                            ui.close();
                            let current_ms = snapshot.media_position_pts as i64 / 1_000_000;
                            let dur_ms = snapshot.media_duration_pts / 1_000_000;
                            let target = (current_ms + 5_000).max(0) as u64;
                            self.viewport.clear_pending();
                            let _ = self.command_tx.send(PlayerCommand::Seek(target.min(dur_ms)));
                        }
                        if ui.button("Seek -30s (Shift+←)").clicked() {
                            ui.close();
                            let current_ms = snapshot.media_position_pts as i64 / 1_000_000;
                            let target = (current_ms - 30_000).max(0) as u64;
                            self.viewport.clear_pending();
                            let _ = self.command_tx.send(PlayerCommand::Seek(target));
                        }
                        if ui.button("Seek +30s (Shift+→)").clicked() {
                            ui.close();
                            let current_ms = snapshot.media_position_pts as i64 / 1_000_000;
                            let dur_ms = snapshot.media_duration_pts / 1_000_000;
                            let target = (current_ms + 30_000).max(0) as u64;
                            self.viewport.clear_pending();
                            let _ = self.command_tx.send(PlayerCommand::Seek(target.min(dur_ms)));
                        }

                        ui.separator();

                        ui.menu_button("Speed", |ui| {
                            for &spd in &crate::gui::controls::SUPPORTED_SPEEDS {
                                let label = if (spd - 1.0).abs() < 1e-3 {
                                    "1.0x (Normal)".to_string()
                                } else {
                                    format!("{:.2}x", spd)
                                };
                                if ui.button(label).clicked() {
                                    ui.close();
                                    self.controls_state.speed = spd;
                                    let _ = self.command_tx.send(PlayerCommand::SetSpeed(spd));
                                }
                            }
                        });

                        ui.menu_button("Volume", |ui| {
                            let mute_label = if self.controls_state.volume.is_muted { "Unmute (M)" } else { "Mute (M)" };
                            if ui.button(mute_label).clicked() {
                                ui.close();
                                self.controls_state.volume.toggle_mute();
                                let _ = self.command_tx.send(PlayerCommand::ToggleMute);
                            }
                            if ui.button("Volume Up (+5%) (↑)").clicked() {
                                let new_vol = (self.controls_state.volume.volume + 0.05).min(1.0);
                                self.controls_state.volume.set_volume(new_vol);
                                let _ = self.command_tx.send(PlayerCommand::SetVolume(new_vol));
                            }
                            if ui.button("Volume Down (-5%) (↓)").clicked() {
                                let new_vol = (self.controls_state.volume.volume - 0.05).max(0.0);
                                self.controls_state.volume.set_volume(new_vol);
                                let _ = self.command_tx.send(PlayerCommand::SetVolume(new_vol));
                            }
                        });
                    });

                    // --- WATCH PARTY MENU ---
                    ui.menu_button("Watch Party", |ui| {
                        if ui.button("🌐 Host Watch Party Room...").clicked() {
                            ui.close();
                            self.host_dialog.open();
                        }
                        if ui.button("🔗 Connect to Watch Party...").clicked() {
                            ui.close();
                            self.connect_dialog.open();
                        }

                        ui.separator();

                        let is_in_room = self.sidebar_state.members.len() > 1
                            || self.sidebar_state.members.iter().any(|m| m.is_local && m.is_host);
                        if is_in_room {
                            if ui.button("Disconnect / Leave Room").clicked() {
                                ui.close();
                                let _ = self.command_tx.send(PlayerCommand::DisconnectParty);
                                self.sidebar_state.members = vec![crate::gui::sidebar::RoomMember::new(
                                    self.sidebar_state.local_username.clone(),
                                    true,
                                    false,
                                    0.0,
                                )];
                            }
                            ui.separator();
                        }

                        let sb_label = if self.sidebar_state.is_expanded { "Hide Sidebar (Tab)" } else { "Show Sidebar (Tab)" };
                        if ui.button(sb_label).clicked() {
                            ui.close();
                            self.sidebar_state.toggle();
                        }
                    });

                    // --- VIEW MENU ---
                    ui.menu_button("View", |ui| {
                        let fs_label = if self.controls_state.is_fullscreen { "Exit Fullscreen (F11)" } else { "Fullscreen (F11)" };
                        if ui.button(fs_label).clicked() {
                            ui.close();
                            self.controls_state.is_fullscreen = !self.controls_state.is_fullscreen;
                            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(self.controls_state.is_fullscreen));
                        }

                        ui.menu_button("Aspect Ratio", |ui| {
                            for &ar in &crate::gui::aspect_ratio::AspectRatioOverride::ALL {
                                let is_selected = self.viewport.aspect_override() == ar;
                                if ui.selectable_label(is_selected, ar.display_name()).clicked() {
                                    ui.close();
                                    self.viewport.set_aspect_override(ar);
                                    self.controls_state.aspect_ratio = ar;
                                }
                            }
                        });

                        let sb_label = if self.sidebar_state.is_expanded { "Hide Sidebar (Tab)" } else { "Show Sidebar (Tab)" };
                        if ui.button(sb_label).clicked() {
                            ui.close();
                            self.sidebar_state.toggle();
                        }
                    });

                    // --- HELP MENU ---
                    ui.menu_button("Help", |ui| {
                        if ui.button("🔄 Check for Updates...").clicked() {
                            ui.close();
                            self.update_status_toast = Some((
                                "Checking for updates...".to_string(),
                                Instant::now(),
                            ));
                            crate::gui::updater::spawn_update_check(
                                env!("CARGO_PKG_VERSION"),
                                self.local_event_tx.clone(),
                                ctx.clone(),
                            );
                        }
                        ui.separator();
                        if ui.button("⌨ Keyboard Shortcuts & Controls (F1)").clicked() {
                            ui.close();
                            self.shortcuts_dialog.open();
                        }
                        if ui.button("ℹ About Syncverse").clicked() {
                            ui.close();
                            self.about_dialog.open();
                        }
                    });

                    // Top bar media filename HUD
                    if let Some(ref path) = self.current_file {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                            ui.label(egui::RichText::new(name).small().color(egui::Color32::from_rgb(148, 163, 184)));
                        });
                    }
                });
            });
        }

        // 9. Bottom Docked Playback Controls Bar
        if self.controls_state.is_visible(now) {
            egui::Panel::bottom("playback_controls_bar").show(ui, |ui| {
                let actions = PlaybackControlsBar::show(ui, &snapshot, &mut self.controls_state, now);
                for action in actions {
                    match action {
                        ControlsBarAction::TogglePause => {
                            let _ = self.command_tx.send(PlayerCommand::TogglePause);
                        }
                        ControlsBarAction::Stop => {
                            let _ = self.command_tx.send(PlayerCommand::Stop);
                        }
                        ControlsBarAction::Seek(target_pts) => {
                            self.viewport.clear_pending();
                            let _ = self.command_tx.send(PlayerCommand::Seek(target_pts));
                        }
                        ControlsBarAction::SetVolume(vol) => {
                            let _ = self.command_tx.send(PlayerCommand::SetVolume(vol));
                        }
                        ControlsBarAction::ToggleMute => {
                            let _ = self.command_tx.send(PlayerCommand::ToggleMute);
                        }
                        ControlsBarAction::SetSpeed(spd) => {
                            let _ = self.command_tx.send(PlayerCommand::SetSpeed(spd));
                        }
                        ControlsBarAction::ToggleSidebar => {
                            self.sidebar_state.toggle();
                        }
                        ControlsBarAction::ToggleFullscreen => {
                            self.controls_state.is_fullscreen = !self.controls_state.is_fullscreen;
                            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(self.controls_state.is_fullscreen));
                        }
                        ControlsBarAction::SetAspectRatio(ar) => {
                            self.viewport.set_aspect_override(ar);
                            self.controls_state.aspect_ratio = ar;
                        }
                        ControlsBarAction::CycleAspectRatio => {
                            let new_ar = self.viewport.cycle_aspect_override();
                            self.controls_state.aspect_ratio = new_ar;
                        }
                        ControlsBarAction::OpenAudioSettings => {
                            self.controls_state.audio_dialog.open();
                        }
                        ControlsBarAction::OpenSubtitleSettings => {
                            self.controls_state.subtitle_dialog.open();
                        }
                        ControlsBarAction::OpenVideoSettings => {
                            self.controls_state.video_dialog.open();
                        }
                    }
                }
            });
        }

        // 10. Watch-Party Sidebar (Right panel when expanded, floating button when collapsed)
        // Hidden only when controls are auto-hidden in fullscreen
        if !self.controls_state.is_fullscreen || self.controls_state.is_visible(now) {
            self.sidebar_state.show(ui, &snapshot, &self.chat_records, &self.command_tx);
        }

        // 11. Central Viewport (Consumes 100% of remaining space with perfect aspect ratio)
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(egui::Color32::BLACK))
            .show(ui, |ui| {
                let action = self.viewport.show(ui);
                if action.toggle_fullscreen {
                    self.controls_state.is_fullscreen = !self.controls_state.is_fullscreen;
                    ctx.send_viewport_cmd(ViewportCommand::Fullscreen(self.controls_state.is_fullscreen));
                }
                if action.toggle_play_pause {
                    let _ = self.command_tx.send(PlayerCommand::TogglePause);
                }

                // 11a. Synchronized Subtitle Overlay (anchored strictly inside active video_rect)
                let target_video_rect = self
                    .viewport
                    .last_video_rect()
                    .or_else(|| self.viewport.compute_video_rect(ui.available_rect_before_wrap()));
                if let Some(video_rect) = target_video_rect {
                    if let Some(active_text) =
                        self.current_active_subtitle(snapshot.media_position_pts)
                    {
                        self.subtitle_overlay.render(ui, video_rect, &active_text);
                    }
                }
            });

        // 11b. On-screen Unpause Blocked Alert Banner over Viewport
        if let Some(ref banner) = self.unpause_blocked_banner.clone() {
            if snapshot.is_playing() {
                self.unpause_blocked_banner = None;
            } else {
                let mut dismiss = false;
                let is_host = self.sidebar_state.members.iter().any(|m| m.is_local && m.is_host);

                egui::Area::new(egui::Id::unique("unpause_blocked_alert_area"))
                    .anchor(Align2::CENTER_TOP, [0.0, 48.0])
                    .order(egui::Order::Foreground)
                    .show(&ctx, |ui| {
                        egui::Frame::popup(ui.style())
                            .fill(Color32::from_rgba_premultiplied(30, 20, 10, 240))
                            .stroke(Stroke::new(1.5, Color32::from_rgb(245, 158, 11)))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(egui::Margin::symmetric(16, 10))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(&banner.message)
                                            .strong()
                                            .color(Color32::from_rgb(254, 240, 138)),
                                    );

                                    if is_host {
                                        ui.add_space(8.0);
                                        if ui
                                            .button(
                                                egui::RichText::new("⚡ Force Play")
                                                    .strong()
                                                    .color(Color32::from_rgb(250, 204, 21)),
                                            )
                                            .on_hover_text("Bypass unpause gate and force start playback for all peers")
                                            .clicked()
                                        {
                                            let _ = self.command_tx.send(PlayerCommand::ForcePlay);
                                            dismiss = true;
                                        }
                                    }

                                    ui.add_space(4.0);
                                    if ui.button("✖").on_hover_text("Dismiss alert").clicked() {
                                        dismiss = true;
                                    }
                                });
                            });
                    });

                if dismiss {
                    self.unpause_blocked_banner = None;
                }
            }
        }

        // 11c. Floating OSD HUD notification banner overlay
        self.osd_overlay.show(&ctx, now);
        if self.osd_overlay.active_notification(now).is_some() {
            ctx.request_repaint();
        }

        // 12. Modal Dialogs
        self.host_dialog.show(&ctx, &mut self.sidebar_state, &self.command_tx);
        self.connect_dialog.show(&ctx, &mut self.sidebar_state, &self.command_tx);
        self.shortcuts_dialog.show(&ctx);
        self.about_dialog.show(&ctx);

        let audio_actions = self.controls_state.audio_dialog.show(&ctx);
        for action in audio_actions {
            match action {
                crate::gui::AudioConfigAction::SelectTrack(track_id) => {
                    let _ = self.command_tx.send(PlayerCommand::SelectAudioTrack(track_id));
                }
                crate::gui::AudioConfigAction::SetVolume(vol) => {
                    self.controls_state.volume.set_volume(vol);
                    let _ = self.command_tx.send(PlayerCommand::SetVolume(vol));
                }
                crate::gui::AudioConfigAction::ToggleMute => {
                    self.controls_state.volume.toggle_mute();
                    let _ = self.command_tx.send(PlayerCommand::ToggleMute);
                }
                crate::gui::AudioConfigAction::SetDelayMs(delay_ms) => {
                    self.audio_delay_ms = delay_ms;
                    let _ = self.command_tx.send(PlayerCommand::SetAudioDelay(delay_ms));
                }
            }
        }

        let video_actions = self.controls_state.video_dialog.show(&ctx);
        for action in video_actions {
            match action {
                crate::gui::VideoConfigAction::SetAspectRatio(ar) => {
                    self.viewport.set_aspect_override(ar);
                    self.controls_state.aspect_ratio = ar;
                }
                crate::gui::VideoConfigAction::SetSpeed(spd) => {
                    self.controls_state.speed = spd;
                    let _ = self.command_tx.send(PlayerCommand::SetSpeed(spd));
                }
            }
        }

        let sub_actions = self.controls_state.subtitle_dialog.show(&ctx);
        for action in sub_actions {
            match action {
                crate::gui::SubtitleConfigAction::SelectTrack(track_id) => {
                    self.subtitle_track_index = track_id;
                    if let Some(idx) = track_id {
                        if idx < self.subtitle_track_data.len() {
                            self.active_subtitle_cues = self.subtitle_track_data[idx].cues.clone();
                        }
                    } else {
                        self.active_subtitle_cues.clear();
                    }
                    let _ = self.command_tx.send(PlayerCommand::SelectSubtitleTrack(track_id));
                }
                crate::gui::SubtitleConfigAction::LoadExternal(path) => {
                    self.load_external_subtitle_file(path, now);
                }
                crate::gui::SubtitleConfigAction::SetDelayMs(delay_ms) => {
                    self.subtitle_delay_ms = delay_ms;
                    let _ = self.command_tx.send(PlayerCommand::SetSubtitleDelay(delay_ms));
                }
            }
        }

        // 13. Drag-hover visual overlay
        DragDropHandler::render_hover_overlay(&ctx);
    }
}

/// Constructs `eframe::NativeOptions` configured for Wayland/X11 and Windows.
pub fn create_native_options() -> eframe::NativeOptions {
    let viewport = ViewportBuilder::default()
        .with_title("Syncverse — Desktop Media Player")
        .with_inner_size([1280.0, 720.0])
        .with_min_inner_size([640.0, 360.0])
        .with_resizable(true)
        .with_active(true)
        .with_drag_and_drop(true);

    #[cfg(target_os = "linux")]
    let viewport = viewport.with_app_id("syncverse");

    eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    }
}

/// Runs the native desktop graphical player with explicit subsystems.
pub fn run_desktop_player(
    initial_file: Option<PathBuf>,
    exchange: Arc<TripleBufferSnapshotExchange>,
    video_ring: Arc<SpscRingBuffer<VideoFrameRGBA, 32>>,
    clock: Arc<MasterClock>,
    command_tx: Sender<PlayerCommand>,
    event_rx: Receiver<UiEvent>,
) -> Result<(), eframe::Error> {
    let native_options = create_native_options();

    eframe::run_native(
        "Syncverse — Desktop Media Player",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(SyncplayApp::with_subsystems(
                cc,
                exchange,
                video_ring,
                clock,
                command_tx,
                event_rx,
                initial_file,
            )))
        }),
    )
}

/// Launches the desktop graphical player with automatic default subsystem initialization.
pub fn run_desktop_gui(initial_file: Option<PathBuf>) -> Result<(), eframe::Error> {
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let (command_tx, _command_rx) = channel();
    let (_event_tx, event_rx) = channel();

    run_desktop_player(
        initial_file,
        exchange,
        video_ring,
        clock,
        command_tx,
        event_rx,
    )
}
