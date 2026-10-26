//! Player commands dispatched from UI event thread to core runtime coordinator.

use std::path::PathBuf;

/// Command emitted by the UI event thread into the command queue or coordinator channel.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerCommand {
    /// Toggle playback between Playing and Paused.
    TogglePause,
    /// Seek presentation to target PTS in milliseconds.
    Seek(u64),
    /// Set requested playback speed ratio.
    SetSpeed(f64),
    /// Stop playback and return to presentation PTS 0.
    Stop,
    /// Set normalized audio volume in range [0.0, 1.0].
    SetVolume(f32),
    /// Toggle audio mute state.
    ToggleMute,
    /// Open and load a new media container.
    OpenFile(PathBuf),
    /// Send chat message to watch-party room.
    SendChatMessage(String),
    /// Execute an interactive slash command.
    ExecuteSlashCommand(String),
    /// Host a watch-party room on the specified port.
    HostParty {
        /// Port to bind to (e.g. 18242).
        port: u16,
        /// Room name identifier.
        room: String,
        /// Local username.
        username: String,
    },
    /// Connect to a remote watch-party room.
    ConnectParty {
        /// Remote host address (e.g. "127.0.0.1:18242").
        addr: String,
        /// Room name identifier.
        room: String,
        /// Local username.
        username: String,
    },
    /// Leave or disconnect from the current watch-party room.
    DisconnectParty,
}
