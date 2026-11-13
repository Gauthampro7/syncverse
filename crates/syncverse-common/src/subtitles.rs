//! Subtitle data models, cue definitions, and track representations for synchronized media playback.

use std::sync::Arc;

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

/// Decoded rasterized RGBA subtitle bitmap (e.g., DVD VobSub, PGS, or DVB bitmap subtitle).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubtitleBitmap {
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

/// A timed subtitle cue with presentation start/end bounds and text or bitmap content.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubtitleCue {
    /// Media presentation timestamp at which this cue becomes active.
    pub start: MediaTime,
    /// Media presentation timestamp at which this cue expires.
    pub end: MediaTime,
    /// Display text for this cue (formatting tags stripped or normalized).
    pub text: String,
    /// Optional rasterized RGBA bitmap payload for image-based subtitles (PGS, VobSub, DVB).
    pub bitmap: Option<SubtitleBitmap>,
}

impl SubtitleCue {
    /// Constructs a new text subtitle cue with specified start time, end time, and text.
    pub fn new(start: MediaTime, end: MediaTime, text: impl Into<String>) -> Self {
        Self {
            start,
            end,
            text: text.into(),
            bitmap: None,
        }
    }

    /// Constructs a new bitmap subtitle cue with specified start time, end time, and RGBA bitmap.
    pub fn new_bitmap(start: MediaTime, end: MediaTime, bitmap: SubtitleBitmap) -> Self {
        Self {
            start,
            end,
            text: String::new(),
            bitmap: Some(bitmap),
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

/// Inserts a cue chronologically by `start` into a sorted `Vec<SubtitleCue>`, skipping exact duplicates.
///
/// Two cues are considered duplicates if `(start, end, text, bitmap.is_some())` all match.
/// Returns `true` if the cue was inserted, or `false` if a duplicate already existed.
pub fn insert_cue_dedup(cues: &mut Vec<SubtitleCue>, cue: SubtitleCue) -> bool {
    let lower = cues.partition_point(|c| c.start < cue.start);
    let upper = cues.partition_point(|c| c.start <= cue.start);

    for existing in &cues[lower..upper] {
        if existing.start == cue.start
            && existing.end == cue.end
            && existing.text == cue.text
            && existing.bitmap.is_some() == cue.bitmap.is_some()
        {
            return false;
        }
    }

    cues.insert(upper, cue);
    true
}

/// Truncates any currently active cues at `clear_time` by clamping `c.end = clear_time`.
///
/// Handles explicit subtitle clear packets (such as MP4 `mov_text` `[0x00, 0x00]` or PGS/DVD
/// zero-rect clear frames) for cues satisfying `c.start < clear_time && c.end > clear_time`.
pub fn truncate_active_cues_at(cues: &mut [SubtitleCue], clear_time: MediaTime) {
    for cue in cues.iter_mut() {
        if cue.start < clear_time && cue.end > clear_time {
            cue.end = clear_time;
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

    /// Inserts a cue chronologically by `start`, skipping exact duplicates.
    ///
    /// Two cues are duplicates when `(start, end, text, bitmap.is_some())` match, preventing
    /// backward seeks from stacking duplicate lines.
    pub fn insert_cue_dedup(&mut self, cue: SubtitleCue) -> bool {
        insert_cue_dedup(&mut self.cues, cue)
    }

    /// Truncates any currently active cues at `clear_time` by clamping `c.end = clear_time`.
    pub fn truncate_active_cues_at(&mut self, clear_time: MediaTime) {
        truncate_active_cues_at(&mut self.cues, clear_time);
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
        let upper = self.cues.partition_point(|c| c.start <= time);
        if upper == 0 {
            return None;
        }
        let pos = upper - 1;

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

        let upper = self.cues.partition_point(|c| c.start <= time);
        if upper == 0 {
            return Vec::new();
        }
        let pos = upper - 1;

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

    /// Returns combined active subtitle text at the specified media timestamp, if any non-empty
    /// text cues are active.
    pub fn active_text_at(&self, time: MediaTime) -> Option<String> {
        let active: Vec<&str> = self
            .active_cues_at(time)
            .into_iter()
            .filter(|c| !c.text.is_empty())
            .map(|c| c.text.as_str())
            .collect();
        if active.is_empty() {
            None
        } else {
            Some(active.join("\n"))
        }
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

/// Candidate subtitle track metadata used for VLC-style initial track auto-selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleTrackCandidate {
    /// Index of this track in the playback session's subtitle track list.
    pub index: usize,
    /// Whether this track originates from an external sidecar file.
    pub is_external: bool,
    /// Whether the track is flagged as forced subtitles.
    pub is_forced: bool,
    /// Whether the track is flagged as default by the container/stream.
    pub is_default: bool,
    /// Optional ISO 639-1 / ISO 639-2 language code (e.g., `"en"`, `"eng"`, `"ja"`).
    pub language: Option<String>,
    /// Whether an external sidecar filename stem exactly matches the media filename stem.
    pub exact_stem_match: bool,
}

/// Normalizes common ISO 639-1 / ISO 639-2 language codes to a canonical 2-letter tag.
fn normalize_lang_tag(lang: &str) -> String {
    let base = lang
        .trim()
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match base.as_str() {
        "en" | "eng" | "english" => "en".to_string(),
        "es" | "spa" | "spanish" => "es".to_string(),
        "fr" | "fra" | "fre" | "french" => "fr".to_string(),
        "de" | "deu" | "ger" | "german" => "de".to_string(),
        "it" | "ita" | "italian" => "it".to_string(),
        "pt" | "por" | "portuguese" => "pt".to_string(),
        "ja" | "jpn" | "japanese" => "ja".to_string(),
        "ko" | "kor" | "korean" => "ko".to_string(),
        "zh" | "zho" | "chi" | "chinese" => "zh".to_string(),
        "ru" | "rus" | "russian" => "ru".to_string(),
        "ar" | "ara" | "arabic" => "ar".to_string(),
        "hi" | "hin" | "hindi" => "hi".to_string(),
        "nl" | "nld" | "dut" | "dutch" => "nl".to_string(),
        "pl" | "pol" | "polish" => "pl".to_string(),
        "sv" | "swe" | "swedish" => "sv".to_string(),
        "da" | "dan" | "danish" => "da".to_string(),
        "fi" | "fin" | "finnish" => "fi".to_string(),
        "no" | "nor" | "norwegian" => "no".to_string(),
        "tr" | "tur" | "turkish" => "tr".to_string(),
        "el" | "ell" | "gre" | "greek" => "el".to_string(),
        "cs" | "ces" | "cze" | "czech" => "cs".to_string(),
        "hu" | "hun" | "hungarian" => "hu".to_string(),
        "uk" | "ukr" | "ukrainian" => "uk".to_string(),
        "vi" | "vie" | "vietnamese" => "vi".to_string(),
        "id" | "ind" | "indonesian" => "id".to_string(),
        "th" | "tha" | "thai" => "th".to_string(),
        "he" | "heb" | "hebrew" => "he".to_string(),
        "ro" | "ron" | "rum" | "romanian" => "ro".to_string(),
        "fa" | "fas" | "per" | "persian" => "fa".to_string(),
        _ => base,
    }
}

fn candidate_matches_lang(candidate: &SubtitleTrackCandidate, preferred_lang: &str) -> bool {
    let Some(ref lang) = candidate.language else {
        return false;
    };
    let pref_norm = normalize_lang_tag(preferred_lang);
    let cand_norm = normalize_lang_tag(lang);
    !pref_norm.is_empty() && pref_norm == cand_norm
}

/// Selects the initial subtitle track index using VLC-style priority ranking.
///
/// Priority order:
/// 1. External exact stem match (preferred language first)
/// 2. Any external sidecar (preferred language first)
/// 3. Embedded `is_forced` track (preferred language first)
/// 4. Embedded `is_default` track (preferred language first)
///
/// Returns `None` if no candidate satisfies any of the four priority tiers.
pub fn select_initial_subtitle_track(
    candidates: &[SubtitleTrackCandidate],
    preferred_lang: Option<&str>,
) -> Option<usize> {
    let pick_from_tier = |predicate: &dyn Fn(&SubtitleTrackCandidate) -> bool| -> Option<usize> {
        if let Some(pref) = preferred_lang {
            if let Some(found) = candidates
                .iter()
                .find(|c| predicate(c) && candidate_matches_lang(c, pref))
            {
                return Some(found.index);
            }
        }
        candidates.iter().find(|c| predicate(c)).map(|c| c.index)
    };

    pick_from_tier(&|c| c.is_external && c.exact_stem_match)
        .or_else(|| pick_from_tier(&|c| c.is_external))
        .or_else(|| pick_from_tier(&|c| !c.is_external && c.is_forced))
        .or_else(|| pick_from_tier(&|c| !c.is_external && c.is_default))
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
        assert!(cue.bitmap.is_none());
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
        assert_eq!(
            track.active_text_at(MediaTime::from_millis(1500)).as_deref(),
            Some("First")
        );
        assert_eq!(track.active_cue_at(MediaTime::from_millis(2500)), None);
        assert_eq!(
            track.active_cue_at(MediaTime::from_millis(4000)).map(|c| c.text.as_str()),
            Some("Second")
        );
    }

    #[test]
    fn test_cue_dedup_and_clear_truncation() {
        let mut track = SubtitleTrack::new(
            0,
            SubtitleSource::Embedded,
            "mov_text",
            Some("en".to_string()),
            "English",
            Vec::new(),
        );

        let c1 = SubtitleCue::new(
            MediaTime::from_millis(1000),
            MediaTime::from_millis(5000),
            "Line A",
        );
        let c2 = SubtitleCue::new(
            MediaTime::from_millis(2000),
            MediaTime::from_millis(6000),
            "Line B",
        );
        let bmp = SubtitleBitmap {
            x: 10,
            y: 20,
            width: 2,
            height: 2,
            canvas_width: 1920,
            canvas_height: 1080,
            rgba: Arc::from([255u8; 16]),
        };
        let c_bmp = SubtitleCue::new_bitmap(
            MediaTime::from_millis(1000),
            MediaTime::from_millis(5000),
            bmp.clone(),
        );

        // Insert out-of-order and repeat (simulating backward seek)
        assert!(track.insert_cue_dedup(c2.clone()));
        assert!(track.insert_cue_dedup(c1.clone()));
        assert!(!track.insert_cue_dedup(c1.clone()));
        assert!(!track.insert_cue_dedup(c2));
        // Bitmap cue at same start/end is distinct because bitmap.is_some() != text cue
        assert!(track.insert_cue_dedup(c_bmp.clone()));
        assert!(!track.insert_cue_dedup(c_bmp));

        assert_eq!(track.cues.len(), 3);
        assert_eq!(track.cues[0].start, MediaTime::from_millis(1000));
        assert_eq!(track.cues[2].start, MediaTime::from_millis(2000));

        // Clear packet at 2500ms should clamp all cues spanning across 2500ms
        track.truncate_active_cues_at(MediaTime::from_millis(2500));
        for cue in &track.cues {
            assert_eq!(cue.end, MediaTime::from_millis(2500));
        }

        // Clear packet before cue start should not touch cue
        track.truncate_active_cues_at(MediaTime::from_millis(500));
        assert_eq!(track.cues[0].end, MediaTime::from_millis(2500));
    }

    #[test]
    fn test_select_initial_subtitle_track_priority() {
        let candidates = vec![
            SubtitleTrackCandidate {
                index: 0,
                is_external: false,
                is_forced: false,
                is_default: true,
                language: Some("en".to_string()),
                exact_stem_match: false,
            },
            SubtitleTrackCandidate {
                index: 1,
                is_external: false,
                is_forced: true,
                is_default: false,
                language: Some("en".to_string()),
                exact_stem_match: false,
            },
            SubtitleTrackCandidate {
                index: 2,
                is_external: true,
                is_forced: false,
                is_default: false,
                language: Some("fr".to_string()),
                exact_stem_match: false,
            },
            SubtitleTrackCandidate {
                index: 3,
                is_external: true,
                is_forced: false,
                is_default: false,
                language: Some("eng".to_string()),
                exact_stem_match: true,
            },
        ];

        // Tier 1: external exact stem match wins
        assert_eq!(select_initial_subtitle_track(&candidates, Some("en")), Some(3));

        // Without exact stem match: Tier 2 (any external), preferred language first
        let mut ext_candidates = candidates[..3].to_vec();
        ext_candidates.push(SubtitleTrackCandidate {
            index: 4,
            is_external: true,
            is_forced: false,
            is_default: false,
            language: Some("en-US".to_string()),
            exact_stem_match: false,
        });
        assert_eq!(select_initial_subtitle_track(&ext_candidates, Some("en")), Some(4));
        assert_eq!(select_initial_subtitle_track(&ext_candidates, None), Some(2));

        // Embedded only: Tier 3 (is_forced) beats Tier 4 (is_default)
        assert_eq!(select_initial_subtitle_track(&candidates[..2], Some("en")), Some(1));

        // Embedded default only: Tier 4
        assert_eq!(select_initial_subtitle_track(&candidates[..1], Some("ja")), Some(0));

        // Non-forced, non-default embedded tracks only: None
        let plain_embedded = vec![SubtitleTrackCandidate {
            index: 0,
            is_external: false,
            is_forced: false,
            is_default: false,
            language: Some("en".to_string()),
            exact_stem_match: false,
        }];
        assert_eq!(select_initial_subtitle_track(&plain_embedded, Some("en")), None);
    }
}
