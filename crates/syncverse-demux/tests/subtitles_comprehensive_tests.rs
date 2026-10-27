//! Comprehensive integration tests for subtitle parsing (SRT, VTT, ASS/SSA),
//! format resiliency, edge cases (BOM, CRLF, tags, corrupted lines), and directory scanning.

use std::fs;
use std::io::Write;
use syncverse_common::subtitles::SubtitleSource;
use syncverse_common::time::MediaTime;
use syncverse_demux::subtitles::{
    clean_ass_text, clean_srt_text, clean_vtt_text, detect_subtitle_language, parse_ass, parse_srt,
    parse_subtitle_file, parse_vtt, scan_external_subtitles,
};

#[test]
fn test_srt_edge_cases_and_resilience() {
    // 1. Mixed CRLF / LF, UTF-8 BOM, empty blocks, multi-digit / single-digit hours
    let srt_content = "\u{feff}1\r\n0:00:05,050 --> 0:00:08,900\r\nFirst cue with short hour format\r\n\r\n\r\n2\n01:30:15,000 --> 01:30:20,500 X1:100 X2:200\nSecond cue with <i>italics</i> &amp; coords\n\n3\n99:99:99,999 --> 00:00:01,000\nInverted cue (should be skipped)\n\n4\n02:00:00,000 --> 02:00:05,000\nFourth cue after corrupt entry\n";

    let cues = parse_srt(srt_content).expect("parse_srt should succeed");
    assert_eq!(cues.len(), 3);

    // Cue 1
    assert_eq!(cues[0].start, MediaTime::from_millis(5050));
    assert_eq!(cues[0].end, MediaTime::from_millis(8900));
    assert_eq!(cues[0].text, "First cue with short hour format");

    // Cue 2
    let cue2_start_ms = (3600 + 30 * 60 + 15) * 1000;
    let cue2_end_ms = (3600 + 30 * 60 + 20) * 1000 + 500;
    assert_eq!(cues[1].start, MediaTime::from_millis(cue2_start_ms));
    assert_eq!(cues[1].end, MediaTime::from_millis(cue2_end_ms));
    assert_eq!(cues[1].text, "Second cue with italics & coords");

    // Cue 4 (cue 3 was rejected because end < start)
    let cue4_start_ms = 2 * 3600 * 1000;
    assert_eq!(cues[2].start, MediaTime::from_millis(cue4_start_ms));
    assert_eq!(cues[2].text, "Fourth cue after corrupt entry");
}

#[test]
fn test_srt_tag_cleaning() {
    let raw = "<font color=\"#00ff00\"><b>Green Bold</b></font> {y:i}Subtitle text{/y:i}";
    let cleaned = clean_srt_text(raw);
    assert_eq!(cleaned, "Green Bold Subtitle text");
}

#[test]
fn test_vtt_edge_cases_and_resilience() {
    let vtt_content = r#"WEBVTT - Demonstration Video

NOTE
This is a multi-line note block.
It should be skipped entirely by the parser.

STYLE
::cue {
    color: #fff;
}

cue-alpha
05:10.500 --> 05:14.250 align:left size:50%
<v John>Hello, WebVTT short-timestamp format!</v>

cue-beta
01:05:10.000 --> 01:05:15.000
<c.red.bold>Red and bold text</c> &lt;escaped&gt;
"#;

    let cues = parse_vtt(vtt_content).expect("parse_vtt should succeed");
    assert_eq!(cues.len(), 2);

    // First cue: 05:10.500 = 5 * 60 * 1000 + 10500 = 310500 ms
    assert_eq!(cues[0].start, MediaTime::from_millis(310500));
    assert_eq!(cues[0].end, MediaTime::from_millis(314250));
    assert_eq!(cues[0].text, "Hello, WebVTT short-timestamp format!");

    // Second cue: 01:05:10.000
    let cue2_start_ms = (3600 + 5 * 60 + 10) * 1000;
    assert_eq!(cues[1].start, MediaTime::from_millis(cue2_start_ms));
    assert_eq!(cues[1].text, "Red and bold text <escaped>");
}

#[test]
fn test_vtt_tag_cleaning() {
    let raw = "<v.narrator Speaker>Welcome to the <b>show</b>!</v>";
    let cleaned = clean_vtt_text(raw);
    assert_eq!(cleaned, "Welcome to the show!");
}

#[test]
fn test_ass_arbitrary_column_ordering_and_escapes() {
    // Non-standard column order in Format line (End before Start) with Text as last column
    let ass_content = r#"[Script Info]
ScriptType: v4.00+

[Events]
Format: Layer, End, Style, Start, Text
Dialogue: 0,0:00:04.50,Default,0:00:01.00,First text message with {\b1}bold{\b0}
Dialogue: 0,0:00:08.80,Default,0:00:05.20,Second text message with embedded, commas, and \Nlinebreak
"#;

    let cues = parse_ass(ass_content).expect("parse_ass should succeed");
    assert_eq!(cues.len(), 2);

    assert_eq!(cues[0].start, MediaTime::from_millis(1000));
    assert_eq!(cues[0].end, MediaTime::from_millis(4500));
    assert_eq!(cues[0].text, "First text message with bold");

    assert_eq!(cues[1].start, MediaTime::from_millis(5200));
    assert_eq!(cues[1].end, MediaTime::from_millis(8800));
    assert_eq!(
        cues[1].text,
        "Second text message with embedded, commas, and \nlinebreak"
    );
}

#[test]
fn test_ass_override_stripping() {
    let raw = r#"{\an8\pos(192,40)\fnArial\fs28\c&H00FFFF&\3c&H000000&}Top Centered Text\hWith\hSpaces"#;
    let clean = clean_ass_text(raw);
    assert_eq!(clean, "Top Centered Text With Spaces");
}

#[test]
fn test_language_detection_matrix() {
    let test_cases = [
        ("movie.en.srt", "movie", Some("en"), "English"),
        ("movie.eng.srt", "movie", Some("eng"), "English"),
        ("movie.es.vtt", "movie", Some("es"), "Spanish"),
        ("movie.de-DE.ass", "movie", Some("de-DE"), "German"),
        ("movie.fr.sdh.srt", "movie", Some("fr"), "French (SDH)"),
        ("movie.ja.forced.ass", "movie", Some("ja"), "Japanese (Forced)"),
        ("movie.zh-CN.srt", "movie", Some("zh-CN"), "Chinese"),
        ("movie.it.cc.vtt", "movie", Some("it"), "Italian (CC)"),
        ("movie.commentary.srt", "movie", None, "commentary"),
        ("movie.srt", "movie", None, "External Subtitle"),
    ];

    for (fname, stem, expected_lang, expected_title) in test_cases {
        let (lang, title) = detect_subtitle_language(fname, stem);
        assert_eq!(
            lang.as_deref(),
            expected_lang,
            "Failed language check for {fname}"
        );
        assert_eq!(
            title, expected_title,
            "Failed title check for {fname}"
        );
    }
}

#[test]
fn test_full_external_scanner_workflow() {
    let temp_dir = std::env::temp_dir().join(format!("syncverse_sub_suite_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let video_file = temp_dir.join("matrix_revolutions.mp4");
    fs::write(&video_file, b"sample mp4 binary").expect("write video file");

    let sub1 = temp_dir.join("matrix_revolutions.en.srt");
    let sub2 = temp_dir.join("matrix_revolutions.es.vtt");
    let sub3 = temp_dir.join("matrix_revolutions.ja.ass");
    let unrelated = temp_dir.join("matrix_reloaded.en.srt");

    let srt_body = "1\n00:00:01,000 --> 00:00:03,000\nEnglish line\n";
    let vtt_body = "WEBVTT\n\n00:00:04.000 --> 00:00:06.000\nLinea espanola\n";
    let ass_body = "[Script Info]\n[Events]\nFormat: Layer, Start, End, Text\nDialogue: 0,0:00:07.00,0:00:09.00,日本語テキスト\n";

    fs::File::create(&sub1).unwrap().write_all(srt_body.as_bytes()).unwrap();
    fs::File::create(&sub2).unwrap().write_all(vtt_body.as_bytes()).unwrap();
    fs::File::create(&sub3).unwrap().write_all(ass_body.as_bytes()).unwrap();
    fs::File::create(&unrelated).unwrap().write_all(srt_body.as_bytes()).unwrap();

    let tracks = scan_external_subtitles(&video_file);
    assert_eq!(tracks.len(), 3);

    // Verify track 0 (en)
    assert_eq!(tracks[0].source, SubtitleSource::External);
    assert_eq!(tracks[0].format, "srt");
    assert_eq!(tracks[0].language.as_deref(), Some("en"));
    assert_eq!(tracks[0].title, "English");
    assert_eq!(tracks[0].cues.len(), 1);
    assert_eq!(tracks[0].cues[0].text, "English line");

    // Verify track 1 (es)
    assert_eq!(tracks[1].format, "vtt");
    assert_eq!(tracks[1].language.as_deref(), Some("es"));
    assert_eq!(tracks[1].title, "Spanish");
    assert_eq!(tracks[1].cues[0].text, "Linea espanola");

    // Verify track 2 (ja)
    assert_eq!(tracks[2].format, "ass");
    assert_eq!(tracks[2].language.as_deref(), Some("ja"));
    assert_eq!(tracks[2].title, "Japanese");
    assert_eq!(tracks[2].cues[0].text, "日本語テキスト");

    // Test active_cue_at query across timestamps
    let en_track = &tracks[0];
    assert_eq!(en_track.active_cue_at(MediaTime::from_millis(500)), None);
    assert_eq!(
        en_track.active_cue_at(MediaTime::from_millis(2000)).map(|c| c.text.as_str()),
        Some("English line")
    );
    assert_eq!(en_track.active_cue_at(MediaTime::from_millis(3500)), None);

    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_parse_subtitle_file_direct() {
    let temp_file = std::env::temp_dir().join(format!("direct_test_{}.srt", std::process::id()));
    let content = "1\n00:00:10,000 --> 00:00:15,000\nDirect load test\n";
    fs::write(&temp_file, content).unwrap();

    let track = parse_subtitle_file(&temp_file, 42, None).expect("direct parse succeeds");
    assert_eq!(track.id, 42);
    assert_eq!(track.cues.len(), 1);
    assert_eq!(track.cues[0].text, "Direct load test");

    let _ = fs::remove_file(&temp_file);
}
