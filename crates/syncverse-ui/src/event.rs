//! Events delivered from the background runtime engine to the graphical UI presentation thread.

use std::time::Instant;
use syncverse_chat::ChatRecord;
use crate::gui::RoomMember;

/// Events delivered from the background engine to the graphical UI thread.
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// New chat message to append to the sidebar chat history.
    NewChatRecord(ChatRecord),
    /// Updated list of watch-party room members.
    RoomMembersUpdated(Vec<RoomMember>),
    /// Playback unpause gate blocked notification with list of blocking usernames.
    UnpauseBlocked {
        /// List of usernames that are not ready or have not loaded media.
        unready_peers: Vec<String>,
        /// Descriptive alert message.
        message: String,
    },
    /// Notification that a newer version is available.
    UpdateAvailable {
        /// Latest version tag string.
        latest_version: String,
        /// Web URL to the GitHub release page.
        release_url: String,
        /// Direct download URL for the platform asset, if available.
        download_url: Option<String>,
    },
    /// Notification that an update check completed.
    UpdateChecked {
        /// Whether the local version is the latest.
        is_latest: bool,
        /// Current application version string.
        current_version: String,
    },
    /// Notification that an update check encountered an error.
    UpdateFailed {
        /// Error description message.
        error: String,
    },
    /// Transient HUD / OSD notification banner to display on the video viewport.
    HudNotification {
        /// Banner notification text to present.
        text: String,
        /// Instant when the notification was triggered.
        timestamp: Instant,
    },
    /// Notification that active audio track was selected or switched.
    AudioTrackSelected {
        /// Selected stream track identifier.
        track_id: u64,
        /// Human-readable track display name or metadata description.
        name: String,
    },
    /// Notification that active subtitle track was selected or switched.
    SubtitleTrackSelected {
        /// Selected subtitle track index (`None` when subtitles are disabled).
        track_index: Option<usize>,
        /// Human-readable track display name or "Off".
        name: String,
    },
    /// Notification that audio presentation delay was updated.
    AudioDelayChanged(i64),
    /// Notification that subtitle presentation delay was updated.
    SubtitleDelayChanged(i64),
    /// Discovered audio and subtitle track inventories for loaded media.
    TracksDiscovered {
        /// List of discovered audio track descriptions.
        audio_tracks: Vec<String>,
        /// List of discovered subtitle track descriptions.
        subtitle_tracks: Vec<String>,
    },
    /// Discovered subtitle track data with full parsed cues.
    SubtitleDataDiscovered(Vec<crate::gui::subtitle_overlay::SubtitleTrack>),
    /// Streaming subtitle cue dynamically demuxed from container bitstream.
    SubtitleCueDemuxed(crate::gui::subtitle_overlay::SubtitleCue),
    /// Discovered audio streams with detailed channel, codec, and sample rate metadata.
    AudioDataDiscovered(Vec<crate::gui::dialogs::AudioTrackItem>),
    /// Discovered video stream metadata including resolution, codec, and timescale.
    VideoMetadataDiscovered(crate::gui::dialogs::VideoStreamMetadata),
    /// Floating emoji reaction arrived from a watch-party peer.
    FloatingReaction {
        /// Emoji string (e.g. "❤️", "😂", "🔥").
        emoji: String,
        /// Username of the sender.
        username: String,
    },
    /// UPnP port forwarding status update.
    UpnpStatusUpdated {
        /// Whether port mapping is active on router.
        is_mapped: bool,
        /// Public WAN IP address of the router, if discoverable.
        external_ip: Option<String>,
        /// Forwarded UDP port.
        port: u16,
        /// Diagnostic message or status.
        message: String,
    },
    /// Discovered container chapter markers.
    ChaptersDiscovered(Vec<syncverse_demux::MediaChapter>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ui_event_hud_and_tracks() {
        let now = Instant::now();
        let hud_ev = UiEvent::HudNotification {
            text: "Audio: Track 1/2".to_string(),
            timestamp: now,
        };
        if let UiEvent::HudNotification { text, timestamp } = hud_ev.clone() {
            assert_eq!(text, "Audio: Track 1/2");
            assert_eq!(timestamp, now);
        } else {
            panic!("Expected HudNotification");
        }

        let audio_ev = UiEvent::AudioTrackSelected {
            track_id: 1,
            name: "English (AAC 2ch)".to_string(),
        };
        let sub_ev = UiEvent::SubtitleTrackSelected {
            track_index: Some(0),
            name: "English (.srt)".to_string(),
        };
        let sub_off = UiEvent::SubtitleTrackSelected {
            track_index: None,
            name: "Off".to_string(),
        };
        let audio_delay = UiEvent::AudioDelayChanged(-50);
        let sub_delay = UiEvent::SubtitleDelayChanged(50);
        let tracks_disc = UiEvent::TracksDiscovered {
            audio_tracks: vec!["English (AAC)".to_string()],
            subtitle_tracks: vec!["English (.srt)".to_string()],
        };

        let sub_data = UiEvent::SubtitleDataDiscovered(vec![
            crate::gui::subtitle_overlay::SubtitleTrack::new(
                0,
                "English",
                Some("en".to_string()),
                vec![crate::gui::subtitle_overlay::SubtitleCue::new(100, 500, "Test")],
            ),
        ]);
        let sub_cue = UiEvent::SubtitleCueDemuxed(crate::gui::subtitle_overlay::SubtitleCue::new(
            600,
            1200,
            "Streamed",
        ));

        let audio_data = UiEvent::AudioDataDiscovered(vec![crate::gui::dialogs::AudioTrackItem::new(
            0,
            "English",
            "aac",
        )]);
        let video_meta = UiEvent::VideoMetadataDiscovered(crate::gui::dialogs::VideoStreamMetadata::default());

        assert!(matches!(audio_ev, UiEvent::AudioTrackSelected { track_id: 1, .. }));
        assert!(matches!(sub_ev, UiEvent::SubtitleTrackSelected { track_index: Some(0), .. }));
        assert!(matches!(sub_off, UiEvent::SubtitleTrackSelected { track_index: None, .. }));
        assert!(matches!(audio_delay, UiEvent::AudioDelayChanged(-50)));
        assert!(matches!(sub_delay, UiEvent::SubtitleDelayChanged(50)));
        assert!(matches!(tracks_disc, UiEvent::TracksDiscovered { .. }));
        assert!(matches!(sub_data, UiEvent::SubtitleDataDiscovered(..)));
        assert!(matches!(audio_data, UiEvent::AudioDataDiscovered(..)));
        assert!(matches!(video_meta, UiEvent::VideoMetadataDiscovered(..)));
        assert!(matches!(sub_cue, UiEvent::SubtitleCueDemuxed(..)));
    }
}
