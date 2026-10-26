//! On-Screen Display (OSD) HUD formatting, tight sync health evaluations, and floating comments.

pub use crate::overlay::{evaluate_sync_health, format_timecode_hud, ChatBubble, SyncBadgeColor};

/// Tri-color tight sync health badge evaluation for interactive player HUD.
///
/// Thresholds:
/// - Green: `|e| < 15.0ms` (Tight in-sync)
/// - Yellow: `15.0ms <= |e| <= 50.0ms` (Micro-drift steering active)
/// - Red: `|e| > 50.0ms` (Significant drift / desync)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TightSyncBadge {
    /// Green: Phase error |e| < 15ms.
    Green,
    /// Yellow: Phase error 15ms <= |e| <= 50ms.
    Yellow,
    /// Red: Phase error |e| > 50ms.
    Red,
}

impl TightSyncBadge {
    /// Evaluates badge from phase error in milliseconds.
    pub fn from_phase_error(phase_error_ms: f64) -> Self {
        let abs_e = phase_error_ms.abs();
        if abs_e < 15.0 {
            TightSyncBadge::Green
        } else if abs_e <= 50.0 {
            TightSyncBadge::Yellow
        } else {
            TightSyncBadge::Red
        }
    }

    /// Short textual badge label.
    pub const fn label(&self) -> &'static str {
        match self {
            TightSyncBadge::Green => "SYNCED",
            TightSyncBadge::Yellow => "DRIFT",
            TightSyncBadge::Red => "DESYNC",
        }
    }

    /// ANSI-colored badge string.
    pub const fn ansi_badge(&self) -> &'static str {
        match self {
            TightSyncBadge::Green => "\x1b[1;32m● SYNCED\x1b[0m",
            TightSyncBadge::Yellow => "\x1b[1;33m● DRIFT\x1b[0m",
            TightSyncBadge::Red => "\x1b[1;31m● DESYNC\x1b[0m",
        }
    }

    /// Maps to OpenSpec operational PLL zone (1, 2, or 3).
    ///
    /// Note: OpenSpec defines Zone 1 (<15ms), Zone 2 (15-1500ms), Zone 3 (>1500ms).
    pub fn to_openspec_zone(&self, phase_error_ms: f64) -> u8 {
        let abs_e = phase_error_ms.abs();
        if abs_e < 15.0 {
            1
        } else if abs_e <= 1500.0 {
            2
        } else {
            3
        }
    }

    /// Maps OpenSpec zone (1, 2, 3) to a corresponding sync badge color.
    pub fn from_openspec_zone(zone: u8) -> Self {
        match zone {
            1 => TightSyncBadge::Green,
            2 => TightSyncBadge::Yellow,
            _ => TightSyncBadge::Red,
        }
    }
}

/// Evaluates tight sync health badge returning `(label, ansi_badge)`.
pub fn evaluate_tight_sync_badge(phase_error_ms: f64) -> (&'static str, &'static str) {
    let badge = TightSyncBadge::from_phase_error(phase_error_ms);
    (badge.label(), badge.ansi_badge())
}

/// Computes the linear opacity factor for a floating comment.
///
/// Follows a 4.0s dwell (full opacity 1.0) followed by 1.0s linear fade-out:
/// - `dt <= 4.0s`: 1.0
/// - `4.0s < dt <= 5.0s`: `1.0 - (dt - 4.0) = 5.0 - dt`
/// - `dt > 5.0s`: 0.0 (evicted)
pub fn calculate_alpha_dwell_fade(
    arrival_time_secs: f64,
    current_time_secs: f64,
    dwell_secs: f64,
    fade_secs: f64,
) -> f32 {
    let dt = (current_time_secs - arrival_time_secs).max(0.0);
    if dt <= dwell_secs {
        1.0
    } else if dt <= dwell_secs + fade_secs {
        let fade_progress = (dt - dwell_secs) / fade_secs;
        (1.0 - fade_progress as f32).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Formats a floating bullet comment with 24-bit TrueColor alpha blending into terminal background.
///
/// Uses linear color interpolation:
/// `C = round(C_bg + alpha * (C_fg - C_bg))`
/// Returns `None` if the comment has expired (`dt > 5.0s`).
pub fn render_faded_chat_comment(
    bubble: &ChatBubble,
    current_time_secs: f64,
    bg_rgb: (u8, u8, u8),
    fg_rgb: (u8, u8, u8),
) -> Option<String> {
    let alpha = calculate_alpha_dwell_fade(bubble.arrival_time_secs, current_time_secs, 4.0, 1.0);
    if alpha <= 0.0 {
        return None;
    }

    let r = (bg_rgb.0 as f32 + alpha * (fg_rgb.0 as f32 - bg_rgb.0 as f32)).round() as u8;
    let g = (bg_rgb.1 as f32 + alpha * (fg_rgb.1 as f32 - bg_rgb.1 as f32)).round() as u8;
    let b = (bg_rgb.2 as f32 + alpha * (fg_rgb.2 as f32 - bg_rgb.2 as f32)).round() as u8;

    Some(format!(
        "\x1b[38;2;{};{};{}m● {}\x1b[0m",
        r, g, b, bubble.message
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tight_sync_badge_thresholds() {
        assert_eq!(TightSyncBadge::from_phase_error(0.0), TightSyncBadge::Green);
        assert_eq!(TightSyncBadge::from_phase_error(14.9), TightSyncBadge::Green);
        assert_eq!(TightSyncBadge::from_phase_error(-14.9), TightSyncBadge::Green);
        assert_eq!(TightSyncBadge::from_phase_error(15.0), TightSyncBadge::Yellow);
        assert_eq!(TightSyncBadge::from_phase_error(50.0), TightSyncBadge::Yellow);
        assert_eq!(TightSyncBadge::from_phase_error(50.1), TightSyncBadge::Red);
        assert_eq!(TightSyncBadge::from_phase_error(-120.0), TightSyncBadge::Red);

        let (label, ansi) = evaluate_tight_sync_badge(10.0);
        assert_eq!(label, "SYNCED");
        assert!(ansi.contains("SYNCED"));
    }

    #[test]
    fn test_alpha_dwell_and_fade() {
        let t0 = 100.0;
        // Dwell period
        assert_eq!(calculate_alpha_dwell_fade(t0, 100.0, 4.0, 1.0), 1.0);
        assert_eq!(calculate_alpha_dwell_fade(t0, 103.5, 4.0, 1.0), 1.0);
        assert_eq!(calculate_alpha_dwell_fade(t0, 104.0, 4.0, 1.0), 1.0);

        // Linear fade period
        let mid_fade = calculate_alpha_dwell_fade(t0, 104.5, 4.0, 1.0);
        assert!((mid_fade - 0.5).abs() < 1e-4);

        // Post-fade eviction
        assert_eq!(calculate_alpha_dwell_fade(t0, 105.1, 4.0, 1.0), 0.0);
    }

    #[test]
    fn test_render_faded_chat_comment() {
        let bubble = ChatBubble {
            message: "Hello watch party!".to_string(),
            arrival_time_secs: 10.0,
        };
        let bg = (0, 0, 0);
        let fg = (200, 200, 200);

        let res_dwell = render_faded_chat_comment(&bubble, 12.0, bg, fg);
        assert!(res_dwell.is_some());
        assert!(res_dwell.unwrap().contains("200;200;200"));

        let res_expired = render_faded_chat_comment(&bubble, 16.0, bg, fg);
        assert!(res_expired.is_none());
    }
}
