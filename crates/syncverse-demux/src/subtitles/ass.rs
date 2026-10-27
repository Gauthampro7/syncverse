//! Pure-Rust Advanced SubStation Alpha (.ass / .ssa) subtitle parser.

use syncverse_common::subtitles::SubtitleCue;
use syncverse_common::time::MediaTime;

use crate::subtitles::SubtitleParseError;

/// Strips ASS style override tags (`{\...}`) and replaces ASS escape sequences (`\N`, `\n`, `\h`).
pub fn clean_ass_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_override = false;
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if ch == '{' {
            in_override = true;
            i += 1;
            continue;
        }
        if ch == '}' {
            in_override = false;
            i += 1;
            continue;
        }

        if in_override {
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
}
