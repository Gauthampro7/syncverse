//! Smart in-player chat, binary log storage, inverted index FTS, and slash commands.

#![deny(warnings)]
#![deny(missing_docs)]

pub mod commands;
pub mod error;
pub mod export;
pub mod index;
pub mod record;
pub mod storage;
pub mod store;

pub use commands::{
    format_sync_status_report, parse_time_to_ms, ActivePoll, Bookmark, ClipDefinition,
    DispatchAction, ParsedCommand, ParsedSlashCommand, SlashCommandDispatcher, SlashCommandParser,
    SyncDiagnostics,
};
pub use error::ChatError;
pub use export::{export_history_json, export_transcript_markdown, TranscriptFormat};
pub use index::{is_cjk, tokenize_spec, InvertedIndexFts, STOP_WORDS};
pub use record::{
    make_file_header, ChatNetworkPacket, ChatRecord, HEADER_MAGIC, HEADER_VERSION, SYNC_WORD,
    UNANCHORED_MEDIA_PTS,
};
pub use storage::{ChatLogStorage, ChatRecoveryScanner, RecoveryStats};
pub use store::{ChatStore, ChatStoreTrait, SearchResult};
