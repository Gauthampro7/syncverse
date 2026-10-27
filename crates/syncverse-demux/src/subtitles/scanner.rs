//! Filesystem scanner and language detector for external subtitles (.srt, .vtt, .ass, .ssa).

use std::fs;
use std::path::Path;

use syncverse_common::subtitles::{SubtitleSource, SubtitleTrack};

use crate::subtitles::ass::parse_ass;
use crate::subtitles::srt::parse_srt;
use crate::subtitles::vtt::parse_vtt;
use crate::subtitles::SubtitleParseError;

/// Supported external subtitle file extensions.
pub const SUBTITLE_EXTENSIONS: &[&str] = &["srt", "vtt", "ass", "ssa"];

/// Maps ISO 639-1 / ISO 639-2 codes and common language names to normalized (code, title).
pub fn lookup_language(token: &str) -> Option<(&'static str, &'static str)> {
    let lower = token.to_ascii_lowercase();
    match lower.as_str() {
        "en" | "eng" | "english" => Some(("en", "English")),
        "es" | "spa" | "spanish" | "espanol" => Some(("es", "Spanish")),
        "fr" | "fra" | "fre" | "french" | "francais" => Some(("fr", "French")),
        "de" | "deu" | "ger" | "german" | "deutsch" => Some(("de", "German")),
        "it" | "ita" | "italian" | "italiano" => Some(("it", "Italian")),
        "pt" | "por" | "portuguese" => Some(("pt", "Portuguese")),
        "ja" | "jpn" | "japanese" => Some(("ja", "Japanese")),
        "ko" | "kor" | "korean" => Some(("ko", "Korean")),
        "zh" | "zho" | "chi" | "chinese" => Some(("zh", "Chinese")),
        "ru" | "rus" | "russian" => Some(("ru", "Russian")),
        "ar" | "ara" | "arabic" => Some(("ar", "Arabic")),
        "hi" | "hin" | "hindi" => Some(("hi", "Hindi")),
        "nl" | "nld" | "dut" | "dutch" => Some(("nl", "Dutch")),
        "pl" | "pol" | "polish" => Some(("pl", "Polish")),
        "sv" | "swe" | "swedish" => Some(("sv", "Swedish")),
        "da" | "dan" | "danish" => Some(("da", "Danish")),
        "fi" | "fin" | "finnish" => Some(("fi", "Finnish")),
        "no" | "nor" | "norwegian" => Some(("no", "Norwegian")),
        "tr" | "tur" | "turkish" => Some(("tr", "Turkish")),
        "el" | "ell" | "gre" | "greek" => Some(("el", "Greek")),
        "cs" | "ces" | "cze" | "czech" => Some(("cs", "Czech")),
        "hu" | "hun" | "hungarian" => Some(("hu", "Hungarian")),
        "uk" | "ukr" | "ukrainian" => Some(("uk", "Ukrainian")),
        "vi" | "vie" | "vietnamese" => Some(("vi", "Vietnamese")),
        "id" | "ind" | "indonesian" => Some(("id", "Indonesian")),
        "th" | "tha" | "thai" => Some(("th", "Thai")),
        "he" | "heb" | "hebrew" => Some(("he", "Hebrew")),
        "ro" | "ron" | "rum" | "romanian" => Some(("ro", "Romanian")),
        "fa" | "fas" | "per" | "persian" => Some(("fa", "Persian")),
        _ => None,
    }
}

/// Detects language code and human-readable title from a subtitle filename relative to the media stem.
///
/// Examples:
/// - `video.en.srt` (media_stem = `video`) -> `(Some("en"), "English")`
/// - `video.eng.sdh.srt` -> `(Some("eng"), "English (SDH)")`
/// - `video.es.vtt` -> `(Some("es"), "Spanish")`
/// - `video.srt` -> `(None, "External Subtitle")`
pub fn detect_subtitle_language(filename: &str, media_stem: &str) -> (Option<String>, String) {
    let mut name = filename;

    // Strip subtitle extension
    if let Some(pos) = name.rfind('.') {
        let ext = &name[pos + 1..];
        if SUBTITLE_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)) {
            name = &name[..pos];
        }
    }

    // Strip media_stem prefix if present
    let remainder = if name.to_ascii_lowercase().starts_with(&media_stem.to_ascii_lowercase()) {
        let rem = &name[media_stem.len()..];
        rem.trim_matches(['.', '_', '-'])
    } else {
        name
    };

    if remainder.is_empty() {
        return (None, "External Subtitle".to_string());
    }

    let tokens: Vec<&str> = remainder
        .split(['.', '_'])
        .filter(|t| !t.is_empty())
        .collect();

    let mut detected_lang_code: Option<String> = None;
    let mut detected_lang_name: Option<String> = None;
    let mut is_sdh = false;
    let mut is_forced = false;
    let mut is_cc = false;

    for token in &tokens {
        let parts: Vec<&str> = token.split('-').collect();
        for (p_idx, part) in parts.iter().enumerate() {
            let lower = part.to_ascii_lowercase();
            if lower == "sdh" {
                is_sdh = true;
            } else if lower == "forced" {
                is_forced = true;
            } else if lower == "cc" {
                is_cc = true;
            } else if detected_lang_code.is_none() {
                if let Some((_code, title)) = lookup_language(&lower) {
                    if parts.len() > p_idx + 1 && (parts[p_idx + 1].len() == 2 || parts[p_idx + 1].len() == 3) {
                        let country = parts[p_idx + 1];
                        let country_lower = country.to_ascii_lowercase();
                        if country_lower != "sdh" && country_lower != "cc" {
                            detected_lang_code = Some(format!("{part}-{country}"));
                            detected_lang_name = Some(title.to_string());
                            continue;
                        }
                    }
                    detected_lang_code = Some(part.to_string());
                    detected_lang_name = Some(title.to_string());
                }
            }
        }
    }

    let mut title = if let Some(name) = detected_lang_name {
        name
    } else if let Some(first) = tokens.first() {
        first.to_string()
    } else {
        "External Subtitle".to_string()
    };

    if is_sdh {
        title.push_str(" (SDH)");
    } else if is_forced {
        title.push_str(" (Forced)");
    } else if is_cc {
        title.push_str(" (CC)");
    }

    (detected_lang_code, title)
}

/// Parses a subtitle file from the filesystem into a `SubtitleTrack`.
pub fn parse_subtitle_file<P: AsRef<Path>>(
    path: P,
    track_id: usize,
    media_stem: Option<&str>,
) -> Result<SubtitleTrack, SubtitleParseError> {
    let path = path.as_ref();
    let content = fs::read_to_string(path)
        .map_err(|e| SubtitleParseError::Io(format!("{}: {}", path.display(), e)))?;

    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let (cues, format) = match ext.as_str() {
        "srt" => (parse_srt(&content)?, "srt".to_string()),
        "vtt" => (parse_vtt(&content)?, "vtt".to_string()),
        "ass" | "ssa" => (parse_ass(&content)?, ext),
        other => {
            return Err(SubtitleParseError::InvalidFormat(format!(
                "Unsupported subtitle extension: .{other}"
            )))
        }
    };

    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("subtitle");

    let stem = media_stem.unwrap_or("");
    let (language, title) = detect_subtitle_language(filename, stem);

    Ok(SubtitleTrack::new(
        track_id,
        SubtitleSource::External,
        format,
        language,
        title,
        cues,
    ))
}

/// Automatically scans the directory of `media_path` for matching external subtitles.
///
/// Matches any file in the same directory where the filename starts with the media
/// file stem and has a supported extension (`.srt`, `.vtt`, `.ass`, `.ssa`).
pub fn scan_external_subtitles<P: AsRef<Path>>(media_path: P) -> Vec<SubtitleTrack> {
    let media_path = media_path.as_ref();
    let parent_dir = match media_path.parent() {
        Some(dir) if dir.exists() => dir,
        _ => return Vec::new(),
    };

    let media_stem = match media_path.file_stem().and_then(|s| s.to_str()) {
        Some(stem) => stem,
        None => return Vec::new(),
    };

    let mut tracks = Vec::new();
    let read_dir = match fs::read_dir(parent_dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut entries: Vec<_> = read_dir.filter_map(|e| e.ok()).collect();
    // Sort entries alphabetically for deterministic track order
    entries.sort_by_key(|e| e.file_name());

    let mut track_id = 1000; // Start external tracks at 1000 to avoid ID collisions with embedded tracks
    for entry in entries {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let fname = match path.file_name().and_then(|s| s.to_str()) {
            Some(name) => name,
            None => continue,
        };

        let ext = match path.extension().and_then(|s| s.to_str()) {
            Some(ext) => ext.to_ascii_lowercase(),
            None => continue,
        };

        if !SUBTITLE_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }

        // Check if filename matches media stem prefix
        if !fname.to_ascii_lowercase().starts_with(&media_stem.to_ascii_lowercase()) {
            continue;
        }

        if let Ok(track) = parse_subtitle_file(&path, track_id, Some(media_stem)) {
            tracks.push(track);
            track_id += 1;
        }
    }

    tracks
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_detect_subtitle_language() {
        let (lang, title) = detect_subtitle_language("my_video.en.srt", "my_video");
        assert_eq!(lang.as_deref(), Some("en"));
        assert_eq!(title, "English");

        let (lang, title) = detect_subtitle_language("my_video.eng.sdh.srt", "my_video");
        assert_eq!(lang.as_deref(), Some("eng"));
        assert_eq!(title, "English (SDH)");

        let (lang, title) = detect_subtitle_language("my_video.ja.ass", "my_video");
        assert_eq!(lang.as_deref(), Some("ja"));
        assert_eq!(title, "Japanese");

        let (lang, title) = detect_subtitle_language("my_video.srt", "my_video");
        assert_eq!(lang, None);
        assert_eq!(title, "External Subtitle");
    }

    #[test]
    fn test_scan_external_subtitles_directory() {
        let temp_dir = std::env::temp_dir().join(format!("syncverse_scan_test_{}", std::process::id()));
        fs::create_dir_all(&temp_dir).unwrap();

        let movie_path = temp_dir.join("avatar.mkv");
        let en_sub = temp_dir.join("avatar.en.srt");
        let fr_sub = temp_dir.join("avatar.fr.vtt");
        let unrelated = temp_dir.join("other_movie.en.srt");

        fs::write(&movie_path, b"fake video").unwrap();

        let srt_content = "1\n00:00:01,000 --> 00:00:03,000\nHello English\n";
        let vtt_content = "WEBVTT\n\n00:00:02.000 --> 00:00:04.000\nBonjour le monde\n";

        let mut f1 = fs::File::create(&en_sub).unwrap();
        f1.write_all(srt_content.as_bytes()).unwrap();

        let mut f2 = fs::File::create(&fr_sub).unwrap();
        f2.write_all(vtt_content.as_bytes()).unwrap();

        let mut f3 = fs::File::create(&unrelated).unwrap();
        f3.write_all(srt_content.as_bytes()).unwrap();

        let discovered = scan_external_subtitles(&movie_path);
        assert_eq!(discovered.len(), 2);

        assert_eq!(discovered[0].language.as_deref(), Some("en"));
        assert_eq!(discovered[0].title, "English");
        assert_eq!(discovered[0].cues.len(), 1);

        assert_eq!(discovered[1].language.as_deref(), Some("fr"));
        assert_eq!(discovered[1].title, "French");
        assert_eq!(discovered[1].cues.len(), 1);

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
