//! Playlist and folder queue management, auto-advancing, shuffle, and repeat modes.

use std::path::{Path, PathBuf};
use egui::{Align2, Color32, Context, RichText, Vec2};

/// Supported multimedia container and audio bitstream file extensions for folder discovery.
pub const SUPPORTED_PLAYLIST_EXTENSIONS: &[&str] = &[
    "mkv", "webm", "mp4", "m4v", "mov", "avi", "ts", "flv", "ogg", "ogv",
    "mp3", "flac", "wav", "aac", "opus",
];

/// Repeat mode for playlist queue playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaylistRepeatMode {
    /// Normal playback: stops when the last file in the queue finishes.
    #[default]
    Off,
    /// Repeats the currently playing file continuously.
    Track,
    /// Loops through the entire playlist continuously.
    All,
}

impl PlaylistRepeatMode {
    /// Human-readable display label.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Off => "Repeat: Off",
            Self::Track => "Repeat: Track (1)",
            Self::All => "Repeat: All",
        }
    }

    /// Cycles sequentially to the next repeat mode.
    pub fn cycle_next(&self) -> Self {
        match self {
            Self::Off => Self::Track,
            Self::Track => Self::All,
            Self::All => Self::Off,
        }
    }
}

/// An individual media file entry in the active playlist queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistItem {
    /// Absolute filesystem path to the media file.
    pub path: PathBuf,
    /// Extracted file name for display.
    pub filename: String,
}

impl PlaylistItem {
    /// Constructs a `PlaylistItem` from a filesystem path.
    pub fn new(path: PathBuf) -> Self {
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown File")
            .to_string();
        Self { path, filename }
    }

    /// Constructs a `PlaylistItem` with an explicit display title.
    pub fn with_title(path: PathBuf, title: String) -> Self {
        Self { path, filename: title }
    }
}

/// Manages media queue order, directory auto-scan, shuffle randomization, and track advancement.
#[derive(Debug, Clone, Default)]
pub struct PlaylistManager {
    /// Discovered or user-added queue items.
    pub items: Vec<PlaylistItem>,
    /// Currently playing index within `items`.
    pub current_index: Option<usize>,
    /// Active playlist repeat mode.
    pub repeat_mode: PlaylistRepeatMode,
    /// Whether shuffle random order is enabled.
    pub shuffle_enabled: bool,
    /// Random permutation indices for shuffle playback.
    shuffled_order: Vec<usize>,
    /// Current pointer inside `shuffled_order`.
    shuffled_pointer: usize,
}

impl PlaylistManager {
    /// Constructs a new empty `PlaylistManager`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Scans the directory containing `current_file` and populates the playlist with all
    /// supported multimedia files found in that directory in natural alphabetical order.
    pub fn scan_folder_of_file(&mut self, current_file: &Path) {
        let parent = match current_file.parent() {
            Some(p) => p,
            None => return,
        };

        let mut discovered = Vec::new();
        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let ext = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if SUPPORTED_PLAYLIST_EXTENSIONS.contains(&ext.as_str()) {
                        discovered.push(path);
                    }
                }
            }
        }

        // Sort naturally by filename
        discovered.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

        let cur_canonical = current_file.canonicalize().unwrap_or_else(|_| current_file.to_path_buf());
        let mut selected_idx = None;

        self.items.clear();
        for (i, p) in discovered.into_iter().enumerate() {
            if selected_idx.is_none() {
                let p_can = p.canonicalize().unwrap_or_else(|_| p.clone());
                if p_can == cur_canonical {
                    selected_idx = Some(i);
                }
            }
            self.items.push(PlaylistItem::new(p));
        }

        self.current_index = selected_idx;
        self.rebuild_shuffle_order();
    }

    /// Returns `true` if the playlist queue already contains the specified media file.
    pub fn contains(&self, path: &Path) -> bool {
        let cur_canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.items.iter().any(|item| {
            item.path == path
                || item.path.canonicalize().unwrap_or_else(|_| item.path.clone()) == cur_canonical
        })
    }

    /// Sets the current playing track to `path` if present in the playlist without clearing the queue.
    /// Returns `true` if found and updated.
    pub fn set_current_file(&mut self, path: &Path) -> bool {
        let cur_canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if let Some(idx) = self.items.iter().position(|item| {
            item.path == path
                || item.path.canonicalize().unwrap_or_else(|_| item.path.clone()) == cur_canonical
        }) {
            self.current_index = Some(idx);
            if let Some(pos) = self.shuffled_order.iter().position(|&i| i == idx) {
                self.shuffled_pointer = pos;
            }
            true
        } else {
            false
        }
    }

    /// Appends a media file to the playlist queue.
    pub fn add_file(&mut self, path: PathBuf) {
        if !self.items.iter().any(|item| item.path == path) {
            self.items.push(PlaylistItem::new(path));
            self.rebuild_shuffle_order();
        }
    }

    /// Removes an item from the playlist queue by index and adjusts the current index.
    pub fn remove_item(&mut self, index: usize) -> Option<PlaylistItem> {
        if index >= self.items.len() {
            return None;
        }
        let item = self.items.remove(index);
        if let Some(cur) = self.current_index {
            if index < cur {
                self.current_index = Some(cur - 1);
            } else if index == cur {
                if self.items.is_empty() {
                    self.current_index = None;
                } else if cur >= self.items.len() {
                    self.current_index = Some(self.items.len() - 1);
                }
            }
        }
        self.rebuild_shuffle_order();
        Some(item)
    }

    /// Moves the item at `index` up by one position in the queue.
    pub fn move_up(&mut self, index: usize) {
        if index > 0 && index < self.items.len() {
            self.items.swap(index, index - 1);
            if let Some(cur) = self.current_index {
                if cur == index {
                    self.current_index = Some(index - 1);
                } else if cur == index - 1 {
                    self.current_index = Some(index);
                }
            }
            self.rebuild_shuffle_order();
        }
    }

    /// Moves the item at `index` down by one position in the queue.
    pub fn move_down(&mut self, index: usize) {
        if index + 1 < self.items.len() {
            self.items.swap(index, index + 1);
            if let Some(cur) = self.current_index {
                if cur == index {
                    self.current_index = Some(index + 1);
                } else if cur == index + 1 {
                    self.current_index = Some(index);
                }
            }
            self.rebuild_shuffle_order();
        }
    }

    /// Saves the current playlist queue to an M3U file at `path`.
    pub fn save_to_m3u(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        let mut file = std::fs::File::create(path)?;
        writeln!(file, "#EXTM3U")?;
        for item in &self.items {
            writeln!(file, "#EXTINF:-1,{}", item.filename)?;
            writeln!(file, "{}", item.path.to_string_lossy())?;
        }
        Ok(())
    }

    /// Loads multimedia items from an M3U / M3U8 playlist file at `path`.
    ///
    /// Supports `#EXTINF` metadata directives, `file://` URI decoding, URL percent-decoding,
    /// and relative path resolution against the playlist directory. Returns the number of newly added tracks.
    pub fn load_from_m3u(&mut self, path: &Path) -> std::io::Result<usize> {
        use std::io::BufRead;
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let mut added = 0;
        let mut last_title = None;

        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("#EXTINF:") {
                if let Some((_dur, title)) = rest.split_once(',') {
                    let t = title.trim();
                    if !t.is_empty() {
                        last_title = Some(t.to_string());
                    }
                }
                continue;
            }
            if trimmed.starts_with('#') {
                continue;
            }

            let unescaped = if let Some(after) = trimmed.strip_prefix("file://") {
                let path_part = after.strip_prefix("localhost").unwrap_or(after);
                let mut decoded = decode_percent_encoded(path_part);
                if decoded.starts_with('/') && decoded.len() >= 3 && decoded.as_bytes()[2] == b':' {
                    decoded.remove(0);
                }
                decoded
            } else {
                decode_percent_encoded(trimmed)
            };

            let is_abs = unescaped.is_empty()
                || Path::new(&unescaped).is_absolute()
                || (unescaped.len() >= 3
                    && unescaped.as_bytes()[0].is_ascii_alphabetic()
                    && unescaped.as_bytes()[1] == b':'
                    && (unescaped.as_bytes()[2] == b'/' || unescaped.as_bytes()[2] == b'\\'));

            let p = PathBuf::from(unescaped);
            let full_path = if is_abs {
                p
            } else {
                parent.join(p)
            };

            if !self.contains(&full_path) {
                let item = match last_title.take() {
                    Some(t) => PlaylistItem::with_title(full_path, t),
                    None => PlaylistItem::new(full_path),
                };
                self.items.push(item);
                added += 1;
            } else {
                last_title = None;
            }
        }
        if added > 0 {
            self.rebuild_shuffle_order();
        }
        Ok(added)
    }

    /// Clears all items from the playlist queue.
    pub fn clear(&mut self) {
        self.items.clear();
        self.current_index = None;
        self.shuffled_order.clear();
        self.shuffled_pointer = 0;
    }

    /// Selects a track by index and returns its file path.
    pub fn select_index(&mut self, index: usize) -> Option<PathBuf> {
        if index < self.items.len() {
            self.current_index = Some(index);
            if let Some(pos) = self.shuffled_order.iter().position(|&i| i == index) {
                self.shuffled_pointer = pos;
            }
            Some(self.items[index].path.clone())
        } else {
            None
        }
    }

    /// Computes and returns the next media file in queue to play based on current index, shuffle, and repeat mode.
    pub fn next_track(&mut self) -> Option<PathBuf> {
        if self.items.is_empty() {
            return None;
        }

        if self.repeat_mode == PlaylistRepeatMode::Track {
            return self.current_index.map(|i| self.items[i].path.clone());
        }

        if self.shuffle_enabled {
            if self.shuffled_order.is_empty() {
                self.rebuild_shuffle_order();
            }
            if self.shuffled_pointer + 1 < self.shuffled_order.len() {
                self.shuffled_pointer += 1;
                let next_idx = self.shuffled_order[self.shuffled_pointer];
                self.current_index = Some(next_idx);
                return Some(self.items[next_idx].path.clone());
            } else if self.repeat_mode == PlaylistRepeatMode::All {
                self.rebuild_shuffle_order();
                self.shuffled_pointer = 0;
                let next_idx = self.shuffled_order[0];
                self.current_index = Some(next_idx);
                return Some(self.items[next_idx].path.clone());
            }
            return None;
        }

        // Sequential queue
        match self.current_index {
            Some(idx) => {
                if idx + 1 < self.items.len() {
                    let next = idx + 1;
                    self.current_index = Some(next);
                    Some(self.items[next].path.clone())
                } else if self.repeat_mode == PlaylistRepeatMode::All {
                    self.current_index = Some(0);
                    Some(self.items[0].path.clone())
                } else {
                    None
                }
            }
            None => {
                self.current_index = Some(0);
                Some(self.items[0].path.clone())
            }
        }
    }

    /// Computes and returns the previous media file in queue to play.
    pub fn previous_track(&mut self) -> Option<PathBuf> {
        if self.items.is_empty() {
            return None;
        }

        if self.repeat_mode == PlaylistRepeatMode::Track {
            return self.current_index.map(|i| self.items[i].path.clone());
        }

        if self.shuffle_enabled {
            if self.shuffled_pointer > 0 {
                self.shuffled_pointer -= 1;
                let prev_idx = self.shuffled_order[self.shuffled_pointer];
                self.current_index = Some(prev_idx);
                return Some(self.items[prev_idx].path.clone());
            } else if self.repeat_mode == PlaylistRepeatMode::All && !self.shuffled_order.is_empty() {
                self.shuffled_pointer = self.shuffled_order.len() - 1;
                let prev_idx = self.shuffled_order[self.shuffled_pointer];
                self.current_index = Some(prev_idx);
                return Some(self.items[prev_idx].path.clone());
            }
            return None;
        }

        // Sequential queue
        match self.current_index {
            Some(idx) => {
                if idx > 0 {
                    let prev = idx - 1;
                    self.current_index = Some(prev);
                    Some(self.items[prev].path.clone())
                } else if self.repeat_mode == PlaylistRepeatMode::All {
                    let last = self.items.len() - 1;
                    self.current_index = Some(last);
                    Some(self.items[last].path.clone())
                } else {
                    None
                }
            }
            None => {
                let last = self.items.len().saturating_sub(1);
                self.current_index = Some(last);
                Some(self.items[last].path.clone())
            }
        }
    }

    /// Toggles shuffle mode and re-indexes permutation.
    pub fn toggle_shuffle(&mut self) {
        self.shuffle_enabled = !self.shuffle_enabled;
        if self.shuffle_enabled {
            self.rebuild_shuffle_order();
        }
    }

    /// Cycles to the next playlist repeat mode.
    pub fn cycle_repeat(&mut self) -> PlaylistRepeatMode {
        self.repeat_mode = self.repeat_mode.cycle_next();
        self.repeat_mode
    }

    /// Rebuilds deterministic pseudo-random permutation order for shuffle playback.
    fn rebuild_shuffle_order(&mut self) {
        self.shuffled_order = (0..self.items.len()).collect();
        // Simple deterministic Fisher-Yates shuffle using fixed LCG PRNG
        let mut seed: u64 = 0x9E3779B97F4A7C15;
        for i in (1..self.shuffled_order.len()).rev() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let j = (seed >> 33) as usize % (i + 1);
            self.shuffled_order.swap(i, j);
        }
        self.shuffled_pointer = 0;
        if let Some(cur) = self.current_index {
            if let Some(pos) = self.shuffled_order.iter().position(|&i| i == cur) {
                self.shuffled_pointer = pos;
            }
        }
    }
}

/// User interaction action emitted by the playlist queue dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaylistAction {
    /// User selected a track to play immediately.
    PlayTrack(PathBuf),
    /// User requested to open native file picker to add media.
    AddMediaFiles,
    /// User requested to open native file picker to load an M3U playlist.
    LoadM3u,
    /// User requested to open native file picker to save current queue as M3U.
    SaveM3u,
    /// User requested to clear queue.
    ClearQueue,
}

/// Modal or floating dialog presenting the playlist queue and playback order controls.
#[derive(Debug, Clone, Default)]
pub struct PlaylistDialog {
    /// Whether the playlist window is open.
    pub is_open: bool,
}

impl PlaylistDialog {
    /// Constructs a new `PlaylistDialog`.
    pub fn new() -> Self {
        Self { is_open: false }
    }

    /// Opens the playlist dialog window.
    pub fn open(&mut self) {
        self.is_open = true;
    }

    /// Closes the playlist dialog window.
    pub fn close(&mut self) {
        self.is_open = false;
    }

    /// Toggles open/close state.
    pub fn toggle(&mut self) {
        self.is_open = !self.is_open;
    }

    /// Renders the playlist window if open and returns user actions.
    pub fn show(
        &mut self,
        ctx: &Context,
        manager: &mut PlaylistManager,
    ) -> Option<PlaylistAction> {
        if !self.is_open {
            return None;
        }

        let mut action = None;
        let mut is_open = self.is_open;
        let screen = ctx.content_rect();
        let max_size = (screen.size() - Vec2::new(24.0, 24.0)).max(Vec2::new(240.0, 160.0));
        let default_size = Vec2::new(540.0, 420.0).min(max_size);

        egui::Window::new("🎶 Playlist Queue")
            .open(&mut is_open)
            .collapsible(false)
            .resizable(true)
            .vscroll(true)
            .default_size(default_size)
            .max_size(max_size)
            .constrain_to(screen)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("➕ Add...").on_hover_text("Add media files to queue").clicked() {
                        action = Some(PlaylistAction::AddMediaFiles);
                    }
                    if ui.button("📂 Load M3U...").on_hover_text("Load playlist from .m3u or .m3u8 file").clicked() {
                        action = Some(PlaylistAction::LoadM3u);
                    }
                    let save_btn = ui.add_enabled(!manager.items.is_empty(), egui::Button::new("💾 Save M3U..."));
                    if save_btn.on_hover_text("Save current queue as .m3u playlist").clicked() {
                        action = Some(PlaylistAction::SaveM3u);
                    }
                    if ui.button("🗑️ Clear").on_hover_text("Clear all tracks from queue").clicked() {
                        manager.clear();
                        action = Some(PlaylistAction::ClearQueue);
                    }

                    ui.separator();

                    let shuffle_btn = if manager.shuffle_enabled {
                        RichText::new("🔀 Shuffle On").color(Color32::from_rgb(56, 189, 248))
                    } else {
                        RichText::new("🔀 Shuffle Off").color(Color32::from_rgb(156, 163, 175))
                    };
                    if ui.button(shuffle_btn).clicked() {
                        manager.toggle_shuffle();
                    }

                    let rep_label = match manager.repeat_mode {
                        PlaylistRepeatMode::Off => "🔁 Repeat: Off",
                        PlaylistRepeatMode::Track => "🔂 Repeat: Track",
                        PlaylistRepeatMode::All => "🔁 Repeat: All",
                    };
                    if ui.button(rep_label).clicked() {
                        manager.cycle_repeat();
                    }
                });

                ui.separator();

                ui.label(
                    RichText::new(format!("Tracks in queue: {}", manager.items.len()))
                        .italics()
                        .color(Color32::from_rgb(156, 163, 175)),
                );

                ui.add_space(4.0);

                egui::ScrollArea::vertical().show(ui, |ui| {
                    if manager.items.is_empty() {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("Playlist queue is empty.\nOpen a video, drop a file, or click 'Add...' to queue files.")
                                    .color(Color32::from_rgb(100, 116, 139)),
                            );
                        });
                    } else {
                        let mut selected_to_play = None;
                        let mut move_up_idx = None;
                        let mut move_down_idx = None;
                        let mut remove_idx = None;

                        for (idx, item) in manager.items.iter().enumerate() {
                            let is_current = manager.current_index == Some(idx);
                            let title = format!("{}. {}", idx + 1, item.filename);

                            let text = if is_current {
                                RichText::new(format!("▶ {}", title))
                                    .strong()
                                    .color(Color32::from_rgb(56, 189, 248))
                            } else {
                                RichText::new(format!("   {}", title))
                                    .color(Color32::WHITE)
                            };

                            ui.horizontal(|ui| {
                                let up_enabled = idx > 0;
                                if ui.add_enabled(up_enabled, egui::Button::new("▲").small()).on_hover_text("Move track up").clicked() {
                                    move_up_idx = Some(idx);
                                }
                                let down_enabled = idx + 1 < manager.items.len();
                                if ui.add_enabled(down_enabled, egui::Button::new("▼").small()).on_hover_text("Move track down").clicked() {
                                    move_down_idx = Some(idx);
                                }
                                if ui.button(RichText::new("✖").small().color(Color32::from_rgb(239, 68, 68))).on_hover_text("Remove track from queue").clicked() {
                                    remove_idx = Some(idx);
                                }

                                let resp = ui
                                    .scope(|ui| {
                                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                                        ui.selectable_label(is_current, text)
                                    })
                                    .inner
                                    .on_hover_text(&title);
                                if resp.clicked() || resp.double_clicked() {
                                    selected_to_play = Some(idx);
                                }
                            });
                        }

                        if let Some(idx) = move_up_idx {
                            manager.move_up(idx);
                        }
                        if let Some(idx) = move_down_idx {
                            manager.move_down(idx);
                        }
                        if let Some(idx) = remove_idx {
                            manager.remove_item(idx);
                        }
                        if let Some(idx) = selected_to_play {
                            if let Some(path) = manager.select_index(idx) {
                                action = Some(PlaylistAction::PlayTrack(path));
                            }
                        }
                    }
                });
            });

        self.is_open = is_open;
        action
    }
}

/// Decodes URL percent-encoded characters (e.g. `%20` to space).
pub fn decode_percent_encoded(s: &str) -> String {
    let raw = s.as_bytes();
    let mut bytes = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            if let (Some(h1), Some(h2)) = (
                (raw[i + 1] as char).to_digit(16),
                (raw[i + 2] as char).to_digit(16),
            ) {
                bytes.push(((h1 << 4) | h2) as u8);
                i += 3;
                continue;
            }
        }
        bytes.push(raw[i]);
        i += 1;
    }
    String::from_utf8(bytes).unwrap_or_else(|_| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playlist_manager_basic_queue_flow() {
        let mut mgr = PlaylistManager::new();
        assert_eq!(mgr.items.len(), 0);
        assert_eq!(mgr.current_index, None);

        mgr.add_file(PathBuf::from("/media/video1.mkv"));
        mgr.add_file(PathBuf::from("/media/video2.mp4"));
        mgr.add_file(PathBuf::from("/media/video3.webm"));

        assert_eq!(mgr.items.len(), 3);
        assert_eq!(mgr.items[0].filename, "video1.mkv");
        assert_eq!(mgr.items[1].filename, "video2.mp4");

        // Next from start
        let next1 = mgr.next_track();
        assert_eq!(next1, Some(PathBuf::from("/media/video1.mkv")));
        assert_eq!(mgr.current_index, Some(0));

        let next2 = mgr.next_track();
        assert_eq!(next2, Some(PathBuf::from("/media/video2.mp4")));
        assert_eq!(mgr.current_index, Some(1));

        // Previous
        let prev = mgr.previous_track();
        assert_eq!(prev, Some(PathBuf::from("/media/video1.mkv")));
        assert_eq!(mgr.current_index, Some(0));

        // Advance to end
        mgr.next_track(); // to 1
        mgr.next_track(); // to 2
        assert_eq!(mgr.current_index, Some(2));

        // At end with Repeat::Off -> None
        assert_eq!(mgr.next_track(), None);

        // Turn on Repeat::All -> wraps around to 0
        mgr.repeat_mode = PlaylistRepeatMode::All;
        assert_eq!(mgr.next_track(), Some(PathBuf::from("/media/video1.mkv")));
        assert_eq!(mgr.current_index, Some(0));

        // Turn on Repeat::Track -> stays on track
        mgr.repeat_mode = PlaylistRepeatMode::Track;
        assert_eq!(mgr.next_track(), Some(PathBuf::from("/media/video1.mkv")));
        assert_eq!(mgr.current_index, Some(0));
    }

    #[test]
    fn test_playlist_repeat_mode_cycling() {
        let mut rep = PlaylistRepeatMode::Off;
        rep = rep.cycle_next();
        assert_eq!(rep, PlaylistRepeatMode::Track);
        rep = rep.cycle_next();
        assert_eq!(rep, PlaylistRepeatMode::All);
        rep = rep.cycle_next();
        assert_eq!(rep, PlaylistRepeatMode::Off);
    }

    #[test]
    fn test_playlist_reorder_and_removal() {
        let mut mgr = PlaylistManager::new();
        mgr.add_file(PathBuf::from("/media/trackA.mp3"));
        mgr.add_file(PathBuf::from("/media/trackB.mp3"));
        mgr.add_file(PathBuf::from("/media/trackC.mp3"));
        mgr.select_index(1); // playing trackB at index 1

        // Move trackB down: order becomes [trackA, trackC, trackB], current_index follows to 2
        mgr.move_down(1);
        assert_eq!(mgr.items[0].filename, "trackA.mp3");
        assert_eq!(mgr.items[1].filename, "trackC.mp3");
        assert_eq!(mgr.items[2].filename, "trackB.mp3");
        assert_eq!(mgr.current_index, Some(2));

        // Move trackB up: order becomes [trackA, trackB, trackC], current_index follows to 1
        mgr.move_up(2);
        assert_eq!(mgr.items[0].filename, "trackA.mp3");
        assert_eq!(mgr.items[1].filename, "trackB.mp3");
        assert_eq!(mgr.items[2].filename, "trackC.mp3");
        assert_eq!(mgr.current_index, Some(1));

        // Remove trackA (index 0, before current): current_index decrements to 0
        let removed = mgr.remove_item(0);
        assert_eq!(removed.unwrap().filename, "trackA.mp3");
        assert_eq!(mgr.items.len(), 2);
        assert_eq!(mgr.items[0].filename, "trackB.mp3");
        assert_eq!(mgr.current_index, Some(0));

        // Remove trackB (current item): current_index stays 0 (now trackC)
        let removed = mgr.remove_item(0);
        assert_eq!(removed.unwrap().filename, "trackB.mp3");
        assert_eq!(mgr.items.len(), 1);
        assert_eq!(mgr.items[0].filename, "trackC.mp3");
        assert_eq!(mgr.current_index, Some(0));

        // Remove last remaining item: current_index becomes None
        mgr.remove_item(0);
        assert_eq!(mgr.items.len(), 0);
        assert_eq!(mgr.current_index, None);
    }

    #[test]
    fn test_playlist_m3u_save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("syncverse_m3u_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let m3u_path = dir.join("test_playlist.m3u");

        let mut mgr = PlaylistManager::new();
        mgr.add_file(dir.join("song1.mp3"));
        mgr.add_file(dir.join("song2.flac"));
        mgr.add_file(dir.join("video.mkv"));

        assert_eq!(mgr.items.len(), 3);
        mgr.save_to_m3u(&m3u_path).expect("save m3u must succeed");

        let mut loaded_mgr = PlaylistManager::new();
        let count = loaded_mgr.load_from_m3u(&m3u_path).expect("load m3u must succeed");
        assert_eq!(count, 3);
        assert_eq!(loaded_mgr.items.len(), 3);
        assert_eq!(loaded_mgr.items[0].filename, "song1.mp3");
        assert_eq!(loaded_mgr.items[1].filename, "song2.flac");
        assert_eq!(loaded_mgr.items[2].filename, "video.mkv");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_decode_percent_encoded() {
        assert_eq!(decode_percent_encoded("hello%20world.mp3"), "hello world.mp3");
        assert_eq!(decode_percent_encoded("Artist%20-%20Song%20%28Remix%29.flac"), "Artist - Song (Remix).flac");
        assert_eq!(decode_percent_encoded("plain_text.mkv"), "plain_text.mkv");
        assert_eq!(decode_percent_encoded("percent%25symbol.mp3"), "percent%symbol.mp3");
    }

    #[test]
    fn test_playlist_m3u_extinf_and_file_uri_parsing() {
        let dir = std::env::temp_dir().join(format!("syncverse_m3u_extinf_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let m3u_path = dir.join("playlist.m3u8");

        let m3u_content = "\
#EXTM3U
#EXTINF:210,Queen - Bohemian Rhapsody
file:///media/Music/Queen%20-%20Bohemian%20Rhapsody.mp3
#EXTINF:180,Track Two Custom Title
relative_song.flac
#EXTVLCOPT:network-caching=1000
# Comment line
/absolute/path/song3.opus
";
        std::fs::write(&m3u_path, m3u_content).expect("write test m3u must succeed");

        let mut mgr = PlaylistManager::new();
        let loaded = mgr.load_from_m3u(&m3u_path).expect("load m3u must succeed");
        assert_eq!(loaded, 3);
        assert_eq!(mgr.items.len(), 3);

        // Check track 1: EXTINF title parsed, file:// stripped, percent-decoding applied
        assert_eq!(mgr.items[0].filename, "Queen - Bohemian Rhapsody");
        assert_eq!(mgr.items[0].path, PathBuf::from("/media/Music/Queen - Bohemian Rhapsody.mp3"));

        // Check track 2: EXTINF title parsed, relative path resolved against m3u directory
        assert_eq!(mgr.items[1].filename, "Track Two Custom Title");
        assert_eq!(mgr.items[1].path, dir.join("relative_song.flac"));

        // Check track 3: no EXTINF title, falls back to file_name "song3.opus"
        assert_eq!(mgr.items[2].filename, "song3.opus");
        assert_eq!(mgr.items[2].path, PathBuf::from("/absolute/path/song3.opus"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_playlist_dialog_viewport_clamped_with_long_titles() {
        let ctx = Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(480.0, 320.0));
        let raw = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };

        let mut dlg = PlaylistDialog::new();
        dlg.open();
        let mut mgr = PlaylistManager::new();
        mgr.add_file(PathBuf::from(
            "/media/movies/An.Extremely.Long.Media.Filename.2026.2160p.UHD.BluRay.REMUX.HEVC.DTS-HD.MA.7.1-GROUP.mkv",
        ));
        mgr.select_index(0);

        for _ in 0..2 {
            let mut out = ctx.run_ui(raw.clone(), |ctx| {
                let _ = dlg.show(ctx, &mut mgr);
            });
            out.textures_delta.clear();
        }

        let rect = ctx
            .memory(|m| m.area_rect(egui::Id::unique(Some("🎶 Playlist Queue"))))
            .expect("PlaylistDialog area_rect must exist");
        assert!(
            screen.contains_rect(rect),
            "PlaylistDialog {:?} escaped 480x320 {:?}",
            rect,
            screen
        );
    }
}
