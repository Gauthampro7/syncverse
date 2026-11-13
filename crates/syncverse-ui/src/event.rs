//! Events delivered from the background runtime engine to the graphical UI presentation thread.

use std::sync::Arc;
use std::time::Instant;
use syncverse_chat::ChatRecord;
use crate::gui::RoomMember;

/// Rasterized RGBA bitmap subtitle cue ready for GPU overlay texture upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleBitmapCue {
    /// Presentation start timestamp in milliseconds.
    pub start_ms: u64,
    /// Presentation end timestamp in milliseconds.
    pub end_ms: u64,
    /// Horizontal offset on the video canvas in pixels.
    pub x: u32,
    /// Vertical offset on the video canvas in pixels.
    pub y: u32,
    /// Width of the bitmap rectangle in pixels.
    pub width: u32,
    /// Height of the bitmap rectangle in pixels.
    pub height: u32,
    /// Reference video canvas width in pixels (`0` if unspecified).
    pub canvas_width: u32,
    /// Reference video canvas height in pixels (`0` if unspecified).
    pub canvas_height: u32,
    /// Packed 32-bit RGBA pixel data (`width * height * 4` bytes).
    pub rgba: Arc<[u8]>,
}

/// Wave-2-ready track-scoped subtitle event supporting text cues, bitmap cues,
/// clear-packet truncation, and initial track auto-selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackSubtitleEvent {
    /// A decoded text subtitle cue routed to `track_index`.
    TextCue {
        /// Target subtitle track index.
        track_index: usize,
        /// Decoded text subtitle cue.
        cue: crate::gui::subtitle_overlay::SubtitleCue,
    },
    /// A decoded rasterized RGBA bitmap subtitle cue routed to `track_index`.
    BitmapCue {
        /// Target subtitle track index.
        track_index: usize,
        /// Decoded bitmap subtitle cue.
        cue: SubtitleBitmapCue,
    },
    /// Truncates active cues on `track_index` at `at_ms`.
    ClearActive {
        /// Target subtitle track index.
        track_index: usize,
        /// Presentation timestamp in milliseconds at which active cues expire.
        at_ms: u64,
    },
    /// Selects the initial subtitle track chosen by auto-selection (`None` disables subtitles).
    AutoSelectTrack {
        /// Selected subtitle track index, or `None` for Off.
        track_index: Option<usize>,
    },
}

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
    /// Playback reached the end of the loaded media (EOF).
    PlaybackEofReached,
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

    #[test]
    fn test_wave2_track_subtitle_event_types() {
        let bmp = SubtitleBitmapCue {
            start_ms: 1000,
            end_ms: 4000,
            x: 100,
            y: 800,
            width: 2,
            height: 2,
            canvas_width: 1920,
            canvas_height: 1080,
            rgba: Arc::from([255u8; 16]),
        };

        let text_ev = TrackSubtitleEvent::TextCue {
            track_index: 0,
            cue: crate::gui::subtitle_overlay::SubtitleCue::new(500, 1500, "Line"),
        };
        let bmp_ev = TrackSubtitleEvent::BitmapCue {
            track_index: 1,
            cue: bmp.clone(),
        };
        let clear_ev = TrackSubtitleEvent::ClearActive {
            track_index: 1,
            at_ms: 2500,
        };
        let auto_ev = TrackSubtitleEvent::AutoSelectTrack {
            track_index: Some(0),
        };

        assert!(matches!(text_ev, TrackSubtitleEvent::TextCue { track_index: 0, .. }));
        assert!(matches!(bmp_ev, TrackSubtitleEvent::BitmapCue { track_index: 1, .. }));
        assert!(matches!(clear_ev, TrackSubtitleEvent::ClearActive { track_index: 1, at_ms: 2500 }));
        assert!(matches!(auto_ev, TrackSubtitleEvent::AutoSelectTrack { track_index: Some(0) }));
    }
}
