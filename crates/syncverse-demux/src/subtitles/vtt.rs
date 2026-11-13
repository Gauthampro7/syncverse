//! Pure-Rust WebVTT (.vtt) subtitle parser.

use syncverse_common::subtitles::SubtitleCue;
use syncverse_common::time::MediaTime;

use crate::subtitles::SubtitleParseError;

/// Strips WebVTT styling, voice (`<v Speaker>`), and inline timestamp (`<00:01.000>`) tags,
/// and unescapes basic HTML entities.
pub fn clean_vtt_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '<' {
            if let Some(rel_end) = chars[i + 1..].iter().position(|&c| c == '>') {
                let end = i + 1 + rel_end;
                if !chars[i + 1..end].contains(&'\n') {
                    i = end + 1;
                    continue;
                }
            }
            out.push('<');
            i += 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }

    let unescaped = out
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&");

    unescaped.trim().to_string()
}

/// Parses a WebVTT timestamp in `HH:MM:SS.mmm` or `MM:SS.mmm` format into a `MediaTime`.
///
/// Properly normalizes 1-, 2-, and 3+ digit fractional seconds
/// (`"00:01.5"` -> `1500ms`, `"00:01.05"` -> `1050ms`, `"00:01.5000"` -> `1500ms`).
pub fn parse_vtt_timestamp(s: &str) -> Option<MediaTime> {
    let s = s.trim();
    let (hms_part, frac_part) = s.split_once('.')?;
    let frac_str = frac_part.trim();
    if frac_str.is_empty() || !frac_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }

    let ms: u64 = match frac_str.len() {
        1 => frac_str.parse::<u64>().ok()? * 100,
        2 => frac_str.parse::<u64>().ok()? * 10,
        _ => frac_str[..3].parse::<u64>().ok()?,
    };

    let hms: Vec<&str> = hms_part.trim().split(':').collect();

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

#[inline]
fn is_vtt_block_keyword(line: &str, keyword: &str) -> bool {
    if let Some(rest) = line.strip_prefix(keyword) {
        rest.is_empty() || rest.starts_with([' ', '\t', ':'])
    } else {
        false
    }
}

/// Updates CSS brace depth and `/* ... */` comment state across a line inside a `STYLE` block.
fn update_css_block_state(line: &str, brace_depth: &mut usize, in_comment: &mut bool) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if *in_comment {
            if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                *in_comment = false;
                i += 2;
                continue;
            }
            i += 1;
        } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            *in_comment = true;
            i += 2;
        } else {
            if bytes[i] == b'{' {
                *brace_depth = brace_depth.saturating_add(1);
            } else if bytes[i] == b'}' {
                *brace_depth = brace_depth.saturating_sub(1);
            }
            i += 1;
        }
    }
}

/// Parses WebVTT text content into a chronologically sorted sequence of `SubtitleCue`s.
///
/// Handles:
/// - Leading `WEBVTT` magic header (with optional BOM or header commentary)
/// - Comments (`NOTE` blocks) and style/region blocks (`STYLE`, `REGION`), including
///   multi-paragraph `STYLE` blocks containing blank lines or CSS comments
/// - Short timestamps (`MM:SS.mmm`) and full timestamps (`HH:MM:SS.mmm`) with 1–3+ fractional digits
/// - Cue identifiers and cue settings
/// - HTML/WebVTT tag cleaning (`<v Alice>`, `<00:01.000>`, `<b>`, `<i>`, `<c.color>`)
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
    let mut in_style_block = false;
    let mut style_brace_depth: usize = 0;
    let mut in_css_comment = false;

    // Skip the first line containing WEBVTT
    let mut line_iter = lines.into_iter();
    let _ = line_iter.next();

    for line in line_iter {
        if line.trim().is_empty() {
            if in_style_block && (style_brace_depth > 0 || in_css_comment) {
                // Keep accumulating multi-paragraph CSS inside STYLE blocks
                block_lines.push(line);
                continue;
            }
            if !block_lines.is_empty() {
                if let Some(cue) = process_vtt_block(&block_lines) {
                    cues.push(cue);
                }
                block_lines.clear();
            }
            in_style_block = false;
            style_brace_depth = 0;
            in_css_comment = false;
        } else {
            if block_lines.is_empty() && is_vtt_block_keyword(line.trim(), "STYLE") {
                in_style_block = true;
            }
            if in_style_block {
                update_css_block_state(line, &mut style_brace_depth, &mut in_css_comment);
            }
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
    if is_vtt_block_keyword(first, "NOTE")
        || is_vtt_block_keyword(first, "STYLE")
        || is_vtt_block_keyword(first, "REGION")
    {
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

    #[test]
    fn test_parse_vtt_short_fractional_ms_and_multiparagraph_headers() {
        assert_eq!(
            parse_vtt_timestamp("00:01.5").map(|t| t.as_millis()),
            Some(1_500)
        );
        assert_eq!(
            parse_vtt_timestamp("00:01.05").map(|t| t.as_millis()),
            Some(1_050)
        );
        assert_eq!(
            parse_vtt_timestamp("01:00:02.25").map(|t| t.as_millis()),
            Some(3_602_250)
        );

        let vtt = r#"WEBVTT

STYLE
::cue {
  color: white;

  /* 00:01.000 --> 00:02.000 comment inside CSS must not leak! */
  background: black;
}

REGION
id:top-banner
width:80%

NOTE Multi-line note

Second note paragraph without timestamps

1
00:01.5 --> 00:03.05
<v Speaker>Hello <00:02.000>world!
"#;
        let cues = parse_vtt(vtt).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].start, MediaTime::from_millis(1_500));
        assert_eq!(cues[0].end, MediaTime::from_millis(3_050));
        assert_eq!(cues[0].text, "Hello world!");
    }
}
