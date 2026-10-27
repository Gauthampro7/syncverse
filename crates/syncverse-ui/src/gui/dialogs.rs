//! Modal dialogs for watch-party hosting, connecting, keyboard shortcuts, and engine information.

use std::sync::mpsc::Sender;
use egui::{Align2, Color32, Context, RichText, Vec2};

use crate::command::PlayerCommand;
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
                    });

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
                            let _ = command_tx.send(PlayerCommand::HostParty {
                                port: self.port,
                                room: self.room_name.clone(),
                                username: self.username.clone(),
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

                            let _ = command_tx.send(PlayerCommand::ConnectParty {
                                addr: self.host_addr.clone(),
                                room: self.room_name.clone(),
                                username: self.username.clone(),
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

                            ui.label(RichText::new("F11 / Double-Click").monospace().strong());
                            ui.label("Toggle Fullscreen Mode");
                            ui.end_row();

                            ui.label(RichText::new("Ctrl + O").monospace().strong());
                            ui.label("Open Media File (Native File Picker)");
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
                    ui.label(RichText::new("v0.1.1").color(Color32::from_rgb(96, 165, 250)));
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
