//! Pure-Rust SubRip (.srt) subtitle parser.

use syncverse_common::subtitles::SubtitleCue;
use syncverse_common::time::MediaTime;

use crate::subtitles::SubtitleParseError;

/// Strips HTML-like formatting tags (`<...>`, `{...}`) and unescapes basic HTML entities.
pub fn clean_srt_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_tag = false;
    let mut in_curly = false;

    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            '{' => in_curly = true,
            '}' => in_curly = false,
            _ if !in_tag && !in_curly => out.push(ch),
            _ => {}
        }
    }

    // Unescape standard HTML entities
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

/// Parses an SRT timestamp in `HH:MM:SS,mmm` or `MM:SS,mmm` format into a `MediaTime`.
pub fn parse_srt_timestamp(s: &str) -> Option<MediaTime> {
    let s = s.trim();
    let parts: Vec<&str> = s.split(',').collect();
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

/// Parses a timestamp line containing `-->` (e.g. `00:01:20,000 --> 00:01:23,456`).
fn parse_timestamp_line(line: &str) -> Option<(MediaTime, MediaTime)> {
    let arrow_pos = line.find("-->")?;
    let start_str = line[..arrow_pos].trim();
    let end_part = line[arrow_pos + 3..].trim();

    // The end part might have trailing positioning coordinates (e.g. `X1:0 X2:0`)
    let end_str = end_part.split_whitespace().next().unwrap_or(end_part);

    let start = parse_srt_timestamp(start_str)?;
    let end = parse_srt_timestamp(end_str)?;

    Some((start, end))
}

/// Parses full SubRip (.srt) text content into a chronologically sorted sequence of `SubtitleCue`s.
///
/// Handles:
/// - UTF-8 Byte Order Marks (BOM)
/// - Windows CRLF (`\r\n`) and Unix LF (`\n`)
/// - HTML tag stripping (`<b>`, `<i>`, `<font>`) and entity unescaping
/// - Graceful skipping of malformed entries without abandoning the entire file
pub fn parse_srt(content: &str) -> Result<Vec<SubtitleCue>, SubtitleParseError> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut cues = Vec::new();

    // Group lines into blocks separated by one or more blank lines
    let lines: Vec<&str> = content.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut block_lines: Vec<&str> = Vec::new();

    for line in lines {
        if line.trim().is_empty() {
            if !block_lines.is_empty() {
                if let Some(cue) = process_srt_block(&block_lines) {
                    cues.push(cue);
                }
                block_lines.clear();
            }
        } else {
            block_lines.push(line);
        }
    }

    if !block_lines.is_empty() {
        if let Some(cue) = process_srt_block(&block_lines) {
            cues.push(cue);
        }
    }

    // Sort cues by start timestamp
    cues.sort_by_key(|c| c.start);

    Ok(cues)
}

/// Helper processing a single block of non-empty lines into a `SubtitleCue`.
fn process_srt_block(lines: &[&str]) -> Option<SubtitleCue> {
    if lines.is_empty() {
        return None;
    }

    // Line 0 might be index number (e.g. "1") or the timestamp line itself
    let (ts_idx, (start, end)) = if let Some(timing) = parse_timestamp_line(lines[0]) {
        (0, timing)
    } else if lines.len() > 1 {
        let timing = parse_timestamp_line(lines[1])?;
        (1, timing)
    } else {
        return None;
    };

    if end < start {
        return None; // Reject inverted timestamps
    }

    let text_lines = &lines[ts_idx + 1..];
    if text_lines.is_empty() {
        return None;
    }

    let raw_text = text_lines.join("\n");
    let cleaned_text = clean_srt_text(&raw_text);
    if cleaned_text.is_empty() {
        return None;
    }

    Some(SubtitleCue::new(start, end, cleaned_text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_srt_standard() {
        let srt = r#"1
00:00:01,000 --> 00:00:04,000
Hello, world!

2
00:00:05,500 --> 00:00:08,250
Second subtitle line
with multiple lines
"#;
        let cues = parse_srt(srt).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, MediaTime::from_millis(1000));
        assert_eq!(cues[0].end, MediaTime::from_millis(4000));
        assert_eq!(cues[0].text, "Hello, world!");

        assert_eq!(cues[1].start, MediaTime::from_millis(5500));
        assert_eq!(cues[1].end, MediaTime::from_millis(8250));
        assert_eq!(cues[1].text, "Second subtitle line\nwith multiple lines");
    }

    #[test]
    fn test_parse_srt_bom_and_tags_and_entities() {
        let srt = "\u{feff}1\r\n00:01:20,123 --> 00:01:24,456\r\n<b>Important:</b> &lt;Tom &amp; Jerry&gt;\r\n\r\n2\r\n00:01:25,000 --> 00:01:27,000\r\n<font color=\"#ff0000\">Red Alert!</font>\r\n";
        let cues = parse_srt(srt).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, MediaTime::from_millis(80123));
        assert_eq!(cues[0].end, MediaTime::from_millis(84456));
        assert_eq!(cues[0].text, "Important: <Tom & Jerry>");
        assert_eq!(cues[1].text, "Red Alert!");
    }

    #[test]
    fn test_parse_srt_corrupt_entry_resilience() {
        let srt = r#"1
00:00:01,000 --> 00:00:02,000
Valid First

2
GARBAGE TIMESTAMP LINE
This cue should be skipped

3
00:00:03,000 --> 00:00:04,000
Valid Third
"#;
        let cues = parse_srt(srt).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "Valid First");
        assert_eq!(cues[1].text, "Valid Third");
    }
}
