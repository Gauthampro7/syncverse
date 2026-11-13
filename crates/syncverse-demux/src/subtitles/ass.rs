//! Pure-Rust Advanced SubStation Alpha (.ass / .ssa) subtitle parser.

use syncverse_common::subtitles::SubtitleCue;
use syncverse_common::time::MediaTime;

use crate::subtitles::SubtitleParseError;

/// Strips ASS style override tags (`{\...}`), suppresses vector drawing payloads (`\p1`..`\p0`/`\r`),
/// and replaces ASS escape sequences (`\N`, `\n`, `\h`).
pub fn clean_ass_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    let mut in_drawing_mode = false;

    while i < chars.len() {
        let ch = chars[i];
        if ch == '{' {
            // Find matching '}' or consume to end if unclosed
            let block_start = i + 1;
            let mut block_end = block_start;
            while block_end < chars.len() && chars[block_end] != '}' {
                block_end += 1;
            }
            let block: String = chars[block_start..block_end].iter().collect();
            for raw_tag in block.split('\\').skip(1) {
                let tag = raw_tag.trim();
                if let Some(after_p) = tag.strip_prefix('p') {
                    let digit_len = after_p
                        .bytes()
                        .take_while(|b| b.is_ascii_digit())
                        .count();
                    if digit_len > 0 {
                        if let Ok(scale) = after_p[..digit_len].parse::<u32>() {
                            in_drawing_mode = scale > 0;
                        }
                    }
                } else if tag == "r" || tag.starts_with('r') {
                    in_drawing_mode = false;
                }
            }
            i = if block_end < chars.len() {
                block_end + 1
            } else {
                block_end
            };
            continue;
        }

        if in_drawing_mode {
            i += 1;
            continue;
        }

        // Handle escape sequences: \N, \n, \h
        if ch == '\\' && i + 1 < chars.len() {
            match chars[i + 1] {
                'N' | 'n' => {
                    out.push('\n');
                    i += 2;
                    continue;
                }
                'h' => {
                    out.push(' ');
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }

        out.push(ch);
        i += 1;
    }

    out.trim().to_string()
}

/// Parses a Matroska ASS/SSA packet (`ReadOrder,Layer,Style,Name,MarginL,MarginR,MarginV,Effect,Text`)
/// or a full `Dialogue:` event line into cleaned subtitle text.
///
/// Validates integer `ReadOrder` and `Layer` on 8-comma Matroska packets with no prefix length
/// cap (supporting arbitrarily long `Style` / `Name` fields), and returns `None` if the input is
/// not a valid ASS packet or if its cleaned text is empty (e.g., vector drawing-only events).
pub fn parse_matroska_ass_packet(raw: &str) -> Option<String> {
    let trimmed = raw.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    if trimmed.is_empty() {
        return None;
    }

    if let Some(after_prefix) = trimmed.strip_prefix("Dialogue:") {
        let parts: Vec<&str> = after_prefix.trim_start().splitn(10, ',').collect();
        if parts.len() == 10 {
            let cleaned = clean_ass_text(parts[9]);
            if cleaned.is_empty() {
                return None;
            }
            return Some(cleaned);
        }
        return None;
    }

    let parts: Vec<&str> = trimmed.splitn(9, ',').collect();
    if parts.len() == 9
        && parts[0].trim().parse::<i64>().is_ok()
        && parts[1].trim().parse::<i64>().is_ok()
    {
        let cleaned = clean_ass_text(parts[8]);
        if cleaned.is_empty() {
            return None;
        }
        return Some(cleaned);
    }

    None
}

/// Parses an ASS/SSA timestamp in `H:MM:SS.cs` or `HH:MM:SS.cs` format into a `MediaTime`.
///
/// Centiseconds (.cs) represent hundredths of a second (10 milliseconds each).
pub fn parse_ass_timestamp(s: &str) -> Option<MediaTime> {
    let s = s.trim();
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 2 {
        return None;
    }

    let frac_str = parts[1].trim();
    let frac: u64 = frac_str.parse().ok()?;
    let ms = match frac_str.len() {
        1 => frac * 100,
        2 => frac * 10,
        3 => frac,
        _ => frac / 10u64.pow(frac_str.len() as u32 - 3),
    };

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

/// Parses an individual `Dialogue:` line from an ASS file.
pub fn parse_ass_dialogue_line(
    line: &str,
    start_col: usize,
    end_col: usize,
    text_col: usize,
    num_cols: usize,
) -> Option<SubtitleCue> {
    let trimmed = line.trim();
    let stripped = trimmed.strip_prefix("Dialogue:")?;
    let after_prefix = stripped.trim();

    // Split up to `num_cols - 1` times so the `Text` column retains any embedded commas
    let fields = split_n_commas(after_prefix, num_cols);
    if fields.len() <= text_col || fields.len() <= start_col || fields.len() <= end_col {
        return None;
    }

    let start = parse_ass_timestamp(fields[start_col])?;
    let end = parse_ass_timestamp(fields[end_col])?;

    if end < start {
        return None;
    }

    let cleaned_text = clean_ass_text(fields[text_col]);
    if cleaned_text.is_empty() {
        return None;
    }

    Some(SubtitleCue::new(start, end, cleaned_text))
}

/// Splits a string by comma up to `n - 1` times, returning at most `n` slices.
fn split_n_commas(s: &str, n: usize) -> Vec<&str> {
    if n <= 1 {
        return vec![s];
    }

    let mut result = Vec::with_capacity(n);
    let mut remainder = s;

    for _ in 0..n - 1 {
        if let Some(pos) = remainder.find(',') {
            result.push(&remainder[..pos]);
            remainder = &remainder[pos + 1..];
        } else {
            break;
        }
    }
    result.push(remainder);
    result
}

/// Parses full Advanced SubStation Alpha (.ass / .ssa) text into a sequence of `SubtitleCue`s.
///
/// Features:
/// - UTF-8 BOM stripping
/// - Dynamic `Format:` column extraction in `[Events]` section
/// - Strips ASS override blocks (`{\b1\pos(192,200)}`)
/// - Replaces `\N` with newlines
/// - Tolerant to comments and extra script headers
pub fn parse_ass(content: &str) -> Result<Vec<SubtitleCue>, SubtitleParseError> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut cues = Vec::new();

    let mut in_events = false;
    let mut start_col = 1;
    let mut end_col = 2;
    let mut text_col = 9;
    let mut num_cols = 10;

    for line in content.lines() {
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            in_events = line.eq_ignore_ascii_case("[Events]");
            continue;
        }

        if !in_events {
            continue;
        }

        if let Some(format_str) = line.strip_prefix("Format:") {
            let cols: Vec<&str> = format_str.split(',').map(|c| c.trim()).collect();
            num_cols = cols.len();
            for (idx, col) in cols.iter().enumerate() {
                if col.eq_ignore_ascii_case("Start") {
                    start_col = idx;
                } else if col.eq_ignore_ascii_case("End") {
                    end_col = idx;
                } else if col.eq_ignore_ascii_case("Text") {
                    text_col = idx;
                }
            }
            continue;
        }

        if line.starts_with("Dialogue:") {
            if let Some(cue) =
                parse_ass_dialogue_line(line, start_col, end_col, text_col, num_cols)
            {
                cues.push(cue);
            }
        }
    }

    cues.sort_by_key(|c| c.start);

    Ok(cues)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ass_standard() {
        let ass = r#"[Script Info]
Title: Sample ASS
ScriptType: v4.00+

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,2,10,10,10,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:01:20.50,0:01:23.75,Default,,0,0,0,,{\pos(192,200)\b1}Hello, world!\NThis is line two.
Comment: 0,0:01:24.00,0:01:25.00,Default,,0,0,0,,Ignored comment
Dialogue: 0,0:01:26.10,0:01:28.00,Default,,0,0,0,,Second dialogue with, embedded commas, intact!
"#;
        let cues = parse_ass(ass).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, MediaTime::from_millis(80500));
        assert_eq!(cues[0].end, MediaTime::from_millis(83750));
        assert_eq!(cues[0].text, "Hello, world!\nThis is line two.");

        assert_eq!(cues[1].start, MediaTime::from_millis(86100));
        assert_eq!(cues[1].end, MediaTime::from_millis(88000));
        assert_eq!(
            cues[1].text,
            "Second dialogue with, embedded commas, intact!"
        );
    }

    #[test]
    fn test_clean_ass_text() {
        let raw = r#"{\fad(200,200)\c&H0000FF&}Colored Text\NSecond Line\hWith Non-Breaking Space"#;
        let clean = clean_ass_text(raw);
        assert_eq!(clean, "Colored Text\nSecond Line With Non-Breaking Space");
    }

    #[test]
    fn test_clean_ass_text_drawing_mode_suppression() {
        // \p1 enters drawing mode, \p0 exits
        let drawing_only = r#"{\an7\pos(100,200)\p1}m 0 0 l 100 0 100 100 0 100{\p0}"#;
        assert_eq!(clean_ass_text(drawing_only), "");

        // Mixed text and \p2 ... \r reset
        let mixed = r#"Before{\p2\bord0}m 0 0 b 10 20 30 40 50 60{\r}After"#;
        assert_eq!(clean_ass_text(mixed), "BeforeAfter");

        // \pos and \pbo must NOT trigger drawing mode
        let pos_only = r#"{\pos(120,340)\pbo-5}Visible Dialogue\nNext Line"#;
        assert_eq!(clean_ass_text(pos_only), "Visible Dialogue\nNext Line");
    }

    #[test]
    fn test_parse_matroska_ass_packet_long_style_and_dialogue() {
        // Long style and speaker name (> 30 chars before 8th comma)
        let pkt = r#"42,0,VeryLongCustomAnimeDialogueStyleName,VeryLongSpeakerCharacterName,0010,0010,0020,,{\i1}Long style works!\NLine 2"#;
        assert_eq!(
            parse_matroska_ass_packet(pkt).as_deref(),
            Some("Long style works!\nLine 2")
        );

        // Drawing-only Matroska ASS packet returns None
        let drawing_pkt = r#"10,0,SignStyle,SignLayer,0,0,0,,{\p1}m 0 0 l 50 0 50 50 0 50{\p0}"#;
        assert_eq!(parse_matroska_ass_packet(drawing_pkt), None);

        // Full Dialogue: line
        let dialogue = r#"Dialogue: 0,0:01:00.00,0:01:02.50,Default,,0,0,0,,{\b1}Hello, world!"#;
        assert_eq!(
            parse_matroska_ass_packet(dialogue).as_deref(),
            Some("Hello, world!")
        );

        // Invalid ReadOrder (non-integer) returns None
        let invalid = "not_an_int,0,Default,,0,0,0,,Hello";
        assert_eq!(parse_matroska_ass_packet(invalid), None);
    }
}
