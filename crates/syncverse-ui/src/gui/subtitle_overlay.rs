//! Synchronized subtitle overlay rendering, proportional typography, and binary search cue lookup.

use egui::{
    pos2, vec2, Color32, CornerRadius, FontId, Painter, Rect, Stroke, StrokeKind, Ui, Vec2,
};

/// A timed subtitle cue representation for display timing and lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleCue {
    /// Start presentation timestamp in milliseconds.
    pub start_ms: i64,
    /// End presentation timestamp in milliseconds.
    pub end_ms: i64,
    /// Multiline or single-line subtitle text content.
    pub text: String,
}

impl SubtitleCue {
    /// Creates a new `SubtitleCue` with start and end timestamps in milliseconds.
    pub fn new(start_ms: i64, end_ms: i64, text: impl Into<String>) -> Self {
        Self {
            start_ms,
            end_ms,
            text: text.into(),
        }
    }

    /// Returns `true` if the cue is active at the specified presentation timestamp in milliseconds.
    #[inline]
    pub fn is_active_at(&self, pts_ms: i64) -> bool {
        self.start_ms <= pts_ms && pts_ms <= self.end_ms
    }

    /// Returns cue duration in milliseconds.
    #[inline]
    pub fn duration_ms(&self) -> i64 {
        (self.end_ms - self.start_ms).max(0)
    }
}

/// In-memory subtitle track representation for UI selection and rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleTrack {
    /// Unique track identifier or index.
    pub id: usize,
    /// Human-readable title or language name (e.g. "English (.srt)").
    pub title: String,
    /// Optional ISO 639 language code (e.g. "en", "ja").
    pub language: Option<String>,
    /// Chronologically ordered subtitle cues.
    pub cues: Vec<SubtitleCue>,
    /// Track format (e.g. "SRT", "VTT", "ASS", "SUB").
    pub format: String,
    /// Whether this track originated from an external file.
    pub is_external: bool,
}

impl SubtitleTrack {
    /// Constructs a new `SubtitleTrack`, ensuring cues are sorted in ascending order of `start_ms`.
    pub fn new(
        id: usize,
        title: impl Into<String>,
        language: Option<String>,
        mut cues: Vec<SubtitleCue>,
    ) -> Self {
        cues.sort_by_key(|c| c.start_ms);
        Self {
            id,
            title: title.into(),
            language,
            cues,
            format: "SRT".to_string(),
            is_external: false,
        }
    }

    /// Constructs a new `SubtitleTrack` with explicit format and origin.
    pub fn with_format_and_origin(
        id: usize,
        title: impl Into<String>,
        language: Option<String>,
        mut cues: Vec<SubtitleCue>,
        format: impl Into<String>,
        is_external: bool,
    ) -> Self {
        cues.sort_by_key(|c| c.start_ms);
        Self {
            id,
            title: title.into(),
            language,
            cues,
            format: format.into(),
            is_external,
        }
    }

    /// Adds a cue and maintains chronological ordering.
    pub fn add_cue(&mut self, cue: SubtitleCue) {
        let insert_idx = self.cues.partition_point(|c| c.start_ms <= cue.start_ms);
        self.cues.insert(insert_idx, cue);
    }

    /// Finds the first active cue at `effective_pts_ms`, if any.
    pub fn active_cue_at(&self, effective_pts_ms: i64) -> Option<&SubtitleCue> {
        find_active_cue(&self.cues, effective_pts_ms)
    }

    /// Finds the active cue text for a given PTS, concatenating overlapping cues if present.
    pub fn active_text_at(&self, effective_pts_ms: i64) -> Option<String> {
        find_active_cue_text(&self.cues, effective_pts_ms)
    }
}

/// Active subtitle track selection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActiveSubtitleTrack {
    /// Subtitles disabled / Off.
    #[default]
    Off,
    /// Selected subtitle track by 0-based index.
    Track(usize),
}

/// Computes the effective subtitle presentation timestamp in milliseconds,
/// applying the user-configured subtitle delay offset:
///
/// `effective_pts_ms = (media_position_pts / 1_000_000) as i64 + subtitle_delay_ms`
#[inline]
pub fn compute_effective_pts(media_position_pts: u64, subtitle_delay_ms: i64) -> i64 {
    (media_position_pts / 1_000_000) as i64 + subtitle_delay_ms
}

/// Finds the active subtitle cue for a given presentation timestamp in milliseconds.
///
/// Uses binary search (`partition_point`) over cues sorted by `start_ms`
/// with $O(\log N)$ lookup performance.
pub fn find_active_cue(cues: &[SubtitleCue], pts_ms: i64) -> Option<&SubtitleCue> {
    if cues.is_empty() || pts_ms < 0 {
        return None;
    }

    let upper = cues.partition_point(|cue| cue.start_ms <= pts_ms);
    if upper == 0 {
        return None;
    }

    // Inspect backwards from candidate index
    for i in (0..upper).rev() {
        let cue = &cues[i];
        if cue.start_ms <= pts_ms && pts_ms <= cue.end_ms {
            return Some(cue);
        }
        // Early break: if cue start is far before pts_ms and end_ms is in the past,
        // no earlier cues with realistic durations can match.
        if pts_ms.saturating_sub(cue.start_ms) > 60_000 && cue.end_ms < pts_ms {
            break;
        }
    }

    None
}

/// Finds all active subtitle cues matching `pts_ms`, supporting overlapping cues.
pub fn find_active_cues(cues: &[SubtitleCue], pts_ms: i64) -> Vec<&SubtitleCue> {
    if cues.is_empty() || pts_ms < 0 {
        return Vec::new();
    }

    let upper = cues.partition_point(|cue| cue.start_ms <= pts_ms);
    if upper == 0 {
        return Vec::new();
    }

    let mut matches = Vec::new();
    for i in (0..upper).rev() {
        let cue = &cues[i];
        if cue.start_ms <= pts_ms && pts_ms <= cue.end_ms {
            matches.push(cue);
        }
        if pts_ms.saturating_sub(cue.start_ms) > 60_000 && cue.end_ms < pts_ms {
            break;
        }
    }

    matches.reverse();
    matches
}

/// Finds all active subtitle cues matching `pts_ms` and returns combined multiline text.
pub fn find_active_cue_text(cues: &[SubtitleCue], pts_ms: i64) -> Option<String> {
    let active = find_active_cues(cues, pts_ms);
    if active.is_empty() {
        None
    } else {
        Some(
            active
                .iter()
                .map(|c| c.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

/// Configuration settings for subtitle overlay presentation.
#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleOverlayConfig {
    /// Bottom padding as a fraction of video height (default: 0.06 = 6%).
    pub bottom_padding_ratio: f32,
    /// Font size as a fraction of video height (default: 0.035 = 3.5%).
    pub font_scale_ratio: f32,
    /// Minimum font size in points (default: 14.0 pt).
    pub min_font_size: f32,
    /// Maximum font size in points (default: 36.0 pt).
    pub max_font_size: f32,
    /// Maximum width of subtitle block as a fraction of video width (default: 0.85 = 85%).
    pub max_width_ratio: f32,
    /// Horizontal and vertical padding inside the translucent pill.
    pub pill_padding: Vec2,
    /// Corner radius of the translucent pill.
    pub pill_corner_radius: u8,
    /// Background fill color for the translucent pill.
    pub pill_background_color: Color32,
    /// Primary high-contrast subtitle text color.
    pub text_color: Color32,
    /// Text drop shadow / outline color.
    pub text_shadow_color: Color32,
    /// Border stroke around the translucent pill.
    pub border_stroke: Stroke,
    /// Whether drop shadow rendering is enabled.
    pub enable_shadow: bool,
    /// Whether subtitles may render outside active video rect into letterbox margins.
    pub render_outside_video: bool,
}

impl Default for SubtitleOverlayConfig {
    fn default() -> Self {
        Self {
            bottom_padding_ratio: 0.065,
            font_scale_ratio: 0.035,
            min_font_size: 14.0,
            max_font_size: 36.0,
            max_width_ratio: 0.85,
            pill_padding: vec2(14.0, 6.0),
            pill_corner_radius: 6,
            pill_background_color: Color32::from_rgba_unmultiplied(0, 0, 0, 195),
            text_color: Color32::from_rgb(255, 255, 255),
            text_shadow_color: Color32::from_rgba_unmultiplied(0, 0, 0, 220),
            border_stroke: Stroke::NONE,
            enable_shadow: true,
            render_outside_video: false,
        }
    }
}

/// Subtitle overlay rendering engine anchored strictly within the active video frame.
#[derive(Debug, Clone, Default)]
pub struct SubtitleOverlay {
    config: SubtitleOverlayConfig,
}

impl SubtitleOverlay {
    /// Constructs a new `SubtitleOverlay` with default styling parameters.
    pub fn new() -> Self {
        Self {
            config: SubtitleOverlayConfig::default(),
        }
    }

    /// Constructs a new `SubtitleOverlay` with custom configuration.
    pub fn with_config(config: SubtitleOverlayConfig) -> Self {
        Self { config }
    }

    /// Returns a shared reference to the current configuration.
    pub fn config(&self) -> &SubtitleOverlayConfig {
        &self.config
    }

    /// Returns a mutable reference to the configuration for customization.
    pub fn config_mut(&mut self) -> &mut SubtitleOverlayConfig {
        &mut self.config
    }

    /// Computes the proportional font size in points for a given video frame height.
    #[inline]
    pub fn compute_font_size(&self, video_height: f32) -> f32 {
        (video_height * self.config.font_scale_ratio)
            .clamp(self.config.min_font_size, self.config.max_font_size)
    }

    /// Renders subtitle text anchored inside `video_rect` using an `egui::Ui`.
    pub fn render(&self, ui: &mut Ui, video_rect: Rect, text: &str) -> Option<Rect> {
        self.render_with_painter(ui.painter(), video_rect, text)
    }

    /// Renders subtitle text anchored strictly inside `video_rect` using an `egui::Painter`.
    ///
    /// Invariants enforced:
    /// - Anchored strictly inside active `video_rect` placed near bottom with `bottom_padding_ratio`.
    /// - Pill bounding box is strictly clamped within `video_rect` (never spills into letterbox/pillarbox margins).
    /// - GPU scissor clip rect is applied to prevent any raster leakage.
    /// - Proportional font size clamped between `min_font_size` (14pt) and `max_font_size` (36pt).
    /// - High-contrast text on dark translucent pill background with drop shadow.
    pub fn render_with_painter(
        &self,
        painter: &Painter,
        video_rect: Rect,
        text: &str,
    ) -> Option<Rect> {
        let trimmed = text.trim();
        if trimmed.is_empty() || video_rect.width() <= 0.0 || video_rect.height() <= 0.0 {
            return None;
        }

        // 1. Calculate proportional font size clamped to [min_font_size, max_font_size]
        let font_size = self.compute_font_size(video_rect.height());
        let font_id = FontId::proportional(font_size);

        // 2. Maximum text wrap width inside video_rect (accounting for pill padding)
        let max_wrap_width = (video_rect.width() * self.config.max_width_ratio
            - self.config.pill_padding.x * 2.0)
            .max(40.0);

        // 3. Lay out text into galley to determine exact multiline dimensions
        let galley = painter.layout(
            trimmed.to_string(),
            font_id,
            self.config.text_color,
            max_wrap_width,
        );

        let text_size = galley.size();
        let pill_size = text_size + self.config.pill_padding * 2.0;

        // 4. Calculate bottom-anchored positioning
        let bottom_pad = (video_rect.height() * self.config.bottom_padding_ratio).max(4.0);
        let pill_center_x = video_rect.center().x;
        let pill_bottom_y = video_rect.bottom() - bottom_pad;

        let desired_min_x = pill_center_x - pill_size.x / 2.0;
        let desired_max_x = pill_center_x + pill_size.x / 2.0;
        let desired_min_y = pill_bottom_y - pill_size.y;
        let desired_max_y = pill_bottom_y;

        // 5. Strictly clamp pill bounds inside video_rect
        let (min_x, max_x, min_y, max_y) = if self.config.render_outside_video {
            (desired_min_x, desired_max_x, desired_min_y, desired_max_y)
        } else {
            (
                desired_min_x.max(video_rect.left()),
                desired_max_x.min(video_rect.right()),
                desired_min_y.max(video_rect.top()),
                desired_max_y.min(video_rect.bottom()),
            )
        };

        if min_x >= max_x || min_y >= max_y {
            return None;
        }

        let pill_rect = Rect::from_min_max(pos2(min_x, min_y), pos2(max_x, max_y));

        // 6. Ensure painter is strictly clipped to video_rect
        let clipped_painter = if self.config.render_outside_video {
            painter.clone()
        } else {
            painter.with_clip_rect(video_rect)
        };

        // 7. Draw dark translucent background pill
        let corner_radius = CornerRadius::same(self.config.pill_corner_radius);
        clipped_painter.rect_filled(
            pill_rect,
            corner_radius,
            self.config.pill_background_color,
        );

        if self.config.border_stroke != Stroke::NONE {
            clipped_painter.rect_stroke(
                pill_rect,
                corner_radius,
                self.config.border_stroke,
                StrokeKind::Inside,
            );
        }

        // 8. Text position centered inside pill_rect
        let text_pos = pill_rect.center() - text_size / 2.0;

        // 9. Render drop shadow for high readability over bright video backgrounds
        if self.config.enable_shadow {
            let shadow_offset = vec2(1.0, 1.0);
            clipped_painter.galley(
                text_pos + shadow_offset,
                galley.clone(),
                self.config.text_shadow_color,
            );
        }

        // 10. Render high-contrast main subtitle text
        clipped_painter.galley(text_pos, galley, self.config.text_color);

        Some(pill_rect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subtitle_cue_basic_timing() {
        let cue = SubtitleCue::new(1000, 3000, "Hello world");
        assert_eq!(cue.start_ms, 1000);
        assert_eq!(cue.end_ms, 3000);
        assert_eq!(cue.text, "Hello world");
        assert_eq!(cue.duration_ms(), 2000);

        // Before start
        assert!(!cue.is_active_at(999));
        // At start
        assert!(cue.is_active_at(1000));
        // Inside interval
        assert!(cue.is_active_at(2000));
        // At end
        assert!(cue.is_active_at(3000));
        // After end
        assert!(!cue.is_active_at(3001));
    }

    #[test]
    fn test_compute_effective_pts() {
        // media_position_pts in nanoseconds
        let pts_ns = 2_500_000_000u64; // 2,500 ms
        let delay_ms = 0i64;
        assert_eq!(compute_effective_pts(pts_ns, delay_ms), 2500);

        // Positive delay offset (+250ms)
        assert_eq!(compute_effective_pts(pts_ns, 250), 2750);

        // Negative delay offset (-500ms)
        assert_eq!(compute_effective_pts(pts_ns, -500), 2000);

        // Zero timestamp with negative delay
        assert_eq!(compute_effective_pts(0, -100), -100);
    }

    #[test]
    fn test_binary_search_empty_cues() {
        let cues: Vec<SubtitleCue> = Vec::new();
        assert_eq!(find_active_cue(&cues, 1000), None);
        assert_eq!(find_active_cues(&cues, 1000), Vec::<&SubtitleCue>::new());
        assert_eq!(find_active_cue_text(&cues, 1000), None);
    }

    #[test]
    fn test_binary_search_sequential_cues() {
        let cues = vec![
            SubtitleCue::new(1000, 2000, "First cue"),
            SubtitleCue::new(2500, 4000, "Second cue"),
            SubtitleCue::new(5000, 7000, "Third cue"),
            SubtitleCue::new(8000, 9500, "Fourth cue"),
        ];

        // Before first cue
        assert_eq!(find_active_cue(&cues, 500), None);

        // First cue boundary
        assert_eq!(
            find_active_cue(&cues, 1000).map(|c| c.text.as_str()),
            Some("First cue")
        );
        assert_eq!(
            find_active_cue(&cues, 1500).map(|c| c.text.as_str()),
            Some("First cue")
        );
        assert_eq!(
            find_active_cue(&cues, 2000).map(|c| c.text.as_str()),
            Some("First cue")
        );

        // Gap between first and second cue
        assert_eq!(find_active_cue(&cues, 2001), None);
        assert_eq!(find_active_cue(&cues, 2499), None);

        // Second cue
        assert_eq!(
            find_active_cue(&cues, 3000).map(|c| c.text.as_str()),
            Some("Second cue")
        );

        // Third cue
        assert_eq!(
            find_active_cue(&cues, 6000).map(|c| c.text.as_str()),
            Some("Third cue")
        );

        // Fourth cue boundary
        assert_eq!(
            find_active_cue(&cues, 9500).map(|c| c.text.as_str()),
            Some("Fourth cue")
        );

        // After all cues
        assert_eq!(find_active_cue(&cues, 9501), None);
        assert_eq!(find_active_cue(&cues, 15000), None);
    }

    #[test]
    fn test_binary_search_large_cue_list_efficiency() {
        // Create 2,000 cues simulating a feature-length film (2 hours)
        let mut cues = Vec::with_capacity(2000);
        for i in 0..2000 {
            let start = i * 3500;
            let end = start + 2500;
            cues.push(SubtitleCue::new(start, end, format!("Cue #{}", i)));
        }

        // Test random access points
        for i in (0..2000).step_by(137) {
            let mid_pts = i * 3500 + 1250;
            let found = find_active_cue(&cues, mid_pts);
            assert!(found.is_some());
            assert_eq!(found.unwrap().text, format!("Cue #{}", i));
        }

        // Test in-between gaps
        for i in (0..1999).step_by(143) {
            let gap_pts = i * 3500 + 3000;
            let found = find_active_cue(&cues, gap_pts);
            assert!(found.is_none());
        }
    }

    #[test]
    fn test_overlapping_cues_detection_and_text_join() {
        let cues = vec![
            SubtitleCue::new(1000, 5000, "Speaker 1: Hi"),
            SubtitleCue::new(2000, 4000, "Speaker 2: Hello!"),
            SubtitleCue::new(6000, 8000, "Speaker 1: Bye"),
        ];

        // At 1500: only speaker 1
        assert_eq!(
            find_active_cue_text(&cues, 1500),
            Some("Speaker 1: Hi".to_string())
        );

        // At 3000: both speakers
        assert_eq!(
            find_active_cue_text(&cues, 3000),
            Some("Speaker 1: Hi\nSpeaker 2: Hello!".to_string())
        );

        // At 4500: only speaker 1
        assert_eq!(
            find_active_cue_text(&cues, 4500),
            Some("Speaker 1: Hi".to_string())
        );

        // At 5500: none
        assert_eq!(find_active_cue_text(&cues, 5500), None);

        // At 7000: only speaker 1 Bye
        assert_eq!(
            find_active_cue_text(&cues, 7000),
            Some("Speaker 1: Bye".to_string())
        );
    }

    #[test]
    fn test_subtitle_track_management() {
        let mut track = SubtitleTrack::new(
            1,
            "English (.srt)",
            Some("en".to_string()),
            vec![
                SubtitleCue::new(2000, 3000, "Later cue"),
                SubtitleCue::new(1000, 1500, "Earlier cue"),
            ],
        );

        // Verify sorted ordering
        assert_eq!(track.cues[0].start_ms, 1000);
        assert_eq!(track.cues[1].start_ms, 2000);

        // Add middle cue
        track.add_cue(SubtitleCue::new(1600, 1900, "Middle cue"));
        assert_eq!(track.cues.len(), 3);
        assert_eq!(track.cues[1].text, "Middle cue");

        // Active text lookup
        assert_eq!(track.active_text_at(1200), Some("Earlier cue".to_string()));
        assert_eq!(track.active_text_at(1700), Some("Middle cue".to_string()));
        assert_eq!(track.active_text_at(2500), Some("Later cue".to_string()));
        assert_eq!(track.active_text_at(3500), None);
    }

    #[test]
    fn test_font_size_scaling_and_clamping() {
        let overlay = SubtitleOverlay::new();

        // 360p video: 360 * 0.035 = 12.6 -> clamped to min 14.0
        assert_eq!(overlay.compute_font_size(360.0), 14.0);

        // 480p video: 480 * 0.035 = 16.8
        let fs_480 = overlay.compute_font_size(480.0);
        assert!((fs_480 - 16.8).abs() < 1e-4);

        // 720p video: 720 * 0.035 = 25.2
        let fs_720 = overlay.compute_font_size(720.0);
        assert!((fs_720 - 25.2).abs() < 1e-4);

        // 1080p video: 1080 * 0.035 = 37.8 -> clamped to max 36.0
        assert_eq!(overlay.compute_font_size(1080.0), 36.0);

        // 4K video (2160p): 2160 * 0.035 = 75.6 -> clamped to max 36.0
        assert_eq!(overlay.compute_font_size(2160.0), 36.0);
    }

    #[test]
    fn test_delay_offset_calculation() {
        let base_pts = 10_000_000_000u64; // 10,000 ms

        // Advance subtitles by 200ms (subtitle appears earlier)
        let effective_advanced = compute_effective_pts(base_pts, 200);
        assert_eq!(effective_advanced, 10200);

        // Delay subtitles by 200ms (subtitle appears later)
        let effective_delayed = compute_effective_pts(base_pts, -200);
        assert_eq!(effective_delayed, 9800);
    }

    #[test]
    fn test_subtitle_overlay_rendering_inside_video_rect() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let overlay = SubtitleOverlay::new();

            // 1. Normal 16:9 video rect positioned inside letterboxed screen
            let video_rect = Rect::from_min_size(pos2(100.0, 50.0), vec2(1280.0, 720.0));
            let pill = overlay.render(ui, video_rect, "Synchronized Desktop Player Subtitle");
            assert!(pill.is_some());
            let pill_rect = pill.unwrap();

            // Verification 1: Subtitle must NEVER render outside video_rect
            assert!(video_rect.contains_rect(pill_rect));

            // Verification 2: Anchored near the bottom with 5-8% padding
            let bottom_distance = video_rect.bottom() - pill_rect.bottom();
            let pad_ratio = bottom_distance / video_rect.height();
            assert!((0.05..=0.08).contains(&pad_ratio), "pad_ratio was {}", pad_ratio);

            // Verification 3: Centered horizontally inside video_rect
            assert!((pill_rect.center().x - video_rect.center().x).abs() < 1.0);

            // Verification 4: Empty string returns None (clearing display)
            assert_eq!(overlay.render(ui, video_rect, ""), None);
            assert_eq!(overlay.render(ui, video_rect, "   "), None);

            // Verification 5: Degenerate video_rect returns None
            let zero_rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(0.0, 0.0));
            assert_eq!(overlay.render(ui, zero_rect, "Hello"), None);

            // Verification 6: Multiline text rendering
            let multiline = "Line 1: First speaker speech\nLine 2: Second speaker answer";
            let ml_pill = overlay.render(ui, video_rect, multiline);
            assert!(ml_pill.is_some());
            assert!(video_rect.contains_rect(ml_pill.unwrap()));

            // Verification 7: Ultra-wide subtitle is clamped to max width and stays inside video_rect
            let long_text = "This is a very long subtitle line that is guaranteed to test the text wrapping boundaries of the video overlay component and ensure it does not overflow horizontally outside the bounds of the video viewport rect.";
            let long_pill = overlay.render(ui, video_rect, long_text);
            assert!(long_pill.is_some());
            let long_rect = long_pill.unwrap();
            assert!(video_rect.contains_rect(long_rect));
            assert!(long_rect.width() <= video_rect.width() * overlay.config.max_width_ratio + 1.0);
        });
        output.textures_delta.clear();
    }

    #[test]
    fn test_subtitle_overlay_various_aspect_ratios() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let overlay = SubtitleOverlay::new();

            // Test 4:3 aspect ratio
            let rect_4_3 = Rect::from_min_size(pos2(200.0, 50.0), vec2(960.0, 720.0));
            let pill_4_3 = overlay.render(ui, rect_4_3, "4:3 Classic Aspect").unwrap();
            assert!(rect_4_3.contains_rect(pill_4_3));

            // Test 21:9 ultrawide aspect ratio
            let rect_21_9 = Rect::from_min_size(pos2(50.0, 100.0), vec2(1680.0, 720.0));
            let pill_21_9 = overlay.render(ui, rect_21_9, "21:9 Ultrawide Aspect").unwrap();
            assert!(rect_21_9.contains_rect(pill_21_9));

            // Test 1:1 square aspect ratio
            let rect_1_1 = Rect::from_min_size(pos2(300.0, 50.0), vec2(600.0, 600.0));
            let pill_1_1 = overlay.render(ui, rect_1_1, "1:1 Square Aspect").unwrap();
            assert!(rect_1_1.contains_rect(pill_1_1));

            // Test 9:16 vertical portrait mobile aspect ratio
            let rect_portrait = Rect::from_min_size(pos2(400.0, 50.0), vec2(405.0, 720.0));
            let pill_portrait = overlay.render(ui, rect_portrait, "Vertical Video").unwrap();
            assert!(rect_portrait.contains_rect(pill_portrait));
        });
        output.textures_delta.clear();
    }
}
