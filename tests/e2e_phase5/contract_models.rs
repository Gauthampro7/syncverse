//! Interface contracts and specification models for Phase 5 E2E Testing.
//!
//! Defined in accordance with `PROJECT.md § Interface Contracts`,
//! `TEST_INFRA.md`, and `ORIGINAL_REQUEST.md §R1-R4`.

/// Standardized room activity chat announcements formatter.
///
/// Implements RFC-conformant text generation with emojis matching ORIGINAL_REQUEST §R4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoomAnnouncement;

impl RoomAnnouncement {
    /// Formats announcement when a user loads or switches a media file:
    /// `"🎬 {user} loaded '{filename}' ({duration_hms})"`
    pub fn media_loaded(user: &str, filename: &str, duration_hms: &str) -> String {
        format!("🎬 {} loaded '{}' ({})", user, filename, duration_hms)
    }

    /// Formats announcement when a user marks themselves ready:
    /// `"✓ {user} is ready to watch"`
    pub fn ready(user: &str) -> String {
        format!("✓ {} is ready to watch", user)
    }

    /// Formats announcement when a user marks themselves not ready:
    /// `"⏳ {user} is not ready"`
    pub fn not_ready(user: &str) -> String {
        format!("⏳ {} is not ready", user)
    }

    /// Formats announcement when playback is paused:
    /// `"⏸ {user} paused playback"`
    pub fn paused(user: &str) -> String {
        format!("⏸ {} paused playback", user)
    }

    /// Formats announcement when playback is resumed:
    /// `"▶ {user} resumed playback"`
    pub fn resumed(user: &str) -> String {
        format!("▶ {} resumed playback", user)
    }

    /// Formats announcement when a user seeks:
    /// `"⏩ {user} seeked to {timecode}"`
    pub fn seeked(user: &str, timecode: &str) -> String {
        format!("⏩ {} seeked to {}", user, timecode)
    }

    /// Formats announcement when unpause is blocked waiting on peers:
    /// `"⏸ Paused: Waiting for {users} to load media and get ready..."`
    pub fn gate_waiting(users: &[String]) -> String {
        let joined = users.join(", ");
        format!("⏸ Paused: Waiting for {} to load media and get ready...", joined)
    }
}

/// Formats duration in milliseconds into a standard `HH:MM:SS` string.
pub fn format_duration_hms(duration_ms: u64) -> String {
    let total_secs = duration_ms / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    format!("{:02}:{:02}:{:02}", hours, mins, secs)
}

/// File match diagnostic badges matching ORIGINAL_REQUEST §R2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchBadge {
    /// `[Match ✔]` in green: duration matches within ±2000ms.
    Match,
    /// `[⚠️ Mismatch]` in amber/red: duration differs significantly.
    Mismatch,
    /// `[⏳ No file loaded]`: peer has no media file loaded.
    NoFileLoaded,
}

impl MatchBadge {
    /// Returns the UI display label for this badge.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Match => "[Match ✔]",
            Self::Mismatch => "[⚠️ Mismatch]",
            Self::NoFileLoaded => "[⏳ No file loaded]",
        }
    }
}

/// Evaluates file matching diagnostic badge between local and remote peer media.
///
/// Tolerance: exact duration match within ±2000ms (inclusive) yields `MatchBadge::Match`.
/// Anything greater than 2000ms delta yields `MatchBadge::Mismatch`.
/// If either peer has no media loaded, yields `MatchBadge::NoFileLoaded`.
pub fn evaluate_file_match(
    local_has_media: bool,
    local_duration_ms: u64,
    _local_filename: &str,
    peer_has_media: bool,
    peer_duration_ms: u64,
    _peer_filename: &str,
) -> MatchBadge {
    if !local_has_media || !peer_has_media {
        return MatchBadge::NoFileLoaded;
    }

    let delta = local_duration_ms.abs_diff(peer_duration_ms);

    if delta <= 2000 {
        MatchBadge::Match
    } else {
        MatchBadge::Mismatch
    }
}

/// Decision emitted by the strict playback unpause gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnpauseDecision {
    /// Playback is permitted to resume.
    Permitted,
    /// Playback is blocked because one or more peers are unready or have no media.
    Blocked {
        /// Names of peers blocking playback.
        unready_peers: Vec<String>,
        /// User-facing alert and announcement text.
        message: String,
    },
}

/// Evaluates whether an unpause attempt is permitted or blocked.
///
/// Follows ORIGINAL_REQUEST §R3:
/// - If `force_play_bypass` is active (host override), playback is ALWAYS permitted.
/// - Otherwise, if ANY connected peer has `!has_media` or `!is_ready`, playback is BLOCKED.
/// - If all peers have media and are ready (or 0 remote peers), playback is PERMITTED.
pub fn evaluate_unpause_gate(
    peers: &[(String, bool /* has_media */, bool /* is_ready */)],
    force_play_bypass: bool,
) -> UnpauseDecision {
    if force_play_bypass {
        return UnpauseDecision::Permitted;
    }

    let mut unready = Vec::new();
    for (username, has_media, is_ready) in peers {
        if !has_media || !is_ready {
            unready.push(username.clone());
        }
    }

    if unready.is_empty() {
        UnpauseDecision::Permitted
    } else {
        let msg = RoomAnnouncement::gate_waiting(&unready);
        UnpauseDecision::Blocked {
            unready_peers: unready,
            message: msg,
        }
    }
}

/// Announcement debouncer for timeline scrub seeking.
///
/// Prevents spamming the room chat during continuous 10Hz scrubber dragging.
/// Emits announcements only when scrubbing finishes or after a configured quiet window.
#[derive(Debug)]
#[allow(dead_code)]
pub struct SeekDebouncer {
    debounce_window_ms: u64,
    last_emit_pts_ms: Option<u64>,
    last_seek_time_ms: u64,
    pending_pts_ms: Option<u64>,
}

#[allow(dead_code)]
impl SeekDebouncer {
    /// Creates a new seek debouncer with the specified quiet window in ms (e.g. 500ms).
    pub fn new(debounce_window_ms: u64) -> Self {
        Self {
            debounce_window_ms,
            last_emit_pts_ms: None,
            last_seek_time_ms: 0,
            pending_pts_ms: None,
        }
    }

    /// Records an interactive drag seek event. Returns `Some(pts)` if an announcement should be emitted immediately,
    /// or `None` if the event is being held/debounced.
    pub fn on_drag_seek(&mut self, target_pts_ms: u64, now_ms: u64) -> Option<u64> {
        let is_first = self.pending_pts_ms.is_none()
            && (self.last_emit_pts_ms.is_none() || now_ms >= self.last_seek_time_ms + self.debounce_window_ms);
        self.pending_pts_ms = Some(target_pts_ms);
        self.last_seek_time_ms = now_ms;

        if is_first {
            // First seek in a sequence or seek after quiescence is emitted immediately
            self.last_emit_pts_ms = Some(target_pts_ms);
            self.pending_pts_ms = None;
            Some(target_pts_ms)
        } else {
            None
        }
    }

    /// Called when the user releases the scrubber handle or when time has passed.
    pub fn on_release(&mut self, _now_ms: u64) -> Option<u64> {
        if let Some(pending) = self.pending_pts_ms.take() {
            if Some(pending) != self.last_emit_pts_ms {
                self.last_emit_pts_ms = Some(pending);
                return Some(pending);
            }
        }
        None
    }

    /// Flushes any pending seek if the debounce window has elapsed.
    pub fn check_flush(&mut self, now_ms: u64) -> Option<u64> {
        if self.pending_pts_ms.is_some() && now_ms >= self.last_seek_time_ms + self.debounce_window_ms {
            let pending = self.pending_pts_ms.take().unwrap();
            if Some(pending) != self.last_emit_pts_ms {
                self.last_emit_pts_ms = Some(pending);
                return Some(pending);
            }
        }
        None
    }
}
