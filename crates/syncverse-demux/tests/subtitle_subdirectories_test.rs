//! Integration tests for external subtitle discovery across standard subdirectories.
//!
//! Verifies that `scan_external_subtitles` discovers external subtitles placed in
//! `subtitles/`, `Subtitles/`, `subs/`, and `Subs/` relative to the media file,
//! while correctly associating language tags, cue formats, and ignoring non-matching files.

#![deny(warnings)]

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use syncverse_common::subtitles::SubtitleSource;
use syncverse_demux::subtitles::scan_external_subtitles;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

/// RAII helper that manages an isolated temporary directory and removes it on drop.
struct TempTestDir {
    path: PathBuf,
}

impl TempTestDir {
    fn new(prefix: &str) -> Self {
        let count = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "syncverse_subdir_test_{}_{}_{}_{}",
            prefix,
            std::process::id(),
            timestamp,
            count
        ));
        fs::create_dir_all(&path).expect("failed to create temporary test directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write_file(&self, relative_path: &str, content: &str) -> PathBuf {
        let file_path = self.path.join(relative_path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).expect("failed to create parent directories");
        }
        let mut file = File::create(&file_path).expect("failed to create test file");
        file.write_all(content.as_bytes())
            .expect("failed to write test file contents");
        file_path
    }
}

impl Drop for TempTestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

const SAMPLE_SRT: &str = "1\n00:00:01,000 --> 00:00:04,000\nHello World\n";
const SAMPLE_VTT: &str = "WEBVTT\n\n00:00:02.000 --> 00:00:05.000\nBonjour le monde\n";
const SAMPLE_ASS: &str = "[Script Info]\nTitle: Test\nScriptType: v4.00+\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:03.00,0:00:06.00,Default,,0,0,0,,Hola Mundo\n";
const SAMPLE_SSA: &str = "[Script Info]\nTitle: Test\nScriptType: v4.00\n[Events]\nFormat: Marked, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: Marked=0,0:00:04.00,0:00:07.00,Default,,0,0,0,,Hallo Welt\n";

#[test]
fn test_scan_subtitles_in_lowercase_subtitles_directory() {
    let temp = TempTestDir::new("subtitles_lower");
    let movie = temp.write_file("interstellar.mkv", "dummy video stream");
    temp.write_file("subtitles/interstellar.en.srt", SAMPLE_SRT);

    let tracks = scan_external_subtitles(&movie);
    assert_eq!(tracks.len(), 1, "Should find 1 subtitle in subtitles/");
    assert_eq!(tracks[0].source, SubtitleSource::External);
    assert_eq!(tracks[0].format, "srt");
    assert_eq!(tracks[0].language.as_deref(), Some("en"));
    assert_eq!(tracks[0].title, "English");
    assert_eq!(tracks[0].cues.len(), 1);
    assert_eq!(tracks[0].cues[0].text, "Hello World");
}

#[test]
fn test_scan_subtitles_in_capitalized_subtitles_directory() {
    let temp = TempTestDir::new("subtitles_upper");
    let movie = temp.write_file("blade_runner.mp4", "dummy video stream");
    temp.write_file("Subtitles/blade_runner.fr.vtt", SAMPLE_VTT);

    let tracks = scan_external_subtitles(&movie);
    assert_eq!(tracks.len(), 1, "Should find 1 subtitle in Subtitles/");
    assert_eq!(tracks[0].source, SubtitleSource::External);
    assert_eq!(tracks[0].format, "vtt");
    assert_eq!(tracks[0].language.as_deref(), Some("fr"));
    assert_eq!(tracks[0].title, "French");
    assert_eq!(tracks[0].cues.len(), 1);
    assert_eq!(tracks[0].cues[0].text, "Bonjour le monde");
}

#[test]
fn test_scan_subtitles_in_lowercase_subs_directory() {
    let temp = TempTestDir::new("subs_lower");
    let movie = temp.write_file("matrix.mkv", "dummy video stream");
    temp.write_file("subs/matrix.es.ass", SAMPLE_ASS);

    let tracks = scan_external_subtitles(&movie);
    assert_eq!(tracks.len(), 1, "Should find 1 subtitle in subs/");
    assert_eq!(tracks[0].source, SubtitleSource::External);
    assert_eq!(tracks[0].format, "ass");
    assert_eq!(tracks[0].language.as_deref(), Some("es"));
    assert_eq!(tracks[0].title, "Spanish");
    assert_eq!(tracks[0].cues.len(), 1);
    assert_eq!(tracks[0].cues[0].text, "Hola Mundo");
}

#[test]
fn test_scan_subtitles_in_capitalized_subs_directory() {
    let temp = TempTestDir::new("subs_upper");
    let movie = temp.write_file("dune.mkv", "dummy video stream");
    temp.write_file("Subs/dune.de.ssa", SAMPLE_SSA);

    let tracks = scan_external_subtitles(&movie);
    assert_eq!(tracks.len(), 1, "Should find 1 subtitle in Subs/");
    assert_eq!(tracks[0].source, SubtitleSource::External);
    assert_eq!(tracks[0].format, "ssa");
    assert_eq!(tracks[0].language.as_deref(), Some("de"));
    assert_eq!(tracks[0].title, "German");
    assert_eq!(tracks[0].cues.len(), 1);
    assert_eq!(tracks[0].cues[0].text, "Hallo Welt");
}

#[test]
fn test_scan_subtitles_all_subdirectories_combined_with_root() {
    let temp = TempTestDir::new("combined_subdirs");
    let movie = temp.write_file("features.mkv", "dummy video stream");

    // Subtitle directly alongside the media file
    temp.write_file("features.ja.srt", SAMPLE_SRT);

    // Subtitle in subtitles/
    temp.write_file("subtitles/features.en.srt", SAMPLE_SRT);

    // Subtitle in Subtitles/
    temp.write_file("Subtitles/features.fr.vtt", SAMPLE_VTT);

    // Subtitle in subs/
    temp.write_file("subs/features.es.ass", SAMPLE_ASS);

    // Subtitle in Subs/
    temp.write_file("Subs/features.de.ssa", SAMPLE_SSA);

    // Unrelated files that must NOT be detected as subtitles for features.mkv
    temp.write_file("other_film.en.srt", SAMPLE_SRT);
    temp.write_file("subtitles/other_film.fr.srt", SAMPLE_SRT);
    temp.write_file("subs/other_film.de.ass", SAMPLE_ASS);
    temp.write_file("features.txt", "notes");
    temp.write_file("features.nfo", "info");
    temp.write_file("subtitles/features.unsupported_ext", "invalid");
    temp.write_file("unsupported_subfolder/features.it.srt", SAMPLE_SRT);

    let tracks = scan_external_subtitles(&movie);

    // On Linux (case-sensitive), subtitles/, Subtitles/, subs/, Subs/ and root are all distinct
    // Verify all matching subtitles are discovered
    assert!(
        tracks.len() >= 5,
        "Expected at least 5 tracks (root + 4 subdirs), found {}",
        tracks.len()
    );

    let languages: Vec<Option<&str>> = tracks.iter().map(|t| t.language.as_deref()).collect();
    assert!(languages.contains(&Some("ja")), "Missing root Japanese subtitle");
    assert!(languages.contains(&Some("en")), "Missing subtitles/ English subtitle");
    assert!(languages.contains(&Some("fr")), "Missing Subtitles/ French subtitle");
    assert!(languages.contains(&Some("es")), "Missing subs/ Spanish subtitle");
    assert!(languages.contains(&Some("de")), "Missing Subs/ German subtitle");

    // Verify track IDs are unique and strictly monotonic starting from 1000
    for (i, track) in tracks.iter().enumerate() {
        assert_eq!(track.id, 1000 + i);
    }
}

#[test]
fn test_scan_subtitles_missing_subdirectories_graceful() {
    let temp = TempTestDir::new("missing_subdirs");
    let movie = temp.write_file("solitary.mkv", "dummy video stream");

    // No subdirectories created at all
    let tracks = scan_external_subtitles(&movie);
    assert!(tracks.is_empty(), "Should return empty Vec when no subtitles exist");
}

#[test]
fn test_scan_subtitles_non_matching_stems_ignored_across_all_subdirs() {
    let temp = TempTestDir::new("stem_isolation");
    let movie = temp.write_file("target_movie.mkv", "dummy video stream");

    temp.write_file("subtitles/different_movie.en.srt", SAMPLE_SRT);
    temp.write_file("Subtitles/different_movie.fr.vtt", SAMPLE_VTT);
    temp.write_file("subs/different_movie.es.ass", SAMPLE_ASS);
    temp.write_file("Subs/different_movie.de.ssa", SAMPLE_SSA);

    let tracks = scan_external_subtitles(&movie);
    assert!(tracks.is_empty(), "Subtitles for different movie stems must be ignored");
}

#[test]
fn test_scan_subtitles_nonexistent_media_path() {
    let temp = TempTestDir::new("nonexistent");
    let movie = temp.path().join("does_not_exist.mkv");

    let tracks = scan_external_subtitles(&movie);
    assert!(tracks.is_empty(), "Nonexistent media path should return empty vector");
}
