//! Asynchronous non-blocking native file dialog integration using `rfd`.
//!
//! Spawns file selection dialogs in detached threads to prevent stalling
//! the egui frame presentation loop.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

/// Non-blocking manager for native OS file selection dialogs.
pub struct FileDialogManager {
    receiver: Option<Receiver<PathBuf>>,
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

    /// Opens the native system file dialog asynchronously.
    ///
    /// Configured for all modern media container formats and audio codecs.
    pub fn open_file_dialog(&mut self) {
        if self.is_dialog_active {
            return;
        }

        let (tx, rx) = channel();
        self.receiver = Some(rx);
        self.is_dialog_active = true;

        std::thread::spawn(move || {
            let file = rfd::FileDialog::new()
                .set_title("Open Media File — Syncplay-Alt")
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
                let _ = tx.send(path);
            }
        });
    }

    /// Polls for completed file selection without blocking.
    ///
    /// Returns `Some(PathBuf)` when a file is picked, or `None` if dialog is still pending or dismissed.
    pub fn poll_selected_file(&mut self) -> Option<PathBuf> {
        let rx = self.receiver.as_ref()?;
        match rx.try_recv() {
            Ok(path) => {
                self.receiver = None;
                self.is_dialog_active = false;
                Some(path)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.receiver = None;
                self.is_dialog_active = false;
                None
            }
        }
    }

    /// Returns whether a file dialog is currently open.
    pub fn is_active(&self) -> bool {
        self.is_dialog_active
    }
}
