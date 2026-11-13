//! GPU-accelerated video rendering viewport and aspect ratio preservation.

use std::time::{Duration, Instant};
use egui::{Color32, Context, Rect, TextureHandle, TextureOptions, Ui, Vec2};
use syncverse_clock::{MasterClock, PresentationAction, SpscRingBuffer};
use syncverse_codecs::VideoFrameRGBA;
use crate::gui::aspect_ratio::{AspectRatioOverride, IntoDimension, ZoomMode};
use crate::gui::video_effects::VideoColorAdjustments;

/// Geometric display mode for the video viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewportDisplayMode {
    /// Non-positive dimensions (empty/uninitialized).
    Empty,
    /// Exact match with viewport aspect ratio.
    Exact,
    /// Viewport taller than video: black bars top and bottom.
    Letterbox,
    /// Viewport wider than video: black bars left and right.
    Pillarbox,
}

/// Computed geometry for rendering the video frame inside the available viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportGeometry {
    /// Rendered video width in points/pixels.
    pub render_width: f32,
    /// Rendered video height in points/pixels.
    pub render_height: f32,
    /// Horizontal offset relative to available rect.
    pub offset_x: f32,
    /// Vertical offset relative to available rect.
    pub offset_y: f32,
    /// Display mode.
    pub mode: ViewportDisplayMode,
}

impl ViewportGeometry {
    /// Computes aspect-ratio preserving display rectangle centered in available space.
    pub fn compute(
        available_w: f32,
        available_h: f32,
        video_w: f32,
        video_h: f32,
    ) -> Self {
        Self::compute_with_aspect(
            available_w,
            available_h,
            video_w,
            video_h,
            AspectRatioOverride::Original,
        )
    }

    /// Computes display rectangle centered in available space using concrete `f32` dimensions and an aspect override.
    pub fn compute_with_aspect_f32(
        available_w: f32,
        available_h: f32,
        video_w: f32,
        video_h: f32,
        aspect_override: AspectRatioOverride,
    ) -> Self {
        Self::compute_with_aspect(available_w, available_h, video_w, video_h, aspect_override)
    }

    /// Computes display rectangle centered in available space using concrete `u32` dimensions and an aspect override.
    pub fn compute_with_aspect_u32(
        available_w: f32,
        available_h: f32,
        video_w: u32,
        video_h: u32,
        aspect_override: AspectRatioOverride,
    ) -> Self {
        Self::compute_with_aspect(available_w, available_h, video_w, video_h, aspect_override)
    }

    /// Computes display rectangle centered in available space taking into account an aspect ratio override.
    pub fn compute_with_aspect<W: IntoDimension, H: IntoDimension>(
        available_w: f32,
        available_h: f32,
        video_w: W,
        video_h: H,
        aspect_override: AspectRatioOverride,
    ) -> Self {
        let vw = video_w.into_dimension();
        let vh = video_h.into_dimension();

        if available_w <= 0.0 || available_h <= 0.0 || vw <= 0.0 || vh <= 0.0 {
            return Self {
                render_width: 0.0,
                render_height: 0.0,
                offset_x: 0.0,
                offset_y: 0.0,
                mode: ViewportDisplayMode::Empty,
            };
        }

        if aspect_override == AspectRatioOverride::FillStretch {
            return Self {
                render_width: available_w,
                render_height: available_h,
                offset_x: 0.0,
                offset_y: 0.0,
                mode: ViewportDisplayMode::Exact,
            };
        }

        let effective_aspect = match aspect_override {
            AspectRatioOverride::Original => vw / vh,
            AspectRatioOverride::Ratio16_9 => 16.0 / 9.0,
            AspectRatioOverride::Ratio4_3 => 4.0 / 3.0,
            AspectRatioOverride::Ratio16_10 => 16.0 / 10.0,
            AspectRatioOverride::Ratio21_9 => 21.0 / 9.0,
            AspectRatioOverride::Ratio2_35_1 => 2.35,
            AspectRatioOverride::Ratio1_1 => 1.0,
            AspectRatioOverride::FillStretch => unreachable!(),
        };

        let aspect_avail = available_w / available_h;
        let diff = aspect_avail - effective_aspect;

        if diff > 1e-5 {
            // Viewport wider than target aspect ratio -> Pillarboxing
            let render_w = available_h * effective_aspect;
            let render_h = available_h;
            let offset_x = (available_w - render_w) / 2.0;
            let offset_y = 0.0;
            Self {
                render_width: render_w,
                render_height: render_h,
                offset_x,
                offset_y,
                mode: ViewportDisplayMode::Pillarbox,
            }
        } else if diff < -1e-5 {
            // Viewport taller than target aspect ratio -> Letterboxing
            let render_w = available_w;
            let render_h = available_w / effective_aspect;
            let offset_x = 0.0;
            let offset_y = (available_h - render_h) / 2.0;
            Self {
                render_width: render_w,
                render_height: render_h,
                offset_x,
                offset_y,
                mode: ViewportDisplayMode::Letterbox,
            }
        } else {
            // Exact aspect ratio
            Self {
                render_width: available_w,
                render_height: available_h,
                offset_x: 0.0,
                offset_y: 0.0,
                mode: ViewportDisplayMode::Exact,
            }
        }
    }

    /// Computes display rectangle taking into account both an aspect ratio override and a zoom mode.
    pub fn compute_with_aspect_and_zoom<W: IntoDimension, H: IntoDimension>(
        available_w: f32,
        available_h: f32,
        video_w: W,
        video_h: H,
        aspect_override: AspectRatioOverride,
        zoom_mode: ZoomMode,
    ) -> Self {
        let vw = video_w.into_dimension();
        let vh = video_h.into_dimension();

        if available_w <= 0.0 || available_h <= 0.0 || vw <= 0.0 || vh <= 0.0 {
            return Self {
                render_width: 0.0,
                render_height: 0.0,
                offset_x: 0.0,
                offset_y: 0.0,
                mode: ViewportDisplayMode::Empty,
            };
        }

        if let Some(scale) = zoom_mode.scale() {
            let effective_aspect = match aspect_override {
                AspectRatioOverride::Original => vw / vh,
                AspectRatioOverride::Ratio16_9 => 16.0 / 9.0,
                AspectRatioOverride::Ratio4_3 => 4.0 / 3.0,
                AspectRatioOverride::Ratio16_10 => 16.0 / 10.0,
                AspectRatioOverride::Ratio21_9 => 21.0 / 9.0,
                AspectRatioOverride::Ratio2_35_1 => 2.35,
                AspectRatioOverride::Ratio1_1 => 1.0,
                AspectRatioOverride::FillStretch => vw / vh,
            };
            let render_h = vh * scale;
            let render_w = render_h * effective_aspect;
            let offset_x = (available_w - render_w) / 2.0;
            let offset_y = (available_h - render_h) / 2.0;
            return Self {
                render_width: render_w,
                render_height: render_h,
                offset_x,
                offset_y,
                mode: ViewportDisplayMode::Exact,
            };
        }

        Self::compute_with_aspect(available_w, available_h, video_w, video_h, aspect_override)
    }
}

/// Zero-allocation buffer managing row-stride unpacking for VideoFrameRGBA.
#[derive(Debug, Default)]
pub struct VideoTextureBuffer {
    packed_rgba: Vec<u8>,
}

impl VideoTextureBuffer {
    /// Creates a new `VideoTextureBuffer`.
    pub fn new() -> Self {
        Self {
            packed_rgba: Vec::new(),
        }
    }

    /// Returns a packed RGBA byte slice, unpacking row padding if stride exceeds `width * 4`.
    pub fn prepare<'a>(&'a mut self, frame: &'a VideoFrameRGBA) -> &'a [u8] {
        self.prepare_with_adjustments(frame, &VideoColorAdjustments::default())
    }

    /// Returns a packed RGBA byte slice applying real-time color adjustments and unpacking row padding.
    pub fn prepare_with_adjustments<'a>(
        &'a mut self,
        frame: &'a VideoFrameRGBA,
        adjustments: &VideoColorAdjustments,
    ) -> &'a [u8] {
        let w = frame.width as usize;
        let h = frame.height as usize;
        let stride = frame.stride;
        let total_packed = w * h * 4;

        if adjustments.is_identity() {
            if stride == w * 4 {
                // Fast-path: zero row padding, borrow directly
                &frame.data[..total_packed]
            } else {
                // Unpack row padding into scratch buffer
                self.packed_rgba.clear();
                self.packed_rgba.reserve(total_packed);
                let src = &frame.data;
                for y in 0..h {
                    let start = y * stride;
                    let end = start + w * 4;
                    self.packed_rgba.extend_from_slice(&src[start..end]);
                }
                &self.packed_rgba
            }
        } else {
            adjustments.apply_to_rgba(&frame.data, &mut self.packed_rgba, w, h, stride);
            &self.packed_rgba
        }
    }
}

/// Actions emitted by the video viewport widget to the parent window application.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ViewportAction {
    /// User double-clicked: toggle fullscreen.
    pub toggle_fullscreen: bool,
    /// User clicked: toggle play/pause.
    pub toggle_play_pause: bool,
    /// User clicked empty viewport: open media file picker.
    pub open_file: bool,
    /// User middle-clicked: toggle audio mute.
    pub toggle_mute: bool,
    /// Vertical mouse scroll delta over viewport.
    pub scroll_delta: f32,
}

/// GPU-accelerated video rendering viewport managing texture allocation and presentation.
pub struct VideoViewport {
    texture: Option<TextureHandle>,
    texture_buffer: VideoTextureBuffer,
    current_pts_ms: Option<u64>,
    last_uploaded_generation: Option<u64>,
    video_width: u32,
    video_height: u32,
    pending_frame: Option<VideoFrameRGBA>,
    last_frame: Option<VideoFrameRGBA>,
    aspect_override: AspectRatioOverride,
    zoom_mode: ZoomMode,
    pan_offset: Vec2,
    last_video_rect: Option<Rect>,
    pending_click: Option<(Instant, bool)>,
    last_double_click: Option<Instant>,
    video_adjustments: VideoColorAdjustments,
}

impl Default for VideoViewport {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoViewport {
    /// Constructs a new `VideoViewport`.
    pub fn new() -> Self {
        Self {
            texture: None,
            texture_buffer: VideoTextureBuffer::new(),
            current_pts_ms: None,
            last_uploaded_generation: None,
            video_width: 0,
            video_height: 0,
            pending_frame: None,
            last_frame: None,
            aspect_override: AspectRatioOverride::Original,
            zoom_mode: ZoomMode::Fit,
            pan_offset: Vec2::ZERO,
            last_video_rect: None,
            pending_click: None,
            last_double_click: None,
            video_adjustments: VideoColorAdjustments::default(),
        }
    }

    /// Consumes incoming video frames from SPSC ring buffer according to MasterClock pacing.
    ///
    /// Filters frames against `clock.seek_generation()` so stale pre-seek frames are discarded
    /// immediately without flashing, never mutates `MasterClock` from the UI thread, and avoids
    /// busy repaint loops when paused or waiting for frames.
    pub fn update_playback(
        &mut self,
        ctx: &Context,
        video_ring: &SpscRingBuffer<VideoFrameRGBA, 32>,
        clock: &MasterClock,
        now: Instant,
    ) {
        let current_gen = clock.seek_generation();

        if self
            .pending_frame
            .as_ref()
            .is_some_and(|f| f.generation != current_gen)
        {
            self.pending_frame = None;
        }

        if !clock.is_playing() {
            if self.last_uploaded_generation != Some(current_gen) || self.texture.is_none() {
                let next_frame = if let Some(pending) = self.pending_frame.take() {
                    Some(pending)
                } else {
                    let mut found = None;
                    while let Some(candidate) = video_ring.try_pop() {
                        if candidate.generation == current_gen {
                            found = Some(candidate);
                            break;
                        }
                    }
                    found
                };
                if let Some(frame) = next_frame {
                    self.upload_frame(ctx, frame);
                    ctx.request_repaint();
                }
            }
            return;
        }

        loop {
            if self.pending_frame.is_none() {
                self.pending_frame = video_ring.try_pop();
            }

            let frame = match self.pending_frame.take() {
                Some(f) => f,
                None => {
                    let delay = if self.video_width > 0 {
                        Duration::from_millis(16)
                    } else {
                        Duration::from_millis(100)
                    };
                    ctx.request_repaint_after(delay);
                    break;
                }
            };

            if frame.generation != current_gen {
                continue;
            }

            // First frame presentation: upload and present immediately without racing MasterClock.
            if self.texture.is_none() {
                self.upload_frame(ctx, frame);
                ctx.request_repaint();
                break;
            }

            let action = clock.evaluate_presentation_timing(frame.pts_ms, now);
            match action {
                PresentationAction::Wait(duration) => {
                    if duration > Duration::from_millis(1500) {
                        // Frame timestamp is abnormally far in the future (e.g. stale frame from prior seek or discontinuity).
                        // Discard it so the viewport does not freeze.
                        continue;
                    }
                    self.pending_frame = Some(frame);
                    ctx.request_repaint_after(
                        duration.clamp(Duration::from_millis(1), Duration::from_millis(40)),
                    );
                    break;
                }
                PresentationAction::PresentNow | PresentationAction::PresentNowLate => {
                    self.upload_frame(ctx, frame);
                    ctx.request_repaint();
                    break;
                }
                PresentationAction::DropLateFrame => {
                    continue;
                }
            }
        }
    }

    /// Discards any pending video frame waiting for presentation.
    pub fn clear_pending(&mut self) {
        self.pending_frame = None;
        self.current_pts_ms = None;
        self.last_uploaded_generation = None;
    }

    /// Resets the viewport state and texture handle for a newly opened media stream.
    pub fn reset(&mut self) {
        self.texture = None;
        self.pending_frame = None;
        self.last_frame = None;
        self.current_pts_ms = None;
        self.last_uploaded_generation = None;
        self.video_width = 0;
        self.video_height = 0;
        self.zoom_mode = ZoomMode::Fit;
        self.pan_offset = Vec2::ZERO;
        self.last_video_rect = None;
        self.pending_click = None;
        self.last_double_click = None;
    }

    /// Uploads an owned frame's RGBA data to the `egui::TextureHandle` with zero heap clones.
    pub fn upload_frame(&mut self, ctx: &Context, frame: VideoFrameRGBA) {
        if self.current_pts_ms == Some(frame.pts_ms)
            && self.last_uploaded_generation == Some(frame.generation)
            && self.texture.is_some()
        {
            return;
        }

        let w = frame.width as usize;
        let h = frame.height as usize;
        let bytes = self
            .texture_buffer
            .prepare_with_adjustments(&frame, &self.video_adjustments);
        let color_image = egui::ColorImage::from_rgba_unmultiplied([w, h], bytes);

        if let Some(ref mut handle) = self.texture {
            if handle.size() == [w, h] {
                handle.set(color_image, TextureOptions::LINEAR);
            } else {
                *handle = ctx.load_texture("video_surface", color_image, TextureOptions::LINEAR);
            }
        } else {
            self.texture = Some(ctx.load_texture("video_surface", color_image, TextureOptions::LINEAR));
        }

        self.current_pts_ms = Some(frame.pts_ms);
        self.last_uploaded_generation = Some(frame.generation);
        self.video_width = frame.width;
        self.video_height = frame.height;
        self.last_frame = Some(frame);
    }

    /// Uploads frame RGBA data to the `egui::TextureHandle`.
    pub fn upload_texture(&mut self, ctx: &Context, frame: &VideoFrameRGBA) {
        if self.current_pts_ms == Some(frame.pts_ms)
            && self.last_uploaded_generation == Some(frame.generation)
            && self.texture.is_some()
        {
            return;
        }
        self.upload_frame(ctx, frame.clone());
    }

    /// Sets video color adjustments and immediately refreshes the current frame texture.
    pub fn set_video_adjustments(&mut self, ctx: &Context, adjustments: VideoColorAdjustments) {
        self.video_adjustments = adjustments;
        if let Some(frame) = self.last_frame.take() {
            self.current_pts_ms = None;
            self.last_uploaded_generation = None;
            self.upload_frame(ctx, frame);
            ctx.request_repaint();
        }
    }

    /// Returns the currently active video color adjustments.
    pub fn video_adjustments(&self) -> VideoColorAdjustments {
        self.video_adjustments
    }

    /// Returns a copy of the most recently presented video frame for lossless screenshot saving.
    pub fn capture_frame(&self) -> Option<VideoFrameRGBA> {
        self.last_frame.clone()
    }

    /// Returns the currently active video dimensions `(width, height)`.
    pub fn video_dimensions(&self) -> (u32, u32) {
        (self.video_width, self.video_height)
    }

    /// Returns the currently active aspect ratio override.
    pub fn aspect_override(&self) -> AspectRatioOverride {
        self.aspect_override
    }

    /// Sets the active aspect ratio override.
    pub fn set_aspect_override(&mut self, aspect_override: AspectRatioOverride) {
        self.aspect_override = aspect_override;
    }

    /// Cycles sequentially to the next aspect ratio preset and returns the new preset.
    pub fn cycle_aspect_override(&mut self) -> AspectRatioOverride {
        self.aspect_override = self.aspect_override.cycle_next();
        self.aspect_override
    }

    /// Returns the currently active zoom mode.
    pub fn zoom_mode(&self) -> ZoomMode {
        self.zoom_mode
    }

    /// Sets the active zoom mode.
    pub fn set_zoom_mode(&mut self, zoom_mode: ZoomMode) {
        self.zoom_mode = zoom_mode;
        if zoom_mode == ZoomMode::Fit {
            self.pan_offset = Vec2::ZERO;
        }
    }

    /// Cycles sequentially to the next zoom mode preset and returns the new mode.
    pub fn cycle_zoom_mode(&mut self) -> ZoomMode {
        self.zoom_mode = self.zoom_mode.cycle_next();
        if self.zoom_mode == ZoomMode::Fit {
            self.pan_offset = Vec2::ZERO;
        }
        self.zoom_mode
    }

    /// Resets video frame pan offset to zero.
    pub fn reset_pan(&mut self) {
        self.pan_offset = Vec2::ZERO;
    }

    /// Sets active video dimensions `(width, height)`.
    pub fn set_video_dimensions(&mut self, width: u32, height: u32) {
        self.video_width = width;
        self.video_height = height;
    }

    /// Sets current video frame pan offset.
    pub fn set_pan_offset(&mut self, offset: Vec2) {
        self.pan_offset = offset;
    }

    /// Returns current video frame pan offset.
    pub fn pan_offset(&self) -> Vec2 {
        self.pan_offset
    }

    /// Returns the active video frame rectangle computed during the last frame presentation, if any.
    pub fn last_video_rect(&self) -> Option<Rect> {
        self.last_video_rect
    }

    /// Computes the active video rectangle for a given available bounding rectangle and current video dimensions.
    pub fn compute_video_rect(&self, available_rect: Rect) -> Option<Rect> {
        if self.video_width == 0 || self.video_height == 0 {
            return None;
        }
        let geom = ViewportGeometry::compute_with_aspect_and_zoom(
            available_rect.width(),
            available_rect.height(),
            self.video_width,
            self.video_height,
            self.aspect_override,
            self.zoom_mode,
        );
        if geom.mode == ViewportDisplayMode::Empty {
            None
        } else {
            let max_pan_x = ((geom.render_width - available_rect.width()) / 2.0).max(0.0);
            let max_pan_y = ((geom.render_height - available_rect.height()) / 2.0).max(0.0);
            let pan = if self.zoom_mode == ZoomMode::Fit {
                Vec2::ZERO
            } else {
                egui::vec2(
                    self.pan_offset.x.clamp(-max_pan_x, max_pan_x),
                    self.pan_offset.y.clamp(-max_pan_y, max_pan_y),
                )
            };
            Some(Rect::from_min_size(
                available_rect.min + egui::vec2(geom.offset_x + pan.x, geom.offset_y + pan.y),
                Vec2::new(geom.render_width, geom.render_height),
            ))
        }
    }

    /// Renders the video viewport widget into egui with automatic pillarbox/letterbox fill.
    pub fn show(&mut self, ui: &mut Ui) -> ViewportAction {
        self.show_with_context_menu(ui, |_| {})
    }

    /// Renders the video viewport widget into egui with an optional right-click context menu.
    pub fn show_with_context_menu<F: FnOnce(&mut Ui)>(
        &mut self,
        ui: &mut Ui,
        context_menu: F,
    ) -> ViewportAction {
        let mut action = ViewportAction::default();
        let available_rect = ui.available_rect_before_wrap();
        let available_w = available_rect.width();
        let available_h = available_rect.height();

        // 1. Allocate full interactive space (drag only active when zoomed so clicks in Fit mode aren't swallowed)
        let sense = if self.zoom_mode == ZoomMode::Fit {
            egui::Sense::click()
        } else {
            egui::Sense::click_and_drag()
        };
        let (rect, response) = ui.allocate_exact_size(available_rect.size(), sense);

        // Context menu on right click
        response.context_menu(context_menu);

        // 2. Black background fill (letterbox/pillarbox margins)
        ui.painter().rect_filled(rect, 0.0, Color32::BLACK);

        // 3. Aspect ratio and zoom calculation
        let geom = ViewportGeometry::compute_with_aspect_and_zoom(
            available_w,
            available_h,
            self.video_width,
            self.video_height,
            self.aspect_override,
            self.zoom_mode,
        );

        let max_pan_x = ((geom.render_width - available_w) / 2.0).max(0.0);
        let max_pan_y = ((geom.render_height - available_h) / 2.0).max(0.0);

        if self.zoom_mode == ZoomMode::Fit {
            self.pan_offset = Vec2::ZERO;
        } else {
            if response.dragged() {
                self.pan_offset += response.drag_delta();
                self.pending_click = None;
            }
            self.pan_offset.x = self.pan_offset.x.clamp(-max_pan_x, max_pan_x);
            self.pan_offset.y = self.pan_offset.y.clamp(-max_pan_y, max_pan_y);
        }

        let video_rect = if geom.mode != ViewportDisplayMode::Empty {
            let pan = if self.zoom_mode == ZoomMode::Fit { Vec2::ZERO } else { self.pan_offset };
            let video_min = rect.min + egui::vec2(geom.offset_x + pan.x, geom.offset_y + pan.y);
            Some(Rect::from_min_size(
                video_min,
                Vec2::new(geom.render_width, geom.render_height),
            ))
        } else {
            None
        };
        self.last_video_rect = video_rect;

        // 4. Render video frame texture if available (clipped to viewport rect to prevent bleed)
        if let Some(ref texture) = self.texture {
            if let Some(v_rect) = video_rect {
                let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter().with_clip_rect(rect).image(texture.id(), v_rect, uv, Color32::WHITE);
            }
        } else {
            // Sleek centered placeholder when no media is playing
            let center = rect.center();
            ui.painter().text(
                center,
                egui::Align2::CENTER_CENTER,
                "Syncverse\nDrop video file here or click to open",
                egui::FontId::proportional(18.0),
                Color32::from_rgb(100, 116, 139),
            );
        }

        // 5. Handle mouse click and scroll events
        if response.hovered() {
            action.scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
            if response.clicked_by(egui::PointerButton::Middle) {
                action.toggle_mute = true;
            }
        }

        let is_empty = self.texture.is_none();
        if response.double_clicked() {
            // Double-click toggles fullscreen: cancel any pending single-click
            self.pending_click = None;
            self.last_double_click = Some(Instant::now());
            action.toggle_fullscreen = true;
        } else if response.clicked() {
            let is_trailing_click = self
                .last_double_click
                .map(|t| t.elapsed() < Duration::from_millis(300))
                .unwrap_or(false);
            if !is_trailing_click {
                // Single-click: stage for delayed dispatch to avoid spurious toggle on double-click
                self.pending_click = Some((Instant::now(), is_empty));
                ui.ctx().request_repaint_after(Duration::from_millis(260));
            }
        }

        if let Some((click_time, empty)) = self.pending_click {
            if Instant::now().duration_since(click_time) >= Duration::from_millis(250) {
                self.pending_click = None;
                if empty {
                    action.open_file = true;
                } else {
                    action.toggle_play_pause = true;
                }
            }
        }

        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewport_geometry_exact() {
        let geom = ViewportGeometry::compute(1280.0, 720.0, 1920.0, 1080.0);
        assert_eq!(geom.mode, ViewportDisplayMode::Exact);
        assert_eq!(geom.render_width, 1280.0);
        assert_eq!(geom.render_height, 720.0);
        assert_eq!(geom.offset_x, 0.0);
        assert_eq!(geom.offset_y, 0.0);
    }

    #[test]
    fn test_viewport_geometry_letterbox() {
        let geom = ViewportGeometry::compute(1280.0, 960.0, 1920.0, 1080.0);
        assert_eq!(geom.mode, ViewportDisplayMode::Letterbox);
        assert_eq!(geom.render_width, 1280.0);
        assert_eq!(geom.render_height, 720.0);
        assert_eq!(geom.offset_x, 0.0);
        assert_eq!(geom.offset_y, 120.0);
    }

    #[test]
    fn test_viewport_geometry_pillarbox() {
        let geom = ViewportGeometry::compute(1920.0, 1080.0, 1440.0, 1080.0);
        assert_eq!(geom.mode, ViewportDisplayMode::Pillarbox);
        assert_eq!(geom.render_width, 1440.0);
        assert_eq!(geom.render_height, 1080.0);
        assert_eq!(geom.offset_x, 240.0);
        assert_eq!(geom.offset_y, 0.0);
    }

    #[test]
    fn test_viewport_geometry_empty() {
        let geom = ViewportGeometry::compute(0.0, 720.0, 1920.0, 1080.0);
        assert_eq!(geom.mode, ViewportDisplayMode::Empty);
        assert_eq!(geom.render_width, 0.0);
        assert_eq!(geom.render_height, 0.0);
    }

    #[test]
    fn test_compute_with_aspect_all_presets() {
        // 1. Original (16:9 in 1280x960 4:3 viewport -> Letterbox)
        let geom_orig = ViewportGeometry::compute_with_aspect(
            1280.0,
            960.0,
            1920u32,
            1080u32,
            AspectRatioOverride::Original,
        );
        assert_eq!(geom_orig.mode, ViewportDisplayMode::Letterbox);
        assert_eq!(geom_orig.render_width, 1280.0);
        assert_eq!(geom_orig.render_height, 720.0);
        assert_eq!(geom_orig.offset_x, 0.0);
        assert_eq!(geom_orig.offset_y, 120.0);

        // 2. Force 16:9 on 4:3 natural video (1440x1080) in 1920x1080 viewport -> Exact 16:9
        let geom_16_9 = ViewportGeometry::compute_with_aspect(
            1920.0,
            1080.0,
            1440u32,
            1080u32,
            AspectRatioOverride::Ratio16_9,
        );
        assert_eq!(geom_16_9.mode, ViewportDisplayMode::Exact);
        assert_eq!(geom_16_9.render_width, 1920.0);
        assert_eq!(geom_16_9.render_height, 1080.0);
        assert_eq!(geom_16_9.offset_x, 0.0);
        assert_eq!(geom_16_9.offset_y, 0.0);

        // 3. Force 4:3 on 16:9 natural video (1920x1080) in 1920x1080 viewport -> Pillarbox
        let geom_4_3 = ViewportGeometry::compute_with_aspect(
            1920.0,
            1080.0,
            1920u32,
            1080u32,
            AspectRatioOverride::Ratio4_3,
        );
        assert_eq!(geom_4_3.mode, ViewportDisplayMode::Pillarbox);
        assert_eq!(geom_4_3.render_width, 1440.0);
        assert_eq!(geom_4_3.render_height, 1080.0);
        assert_eq!(geom_4_3.offset_x, 240.0);
        assert_eq!(geom_4_3.offset_y, 0.0);

        // 4. Force 21:9 on 16:9 natural video (1920x1080) in 1920x1080 viewport -> Letterbox
        let geom_21_9 = ViewportGeometry::compute_with_aspect(
            1920.0,
            1080.0,
            1920u32,
            1080u32,
            AspectRatioOverride::Ratio21_9,
        );
        assert_eq!(geom_21_9.mode, ViewportDisplayMode::Letterbox);
        assert_eq!(geom_21_9.render_width, 1920.0);
        let expected_h = 1920.0 / (21.0 / 9.0);
        assert!((geom_21_9.render_height - expected_h).abs() < 1e-4);
        assert_eq!(geom_21_9.offset_x, 0.0);
        let expected_oy = (1080.0 - expected_h) / 2.0;
        assert!((geom_21_9.offset_y - expected_oy).abs() < 1e-4);

        // 5. Force 1:1 square on 16:9 video in 1920x1080 viewport -> Pillarbox
        let geom_1_1 = ViewportGeometry::compute_with_aspect(
            1920.0,
            1080.0,
            1920u32,
            1080u32,
            AspectRatioOverride::Ratio1_1,
        );
        assert_eq!(geom_1_1.mode, ViewportDisplayMode::Pillarbox);
        assert_eq!(geom_1_1.render_width, 1080.0);
        assert_eq!(geom_1_1.render_height, 1080.0);
        assert_eq!(geom_1_1.offset_x, 420.0);
        assert_eq!(geom_1_1.offset_y, 0.0);

        // 6. FillStretch mode -> Exact full viewport fill without black bars
        let geom_stretch = ViewportGeometry::compute_with_aspect(
            1920.0,
            1200.0,
            1920u32,
            1080u32,
            AspectRatioOverride::FillStretch,
        );
        assert_eq!(geom_stretch.mode, ViewportDisplayMode::Exact);
        assert_eq!(geom_stretch.render_width, 1920.0);
        assert_eq!(geom_stretch.render_height, 1200.0);
        assert_eq!(geom_stretch.offset_x, 0.0);
        assert_eq!(geom_stretch.offset_y, 0.0);
    }

    #[test]
    fn test_compute_with_aspect_concrete_helpers() {
        let geom_f32 = ViewportGeometry::compute_with_aspect_f32(
            1920.0,
            1080.0,
            1440.0,
            1080.0,
            AspectRatioOverride::Ratio4_3,
        );
        let geom_u32 = ViewportGeometry::compute_with_aspect_u32(
            1920.0,
            1080.0,
            1440,
            1080,
            AspectRatioOverride::Ratio4_3,
        );
        assert_eq!(geom_f32, geom_u32);
        assert_eq!(geom_f32.mode, ViewportDisplayMode::Pillarbox);
        assert_eq!(geom_f32.render_width, 1440.0);
    }

    #[test]
    fn test_video_viewport_aspect_ratio_state_and_cycling() {
        let mut viewport = VideoViewport::new();
        assert_eq!(viewport.aspect_override(), AspectRatioOverride::Original);

        viewport.set_aspect_override(AspectRatioOverride::Ratio16_9);
        assert_eq!(viewport.aspect_override(), AspectRatioOverride::Ratio16_9);

        let cycled = viewport.cycle_aspect_override();
        assert_eq!(cycled, AspectRatioOverride::Ratio4_3);
        assert_eq!(viewport.aspect_override(), AspectRatioOverride::Ratio4_3);
    }

    #[test]
    fn test_video_viewport_reset_clears_all_pending_and_clicks() {
        let mut viewport = VideoViewport::new();
        viewport.pending_click = Some((Instant::now(), true));
        viewport.last_double_click = Some(Instant::now());
        viewport.video_width = 1920;
        viewport.video_height = 1080;

        viewport.reset();
        assert!(viewport.pending_click.is_none());
        assert!(viewport.last_double_click.is_none());
        assert_eq!(viewport.video_width, 0);
        assert_eq!(viewport.video_height, 0);
    }

    #[test]
    fn test_viewport_action_defaults() {
        let action = ViewportAction::default();
        assert!(!action.toggle_fullscreen);
        assert!(!action.toggle_play_pause);
        assert!(!action.open_file);
        assert_eq!(action.scroll_delta, 0.0);
    }

    #[test]
    fn test_video_viewport_zoom_mode_and_geometry() {
        let mut viewport = VideoViewport::new();
        assert_eq!(viewport.zoom_mode(), ZoomMode::Fit);
        viewport.set_zoom_mode(ZoomMode::Quarter);
        assert_eq!(viewport.zoom_mode(), ZoomMode::Quarter);

        let cycled = viewport.cycle_zoom_mode();
        assert_eq!(cycled, ZoomMode::Half);
        assert_eq!(viewport.zoom_mode(), ZoomMode::Half);

        // Test zoom geometry calculation at 50% scale
        let geom = ViewportGeometry::compute_with_aspect_and_zoom(
            1920.0,
            1080.0,
            1920u32,
            1080u32,
            AspectRatioOverride::Original,
            ZoomMode::Half,
        );
        assert_eq!(geom.render_width, 960.0);
        assert_eq!(geom.render_height, 540.0);
        assert_eq!(geom.offset_x, 480.0);
        assert_eq!(geom.offset_y, 270.0);
    }

    #[test]
    fn test_video_viewport_pan_offset_and_clamping() {
        let mut viewport = VideoViewport::new();
        viewport.video_width = 1920;
        viewport.video_height = 1080;
        viewport.set_zoom_mode(ZoomMode::Double); // 200% zoom -> 3840 x 2160 in 1920x1080 viewport
        assert_eq!(viewport.pan_offset(), Vec2::ZERO);

        let available = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(1920.0, 1080.0));
        let base_rect = viewport.compute_video_rect(available).expect("must compute rect");
        // Center offset: (1920 - 3840)/2 = -960, (1080 - 2160)/2 = -540
        assert_eq!(base_rect.min, egui::pos2(-960.0, -540.0));

        // When pan offset is within bounds (e.g. +200, +100)
        viewport.pan_offset = Vec2::new(200.0, 100.0);
        let panned_rect = viewport.compute_video_rect(available).expect("must compute rect");
        assert_eq!(panned_rect.min, egui::pos2(-760.0, -440.0));

        // When pan offset exceeds maximum bounds (e.g. +5000, -5000), it is clamped to +/-960 and +/-540
        viewport.pan_offset = Vec2::new(5000.0, -5000.0);
        let clamped_rect = viewport.compute_video_rect(available).expect("must compute rect");
        assert_eq!(clamped_rect.min, egui::pos2(0.0, -1080.0));

        // Returning to Fit resets pan to zero
        viewport.set_zoom_mode(ZoomMode::Fit);
        assert_eq!(viewport.pan_offset(), Vec2::ZERO);
        let fit_rect = viewport.compute_video_rect(available).expect("must compute rect");
        assert_eq!(fit_rect.min, egui::pos2(0.0, 0.0));
    }
}

