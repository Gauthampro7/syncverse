//! Drag-and-drop media file loading and visual hover feedback overlay.

use std::path::{Path, PathBuf};

use egui::{Align2, Color32, Context, CornerRadius, FontId, Id, LayerId, Order, Rect, Stroke, StrokeKind, Vec2};

/// Supported media file extensions.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "mkv", "webm", "mp4", "m4v", "mov", "avi", "ts", "flv", "ogg", "ogv",
    "mp3", "flac", "wav", "aac", "opus",
];

/// Supported playlist file extensions.
pub const PLAYLIST_EXTENSIONS: &[&str] = &["m3u", "m3u8"];

/// Supported subtitle file extensions.
pub const SUBTITLE_EXTENSIONS: &[&str] = &["srt", "vtt", "ass", "ssa", "sub"];

/// Alias for supported subtitle file extensions.
pub const SUPPORTED_SUBTITLE_EXTS: &[&str] = SUBTITLE_EXTENSIONS;

/// Checks if the given path is a supported media file.
pub fn is_supported_media(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| SUPPORTED_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Checks if the given path is a supported playlist file (.m3u, .m3u8).
pub fn is_supported_playlist(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| PLAYLIST_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Checks if the given path is a supported subtitle file.
pub fn is_supported_subtitle(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| SUBTITLE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Action determined from a dropped file in the window.
#[derive(Debug, Clone, PartialEq)]
pub enum DroppedFileAction {
    /// Open a new media file.
    OpenMedia(PathBuf),
    /// Load an external subtitle file for the current media.
    LoadSubtitle(PathBuf),
    /// Load an external M3U playlist file into the playlist queue.
    LoadPlaylist(PathBuf),
}

/// Handler for window drag-and-drop operations and hover rendering.
pub struct DragDropHandler;

impl DragDropHandler {
    /// Inspects egui raw input and returns all dropped media, playlist, and subtitle file actions.
    pub fn handle_dropped_files(ctx: &Context) -> Vec<DroppedFileAction> {
        let mut actions = Vec::new();
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in &dropped {
            let path = file.path();
            if path.is_dir() {
                if let Ok(entries) = std::fs::read_dir(path) {
                    let mut dir_media = Vec::new();
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_file() {
                            if is_supported_playlist(&p) {
                                actions.push(DroppedFileAction::LoadPlaylist(p));
                            } else if is_supported_media(&p) {
                                dir_media.push(p);
                            }
                        }
                    }
                    dir_media.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
                    for p in dir_media {
                        actions.push(DroppedFileAction::OpenMedia(p));
                    }
                }
            } else if is_supported_playlist(path) {
                actions.push(DroppedFileAction::LoadPlaylist(path.to_path_buf()));
            } else if is_supported_media(path) {
                actions.push(DroppedFileAction::OpenMedia(path.to_path_buf()));
            } else if is_supported_subtitle(path) {
                actions.push(DroppedFileAction::LoadSubtitle(path.to_path_buf()));
            }
        }
        actions.sort_by_key(|a| match a {
            DroppedFileAction::LoadPlaylist(_) => 0,
            DroppedFileAction::OpenMedia(_) => 1,
            DroppedFileAction::LoadSubtitle(_) => 2,
        });
        actions
    }

    /// Inspects egui raw input and returns the first dropped media or subtitle file action, if any.
    pub fn handle_dropped_file(ctx: &Context) -> Option<DroppedFileAction> {
        Self::handle_dropped_files(ctx).into_iter().next()
    }

    /// Renders a centered semi-transparent drag-hover overlay when a file is hovered over the window.
    pub fn render_hover_overlay(ctx: &Context) {
        let is_hovered = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if !is_hovered {
            return;
        }

        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::unique("drag_hover_overlay")));
        let screen_rect = ctx.content_rect();
        let clipped = painter.with_clip_rect(screen_rect);

        // 1. Semi-transparent dark background
        clipped.rect_filled(screen_rect, CornerRadius::ZERO, Color32::from_rgba_premultiplied(10, 10, 15, 210));

        // 2. Centered dashed target box (constrained strictly inside screen_rect)
        let center = screen_rect.center();
        let max_box = (screen_rect.size() - Vec2::new(24.0, 24.0)).max(Vec2::new(160.0, 100.0));
        let box_size = Vec2::new(420.0, 220.0).min(max_box).min(screen_rect.size());
        let box_rect = Rect::from_center_size(center, box_size).intersect(screen_rect);

        clipped.rect_filled(box_rect, CornerRadius::same(16), Color32::from_rgba_premultiplied(20, 25, 35, 240));
        clipped.rect_stroke(
            box_rect,
            CornerRadius::same(16),
            Stroke::new(2.5, Color32::from_rgb(56, 189, 248)),
            StrokeKind::Inside,
        );

        // 3. Icon & Instructional Text
        let icon_size = if box_size.y < 160.0 { 32.0 } else { 54.0 };
        let title_size = if box_size.x < 340.0 { 14.0 } else { 20.0 };
        let sub_size = if box_size.x < 340.0 { 10.0 } else { 12.0 };

        clipped.text(
            center - Vec2::new(0.0, box_size.y * 0.16),
            Align2::CENTER_CENTER,
            "📂",
            FontId::proportional(icon_size),
            Color32::from_rgb(56, 189, 248),
        );

        clipped.text(
            center + Vec2::new(0.0, box_size.y * 0.11),
            Align2::CENTER_CENTER,
            "Drop Media to Play or Subtitles to Load",
            FontId::proportional(title_size),
            Color32::WHITE,
        );

        clipped.text(
            center + Vec2::new(0.0, box_size.y * 0.25),
            Align2::CENTER_CENTER,
            "Video: MKV, MP4, WebM, AVI | Subtitles: SRT, VTT, ASS, SSA, SUB",
            FontId::proportional(sub_size),
            Color32::from_rgb(160, 160, 175),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_supported_extensions() {
        assert!(is_supported_media(Path::new("video.mkv")));
        assert!(is_supported_media(Path::new("movie.MP4")));
        assert!(is_supported_media(Path::new("song.flac")));
        assert!(!is_supported_media(Path::new("notes.txt")));
        assert!(!is_supported_media(Path::new("playlist.m3u")));

        assert!(is_supported_playlist(Path::new("queue.m3u")));
        assert!(is_supported_playlist(Path::new("stream.M3U8")));
        assert!(!is_supported_playlist(Path::new("song.mp3")));

        assert!(is_supported_subtitle(Path::new("subs.srt")));
        assert!(is_supported_subtitle(Path::new("subs.vtt")));
        assert!(is_supported_subtitle(Path::new("subs.ass")));
        assert!(is_supported_subtitle(Path::new("subs.ssa")));
        assert!(is_supported_subtitle(Path::new("/tmp/movie.sub")));
        assert!(is_supported_subtitle(Path::new("/tmp/MOVIE.SUB")));
        assert!(!is_supported_subtitle(Path::new("subs.pdf")));
        assert!(SUPPORTED_SUBTITLE_EXTS.contains(&"sub"));
    }

    #[test]
    fn test_drag_drop_hover_overlay_compact_viewport_headless() {
        let ctx = Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(360.0, 240.0))),
            hovered_files: vec![egui::HoveredFile::default()],
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ctx| {
            DragDropHandler::render_hover_overlay(ctx);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn test_dropped_file_action_sorting() {
        let mut actions = [
            DroppedFileAction::LoadSubtitle(PathBuf::from("sub.srt")),
            DroppedFileAction::OpenMedia(PathBuf::from("movie.mkv")),
            DroppedFileAction::LoadPlaylist(PathBuf::from("list.m3u")),
        ];
        actions.sort_by_key(|a| match a {
            DroppedFileAction::LoadPlaylist(_) => 0,
            DroppedFileAction::OpenMedia(_) => 1,
            DroppedFileAction::LoadSubtitle(_) => 2,
        });
        assert!(matches!(actions[0], DroppedFileAction::LoadPlaylist(_)));
        assert!(matches!(actions[1], DroppedFileAction::OpenMedia(_)));
        assert!(matches!(actions[2], DroppedFileAction::LoadSubtitle(_)));
    }
}

