//! Pure-Rust WebVTT (.vtt) subtitle parser.

use syncverse_common::subtitles::SubtitleCue;
use syncverse_common::time::MediaTime;

use crate::subtitles::SubtitleParseError;

/// Strips WebVTT styling/voice tags (`<...>`, `<v Speaker>`) and unescapes basic HTML entities.
pub fn clean_vtt_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_tag = false;

    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }

    let unescaped = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");

    unescaped.trim().to_string()
}

/// Parses a WebVTT timestamp in `HH:MM:SS.mmm` or `MM:SS.mmm` format into a `MediaTime`.
pub fn parse_vtt_timestamp(s: &str) -> Option<MediaTime> {
    let s = s.trim();
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 2 {
        return None;
    }

    let ms: u64 = parts[1].trim().parse().ok()?;
    let hms: Vec<&str> = parts[0].trim().split(':').collect();

    let (hours, mins, secs) = match hms.len() {
        3 => {
            let h = hms[0].trim().parse::<u64>().ok()?;
            let m = hms[1].trim().parse::<u64>().ok()?;
            let s = hms[2].trim().parse::<u64>().ok()?;
            (h, m, s)
        }
        2 => {
            let m = hms[0].trim().parse::<u64>().ok()?;
            let s = hms[1].trim().parse::<u64>().ok()?;
            (0, m, s)
        }
        _ => return None,
    };

    let total_ms = hours
        .checked_mul(3_600_000)?
        .checked_add(mins.checked_mul(60_000)?)?
        .checked_add(secs.checked_mul(1_000)?)?
        .checked_add(ms)?;

    Some(MediaTime::from_millis(total_ms))
}

/// Parses a WebVTT timestamp line containing `-->` (e.g. `00:01:20.000 --> 00:01:23.456 line:0%`).
fn parse_vtt_timestamp_line(line: &str) -> Option<(MediaTime, MediaTime)> {
    let arrow_pos = line.find("-->")?;
    let start_str = line[..arrow_pos].trim();
    let end_part = line[arrow_pos + 3..].trim();

    // Cue settings (e.g., `position:50% align:middle`) may follow the end timestamp
    let end_str = end_part.split_whitespace().next().unwrap_or(end_part);

    let start = parse_vtt_timestamp(start_str)?;
    let end = parse_vtt_timestamp(end_str)?;

    Some((start, end))
}

/// Parses WebVTT text content into a chronologically sorted sequence of `SubtitleCue`s.
///
/// Handles:
/// - Leading `WEBVTT` magic header (with optional BOM or header commentary)
/// - Comments (`NOTE` blocks) and style/region blocks (`STYLE`, `REGION`)
/// - Short timestamps (`MM:SS.mmm`) and full timestamps (`HH:MM:SS.mmm`)
/// - Cue identifiers and cue settings
/// - HTML/WebVTT tag cleaning (`<v Alice>`, `<b>`, `<i>`, `<c.color>`)
pub fn parse_vtt(content: &str) -> Result<Vec<SubtitleCue>, SubtitleParseError> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let trimmed = content.trim_start();

    if !trimmed.starts_with("WEBVTT") {
        return Err(SubtitleParseError::InvalidFormat(
            "Missing WEBVTT header signature".into(),
        ));
    }

    let mut cues = Vec::new();
    let lines: Vec<&str> = content.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut block_lines: Vec<&str> = Vec::new();

    // Skip the first line containing WEBVTT
    let mut line_iter = lines.into_iter();
    let _ = line_iter.next();

    for line in line_iter {
        if line.trim().is_empty() {
            if !block_lines.is_empty() {
                if let Some(cue) = process_vtt_block(&block_lines) {
                    cues.push(cue);
                }
                block_lines.clear();
            }
        } else {
            block_lines.push(line);
        }
    }

    if !block_lines.is_empty() {
        if let Some(cue) = process_vtt_block(&block_lines) {
            cues.push(cue);
        }
    }

    cues.sort_by_key(|c| c.start);

    Ok(cues)
}

/// Processes a single WebVTT block.
fn process_vtt_block(lines: &[&str]) -> Option<SubtitleCue> {
    if lines.is_empty() {
        return None;
    }

    // Skip NOTE, STYLE, and REGION blocks
    let first = lines[0].trim();
    if first.starts_with("NOTE") || first.starts_with("STYLE") || first.starts_with("REGION") {
        return None;
    }

    // Line 0 or Line 1 may contain the timestamp line
    let (ts_idx, (start, end)) = if let Some(timing) = parse_vtt_timestamp_line(lines[0]) {
        (0, timing)
    } else if lines.len() > 1 {
        let timing = parse_vtt_timestamp_line(lines[1])?;
        (1, timing)
    } else {
        return None;
    };

    if end < start {
        return None;
    }

    let text_lines = &lines[ts_idx + 1..];
    if text_lines.is_empty() {
        return None;
    }

    let raw_text = text_lines.join("\n");
    let cleaned_text = clean_vtt_text(&raw_text);
    if cleaned_text.is_empty() {
        return None;
    }

    Some(SubtitleCue::new(start, end, cleaned_text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_vtt_standard() {
        let vtt = r#"WEBVTT - WebVTT test file

NOTE This is a commentary note that should be ignored

1
00:01.000 --> 00:04.500 position:50% line:0%
<v Roger>Hello from WebVTT!</v>

cue-2
01:05.200 --> 01:08.800
<c.yellow>Second cue with styling</c>
"#;
        let cues = parse_vtt(vtt).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, MediaTime::from_millis(1000));
        assert_eq!(cues[0].end, MediaTime::from_millis(4500));
        assert_eq!(cues[0].text, "Hello from WebVTT!");

        assert_eq!(cues[1].start, MediaTime::from_millis(65200));
        assert_eq!(cues[1].end, MediaTime::from_millis(68800));
        assert_eq!(cues[1].text, "Second cue with styling");
    }

    #[test]
    fn test_parse_vtt_invalid_header_rejection() {
        let bad = "NOT A WEBVTT FILE\n00:00:01.000 --> 00:00:02.000\nText";
        assert!(parse_vtt(bad).is_err());
    }
}
