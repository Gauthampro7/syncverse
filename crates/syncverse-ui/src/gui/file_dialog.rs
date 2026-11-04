//! Asynchronous non-blocking native file dialog integration using `rfd`.
//!
//! Spawns file selection dialogs in detached threads to prevent stalling
//! the egui frame presentation loop.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

/// Purpose for which a native file dialog was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDialogPurpose {
    /// Open and immediately play a single media file.
    OpenMedia,
    /// Add one or more files to the active playlist queue.
    AddToPlaylist,
    /// Load an M3U / M3U8 playlist file.
    LoadM3u,
    /// Save current playlist queue to an M3U file.
    SaveM3u,
}

/// Result payload emitted upon asynchronous file dialog completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDialogResult {
    /// Action purpose associated with this dialog invocation.
    pub purpose: FileDialogPurpose,
    /// Files selected or destination save file path.
    pub paths: Vec<PathBuf>,
}

/// Non-blocking manager for native OS file selection dialogs.
pub struct FileDialogManager {
    receiver: Option<Receiver<FileDialogResult>>,
    is_dialog_active: bool,
}

impl Default for FileDialogManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FileDialogManager {
    /// Constructs a new `FileDialogManager`.
    pub fn new() -> Self {
        Self {
            receiver: None,
            is_dialog_active: false,
        }
    }

    /// Opens the native system file dialog asynchronously to pick a single media file to play.
    pub fn open_file_dialog(&mut self) {
        if self.is_dialog_active {
            return;
        }

        let (tx, rx) = channel();
        self.receiver = Some(rx);
        self.is_dialog_active = true;

        std::thread::spawn(move || {
            let file = rfd::FileDialog::new()
                .set_title("Open Media File — Syncverse")
                .add_filter(
                    "All Supported Media",
                    &[
                        "mkv", "webm", "mp4", "mov", "avi", "ts", "flv", "ogg", "ogv",
                        "mp3", "flac", "wav", "aac", "opus",
                    ],
                )
                .add_filter(
                    "Video Files (*.mkv, *.mp4, *.webm, *.avi, *.mov, *.ts, *.flv)",
                    &["mkv", "webm", "mp4", "mov", "avi", "ts", "flv"],
                )
                .add_filter(
                    "Audio Files (*.mp3, *.flac, *.wav, *.aac, *.opus, *.ogg)",
                    &["mp3", "flac", "wav", "aac", "opus", "ogg"],
                )
                .add_filter("All Files (*)", &["*"])
                .pick_file();

            if let Some(path) = file {
                let _ = tx.send(FileDialogResult {
                    purpose: FileDialogPurpose::OpenMedia,
                    paths: vec![path],
                });
            }
        });
    }

    /// Opens the native system file dialog asynchronously to pick one or more files to append to the playlist.
    pub fn open_add_to_playlist_dialog(&mut self) {
        if self.is_dialog_active {
            return;
        }

        let (tx, rx) = channel();
        self.receiver = Some(rx);
        self.is_dialog_active = true;

        std::thread::spawn(move || {
            let files = rfd::FileDialog::new()
                .set_title("Add Media to Playlist — Syncverse")
                .add_filter(
                    "All Supported Media",
                    &[
                        "mkv", "webm", "mp4", "mov", "avi", "ts", "flv", "ogg", "ogv",
                        "mp3", "flac", "wav", "aac", "opus",
                    ],
                )
                .add_filter("All Files (*)", &["*"])
                .pick_files();

            if let Some(paths) = files {
                if !paths.is_empty() {
                    let _ = tx.send(FileDialogResult {
                        purpose: FileDialogPurpose::AddToPlaylist,
                        paths,
                    });
                }
            }
        });
    }

    /// Opens the native system file dialog asynchronously to pick an M3U / M3U8 playlist file.
    pub fn open_load_m3u_dialog(&mut self) {
        if self.is_dialog_active {
            return;
        }

        let (tx, rx) = channel();
        self.receiver = Some(rx);
        self.is_dialog_active = true;

        std::thread::spawn(move || {
            let file = rfd::FileDialog::new()
                .set_title("Load Playlist File — Syncverse")
                .add_filter("Playlist Files (*.m3u, *.m3u8)", &["m3u", "m3u8"])
                .add_filter("All Files (*)", &["*"])
                .pick_file();

            if let Some(path) = file {
                let _ = tx.send(FileDialogResult {
                    purpose: FileDialogPurpose::LoadM3u,
                    paths: vec![path],
                });
            }
        });
    }

    /// Opens the native system file dialog asynchronously to save the current playlist to an M3U file.
    pub fn open_save_m3u_dialog(&mut self) {
        if self.is_dialog_active {
            return;
        }

        let (tx, rx) = channel();
        self.receiver = Some(rx);
        self.is_dialog_active = true;

        std::thread::spawn(move || {
            let file = rfd::FileDialog::new()
                .set_title("Save Playlist As — Syncverse")
                .set_file_name("playlist.m3u")
                .add_filter("M3U Playlist (*.m3u)", &["m3u"])
                .add_filter("All Files (*)", &["*"])
                .save_file();

            if let Some(path) = file {
                let _ = tx.send(FileDialogResult {
                    purpose: FileDialogPurpose::SaveM3u,
                    paths: vec![path],
                });
            }
        });
    }

    /// Polls for completed file dialog result without blocking.
    ///
    /// Returns `Some(FileDialogResult)` when dialog completes with selection, or `None` if pending/dismissed.
    pub fn poll_result(&mut self) -> Option<FileDialogResult> {
        let rx = self.receiver.as_ref()?;
        match rx.try_recv() {
            Ok(res) => {
                self.receiver = None;
                self.is_dialog_active = false;
                Some(res)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.receiver = None;
                self.is_dialog_active = false;
                None
            }
        }
    }

    /// Polls for completed file selection for opening media without blocking.
    ///
    /// Preserved for backwards compatibility with single-file open flows.
    pub fn poll_selected_file(&mut self) -> Option<PathBuf> {
        let res = self.poll_result()?;
        if res.purpose == FileDialogPurpose::OpenMedia {
            res.paths.into_iter().next()
        } else {
            None
        }
    }

    /// Returns whether a file dialog is currently open.
    pub fn is_active(&self) -> bool {
        self.is_dialog_active
    }
}
