//! In-memory inverted index FTS with Unicode CJK tokenization and spatial timeline queries.

use std::collections::{BTreeMap, BTreeSet};
use crate::record::ChatRecord;

/// Standard English stop-words list excluded from Western token indexing.
pub const STOP_WORDS: &[&str] = &[
    "the", "is", "at", "which", "on", "and", "a", "an", "to", "for",
    "in", "of", "it", "this", "that", "with", "as", "by",
];

/// Returns true if a character belongs to the CJK Unified Ideographs block (U+4E00..U+9FFF).
#[inline]
pub fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x4E00..=0x9FFF)
}

/// Tokenizes text into search tokens adhering to the OpenSpec specification:
/// 1. Unicode case folding (including Turkish dotted I `İ` -> `i\u{307}` or `istanbul`).
/// 2. CJK character segmentation generating unigrams and overlapping bigrams.
/// 3. Western word segmentation preserving hashtags `#` and mentions `@`.
/// 4. Stop-word removal (Western tokens only) and token length bounding (1..48 for CJK, 2..48 for Western).
pub fn tokenize_spec(text: &str) -> Vec<String> {
    let folded = text.to_lowercase();
    let chars: Vec<char> = folded.chars().collect();
    let n = chars.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    let stop_words_set: BTreeSet<&str> = STOP_WORDS.iter().copied().collect();

    while i < n {
        let ch = chars[i];
        if is_cjk(ch) {
            let mut cjk_run = Vec::new();
            while i < n && is_cjk(chars[i]) {
                cjk_run.push(chars[i]);
                i += 1;
            }
            // 1. Unigrams (exempt from 2-character minimum)
            for &c in &cjk_run {
                tokens.push(c.to_string());
            }
            // 2. Overlapping bigrams
            if cjk_run.len() >= 2 {
                for j in 0..cjk_run.len() - 1 {
                    let mut bigram = String::with_capacity(8);
                    bigram.push(cjk_run[j]);
                    bigram.push(cjk_run[j + 1]);
                    tokens.push(bigram);
                }
            }
        } else if ch == '#' || ch == '@' || ch.is_alphanumeric() || ch == '_' {
            let mut word = String::new();
            while i < n && !is_cjk(chars[i]) && !chars[i].is_whitespace() {
                let c2 = chars[i];
                if c2 == '#' || c2 == '@' || c2.is_alphanumeric() || c2 == '_' {
                    word.push(c2);
                }
                i += 1;
            }
            if !word.is_empty() {
                let char_len = word.chars().count();
                if (2..=48).contains(&char_len) && !stop_words_set.contains(word.as_str()) {
                    tokens.push(word);
                }
            }
        } else {
            i += 1;
        }
    }

    tokens
}

/// In-memory inverted index supporting full-text Boolean AND search and temporal spatial queries.
#[derive(Debug, Default)]
pub struct InvertedIndexFts {
    records: Vec<ChatRecord>,
    /// Posting lists: token -> list of record indices
    postings: BTreeMap<String, Vec<usize>>,
    /// Spatial timeline index: media PTS ms -> list of record indices
    timeline: BTreeMap<u64, Vec<usize>>,
}

impl InvertedIndexFts {
    /// Constructs a new empty `InvertedIndexFts`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Indexes a chat record into both the inverted FTS index and spatial timeline index.
    pub fn insert(&mut self, record: ChatRecord) -> usize {
        let idx = self.records.len();

        // 1. Index full-text tokens if text message
        if let Ok(text) = std::str::from_utf8(&record.payload) {
            let tokens = tokenize_spec(text);
            let mut unique_tokens = BTreeSet::new();
            for token in tokens {
                if unique_tokens.insert(token.clone()) {
                    self.postings.entry(token).or_default().push(idx);
                }
            }
        }

        // 2. Index timeline position if anchored
        if record.media_timestamp_ms != u64::MAX {
            self.timeline
                .entry(record.media_timestamp_ms)
                .or_default()
                .push(idx);
        }

        self.records.push(record);
        idx
    }

    /// Executes a multi-term Boolean AND search across posting lists.
    ///
    /// Returns matching chat records in chronological order.
    pub fn search(&self, query: &str) -> Vec<&ChatRecord> {
        let query_tokens = tokenize_spec(query);
        if query_tokens.is_empty() {
            return Vec::new();
        }

        let mut matching_indices: Option<Vec<usize>> = None;

        for term in query_tokens {
            let term_postings = match self.postings.get(&term) {
                Some(p) => p,
                None => return Vec::new(), // Term not found anywhere -> AND query yields empty
            };

            matching_indices = match matching_indices {
                None => Some(term_postings.clone()),
                Some(current) => {
                    // Linear two-pointer intersection in O(|L1| + |L2|)
                    let mut intersected = Vec::new();
                    let mut i = 0;
                    let mut j = 0;
                    while i < current.len() && j < term_postings.len() {
                        if current[i] == term_postings[j] {
                            intersected.push(current[i]);
                            i += 1;
                            j += 1;
                        } else if current[i] < term_postings[j] {
                            i += 1;
                        } else {
                            j += 1;
                        }
                    }
                    Some(intersected)
                }
            };
        }

        matching_indices
            .unwrap_or_default()
            .iter()
            .filter_map(|&idx| {
                let rec = &self.records[idx];
                if rec.flags & 1 == 0 {
                    Some(rec)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Queries records anchored within a temporal window `[current_pts - past_ms, current_pts + future_ms]`.
    ///
    /// Executes in $O(\log N + K)$ time over `BTreeMap`.
    pub fn query_timeline_window(
        &self,
        current_pts: u64,
        past_ms: u64,
        future_ms: u64,
    ) -> Vec<&ChatRecord> {
        let start = current_pts.saturating_sub(past_ms);
        let end = current_pts.saturating_add(future_ms);

        let mut results = Vec::new();
        for (_, indices) in self.timeline.range(start..=end) {
            for &idx in indices {
                let rec = &self.records[idx];
                if rec.flags & 1 == 0 {
                    // Exclude tombstoned/deleted records
                    results.push(rec);
                }
            }
        }
        results
    }

    /// Returns a slice of all stored records.
    pub fn all_records(&self) -> &[ChatRecord] {
        &self.records
    }

    /// Queries records anchored within a temporal range `[current_pts - past_ms, current_pts + future_ms]`.
    pub fn query_timeline_range(
        &self,
        current_pts: u64,
        past_ms: u64,
        future_ms: u64,
    ) -> Vec<&ChatRecord> {
        self.query_timeline_window(current_pts, past_ms, future_ms)
    }

    /// Returns the total number of records indexed.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns true if the index contains zero records.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Returns a reference to the record at the given index.
    pub fn record_at(&self, idx: usize) -> Option<&ChatRecord> {
        self.records.get(idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fts_english_tokenization() {
        let text = "Check out that awesome guitar solo at 14:30! #riff @alice";
        let tokens = tokenize_spec(text);
        assert!(tokens.contains(&"guitar".to_string()));
        assert!(tokens.contains(&"awesome".to_string()));
        assert!(tokens.contains(&"#riff".to_string()));
        assert!(tokens.contains(&"@alice".to_string()));
        assert!(!tokens.contains(&"that".to_string())); // Stop-word
    }

    #[test]
    fn test_fts_cjk_unigram_and_bigram() {
        let text = "这是一段非常精彩的吉他独奏";
        let tokens = tokenize_spec(text);
        assert!(tokens.contains(&"吉".to_string()));
        assert!(tokens.contains(&"他".to_string()));
        assert!(tokens.contains(&"吉他".to_string()));

        let single_cjk = "猫 狗";
        let single_tokens = tokenize_spec(single_cjk);
        assert!(single_tokens.contains(&"猫".to_string()));
        assert!(single_tokens.contains(&"狗".to_string()));
    }

    #[test]
    fn test_fts_turkish_case_folding() {
        let text_tr = "İSTANBUL";
        let tokens_tr = tokenize_spec(text_tr);
        assert_eq!(tokens_tr, vec!["istanbul".to_string()]);
    }

    #[test]
    fn test_inverted_index_boolean_and_search() {
        let mut index = InvertedIndexFts::new();
        let sid = [0u8; 16];

        index.insert(ChatRecord::new_text(
            1000, 10000, sid, "A".into(),
            "Check out that awesome guitar solo at 14:30!",
        ));
        index.insert(ChatRecord::new_text(
            1001, 12500, sid, "B".into(),
            "The guitar tone is okay, but the bass is awesome.",
        ));
        index.insert(ChatRecord::new_text(
            1002, 18000, sid, "C".into(),
            "Check out the drums at 20:00.",
        ));

        // Boolean AND for "guitar awesome"
        let results = index.search("guitar awesome");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].sender_name, "A");
        assert_eq!(results[1].sender_name, "B");

        // Spatial query at current_pts = 12000, past = 3000, future = 1000 -> [9000, 13000]
        let overlay = index.query_timeline_window(12000, 3000, 1000);
        assert_eq!(overlay.len(), 2); // Rec A (10000) and Rec B (12500)
    }
}
