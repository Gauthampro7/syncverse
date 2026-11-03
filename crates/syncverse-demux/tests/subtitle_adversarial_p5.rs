//! Adversarial stress test suite for subtitle scanning in `syncverse-demux`.
//!
//! Stress-tests:
//! 1. Symlink cycles and loops in subtitle subdirectories.
//! 2. Deeply nested non-direct subdirectories.
//! 3. Non-ASCII, Cyrillic, emoji, and multilingual filenames.
//! 4. Observation of Unicode case sensitivity limitations (`to_ascii_lowercase`).

#![deny(warnings)]

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use syncverse_demux::subtitles::scanner::scan_external_subtitles;

/// RAII test directory wrapper ensuring automatic cleanup on drop.
struct TempTestDir {
    path: PathBuf,
}

impl TempTestDir {
    fn new(prefix: &str) -> Self {
        let unique = format!(
            "syncverse_sub_adv_{}_{}_{}",
            prefix,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("Failed to create temporary directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn create_file(&self, rel: &str, content: &[u8]) -> PathBuf {
        let full = self.path.join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("Failed to create parent dirs");
        }
        let mut f = File::create(&full).expect("Failed to create file");
        f.write_all(content).expect("Failed to write content");
        full
    }
}

impl Drop for TempTestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

const SAMPLE_SRT: &[u8] = b"1\n00:00:01,000 --> 00:00:03,000\nHello World\n\n";

#[test]
fn test_subtitles_symlink_cycles_safe() {
    let temp = TempTestDir::new("symlink_cycle");
    let media = temp.create_file("movie.mp4", b"dummy media bytes");

    // Create subtitles/ directory with valid subtitle
    let sub = temp.create_file("subtitles/movie.en.srt", SAMPLE_SRT);
    assert!(sub.exists());

    // Create symlink loop: subtitles/loop -> subtitles (self-referential)
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let loop_link = temp.path().join("subtitles").join("loop");
        let _ = symlink(temp.path().join("subtitles"), &loop_link);

        // Also symlink subs -> subtitles
        let subs_link = temp.path().join("subs");
        let _ = symlink(temp.path().join("subtitles"), &subs_link);
    }

    // Must terminate promptly without infinite loop or stack overflow
    let tracks = scan_external_subtitles(&media);

    // Exactly one track should be discovered (deduplicated across symlinked dirs)
    assert_eq!(
        tracks.len(),
        1,
        "Expected exactly 1 subtitle track despite symlink cycle, found {}",
        tracks.len()
    );
    assert_eq!(tracks[0].language, Some("en".to_string()));
}

#[test]
fn test_subtitles_deep_nesting_ignored_safely() {
    let temp = TempTestDir::new("deep_nesting");
    let media = temp.create_file("movie.mkv", b"dummy");

    // Create deeply nested subtitle: subtitles/season1/disc1/movie.fr.srt
    temp.create_file("subtitles/season1/disc1/movie.fr.srt", SAMPLE_SRT);

    // Scanner should not recurse into deeper non-standard subdirectories
    let tracks = scan_external_subtitles(&media);
    assert_eq!(
        tracks.len(),
        0,
        "Deeply nested subtitles should be ignored safely"
    );
}

#[test]
fn test_subtitles_non_ascii_and_emoji_filenames() {
    let temp = TempTestDir::new("non_ascii");

    // 1. Cyrillic lowercase matching
    let media_cyr = temp.create_file("фильм.mp4", b"dummy");
    temp.create_file("subtitles/фильм.ru.srt", SAMPLE_SRT);

    let tracks = scan_external_subtitles(&media_cyr);
    assert_eq!(
        tracks.len(),
        1,
        "Matching Cyrillic subtitle should be detected"
    );
    assert_eq!(tracks[0].language, Some("ru".to_string()));

    // 2. Emoji filename matching
    let media_emoji = temp.create_file("🎬 movie.mp4", b"dummy");
    temp.create_file("subs/🎬 movie.en.vtt", b"WEBVTT\n\n00:00:01.000 --> 00:00:03.000\nEmoji Movie\n");

    let tracks_emoji = scan_external_subtitles(&media_emoji);
    assert_eq!(
        tracks_emoji.len(),
        1,
        "Matching emoji subtitle should be detected"
    );
    assert_eq!(tracks_emoji[0].language, Some("en".to_string()));
}

#[test]
fn test_subtitles_unicode_casing_behavior() {
    let temp = TempTestDir::new("unicode_casing");

    // Cyrillic uppercase media stem: "Фильм.mp4"
    let media_cyr_upper = temp.create_file("Фильм.mp4", b"dummy");
    // Cyrillic lowercase subtitle: "фильм.ru.srt"
    temp.create_file("subtitles/фильм.ru.srt", SAMPLE_SRT);

    let tracks = scan_external_subtitles(&media_cyr_upper);

    // Because `to_ascii_lowercase()` only folds ASCII [A-Z] -> [a-z],
    // Cyrillic uppercase 'Ф' does not match lowercase 'ф'.
    // We document this empirical observation:
    if tracks.is_empty() {
        println!("Documented: to_ascii_lowercase() does not fold non-ASCII Cyrillic casing.");
    } else {
        println!("Full Unicode case folding is active.");
    }
}
