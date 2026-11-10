//! "Jump to Specific Time" modal dialog (Ctrl+T) with timecode parsing and quick offset buttons.

use egui::{Align2, Color32, Context, CornerRadius, Key, RichText, Stroke, Vec2, Window};
use crate::gui::controls::ControlsBarState;

/// Parses a timecode string into milliseconds.
///
/// Supported formats:
/// - `HH:MM:SS` (e.g. `"01:23:45"` -> 5,025,000 ms)
/// - `MM:SS` (e.g. `"12:30"` -> 750,000 ms)
/// - Pure seconds (e.g. `"90"` or `"90.5"` -> 90,000 or 90,500 ms)
/// - Relative offsets relative to `current_ms` (e.g. `"+30"`, `"-15s"`, `"+2m"`, `"-5m"`)
pub fn parse_time_input(input: &str, current_ms: u64, duration_ms: u64) -> Result<u64, String> {
    let text = input.trim();
    if text.is_empty() {
        return Err("Please enter a timecode".to_string());
    }

    // Relative offset (+/-)
    if text.starts_with('+') || text.starts_with('-') {
        let is_positive = text.starts_with('+');
        let remainder = &text[1..].trim();
        let (num_str, multiplier) = if remainder.ends_with('m') || remainder.ends_with('M') {
            (&remainder[..remainder.len() - 1].trim(), 60_000.0)
        } else if remainder.ends_with('s') || remainder.ends_with('S') {
            (&remainder[..remainder.len() - 1].trim(), 1_000.0)
        } else {
            (remainder, 1_000.0) // default unit is seconds
        };

        let val: f64 = num_str.parse().map_err(|_| format!("Invalid offset: '{}'", text))?;
        let delta_ms = (val * multiplier) as i64;
        let signed_delta = if is_positive { delta_ms } else { -delta_ms };
        let new_pos = (current_ms as i64 + signed_delta).max(0) as u64;
        return Ok(if duration_ms > 0 { new_pos.min(duration_ms) } else { new_pos });
    }

    // Colons: HH:MM:SS or MM:SS
    if text.contains(':') {
        let parts: Vec<&str> = text.split(':').collect();
        let total_ms = match parts.len() {
            2 => {
                let mins: u64 = parts[0].trim().parse().map_err(|_| "Invalid minutes format")?;
                let secs: f64 = parts[1].trim().parse().map_err(|_| "Invalid seconds format")?;
                mins * 60_000 + (secs * 1000.0) as u64
            }
            3 => {
                let hours: u64 = parts[0].trim().parse().map_err(|_| "Invalid hours format")?;
                let mins: u64 = parts[1].trim().parse().map_err(|_| "Invalid minutes format")?;
                let secs: f64 = parts[2].trim().parse().map_err(|_| "Invalid seconds format")?;
                hours * 3_600_000 + mins * 60_000 + (secs * 1000.0) as u64
            }
            _ => return Err("Invalid time format (expected MM:SS or HH:MM:SS)".to_string()),
        };
        return Ok(if duration_ms > 0 { total_ms.min(duration_ms) } else { total_ms });
    }

    // Total seconds or with suffix "s" / "m"
    let (num_str, multiplier) = if text.ends_with('m') || text.ends_with('M') {
        (&text[..text.len() - 1].trim(), 60_000.0)
    } else if text.ends_with('s') || text.ends_with('S') {
        (&text[..text.len() - 1].trim(), 1_000.0)
    } else {
        (&text, 1_000.0)
    };

    let val: f64 = num_str.parse().map_err(|_| format!("Invalid number: '{}'", text))?;
    let total_ms = (val * multiplier).max(0.0) as u64;
    Ok(if duration_ms > 0 { total_ms.min(duration_ms) } else { total_ms })
}

/// Modal dialog for jumping to a specific playback time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct JumpToTimeDialog {
    /// Whether the dialog window is open.
    pub is_open: bool,
    /// Editable text field buffer for timecode input.
    pub input_text: String,
    /// Parse or validation error text to display, if any.
    pub error_message: Option<String>,
    /// Needs initial keyboard focus on text input.
    needs_focus: bool,
}

impl JumpToTimeDialog {
    /// Constructs a new `JumpToTimeDialog`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the dialog prefilled with the current playback timecode.
    pub fn open(&mut self, current_ms: u64) {
        self.is_open = true;
        self.input_text = ControlsBarState::format_timestamp(current_ms);
        self.error_message = None;
        self.needs_focus = true;
    }

    /// Closes the dialog.
    pub fn close(&mut self) {
        self.is_open = false;
        self.error_message = None;
    }

    /// Renders the modal window dialog, returning `Some(target_pts_ms)` if confirmed.
    pub fn show(&mut self, ctx: &Context, current_ms: u64, duration_ms: u64) -> Option<u64> {
        if !self.is_open {
            return None;
        }

        let mut target_seek_ms: Option<u64> = None;
        let mut close_dialog = false;

        // Escape cancels and closes dialog
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            close_dialog = true;
        }

        Window::new("⏱ Jump to Specific Time")
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .fixed_pos(ctx.content_rect().center())
            .fixed_size(Vec2::new(360.0, 220.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 10.0);

                // Header time info
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Current:").color(Color32::from_rgb(160, 160, 175)));
                    ui.label(RichText::new(ControlsBarState::format_timestamp(current_ms)).strong().color(Color32::WHITE));
                    ui.label(RichText::new("/").color(Color32::from_rgb(100, 100, 115)));
                    ui.label(RichText::new(ControlsBarState::format_timestamp(duration_ms)).color(Color32::from_rgb(160, 160, 175)));
                });

                ui.separator();

                // Input label and text edit
                ui.label(RichText::new("Enter time (HH:MM:SS, MM:SS, seconds, or +/-offset):").small().color(Color32::from_rgb(200, 200, 215)));

                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.input_text)
                        .desired_width(ui.available_width())
                        .font(egui::TextStyle::Monospace)
                        .hint_text("e.g. 01:23:45 or 45:30 or +30s"),
                );

                if self.needs_focus {
                    edit.request_focus();
                    self.needs_focus = false;
                }

                // Error message banner
                if let Some(ref err) = self.error_message {
                    ui.colored_label(Color32::from_rgb(239, 68, 68), format!("⚠ {}", err));
                }

                // Quick jump offset buttons
                ui.horizontal_wrapped(|ui| {
                    let quick_offsets: [(&str, i64); 6] = [
                        ("-5m", -300_000),
                        ("-1m", -60_000),
                        ("-10s", -10_000),
                        ("+10s", 10_000),
                        ("+1m", 60_000),
                        ("+5m", 300_000),
                    ];
                    for (label, delta_ms) in quick_offsets {
                        if ui.button(label).clicked() {
                            let new_pos = (current_ms as i64 + delta_ms).max(0) as u64;
                            let bounded = if duration_ms > 0 { new_pos.min(duration_ms) } else { new_pos };
                            self.input_text = ControlsBarState::format_timestamp(bounded);
                            self.error_message = None;
                        }
                    }
                });

                ui.separator();

                // Action buttons (Jump / Cancel) or Enter key
                let enter_pressed = edit.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter));
                ui.horizontal(|ui| {
                    let jump_clicked = ui.add(
                        egui::Button::new(RichText::new("🚀 Jump to Time").strong().color(Color32::from_rgb(10, 20, 30)))
                            .fill(Color32::from_rgb(56, 189, 248))
                            .stroke(Stroke::new(1.0, Color32::from_rgb(125, 211, 252)))
                            .corner_radius(CornerRadius::same(6)),
                    ).clicked();

                    if jump_clicked || enter_pressed {
                        match parse_time_input(&self.input_text, current_ms, duration_ms) {
                            Ok(pts) => {
                                target_seek_ms = Some(pts);
                                close_dialog = true;
                            }
                            Err(e) => {
                                self.error_message = Some(e);
                            }
                        }
                    }

                    if ui.button("Cancel").clicked() {
                        close_dialog = true;
                    }
                });
            });

        if close_dialog {
            self.close();
        }

        target_seek_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hh_mm_ss() {
        assert_eq!(parse_time_input("01:23:45", 0, 10_000_000), Ok(5_025_000));
        assert_eq!(parse_time_input("00:00:10", 0, 10_000_000), Ok(10_000));
    }

    #[test]
    fn test_parse_mm_ss() {
        assert_eq!(parse_time_input("12:30", 0, 10_000_000), Ok(750_000));
        assert_eq!(parse_time_input("01:05", 0, 10_000_000), Ok(65_000));
    }

    #[test]
    fn test_parse_pure_seconds_and_suffixes() {
        assert_eq!(parse_time_input("90", 0, 10_000_000), Ok(90_000));
        assert_eq!(parse_time_input("45s", 0, 10_000_000), Ok(45_000));
        assert_eq!(parse_time_input("2m", 0, 10_000_000), Ok(120_000));
    }

    #[test]
    fn test_parse_relative_offsets() {
        let current = 60_000; // 1 minute
        assert_eq!(parse_time_input("+30s", current, 10_000_000), Ok(90_000));
        assert_eq!(parse_time_input("-15", current, 10_000_000), Ok(45_000));
        assert_eq!(parse_time_input("+1m", current, 10_000_000), Ok(120_000));
        // Underflow clamped to 0
        assert_eq!(parse_time_input("-2m", current, 10_000_000), Ok(0));
    }

    #[test]
    fn test_parse_duration_clamping() {
        let max_dur = 100_000;
        assert_eq!(parse_time_input("05:00:00", 0, max_dur), Ok(max_dur));
        assert_eq!(parse_time_input("+500s", 0, max_dur), Ok(max_dur));
    }

    #[test]
    fn test_parse_invalid_inputs() {
        assert!(parse_time_input("", 0, 10_000_000).is_err());
        assert!(parse_time_input("abc", 0, 10_000_000).is_err());
        assert!(parse_time_input("1:2:3:4", 0, 10_000_000).is_err());
    }
}
