//! Modal dialogs for watch-party hosting, connecting, keyboard shortcuts, audio/video/subtitle configuration, and engine information.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use egui::{Align2, Color32, Context, RichText, Vec2};

use crate::command::PlayerCommand;
use crate::gui::aspect_ratio::AspectRatioOverride;
use crate::gui::sidebar::{RoomMember, SidebarState};

/// Queries the local network interface IP address without external dependencies.
pub fn get_local_lan_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

/// Modal dialog for hosting a synchronized watch-party room.
#[derive(Debug, Clone)]
pub struct HostPartyDialog {
    /// Whether the dialog window is open.
    pub is_open: bool,
    /// Port number to bind the room server to.
    pub port: u16,
    /// Room name identifier.
    pub room_name: String,
    /// Username for the local host.
    pub username: String,
    /// Detected local LAN IP address.
    pub local_ip: String,
    /// Optional room password for ChaCha20-Poly1305 E2E encryption.
    pub password: String,
    /// Whether to attempt UPnP automatic port-forwarding on router.
    pub enable_upnp: bool,
    /// Transient status message (e.g. "Copied!").
    pub status_message: Option<String>,
}

impl Default for HostPartyDialog {
    fn default() -> Self {
        Self {
            is_open: false,
            port: 18242,
            room_name: "Watch Party".to_string(),
            username: std::env::var("USER").unwrap_or_else(|_| "Host".to_string()),
            local_ip: get_local_lan_ip(),
            password: String::new(),
            enable_upnp: true,
            status_message: None,
        }
    }
}

impl HostPartyDialog {
    /// Opens the dialog and refreshes the local LAN IP address.
    pub fn open(&mut self) {
        self.is_open = true;
        self.local_ip = get_local_lan_ip();
        self.status_message = None;
    }

    /// Renders the Host Party modal dialog if open.
    pub fn show(
        &mut self,
        ctx: &Context,
        sidebar: &mut SidebarState,
        command_tx: &Sender<PlayerCommand>,
    ) {
        if !self.is_open {
            return;
        }

        let mut is_open = self.is_open;
        egui::Window::new("🌐 Host Watch Party Room")
            .open(&mut is_open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new("Host a deterministic clock-synchronized watch party.").italics().color(Color32::from_rgb(156, 163, 175)));
                ui.separator();

                egui::Grid::new("host_party_grid")
                    .num_columns(2)
                    .spacing([12.0, 10.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("Room Name:").strong());
                        ui.text_edit_singleline(&mut self.room_name);
                        ui.end_row();

                        ui.label(RichText::new("Server Port:").strong());
                        ui.add(egui::DragValue::new(&mut self.port).range(1024..=65535));
                        ui.end_row();

                        ui.label(RichText::new("Your Username:").strong());
                        ui.text_edit_singleline(&mut self.username);
                        ui.end_row();

                        ui.label(RichText::new("Room Password:").strong());
                        ui.add(egui::TextEdit::singleline(&mut self.password).password(true))
                            .on_hover_text("Optional password for ChaCha20-Poly1305 end-to-end encryption");
                        ui.end_row();
                    });

                ui.add_space(4.0);
                ui.checkbox(&mut self.enable_upnp, "Enable UPnP automatic port-forwarding on router")
                    .on_hover_text("Allows WAN friends to connect automatically without manual router port-forwarding");

                ui.add_space(8.0);
                ui.group(|ui| {
                    ui.label(RichText::new("Your Connection Addresses:").strong().color(Color32::from_rgb(56, 189, 248)));
                    ui.horizontal(|ui| {
                        let lan_addr = format!("{}:{}", self.local_ip, self.port);
                        ui.label(format!("LAN / Wi-Fi / VPN: {}", lan_addr));
                        if ui.button("📋 Copy").on_hover_text("Copy LAN address to clipboard").clicked() {
                            ui.ctx().copy_text(lan_addr);
                            self.status_message = Some("Copied LAN address to clipboard!".to_string());
                        }
                    });

                    ui.horizontal(|ui| {
                        let loopback_addr = format!("127.0.0.1:{}", self.port);
                        ui.label(format!("Localhost: {}", loopback_addr));
                        if ui.button("📋 Copy").on_hover_text("Copy localhost address to clipboard").clicked() {
                            ui.ctx().copy_text(loopback_addr);
                            self.status_message = Some("Copied localhost address to clipboard!".to_string());
                        }
                    });

                    ui.label(
                        RichText::new("Share this address with friends so they can join your synchronized room.")
                            .small()
                            .color(Color32::from_rgb(148, 163, 184)),
                    );
                });

                if let Some(ref msg) = self.status_message {
                    ui.add_space(4.0);
                    ui.label(RichText::new(msg).color(Color32::from_rgb(74, 222, 128)));
                }

                ui.add_space(10.0);
                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.is_open = false;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new("★ Start Hosting Room").strong().color(Color32::from_rgb(34, 197, 94))).clicked() {
                            let pwd = if self.password.trim().is_empty() {
                                None
                            } else {
                                Some(self.password.trim().to_string())
                            };
                            let _ = command_tx.send(PlayerCommand::HostParty {
                                port: self.port,
                                room: self.room_name.clone(),
                                username: self.username.clone(),
                                password: pwd,
                                enable_upnp: self.enable_upnp,
                            });

                            sidebar.members = vec![RoomMember::new(
                                self.username.clone(),
                                true,
                                true,
                                0.0,
                            )];
                            sidebar.local_username = self.username.clone();

                            self.is_open = false;
                        }
                    });
                });
            });

        self.is_open = is_open;
    }
}

/// Modal dialog for connecting to a remote watch-party host.
#[derive(Debug, Clone)]
pub struct ConnectPartyDialog {
    /// Whether the dialog window is open.
    pub is_open: bool,
    /// Target host address string (e.g. "127.0.0.1:18242" or "192.168.1.10:18242").
    pub host_addr: String,
    /// Room name identifier.
    pub room_name: String,
    /// Username for the local client.
    pub username: String,
    /// Optional room password for ChaCha20-Poly1305 E2E encryption.
    pub password: String,
    /// Error message on validation failure.
    pub error_msg: Option<String>,
}

impl Default for ConnectPartyDialog {
    fn default() -> Self {
        Self {
            is_open: false,
            host_addr: "127.0.0.1:18242".to_string(),
            room_name: "Watch Party".to_string(),
            username: std::env::var("USER").unwrap_or_else(|_| "Peer".to_string()),
            password: String::new(),
            error_msg: None,
        }
    }
}

impl ConnectPartyDialog {
    /// Opens the connection dialog.
    pub fn open(&mut self) {
        self.is_open = true;
        self.error_msg = None;
    }

    /// Renders the Connect Party modal dialog if open.
    pub fn show(
        &mut self,
        ctx: &Context,
        sidebar: &mut SidebarState,
        command_tx: &Sender<PlayerCommand>,
    ) {
        if !self.is_open {
            return;
        }

        let mut is_open = self.is_open;
        egui::Window::new("🔗 Connect to Watch Party")
            .open(&mut is_open)
            .collapsible(false)
            .resizable(false)
            .default_width(400.0)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new("Connect to a friend's watch party room.").italics().color(Color32::from_rgb(156, 163, 175)));
                ui.separator();

                egui::Grid::new("connect_party_grid")
                    .num_columns(2)
                    .spacing([12.0, 10.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("Host Address:").strong());
                        ui.text_edit_singleline(&mut self.host_addr)
                            .on_hover_text("e.g. 192.168.1.50:18242 or 127.0.0.1:18242");
                        ui.end_row();

                        ui.label(RichText::new("Your Username:").strong());
                        ui.text_edit_singleline(&mut self.username);
                        ui.end_row();

                        ui.label(RichText::new("Room Name:").strong());
                        ui.text_edit_singleline(&mut self.room_name);
                        ui.end_row();

                        ui.label(RichText::new("Room Password:").strong());
                        ui.add(egui::TextEdit::singleline(&mut self.password).password(true))
                            .on_hover_text("Optional password for ChaCha20-Poly1305 end-to-end encryption");
                        ui.end_row();
                    });

                if let Some(ref err) = self.error_msg {
                    ui.add_space(6.0);
                    ui.label(RichText::new(err).color(Color32::from_rgb(239, 68, 68)));
                }

                ui.add_space(10.0);
                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.is_open = false;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new("🔗 Connect to Room").strong().color(Color32::from_rgb(59, 130, 246))).clicked() {
                            if self.host_addr.trim().is_empty() {
                                self.error_msg = Some("Please provide a host address.".to_string());
                                return;
                            }

                            let pwd = if self.password.trim().is_empty() {
                                None
                            } else {
                                Some(self.password.trim().to_string())
                            };
                            let _ = command_tx.send(PlayerCommand::ConnectParty {
                                addr: self.host_addr.clone(),
                                room: self.room_name.clone(),
                                username: self.username.clone(),
                                password: pwd,
                            });

                            sidebar.members = vec![RoomMember::new(
                                self.username.clone(),
                                true,
                                false,
                                0.0,
                            )];
                            sidebar.local_username = self.username.clone();

                            self.is_open = false;
                        }
                    });
                });
            });

        self.is_open = is_open;
    }
}

/// Modal dialog displaying keyboard shortcuts, controls, and slash commands.
#[derive(Debug, Default, Clone)]
pub struct ShortcutsDialog {
    /// Whether the shortcuts cheat sheet window is open.
    pub is_open: bool,
}

impl ShortcutsDialog {
    /// Opens the shortcuts cheat sheet.
    pub fn open(&mut self) {
        self.is_open = true;
    }

    /// Renders the keyboard shortcuts modal window.
    pub fn show(&mut self, ctx: &Context) {
        if !self.is_open {
            return;
        }

        let mut is_open = self.is_open;
        egui::Window::new("⌨ Keyboard Shortcuts & Controls Guide")
            .open(&mut is_open)
            .resizable(true)
            .default_width(560.0)
            .default_height(480.0)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Media Playback & Navigation");
                    egui::Grid::new("nav_shortcuts_grid")
                        .num_columns(2)
                        .spacing([16.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Space / Click Viewport").monospace().strong());
                            ui.label("Toggle Play / Pause");
                            ui.end_row();

                            ui.label(RichText::new("F / F11 / Double-Click").monospace().strong());
                            ui.label("Toggle Fullscreen Mode");
                            ui.end_row();

                            ui.label(RichText::new("Esc").monospace().strong());
                            ui.label("Close Modal Dialog / Exit Fullscreen");
                            ui.end_row();

                            ui.label(RichText::new("Ctrl + O").monospace().strong());
                            ui.label("Open Media File (Native File Picker)");
                            ui.end_row();

                            ui.label(RichText::new("Ctrl + Q").monospace().strong());
                            ui.label("Quit Application");
                            ui.end_row();

                            ui.label(RichText::new("Mouse Wheel / Scroll").monospace().strong());
                            ui.label("Adjust Volume (±5%)");
                            ui.end_row();

                            ui.label(RichText::new("Tab").monospace().strong());
                            ui.label("Toggle Watch-Party Sidebar & Chat");
                            ui.end_row();

                            ui.label(RichText::new("Left Arrow (←)").monospace().strong());
                            ui.label("Seek Backward 5 seconds");
                            ui.end_row();

                            ui.label(RichText::new("Right Arrow (→)").monospace().strong());
                            ui.label("Seek Forward 5 seconds");
                            ui.end_row();

                            ui.label(RichText::new("Shift + Left Arrow").monospace().strong());
                            ui.label("Seek Backward 30 seconds");
                            ui.end_row();

                            ui.label(RichText::new("Shift + Right Arrow").monospace().strong());
                            ui.label("Seek Forward 30 seconds");
                            ui.end_row();

                            ui.label(RichText::new("S").monospace().strong());
                            ui.label("Stop Playback & Rewind to 00:00:00");
                            ui.end_row();

                            ui.label(RichText::new("[ / ]").monospace().strong());
                            ui.label("Decrease / Increase Playback Speed");
                            ui.end_row();

                            ui.label(RichText::new("Backspace").monospace().strong());
                            ui.label("Reset Playback Speed to 1.0x");
                            ui.end_row();
                        });

                    ui.add_space(12.0);
                    ui.heading("Audio & Volume Controls");
                    egui::Grid::new("audio_shortcuts_grid")
                        .num_columns(2)
                        .spacing([16.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Up Arrow (↑)").monospace().strong());
                            ui.label("Increase Volume (+5%)");
                            ui.end_row();

                            ui.label(RichText::new("Down Arrow (↓)").monospace().strong());
                            ui.label("Decrease Volume (-5%)");
                            ui.end_row();

                            ui.label(RichText::new("M").monospace().strong());
                            ui.label("Toggle Audio Mute");
                            ui.end_row();

                            ui.label(RichText::new("B").monospace().strong());
                            ui.label("Cycle Audio Track Sequentially");
                            ui.end_row();

                            ui.label(RichText::new("J / K").monospace().strong());
                            ui.label("Adjust Audio Delay (-50 ms / +50 ms)");
                            ui.end_row();
                        });

                    ui.add_space(12.0);
                    ui.heading("Subtitles & Aspect Ratio");
                    egui::Grid::new("subtitles_aspect_shortcuts_grid")
                        .num_columns(2)
                        .spacing([16.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("V / C").monospace().strong());
                            ui.label("Cycle Subtitle Track (Including Off)");
                            ui.end_row();

                            ui.label(RichText::new("Z / X").monospace().strong());
                            ui.label("Adjust Subtitle Delay (-50 ms / +50 ms)");
                            ui.end_row();

                            ui.label(RichText::new("A").monospace().strong());
                            ui.label("Cycle Aspect Ratio (Original, 16:9, 4:3, 21:9, 1:1, Fill)");
                            ui.end_row();
                        });

                    ui.add_space(12.0);
                    ui.heading("Watch Party Chat Slash Commands");
                    ui.label(
                        RichText::new("Type these commands into the chat bar to orchestrate watch parties:")
                            .small()
                            .italics()
                            .color(Color32::from_rgb(148, 163, 184)),
                    );

                    egui::Grid::new("slash_commands_grid")
                        .num_columns(2)
                        .spacing([16.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("/seek <time>").monospace().strong().color(Color32::from_rgb(56, 189, 248)));
                            ui.label("Synchronize all peers to timestamp (e.g. /seek 01:23:45 or /seek 450s)");
                            ui.end_row();

                            ui.label(RichText::new("/bookmark <label>").monospace().strong().color(Color32::from_rgb(56, 189, 248)));
                            ui.label("Save current timestamp bookmark with description");
                            ui.end_row();

                            ui.label(RichText::new("/clip <duration_sec>").monospace().strong().color(Color32::from_rgb(56, 189, 248)));
                            ui.label("Share a highlight clip segment with party members");
                            ui.end_row();

                            ui.label(RichText::new("/poll \"Question\" \"Option1\" \"Option2\"").monospace().strong().color(Color32::from_rgb(56, 189, 248)));
                            ui.label("Create an interactive real-time vote poll card in chat");
                            ui.end_row();

                            ui.label(RichText::new("/sync-status").monospace().strong().color(Color32::from_rgb(56, 189, 248)));
                            ui.label("Print peer RTT, clock offset (θ), and PLL micro-steering health");
                            ui.end_row();
                        });
                });
            });

        self.is_open = is_open;
    }
}

/// Modal dialog displaying architecture, version, and invariants.
#[derive(Debug, Default, Clone)]
pub struct AboutDialog {
    /// Whether the about modal is open.
    pub is_open: bool,
}

impl AboutDialog {
    /// Opens the about modal.
    pub fn open(&mut self) {
        self.is_open = true;
    }

    /// Renders the About Syncverse modal dialog.
    pub fn show(&mut self, ctx: &Context) {
        if !self.is_open {
            return;
        }

        let mut is_open = self.is_open;
        egui::Window::new("ℹ About Syncverse")
            .open(&mut is_open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.heading("Syncverse");
                    ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).color(Color32::from_rgb(96, 165, 250)));
                });
                ui.label(RichText::new("Pure-Rust Standalone Media Player & High-Precision Watch Party Engine").italics().color(Color32::from_rgb(156, 163, 175)));
                ui.separator();

                ui.label("• Standalone Architecture: Universal container demuxing (MKV, MP4, WebM, AVI, MOV, TS, FLV) and bitstream decoding (HEVC 10-bit, H.264, VP9, AV1; DTS-HD MA, AAC, Opus, FLAC, AC3) running entirely inside its own process space without external player wrappers.");
                ui.add_space(4.0);
                ui.label("• Mathematical Clock Phase Sync: RFC 5905 NTP 4-timestamp exchange, sliding-window min-RTT filter, discrete PI PLL micro-speed steering, and two-phase barrier unpause (< 10ms cross-peer alignment).");
                ui.add_space(4.0);
                ui.label("• Lock-Free Threading: Decoupled UI, demuxing, video decoding, and audio output running on isolated OS threads connected by 128B cache-line aligned SPSC ring buffers.");
                ui.add_space(4.0);
                ui.label("• Native Desktop Graphics: Hardware-accelerated presentation natively supporting Linux (Wayland & X11) and Windows via wgpu.");
                ui.add_space(4.0);
                ui.label("• In-Tree Binary Log & FTS: Crash-resilient 0xAA55 binary log with CRC32 verification and pure-Rust inverted index full-text search.");

                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        self.is_open = false;
                    }
                });
            });

        self.is_open = is_open;
    }
}

/// Metadata representation of an audio track for UI display.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioTrackItem {
    /// Unique track identifier.
    pub id: u64,
    /// Human-readable title / description tag.
    pub title: String,
    /// Language code or tag (e.g. "eng", "jpn", "spa").
    pub language: Option<String>,
    /// Codec identifier string (e.g. "aac", "opus", "flac").
    pub codec: String,
    /// Number of audio channels (1 = Mono, 2 = Stereo, 6 = 5.1).
    pub channels: Option<u32>,
    /// Audio sample rate in Hz (e.g. 48000.0).
    pub sample_rate: Option<f64>,
}

impl AudioTrackItem {
    /// Constructs a new `AudioTrackItem`.
    pub fn new(id: u64, title: impl Into<String>, codec: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            language: None,
            codec: codec.into(),
            channels: None,
            sample_rate: None,
        }
    }

    /// Formats a comprehensive descriptive label for UI menus and lists.
    pub fn display_label(&self) -> String {
        let mut parts = Vec::new();
        if !self.title.is_empty() {
            parts.push(self.title.clone());
        } else {
            parts.push(format!("Track {}", self.id));
        }
        if let Some(ref lang) = self.language {
            parts.push(format!("[{}]", lang.to_uppercase()));
        }
        if !self.codec.is_empty() {
            parts.push(self.codec.to_uppercase());
        }
        if let Some(ch) = self.channels {
            parts.push(match ch {
                1 => "Mono".to_string(),
                2 => "Stereo".to_string(),
                6 => "5.1 Surround".to_string(),
                8 => "7.1 Surround".to_string(),
                _ => format!("{} ch", ch),
            });
        }
        if let Some(sr) = self.sample_rate {
            if sr >= 1000.0 {
                parts.push(format!("{:.1} kHz", sr / 1000.0));
            } else {
                parts.push(format!("{} Hz", sr as u64));
            }
        }
        parts.join(" • ")
    }
}

/// Action emitted by audio configuration dialog upon user interaction.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioConfigAction {
    /// Switch active audio track to specified track ID.
    SelectTrack(u64),
    /// Set normalized audio volume [0.0, 1.0].
    SetVolume(f32),
    /// Toggle audio mute state.
    ToggleMute,
    /// Set audio delay sync offset in milliseconds [-5000, 5000].
    SetDelayMs(i64),
}

/// Modal dialog for configuring audio tracks, volume/mute, and audio delay sync offset.
#[derive(Debug, Clone)]
pub struct AudioConfigDialog {
    /// Whether the dialog window is open.
    pub is_open: bool,
    /// List of available audio tracks discovered from media container.
    pub tracks: Vec<AudioTrackItem>,
    /// Currently selected audio track ID.
    pub selected_track_id: Option<u64>,
    /// Normalized volume in range [0.0, 1.0].
    pub volume: f32,
    /// Whether audio output is currently muted.
    pub is_muted: bool,
    /// Audio synchronization delay / offset in milliseconds [-5000, 5000].
    pub delay_ms: i64,
}

impl Default for AudioConfigDialog {
    fn default() -> Self {
        Self {
            is_open: false,
            tracks: Vec::new(),
            selected_track_id: None,
            volume: 1.0,
            is_muted: false,
            delay_ms: 0,
        }
    }
}

impl AudioConfigDialog {
    /// Constructs a new `AudioConfigDialog`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the audio configuration dialog.
    pub fn open(&mut self) {
        self.is_open = true;
    }

    /// Closes the audio configuration dialog.
    pub fn close(&mut self) {
        self.is_open = false;
    }

    /// Sets discovered audio tracks.
    pub fn set_tracks(&mut self, tracks: Vec<AudioTrackItem>) {
        if self.selected_track_id.is_none() && !tracks.is_empty() {
            self.selected_track_id = Some(tracks[0].id);
        }
        self.tracks = tracks;
    }

    /// Selects an audio track by ID.
    pub fn select_track(&mut self, track_id: u64) {
        self.selected_track_id = Some(track_id);
    }

    /// Sets volume normalized [0.0, 1.0].
    pub fn set_volume(&mut self, vol: f32) {
        self.volume = vol.clamp(0.0, 1.0);
        if self.volume > 0.0 {
            self.is_muted = false;
        }
    }

    /// Sets mute state.
    pub fn set_muted(&mut self, muted: bool) {
        self.is_muted = muted;
    }

    /// Sets audio sync delay in milliseconds clamped to [-5000, 5000].
    pub fn set_delay_ms(&mut self, delay: i64) {
        self.delay_ms = delay.clamp(-5000, 5000);
    }

    /// Resets audio delay to 0 ms (synchronized).
    pub fn reset_delay(&mut self) {
        self.delay_ms = 0;
    }

    /// Returns effective volume (0.0 if muted, else volume).
    pub fn effective_volume(&self) -> f32 {
        if self.is_muted {
            0.0
        } else {
            self.volume
        }
    }

    /// Renders the Audio Configuration modal dialog if open.
    pub fn show(&mut self, ctx: &Context) -> Vec<AudioConfigAction> {
        let mut actions = Vec::new();
        if !self.is_open {
            return actions;
        }

        let mut is_open = self.is_open;
        egui::Window::new("🔊 Audio Settings")
            .open(&mut is_open)
            .collapsible(false)
            .resizable(true)
            .default_width(450.0)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Configure audio streams, output volume, and sync calibration.")
                        .italics()
                        .color(Color32::from_rgb(156, 163, 175)),
                );
                ui.separator();

                // 1. Audio Track Selection
                ui.heading("Audio Streams & Tracks");
                if self.tracks.is_empty() {
                    ui.group(|ui| {
                        ui.label(
                            RichText::new("Default Audio Stream (1 track detected)")
                                .color(Color32::from_rgb(226, 232, 240)),
                        );
                    });
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(140.0)
                        .show(ui, |ui| {
                            for track in &self.tracks {
                                let is_selected = self.selected_track_id == Some(track.id);
                                let label = track.display_label();
                                if ui.radio(is_selected, label).clicked() {
                                    self.selected_track_id = Some(track.id);
                                    actions.push(AudioConfigAction::SelectTrack(track.id));
                                }
                            }
                        });
                }

                ui.add_space(8.0);
                ui.separator();

                // 2. Volume & Quick Mute
                ui.heading("Volume & Output Gain");
                ui.horizontal(|ui| {
                    let mute_label = if self.is_muted { "🔇 Muted" } else { "🔊 Mute" };
                    let mute_btn = egui::Button::new(
                        RichText::new(mute_label)
                            .strong()
                            .color(if self.is_muted {
                                Color32::from_rgb(239, 68, 68)
                            } else {
                                Color32::from_rgb(226, 232, 240)
                            }),
                    );
                    if ui.add(mute_btn).clicked() {
                        self.is_muted = !self.is_muted;
                        actions.push(AudioConfigAction::ToggleMute);
                    }

                    let pct = (self.effective_volume() * 100.0).round() as u32;
                    ui.label(
                        RichText::new(format!("{:3}%", pct))
                            .monospace()
                            .strong(),
                    );

                    let mut vol = self.volume;
                    let slider = egui::Slider::new(&mut vol, 0.0..=1.0)
                        .show_value(false)
                        .trailing_fill(true);
                    if ui.add(slider).changed() {
                        self.set_volume(vol);
                        actions.push(AudioConfigAction::SetVolume(self.effective_volume()));
                    }
                });

                ui.add_space(8.0);
                ui.separator();

                // 3. Audio Delay / Sync Offset
                ui.heading("Audio Sync / Latency Offset");
                ui.label(
                    RichText::new("Shift audio earlier or later to compensate for Bluetooth/HDMI delay.")
                        .small()
                        .color(Color32::from_rgb(148, 163, 184)),
                );

                let delay_status = if self.delay_ms == 0 {
                    "0 ms (Synchronized)".to_string()
                } else if self.delay_ms > 0 {
                    format!("+{} ms (Audio Ahead of Video)", self.delay_ms)
                } else {
                    format!("{} ms (Audio Delayed behind Video)", self.delay_ms)
                };

                ui.label(
                    RichText::new(delay_status)
                        .monospace()
                        .strong()
                        .color(if self.delay_ms == 0 {
                            Color32::from_rgb(74, 222, 128)
                        } else {
                            Color32::from_rgb(250, 204, 21)
                        }),
                );

                let prev_delay = self.delay_ms;
                let slider = egui::Slider::new(&mut self.delay_ms, -5000..=5000)
                    .suffix(" ms");
                if ui.add(slider).changed() {
                    actions.push(AudioConfigAction::SetDelayMs(self.delay_ms));
                }

                ui.horizontal(|ui| {
                    if ui.button("-250 ms").clicked() {
                        self.set_delay_ms(self.delay_ms - 250);
                    }
                    if ui.button("-50 ms").clicked() {
                        self.set_delay_ms(self.delay_ms - 50);
                    }
                    if ui.button("⟲ Reset (0 ms)").on_hover_text("Reset audio sync offset to 0 ms").clicked() {
                        self.reset_delay();
                    }
                    if ui.button("+50 ms").clicked() {
                        self.set_delay_ms(self.delay_ms + 50);
                    }
                    if ui.button("+250 ms").clicked() {
                        self.set_delay_ms(self.delay_ms + 250);
                    }
                });
                if self.delay_ms != prev_delay && !actions.iter().any(|a| matches!(a, AudioConfigAction::SetDelayMs(_))) {
                    actions.push(AudioConfigAction::SetDelayMs(self.delay_ms));
                }

                ui.add_space(10.0);
                ui.separator();

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        self.is_open = false;
                    }
                });
            });

        self.is_open = is_open;
        actions
    }
}

/// Video stream metadata tags for player information display.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VideoStreamMetadata {
    /// Video frame width in pixels.
    pub width: Option<u32>,
    /// Video frame height in pixels.
    pub height: Option<u32>,
    /// Video codec identifier string (e.g. "h264", "hevc", "vp9", "av1").
    pub codec: Option<String>,
    /// Video frame rate in frames per second (fps).
    pub frame_rate: Option<f64>,
    /// Color bit depth (e.g. 8, 10, 12).
    pub bit_depth: Option<u32>,
    /// Pixel format description (e.g. "yuv420p", "yuv420p10le").
    pub pixel_format: Option<String>,
    /// Container or video timescale base in ticks per second.
    pub timescale: Option<u64>,
}

impl VideoStreamMetadata {
    /// Formatted resolution description string.
    pub fn resolution_str(&self) -> String {
        match (self.width, self.height) {
            (Some(w), Some(h)) => {
                let tag = if w >= 3840 || h >= 2160 {
                    " (4K UHD)"
                } else if w >= 2560 || h >= 1440 {
                    " (1440p QHD)"
                } else if w >= 1920 || h >= 1080 {
                    " (1080p FHD)"
                } else if w >= 1280 || h >= 720 {
                    " (720p HD)"
                } else {
                    ""
                };
                format!("{} × {}{}", w, h, tag)
            }
            _ => "Unknown".to_string(),
        }
    }

    /// Calculated aspect ratio string.
    pub fn aspect_ratio_str(&self) -> String {
        match (self.width, self.height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => {
                let ratio = w as f64 / h as f64;
                if (ratio - 16.0 / 9.0).abs() < 0.05 {
                    format!("16:9 ({:.2}:1)", ratio)
                } else if (ratio - 4.0 / 3.0).abs() < 0.05 {
                    format!("4:3 ({:.2}:1)", ratio)
                } else if (ratio - 21.0 / 9.0).abs() < 0.05 {
                    format!("21:9 ({:.2}:1)", ratio)
                } else if (ratio - 1.0).abs() < 0.05 {
                    format!("1:1 ({:.2}:1)", ratio)
                } else {
                    format!("{:.2}:1", ratio)
                }
            }
            _ => "Unknown".to_string(),
        }
    }

    /// Formatted frame rate string.
    pub fn frame_rate_str(&self) -> String {
        match self.frame_rate {
            Some(fps) if fps > 0.0 => format!("{:.2} fps", fps),
            _ => "Variable / Unknown".to_string(),
        }
    }

    /// Formatted codec and pixel format string.
    pub fn codec_str(&self) -> String {
        let mut parts = Vec::new();
        if let Some(ref c) = self.codec {
            parts.push(c.to_uppercase());
        }
        if let Some(ref pf) = self.pixel_format {
            parts.push(pf.clone());
        }
        if let Some(bd) = self.bit_depth {
            parts.push(format!("{}-bit", bd));
        }
        if parts.is_empty() {
            "Unknown".to_string()
        } else {
            parts.join(" • ")
        }
    }
}

/// Action emitted by video configuration dialog upon user interaction.
#[derive(Debug, Clone, PartialEq)]
pub enum VideoConfigAction {
    /// Set video aspect ratio override.
    SetAspectRatio(AspectRatioOverride),
    /// Set playback speed ratio.
    SetSpeed(f64),
}

/// Modal dialog for configuring aspect ratio, playback speed, and viewing video stream metadata.
#[derive(Debug, Clone)]
pub struct VideoConfigDialog {
    /// Whether the dialog window is open.
    pub is_open: bool,
    /// Selected aspect ratio mode.
    pub aspect_ratio: AspectRatioOverride,
    /// Current playback speed multiplier.
    pub speed: f64,
    /// Active video stream metadata.
    pub metadata: VideoStreamMetadata,
}

impl Default for VideoConfigDialog {
    fn default() -> Self {
        Self {
            is_open: false,
            aspect_ratio: AspectRatioOverride::Original,
            speed: 1.0,
            metadata: VideoStreamMetadata::default(),
        }
    }
}

impl VideoConfigDialog {
    /// Constructs a new `VideoConfigDialog`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the video configuration dialog.
    pub fn open(&mut self) {
        self.is_open = true;
    }

    /// Closes the video configuration dialog.
    pub fn close(&mut self) {
        self.is_open = false;
    }

    /// Sets the video stream metadata.
    pub fn set_metadata(&mut self, metadata: VideoStreamMetadata) {
        self.metadata = metadata;
    }

    /// Sets the aspect ratio override.
    pub fn set_aspect_ratio(&mut self, aspect: AspectRatioOverride) {
        self.aspect_ratio = aspect;
    }

    /// Sets the playback speed.
    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed.clamp(0.25, 4.0);
    }

    /// Renders the Video Configuration modal dialog if open.
    pub fn show(&mut self, ctx: &Context) -> Vec<VideoConfigAction> {
        let mut actions = Vec::new();
        if !self.is_open {
            return actions;
        }

        let mut is_open = self.is_open;
        egui::Window::new("🎬 Video Settings")
            .open(&mut is_open)
            .collapsible(false)
            .resizable(true)
            .default_width(460.0)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Configure aspect ratio framing, playback speed, and stream properties.")
                        .italics()
                        .color(Color32::from_rgb(156, 163, 175)),
                );
                ui.separator();

                // 1. Aspect Ratio Override
                ui.heading("Aspect Ratio Override");
                ui.horizontal_wrapped(|ui| {
                    for &mode in &AspectRatioOverride::ALL {
                        let is_selected = self.aspect_ratio == mode;
                        if ui.selectable_label(is_selected, mode.display_name()).clicked() {
                            self.aspect_ratio = mode;
                            actions.push(VideoConfigAction::SetAspectRatio(mode));
                        }
                    }
                });

                ui.add_space(8.0);
                ui.separator();

                // 2. Playback Speed Control
                ui.heading("Playback Speed");
                ui.horizontal(|ui| {
                    for &spd in &[0.5, 0.75, 1.0, 1.25, 1.5, 2.0] {
                        let label = format!("{:.2}x", spd);
                        let is_selected = (self.speed - spd).abs() < 0.01;
                        if ui.selectable_label(is_selected, label).clicked() {
                            self.speed = spd;
                            actions.push(VideoConfigAction::SetSpeed(spd));
                        }
                    }
                });

                ui.horizontal(|ui| {
                    let mut spd = self.speed;
                    let slider = egui::Slider::new(&mut spd, 0.25..=4.0)
                        .step_by(0.05)
                        .custom_formatter(|v, _| format!("{:.2}x", v))
                        .text("Custom");
                    if ui.add(slider).changed() {
                        self.speed = spd;
                        actions.push(VideoConfigAction::SetSpeed(spd));
                    }

                    if ui.button("Reset (1.0x)").clicked() {
                        self.speed = 1.0;
                        actions.push(VideoConfigAction::SetSpeed(1.0));
                    }
                });

                ui.add_space(8.0);
                ui.separator();

                // 3. Video Stream Metadata Display
                ui.heading("Video Stream Details");
                egui::Grid::new("video_stream_metadata_grid")
                    .num_columns(2)
                    .spacing([16.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Resolution:").strong());
                        ui.label(self.metadata.resolution_str());
                        ui.end_row();

                        ui.label(RichText::new("Aspect Ratio:").strong());
                        ui.label(self.metadata.aspect_ratio_str());
                        ui.end_row();

                        ui.label(RichText::new("Frame Rate:").strong());
                        ui.label(self.metadata.frame_rate_str());
                        ui.end_row();

                        ui.label(RichText::new("Codec / Format:").strong());
                        ui.label(self.metadata.codec_str());
                        ui.end_row();

                        if let Some(ts) = self.metadata.timescale {
                            ui.label(RichText::new("Timescale:").strong());
                            ui.label(format!("{} ticks/sec", ts));
                            ui.end_row();
                        }
                    });

                ui.add_space(10.0);
                ui.separator();

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        self.is_open = false;
                    }
                });
            });

        self.is_open = is_open;
        actions
    }
}

/// Metadata representation of a detected or loaded subtitle track.
#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleTrackItem {
    /// Track index / identifier.
    pub id: usize,
    /// Descriptive title or filename.
    pub title: String,
    /// Subtitle language code (e.g. "eng", "spa", "fra", "jpn").
    pub language: Option<String>,
    /// Format identifier (e.g. "SRT", "VTT", "ASS", "SSA").
    pub format: String,
    /// Whether this track was loaded from an external file.
    pub is_external: bool,
    /// Filesystem path if loaded from external file.
    pub file_path: Option<PathBuf>,
}

impl SubtitleTrackItem {
    /// Constructs a new `SubtitleTrackItem`.
    pub fn new(id: usize, title: impl Into<String>, format: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            language: None,
            format: format.into(),
            is_external: false,
            file_path: None,
        }
    }

    /// Formats a descriptive label for UI display.
    pub fn display_label(&self) -> String {
        let mut parts = Vec::new();
        if self.is_external {
            parts.push("[External]".to_string());
        } else {
            parts.push("[Embedded]".to_string());
        }
        if !self.title.is_empty() {
            parts.push(self.title.clone());
        } else {
            parts.push(format!("Track {}", self.id + 1));
        }
        if let Some(ref lang) = self.language {
            parts.push(format!("[{}]", lang.to_uppercase()));
        }
        if !self.format.is_empty() {
            parts.push(format!("({})", self.format.to_uppercase()));
        }
        parts.join(" ")
    }
}

/// Action emitted by subtitle configuration dialog upon user interaction.
#[derive(Debug, Clone, PartialEq)]
pub enum SubtitleConfigAction {
    /// Select active subtitle track (`None` = disabled/off, `Some(id)` = active track).
    SelectTrack(Option<usize>),
    /// Request loading an external subtitle file from filesystem path.
    LoadExternal(PathBuf),
    /// Set subtitle presentation delay offset in milliseconds [-5000, 5000].
    SetDelayMs(i64),
}

/// Modal dialog for subtitle track selection, external file loading, and sync timing.
pub struct SubtitleConfigDialog {
    /// Whether the dialog window is open.
    pub is_open: bool,
    /// List of detected subtitle tracks (embedded container tracks and loaded external files).
    pub tracks: Vec<SubtitleTrackItem>,
    /// Currently selected track ID (`None` = Off/Disabled).
    pub selected_track_id: Option<usize>,
    /// Subtitle synchronization delay / offset in milliseconds [-5000, 5000].
    pub delay_ms: i64,
    /// Receiver for asynchronous file dialog picker thread.
    file_dialog_rx: Option<Receiver<PathBuf>>,
    /// Whether file picker dialog is currently open in background.
    is_file_dialog_active: bool,
    /// Transient status message (e.g. "Loaded external subtitle: movie.en.srt").
    pub status_message: Option<String>,
}

#[allow(clippy::derivable_impls)]
impl Default for SubtitleConfigDialog {
    fn default() -> Self {
        Self {
            is_open: false,
            tracks: Vec::new(),
            selected_track_id: None,
            delay_ms: 0,
            file_dialog_rx: None,
            is_file_dialog_active: false,
            status_message: None,
        }
    }
}

impl std::fmt::Debug for SubtitleConfigDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubtitleConfigDialog")
            .field("is_open", &self.is_open)
            .field("tracks", &self.tracks)
            .field("selected_track_id", &self.selected_track_id)
            .field("delay_ms", &self.delay_ms)
            .field("is_file_dialog_active", &self.is_file_dialog_active)
            .field("status_message", &self.status_message)
            .finish()
    }
}

impl SubtitleConfigDialog {
    /// Constructs a new `SubtitleConfigDialog`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the subtitle configuration dialog.
    pub fn open(&mut self) {
        self.is_open = true;
    }

    /// Closes the subtitle configuration dialog.
    pub fn close(&mut self) {
        self.is_open = false;
    }

    /// Sets discovered subtitle tracks.
    pub fn set_tracks(&mut self, tracks: Vec<SubtitleTrackItem>) {
        self.tracks = tracks;
    }

    /// Adds a subtitle track to the inventory.
    pub fn add_track(&mut self, track: SubtitleTrackItem) {
        self.tracks.push(track);
    }

    /// Selects a subtitle track by index (`None` = disable subtitles).
    pub fn select_track(&mut self, track_id: Option<usize>) {
        self.selected_track_id = track_id;
    }

    /// Sets subtitle sync delay in milliseconds [-5000, 5000].
    pub fn set_delay_ms(&mut self, delay: i64) {
        self.delay_ms = delay.clamp(-5000, 5000);
    }

    /// Resets subtitle delay to 0 ms (synchronized).
    pub fn reset_delay(&mut self) {
        self.delay_ms = 0;
    }

    /// Spawns native asynchronous non-blocking file picker for subtitle files.
    pub fn open_file_picker(&mut self) {
        if self.is_file_dialog_active {
            return;
        }

        let (tx, rx) = channel();
        self.file_dialog_rx = Some(rx);
        self.is_file_dialog_active = true;

        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .set_title("Open Subtitle File — Syncverse")
                .add_filter(
                    "All Supported Subtitles (*.srt, *.vtt, *.ass, *.ssa)",
                    &["srt", "vtt", "ass", "ssa"],
                )
                .add_filter("SubRip Subtitles (*.srt)", &["srt"])
                .add_filter("WebVTT Subtitles (*.vtt)", &["vtt"])
                .add_filter("Advanced SubStation Alpha (*.ass, *.ssa)", &["ass", "ssa"])
                .add_filter("All Files (*)", &["*"])
                .pick_file();

            if let Some(path) = picked {
                let _ = tx.send(path);
            }
        });
    }

    /// Polls background file picker for completed selection.
    pub fn poll_file_dialog(&mut self) -> Option<PathBuf> {
        let rx = self.file_dialog_rx.as_ref()?;
        match rx.try_recv() {
            Ok(path) => {
                self.file_dialog_rx = None;
                self.is_file_dialog_active = false;
                Some(path)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.file_dialog_rx = None;
                self.is_file_dialog_active = false;
                None
            }
        }
    }

    /// Renders the Subtitle Configuration modal dialog if open.
    pub fn show(&mut self, ctx: &Context) -> Vec<SubtitleConfigAction> {
        let mut actions = Vec::new();

        // Check if an external subtitle file was selected via background thread
        if let Some(path) = self.poll_file_dialog() {
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("external.srt")
                .to_string();
            let format = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("srt")
                .to_uppercase();

            let new_id = self.tracks.len();
            let track_item = SubtitleTrackItem {
                id: new_id,
                title: filename.clone(),
                language: None,
                format,
                is_external: true,
                file_path: Some(path.clone()),
            };
            self.tracks.push(track_item);
            self.selected_track_id = Some(new_id);
            self.status_message = Some(format!("Loaded external subtitle: {}", filename));

            actions.push(SubtitleConfigAction::LoadExternal(path));
            actions.push(SubtitleConfigAction::SelectTrack(Some(new_id)));
        }

        if !self.is_open {
            return actions;
        }

        let mut is_open = self.is_open;
        egui::Window::new("💬 Subtitle Settings")
            .open(&mut is_open)
            .collapsible(false)
            .resizable(true)
            .default_width(480.0)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Select embedded or external subtitles and calibrate display timing.")
                        .italics()
                        .color(Color32::from_rgb(156, 163, 175)),
                );
                ui.separator();

                // 1. Subtitle Track Selection
                ui.heading("Active Subtitle Track");

                // Off / Disable Option
                let is_off = self.selected_track_id.is_none();
                if ui.radio(is_off, "🚫 Off (Disable Subtitles)").clicked() {
                    self.selected_track_id = None;
                    actions.push(SubtitleConfigAction::SelectTrack(None));
                }

                ui.separator();

                if self.tracks.is_empty() {
                    ui.label(
                        RichText::new("No subtitle tracks detected. Load an external .srt, .vtt, or .ass file below.")
                            .italics()
                            .color(Color32::from_rgb(156, 163, 175)),
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(140.0)
                        .show(ui, |ui| {
                            for track in &self.tracks {
                                let is_selected = self.selected_track_id == Some(track.id);
                                let label = track.display_label();
                                if ui.radio(is_selected, label).clicked() {
                                    self.selected_track_id = Some(track.id);
                                    actions.push(SubtitleConfigAction::SelectTrack(Some(track.id)));
                                }
                            }
                        });
                }

                ui.add_space(8.0);
                ui.separator();

                // 2. Load External Subtitle File
                ui.heading("External Subtitles");
                ui.horizontal(|ui| {
                    let picker_btn_text = if self.is_file_dialog_active {
                        "⏳ Opening File Picker..."
                    } else {
                        "📂 Load External Subtitle..."
                    };

                    if ui.add_enabled(!self.is_file_dialog_active, egui::Button::new(picker_btn_text)).clicked() {
                        self.open_file_picker();
                    }

                    ui.label(
                        RichText::new("(.srt, .vtt, .ass, .ssa)")
                            .small()
                            .color(Color32::from_rgb(148, 163, 184)),
                    );
                });

                if let Some(ref msg) = self.status_message {
                    ui.add_space(4.0);
                    ui.label(RichText::new(msg).color(Color32::from_rgb(74, 222, 128)));
                }

                ui.add_space(8.0);
                ui.separator();

                // 3. Subtitle Delay / Sync Offset
                ui.heading("Subtitle Sync / Delay Offset");
                ui.label(
                    RichText::new("Shift subtitle display earlier or later to match spoken dialogue.")
                        .small()
                        .color(Color32::from_rgb(148, 163, 184)),
                );

                let delay_status = if self.delay_ms == 0 {
                    "0 ms (Synchronized)".to_string()
                } else if self.delay_ms > 0 {
                    format!("+{} ms (Subtitles Early / Ahead)", self.delay_ms)
                } else {
                    format!("{} ms (Subtitles Delayed)", self.delay_ms)
                };

                ui.label(
                    RichText::new(delay_status)
                        .monospace()
                        .strong()
                        .color(if self.delay_ms == 0 {
                            Color32::from_rgb(74, 222, 128)
                        } else {
                            Color32::from_rgb(250, 204, 21)
                        }),
                );

                let prev_delay = self.delay_ms;
                let slider = egui::Slider::new(&mut self.delay_ms, -5000..=5000)
                    .suffix(" ms");
                if ui.add(slider).changed() {
                    actions.push(SubtitleConfigAction::SetDelayMs(self.delay_ms));
                }

                ui.horizontal(|ui| {
                    if ui.button("-250 ms").clicked() {
                        self.set_delay_ms(self.delay_ms - 250);
                    }
                    if ui.button("-50 ms").clicked() {
                        self.set_delay_ms(self.delay_ms - 50);
                    }
                    if ui.button("⟲ Reset (0 ms)").on_hover_text("Reset subtitle sync offset to 0 ms").clicked() {
                        self.reset_delay();
                    }
                    if ui.button("+50 ms").clicked() {
                        self.set_delay_ms(self.delay_ms + 50);
                    }
                    if ui.button("+250 ms").clicked() {
                        self.set_delay_ms(self.delay_ms + 250);
                    }
                });
                if self.delay_ms != prev_delay && !actions.iter().any(|a| matches!(a, SubtitleConfigAction::SetDelayMs(_))) {
                    actions.push(SubtitleConfigAction::SetDelayMs(self.delay_ms));
                }

                ui.add_space(10.0);
                ui.separator();

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        self.is_open = false;
                    }
                });
            });

        self.is_open = is_open;
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_track_item_display_label() {
        let mut item = AudioTrackItem::new(1, "Main English", "aac");
        item.language = Some("eng".to_string());
        item.channels = Some(2);
        item.sample_rate = Some(48000.0);
        assert_eq!(
            item.display_label(),
            "Main English • [ENG] • AAC • Stereo • 48.0 kHz"
        );

        let mut item51 = AudioTrackItem::new(2, "", "ac3");
        item51.channels = Some(6);
        assert_eq!(item51.display_label(), "Track 2 • AC3 • 5.1 Surround");
    }

    #[test]
    fn test_audio_config_dialog_state_transitions() {
        let mut dlg = AudioConfigDialog::new();
        assert!(!dlg.is_open);
        assert_eq!(dlg.volume, 1.0);
        assert_eq!(dlg.effective_volume(), 1.0);
        assert!(!dlg.is_muted);
        assert_eq!(dlg.delay_ms, 0);

        dlg.open();
        assert!(dlg.is_open);

        dlg.set_volume(0.65);
        assert_eq!(dlg.volume, 0.65);
        assert_eq!(dlg.effective_volume(), 0.65);

        dlg.set_muted(true);
        assert!(dlg.is_muted);
        assert_eq!(dlg.effective_volume(), 0.0);

        dlg.set_volume(0.8);
        assert!(!dlg.is_muted);
        assert_eq!(dlg.effective_volume(), 0.8);

        dlg.set_delay_ms(150);
        assert_eq!(dlg.delay_ms, 150);
        dlg.set_delay_ms(-6000);
        assert_eq!(dlg.delay_ms, -5000);
        dlg.reset_delay();
        assert_eq!(dlg.delay_ms, 0);

        let tracks = vec![
            AudioTrackItem::new(1, "Track 1", "aac"),
            AudioTrackItem::new(2, "Track 2", "opus"),
        ];
        dlg.set_tracks(tracks);
        assert_eq!(dlg.selected_track_id, Some(1));
        dlg.select_track(2);
        assert_eq!(dlg.selected_track_id, Some(2));

        dlg.close();
        assert!(!dlg.is_open);
    }

    #[test]
    fn test_video_metadata_formatting() {
        let meta = VideoStreamMetadata {
            width: Some(1920),
            height: Some(1080),
            codec: Some("h264".to_string()),
            frame_rate: Some(59.94),
            bit_depth: Some(8),
            pixel_format: Some("yuv420p".to_string()),
            timescale: Some(1000),
        };

        assert_eq!(meta.resolution_str(), "1920 × 1080 (1080p FHD)");
        assert_eq!(meta.aspect_ratio_str(), "16:9 (1.78:1)");
        assert_eq!(meta.frame_rate_str(), "59.94 fps");
        assert_eq!(meta.codec_str(), "H264 • yuv420p • 8-bit");

        let empty = VideoStreamMetadata::default();
        assert_eq!(empty.resolution_str(), "Unknown");
        assert_eq!(empty.aspect_ratio_str(), "Unknown");
        assert_eq!(empty.frame_rate_str(), "Variable / Unknown");
        assert_eq!(empty.codec_str(), "Unknown");
    }

    #[test]
    fn test_video_config_dialog_state() {
        let mut dlg = VideoConfigDialog::new();
        assert!(!dlg.is_open);
        assert_eq!(dlg.aspect_ratio, AspectRatioOverride::Original);
        assert_eq!(dlg.speed, 1.0);

        dlg.open();
        assert!(dlg.is_open);

        dlg.set_aspect_ratio(AspectRatioOverride::Ratio16_9);
        assert_eq!(dlg.aspect_ratio, AspectRatioOverride::Ratio16_9);

        dlg.set_speed(1.5);
        assert_eq!(dlg.speed, 1.5);
        dlg.set_speed(5.0);
        assert_eq!(dlg.speed, 4.0);

        dlg.close();
        assert!(!dlg.is_open);
    }

    #[test]
    fn test_subtitle_track_item_display_label() {
        let embedded = SubtitleTrackItem {
            id: 0,
            title: "English".to_string(),
            language: Some("eng".to_string()),
            format: "SRT".to_string(),
            is_external: false,
            file_path: None,
        };
        assert_eq!(embedded.display_label(), "[Embedded] English [ENG] (SRT)");

        let external = SubtitleTrackItem {
            id: 1,
            title: "custom_sub.ass".to_string(),
            language: None,
            format: "ASS".to_string(),
            is_external: true,
            file_path: Some(PathBuf::from("/path/to/custom_sub.ass")),
        };
        assert_eq!(external.display_label(), "[External] custom_sub.ass (ASS)");
    }

    #[test]
    fn test_subtitle_config_dialog_state() {
        let mut dlg = SubtitleConfigDialog::new();
        assert!(!dlg.is_open);
        assert_eq!(dlg.selected_track_id, None);
        assert_eq!(dlg.delay_ms, 0);

        dlg.open();
        assert!(dlg.is_open);

        let t0 = SubtitleTrackItem::new(0, "Sub 1", "srt");
        dlg.add_track(t0);
        assert_eq!(dlg.tracks.len(), 1);

        dlg.select_track(Some(0));
        assert_eq!(dlg.selected_track_id, Some(0));

        // Disable / Off
        dlg.select_track(None);
        assert_eq!(dlg.selected_track_id, None);

        dlg.set_delay_ms(-200);
        assert_eq!(dlg.delay_ms, -200);
        dlg.reset_delay();
        assert_eq!(dlg.delay_ms, 0);

        dlg.close();
        assert!(!dlg.is_open);
    }

    #[test]
    fn test_dialog_egui_rendering_headless() {
        let ctx = Context::default();
        let mut audio_dlg = AudioConfigDialog::new();
        audio_dlg.open();
        let mut video_dlg = VideoConfigDialog::new();
        video_dlg.open();
        let mut sub_dlg = SubtitleConfigDialog::new();
        sub_dlg.open();

        let mut output = ctx.run_ui(egui::RawInput::default(), |ctx| {
            let _ = audio_dlg.show(ctx);
            let _ = video_dlg.show(ctx);
            let _ = sub_dlg.show(ctx);
        });
        output.textures_delta.clear();
    }
}
