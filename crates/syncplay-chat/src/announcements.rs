//! Standardized room activity announcements conforming to ORIGINAL_REQUEST.md (R4 and R3).
//!
//! Provides deterministic formatting for all room events (media loading, readiness state,
//! playback pause/resume, seeking, and unpause gate waiting alerts), helper functions for
//! timecode formatting, and dual-temporal `ChatRecord` integration.

use crate::record::ChatRecord;
use std::fmt;

/// System sender UUID constant (`[0u8; 16]`) used for room announcements.
pub const SYSTEM_SENDER_ID: [u8; 16] = [0u8; 16];

/// System sender display name constant (`"System"`).
pub const SYSTEM_SENDER_NAME: &str = "System";

/// Formats millisecond presentation timestamps into standard timecode strings.
///
/// If `pts_ms >= 3,600,000` (1 hour or greater), produces `HH:MM:SS` (e.g. `"01:23:45"`).
/// If `pts_ms < 3,600,000` (under 1 hour), produces `MM:SS` (e.g. `"05:30"`).
///
/// Milliseconds are integer-truncated to whole seconds without rounding jitter.
#[inline]
pub fn format_timecode(pts_ms: u64) -> String {
    let total_secs = pts_ms / 1000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    }
}

/// Formats millisecond presentation timestamps strictly as `HH:MM:SS`.
///
/// Always outputs hours, minutes, and seconds with zero-padding (e.g. `"00:05:30"`).
#[inline]
pub fn format_timecode_hms(pts_ms: u64) -> String {
    let total_secs = pts_ms / 1000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}

/// Strongly typed representation of a room activity announcement event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomAnnouncementEvent {
    /// User loaded a media file.
    MediaLoaded {
        /// Display name of the user.
        user: String,
        /// Base filename of the loaded media.
        filename: String,
        /// Human-readable duration string (`HH:MM:SS` or `MM:SS`).
        duration: String,
    },
    /// User marked themselves as ready to watch.
    Ready {
        /// Display name of the ready user.
        user: String,
    },
    /// User marked themselves as not ready.
    NotReady {
        /// Display name of the unready user.
        user: String,
    },
    /// User paused playback.
    Paused {
        /// Display name of the user who paused.
        user: String,
    },
    /// User resumed playback.
    Resumed {
        /// Display name of the user who resumed.
        user: String,
    },
    /// User seeked to a new presentation timestamp.
    Seeked {
        /// Display name of the user who seeked.
        user: String,
        /// Formatted presentation timecode (`HH:MM:SS` or `MM:SS`).
        timecode: String,
    },
    /// Unpause gate is waiting for unready peers to load media or get ready.
    GateWaiting {
        /// List of usernames whose readiness is awaited.
        users: Vec<String>,
    },
}

impl RoomAnnouncementEvent {
    /// Formats the event into its standardized room announcement string.
    pub fn format_message(&self) -> String {
        match self {
            Self::MediaLoaded {
                user,
                filename,
                duration,
            } => RoomAnnouncement::media_loaded(user, filename, duration),
            Self::Ready { user } => RoomAnnouncement::ready(user),
            Self::NotReady { user } => RoomAnnouncement::not_ready(user),
            Self::Paused { user } => RoomAnnouncement::paused(user),
            Self::Resumed { user } => RoomAnnouncement::resumed(user),
            Self::Seeked { user, timecode } => RoomAnnouncement::seeked(user, timecode),
            Self::GateWaiting { users } => RoomAnnouncement::gate_waiting(users),
        }
    }
}

impl fmt::Display for RoomAnnouncementEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_message())
    }
}

/// Standardized room activity announcement generator conforming to ORIGINAL_REQUEST.md (R4 and R3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RoomAnnouncement;

impl RoomAnnouncement {
    /// Formats a media loaded announcement:
    /// `🎬 {user} loaded '{filename}' ({duration})`
    pub fn media_loaded(user: &str, filename: &str, duration: &str) -> String {
        format!("🎬 {} loaded '{}' ({})", user, filename, duration)
    }

    /// Formats a media loaded announcement using millisecond duration:
    /// `🎬 {user} loaded '{filename}' ({duration})`
    pub fn media_loaded_ms(user: &str, filename: &str, duration_ms: u64) -> String {
        Self::media_loaded(user, filename, &format_timecode(duration_ms))
    }

    /// Formats a readiness changed announcement when a user is ready:
    /// `✓ {user} is ready to watch`
    pub fn ready(user: &str) -> String {
        format!("✓ {} is ready to watch", user)
    }

    /// Formats a readiness changed announcement when a user is not ready:
    /// `⏳ {user} is not ready`
    pub fn not_ready(user: &str) -> String {
        format!("⏳ {} is not ready", user)
    }

    /// Formats a readiness announcement based on a boolean flag:
    /// `✓ {user} is ready to watch` if true, or `⏳ {user} is not ready` if false.
    pub fn readiness(user: &str, is_ready: bool) -> String {
        if is_ready {
            Self::ready(user)
        } else {
            Self::not_ready(user)
        }
    }

    /// Formats a playback paused announcement:
    /// `⏸ {user} paused playback`
    pub fn paused(user: &str) -> String {
        format!("⏸ {} paused playback", user)
    }

    /// Formats a playback resumed announcement:
    /// `▶ {user} resumed playback`
    pub fn resumed(user: &str) -> String {
        format!("▶ {} resumed playback", user)
    }

    /// Formats a seeking announcement:
    /// `⏩ {user} seeked to {timecode}`
    pub fn seeked(user: &str, timecode: &str) -> String {
        format!("⏩ {} seeked to {}", user, timecode)
    }

    /// Formats a seeking announcement using millisecond timestamp:
    /// `⏩ {user} seeked to {timecode}`
    pub fn seeked_ms(user: &str, pts_ms: u64) -> String {
        Self::seeked(user, &format_timecode(pts_ms))
    }

    /// Formats an unpause gate waiting alert:
    /// `⏸ Paused: Waiting for {users} to load media and get ready...`
    pub fn gate_waiting<S: AsRef<str>>(users: &[S]) -> String {
        if users.is_empty() {
            "⏸ Paused: Waiting for peers to load media and get ready...".to_string()
        } else {
            let user_list = users
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "⏸ Paused: Waiting for {} to load media and get ready...",
                user_list
            )
        }
    }

    /// Formats a standardized room announcement from a typed event.
    pub fn from_event(event: &RoomAnnouncementEvent) -> String {
        event.format_message()
    }

    /// Constructs a dual-temporal `ChatRecord` with system authorship for an announcement message.
    ///
    /// Uses `SYSTEM_SENDER_ID` (`[0u8; 16]`) and `SYSTEM_SENDER_NAME` (`"System"`).
    pub fn create_record(wall_ms: u64, media_ms: u64, text: &str) -> ChatRecord {
        ChatRecord::new_text(
            wall_ms,
            media_ms,
            SYSTEM_SENDER_ID,
            SYSTEM_SENDER_NAME.to_string(),
            text,
        )
    }

    /// Constructs a dual-temporal `ChatRecord` directly from a typed announcement event.
    pub fn create_record_from_event(
        wall_ms: u64,
        media_ms: u64,
        event: &RoomAnnouncementEvent,
    ) -> ChatRecord {
        Self::create_record(wall_ms, media_ms, &event.format_message())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::InvertedIndexFts;
    use crate::storage::{ChatLogStorage, ChatRecoveryScanner};
    use syncplay_common::Crc32;

    #[test]
    fn test_format_timecode_sub_hour() {
        assert_eq!(format_timecode(0), "00:00");
        assert_eq!(format_timecode(999), "00:00");
        assert_eq!(format_timecode(1_000), "00:01");
        assert_eq!(format_timecode(5_000), "00:05");
        assert_eq!(format_timecode(59_000), "00:59");
        assert_eq!(format_timecode(60_000), "01:00");
        assert_eq!(format_timecode(354_000), "05:54");
        assert_eq!(format_timecode(3_599_000), "59:59");
    }

    #[test]
    fn test_format_timecode_multi_hour() {
        assert_eq!(format_timecode(3_600_000), "01:00:00");
        assert_eq!(format_timecode(3_601_000), "01:00:01");
        assert_eq!(format_timecode(8_190_000), "02:16:30");
        assert_eq!(format_timecode(360_000_000), "100:00:00");
    }

    #[test]
    fn test_format_timecode_hms() {
        assert_eq!(format_timecode_hms(0), "00:00:00");
        assert_eq!(format_timecode_hms(5_000), "00:00:05");
        assert_eq!(format_timecode_hms(354_000), "00:05:54");
        assert_eq!(format_timecode_hms(8_190_000), "02:16:30");
    }

    #[test]
    fn test_announcements_formatting_all_types() {
        // 1. Media loaded
        let msg = RoomAnnouncement::media_loaded("Alice", "matrix.mkv", "02:16:30");
        assert_eq!(msg, "🎬 Alice loaded 'matrix.mkv' (02:16:30)");

        let msg_ms = RoomAnnouncement::media_loaded_ms("Alice", "matrix.mkv", 8_190_000);
        assert_eq!(msg_ms, "🎬 Alice loaded 'matrix.mkv' (02:16:30)");

        // 2. Readiness changed
        let msg_ready = RoomAnnouncement::ready("Alice");
        assert_eq!(msg_ready, "✓ Alice is ready to watch");

        let msg_not_ready = RoomAnnouncement::not_ready("Bob");
        assert_eq!(msg_not_ready, "⏳ Bob is not ready");

        assert_eq!(
            RoomAnnouncement::readiness("Alice", true),
            "✓ Alice is ready to watch"
        );
        assert_eq!(
            RoomAnnouncement::readiness("Bob", false),
            "⏳ Bob is not ready"
        );

        // 3. Playback paused
        let msg_paused = RoomAnnouncement::paused("Charlie");
        assert_eq!(msg_paused, "⏸ Charlie paused playback");

        // 4. Playback resumed
        let msg_resumed = RoomAnnouncement::resumed("Dave");
        assert_eq!(msg_resumed, "▶ Dave resumed playback");

        // 5. Seeking
        let msg_seeked = RoomAnnouncement::seeked("Eve", "01:23:45");
        assert_eq!(msg_seeked, "⏩ Eve seeked to 01:23:45");

        let msg_seeked_ms = RoomAnnouncement::seeked_ms("Eve", 83_000);
        assert_eq!(msg_seeked_ms, "⏩ Eve seeked to 01:23");

        // 6. Unpause gate waiting
        let empty_users: Vec<String> = vec![];
        let msg_gate_empty = RoomAnnouncement::gate_waiting(&empty_users);
        assert_eq!(
            msg_gate_empty,
            "⏸ Paused: Waiting for peers to load media and get ready..."
        );

        let single_user = vec!["Heidi".to_string()];
        let msg_gate_single = RoomAnnouncement::gate_waiting(&single_user);
        assert_eq!(
            msg_gate_single,
            "⏸ Paused: Waiting for Heidi to load media and get ready..."
        );

        let multi_users = vec!["Alice".to_string(), "Bob".to_string()];
        let msg_gate_multi = RoomAnnouncement::gate_waiting(&multi_users);
        assert_eq!(
            msg_gate_multi,
            "⏸ Paused: Waiting for Alice, Bob to load media and get ready..."
        );
    }

    #[test]
    fn test_room_announcement_event_enum_and_display() {
        let event = RoomAnnouncementEvent::MediaLoaded {
            user: "Frank".to_string(),
            filename: "episode_01.mkv".to_string(),
            duration: "24:15".to_string(),
        };
        assert_eq!(
            event.to_string(),
            "🎬 Frank loaded 'episode_01.mkv' (24:15)"
        );
        assert_eq!(
            RoomAnnouncement::from_event(&event),
            "🎬 Frank loaded 'episode_01.mkv' (24:15)"
        );

        let gate_event = RoomAnnouncementEvent::GateWaiting {
            users: vec!["Alice".to_string(), "Charlie".to_string()],
        };
        assert_eq!(
            gate_event.to_string(),
            "⏸ Paused: Waiting for Alice, Charlie to load media and get ready..."
        );
    }

    #[test]
    fn test_announcements_binary_persistence_framing_and_crc32() {
        let wall_ms = 1_700_000_000_000;
        let media_ms = 45_000;
        let announcement_text = RoomAnnouncement::media_loaded("Grace", "test.mp4", "10:00");

        let record = RoomAnnouncement::create_record(wall_ms, media_ms, &announcement_text);

        assert_eq!(record.record_type, 1);
        assert_eq!(record.sender_id, SYSTEM_SENDER_ID);
        assert_eq!(record.sender_name, SYSTEM_SENDER_NAME);
        assert_eq!(
            std::str::from_utf8(&record.payload).unwrap(),
            &announcement_text
        );

        // Serialize record with 0xAA55 framing and CRC32
        let wire_bytes = record.serialize_record();
        assert!(wire_bytes.len() >= 48);

        // Verify sync word 0xAA55
        assert_eq!(wire_bytes[0], 0xAA);
        assert_eq!(wire_bytes[1], 0x55);

        // Verify total length
        let total_len = u32::from_le_bytes([
            wire_bytes[2],
            wire_bytes[3],
            wire_bytes[4],
            wire_bytes[5],
        ]) as usize;
        assert_eq!(total_len, wire_bytes.len());

        // Verify CRC32
        let body = &wire_bytes[6..total_len - 4];
        let calc_crc = Crc32::digest(body);
        let stored_crc = u32::from_le_bytes([
            wire_bytes[total_len - 4],
            wire_bytes[total_len - 3],
            wire_bytes[total_len - 2],
            wire_bytes[total_len - 1],
        ]);
        assert_eq!(calc_crc, stored_crc);

        // Verify deserialization round-trip
        let parsed = ChatRecord::from_bytes(&wire_bytes).expect("valid record");
        assert_eq!(parsed.record_type, 1);
        assert_eq!(parsed.wall_timestamp_utc_ms, wall_ms);
        assert_eq!(parsed.media_timestamp_ms, media_ms);
        assert_eq!(parsed.sender_id, SYSTEM_SENDER_ID);
        assert_eq!(parsed.sender_name, "System");
        assert_eq!(
            std::str::from_utf8(&parsed.payload).unwrap(),
            &announcement_text
        );
    }

    #[test]
    fn test_announcements_storage_and_recovery_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!(
            "syncplay_chat_announcement_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let log_path = temp_dir.join("syncplay_chat.bin");

        let mut storage = ChatLogStorage::open(&log_path).expect("open log file");

        let announcements = [
            RoomAnnouncement::media_loaded("Alice", "movie.mkv", "02:00:00"),
            RoomAnnouncement::ready("Alice"),
            RoomAnnouncement::not_ready("Bob"),
            RoomAnnouncement::paused("Alice"),
            RoomAnnouncement::resumed("Alice"),
            RoomAnnouncement::seeked("Bob", "00:15:30"),
            RoomAnnouncement::gate_waiting(&["Bob".to_string()]),
        ];

        for (i, text) in announcements.iter().enumerate() {
            let record = RoomAnnouncement::create_record(
                1_000_000 + i as u64,
                (i as u64) * 10_000,
                text,
            );
            storage.append(&record).expect("append record");
        }

        // Recover all records using ChatRecoveryScanner
        let (recovered, stats) =
            ChatRecoveryScanner::recover_from_file(&log_path).expect("recover from file");

        assert_eq!(recovered.len(), 7);
        assert_eq!(stats.valid_records, 7);
        assert_eq!(stats.corrupted_chunks, 0);
        assert_eq!(stats.torn_writes, 0);

        for (i, record) in recovered.iter().enumerate() {
            assert_eq!(record.sender_name, "System");
            let text = std::str::from_utf8(&record.payload).unwrap();
            assert_eq!(text, &announcements[i]);
        }

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_announcements_inverted_index_full_text_search() {
        let mut index = InvertedIndexFts::new();

        let records = vec![
            RoomAnnouncement::create_record(
                100,
                1000,
                &RoomAnnouncement::media_loaded("Alice", "matrix.mkv", "02:16:30"),
            ),
            RoomAnnouncement::create_record(
                101,
                2000,
                &RoomAnnouncement::ready("Alice"),
            ),
            RoomAnnouncement::create_record(
                102,
                3000,
                &RoomAnnouncement::not_ready("Bob"),
            ),
            RoomAnnouncement::create_record(
                103,
                4000,
                &RoomAnnouncement::paused("Charlie"),
            ),
            RoomAnnouncement::create_record(
                104,
                5000,
                &RoomAnnouncement::resumed("Dave"),
            ),
            RoomAnnouncement::create_record(
                105,
                6000,
                &RoomAnnouncement::seeked("Eve", "01:23:45"),
            ),
            RoomAnnouncement::create_record(
                106,
                7000,
                &RoomAnnouncement::gate_waiting(&["Frank".to_string(), "Grace".to_string()]),
            ),
        ];

        for r in records {
            index.insert(r);
        }

        // Search for "loaded"
        let loaded_results = index.search("loaded");
        assert_eq!(loaded_results.len(), 1);
        assert!(std::str::from_utf8(&loaded_results[0].payload)
            .unwrap()
            .contains("loaded 'matrix.mkv'"));

        // Search for "ready" -> matches "ready to watch", "not ready", and "gate waiting (...get ready...)"
        let ready_results = index.search("ready");
        assert_eq!(ready_results.len(), 3);

        // Search for "paused" -> matches paused and gate waiting
        let paused_results = index.search("paused");
        assert_eq!(paused_results.len(), 2);

        // Search for "resumed"
        let resumed_results = index.search("resumed");
        assert_eq!(resumed_results.len(), 1);
        assert!(std::str::from_utf8(&resumed_results[0].payload)
            .unwrap()
            .contains("Dave resumed"));

        // Search for "seeked"
        let seeked_results = index.search("seeked");
        assert_eq!(seeked_results.len(), 1);
        assert!(std::str::from_utf8(&seeked_results[0].payload)
            .unwrap()
            .contains("Eve seeked"));

        // Search for "waiting"
        let waiting_results = index.search("waiting");
        assert_eq!(waiting_results.len(), 1);
        assert!(std::str::from_utf8(&waiting_results[0].payload)
            .unwrap()
            .contains("Waiting for Frank, Grace"));

        // Boolean multi-token search: "alice loaded"
        let multi_results = index.search("alice loaded");
        assert_eq!(multi_results.len(), 1);

        // Search for user name "charlie"
        let user_results = index.search("charlie");
        assert_eq!(user_results.len(), 1);
        assert!(std::str::from_utf8(&user_results[0].payload)
            .unwrap()
            .contains("Charlie paused"));
    }
}
