//! Unified chat store providing persistent logging, in-memory indexing, search, and slash commands.

use std::path::Path;

use crate::commands::{DispatchAction, SlashCommandDispatcher};
use crate::error::ChatError;
use crate::export::{export_history_json, export_transcript_markdown};
use crate::index::{tokenize_spec, InvertedIndexFts};
use crate::record::ChatRecord;
use crate::storage::{ChatLogStorage, RecoveryStats};

/// Result item produced by a full-text search query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    /// 0-based index of the matching record.
    pub record_index: usize,
    /// Matching chat record.
    pub record: ChatRecord,
    /// Specific terms from the search query that matched.
    pub matched_terms: Vec<String>,
}

/// Trait abstraction defining the interface for chat store backends.
pub trait ChatStoreTrait: Send + Sync {
    /// Appends a new chat record.
    fn append(&mut self, record: &ChatRecord) -> Result<u64, ChatError>;
    /// Searches stored records for a query string.
    fn search(&self, query: &str) -> Vec<SearchResult>;
    /// Queries records in a timeline window around `media_ts`.
    fn query_timeline_window(&self, media_ts: u64, window_ms: u64) -> Vec<ChatRecord>;
    /// Exports conversation history to JSON string.
    fn export_history_json(&self) -> String;
}

/// Unified chat store binding crash-resilient file persistence, inverted index FTS,
/// and interactive slash commands.
#[derive(Debug, Default)]
pub struct ChatStore {
    storage: Option<ChatLogStorage>,
    index: InvertedIndexFts,
    dispatcher: SlashCommandDispatcher,
    recovery_stats: Option<RecoveryStats>,
}

impl ChatStore {
    /// Creates a new in-memory chat store without disk persistence.
    pub fn new_in_memory() -> Self {
        Self::default()
    }

    /// Opens or creates a binary log file at `path`, recovers all valid records,
    /// indexes them into memory, and prepares the store for appending.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ChatError> {
        let mut storage = ChatLogStorage::open(path)?;
        let (records, stats) = storage.recover()?;

        let mut index = InvertedIndexFts::new();
        for rec in records {
            index.insert(rec);
        }

        Ok(Self {
            storage: Some(storage),
            index,
            dispatcher: SlashCommandDispatcher::new(),
            recovery_stats: Some(stats),
        })
    }

    /// Appends a new chat record to the persistent log (if open) and indexes it in memory.
    ///
    /// Returns the byte offset in the log file, or the record index for in-memory stores.
    pub fn append(&mut self, record: &ChatRecord) -> Result<u64, ChatError> {
        let pos = match self.storage.as_mut() {
            Some(storage) => storage.append(record)?,
            None => self.index.len() as u64,
        };
        self.index.insert(record.clone());
        Ok(pos)
    }

    /// Executes a multi-term Boolean AND search across the full-text inverted index.
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        let terms = tokenize_spec(query);
        let matching_records = self.index.search(query);

        matching_records
            .into_iter()
            .enumerate()
            .map(|(idx, rec)| SearchResult {
                record_index: idx,
                record: rec.clone(),
                matched_terms: terms.clone(),
            })
            .collect()
    }

    /// Searches for records matching `query` and returns direct references.
    pub fn search_records(&self, query: &str) -> Vec<&ChatRecord> {
        self.index.search(query)
    }

    /// Queries all records anchored within `[media_ts - window_ms, media_ts + window_ms]`.
    pub fn query_timeline_window(&self, media_ts: u64, window_ms: u64) -> Vec<ChatRecord> {
        self.index
            .query_timeline_window(media_ts, window_ms, window_ms)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Queries records anchored within asymmetric window `[current_pts - past_ms, current_pts + future_ms]`.
    pub fn query_timeline_range(
        &self,
        current_pts: u64,
        past_ms: u64,
        future_ms: u64,
    ) -> Vec<ChatRecord> {
        self.index
            .query_timeline_window(current_pts, past_ms, future_ms)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Dispatches a raw slash-command string through the internal dispatcher.
    pub fn dispatch_command(
        &mut self,
        input: &str,
        current_pts_ms: Option<u64>,
    ) -> Result<DispatchAction, ChatError> {
        self.dispatcher.dispatch(input, current_pts_ms)
    }

    /// Exports stored messages in JSON format.
    pub fn export_history_json(&self) -> String {
        export_history_json(self.records())
    }

    /// Exports stored messages in Markdown transcript format.
    pub fn export_transcript_markdown(&self) -> String {
        export_transcript_markdown(self.records())
    }

    /// Returns a slice of all stored records.
    pub fn records(&self) -> &[ChatRecord] {
        self.index.all_records()
    }

    /// Returns the total number of records stored.
    pub fn record_count(&self) -> usize {
        self.index.len()
    }

    /// Returns a reference to the internal slash-command dispatcher.
    pub fn dispatcher(&self) -> &SlashCommandDispatcher {
        &self.dispatcher
    }

    /// Returns a mutable reference to the internal slash-command dispatcher.
    pub fn dispatcher_mut(&mut self) -> &mut SlashCommandDispatcher {
        &mut self.dispatcher
    }

    /// Returns recovery statistics if the store was opened from a file.
    pub fn recovery_stats(&self) -> Option<RecoveryStats> {
        self.recovery_stats
    }

    /// Returns a reference to the underlying inverted index.
    pub fn index(&self) -> &InvertedIndexFts {
        &self.index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_chat_store() {
        let mut store = ChatStore::new_in_memory();
        let sid = [7u8; 16];

        let r1 = ChatRecord::new_text(1000, 10000, sid, "User1".into(), "Intro Scene #cool");
        let r2 = ChatRecord::new_text(1010, 20000, sid, "User2".into(), "Awesome Guitar Solo #cool");

        store.append(&r1).unwrap();
        store.append(&r2).unwrap();

        assert_eq!(store.record_count(), 2);

        // Search
        let results = store.search("#cool");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].record.sender_name, "User1");

        let results_guitar = store.search("guitar");
        assert_eq!(results_guitar.len(), 1);
        assert_eq!(results_guitar[0].record.sender_name, "User2");

        // Timeline window: around 10000 with 2000 window -> [8000, 12000]
        let window = store.query_timeline_window(10000, 2000);
        assert_eq!(window.len(), 1);
        assert_eq!(window[0].sender_name, "User1");

        // Slash command
        let action = store.dispatch_command("/seek 15:00", None).unwrap();
        assert_eq!(action, DispatchAction::Seek { target_pts_ms: 900000 });

        // Export
        let md = store.export_transcript_markdown();
        assert!(md.contains("User1"));
        assert!(md.contains("User2"));

        let json = store.export_history_json();
        assert!(json.contains("User1"));
        assert!(json.contains("User2"));
    }
}
