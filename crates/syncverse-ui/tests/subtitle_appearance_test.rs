//! Comprehensive unit and integration test suite for subtitle overlay appearance configuration,
//! typography scaling, color customization, shadow toggles, and viewport bounding invariants.
//!
//! Validates:
//! 1. Default configuration invariant parameters (`SubtitleOverlayConfig::default()`).
//! 2. Custom font scaling across the range 0.5x to 2.0x for various video resolutions (240p to 4K).
//! 3. Proportional font size computation and clamping to `[min_font_size, max_font_size]`.
//! 4. Custom text colors, background pill fills, shadow colors, and border strokes.
//! 5. Shadow toggle (`enable_shadow = true` vs `enable_shadow = false`).
//! 6. Active viewport rendering invariants: bounding strictly inside `video_rect`, horizontal centering,
//!    bottom anchor padding, and handling of empty or degenerate inputs.

#![deny(warnings)]

use egui::{pos2, vec2, Color32, Rect, Stroke};
use syncverse_ui::gui::{SubtitleOverlay, SubtitleOverlayConfig};

#[test]
fn test_default_subtitle_overlay_config_invariants() {
    let config = SubtitleOverlayConfig::default();

    // Verify baseline typography scaling defaults
    assert_eq!(config.bottom_padding_ratio, 0.065);
    assert_eq!(config.font_scale_ratio, 0.035);
    assert_eq!(config.min_font_size, 14.0);
    assert_eq!(config.max_font_size, 36.0);
    assert_eq!(config.max_width_ratio, 0.85);

    // Verify pill box shape and padding
    assert_eq!(config.pill_padding, vec2(14.0, 6.0));
    assert_eq!(config.pill_corner_radius, 6);

    // Verify high-contrast default color palette
    assert_eq!(config.text_color, Color32::from_rgb(255, 255, 255));
    assert_eq!(
        config.pill_background_color,
        Color32::from_rgba_unmultiplied(0, 0, 0, 195)
    );
    assert_eq!(
        config.text_shadow_color,
        Color32::from_rgba_unmultiplied(0, 0, 0, 220)
    );
    assert_eq!(config.border_stroke, Stroke::NONE);

    // Verify feature toggles
    assert!(config.enable_shadow);
    assert!(!config.render_outside_video);
}

#[test]
fn test_custom_font_scaling_range_0_5_to_2_0() {
    let base_scale = 0.035f32;
    let multipliers = [0.5f32, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];
    let resolutions: [f32; 7] = [240.0, 360.0, 480.0, 720.0, 1080.0, 1440.0, 2160.0];

    for &mult in &multipliers {
        let config = SubtitleOverlayConfig {
            font_scale_ratio: base_scale * mult,
            min_font_size: 14.0 * mult,
            max_font_size: 36.0 * mult,
            ..Default::default()
        };
        let overlay = SubtitleOverlay::with_config(config.clone());

        let mut prev_size = 0.0f32;
        for &height in &resolutions {
            let size = overlay.compute_font_size(height);

            // 1. Must be strictly clamped between min and max font size
            assert!(
                size >= config.min_font_size,
                "Font size {} below min {} for scale {} at height {}",
                size,
                config.min_font_size,
                mult,
                height
            );
            assert!(
                size <= config.max_font_size,
                "Font size {} above max {} for scale {} at height {}",
                size,
                config.max_font_size,
                mult,
                height
            );

            // 2. Monotonicity with respect to video height
            assert!(
                size >= prev_size,
                "Font size decreased from {} to {} with increasing height {}",
                prev_size,
                size,
                height
            );
            prev_size = size;
        }
    }
}

#[test]
fn test_font_size_monotonic_with_multiplier() {
    let video_height = 720.0f32;
    let multipliers = [0.5f32, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

    let mut prev_size = 0.0f32;
    for &mult in &multipliers {
        let config = SubtitleOverlayConfig {
            font_scale_ratio: 0.035 * mult,
            min_font_size: 10.0 * mult,
            max_font_size: 40.0 * mult,
            ..Default::default()
        };
        let overlay = SubtitleOverlay::with_config(config);
        let size = overlay.compute_font_size(video_height);

        assert!(
            size >= prev_size,
            "Font size {} for multiplier {} must be >= previous size {}",
            size,
            mult,
            prev_size
        );
        prev_size = size;
    }
}

#[test]
fn test_custom_colors_configuration() {
    // 1. Configure vibrant yellow text with semi-transparent black background
    let yellow = Color32::from_rgb(255, 255, 0);
    let translucent_black = Color32::from_rgba_unmultiplied(0, 0, 0, 128);
    let deep_shadow = Color32::from_rgba_unmultiplied(10, 10, 10, 240);
    let white_stroke = Stroke::new(1.5, Color32::from_rgb(255, 255, 255));

    let config = SubtitleOverlayConfig {
        text_color: yellow,
        pill_background_color: translucent_black,
        text_shadow_color: deep_shadow,
        border_stroke: white_stroke,
        ..Default::default()
    };

    let mut overlay = SubtitleOverlay::with_config(config.clone());
    assert_eq!(overlay.config().text_color, yellow);
    assert_eq!(overlay.config().pill_background_color, translucent_black);
    assert_eq!(overlay.config().text_shadow_color, deep_shadow);
    assert_eq!(overlay.config().border_stroke, white_stroke);

    // 2. Test in-place mutation via config_mut()
    let cyan = Color32::from_rgb(0, 255, 255);
    overlay.config_mut().text_color = cyan;
    assert_eq!(overlay.config().text_color, cyan);

    // 3. Test fully transparent background fill
    overlay.config_mut().pill_background_color = Color32::TRANSPARENT;
    assert_eq!(
        overlay.config().pill_background_color,
        Color32::TRANSPARENT
    );
}

#[test]
fn test_shadow_toggle_and_rendering() {
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(Default::default(), |ui| {
        let video_rect = Rect::from_min_size(pos2(50.0, 50.0), vec2(1280.0, 720.0));
        let text = "Shadow toggle rendering test";

        // 1. With shadow enabled (default)
        let cfg_with_shadow = SubtitleOverlayConfig {
            enable_shadow: true,
            ..Default::default()
        };
        let overlay_with_shadow = SubtitleOverlay::with_config(cfg_with_shadow);
        let pill_shadow = overlay_with_shadow.render(ui, video_rect, text);
        assert!(pill_shadow.is_some());
        let rect_shadow = pill_shadow.unwrap();
        assert!(video_rect.contains_rect(rect_shadow));

        // 2. With shadow disabled
        let cfg_no_shadow = SubtitleOverlayConfig {
            enable_shadow: false,
            ..Default::default()
        };
        let overlay_no_shadow = SubtitleOverlay::with_config(cfg_no_shadow);
        let pill_no_shadow = overlay_no_shadow.render(ui, video_rect, text);
        assert!(pill_no_shadow.is_some());
        let rect_no_shadow = pill_no_shadow.unwrap();
        assert!(video_rect.contains_rect(rect_no_shadow));

        // Dimensions of the pill box should be identical whether shadow is drawn or not
        assert!((rect_shadow.width() - rect_no_shadow.width()).abs() < 1.0);
        assert!((rect_shadow.height() - rect_no_shadow.height()).abs() < 1.0);
    });
    output.textures_delta.clear();
}

#[test]
fn test_extreme_scaling_and_aspect_ratios_rendering() {
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(Default::default(), |ui| {
        let subtitle_text = "Proportional typography across varied viewports";

        // 1. Small scaling (0.5x) on classic 4:3 video
        let cfg_small = SubtitleOverlayConfig {
            font_scale_ratio: 0.035 * 0.5,
            min_font_size: 7.0,
            max_font_size: 18.0,
            ..Default::default()
        };
        let overlay_small = SubtitleOverlay::with_config(cfg_small);
        let rect_4_3 = Rect::from_min_size(pos2(100.0, 50.0), vec2(960.0, 720.0));
        let pill_small = overlay_small.render(ui, rect_4_3, subtitle_text);
        assert!(pill_small.is_some());
        let pill_rect_small = pill_small.unwrap();
        assert!(rect_4_3.contains_rect(pill_rect_small));
        assert!((pill_rect_small.center().x - rect_4_3.center().x).abs() < 1.0);

        // 2. Large scaling (2.0x) on ultrawide 21:9 video
        let cfg_large = SubtitleOverlayConfig {
            font_scale_ratio: 0.035 * 2.0,
            min_font_size: 28.0,
            max_font_size: 72.0,
            pill_corner_radius: 12,
            text_color: Color32::from_rgb(255, 220, 100),
            ..Default::default()
        };
        let overlay_large = SubtitleOverlay::with_config(cfg_large);
        let rect_21_9 = Rect::from_min_size(pos2(0.0, 100.0), vec2(1680.0, 720.0));
        let pill_large = overlay_large.render(ui, rect_21_9, subtitle_text);
        assert!(pill_large.is_some());
        let pill_rect_large = pill_large.unwrap();
        assert!(rect_21_9.contains_rect(pill_rect_large));
        assert!((pill_rect_large.center().x - rect_21_9.center().x).abs() < 1.0);

        // 3. Multiline text with custom padding
        let multiline = "Line 1: High contrast dialogue\nLine 2: Secondary speaker translation";
        let cfg_multi = SubtitleOverlayConfig {
            pill_padding: vec2(20.0, 10.0),
            ..Default::default()
        };
        let overlay_multi = SubtitleOverlay::with_config(cfg_multi);
        let pill_multi = overlay_multi.render(ui, rect_4_3, multiline);
        assert!(pill_multi.is_some());
        assert!(rect_4_3.contains_rect(pill_multi.unwrap()));
    });
    output.textures_delta.clear();
}

#[test]
fn test_empty_and_whitespace_subtitles_render_none() {
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(Default::default(), |ui| {
        let overlay = SubtitleOverlay::new();
        let video_rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 720.0));

        // Empty string
        assert_eq!(overlay.render(ui, video_rect, ""), None);

        // Whitespace only
        assert_eq!(overlay.render(ui, video_rect, "   "), None);
        assert_eq!(overlay.render(ui, video_rect, "\t\n  \r\n"), None);

        // Zero-sized viewport rect
        let zero_rect = Rect::from_min_size(pos2(100.0, 100.0), vec2(0.0, 0.0));
        assert_eq!(overlay.render(ui, zero_rect, "Active text"), None);

        // Negative size viewport rect
        let neg_rect = Rect::from_min_size(pos2(100.0, 100.0), vec2(-50.0, 100.0));
        assert_eq!(overlay.render(ui, neg_rect, "Active text"), None);
    });
    output.textures_delta.clear();
}
