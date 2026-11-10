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
    /// Jump forward to the start of the next chapter.
    NextChapter,
    /// Jump backward to the start of the current or previous chapter.
    PreviousChapter,
    /// Capture a lossless PNG screenshot of the current video frame.
    TakeScreenshot,
    /// Load an external subtitle file (.srt, .vtt, .ass) into the active session.
    LoadExternalSubtitle(PathBuf),
    /// Step presentation forward or backward by a single frame while paused.
    StepFrame {
        /// If true, advance forward by one frame (~42ms); if false, step backward by one frame (~42ms).
        forward: bool,
    },
    /// Toggle loop / repeat playback mode.
    ToggleLoop,
    /// Explicitly enable or disable loop / repeat playback mode.
    SetLoop(bool),
    /// Update audio equalizer settings (pre-amp, 10 band gains, enable toggle).
    SetAudioEqualizer(syncverse_codecs::audio::AudioEqualizerConfig),
    /// Update dynamic range compressor / night mode settings.
    SetAudioCompressor {
        /// Whether compressor is enabled.
        enabled: bool,
        /// Threshold in dBFS (e.g. -16.0).
        threshold_db: f32,
        /// Ratio (e.g. 4.0).
        ratio: f32,
        /// Makeup gain in dBFS (e.g. 2.0).
        makeup_gain_db: f32,
    },
    /// Set audio stereo balance / panning in range [-1.0, 1.0].
    SetAudioStereoPan(f32),
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
        let cmd_step_fwd = PlayerCommand::StepFrame { forward: true };
        let cmd_step_bwd = PlayerCommand::StepFrame { forward: false };
        let cmd_toggle_loop = PlayerCommand::ToggleLoop;
        let cmd_set_loop = PlayerCommand::SetLoop(true);
        let cmd_eq = PlayerCommand::SetAudioEqualizer(syncverse_codecs::audio::AudioEqualizerConfig::flat());
        let cmd_comp = PlayerCommand::SetAudioCompressor {
            enabled: true,
            threshold_db: -12.0,
            ratio: 4.0,
            makeup_gain_db: 2.0,
        };
        let cmd_pan = PlayerCommand::SetAudioStereoPan(0.5);

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
        assert_eq!(cmd_step_fwd, PlayerCommand::StepFrame { forward: true });
        assert_eq!(cmd_toggle_loop, PlayerCommand::ToggleLoop);
        assert_eq!(cmd_set_loop, PlayerCommand::SetLoop(true));
        assert_eq!(cmd_eq, PlayerCommand::SetAudioEqualizer(syncverse_codecs::audio::AudioEqualizerConfig::flat()));
        assert_eq!(cmd_pan, PlayerCommand::SetAudioStereoPan(0.5));
        assert_ne!(cmd_comp, cmd_eq);

        let cmd_comp_diff = PlayerCommand::SetAudioCompressor {
            enabled: false,
            threshold_db: -12.0,
            ratio: 4.0,
            makeup_gain_db: 2.0,
        };
        assert_ne!(cmd_comp, cmd_comp_diff);

        assert_ne!(cmd_audio, cmd_cycle_audio);
        assert_ne!(cmd_sub, cmd_sub_off);
        assert_ne!(cmd_step_fwd, cmd_step_bwd);
        assert_ne!(cmd_set_loop, PlayerCommand::SetLoop(false));
        assert_ne!(cmd_pan, PlayerCommand::SetAudioStereoPan(-0.5));
    }
}
