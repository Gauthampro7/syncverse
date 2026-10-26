//! Events delivered from the background runtime engine to the graphical UI presentation thread.

use syncplay_chat::ChatRecord;
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
}
