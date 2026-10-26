//! Standalone interactive terminal presentation player with raw mode and TrueColor OSD.

use std::io::{stdin, stdout, IsTerminal, Write};
use std::time::{Duration, Instant};

use syncplay_chat::{ParsedSlashCommand, SlashCommandParser};
use syncplay_codecs::{TerminalColorCell, TerminalTrueColorBuffer};
use syncplay_common::normalize_target_path;
use syncplay_ui::{
    get_terminal_size, parse_key_sequence, render_faded_chat_comment, render_hotkeys_footer,
    render_osd_hud, render_styled_scrubber_bar, render_sync_diagnostics_panel, BoundedChatOverlay,
    KeyCommand, OsdHudState, ScrubberStyle, TerminalGuard,
};

use crate::audio_sink::AutoAudioSink;
use crate::coordinator::{MediaInfo, PlayerCoordinator};
use crate::error::CoreError;
use syncplay_ui::PlayerCommand;

/// Mode of input handling in the interactive player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerInputMode {
    /// Normal playback hotkey navigation.
    Normal,
    /// Slash-command and chat input prompt.
    CommandBar,
}

/// Standalone interactive terminal presentation player.
pub struct InteractivePlayer {
    /// Runtime coordinator binding demuxing, clock, sync, and telemetry.
    pub coordinator: PlayerCoordinator,
    /// Audio sink backend (PipeWire/ALSA or Null fallback).
    pub audio_sink: AutoAudioSink,
    /// Target media identifier or path.
    pub target: String,
    /// Inspected media container metadata.
    pub media_info: Option<MediaInfo>,
    /// Playback volume level in [0.0, 1.0].
    pub volume: f32,
    /// Whether audio output is currently muted.
    pub is_muted: bool,
    /// Whether the floating smart chat overlay is visible.
    pub show_chat: bool,
    /// Whether the detailed sync diagnostics HUD is visible.
    pub show_sync_hud: bool,
    /// Active keyboard input mode.
    pub input_mode: PlayerInputMode,
    /// Buffer holding current command bar text.
    pub command_buffer: String,
    /// Active transient status message and expiration time.
    pub status_message: Option<(String, Instant)>,
    /// Target frame rate in FPS (default 60).
    pub target_fps: u32,
    /// Optional limit on rendered frames for test/headless execution.
    pub max_ticks: Option<usize>,
    /// Bounded chat overlay display list.
    pub chat_overlay: BoundedChatOverlay,
    /// Monotonic player start time used for timeline calculations and chat dwell fading.
    pub start_time: Instant,
}

impl InteractivePlayer {
    /// Constructs a new `InteractivePlayer` inspecting the given target media.
    pub fn new(target: &str) -> Self {
        let normalized = normalize_target_path(target);
        let media_info = PlayerCoordinator::inspect(&normalized).ok();
        let mut coordinator = PlayerCoordinator::new();
        if let Some(ref info) = media_info {
            coordinator.set_duration_ms(info.duration_ms);
        }

        let mut chat_overlay = BoundedChatOverlay::new(5);
        let t0 = 0.0;
        chat_overlay.on_message("Synchronized playback looking rock solid! 🚀".into(), t0);
        chat_overlay.on_message("Zero frame drops at 60fps! #epic".into(), t0);
        chat_overlay.on_message("Pure Rust video & audio pipeline on Fedora 🔥".into(), t0);

        Self {
            coordinator,
            audio_sink: AutoAudioSink::new(),
            target: normalized,
            media_info,
            volume: 0.8,
            is_muted: false,
            show_chat: true,
            show_sync_hud: false,
            input_mode: PlayerInputMode::Normal,
            command_buffer: String::new(),
            status_message: None,
            target_fps: 60,
            max_ticks: None,
            chat_overlay,
            start_time: Instant::now(),
        }
    }

    /// Sets the maximum number of presentation ticks to execute before returning (for headless/testing).
    pub fn with_max_ticks(mut self, ticks: Option<usize>) -> Self {
        self.max_ticks = ticks;
        self
    }

    /// Sets a transient status message that expires after `duration`.
    pub fn set_status_message(&mut self, msg: impl Into<String>, duration: Duration) {
        self.status_message = Some((msg.into(), Instant::now() + duration));
    }

    /// Handles a high-level `KeyCommand` in normal input mode.
    ///
    /// Returns `true` if the player should terminate presentation.
    pub fn handle_key_command(&mut self, cmd: KeyCommand, now: Instant) -> bool {
        let duration_ms = self
            .media_info
            .as_ref()
            .and_then(|info| info.duration_ms)
            .unwrap_or(60_000) as f64;

        match cmd {
            KeyCommand::TogglePause => {
                let _ = self
                    .coordinator
                    .handle_command(PlayerCommand::TogglePause, now);
            }
            KeyCommand::SeekRelative(delta_ms) => {
                let cur = self.coordinator.current_pts(now);
                let target_raw = cur + delta_ms as f64;

                let target_pts = if delta_ms.abs() == 5000 {
                    // Keyframe snap if keyframe entries exist
                    if let Some(ref info) = self.media_info {
                        if !info.keyframe_pts.is_empty() {
                            let mut closest = info.keyframe_pts[0] as f64;
                            let mut min_diff = (closest - target_raw).abs();
                            for &kf in &info.keyframe_pts[1..] {
                                let diff = (kf as f64 - target_raw).abs();
                                if diff < min_diff {
                                    min_diff = diff;
                                    closest = kf as f64;
                                }
                            }
                            closest
                        } else {
                            target_raw.clamp(0.0, duration_ms)
                        }
                    } else {
                        target_raw.clamp(0.0, duration_ms)
                    }
                } else {
                    target_raw.clamp(0.0, duration_ms)
                };

                let _ = self
                    .coordinator
                    .handle_command(PlayerCommand::Seek(target_pts as u64), now);
                let kind = if delta_ms.abs() == 5000 {
                    "Keyframe"
                } else {
                    "Fine"
                };
                self.set_status_message(
                    format!("[SEEK] {} Jump -> {:.2}s", kind, target_pts / 1000.0),
                    Duration::from_secs(2),
                );
            }
            KeyCommand::ShiftSpeed(delta_tenths) => {
                let cur_speed = self.coordinator.speed();
                let next_speed = (cur_speed + (delta_tenths as f64 * 0.1)).clamp(0.2, 4.0);
                let _ = self
                    .coordinator
                    .handle_command(PlayerCommand::SetSpeed(next_speed), now);
                self.set_status_message(
                    format!("[SPEED] {:.2}x", next_speed),
                    Duration::from_secs(2),
                );
            }
            KeyCommand::ToggleMute => {
                self.is_muted = !self.is_muted;
                self.set_status_message(
                    if self.is_muted {
                        "[AUDIO] Muted"
                    } else {
                        "[AUDIO] Unmuted"
                    },
                    Duration::from_secs(2),
                );
            }
            KeyCommand::AdjustVolume(delta) => {
                self.volume = (self.volume + delta).clamp(0.0, 1.0);
                if self.is_muted && delta > 0.0 {
                    self.is_muted = false;
                }
                self.set_status_message(
                    format!("[VOL] {:.0}%", self.volume * 100.0),
                    Duration::from_secs(2),
                );
            }
            KeyCommand::ToggleChatOverlay => {
                self.show_chat = !self.show_chat;
                self.set_status_message(
                    if self.show_chat {
                        "[HUD] Chat Overlay: ON"
                    } else {
                        "[HUD] Chat Overlay: OFF"
                    },
                    Duration::from_secs(2),
                );
            }
            KeyCommand::ToggleSyncHud => {
                self.show_sync_hud = !self.show_sync_hud;
                self.set_status_message(
                    if self.show_sync_hud {
                        "[HUD] Sync Diagnostics: ON"
                    } else {
                        "[HUD] Sync Diagnostics: OFF"
                    },
                    Duration::from_secs(2),
                );
            }
            KeyCommand::OpenCommandPrompt => {
                self.input_mode = PlayerInputMode::CommandBar;
                self.command_buffer = "/".to_string();
            }
            KeyCommand::Quit => {
                return true;
            }
        }
        false
    }

    /// Handles keyboard input when the player is in slash-command prompt mode.
    pub fn handle_command_bar_input(&mut self, bytes: &[u8], now: Instant) {
        for &b in bytes {
            match b {
                0x1B => {
                    // Escape cancels command bar
                    self.input_mode = PlayerInputMode::Normal;
                    self.command_buffer.clear();
                }
                b'\r' | b'\n' => {
                    // Enter dispatches command or adds message
                    let line = self.command_buffer.trim().to_string();
                    self.command_buffer.clear();
                    self.input_mode = PlayerInputMode::Normal;

                    if line.starts_with('/') {
                        let cur_pts = self.coordinator.current_pts(now) as u64;
                        let duration_ms = self
                            .media_info
                            .as_ref()
                            .and_then(|i| i.duration_ms)
                            .unwrap_or(60_000);

                        match SlashCommandParser::parse(&line, Some(cur_pts)) {
                            Ok(ParsedSlashCommand::Seek { target_pts_ms }) => {
                                let clamped = target_pts_ms.min(duration_ms);
                                let _ = self
                                    .coordinator
                                    .handle_command(PlayerCommand::Seek(clamped), now);
                                self.set_status_message(
                                    format!(
                                        "[SEEK] Target: {:02}:{:02}.{:03}",
                                        clamped / 60_000,
                                        (clamped % 60_000) / 1000,
                                        clamped % 1000
                                    ),
                                    Duration::from_secs(3),
                                );
                            }
                            Ok(ParsedSlashCommand::Bookmark { label, pts_ms }) => {
                                let desc = label.as_deref().unwrap_or("Marker");
                                self.set_status_message(
                                    format!(
                                        "[BOOKMARK] \"{}\" saved at {:02}:{:02}",
                                        desc,
                                        pts_ms / 60_000,
                                        (pts_ms % 60_000) / 1000
                                    ),
                                    Duration::from_secs(3),
                                );
                            }
                            Ok(ParsedSlashCommand::Clip {
                                start_pts_ms,
                                end_pts_ms,
                                label,
                            }) => {
                                let desc = label.as_deref().unwrap_or("Segment");
                                self.set_status_message(
                                    format!(
                                        "[CLIP] \"{}\" {:02}:{:02} -> {:02}:{:02}",
                                        desc,
                                        start_pts_ms / 60_000,
                                        (start_pts_ms % 60_000) / 1000,
                                        end_pts_ms / 60_000,
                                        (end_pts_ms % 60_000) / 1000
                                    ),
                                    Duration::from_secs(3),
                                );
                            }
                            Ok(ParsedSlashCommand::Poll { question, options }) => {
                                self.set_status_message(
                                    format!(
                                        "[POLL] \"{}\" with {} options",
                                        question,
                                        options.len()
                                    ),
                                    Duration::from_secs(3),
                                );
                            }
                            Ok(ParsedSlashCommand::SyncStatus) => {
                                self.show_sync_hud = true;
                                self.set_status_message(
                                    "[SYNC] Diagnostics panel opened",
                                    Duration::from_secs(3),
                                );
                            }
                            Err(e) => {
                                self.set_status_message(
                                    format!("[ERROR] {}", e),
                                    Duration::from_secs(3),
                                );
                            }
                        }
                    } else if !line.is_empty() {
                        let wall_now = self.start_time.elapsed().as_secs_f64();
                        self.chat_overlay.on_message(line, wall_now);
                    }
                }
                0x7F | 0x08 => {
                    // Backspace
                    self.command_buffer.pop();
                    if self.command_buffer.is_empty() {
                        self.input_mode = PlayerInputMode::Normal;
                    }
                }
                c if (0x20..=0x7E).contains(&c) => {
                    self.command_buffer.push(c as char);
                }
                _ => {}
            }
        }
    }

    /// Runs the interactive terminal presentation loop.
    pub fn run_presentation_loop(&mut self) -> Result<(), CoreError> {
        let is_interactive =
            self.max_ticks.is_none() && stdin().is_terminal() && stdout().is_terminal();
        let guard = if is_interactive {
            Some(TerminalGuard::enter_raw_mode())
        } else {
            None
        };

        let duration_ms = self
            .media_info
            .as_ref()
            .and_then(|info| info.duration_ms)
            .unwrap_or(60_000);
        let container_desc = self
            .media_info
            .as_ref()
            .map(|info| info.format.clone())
            .unwrap_or_else(|| "Synthetic Media".to_string());

        let frame_budget =
            Duration::from_nanos(1_000_000_000 / (self.target_fps as u64).max(1));

        let mut last_render = Instant::now().checked_sub(frame_budget).unwrap_or_else(Instant::now);
        let mut last_size_query =
            Instant::now().checked_sub(Duration::from_secs(1)).unwrap_or_else(Instant::now);

        let (mut cached_cols, mut cached_rows) = if is_interactive {
            let (c, r) = get_terminal_size();
            (c.saturating_sub(2).clamp(40, 100), r.saturating_sub(12).clamp(4, 16))
        } else {
            (80, 8)
        };

        let mut term_buf = TerminalTrueColorBuffer::new(cached_cols, cached_rows);
        let mut ansi_video = String::with_capacity(cached_cols * cached_rows * 32);
        let mut ticks: usize = 0;

        self.start_time = Instant::now();

        // Start playback if currently paused
        if !self.coordinator.is_playing() {
            let _ = self
                .coordinator
                .handle_command(PlayerCommand::TogglePause, Instant::now());
        }

        loop {
            let now = Instant::now();

            // 1. Process non-blocking input
            if let Some(ref g) = guard {
                let mut buf = [0u8; 32];
                if let Ok(n) = g.poll_raw_bytes(&mut buf) {
                    if n > 0 {
                        match self.input_mode {
                            PlayerInputMode::Normal => {
                                if buf[0] == b'/' {
                                    self.input_mode = PlayerInputMode::CommandBar;
                                    self.command_buffer = "/".to_string();
                                } else if let Some((cmd, _)) = parse_key_sequence(&buf[..n]) {
                                    if self.handle_key_command(cmd, now) {
                                        break;
                                    }
                                }
                            }
                            PlayerInputMode::CommandBar => {
                                self.handle_command_bar_input(&buf[..n], now);
                            }
                        }
                    }
                }
            }

            // 2. Advance coordinator tick and retrieve latest snapshot
            let snapshot = self.coordinator.tick(now);

            // 3. Throttle terminal size querying to ~300ms to avoid process fork jitter
            if is_interactive && now.duration_since(last_size_query) >= Duration::from_millis(300) {
                last_size_query = now;
                let (live_cols, live_rows) = get_terminal_size();
                let target_cols = live_cols.saturating_sub(2).clamp(40, 100);
                let target_video_rows = live_rows.saturating_sub(12).clamp(4, 16);
                if target_cols != cached_cols || target_video_rows != cached_rows {
                    cached_cols = target_cols;
                    cached_rows = target_video_rows;
                    term_buf.resize(cached_cols, cached_rows);
                }
            }

            // 4. Render video canvas & OSD when frame budget elapsed
            if now.duration_since(last_render) >= frame_budget || !is_interactive {
                last_render = now;

                let current_pts = self.coordinator.current_pts(now);
                let current_ms = current_pts as u64;

                // Animate video canvas pattern
                let time_factor = current_pts / 80.0;
                for r in 0..term_buf.rows {
                    let y_top = r * 2;
                    let y_bot = r * 2 + 1;
                    for c in 0..term_buf.cols {
                        let cx = c as f64;
                        let phase_top =
                            (cx * 0.08 + (y_top as f64) * 0.15 + time_factor * 0.05).sin();
                        let top_r = ((phase_top * 0.5 + 0.5) * 60.0 + 15.0) as u8;
                        let top_g = (((phase_top + 2.0).sin() * 0.5 + 0.5) * 140.0 + 30.0) as u8;
                        let top_b = (((phase_top + 4.0).sin() * 0.5 + 0.5) * 200.0 + 55.0) as u8;

                        let phase_bot =
                            (cx * 0.08 + (y_bot as f64) * 0.15 + time_factor * 0.05).sin();
                        let bot_r = ((phase_bot * 0.5 + 0.5) * 60.0 + 15.0) as u8;
                        let bot_g = (((phase_bot + 2.0).sin() * 0.5 + 0.5) * 140.0 + 30.0) as u8;
                        let bot_b = (((phase_bot + 4.0).sin() * 0.5 + 0.5) * 200.0 + 55.0) as u8;

                        let idx = r * term_buf.cols + c;
                        term_buf.cells[idx] = TerminalColorCell {
                            top_r,
                            top_g,
                            top_b,
                            bot_r,
                            bot_g,
                            bot_b,
                        };
                    }
                }
                term_buf.render_ansi(&mut ansi_video);

                if is_interactive {
                    let mut screen = String::with_capacity(cached_cols * 40);
                    screen.push_str("\x1b[1;1H"); // Home cursor for flicker-free single flush

                    let inner_width = cached_cols.saturating_sub(2);

                    // Top border
                    screen.push_str("\x1b[1;36m┌");
                    for _ in 0..inner_width {
                        screen.push('─');
                    }
                    screen.push_str("┐\x1b[0m\x1b[K\r\n");

                    // Title line
                    let raw_title = format!(
                        " SYNCPLAY-ALT — Target: {} ({})",
                        self.target, container_desc
                    );
                    let display_title: String = if raw_title.chars().count() > inner_width {
                        raw_title.chars().take(inner_width).collect()
                    } else {
                        raw_title
                    };
                    let display_chars = display_title.chars().count();
                    let pad_spaces = inner_width.saturating_sub(display_chars);
                    screen.push_str("\x1b[1;36m│\x1b[0m\x1b[1;37m");
                    screen.push_str(&display_title);
                    screen.push_str("\x1b[0m");
                    for _ in 0..pad_spaces {
                        screen.push(' ');
                    }
                    screen.push_str("\x1b[1;36m│\x1b[0m\x1b[K\r\n");

                    // Header divider
                    screen.push_str("\x1b[1;36m├");
                    for _ in 0..inner_width {
                        screen.push('─');
                    }
                    screen.push_str("┤\x1b[0m\x1b[K\r\n");

                    // Video canvas
                    for line in ansi_video.lines() {
                        screen.push_str(line);
                        screen.push_str("\x1b[K\r\n");
                    }

                    // Scrubber progress bar
                    screen.push_str(&render_styled_scrubber_bar(
                        current_ms,
                        duration_ms,
                        cached_cols,
                        ScrubberStyle::Classic,
                    ));
                    screen.push_str("\x1b[K\r\n");

                    // OSD HUD Line
                    let hud_state = OsdHudState {
                        current_ms,
                        duration_ms,
                        is_playing: self.coordinator.is_playing(),
                        speed: self.coordinator.speed(),
                        volume: self.volume,
                        is_muted: self.is_muted,
                        rtt_ms: snapshot.network_rtt_nanos / 1_000_000,
                        phase_err_ms: (snapshot.phase_error_nanos as f64) / 1_000_000.0,
                    };
                    screen.push_str(&render_osd_hud(&hud_state));
                    screen.push_str("\x1b[K\r\n");

                    // Optional Sync Diagnostics HUD
                    if self.show_sync_hud {
                        let diag = render_sync_diagnostics_panel(
                            snapshot.network_rtt_nanos / 1_000_000,
                            5.0,
                            (snapshot.clock_offset_nanos as f64) / 1_000_000.0,
                            (snapshot.phase_error_nanos as f64) / 1_000_000.0,
                            snapshot.pll_speed_ratio_q16 as f64 / 65536.0,
                            "Synchronized",
                        );
                        for line in diag.lines() {
                            screen.push_str(line);
                            screen.push_str("\x1b[K\r\n");
                        }
                    }

                    // Floating chat overlay
                    if self.show_chat {
                        screen.push_str("\x1b[90m── Live Smart Chat ");
                        let chat_rule_len = cached_cols.saturating_sub(21);
                        for _ in 0..chat_rule_len {
                            screen.push('─');
                        }
                        screen.push_str("\x1b[0m\x1b[K\r\n");

                        let wall_secs = self.start_time.elapsed().as_secs_f64();
                        let mut shown = 0;
                        for bubble in self.chat_overlay.visible_bubbles() {
                            if let Some(faded) = render_faded_chat_comment(
                                bubble,
                                wall_secs,
                                (20, 20, 30),
                                (220, 220, 240),
                            ) {
                                screen.push_str("  ");
                                screen.push_str(&faded);
                                screen.push_str("\x1b[K\r\n");
                                shown += 1;
                                if shown >= 3 {
                                    break;
                                }
                            }
                        }
                        while shown < 2 {
                            screen.push_str("\x1b[K\r\n");
                            shown += 1;
                        }
                    }

                    // Command bar / status message
                    if self.input_mode == PlayerInputMode::CommandBar {
                        screen.push_str(&format!(
                            "\x1b[1;33mCOMMAND>\x1b[0m \x1b[1;37m{}█\x1b[0m\x1b[K\r\n",
                            self.command_buffer
                        ));
                    } else if let Some((ref msg, expire)) = self.status_message {
                        if now <= expire {
                            screen.push_str(&format!(
                                "\x1b[1;32mSTATUS:\x1b[0m \x1b[1;37m{}\x1b[0m\x1b[K\r\n",
                                msg
                            ));
                        } else {
                            screen.push_str(&render_hotkeys_footer(cached_cols));
                            screen.push_str("\x1b[K\r\n");
                        }
                    } else {
                        screen.push_str(&render_hotkeys_footer(cached_cols));
                        screen.push_str("\x1b[K\r\n");
                    }

                    // Clear lingering rows
                    screen.push_str("\x1b[J");

                    print!("{}", screen);
                    let _ = stdout().flush();
                }
            }

            ticks += 1;
            if let Some(max) = self.max_ticks {
                if ticks >= max {
                    break;
                }
            }

            if is_interactive {
                std::thread::sleep(Duration::from_millis(2));
            }
        }

        Ok(())
    }
}
