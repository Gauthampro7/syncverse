//! Subtitle demuxing, pure-Rust parsers (SRT, WebVTT, ASS/SSA), and external file scanner.

pub mod ass;
pub mod scanner;
pub mod srt;
pub mod vtt;

pub use ass::{clean_ass_text, parse_ass, parse_ass_dialogue_line, parse_ass_timestamp};
pub use scanner::{
    detect_subtitle_language, lookup_language, parse_subtitle_file, scan_external_subtitles,
    SUBTITLE_EXTENSIONS,
};
pub use srt::{clean_srt_text, parse_srt, parse_srt_timestamp};
pub use vtt::{clean_vtt_text, parse_vtt, parse_vtt_timestamp};

/// Strongly typed errors encountered during subtitle parsing and demuxing.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SubtitleParseError {
    /// Subtitle file violates the expected format specification or is missing required headers.
    #[error("Invalid subtitle format: {0}")]
    InvalidFormat(String),

    /// A timestamp string could not be parsed into valid presentation hours/minutes/seconds/milliseconds.
    #[error("Failed to parse subtitle timestamp: '{0}'")]
    InvalidTimestamp(String),

    /// An I/O error occurred while reading a subtitle file from the filesystem.
    #[error("I/O error reading subtitle file: {0}")]
    Io(String),
}
