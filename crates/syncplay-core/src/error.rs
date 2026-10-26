//! Error types for the core runtime coordinator.

use thiserror::Error;

/// Error variants encountered during core coordinator operations.
#[derive(Error, Debug)]
pub enum CoreError {
    /// Container demuxing error.
    #[error("demux error: {0}")]
    Demux(#[from] syncplay_demux::DemuxError),

    /// Codec or color conversion error.
    #[error("codec error: {0}")]
    Codec(#[from] syncplay_codecs::CodecError),

    /// Master clock error.
    #[error("clock error: {0}")]
    Clock(#[from] syncplay_clock::ClockError),

    /// Network sync protocol error.
    #[error("sync error: {0}")]
    Sync(#[from] syncplay_sync::SyncError),

    /// Chat storage or search error.
    #[error("chat error: {0}")]
    Chat(#[from] syncplay_chat::ChatError),

    /// UI presentation error.
    #[error("UI error: {0}")]
    Ui(#[from] syncplay_ui::UiError),

    /// Standard I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Coordinator lifecycle error.
    #[error("runtime coordinator error: {0}")]
    Runtime(String),
}

impl From<syncplay_codecs::UniversalAudioError> for CoreError {
    fn from(err: syncplay_codecs::UniversalAudioError) -> Self {
        CoreError::Codec(syncplay_codecs::CodecError::from(err))
    }
}

