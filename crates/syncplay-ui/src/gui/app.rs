//! Main desktop graphical player application and eframe lifecycle coordinator.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

use egui::{Align2, Color32, Context, CornerRadius, Stroke, ViewportBuilder, ViewportCommand};
use syncplay_chat::ChatRecord;
use syncplay_clock::{MasterClock, PlaybackState, SpscRingBuffer};
use syncplay_codecs::VideoFrameRGBA;

use crate::command::PlayerCommand;
use crate::event::UiEvent;
use crate::gui::controls::{ControlsBarAction, ControlsBarState, PlaybackControlsBar};
use crate::gui::dialogs::{AboutDialog, ConnectPartyDialog, HostPartyDialog, ShortcutsDialog};
use crate::gui::drag_drop::DragDropHandler;
use crate::gui::file_dialog::FileDialogManager;
use crate::gui::recent_files::RecentFilesManager;
use crate::gui::sidebar::SidebarState;
use crate::gui::viewport::VideoViewport;
use crate::snapshot::TripleBufferSnapshotExchange;

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
}

impl SyncplayApp {
    /// Constructs a new `SyncplayApp` instance.
    pub fn new(
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
        };

        if let Some(path) = initial_file {
            app.load_file(path);
        }

        app
    }

    /// Loads and opens a media file.
    pub fn load_file(&mut self, path: PathBuf) {
        self.viewport.reset();
        self.recent_files.add_file(&path);
        let _ = self.command_tx.send(PlayerCommand::OpenFile(path.clone()));
        self.current_file = Some(path);
        self.sidebar_state.local_ready = true;
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
                            format!("Syncplay-Alt v{} is up to date!", current_version),
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
                let _ = self.command_tx.send(PlayerCommand::Seek(target));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                let offset_ms = if ctx.input(|i| i.modifiers.shift) { 30_000 } else { 5_000 };
                let current_ms = snapshot.media_position_pts as i64 / 1_000_000;
                let dur_ms = snapshot.media_duration_pts / 1_000_000;
                let target = ((current_ms + offset_ms).max(0) as u64).min(dur_ms);
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
        }

        // 5. Poll background native file picker & drag-and-drop
        if let Some(picked_file) = self.file_dialog.poll_selected_file() {
            self.load_file(picked_file);
        }
        if let Some(dropped) = DragDropHandler::handle_dropped_file(&ctx) {
            self.load_file(dropped);
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
                            let _ = self.command_tx.send(PlayerCommand::Seek(target));
                        }
                        if ui.button("Seek +5s (→)").clicked() {
                            ui.close();
                            let current_ms = snapshot.media_position_pts as i64 / 1_000_000;
                            let dur_ms = snapshot.media_duration_pts / 1_000_000;
                            let target = (current_ms + 5_000).max(0) as u64;
                            let _ = self.command_tx.send(PlayerCommand::Seek(target.min(dur_ms)));
                        }
                        if ui.button("Seek -30s (Shift+←)").clicked() {
                            ui.close();
                            let current_ms = snapshot.media_position_pts as i64 / 1_000_000;
                            let target = (current_ms - 30_000).max(0) as u64;
                            let _ = self.command_tx.send(PlayerCommand::Seek(target));
                        }
                        if ui.button("Seek +30s (Shift+→)").clicked() {
                            ui.close();
                            let current_ms = snapshot.media_position_pts as i64 / 1_000_000;
                            let dur_ms = snapshot.media_duration_pts / 1_000_000;
                            let target = (current_ms + 30_000).max(0) as u64;
                            let _ = self.command_tx.send(PlayerCommand::Seek(target.min(dur_ms)));
                        }

                        ui.separator();

                        ui.menu_button("Speed", |ui| {
                            for &spd in &[0.5f64, 0.75, 1.0, 1.25, 1.5, 2.0] {
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
                        if ui.button("ℹ About Syncplay-Alt").clicked() {
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

        // 12. Modal Dialogs
        self.host_dialog.show(&ctx, &mut self.sidebar_state, &self.command_tx);
        self.connect_dialog.show(&ctx, &mut self.sidebar_state, &self.command_tx);
        self.shortcuts_dialog.show(&ctx);
        self.about_dialog.show(&ctx);

        // 13. Drag-hover visual overlay
        DragDropHandler::render_hover_overlay(&ctx);
    }
}

/// Constructs `eframe::NativeOptions` configured for Wayland/X11 and Windows.
pub fn create_native_options() -> eframe::NativeOptions {
    let viewport = ViewportBuilder::default()
        .with_title("Syncplay-Alt — Desktop Media Player")
        .with_inner_size([1280.0, 720.0])
        .with_min_inner_size([640.0, 360.0])
        .with_resizable(true)
        .with_active(true)
        .with_drag_and_drop(true);

    #[cfg(target_os = "linux")]
    let viewport = viewport.with_app_id("syncplay-alt");

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
        "Syncplay-Alt — Desktop Media Player",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(SyncplayApp::new(
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
