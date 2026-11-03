//! Pure-Rust MicroDVD (.sub) subtitle parser.
//!
//! Parses frame-based MicroDVD subtitles (`{start_frame}{end_frame}Subtitle text|Second line`),
//! detects embedded framerate headers (`{1}{1}23.976`), converts frame indices to `MediaTime`
//! presentation timestamps, and cleans styling control codes.

use syncverse_common::subtitles::SubtitleCue;
use syncverse_common::time::MediaTime;

use crate::subtitles::SubtitleParseError;

/// Strips MicroDVD style tags (`{y:...}`, `{c:...}`, `{P:...}`, `{o:...}`, `<...>`)
/// and replaces pipe characters (`|`) with standard newlines (`\n`).
///
/// Delimiters without a valid matching closing bracket are preserved as literal text
/// to prevent emoticons (e.g. `<3`) or comparison operators from swallowing dialogue.
pub fn clean_microdvd_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '{' => {
                // Look ahead for closing '}'
                if let Some(close_idx) = chars[i + 1..].iter().position(|&c| c == '}') {
                    let end = i + 1 + close_idx;
                    let inside = &chars[i + 1..end];
                    if !inside.contains(&'\n') && !inside.contains(&'|') {
                        i = end + 1;
                        continue;
                    }
                }
                out.push('{');
                i += 1;
            }
            '<' => {
                // Look ahead for closing '>'
                if let Some(close_idx) = chars[i + 1..].iter().position(|&c| c == '>') {
                    let end = i + 1 + close_idx;
                    let inside = &chars[i + 1..end];
                    if !inside.contains(&'\n') && !inside.contains(&'|') {
                        i = end + 1;
                        continue;
                    }
                }
                out.push('<');
                i += 1;
            }
            '|' => {
                out.push('\n');
                i += 1;
            }
            ch => {
                out.push(ch);
                i += 1;
            }
        }
    }

    out.trim().to_string()
}

/// Parses an individual MicroDVD line: `{start_frame}{end_frame}Text`.
///
/// Returns `Some((start_frame, end_frame, text_slice))` if the line matches the bracket pattern.
pub fn parse_microdvd_line(line: &str) -> Option<(u64, u64, &str)> {
    let trimmed = line.trim();
    if !trimmed.starts_with('{') {
        return None;
    }

    let end_first_bracket = trimmed.find('}')?;
    let rem = &trimmed[end_first_bracket + 1..];
    let start_second_offset = rem.find('{')?;

    // In MicroDVD syntax, the two frame brackets must be adjacent or separated only by whitespace.
    if !rem[..start_second_offset].trim().is_empty() {
        return None;
    }

    let start_second_bracket = end_first_bracket + 1 + start_second_offset;
    let end_second_bracket = start_second_bracket + trimmed[start_second_bracket..].find('}')?;

    let start_frame: u64 = trimmed[1..end_first_bracket].trim().parse().ok()?;
    let end_frame: u64 = trimmed[start_second_bracket + 1..end_second_bracket]
        .trim()
        .parse()
        .ok()?;

    let text = trimmed[end_second_bracket + 1..].trim();
    Some((start_frame, end_frame, text))
}

/// Parses full MicroDVD (.sub) text content into a chronologically sorted sequence of `SubtitleCue`s.
///
/// Handles:
/// - UTF-8 Byte Order Marks (BOM)
/// - Framerate extraction from `{1}{1}FPS` or `{0}{0}FPS` headers (supporting both '.' and ',' decimal separators)
/// - Fallback to `default_fps` (or 25.0 fps if not provided)
/// - Frame-to-millisecond conversion via integer-safe rational scaling
/// - Pipe `|` to newline translation and style tag stripping
/// - Rejection / skipping of inverted frame pairs and minimum 1-frame duration enforcement
pub fn parse_microdvd(
    content: &str,
    default_fps: Option<f64>,
) -> Result<Vec<SubtitleCue>, SubtitleParseError> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut fps = default_fps.unwrap_or(25.0);
    if fps <= 0.0 || !fps.is_finite() {
        fps = 25.0;
    }

    let mut cues = Vec::new();
    let mut is_first_line = true;

    for line in content.lines() {
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() {
            continue;
        }

        if let Some((start_frame, end_frame, text)) = parse_microdvd_line(line) {
            // Check if the very first cue is a framerate header: {1}{1}23.976 or {0}{0}25,000
            if is_first_line && (start_frame == 0 || start_frame == 1) && (end_frame == 0 || end_frame == 1) {
                let norm_fps_str = text.trim().replace(',', ".");
                if let Ok(parsed_fps) = norm_fps_str.parse::<f64>() {
                    if parsed_fps > 0.0 && parsed_fps <= 240.0 && parsed_fps.is_finite() {
                        fps = parsed_fps;
                        is_first_line = false;
                        continue;
                    }
                }
            }
            is_first_line = false;

            if end_frame < start_frame {
                continue;
            }

            let start_ms = ((start_frame as f64) * 1000.0 / fps).round() as u64;
            let frame_dur_ms = (1000.0 / fps).round().max(1.0) as u64;
            let mut end_ms = ((end_frame as f64) * 1000.0 / fps).round() as u64;
            if end_ms <= start_ms {
                end_ms = start_ms.saturating_add(frame_dur_ms);
            }

            let cleaned_text = clean_microdvd_text(text);
            if !cleaned_text.is_empty() {
                cues.push(SubtitleCue::new(
                    MediaTime::from_millis(start_ms),
                    MediaTime::from_millis(end_ms),
                    cleaned_text,
                ));
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
    fn test_clean_microdvd_text() {
        let raw = "{y:i}Hello {y:b}world!|Second line{c:$0000ff}";
        assert_eq!(clean_microdvd_text(raw), "Hello world!\nSecond line");

        let raw_tag = "<b>Bold</b>|<font color=\"red\">Red</font>";
        assert_eq!(clean_microdvd_text(raw_tag), "Bold\nRed");
    }

    #[test]
    fn test_parse_microdvd_with_fps_header() {
        let sub = "\
{1}{1}25.000
{25}{50}Hello world!
{75}{100}Line one|Line two
";
        let cues = parse_microdvd(sub, None).unwrap();
        assert_eq!(cues.len(), 2);

        // At 25 fps, 25 frames = 1000ms, 50 frames = 2000ms
        assert_eq!(cues[0].start.as_millis(), 1000);
        assert_eq!(cues[0].end.as_millis(), 2000);
        assert_eq!(cues[0].text, "Hello world!");

        // At 25 fps, 75 frames = 3000ms, 100 frames = 4000ms
        assert_eq!(cues[1].start.as_millis(), 3000);
        assert_eq!(cues[1].end.as_millis(), 4000);
        assert_eq!(cues[1].text, "Line one\nLine two");
    }

    #[test]
    fn test_parse_microdvd_with_custom_default_fps() {
        let sub = "\
{24}{48}24 fps movie cue
";
        let cues = parse_microdvd(sub, Some(24.0)).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].start.as_millis(), 1000);
        assert_eq!(cues[0].end.as_millis(), 2000);
        assert_eq!(cues[0].text, "24 fps movie cue");
    }

    #[test]
    fn test_parse_microdvd_malformed_and_inverted() {
        let sub = "\
{100}{50}Inverted cue should be ignored
{invalid}{100}Broken bracket
Not a microdvd line
{100} Some text between brackets {200} should be rejected
{200}{300}Valid cue
";
        let cues = parse_microdvd(sub, None).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "Valid cue");
    }

    #[test]
    fn test_clean_microdvd_text_unclosed_and_emoticons() {
        // Emoticons with '<' should not eat subsequent text
        assert_eq!(clean_microdvd_text("I love you <3 so much!"), "I love you <3 so much!");
        assert_eq!(clean_microdvd_text("Threshold < 50%"), "Threshold < 50%");

        // Unclosed '{' should not eat subsequent text
        assert_eq!(clean_microdvd_text("Don't leave {now"), "Don't leave {now");

        // European decimal comma in framerate header
        let sub_eu = "\
{1}{1}23,976
{24}{48}European framerate cue
";
        let cues = parse_microdvd(sub_eu, None).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "European framerate cue");
        assert!((cues[0].start.as_millis() as i64 - 1001).abs() <= 1);
    }
}
