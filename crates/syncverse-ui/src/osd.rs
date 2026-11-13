//! On-Screen Display (OSD) HUD formatting, tight sync health evaluations, and floating notification overlay.

use std::time::{Duration, Instant};

pub use crate::overlay::{evaluate_sync_health, format_timecode_hud, ChatBubble, SyncBadgeColor};

/// On-Screen Display (OSD) HUD notification banner overlay.
///
/// Features a floating translucent pill / banner positioned at the top-left or top-center
/// of the video viewport, with a 1.5s dwell period (alpha 1.0) followed by a 0.5s smooth
/// linear alpha fade-out (total 2.0s duration).
#[derive(Debug, Clone)]
pub struct OsdOverlay {
    /// Active notification text.
    active_text: Option<String>,
    /// Instant when current notification was triggered.
    trigger_time: Option<Instant>,
    /// Visible dwell period duration (1.5s).
    dwell_duration: Duration,
    /// Linear alpha fade duration (0.5s).
    fade_duration: Duration,
}

impl Default for OsdOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl OsdOverlay {
    /// Constructs a new `OsdOverlay` with standard 1.5s dwell and 0.5s fade durations (total 2.0s).
    pub fn new() -> Self {
        Self {
            active_text: None,
            trigger_time: None,
            dwell_duration: Duration::from_millis(1500),
            fade_duration: Duration::from_millis(500),
        }
    }

    /// Triggers a new OSD HUD notification banner with the provided text at the given instant.
    ///
    /// If an identical toast is already active, its display timer is seamlessly refreshed
    /// without visual flicker or re-allocation.
    pub fn show_notification(&mut self, text: impl Into<String>, timestamp: Instant) {
        let text_str = text.into();
        if self.active_text.as_deref() == Some(&text_str) && self.opacity(timestamp) > 0.0 {
            self.trigger_time = Some(timestamp);
            return;
        }
        self.active_text = Some(text_str);
        self.trigger_time = Some(timestamp);
    }

    /// Calculates current opacity factor in range [0.0, 1.0].
    ///
    /// - `dt <= 1.5s`: 1.0 (full opacity)
    /// - `1.5s < dt <= 2.0s`: smooth linear fade from 1.0 down to 0.0
    /// - `dt > 2.0s`: 0.0 (expired)
    pub fn opacity(&self, now: Instant) -> f32 {
        let trigger = match self.trigger_time {
            Some(t) => t,
            None => return 0.0,
        };

        if now < trigger {
            return 1.0;
        }

        let elapsed = now.duration_since(trigger);
        let total_duration = self.dwell_duration + self.fade_duration;

        if elapsed >= total_duration {
            0.0
        } else if elapsed <= self.dwell_duration {
            1.0
        } else {
            let fade_elapsed = elapsed - self.dwell_duration;
            let fade_progress = fade_elapsed.as_secs_f32() / self.fade_duration.as_secs_f32();
            (1.0 - fade_progress).clamp(0.0, 1.0)
        }
    }

    /// Returns the active notification text and current alpha factor if not expired.
    pub fn active_notification(&self, now: Instant) -> Option<(&str, f32)> {
        let alpha = self.opacity(now);
        if alpha > 0.0 {
            self.active_text.as_deref().map(|t| (t, alpha))
        } else {
            None
        }
    }

    /// Clears any active notification immediately.
    pub fn clear(&mut self) {
        self.active_text = None;
        self.trigger_time = None;
    }

    /// Renders the floating translucent pill banner directly inside the video viewport rect.
    ///
    /// Delegates to [`OsdOverlay::show_in_viewport`] so text wrapping, clipping, and margin bounds
    /// remain strictly contained within `viewport_rect`.
    pub fn render(&self, ui: &mut egui::Ui, viewport_rect: egui::Rect, now: Instant) {
        self.show_in_viewport(ui, viewport_rect, now);
    }

    /// Renders the floating translucent pill banner strictly bounded inside `viewport_rect.shrink(16.0)`
    /// at the top-left of the video viewport so it never collides with top banners or the sidebar.
    pub fn show_in_viewport(&self, ui: &mut egui::Ui, viewport_rect: egui::Rect, now: Instant) {
        let (text, alpha) = match self.active_notification(now) {
            Some((t, a)) => (t, a),
            None => return,
        };

        if !viewport_rect.is_positive() || viewport_rect.width() < 24.0 || viewport_rect.height() < 24.0 {
            return;
        }

        let bounded_rect = if viewport_rect.width() > 48.0 && viewport_rect.height() > 48.0 {
            viewport_rect.shrink(16.0)
        } else {
            viewport_rect.shrink(4.0)
        };

        let padding = egui::vec2(14.0, 8.0);
        let max_text_width = (bounded_rect.width() - padding.x * 2.0).max(40.0);

        let font_id = egui::FontId::proportional(15.0);
        let text_color = egui::Color32::from_rgba_premultiplied(
            (255.0 * alpha).round() as u8,
            (255.0 * alpha).round() as u8,
            (255.0 * alpha).round() as u8,
            (255.0 * alpha).round() as u8,
        );
        let galley = ui
            .painter()
            .layout(text.to_string(), font_id, text_color, max_text_width);

        let pill_w = (galley.size().x + padding.x * 2.0).min(bounded_rect.width());
        let pill_h = (galley.size().y + padding.y * 2.0).min(bounded_rect.height());
        let pill_rect = egui::Rect::from_min_size(bounded_rect.min, egui::vec2(pill_w, pill_h));

        // Dark semi-transparent background: #0f172a with ~85% opacity
        let bg_color = egui::Color32::from_rgba_premultiplied(
            (15.0 * 0.85 * alpha).round() as u8,
            (23.0 * 0.85 * alpha).round() as u8,
            (42.0 * 0.85 * alpha).round() as u8,
            (217.0 * alpha).round() as u8,
        );

        let border_stroke = egui::Stroke::new(
            1.0,
            egui::Color32::from_rgba_premultiplied(
                (56.0 * 0.5 * alpha).round() as u8,
                (189.0 * 0.5 * alpha).round() as u8,
                (248.0 * 0.5 * alpha).round() as u8,
                (128.0 * alpha).round() as u8,
            ),
        );

        let outer_clip = bounded_rect.intersect(ui.clip_rect());
        ui.painter().with_clip_rect(outer_clip).rect(
            pill_rect,
            egui::CornerRadius::same(6),
            bg_color,
            border_stroke,
            egui::StrokeKind::Outside,
        );
        let text_pos = pill_rect.min + padding;
        let text_clip = pill_rect.shrink(2.0).intersect(ui.clip_rect());
        ui.painter()
            .with_clip_rect(text_clip)
            .galley(text_pos, galley, egui::Color32::TRANSPARENT);
    }

    /// Renders the floating translucent pill banner using an `egui::Area` constrained to
    /// `ctx.content_rect()` on `Order::Middle` so it never overflows narrow windows or covers modals.
    pub fn show(&self, ctx: &egui::Context, now: Instant) {
        let (text, alpha) = match self.active_notification(now) {
            Some((t, a)) => (t, a),
            None => return,
        };

        let content_rect = ctx.content_rect();
        let max_pill_w = (content_rect.width() - 48.0).max(120.0);
        let max_text_w = (max_pill_w - 28.0).max(60.0);
        let offset_x = if content_rect.width() < 280.0 { 12.0 } else { 24.0 };
        let offset_y = if content_rect.height() < 200.0 { 24.0 } else { 48.0 };

        egui::Area::new(egui::Id::unique("osd_overlay_pill_area"))
            .anchor(egui::Align2::LEFT_TOP, [offset_x, offset_y])
            .order(egui::Order::Middle)
            .constrain_to(content_rect)
            .interactable(false)
            .show(ctx, |ui| {
                let bg_color = egui::Color32::from_rgba_premultiplied(
                    (15.0 * 0.88 * alpha).round() as u8,
                    (23.0 * 0.88 * alpha).round() as u8,
                    (42.0 * 0.88 * alpha).round() as u8,
                    (225.0 * alpha).round() as u8,
                );
                let border_stroke = egui::Stroke::new(
                    1.0,
                    egui::Color32::from_rgba_premultiplied(
                        (56.0 * 0.6 * alpha).round() as u8,
                        (189.0 * 0.6 * alpha).round() as u8,
                        (248.0 * 0.6 * alpha).round() as u8,
                        (150.0 * alpha).round() as u8,
                    ),
                );
                let text_color = egui::Color32::from_rgba_premultiplied(
                    (255.0 * alpha).round() as u8,
                    (255.0 * alpha).round() as u8,
                    (255.0 * alpha).round() as u8,
                    (255.0 * alpha).round() as u8,
                );

                egui::Frame::popup(ui.style())
                    .fill(bg_color)
                    .stroke(border_stroke)
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.set_max_width(max_text_w);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(text)
                                    .font(egui::FontId::proportional(15.0))
                                    .strong()
                                    .color(text_color),
                            )
                            .wrap(),
                        );
                    });
            });
    }
}

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

    #[test]
    fn test_osd_overlay_lifecycle() {
        let mut osd = OsdOverlay::new();
        let t0 = Instant::now();

        // Initially no active notification
        assert_eq!(osd.opacity(t0), 0.0);
        assert!(osd.active_notification(t0).is_none());

        // Trigger notification
        osd.show_notification("Audio: Track 2/3 - Japanese (AAC 2ch)", t0);

        // Within dwell period (1.5s): opacity = 1.0
        assert_eq!(osd.opacity(t0), 1.0);
        assert_eq!(osd.opacity(t0 + Duration::from_millis(500)), 1.0);
        assert_eq!(osd.opacity(t0 + Duration::from_millis(1500)), 1.0);

        let active = osd.active_notification(t0 + Duration::from_millis(1000));
        assert!(active.is_some());
        let (text, alpha) = active.unwrap();
        assert_eq!(text, "Audio: Track 2/3 - Japanese (AAC 2ch)");
        assert_eq!(alpha, 1.0);

        // Mid-fade at 1.75s: opacity ≈ 0.5
        let mid_alpha = osd.opacity(t0 + Duration::from_millis(1750));
        assert!((mid_alpha - 0.5).abs() < 1e-3);

        // At exactly 2.0s: opacity = 0.0, notification expired
        let end_alpha = osd.opacity(t0 + Duration::from_millis(2000));
        assert_eq!(end_alpha, 0.0);
        assert!(osd.active_notification(t0 + Duration::from_millis(2000)).is_none());

        // Beyond 2.0s: strictly expired
        assert_eq!(osd.opacity(t0 + Duration::from_millis(2500)), 0.0);
        assert!(osd.active_notification(t0 + Duration::from_millis(2500)).is_none());

        // Test clear
        osd.show_notification("Aspect Ratio: 16:9", t0);
        assert_eq!(osd.opacity(t0), 1.0);
        osd.clear();
        assert_eq!(osd.opacity(t0), 0.0);
        assert!(osd.active_notification(t0).is_none());
    }

    #[test]
    fn test_osd_headless_show_and_viewport_across_widths_and_dpi() {
        let widths = [
            200.0, 320.0, 480.0, 560.0, 640.0, 700.0, 800.0, 930.0, 1024.0, 1280.0, 1920.0,
        ];
        let dpis = [1.0_f32, 1.5, 2.0];

        let mut osd = OsdOverlay::new();
        let now = Instant::now();
        osd.show_notification(
            "Audio: Track 2/5 - Japanese 5.1 Surround (Dolby Digital Plus / E-AC3, +50 ms delay)",
            now,
        );

        for &ppp in &dpis {
            for &w in &widths {
                let ctx = egui::Context::default();
                ctx.set_pixels_per_point(ppp);

                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, 360.0));
                let raw_input = egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                };

                let mut output = ctx.run_ui(raw_input, |ui| {
                    let viewport_rect = egui::Rect::from_min_size(
                        egui::pos2(0.0, 32.0),
                        egui::vec2((w - 20.0).max(40.0), 240.0),
                    );
                    osd.show_in_viewport(ui, viewport_rect, now);
                    osd.show(ui.ctx(), now);
                });
                output.textures_delta.clear();
            }
        }
    }
}
