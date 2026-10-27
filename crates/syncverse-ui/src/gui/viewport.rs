//! GPU-accelerated video rendering viewport and aspect ratio preservation.

use std::time::{Duration, Instant};
use egui::{Color32, Context, Rect, TextureHandle, TextureOptions, Ui, Vec2};
use syncverse_clock::{MasterClock, PresentationAction, SpscRingBuffer};
use syncverse_codecs::VideoFrameRGBA;

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
        if available_w <= 0.0 || available_h <= 0.0 || video_w <= 0.0 || video_h <= 0.0 {
            return Self {
                render_width: 0.0,
                render_height: 0.0,
                offset_x: 0.0,
                offset_y: 0.0,
                mode: ViewportDisplayMode::Empty,
            };
        }

        let aspect_video = video_w / video_h;
        let aspect_avail = available_w / available_h;
        let diff = aspect_avail - aspect_video;

        if diff > 1e-5 {
            // Viewport wider than video -> Pillarboxing
            let render_w = available_h * aspect_video;
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
            // Viewport taller than video -> Letterboxing
            let render_w = available_w;
            let render_h = available_w / aspect_video;
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
        let w = frame.width as usize;
        let h = frame.height as usize;
        let stride = frame.stride;
        let total_packed = w * h * 4;

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
    }
}

/// Actions emitted by the video viewport widget to the parent window application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ViewportAction {
    /// User double-clicked: toggle fullscreen.
    pub toggle_fullscreen: bool,
    /// User clicked: toggle play/pause.
    pub toggle_play_pause: bool,
}

/// GPU-accelerated video rendering viewport managing texture allocation and presentation.
pub struct VideoViewport {
    texture: Option<TextureHandle>,
    texture_buffer: VideoTextureBuffer,
    current_pts_ms: Option<u64>,
    video_width: u32,
    video_height: u32,
    pending_frame: Option<VideoFrameRGBA>,
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
            video_width: 0,
            video_height: 0,
            pending_frame: None,
        }
    }

    /// Consumes incoming video frames from SPSC ring buffer according to MasterClock pacing.
    pub fn update_playback(
        &mut self,
        ctx: &Context,
        video_ring: &SpscRingBuffer<VideoFrameRGBA, 32>,
        clock: &MasterClock,
        now: Instant,
    ) {
        if !clock.is_playing() {
            if self.texture.is_none() {
                if let Some(frame) = self.pending_frame.take().or_else(|| video_ring.try_pop()) {
                    clock.seek_to(frame.pts_ms as f64, now);
                    self.upload_texture(ctx, &frame);
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
                    ctx.request_repaint_after(Duration::from_millis(8));
                    break;
                }
            };

            // First frame presentation: Anchor presentation clock to frame timestamp for zero startup jitter
            if self.texture.is_none() {
                clock.seek_to(frame.pts_ms as f64, now);
                self.upload_texture(ctx, &frame);
                ctx.request_repaint();
                break;
            }

            let action = clock.evaluate_presentation_timing(frame.pts_ms, now);
            match action {
                PresentationAction::Wait(duration) => {
                    self.pending_frame = Some(frame);
                    ctx.request_repaint_after(duration);
                    break;
                }
                PresentationAction::PresentNow | PresentationAction::PresentNowLate => {
                    self.upload_texture(ctx, &frame);
                    ctx.request_repaint();
                    break;
                }
                PresentationAction::DropLateFrame => {
                    continue;
                }
            }
        }
    }

    /// Resets the viewport state and texture handle for a newly opened media stream.
    pub fn reset(&mut self) {
        self.texture = None;
        self.pending_frame = None;
        self.current_pts_ms = None;
        self.video_width = 0;
        self.video_height = 0;
    }

    /// Uploads frame RGBA data to the `egui::TextureHandle`.
    pub fn upload_texture(&mut self, ctx: &Context, frame: &VideoFrameRGBA) {
        if self.current_pts_ms == Some(frame.pts_ms) && self.texture.is_some() {
            return;
        }

        let w = frame.width as usize;
        let h = frame.height as usize;
        let bytes = self.texture_buffer.prepare(frame);
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
        self.video_width = frame.width;
        self.video_height = frame.height;
    }

    /// Returns the currently active video dimensions `(width, height)`.
    pub fn video_dimensions(&self) -> (u32, u32) {
        (self.video_width, self.video_height)
    }

    /// Renders the video viewport widget into egui with automatic pillarbox/letterbox fill.
    pub fn show(&mut self, ui: &mut Ui) -> ViewportAction {
        let mut action = ViewportAction::default();
        let available_rect = ui.available_rect_before_wrap();
        let available_w = available_rect.width();
        let available_h = available_rect.height();

        // 1. Allocate full interactive space
        let (rect, response) = ui.allocate_exact_size(available_rect.size(), egui::Sense::click());

        // 2. Black background fill (letterbox/pillarbox margins)
        ui.painter().rect_filled(rect, 0.0, Color32::BLACK);

        // 3. Aspect ratio calculation
        let geom = ViewportGeometry::compute(
            available_w,
            available_h,
            self.video_width as f32,
            self.video_height as f32,
        );

        // 4. Render video frame texture if available
        if let Some(ref texture) = self.texture {
            if geom.mode != ViewportDisplayMode::Empty {
                let video_min = rect.min + egui::vec2(geom.offset_x, geom.offset_y);
                let video_rect = Rect::from_min_size(
                    video_min,
                    Vec2::new(geom.render_width, geom.render_height),
                );
                let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter().image(texture.id(), video_rect, uv, Color32::WHITE);
            }
        } else {
            // Sleek centered placeholder when no media is playing
            let center = rect.center();
            ui.painter().text(
                center,
                egui::Align2::CENTER_CENTER,
                "Syncverse\nDrop video file here or click Open File",
                egui::FontId::proportional(18.0),
                Color32::from_rgb(100, 116, 139),
            );
        }

        // 5. Handle mouse click events
        if response.double_clicked() {
            action.toggle_fullscreen = true;
        } else if response.clicked() {
            action.toggle_play_pause = true;
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
}
