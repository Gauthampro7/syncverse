//! Watch-party sidebar panel featuring room members, RTT, sync health badge,
//! real-time dual-timestamp chat history, and slash-command dispatching.

use std::sync::mpsc::Sender;

use egui::{
    Align2, Color32, Context, CornerRadius, FontId, ScrollArea, Stroke, StrokeKind,
    TextEdit, Ui, Vec2,
};
use syncverse_chat::{ActivePoll, ChatRecord, DispatchAction, SlashCommandDispatcher, SyncDiagnostics};

use crate::command::PlayerCommand;
use crate::overlay::{evaluate_sync_health, SyncBadgeColor};
use crate::snapshot::PlayerUiSnapshot;

/// Room member in active watch-party session with media & readiness metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomMember {
    /// Member username.
    pub username: String,
    /// Whether this is the local user.
    pub is_local: bool,
    /// Whether this user is the room host.
    pub is_host: bool,
    /// Round-trip time in milliseconds.
    pub rtt_ms: f64,
    /// Loaded media file name (base name without full path). None if no file loaded.
    pub file_name: Option<String>,
    /// Media container duration in milliseconds. None if no file loaded.
    pub duration_ms: Option<u64>,
    /// Media file size in bytes. None if no file loaded.
    pub file_size_bytes: Option<u64>,
    /// Readiness state ("Ready to Watch" vs "Not Ready").
    pub is_ready: bool,
}

impl RoomMember {
    /// Constructs a basic room member without media info.
    pub fn new(username: String, is_local: bool, is_host: bool, rtt_ms: f64) -> Self {
        Self {
            username,
            is_local,
            is_host,
            rtt_ms,
            file_name: None,
            duration_ms: None,
            file_size_bytes: None,
            is_ready: false,
        }
    }

    /// Constructs a room member with full media and readiness metadata.
    #[allow(clippy::too_many_arguments)]
    pub fn with_media(
        username: String,
        is_local: bool,
        is_host: bool,
        rtt_ms: f64,
        file_name: Option<String>,
        duration_ms: Option<u64>,
        file_size_bytes: Option<u64>,
        is_ready: bool,
    ) -> Self {
        Self {
            username,
            is_local,
            is_host,
            rtt_ms,
            file_name,
            duration_ms,
            file_size_bytes,
            is_ready,
        }
    }
}

/// Formats millisecond duration into standard `HH:MM:SS` timecode.
#[inline]
pub fn format_duration_hms(duration_ms: u64) -> String {
    let total_sec = duration_ms / 1000;
    let h = total_sec / 3600;
    let m = (total_sec % 3600) / 60;
    let s = total_sec % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

/// Evaluation result for comparing peer media against local media.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileMatchStatus {
    /// Files match within ±2000ms duration or identical case-insensitive filename.
    Match,
    /// Files differ significantly in duration and filename.
    Mismatch,
    /// Peer has no media loaded.
    NoFileLoaded,
}

impl FileMatchStatus {
    /// Returns the UI display badge label for this match status.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Match => "[Match ✔]",
            Self::Mismatch => "[⚠️ Mismatch]",
            Self::NoFileLoaded => "[⏳ No file loaded]",
        }
    }
}

/// Helper extracting filename basename across both Unix and Windows separators.
#[inline]
pub fn extract_basename(path_str: &str) -> &str {
    let s = path_str.rsplit('/').next().unwrap_or(path_str);
    s.rsplit('\\').next().unwrap_or(s)
}

/// Evaluates file match diagnostics between local and peer media.
///
/// Rules:
/// - If peer has no file name or empty string: `NoFileLoaded`.
/// - If local has no file, returns `Mismatch`.
/// - If duration matches within ±2000ms OR filename matches (case-insensitive basename): `Match`.
/// - Otherwise: `Mismatch`.
pub fn evaluate_file_match(
    local_file: Option<&str>,
    local_duration_ms: Option<u64>,
    peer_file: Option<&str>,
    peer_duration_ms: Option<u64>,
) -> FileMatchStatus {
    let peer_file = match peer_file {
        Some(f) if !f.trim().is_empty() => f.trim(),
        _ => return FileMatchStatus::NoFileLoaded,
    };

    let local_file = match local_file {
        Some(f) if !f.trim().is_empty() => f.trim(),
        _ => return FileMatchStatus::Mismatch,
    };

    let local_base = extract_basename(local_file);
    let peer_base = extract_basename(peer_file);

    let filename_match = local_base.eq_ignore_ascii_case(peer_base);

    let duration_match = match (local_duration_ms, peer_duration_ms) {
        (Some(loc), Some(peer)) => loc.abs_diff(peer) <= 2000,
        _ => false,
    };

    if filename_match || duration_match {
        FileMatchStatus::Match
    } else {
        FileMatchStatus::Mismatch
    }
}

/// Persistent state for the watch-party sidebar panel.
#[derive(Debug)]
pub struct SidebarState {
    /// Whether the sidebar panel is currently expanded.
    pub is_expanded: bool,
    /// Panel width in logical pixels.
    pub panel_width: f32,
    /// Chat and slash command text input buffer.
    pub input_text: String,
    /// Auto-scroll to bottom of chat history.
    pub autoscroll: bool,
    /// Active room poll if running.
    pub active_poll: Option<ActivePoll>,
    /// Slash command dispatcher.
    pub dispatcher: SlashCommandDispatcher,
    /// List of room members.
    pub members: Vec<RoomMember>,
    /// Local username.
    pub local_username: String,
    /// Request opening Host Party dialog.
    pub request_open_host_dialog: bool,
    /// Request opening Connect Party dialog.
    pub request_open_connect_dialog: bool,
    /// Local user readiness state ("Ready to Watch" checkbox).
    pub local_ready: bool,
    /// Host force play bypass state.
    pub force_play: bool,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            is_expanded: false,
            panel_width: 300.0,
            input_text: String::new(),
            autoscroll: true,
            active_poll: None,
            dispatcher: SlashCommandDispatcher::new(),
            members: vec![RoomMember::new(
                std::env::var("USER").unwrap_or_else(|_| "You".to_string()),
                true,
                true,
                0.0,
            )],
            local_username: std::env::var("USER").unwrap_or_else(|_| "You".to_string()),
            request_open_host_dialog: false,
            request_open_connect_dialog: false,
            local_ready: false,
            force_play: false,
        }
    }
}

impl SidebarState {
    /// Constructs a new sidebar state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggles sidebar expanded state.
    pub fn toggle(&mut self) {
        self.is_expanded = !self.is_expanded;
    }

    /// Renders the sidebar panel or collapsed toggle handle into the parent UI.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        snapshot: &PlayerUiSnapshot,
        chat_records: &[ChatRecord],
        command_tx: &Sender<PlayerCommand>,
    ) {
        if !self.is_expanded {
            let ctx = ui.ctx().clone();
            self.render_collapsed_handle(&ctx);
            return;
        }

        egui::Panel::right("watch_party_sidebar")
            .resizable(true)
            .default_size(self.panel_width)
            .size_range(240.0..=450.0)
            .show(ui, |ui| {
                self.show_content(ui, snapshot, chat_records, command_tx);
            });
    }

    /// Renders the inner content of the watch-party sidebar panel.
    pub fn show_content(
        &mut self,
        ui: &mut Ui,
        snapshot: &PlayerUiSnapshot,
        chat_records: &[ChatRecord],
        command_tx: &Sender<PlayerCommand>,
    ) {
        ui.add_space(4.0);

        // --- 1. HEADER & COLLAPSE BUTTON ---
        ui.horizontal(|ui| {
            ui.heading("Watch Party");
            ui.label(format!("({})", self.members.len()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("▶").on_hover_text("Collapse sidebar (Tab)").clicked() {
                    self.is_expanded = false;
                }
            });
        });

        // --- 1b. QUICK HOST / JOIN ROOM ACTION BAR ---
        let is_host = self.members.iter().any(|m| m.is_local && m.is_host);
        let is_in_room = self.members.len() > 1 || is_host;

        if !is_in_room {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button(egui::RichText::new("🌐 Host Room").strong().color(Color32::from_rgb(74, 222, 128)))
                    .on_hover_text("Host a synchronized watch party room on this computer")
                    .clicked()
                {
                    self.request_open_host_dialog = true;
                }

                if ui.button(egui::RichText::new("🔗 Join Room").strong().color(Color32::from_rgb(96, 165, 250)))
                    .on_hover_text("Connect to a friend's watch party room via IP address")
                    .clicked()
                {
                    self.request_open_connect_dialog = true;
                }
            });

            ui.add_space(2.0);
            ui.group(|ui| {
                ui.label(
                    egui::RichText::new("Synchronize playback across WAN links with < 10ms alignment using RFC 5905 NTP & PI PLL speed steering.")
                        .small()
                        .color(Color32::from_rgb(148, 163, 184)),
                );
            });
        } else {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if is_host {
                    ui.label(egui::RichText::new("★ HOSTING ROOM").strong().color(Color32::from_rgb(250, 204, 21)));
                } else {
                    ui.label(egui::RichText::new("● CONNECTED").strong().color(Color32::from_rgb(34, 197, 94)));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Disconnect").on_hover_text("Leave the watch party room").clicked() {
                        let _ = command_tx.send(PlayerCommand::DisconnectParty);
                        self.members = vec![RoomMember::new(
                            self.local_username.clone(),
                            true,
                            false,
                            0.0,
                        )];
                    }
                });
            });
        }

        ui.separator();

        // --- 2. SYNC HEALTH & DIAGNOSTICS HUD ---
        self.render_sync_health_hud(ui, snapshot);

        ui.separator();

        // --- 3. ROOM MEMBERS LIST ---
        self.render_members_list(ui, command_tx);

        ui.separator();

        // --- 4. ACTIVE POLL CARD (IF RUNNING) ---
        if let Some(poll) = self.active_poll.clone() {
            self.render_poll_card(ui, &poll);
            ui.separator();
        }

        // --- 5. CHAT MESSAGE HISTORY (SCROLL AREA) ---
        ui.label(egui::RichText::new("Smart Chat").strong());

        let available_height = (ui.available_height() - 40.0).max(60.0);
        ScrollArea::vertical()
            .max_height(available_height)
            .auto_shrink([false, false])
            .stick_to_bottom(self.autoscroll)
            .show(ui, |ui| {
                if chat_records.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(20.0);
                        ui.label(egui::RichText::new("No messages yet.").italics().color(Color32::GRAY));
                        ui.label("Type a message or /command below.");
                    });
                } else {
                    for record in chat_records {
                        self.render_chat_message(ui, record, command_tx);
                        ui.add_space(4.0);
                    }
                }
            });

        ui.add_space(4.0);

        // --- 6. CHAT INPUT & SLASH COMMAND PROMPT ---
        let ctx = ui.ctx().clone();
        self.render_input_bar(ui, &ctx, snapshot, command_tx);
    }

    /// Renders a sleek floating collapsed toggle button docked on the right window edge.
    pub fn render_collapsed_handle(&mut self, ctx: &Context) {
        egui::Area::new(egui::Id::unique("sidebar_collapsed_floating_handle"))
            .anchor(Align2::RIGHT_CENTER, [-4.0, 0.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let button_size = Vec2::new(30.0, 84.0);
                let (rect, response) = ui.allocate_exact_size(button_size, egui::Sense::click());
                let is_hovered = response.hovered();
                let bg_color = if is_hovered {
                    Color32::from_rgb(50, 50, 65)
                } else {
                    Color32::from_rgba_premultiplied(25, 25, 35, 190)
                };

                let painter = ui.painter();
                painter.rect_filled(rect, CornerRadius::same(6), bg_color);
                painter.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, Color32::from_rgb(70, 70, 90)), StrokeKind::Inside);

                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    "◀\n👥",
                    FontId::proportional(13.0),
                    if is_hovered { Color32::WHITE } else { Color32::from_rgb(180, 180, 200) },
                );

                if response.clicked() {
                    self.is_expanded = true;
                }
            });
    }

    /// Renders synchronization health badge, phase error, and RTT diagnostics.
    fn render_sync_health_hud(&self, ui: &mut Ui, snapshot: &PlayerUiSnapshot) {
        let phase_err_ms = snapshot.phase_error_nanos as f64 / 1_000_000.0;
        let rtt_ms = snapshot.network_rtt_nanos as f64 / 1_000_000.0;
        let speed_ratio = snapshot.pll_speed_ratio_q16 as f64 / 65536.0;

        let badge = evaluate_sync_health(phase_err_ms, 0.0);
        let (badge_text, badge_color) = match badge {
            SyncBadgeColor::Green => ("● SYNCED", Color32::from_rgb(34, 197, 94)),
            SyncBadgeColor::Amber => ("● DRIFT", Color32::from_rgb(245, 158, 11)),
            SyncBadgeColor::Red => ("● DESYNC", Color32::from_rgb(239, 68, 68)),
        };

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(badge_text).strong().color(badge_color));
            ui.label(format!("| RTT: {:.1}ms", rtt_ms));
            ui.label(format!("| e(t): {:+.1}ms", phase_err_ms));
        });

        if (speed_ratio - 1.0).abs() > 0.001 {
            ui.label(egui::RichText::new(format!("Micro-speed steering: {:.4}x", speed_ratio)).italics().color(Color32::from_rgb(245, 158, 11)));
        }
    }

    /// Renders list of room members with host/local user highlighting, readiness badges,
    /// loaded media metadata, file matching diagnostics, and host force play controls.
    fn render_members_list(&mut self, ui: &mut Ui, command_tx: &Sender<PlayerCommand>) {
        let is_host = self.members.iter().any(|m| m.is_local && m.is_host);

        ui.collapsing(format!("Members ({})", self.members.len()), |ui| {
            // 1. Interactive readiness toggle and Host bypass controls
            ui.horizontal(|ui| {
                if ui
                    .checkbox(&mut self.local_ready, "Ready to Watch")
                    .on_hover_text("Signal to room peers that you are ready to watch")
                    .changed()
                {
                    let _ = command_tx.send(PlayerCommand::SetReady(self.local_ready));
                }

                if is_host
                    && ui
                        .checkbox(&mut self.force_play, "⚡ Force Play")
                        .on_hover_text("Bypass unpause gate if peers are idle or unready")
                        .changed()
                {
                    let _ = command_tx.send(PlayerCommand::SetForcePlay(self.force_play));
                }
            });

            ui.add_space(4.0);

            // Find local member to compare against
            let local_member = self.members.iter().find(|m| m.is_local).cloned();

            for member in &self.members {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        if member.is_host {
                            ui.label(egui::RichText::new("★").color(Color32::from_rgb(250, 204, 21)));
                        } else {
                            ui.label("👤");
                        }

                        if member.is_local {
                            ui.label(
                                egui::RichText::new(format!("{} (You)", member.username))
                                    .strong()
                                    .color(Color32::from_rgb(96, 165, 250)),
                            );
                        } else {
                            ui.label(&member.username);
                        }

                        // Readiness badge: [✓ Ready] (green) vs [⏳ Not Ready] (amber)
                        if member.is_ready {
                            ui.label(
                                egui::RichText::new("[✓ Ready]")
                                    .color(Color32::from_rgb(34, 197, 94))
                                    .small(),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new("[⏳ Not Ready]")
                                    .color(Color32::from_rgb(245, 158, 11))
                                    .small(),
                            );
                        }

                        if member.rtt_ms > 0.0 {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    egui::RichText::new(format!("{:.0}ms", member.rtt_ms))
                                        .small()
                                        .color(Color32::GRAY),
                                );
                            });
                        }
                    });

                    // Media status & diagnostic badge row
                    ui.horizontal(|ui| {
                        if let Some(ref fname) = member.file_name {
                            let dur_str = member
                                .duration_ms
                                .map(format_duration_hms)
                                .unwrap_or_else(|| "--:--:--".to_string());
                            ui.label(
                                egui::RichText::new(format!("📄 {} ({})", fname, dur_str))
                                    .small()
                                    .color(Color32::from_rgb(203, 213, 225)),
                            );

                            // Compare peer media against local media
                            if !member.is_local {
                                if let Some(ref local) = local_member {
                                    let match_st = evaluate_file_match(
                                        local.file_name.as_deref(),
                                        local.duration_ms,
                                        Some(fname.as_str()),
                                        member.duration_ms,
                                    );
                                    match match_st {
                                        FileMatchStatus::Match => {
                                            ui.label(
                                                egui::RichText::new("[Match ✔]")
                                                    .small()
                                                    .strong()
                                                    .color(Color32::from_rgb(34, 197, 94)),
                                            );
                                        }
                                        FileMatchStatus::Mismatch => {
                                            ui.label(
                                                egui::RichText::new("[⚠️ Mismatch]")
                                                    .small()
                                                    .strong()
                                                    .color(Color32::from_rgb(245, 158, 11)),
                                            );
                                        }
                                        FileMatchStatus::NoFileLoaded => {
                                            ui.label(
                                                egui::RichText::new("[⏳ No file loaded]")
                                                    .small()
                                                    .color(Color32::from_rgb(148, 163, 184)),
                                            );
                                        }
                                    }
                                }
                            }
                        } else {
                            ui.label(
                                egui::RichText::new("[⏳ No file loaded]")
                                    .small()
                                    .color(Color32::from_rgb(148, 163, 184)),
                            );
                        }
                    });
                });
            }
        });
    }

    /// Renders an active voting poll card with vote options.
    fn render_poll_card(&mut self, ui: &mut Ui, poll: &ActivePoll) {
        ui.group(|ui| {
            ui.label(egui::RichText::new("📊 Active Poll").strong().color(Color32::from_rgb(56, 189, 248)));
            ui.label(&poll.question);
            for (idx, opt) in poll.options.iter().enumerate() {
                let votes = poll.votes.get(idx).copied().unwrap_or(0);
                if ui.button(format!("{} ({} votes)", opt, votes)).clicked() {
                    if let Some(p) = &mut self.active_poll {
                        if let Some(v) = p.votes.get_mut(idx) {
                            *v += 1;
                        }
                    }
                }
            }
        });
    }

    /// Renders an individual chat message record with dual timestamps.
    fn render_chat_message(
        &self,
        ui: &mut Ui,
        record: &ChatRecord,
        command_tx: &Sender<PlayerCommand>,
    ) {
        ui.horizontal_wrapped(|ui| {
            // 1. Wall-clock timestamp t_wall: HH:MM:SS
            let wall_sec = (record.wall_timestamp_utc_ms / 1000) % 86400;
            let wall_h = wall_sec / 3600;
            let wall_m = (wall_sec % 3600) / 60;
            let wall_s = wall_sec % 60;
            ui.label(
                egui::RichText::new(format!("{:02}:{:02}:{:02}", wall_h, wall_m, wall_s))
                    .small()
                    .monospace()
                    .color(Color32::from_rgb(120, 120, 135)),
            );

            // 2. Media presentation timestamp t_media: [MM:SS] (Interactive button)
            if record.media_timestamp_ms != syncverse_chat::UNANCHORED_MEDIA_PTS {
                let m_sec = record.media_timestamp_ms / 1000;
                let m_min = m_sec / 60;
                let m_s = m_sec % 60;
                let pts_text = format!("[{:02}:{:02}]", m_min, m_s);

                let btn = ui.add(
                    egui::Button::new(
                        egui::RichText::new(pts_text)
                            .small()
                            .monospace()
                            .color(Color32::from_rgb(56, 189, 248)),
                    )
                    .frame(false),
                );

                if btn.on_hover_text("Click to seek to this timestamp").clicked() {
                    let _ = command_tx.send(PlayerCommand::Seek(record.media_timestamp_ms));
                }
            } else {
                ui.label(
                    egui::RichText::new("[--:--]")
                        .small()
                        .monospace()
                        .color(Color32::from_rgb(90, 90, 100)),
                );
            }

            // 3. Sender username
            let is_me = record.sender_name == self.local_username;
            let name_color = if is_me {
                Color32::from_rgb(96, 165, 250)
            } else {
                Color32::from_rgb(220, 220, 230)
            };
            ui.label(egui::RichText::new(format!("{}:", record.sender_name)).strong().color(name_color));

            // 4. Message payload
            let text = String::from_utf8_lossy(&record.payload);
            match record.record_type {
                1 => {
                    ui.label(egui::RichText::new(text.as_ref()).color(Color32::WHITE));
                }
                2 => {
                    ui.label(egui::RichText::new(text.as_ref()).color(Color32::from_rgb(250, 204, 21)));
                }
                3 => {
                    ui.label(
                        egui::RichText::new(format!("🔖 Bookmark: {}", text))
                            .italics()
                            .color(Color32::from_rgb(52, 211, 153)),
                    );
                }
                _ => {
                    ui.label(egui::RichText::new(text.as_ref()).italics().color(Color32::from_rgb(167, 139, 250)));
                }
            }
        });
    }

    /// Renders the bottom chat and slash-command input bar.
    fn render_input_bar(
        &mut self,
        ui: &mut Ui,
        ctx: &Context,
        snapshot: &PlayerUiSnapshot,
        command_tx: &Sender<PlayerCommand>,
    ) {
        ui.horizontal(|ui| {
            let input_response = ui.add_sized(
                [ui.available_width() - 55.0, 28.0],
                TextEdit::singleline(&mut self.input_text)
                    .hint_text("Type chat or /command...")
                    .font(egui::TextStyle::Body),
            );

            let enter_pressed = input_response.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter));
            let send_clicked = ui.button("Send").clicked();

            if (enter_pressed || send_clicked) && !self.input_text.trim().is_empty() {
                let msg = self.input_text.trim().to_string();
                self.input_text.clear();
                input_response.request_focus();

                self.handle_message_submit(msg, snapshot, command_tx);
            }
        });
    }

    /// Dispatches submitted text as either a slash command or a room chat message.
    fn handle_message_submit(
        &mut self,
        text: String,
        snapshot: &PlayerUiSnapshot,
        command_tx: &Sender<PlayerCommand>,
    ) {
        if text.starts_with('/') {
            let current_pts_ms = snapshot.media_position_pts / 1_000_000;
            self.dispatcher.set_sync_diagnostics(SyncDiagnostics {
                rtt_ms: snapshot.network_rtt_nanos as f64 / 1_000_000.0,
                clock_offset_ms: snapshot.clock_offset_nanos as f64 / 1_000_000.0,
                speed_multiplier: snapshot.pll_speed_ratio_q16 as f64 / 65536.0,
                peer_count: snapshot.peer_count as usize,
                current_pts_ms,
                is_paused: snapshot.playback_state == 0,
                pll_zone: match evaluate_sync_health(snapshot.phase_error_nanos as f64 / 1_000_000.0, 0.0) {
                    SyncBadgeColor::Green => "Deadband".to_string(),
                    SyncBadgeColor::Amber => "MicroSpeed".to_string(),
                    SyncBadgeColor::Red => "HardSeek".to_string(),
                },
            });

            match self.dispatcher.dispatch(&text, Some(current_pts_ms)) {
                Ok(action) => match action {
                    DispatchAction::Seek { target_pts_ms } => {
                        let _ = command_tx.send(PlayerCommand::Seek(target_pts_ms));
                    }
                    DispatchAction::Bookmark(_) | DispatchAction::Clip(_) => {
                        let _ = command_tx.send(PlayerCommand::ExecuteSlashCommand(text));
                    }
                    DispatchAction::Poll(poll) => {
                        self.active_poll = Some(poll);
                        let _ = command_tx.send(PlayerCommand::ExecuteSlashCommand(text));
                    }
                    DispatchAction::SyncStatus { .. } => {
                        let _ = command_tx.send(PlayerCommand::ExecuteSlashCommand(text));
                    }
                },
                Err(_) => {
                    let _ = command_tx.send(PlayerCommand::ExecuteSlashCommand(text));
                }
            }
        } else {
            let _ = command_tx.send(PlayerCommand::SendChatMessage(text));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_hms() {
        assert_eq!(format_duration_hms(0), "00:00:00");
        assert_eq!(format_duration_hms(5_000), "00:00:05");
        assert_eq!(format_duration_hms(65_000), "00:01:05");
        assert_eq!(format_duration_hms(3_665_000), "01:01:05");
        assert_eq!(format_duration_hms(36_000_000), "10:00:00");
    }

    #[test]
    fn test_evaluate_file_match_no_peer_file() {
        assert_eq!(
            evaluate_file_match(Some("movie.mkv"), Some(120_000), None, None),
            FileMatchStatus::NoFileLoaded
        );
        assert_eq!(
            evaluate_file_match(Some("movie.mkv"), Some(120_000), Some(""), None),
            FileMatchStatus::NoFileLoaded
        );
        assert_eq!(
            evaluate_file_match(Some("movie.mkv"), Some(120_000), Some("   "), None),
            FileMatchStatus::NoFileLoaded
        );
    }

    #[test]
    fn test_evaluate_file_match_duration_within_tolerance() {
        // Exact match
        assert_eq!(
            evaluate_file_match(
                Some("movie.mkv"),
                Some(120_000),
                Some("other.mkv"),
                Some(120_000)
            ),
            FileMatchStatus::Match
        );

        // Within +2000ms
        assert_eq!(
            evaluate_file_match(
                Some("movie.mkv"),
                Some(120_000),
                Some("different_name.mp4"),
                Some(122_000)
            ),
            FileMatchStatus::Match
        );

        // Within -2000ms
        assert_eq!(
            evaluate_file_match(
                Some("movie.mkv"),
                Some(120_000),
                Some("different_name.mp4"),
                Some(118_000)
            ),
            FileMatchStatus::Match
        );
    }

    #[test]
    fn test_evaluate_file_match_filename_basename_match() {
        // Case-insensitive basename match with different paths and different durations
        assert_eq!(
            evaluate_file_match(
                Some("/home/user/Videos/Big_Buck_Bunny.mkv"),
                Some(600_000),
                Some("C:\\Users\\Friend\\Downloads\\big_buck_bunny.mkv"),
                Some(900_000)
            ),
            FileMatchStatus::Match
        );

        assert_eq!(
            evaluate_file_match(
                Some("EPISODE_01.MP4"),
                None,
                Some("episode_01.mp4"),
                None
            ),
            FileMatchStatus::Match
        );
    }

    #[test]
    fn test_evaluate_file_match_mismatch() {
        // Different filename AND duration diff > 2000ms
        assert_eq!(
            evaluate_file_match(
                Some("movie_a.mkv"),
                Some(120_000),
                Some("movie_b.mkv"),
                Some(125_000)
            ),
            FileMatchStatus::Mismatch
        );

        // Local has no file loaded
        assert_eq!(
            evaluate_file_match(None, None, Some("movie_b.mkv"), Some(125_000)),
            FileMatchStatus::Mismatch
        );
    }

    #[test]
    fn test_evaluate_file_match_overflow_safety() {
        // Extreme duration values must not panic on subtraction and must not falsely match
        assert_eq!(
            evaluate_file_match(
                Some("movie_a.mkv"),
                Some(u64::MAX),
                Some("movie_b.mkv"),
                Some(0)
            ),
            FileMatchStatus::Mismatch
        );

        assert_eq!(
            evaluate_file_match(
                Some("movie_a.mkv"),
                Some(0),
                Some("movie_b.mkv"),
                Some(1u64 << 63)
            ),
            FileMatchStatus::Mismatch
        );
    }

    #[test]
    fn test_room_member_constructors() {
        let member = RoomMember::new("Alice".to_string(), true, true, 12.5);
        assert_eq!(member.username, "Alice");
        assert!(member.is_local);
        assert!(member.is_host);
        assert_eq!(member.rtt_ms, 12.5);
        assert_eq!(member.file_name, None);
        assert_eq!(member.duration_ms, None);
        assert_eq!(member.file_size_bytes, None);
        assert!(!member.is_ready);

        let member_media = RoomMember::with_media(
            "Bob".to_string(),
            false,
            false,
            35.0,
            Some("video.mp4".to_string()),
            Some(600_000),
            Some(1024 * 1024 * 50),
            true,
        );
        assert_eq!(member_media.file_name.as_deref(), Some("video.mp4"));
        assert_eq!(member_media.duration_ms, Some(600_000));
        assert!(member_media.is_ready);
    }
}
