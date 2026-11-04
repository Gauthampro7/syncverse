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
pub const SUBTITLE_EXTENSIONS: &[&str] = &["srt", "vtt", "ass", "ssa"];

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

        // 1. Semi-transparent dark background
        painter.rect_filled(screen_rect, CornerRadius::ZERO, Color32::from_rgba_premultiplied(10, 10, 15, 210));

        // 2. Centered dashed target box
        let center = screen_rect.center();
        let box_size = Vec2::new(420.0, 220.0);
        let box_rect = Rect::from_center_size(center, box_size);

        painter.rect_filled(box_rect, CornerRadius::same(16), Color32::from_rgba_premultiplied(20, 25, 35, 240));
        painter.rect_stroke(
            box_rect,
            CornerRadius::same(16),
            Stroke::new(2.5, Color32::from_rgb(56, 189, 248)),
            StrokeKind::Inside,
        );

        // 3. Icon & Instructional Text
        painter.text(
            center - Vec2::new(0.0, 30.0),
            Align2::CENTER_CENTER,
            "📂",
            FontId::proportional(54.0),
            Color32::from_rgb(56, 189, 248),
        );

        painter.text(
            center + Vec2::new(0.0, 25.0),
            Align2::CENTER_CENTER,
            "Drop Media to Play or Subtitles to Load",
            FontId::proportional(20.0),
            Color32::WHITE,
        );

        painter.text(
            center + Vec2::new(0.0, 55.0),
            Align2::CENTER_CENTER,
            "Video: MKV, MP4, WebM, AVI | Subtitles: SRT, VTT, ASS, SSA",
            FontId::proportional(12.0),
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
        assert!(!is_supported_subtitle(Path::new("subs.pdf")));
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

