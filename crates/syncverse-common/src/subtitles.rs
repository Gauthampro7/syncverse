//! Subtitle data models, cue definitions, and track representations for synchronized media playback.

use crate::time::MediaTime;

/// Origin source of a subtitle track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SubtitleSource {
    /// Subtitle stream embedded within a multimedia container (e.g., Matroska, MP4).
    #[default]
    Embedded,
    /// Subtitle track loaded from an external file (e.g., .srt, .vtt, .ass).
    External,
}

impl std::fmt::Display for SubtitleSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Embedded => write!(f, "Embedded"),
            Self::External => write!(f, "External"),
        }
    }
}

/// A timed subtitle cue with presentation start/end bounds and text content.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubtitleCue {
    /// Media presentation timestamp at which this cue becomes active.
    pub start: MediaTime,
    /// Media presentation timestamp at which this cue expires.
    pub end: MediaTime,
    /// Display text for this cue (formatting tags stripped or normalized).
    pub text: String,
}

impl SubtitleCue {
    /// Constructs a new subtitle cue with specified start time, end time, and text.
    pub fn new(start: MediaTime, end: MediaTime, text: impl Into<String>) -> Self {
        Self {
            start,
            end,
            text: text.into(),
        }
    }

    /// Returns `true` if the cue is active at the given media presentation time.
    ///
    /// A cue is active when `start <= time <= end`.
    #[inline]
    pub fn is_active_at(&self, time: MediaTime) -> bool {
        time >= self.start && time <= self.end
    }

    /// Duration of the cue in milliseconds.
    #[inline]
    pub fn duration_ms(&self) -> u64 {
        if self.end >= self.start {
            self.end.as_millis().saturating_sub(self.start.as_millis())
        } else {
            0
        }
    }
}

/// A subtitle track containing track metadata and a sorted collection of timed cues.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleTrack {
    /// Unique numeric identifier assigned to this track within the playback session.
    pub id: usize,
    /// Track origin source (embedded within media container vs. external file).
    pub source: SubtitleSource,
    /// Format descriptor string (e.g., "srt", "vtt", "ass", "subrip").
    pub format: String,
    /// Optional ISO 639-1 / ISO 639-2 language code (e.g., "en", "fra", "jpn").
    pub language: Option<String>,
    /// Human-readable title or description (e.g., "English (SDH)", "Director's Commentary").
    pub title: String,
    /// Chronologically sorted vector of subtitle cues.
    pub cues: Vec<SubtitleCue>,
}

impl SubtitleTrack {
    /// Constructs a new subtitle track with the given parameters and cues.
    pub fn new(
        id: usize,
        source: SubtitleSource,
        format: impl Into<String>,
        language: Option<String>,
        title: impl Into<String>,
        cues: Vec<SubtitleCue>,
    ) -> Self {
        Self {
            id,
            source,
            format: format.into(),
            language,
            title: title.into(),
            cues,
        }
    }

    /// Finds the active subtitle cue at the specified media timestamp, if any.
    ///
    /// Performs an efficient binary search over sorted cue start times. If multiple
    /// cues overlap at `time`, the most relevant active cue is returned.
    pub fn active_cue_at(&self, time: MediaTime) -> Option<&SubtitleCue> {
        if self.cues.is_empty() {
            return None;
        }

        // Binary search to find the highest index where cue.start <= time
        let pos = match self.cues.binary_search_by(|c| c.start.cmp(&time)) {
            Ok(idx) => idx,
            Err(idx) => {
                if idx == 0 {
                    return None;
                }
                idx - 1
            }
        };

        // Check backwards from pos for any cue that is active at `time`
        // (handles overlapping cues or cues with long durations)
        for i in (0..=pos).rev() {
            let cue = &self.cues[i];
            if cue.is_active_at(time) {
                return Some(cue);
            }
            // Stop scanning back if we're well past standard cue duration windows
            if pos.saturating_sub(i) > 32 {
                break;
            }
        }

        None
    }

    /// Returns all subtitle cues active at the specified media timestamp.
    pub fn active_cues_at(&self, time: MediaTime) -> Vec<&SubtitleCue> {
        if self.cues.is_empty() {
            return Vec::new();
        }

        let pos = match self.cues.binary_search_by(|c| c.start.cmp(&time)) {
            Ok(idx) => idx,
            Err(idx) => {
                if idx == 0 {
                    return Vec::new();
                }
                idx - 1
            }
        };

        let mut active = Vec::new();
        for i in (0..=pos).rev() {
            let cue = &self.cues[i];
            if cue.is_active_at(time) {
                active.push(cue);
            }
            if pos.saturating_sub(i) > 32 {
                break;
            }
        }
        active.reverse();
        active
    }

    /// Generates a human-friendly display label for UI menus and HUD notifications.
    pub fn display_label(&self) -> String {
        let title_clean = self.title.trim();
        if !title_clean.is_empty() {
            if let Some(ref lang) = self.language {
                if !title_clean.to_lowercase().contains(&lang.to_lowercase()) {
                    return format!("{title_clean} [{lang}]");
                }
            }
            return title_clean.to_string();
        }

        if let Some(ref lang) = self.language {
            return format!("Subtitle ({lang})");
        }

        format!("Track {}", self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subtitle_cue_active_and_duration() {
        let cue = SubtitleCue::new(
            MediaTime::from_millis(1000),
            MediaTime::from_millis(3500),
            "Hello world",
        );
        assert_eq!(cue.duration_ms(), 2500);
        assert!(!cue.is_active_at(MediaTime::from_millis(999)));
        assert!(cue.is_active_at(MediaTime::from_millis(1000)));
        assert!(cue.is_active_at(MediaTime::from_millis(2000)));
        assert!(cue.is_active_at(MediaTime::from_millis(3500)));
        assert!(!cue.is_active_at(MediaTime::from_millis(3501)));
    }

    #[test]
    fn test_subtitle_track_search_and_labels() {
        let cues = vec![
            SubtitleCue::new(MediaTime::from_millis(1000), MediaTime::from_millis(2000), "First"),
            SubtitleCue::new(MediaTime::from_millis(3000), MediaTime::from_millis(5000), "Second"),
            SubtitleCue::new(MediaTime::from_millis(6000), MediaTime::from_millis(8000), "Third"),
        ];

        let track = SubtitleTrack::new(
            1,
            SubtitleSource::External,
            "srt",
            Some("en".to_string()),
            "English",
            cues,
        );

        assert_eq!(track.display_label(), "English");
        assert_eq!(track.active_cue_at(MediaTime::from_millis(500)), None);
        assert_eq!(
            track.active_cue_at(MediaTime::from_millis(1500)).map(|c| c.text.as_str()),
            Some("First")
        );
        assert_eq!(track.active_cue_at(MediaTime::from_millis(2500)), None);
        assert_eq!(
            track.active_cue_at(MediaTime::from_millis(4000)).map(|c| c.text.as_str()),
            Some("Second")
        );
    }
}
