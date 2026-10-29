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
        /// Optional room password for ChaCha20-Poly1305 E2E encryption.
        password: Option<String>,
        /// Whether to attempt UPnP automatic port-forwarding.
        enable_upnp: bool,
    },
    /// Connect to a remote watch-party room.
    ConnectParty {
        /// Remote host address (e.g. "127.0.0.1:18242").
        addr: String,
        /// Room name identifier.
        room: String,
        /// Local username.
        username: String,
        /// Optional room password for ChaCha20-Poly1305 E2E encryption.
        password: Option<String>,
    },
    /// Dispatches a floating emoji reaction to the room.
    SendReaction(String),
    /// Leave or disconnect from the current watch-party room.
    DisconnectParty,
    /// Set local user readiness state ("Ready to Watch" checkbox).
    SetReady(bool),
    /// Set host Force Play bypass state.
    SetForcePlay(bool),
    /// Host trigger to bypass unpause gate immediately.
    ForcePlay,
    /// Select audio track by track ID.
    SelectAudioTrack(u64),
    /// Cycle to the next discovered audio track.
    CycleAudioTrack,
    /// Set presentation audio delay in milliseconds.
    SetAudioDelay(i64),
    /// Adjust presentation audio delay by delta milliseconds.
    AdjustAudioDelay(i64),
    /// Select active subtitle track by index (`None` disables subtitles).
    SelectSubtitleTrack(Option<usize>),
    /// Cycle to the next discovered subtitle track (including Off).
    CycleSubtitleTrack,
    /// Set presentation subtitle delay in milliseconds.
    SetSubtitleDelay(i64),
    /// Adjust presentation subtitle delay by delta milliseconds.
    AdjustSubtitleDelay(i64),
    /// Cycle video aspect ratio to the next preset mode.
    CycleAspectRatio,
    /// Load an external subtitle file (.srt, .vtt, .ass) into the active session.
    LoadExternalSubtitle(PathBuf),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_command_variants() {
        let cmd_audio = PlayerCommand::SelectAudioTrack(1);
        let cmd_cycle_audio = PlayerCommand::CycleAudioTrack;
        let cmd_audio_delay = PlayerCommand::SetAudioDelay(-50);
        let cmd_adj_audio = PlayerCommand::AdjustAudioDelay(50);
        let cmd_sub = PlayerCommand::SelectSubtitleTrack(Some(0));
        let cmd_sub_off = PlayerCommand::SelectSubtitleTrack(None);
        let cmd_cycle_sub = PlayerCommand::CycleSubtitleTrack;
        let cmd_sub_delay = PlayerCommand::SetSubtitleDelay(100);
        let cmd_adj_sub = PlayerCommand::AdjustSubtitleDelay(-100);
        let cmd_aspect = PlayerCommand::CycleAspectRatio;

        assert_eq!(cmd_audio, PlayerCommand::SelectAudioTrack(1));
        assert_eq!(cmd_cycle_audio, PlayerCommand::CycleAudioTrack);
        assert_eq!(cmd_audio_delay, PlayerCommand::SetAudioDelay(-50));
        assert_eq!(cmd_adj_audio, PlayerCommand::AdjustAudioDelay(50));
        assert_eq!(cmd_sub, PlayerCommand::SelectSubtitleTrack(Some(0)));
        assert_eq!(cmd_sub_off, PlayerCommand::SelectSubtitleTrack(None));
        assert_eq!(cmd_cycle_sub, PlayerCommand::CycleSubtitleTrack);
        assert_eq!(cmd_sub_delay, PlayerCommand::SetSubtitleDelay(100));
        assert_eq!(cmd_adj_sub, PlayerCommand::AdjustSubtitleDelay(-100));
        assert_eq!(cmd_aspect, PlayerCommand::CycleAspectRatio);

        assert_ne!(cmd_audio, cmd_cycle_audio);
        assert_ne!(cmd_sub, cmd_sub_off);
    }
}
