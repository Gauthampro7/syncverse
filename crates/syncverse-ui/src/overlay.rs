//! OSD overlays, bounded chat bubble display list, and single-channel Alpha8 font glyph atlas.

/// Floating chat bubble rendered on OSD.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatBubble {
    /// Text message or reaction content.
    pub message: String,
    /// Arrival time in seconds.
    pub arrival_time_secs: f64,
}

/// Bounded chat overlay display list capped at 5 visible bubbles.
#[derive(Debug, Clone)]
pub struct BoundedChatOverlay {
    max_visible: usize,
    visible: Vec<ChatBubble>,
    dropped: usize,
}

impl Default for BoundedChatOverlay {
    fn default() -> Self {
        Self::new(5)
    }
}

impl BoundedChatOverlay {
    /// Constructs a bounded chat bubble display list with the given maximum capacity.
    pub fn new(max_visible: usize) -> Self {
        Self {
            max_visible,
            visible: Vec::with_capacity(max_visible),
            dropped: 0,
        }
    }

    /// Receives a new message at wall instant `t_secs`.
    ///
    /// Evicts bubbles older than 5.0s (4.0s dwell + 1.0s fade), and caps visible count at `max_visible`.
    pub fn on_message(&mut self, message: String, t_secs: f64) {
        // Retain bubbles within active 5.0s window
        self.visible.retain(|b| (t_secs - b.arrival_time_secs) < 5.0);

        if self.visible.len() >= self.max_visible {
            self.visible.remove(0);
            self.dropped += 1;
        }

        self.visible.push(ChatBubble {
            message,
            arrival_time_secs: t_secs,
        });
    }

    /// Returns the currently active visible bubbles.
    pub fn visible_bubbles(&self) -> &[ChatBubble] {
        &self.visible
    }

    /// Total number of evicted/dropped bubbles.
    pub fn dropped_count(&self) -> usize {
        self.dropped
    }

    /// Estimated memory footprint of visible bubbles in bytes.
    pub fn estimated_memory_bytes(&self) -> usize {
        self.visible.len() * 256
    }
}

/// Single-channel Alpha8 (R8_UNORM) font glyph atlas texture model.
///
/// Bounded to 4.0 MB for 2048x2048 resolution (1 byte per texel), adhering to the 15MB UI budget.
#[derive(Debug, Clone)]
pub struct FontAtlasAlpha8 {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl FontAtlasAlpha8 {
    /// Allocates an Alpha8 font atlas texture of dimensions `width x height`.
    pub fn new(width: u32, height: u32) -> Self {
        let size = (width as usize) * (height as usize);
        Self {
            width,
            height,
            data: vec![0u8; size],
        }
    }

    /// Atlas width in texels.
    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Atlas height in texels.
    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Texture memory consumption in megabytes.
    #[inline]
    pub fn memory_mb(&self) -> f64 {
        (self.data.len() as f64) / (1024.0 * 1024.0)
    }

    /// Raw single-channel texel data slice.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }
}

/// Kind of transport indicator glyph rendered on OSD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportGlyphKind {
    /// Play glyph (▶).
    Play,
    /// Pause glyph (❚❚).
    Pause,
    /// Fast-forward seek glyph (►►).
    SeekForward,
    /// Rewind seek glyph (◄◄).
    SeekBackward,
}

/// Transport state indicator glyph with 600ms exponential opacity decay:
/// `alpha(t) = 0.8 * e^(-5t)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransportGlyph {
    /// Kind of transport glyph.
    pub kind: TransportGlyphKind,
    /// Initiation timestamp in seconds.
    pub start_time_secs: f64,
}

impl TransportGlyph {
    /// Creates a new `TransportGlyph` starting at wall instant `start_time_secs`.
    pub const fn new(kind: TransportGlyphKind, start_time_secs: f64) -> Self {
        Self {
            kind,
            start_time_secs,
        }
    }

    /// Calculates current opacity `alpha(t)` over a 600ms lifecycle:
    /// `alpha(t) = 0.8 * e^(-5.0 * dt)`, returning 0.0 when `dt > 0.6s`.
    pub fn opacity(&self, current_time_secs: f64) -> f32 {
        let dt = (current_time_secs - self.start_time_secs).max(0.0);
        if dt > 0.6 {
            0.0
        } else {
            (0.8 * (-5.0 * dt).exp()) as f32
        }
    }
}

/// Tri-color status indicator for network sync health diagnostic badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncBadgeColor {
    /// Green (#22C55E): Phase error |e(t)| < 15ms (Deadband in-sync).
    Green,
    /// Amber (#F59E0B): Phase error 15ms <= |e(t)| <= 1500ms (Active PLL micro-speed slew).
    Amber,
    /// Red (#EF4444): Phase error |e(t)| > 1500ms or packet loss > 10% (Desynchronized / Hard seek).
    Red,
}

/// Evaluates the tri-color sync status badge from phase error in ms and packet loss ratio.
pub fn evaluate_sync_health(phase_error_ms: f64, packet_loss_ratio: f64) -> SyncBadgeColor {
    let abs_e = phase_error_ms.abs();
    if abs_e > 1500.0 || packet_loss_ratio > 0.10 {
        SyncBadgeColor::Red
    } else if abs_e >= 15.0 {
        SyncBadgeColor::Amber
    } else {
        SyncBadgeColor::Green
    }
}

/// Formats monospace timecode HUD string: `HH:MM:SS / HH:MM:SS`.
pub fn format_timecode_hud(current_ms: u64, duration_ms: u64) -> String {
    let curr_sec = current_ms / 1000;
    let curr_h = curr_sec / 3600;
    let curr_m = (curr_sec % 3600) / 60;
    let curr_s = curr_sec % 60;

    let dur_sec = duration_ms / 1000;
    let dur_h = dur_sec / 3600;
    let dur_m = (dur_sec % 3600) / 60;
    let dur_s = dur_sec % 60;

    format!(
        "{:02}:{:02}:{:02} / {:02}:{:02}:{:02}",
        curr_h, curr_m, curr_s, dur_h, dur_m, dur_s
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_overlay_bounding_under_burst() {
        let mut overlay = BoundedChatOverlay::new(5);
        for i in 0..1000 {
            overlay.on_message(format!("Spam msg {}", i), i as f64 * 0.001);
        }

        assert!(overlay.visible_bubbles().len() <= 5);
        assert_eq!(overlay.dropped_count(), 995);
    }

    #[test]
    fn test_font_atlas_memory_budget() {
        let atlas_2048 = FontAtlasAlpha8::new(2048, 2048);
        assert!((atlas_2048.memory_mb() - 4.0).abs() < 1e-4);

        let atlas_1024 = FontAtlasAlpha8::new(1024, 1024);
        assert!((atlas_1024.memory_mb() - 1.0).abs() < 1e-4);

        // UI memory budget verification: Atlas (4MB) + Framebuffer (8.5MB) + State (1.0MB) = 13.5MB <= 15MB
        let total_ui_mb = atlas_2048.memory_mb() + 8.5 + 1.0;
        assert!(total_ui_mb <= 15.0);
    }

    #[test]
    fn test_transport_glyph_exponential_fade() {
        let glyph = TransportGlyph::new(TransportGlyphKind::Play, 10.0);
        // At t = 10.0: opacity = 0.8 * e^(0) = 0.8
        assert!((glyph.opacity(10.0) - 0.8).abs() < 1e-4);

        // At t = 10.2: opacity = 0.8 * e^(-1.0) ≈ 0.2943
        let op_02 = glyph.opacity(10.2);
        assert!((op_02 - 0.29430355).abs() < 1e-3);

        // At t = 10.6: opacity = 0.8 * e^(-3.0) ≈ 0.0398
        let op_06 = glyph.opacity(10.6);
        assert!((op_06 - 0.03982963).abs() < 1e-3);

        // Beyond 600ms (t = 10.601): opacity strictly 0.0
        assert_eq!(glyph.opacity(10.601), 0.0);
        assert_eq!(glyph.opacity(11.0), 0.0);
    }

    #[test]
    fn test_sync_health_badge_tri_color() {
        // Deadband zone 1: |e| < 15ms -> Green
        assert_eq!(evaluate_sync_health(0.0, 0.0), SyncBadgeColor::Green);
        assert_eq!(evaluate_sync_health(14.9, 0.0), SyncBadgeColor::Green);
        assert_eq!(evaluate_sync_health(-14.9, 0.02), SyncBadgeColor::Green);

        // Micro-speed zone 2: 15ms <= |e| <= 1500ms -> Amber
        assert_eq!(evaluate_sync_health(15.0, 0.0), SyncBadgeColor::Amber);
        assert_eq!(evaluate_sync_health(450.0, 0.05), SyncBadgeColor::Amber);
        assert_eq!(evaluate_sync_health(-1500.0, 0.08), SyncBadgeColor::Amber);

        // Hard seek zone 3: |e| > 1500ms -> Red
        assert_eq!(evaluate_sync_health(1501.0, 0.0), SyncBadgeColor::Red);
        assert_eq!(evaluate_sync_health(-2000.0, 0.0), SyncBadgeColor::Red);

        // Packet loss > 10% -> Red even if phase error is small
        assert_eq!(evaluate_sync_health(5.0, 0.11), SyncBadgeColor::Red);
    }

    #[test]
    fn test_format_timecode_hud() {
        let tc1 = format_timecode_hud(123_456, 3_661_000);
        assert_eq!(tc1, "00:02:03 / 01:01:01");

        let tc2 = format_timecode_hud(0, 0);
        assert_eq!(tc2, "00:00:00 / 00:00:00");
    }
}
