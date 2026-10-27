//! Terminal raw mode, ANSI escape sequence parsing, and interactive player presentation.
//!
//! Provides zero-external-dependency raw terminal mode management via safe RAII guards,
//! non-blocking keystroke decoding (space, arrows, speed, volume, HUD toggles, quit),
//! visual scrubber progress bar rendering, and OSD composition.

use std::io::{stdin, stdout, IsTerminal, Read, Write};
use std::process::Command;

use crate::overlay::{evaluate_sync_health, format_timecode_hud, SyncBadgeColor};

/// Normalized keyboard commands emitted during interactive terminal playback.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyCommand {
    /// Toggle playback between Playing and Paused (Space).
    TogglePause,
    /// Relative seek in milliseconds (Left/Right: ±5000ms, Shift+Left/Right: ±1000ms).
    SeekRelative(i64),
    /// Adjust playback speed by tenths (e.g. -1 for -0.1x, +1 for +0.1x).
    ShiftSpeed(i32),
    /// Toggle audio muting (M / m).
    ToggleMute,
    /// Adjust audio volume delta (Up: +0.05, Down: -0.05).
    AdjustVolume(f32),
    /// Toggle floating smart chat overlay (T / t / C / c).
    ToggleChatOverlay,
    /// Toggle sync health & diagnostic HUD (S / s).
    ToggleSyncHud,
    /// Open slash-command prompt (/).
    OpenCommandPrompt,
    /// Quit presentation cleanly (Q / q / Esc).
    Quit,
}

/// RAII guard managing terminal raw mode and alternate screen buffer.
///
/// Automatically restores cursor visibility, normal screen buffer, and original
/// terminal line discipline upon normal exit or panic.
pub struct TerminalGuard {
    is_active: bool,
    saved_stty: Option<String>,
}

impl TerminalGuard {
    /// Attempts to enter raw mode and alternate screen buffer if stdin is a TTY.
    ///
    /// If standard input is not a terminal (e.g. in headless CI or piped scripts),
    /// this gracefully constructs an inactive guard without modifying terminal settings.
    pub fn enter_raw_mode() -> Self {
        if !stdin().is_terminal() || !stdout().is_terminal() {
            return Self {
                is_active: false,
                saved_stty: None,
            };
        }

        // 1. Save original stty settings
        let saved_stty = match Command::new("stty").arg("-g").output() {
            Ok(out) if out.status.success() => {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            }
            _ => None,
        };

        // 2. Configure non-blocking raw mode while preserving output postprocessing (OPOST)
        // and signal delivery (ISIG so Ctrl+C / SIGINT cleanly terminates).
        let _ = Command::new("stty")
            .args(["-echo", "-icanon", "opost", "isig", "min", "0", "time", "0"])
            .status();

        // 3. Switch to Alternate Screen buffer, hide cursor, and clear screen
        print!("\x1b[?1049h\x1b[?25l\x1b[2J\x1b[1;1H");
        let _ = stdout().flush();

        Self {
            is_active: true,
            saved_stty,
        }
    }

    /// Returns `true` if raw mode was successfully engaged.
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// Reads available non-blocking input bytes and returns parsed `KeyCommand` if any.
    pub fn poll_key(&self) -> Option<KeyCommand> {
        if !self.is_active {
            return None;
        }

        let mut buf = [0u8; 16];
        match stdin().read(&mut buf) {
            Ok(n) if n > 0 => {
                let (cmd, _) = parse_key_sequence(&buf[..n])?;
                Some(cmd)
            }
            _ => None,
        }
    }

    /// Reads available non-blocking raw bytes directly into the supplied buffer.
    pub fn poll_raw_bytes(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        if !self.is_active {
            return Ok(0);
        }
        stdin().read(buf)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.is_active {
            // Restore normal screen, show cursor, reset formatting
            print!("\x1b[?25h\x1b[0m\x1b[?1049l");
            let _ = stdout().flush();

            // Restore stty settings if saved
            if let Some(ref saved) = self.saved_stty {
                let _ = Command::new("stty").arg(saved).status();
            } else {
                let _ = Command::new("stty").arg("sane").status();
            }
        }
    }
}

/// Parses a byte sequence from raw terminal input into a `KeyCommand` and consumed byte count.
pub fn parse_key_sequence(bytes: &[u8]) -> Option<(KeyCommand, usize)> {
    if bytes.is_empty() {
        return None;
    }

    // Single-byte keys
    match bytes[0] {
        b' ' => return Some((KeyCommand::TogglePause, 1)),
        b'q' | b'Q' => return Some((KeyCommand::Quit, 1)),
        b'm' | b'M' => return Some((KeyCommand::ToggleMute, 1)),
        b't' | b'T' | b'c' | b'C' => return Some((KeyCommand::ToggleChatOverlay, 1)),
        b's' | b'S' => return Some((KeyCommand::ToggleSyncHud, 1)),
        b'/' => return Some((KeyCommand::OpenCommandPrompt, 1)),
        b'[' | b'<' => return Some((KeyCommand::ShiftSpeed(-1), 1)),
        b']' | b'>' => return Some((KeyCommand::ShiftSpeed(1), 1)),
        b'h' => return Some((KeyCommand::SeekRelative(-5000), 1)),
        b'l' => return Some((KeyCommand::SeekRelative(5000), 1)),
        b'j' => return Some((KeyCommand::AdjustVolume(-0.05), 1)),
        b'k' => return Some((KeyCommand::AdjustVolume(0.05), 1)),
        // ESC key alone
        0x1B if bytes.len() == 1 => return Some((KeyCommand::Quit, 1)),
        _ => {}
    }

    // Escape sequences: \x1b[...
    if bytes[0] == 0x1B && bytes.len() >= 3 && bytes[1] == b'[' {
        // Extended modified sequences: \x1b[1;2D (Shift+Left), \x1b[1;2C (Shift+Right)
        if bytes.len() >= 6 && bytes[2] == b'1' && bytes[3] == b';' && bytes[4] == b'2' {
            match bytes[5] {
                b'D' => return Some((KeyCommand::SeekRelative(-1000), 6)),
                b'C' => return Some((KeyCommand::SeekRelative(1000), 6)),
                _ => {}
            }
        }

        // Standard ANSI arrow keys
        match bytes[2] {
            b'D' => return Some((KeyCommand::SeekRelative(-5000), 3)), // Left Arrow
            b'C' => return Some((KeyCommand::SeekRelative(5000), 3)),  // Right Arrow
            b'A' => return Some((KeyCommand::AdjustVolume(0.05), 3)),  // Up Arrow
            b'B' => return Some((KeyCommand::AdjustVolume(-0.05), 3)), // Down Arrow
            _ => {}
        }
    }

    None
}

/// Renders a visual scrubber progress bar with current position, duration, and filled track.
///
/// Output format: `[━━━━━━━●────────────────────]`
pub fn render_scrubber_bar(current_ms: u64, duration_ms: u64, bar_width: usize) -> String {
    let bar_width = bar_width.max(10);
    let inner_width = bar_width.saturating_sub(2);

    let progress = if duration_ms > 0 {
        (current_ms as f64 / duration_ms as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let head_pos = ((progress * (inner_width.saturating_sub(1) as f64)).round() as usize)
        .min(inner_width.saturating_sub(1));

    let mut out = String::with_capacity(bar_width * 4);
    out.push('[');

    // Filled section: bright cyan heavy line
    for _ in 0..head_pos {
        out.push_str("\x1b[1;36m━\x1b[0m");
    }

    // Seekhead indicator: glowing white bullet
    out.push_str("\x1b[1;37m●\x1b[0m");

    // Unfilled section: dark gray line
    for _ in (head_pos + 1)..inner_width {
        out.push_str("\x1b[90m─\x1b[0m");
    }

    out.push(']');
    out
}

/// State parameters for rendering the OSD HUD line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OsdHudState {
    /// Current media presentation time in milliseconds.
    pub current_ms: u64,
    /// Total media duration in milliseconds.
    pub duration_ms: u64,
    /// Whether playback is actively running (true) or paused (false).
    pub is_playing: bool,
    /// Current playback speed multiplier (e.g. 1.00x).
    pub speed: f64,
    /// Current audio volume [0.0, 1.0].
    pub volume: f32,
    /// Whether audio output is currently muted.
    pub is_muted: bool,
    /// Round-trip time in milliseconds to watch-party host or peer.
    pub rtt_ms: u64,
    /// Phase error in milliseconds relative to reference clock.
    pub phase_err_ms: f64,
}

/// Renders the HUD status line displaying timecode, transport state, speed, volume, and sync health.
pub fn render_osd_hud(state: &OsdHudState) -> String {
    let timecode = format_timecode_hud(state.current_ms, state.duration_ms);
    let state_badge = if state.is_playing {
        "\x1b[1;32m▶ PLAY\x1b[0m"
    } else {
        "\x1b[1;33m❚❚ PAUSE\x1b[0m"
    };

    let vol_pct = (state.volume * 100.0).round() as u32;
    let vol_str = if state.is_muted {
        "\x1b[1;31mMUTE\x1b[0m".to_string()
    } else {
        format!("{}%", vol_pct)
    };

    let badge_color = evaluate_sync_health(state.phase_err_ms, 0.0);
    let sync_str = match badge_color {
        SyncBadgeColor::Green => "\x1b[1;32m● SYNCED\x1b[0m",
        SyncBadgeColor::Amber => "\x1b[1;33m● DRIFT\x1b[0m",
        SyncBadgeColor::Red => "\x1b[1;31m● SEEK\x1b[0m",
    };

    format!(
        " {}  {}  \x1b[1;37m[{}]\x1b[0m  Speed: \x1b[1m{:.2}x\x1b[0m  Vol: \x1b[1m{}\x1b[0m  RTT: \x1b[1m{}ms\x1b[0m  Err: \x1b[1m{:+.1}ms\x1b[0m",
        state_badge, sync_str, timecode, state.speed, vol_str, state.rtt_ms, state.phase_err_ms
    )
}

/// Queries current terminal dimensions (columns, rows).
///
/// Falls back to `(80, 24)` if querying via `stty size` fails or stdout is not a TTY.
pub fn get_terminal_size() -> (usize, usize) {
    if let Ok(out) = Command::new("stty").arg("size").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            let mut parts = s.split_whitespace();
            if let (Some(rows_str), Some(cols_str)) = (parts.next(), parts.next()) {
                if let (Ok(rows), Ok(cols)) = (rows_str.parse::<usize>(), cols_str.parse::<usize>()) {
                    if rows > 0 && cols > 0 {
                        return (cols, rows);
                    }
                }
            }
        }
    }

    let cols = std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(80);
    let rows = std::env::var("LINES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(24);
    (cols, rows)
}

/// Renders the bottom keyboard shortcuts helper bar.
pub fn render_hotkeys_footer(width: usize) -> String {
    let full_text = " [Space] Play/Pause  [←/→] ±5s  [[/]] Speed  [M] Mute  [↑/↓] Vol  [T] Chat  [S] Sync  [Q] Quit ";
    let compact_text = " [Space] Play/Pause  [←/→] Seek  [[/]] Speed  [M] Mute  [Q] Quit ";
    let full_chars = full_text.chars().count();
    let compact_chars = compact_text.chars().count();

    let (text, text_chars) = if width >= full_chars {
        (full_text, full_chars)
    } else if width >= compact_chars {
        (compact_text, compact_chars)
    } else {
        (" [Space] Play/Pause  [Q] Quit ", " [Space] Play/Pause  [Q] Quit ".chars().count())
    };

    if width > text_chars {
        let pad = width - text_chars;
        let left_pad = pad / 2;
        let right_pad = pad - left_pad;
        format!(
            "\x1b[48;5;236m\x1b[37m{}{}{}\x1b[0m",
            " ".repeat(left_pad),
            text,
            " ".repeat(right_pad)
        )
    } else {
        format!("\x1b[48;5;236m\x1b[37m{}\x1b[0m", text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_character_keys() {
        assert_eq!(
            parse_key_sequence(b" "),
            Some((KeyCommand::TogglePause, 1))
        );
        assert_eq!(parse_key_sequence(b"q"), Some((KeyCommand::Quit, 1)));
        assert_eq!(parse_key_sequence(b"Q"), Some((KeyCommand::Quit, 1)));
        assert_eq!(parse_key_sequence(b"m"), Some((KeyCommand::ToggleMute, 1)));
        assert_eq!(
            parse_key_sequence(b"t"),
            Some((KeyCommand::ToggleChatOverlay, 1))
        );
        assert_eq!(
            parse_key_sequence(b"s"),
            Some((KeyCommand::ToggleSyncHud, 1))
        );
        assert_eq!(
            parse_key_sequence(b"/"),
            Some((KeyCommand::OpenCommandPrompt, 1))
        );
        assert_eq!(
            parse_key_sequence(b"["),
            Some((KeyCommand::ShiftSpeed(-1), 1))
        );
        assert_eq!(
            parse_key_sequence(b"]"),
            Some((KeyCommand::ShiftSpeed(1), 1))
        );
    }

    #[test]
    fn test_parse_ansi_arrow_keys() {
        // Left arrow: \x1b[D
        assert_eq!(
            parse_key_sequence(b"\x1b[D"),
            Some((KeyCommand::SeekRelative(-5000), 3))
        );
        // Right arrow: \x1b[C
        assert_eq!(
            parse_key_sequence(b"\x1b[C"),
            Some((KeyCommand::SeekRelative(5000), 3))
        );
        // Up arrow: \x1b[A
        assert_eq!(
            parse_key_sequence(b"\x1b[A"),
            Some((KeyCommand::AdjustVolume(0.05), 3))
        );
        // Down arrow: \x1b[B
        assert_eq!(
            parse_key_sequence(b"\x1b[B"),
            Some((KeyCommand::AdjustVolume(-0.05), 3))
        );
        // Shift + Left arrow: \x1b[1;2D
        assert_eq!(
            parse_key_sequence(b"\x1b[1;2D"),
            Some((KeyCommand::SeekRelative(-1000), 6))
        );
        // Shift + Right arrow: \x1b[1;2C
        assert_eq!(
            parse_key_sequence(b"\x1b[1;2C"),
            Some((KeyCommand::SeekRelative(1000), 6))
        );
    }

    #[test]
    fn test_render_scrubber_bar() {
        let bar = render_scrubber_bar(500, 1000, 20);
        assert!(bar.starts_with('['));
        assert!(bar.ends_with(']'));
        assert!(bar.contains('●'));

        // Zero duration
        let bar_zero = render_scrubber_bar(0, 0, 15);
        assert!(bar_zero.starts_with('['));
        assert!(bar_zero.ends_with(']'));
    }

    #[test]
    fn test_render_osd_hud() {
        let state = OsdHudState {
            current_ms: 10000,
            duration_ms: 60000,
            is_playing: true,
            speed: 1.0,
            volume: 0.8,
            is_muted: false,
            rtt_ms: 42,
            phase_err_ms: 5.0,
        };
        let hud = render_osd_hud(&state);
        assert!(hud.contains("00:00:10"));
        assert!(hud.contains("00:01:00"));
        assert!(hud.contains("80%"));
        assert!(hud.contains("42ms"));
        assert!(hud.contains("SYNCED"));
    }

    #[test]
    fn test_render_hotkeys_footer() {
        let footer = render_hotkeys_footer(100);
        assert!(footer.contains("Play/Pause"));
        assert!(footer.contains("Speed"));
        assert!(footer.contains("Quit"));
    }
}
