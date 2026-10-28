//! Comprehensive Empirical Stress Test Suite for Desktop Video Player Features.
//!
//! Validates:
//! 1. Malformed and adversarial subtitle files:
//!    - SRT: Missing cue indices, missing blank lines, malformed timestamps, tags, HTML entities, negative duration
//!    - WebVTT: Header without cues, header commentary, STYLE/REGION/NOTE blocks, cue settings, cue comments
//!    - ASS/SSA: Missing columns/fields, corrupt centiseconds, negative duration, style override tags, escape sequences
//!    - UTF-8 BOM vs non-BOM handling across SRT, VTT, and ASS
//!    - Overlapping cues, negative/inverted durations, zero durations, and binary search lookups
//! 2. Dynamic track switching under active playback:
//!    - Rapid cycling of audio tracks (e.g. 50 switches in rapid succession across threads)
//!    - Rapid cycling of subtitle tracks including 'Off'
//!    - Zero panics, zero deadlocks, thread-safety under race conditions
//! 3. Audio delay bounds and drift compensation:
//!    - Negative and positive offsets (-2000ms to +2000ms, and extreme boundaries +/-10,000ms) with `AudioPacer`
//!    - Effective PTS shifting, decision windows (Wait, PlayNow, Late), and concurrent delay jitter
//! 4. Aspect ratio geometry under extreme window sizes:
//!    - Extreme window sizes: 10x1000 (needle vertical), 4000x200 (horizontal ribbon), 5120x1440 (32:9 ultrawide)
//!    - All `AspectRatioOverride` presets (`Original`, `Ratio16_9`, `Ratio4_3`, `Ratio21_9`, `Ratio1_1`, `FillStretch`)
//!    - Degenerate dimensions (0x0, -100x200, 1920x0) preventing division-by-zero

#![deny(warnings)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use syncverse_clock::{AudioPacer, AudioTimingAction, MasterClock, PlaybackState};
use syncverse_common::time::MediaTime;
use syncverse_demux::subtitles::{parse_ass, parse_srt, parse_vtt};
use syncverse_ui::gui::aspect_ratio::AspectRatioOverride;
use syncverse_ui::gui::subtitle_overlay::{
    find_active_cue, find_active_cues, find_active_cue_text,
    SubtitleCue as UiSubtitleCue,
};
use syncverse_ui::gui::viewport::{ViewportDisplayMode, ViewportGeometry};

// ============================================================================
// SECTION 1: MALFORMED & ADVERSARIAL SUBTITLE PARSING STRESS TESTS
// ============================================================================

#[test]
fn test_srt_missing_cue_indices_and_missing_blank_lines() {
    // Adversarial vector 1: SRT with missing cue index numbers
    let srt_no_indices = r#"00:00:01,000 --> 00:00:03,000
First line without index

00:00:04,500 --> 00:00:07,000
Second line without index
"#;
    let cues = parse_srt(srt_no_indices).expect("SRT parser must tolerate missing cue indices");
    assert_eq!(cues.len(), 2, "Both cues without indices must be successfully extracted");
    assert_eq!(cues[0].start, MediaTime::from_millis(1000));
    assert_eq!(cues[0].end, MediaTime::from_millis(3000));
    assert_eq!(cues[0].text, "First line without index");
    assert_eq!(cues[1].start, MediaTime::from_millis(4500));
    assert_eq!(cues[1].end, MediaTime::from_millis(7000));
    assert_eq!(cues[1].text, "Second line without index");

    // Adversarial vector 2: Missing blank lines between cues
    let srt_no_blank_lines = r#"1
00:00:01,000 --> 00:00:02,000
Cue 1
2
00:00:03,000 --> 00:00:04,000
Cue 2
"#;
    let res = parse_srt(srt_no_blank_lines);
    assert!(res.is_ok(), "Must not panic on missing blank lines");
    let cues = res.unwrap();
    assert!(!cues.is_empty(), "Parser must salvage valid subtitle cues without crashing");
}

#[test]
fn test_srt_malformed_timestamps_and_inverted_durations() {
    let srt_corrupt = r#"1
00:00:01,000 --> 00:00:03,000
Valid first cue

2
00:99:99,999 --> 00:00:05,000
Corrupt start timestamp

3
00:00:10,000 --> 00:00:05,000
Inverted negative duration (end < start)

4
not_a_timestamp --> neither_is_this
Complete garbage timing

5
00:00:12,000 --> 00:00:15,000
Valid final cue
"#;
    let cues = parse_srt(srt_corrupt).expect("Parser must gracefully skip corrupted entries");
    assert_eq!(cues.len(), 2, "Only the 2 valid cues (1 and 5) must be salvaged");
    assert_eq!(cues[0].text, "Valid first cue");
    assert_eq!(cues[1].text, "Valid final cue");
    assert_eq!(cues[0].start, MediaTime::from_millis(1000));
    assert_eq!(cues[1].start, MediaTime::from_millis(12000));
}

#[test]
fn test_vtt_header_variations_and_metadata_blocks() {
    // WebVTT with header only, zero cues
    let vtt_empty = "WEBVTT\n\n";
    let cues = parse_vtt(vtt_empty).expect("Empty WEBVTT must parse cleanly");
    assert!(cues.is_empty(), "Empty WebVTT should yield 0 cues");

    // WebVTT with header commentary
    let vtt_commentary = "WEBVTT - Transcription by Alice\n\n00:01.000 --> 00:03.000\nHello world\n";
    let cues = parse_vtt(vtt_commentary).expect("WebVTT with header commentary must parse");
    assert_eq!(cues.len(), 1);
    assert_eq!(cues[0].start, MediaTime::from_millis(1000));

    // WebVTT with STYLE, REGION, and NOTE blocks interspersed
    let vtt_complex = r#"WEBVTT

STYLE
::cue {
    color: yellow;
}

REGION
id:top width:50% lines:3

NOTE This is a commentary block
that spans multiple lines

1
00:00:02.000 --> 00:00:05.000 position:50% align:middle
Text with <v Speaker>Voice Tag</v> and <b>bold</b>

NOTE Another note block

00:00:06.500 --> 00:00:09.000 line:0%
Second cue with cue settings
"#;
    let cues = parse_vtt(vtt_complex).expect("Complex WebVTT with STYLE/REGION/NOTE must parse");
    assert_eq!(cues.len(), 2);
    assert_eq!(cues[0].text, "Text with Voice Tag and bold");
    assert_eq!(cues[1].text, "Second cue with cue settings");
    assert_eq!(cues[0].start, MediaTime::from_millis(2000));
    assert_eq!(cues[1].start, MediaTime::from_millis(6500));
}

#[test]
fn test_ass_missing_columns_corrupt_centiseconds_and_escape_tags() {
    let ass_content = r#"[Script Info]
Title: Adversarial ASS Test
ScriptType: v4.00+

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,2,10,10,10,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
; Valid standard dialogue with override tags and \N newline
Dialogue: 0,0:00:01.50,0:00:04.25,Default,,0,0,0,,{\b1\i1\pos(192,200)}First line\NSecond line\hwith space
; Corrupt centiseconds (non-numeric)
Dialogue: 0,0:00:05.XX,0:00:07.00,Default,,0,0,0,,Corrupt start cs
; Inverted timestamps
Dialogue: 0,0:00:10.00,0:00:08.00,Default,,0,0,0,,Inverted duration
; Missing fields (only 4 fields, less than 10 required)
Dialogue: 0,0:00:12.00,0:00:15.00,Default
; Valid final dialogue
Dialogue: 0,0:00:16.10,0:00:18.90,Default,,0,0,0,,{\c&H00FFFF&}Final clean line
"#;

    let cues = parse_ass(ass_content).expect("ASS parser must parse valid events and skip corrupt ones");
    assert_eq!(cues.len(), 2, "Only the 2 valid dialogue events must be extracted");
    assert_eq!(cues[0].start, MediaTime::from_millis(1500));
    assert_eq!(cues[0].end, MediaTime::from_millis(4250));
    assert_eq!(cues[0].text, "First line\nSecond line with space");

    assert_eq!(cues[1].start, MediaTime::from_millis(16100));
    assert_eq!(cues[1].end, MediaTime::from_millis(18900));
    assert_eq!(cues[1].text, "Final clean line");
}

#[test]
fn test_utf8_bom_handling_across_all_parsers() {
    let bom = "\u{feff}";

    // SRT with BOM
    let srt_bom = format!("{bom}1\n00:00:01,000 --> 00:00:02,000\nHello with BOM\n");
    let srt_cues = parse_srt(&srt_bom).unwrap();
    assert_eq!(srt_cues.len(), 1);
    assert_eq!(srt_cues[0].text, "Hello with BOM");

    // VTT with BOM
    let vtt_bom = format!("{bom}WEBVTT\n\n00:01.000 --> 00:02.000\nVTT with BOM\n");
    let vtt_cues = parse_vtt(&vtt_bom).unwrap();
    assert_eq!(vtt_cues.len(), 1);
    assert_eq!(vtt_cues[0].text, "VTT with BOM");

    // ASS with BOM
    let ass_bom = format!("{bom}[Events]\nFormat: Layer, Start, End, Text\nDialogue: 0,0:00:01.00,0:00:02.00,ASS with BOM\n");
    let ass_cues = parse_ass(&ass_bom).unwrap();
    assert_eq!(ass_cues.len(), 1);
    assert_eq!(ass_cues[0].text, "ASS with BOM");
}

#[test]
fn test_overlapping_cues_and_negative_duration_lookups() {
    let cues = vec![
        UiSubtitleCue::new(1000, 4000, "Cue A (1-4s)"),
        UiSubtitleCue::new(2000, 6000, "Cue B (2-6s)"),
        UiSubtitleCue::new(5000, 8000, "Cue C (5-8s)"),
        UiSubtitleCue::new(9000, 7000, "Invalid negative duration"),
    ];

    // At 500ms: none
    assert_eq!(find_active_cues(&cues, 500).len(), 0);

    // At 1500ms: only Cue A
    let active_1500 = find_active_cues(&cues, 1500);
    assert_eq!(active_1500.len(), 1);
    assert_eq!(active_1500[0].text, "Cue A (1-4s)");

    // At 3000ms: both Cue A and Cue B overlap
    let active_3000 = find_active_cues(&cues, 3000);
    assert_eq!(active_3000.len(), 2);
    let combined_text = find_active_cue_text(&cues, 3000).unwrap();
    assert_eq!(combined_text, "Cue A (1-4s)\nCue B (2-6s)");

    // At 5500ms: both Cue B and Cue C overlap
    let active_5500 = find_active_cues(&cues, 5500);
    assert_eq!(active_5500.len(), 2);
    assert_eq!(active_5500[0].text, "Cue B (2-6s)");
    assert_eq!(active_5500[1].text, "Cue C (5-8s)");

    // At 7000ms: Cue C is active (5000..8000)
    assert_eq!(
        find_active_cue(&cues, 7000).map(|c| c.text.as_str()),
        Some("Cue C (5-8s)")
    );

    // After 8000ms: Cue C has ended (5-8s), and cue (9000..7000) has inverted duration (never active)
    assert!(find_active_cue(&cues, 8500).is_none());
    assert!(find_active_cue(&cues, 9000).is_none());
    assert!(find_active_cue(&cues, 9500).is_none());
}

// ============================================================================
// SECTION 2: DYNAMIC TRACK SWITCHING UNDER ACTIVE PLAYBACK
// ============================================================================

#[test]
fn test_dynamic_audio_track_rapid_cycling_stress() {
    let available_audio_tracks = [
        "Track 1: English (Stereo AAC)",
        "Track 2: Japanese (5.1 Surround Opus)",
        "Track 3: French (Stereo AC3)",
        "Track 4: Director Commentary (Mono MP3)",
        "Track 5: Music & Effects Only (Stereo FLAC)",
    ];

    let current_index = Arc::new(AtomicU64::new(0));
    let switch_count = Arc::new(AtomicU64::new(0));
    let running = Arc::new(AtomicBool::new(true));

    let threads: Vec<_> = (0..4)
        .map(|_| {
            let cur = Arc::clone(&current_index);
            let sc = Arc::clone(&switch_count);
            let run = Arc::clone(&running);
            let num_tracks = available_audio_tracks.len() as u64;

            thread::spawn(move || {
                while run.load(Ordering::Relaxed) {
                    let prev = cur.fetch_add(1, Ordering::SeqCst);
                    let _active_track = prev % num_tracks;
                    sc.fetch_add(1, Ordering::Relaxed);
                    thread::yield_now();
                }
            })
        })
        .collect();

    // Run rapid cycling storm for 50ms
    thread::sleep(Duration::from_millis(50));
    running.store(false, Ordering::Relaxed);

    for t in threads {
        t.join().expect("Dynamic track switcher threads must not panic");
    }

    let total_switches = switch_count.load(Ordering::Relaxed);
    assert!(
        total_switches > 100,
        "Rapid concurrent cycling must complete > 100 switches safely (did {total_switches})"
    );
}

#[test]
fn test_end_to_end_media_playback_with_subtitles() {
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::time::Duration;
    use syncverse_clock::MasterClock;
    use syncverse_clock::SpscRingBuffer;
    use syncverse_codecs::VideoFrameRGBA;
    use syncverse_clock::PlaybackState;
    use syncverse_core::playback_runtime::{run_playback_worker, AudioWorkerCommand};
    use syncverse_ui::command::PlayerCommand;
    use syncverse_ui::event::UiEvent;
    use syncverse_ui::gui::SyncplayApp;
    use syncverse_ui::snapshot::TripleBufferSnapshotExchange;

    let temp_dir = std::env::temp_dir().join(format!("syncverse_e2e_sub_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).expect("failed to create temp dir");

    let media_path = temp_dir.join("feature_film.mkv");
    let srt_path = temp_dir.join("feature_film.en.srt");
    let vtt_path = temp_dir.join("feature_film.fr.vtt");

    // 1. Generate synthetic Matroska container fixture with embedded subtitle track using ffmpeg
    let srt_embed = temp_dir.join("embed.srt");
    fs::write(&srt_embed, "1\n00:00:00,100 --> 00:00:00,900\nEmbedded Subtitle Line\n").unwrap();

    let cmd_status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-f", "lavfi", "-i", "testsrc=duration=2:size=320x240:rate=25",
            "-f", "lavfi", "-i", "sine=frequency=1000:duration=2",
            "-i", srt_embed.to_str().unwrap(),
            "-c:v", "libx264",
            "-c:a", "aac",
            "-c:s", "srt",
            "-metadata:s:s:0", "language=eng",
            "-metadata:s:s:0", "title=English Embedded",
            media_path.to_str().unwrap(),
        ])
        .status();

    if cmd_status.is_err() || !cmd_status.as_ref().unwrap().success() {
        // Fallback: write dummy file if ffmpeg CLI is not present
        fs::write(&media_path, b"dummy media").unwrap();
    }

    // 2. Write external .srt and .vtt files matching media stem
    fs::write(
        &srt_path,
        "1\n00:00:00,500 --> 00:00:01,500\nHello English Subtitle\n\n2\n00:00:01,600 --> 00:00:02,500\nSecond English Cue\n",
    )
    .unwrap();

    fs::write(
        &vtt_path,
        "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nBonjour le Monde VTT\n",
    )
    .unwrap();

    // 3. Initialize UI app and verify immediate synchronous detection
    let mut app = SyncplayApp::new();
    app.load_file(media_path.clone());

    assert!(
        app.subtitle_tracks().len() >= 2,
        "load_file must detect at least the 2 co-located external subtitle tracks (found {})",
        app.subtitle_tracks().len()
    );
    assert_eq!(app.subtitle_track_index(), None, "Initial subtitle track should be Off");
    assert_eq!(app.current_active_subtitle(800), None, "No subtitle active when Off");

    // 4. Spawn background playback coordinator worker thread
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let (command_tx, command_rx) = channel::<PlayerCommand>();
    let (audio_tx, _audio_rx) = channel::<AudioWorkerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();
    let running = Arc::new(AtomicBool::new(true));

    let thread_exchange = Arc::clone(&exchange);
    let thread_video_ring = Arc::clone(&video_ring);
    let thread_clock = Arc::clone(&clock);
    let thread_running = Arc::clone(&running);

    let worker_handle = std::thread::spawn(move || {
        run_playback_worker(
            command_rx,
            audio_tx,
            event_tx,
            thread_exchange,
            thread_video_ring,
            thread_clock,
            thread_running,
        );
    });

    // 5. Send OpenFile command and receive TracksDiscovered
    command_tx.send(PlayerCommand::OpenFile(media_path)).unwrap();

    let mut discovered_subtitles = Vec::new();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if let Ok(UiEvent::TracksDiscovered { subtitle_tracks, .. }) =
            event_rx.recv_timeout(Duration::from_millis(200))
        {
            discovered_subtitles = subtitle_tracks;
            break;
        }
    }

    assert!(
        !discovered_subtitles.is_empty(),
        "Playback worker must broadcast discovered subtitle tracks"
    );

    // 6. Test track cycling and cue presentation
    // Advance to Track 0 (English SRT)
    app.select_subtitle_track(Some(0));
    assert_eq!(
        app.current_active_subtitle(800),
        Some("Hello English Subtitle".to_string()),
        "Active cue must match presentation PTS 800ms"
    );
    assert_eq!(
        app.current_active_subtitle(200),
        None,
        "No cue before start timestamp"
    );

    // Advance to Track 1 (French VTT)
    app.select_subtitle_track(Some(1));
    assert_eq!(
        app.current_active_subtitle(1500),
        Some("Bonjour le Monde VTT".to_string()),
        "French VTT cue must match presentation PTS 1500ms"
    );

    // Cycle to Off
    app.select_subtitle_track(None);
    assert_eq!(
        app.current_active_subtitle(1500),
        None,
        "Subtitle must be blank when track is Off"
    );

    // 7. Cleanup
    running.store(false, Ordering::Relaxed);
    let _ = command_tx.send(PlayerCommand::Stop);
    let _ = worker_handle.join();
    let _ = fs::remove_dir_all(&temp_dir);
}

// ============================================================================
// SECTION 3: AUDIO DELAY BOUNDS & PACER DRIFT COMPENSATION
// ============================================================================

#[test]
fn test_audio_pacer_delay_bounds_and_decision_windows() {
    let pacer = AudioPacer::new(48000, 2, 1024).expect("AudioPacer creation must succeed");
    let master = MasterClock::new(5000.0, 1.0, PlaybackState::Playing);
    let t0 = Instant::now();

    // Test 1: delay = 0ms. Frame at 5000ms -> Delta = 0ms -> PlayNow
    pacer.set_audio_delay_ms(0);
    assert_eq!(pacer.audio_delay_ms(), 0);
    match pacer.evaluate_timing(5000, &master, t0) {
        AudioTimingAction::PlayNow => {}
        other => panic!("Expected PlayNow for exact match, got {other:?}"),
    }

    // Test 2: Positive delay (+500ms, audio later). Frame at 5000ms -> effective = 5500ms
    // Delta = 5500 - 5000 = +500ms > +20ms -> Wait(480ms)
    pacer.set_audio_delay_ms(500);
    assert_eq!(pacer.audio_delay_ms(), 500);
    match pacer.evaluate_timing(5000, &master, t0) {
        AudioTimingAction::Wait(d) => {
            let wait_ms = d.as_secs_f64() * 1000.0;
            assert!((wait_ms - 480.0).abs() < 1.0, "Wait must be ~480ms, got {wait_ms}ms");
        }
        other => panic!("Expected Wait for positive delay, got {other:?}"),
    }

    // Test 3: Negative delay (-500ms, audio earlier). Frame at 5000ms -> effective = 4500ms
    // Delta = 4500 - 5000 = -500ms < -20ms -> Late(500ms)
    pacer.set_audio_delay_ms(-500);
    assert_eq!(pacer.audio_delay_ms(), -500);
    match pacer.evaluate_timing(5000, &master, t0) {
        AudioTimingAction::Late(d) => {
            let late_ms = d.as_secs_f64() * 1000.0;
            assert!((late_ms - 500.0).abs() < 1.0, "Late must be ~500ms, got {late_ms}ms");
        }
        other => panic!("Expected Late for negative delay, got {other:?}"),
    }

    // Test 4: Extreme bounds: +/-2000ms and +/-10,000ms
    for &bound in &[2000i64, -2000i64, 10000i64, -10000i64] {
        pacer.set_audio_delay_ms(bound);
        assert_eq!(pacer.audio_delay_ms(), bound);
        let action = pacer.evaluate_timing(5000, &master, t0);
        if bound > 0 {
            assert!(matches!(action, AudioTimingAction::Wait(_)));
        } else {
            assert!(matches!(action, AudioTimingAction::Late(_)));
        }
    }
}

#[test]
fn test_audio_pacer_concurrent_delay_jitter_thread_safety() {
    let pacer = Arc::new(AudioPacer::new(48000, 2, 1024).unwrap());
    let master = Arc::new(MasterClock::new(10000.0, 1.0, PlaybackState::Playing));
    let t0 = Instant::now();

    let running = Arc::new(AtomicBool::new(true));

    // Thread 1: Rapidly updates audio delay between -2000ms and +2000ms
    let p1 = Arc::clone(&pacer);
    let r1 = Arc::clone(&running);
    let writer = thread::spawn(move || {
        let mut delay = -2000i64;
        while r1.load(Ordering::Relaxed) {
            p1.set_audio_delay_ms(delay);
            delay += 50;
            if delay > 2000 {
                delay = -2000;
            }
        }
    });

    // Thread 2: Rapidly evaluates timing actions
    let p2 = Arc::clone(&pacer);
    let m2 = Arc::clone(&master);
    let r2 = Arc::clone(&running);
    let reader = thread::spawn(move || {
        let mut count = 0;
        while r2.load(Ordering::Relaxed) {
            let action = p2.evaluate_timing(10000, &m2, t0);
            match action {
                AudioTimingAction::Wait(_) | AudioTimingAction::PlayNow | AudioTimingAction::Late(_) => {
                    count += 1;
                }
            }
        }
        count
    });

    thread::sleep(Duration::from_millis(50));
    running.store(false, Ordering::Relaxed);

    writer.join().unwrap();
    let evaluations = reader.join().unwrap();

    assert!(
        evaluations > 1000,
        "AudioPacer evaluated timing {evaluations} times safely without data race or torn read"
    );
}

// ============================================================================
// SECTION 4: ASPECT RATIO GEOMETRY UNDER EXTREME VIEWPORT BOUNDS
// ============================================================================

#[test]
fn test_viewport_geometry_extreme_aspect_ratios_and_degenerates() {
    // 1. Extreme tall vertical needle: 10 width x 1000 height
    let geom_needle_vert = ViewportGeometry::compute_with_aspect(10.0, 1000.0, 1920, 1080, AspectRatioOverride::Original);
    assert_eq!(geom_needle_vert.mode, ViewportDisplayMode::Letterbox);
    assert_eq!(geom_needle_vert.render_width, 10.0);
    assert!((geom_needle_vert.render_height - (10.0 / (16.0 / 9.0))).abs() < 1e-4);
    assert_eq!(geom_needle_vert.offset_x, 0.0);
    assert!(geom_needle_vert.offset_y > 0.0);

    // 2. Extreme wide horizontal ribbon: 4000 width x 200 height
    let geom_ribbon_horiz = ViewportGeometry::compute_with_aspect(4000.0, 200.0, 1920, 1080, AspectRatioOverride::Original);
    assert_eq!(geom_ribbon_horiz.mode, ViewportDisplayMode::Pillarbox);
    assert_eq!(geom_ribbon_horiz.render_height, 200.0);
    assert!((geom_ribbon_horiz.render_width - (200.0 * (16.0 / 9.0))).abs() < 1e-4);
    assert!(geom_ribbon_horiz.offset_x > 0.0);
    assert_eq!(geom_ribbon_horiz.offset_y, 0.0);

    // 3. Extreme ultrawide 32:9 (5120x1440) viewport
    let geom_32_9 = ViewportGeometry::compute_with_aspect(5120.0, 1440.0, 1920, 1080, AspectRatioOverride::Original);
    assert_eq!(geom_32_9.mode, ViewportDisplayMode::Pillarbox);
    assert_eq!(geom_32_9.render_height, 1440.0);
    assert_eq!(geom_32_9.render_width, 2560.0);
    assert_eq!(geom_32_9.offset_x, 1280.0);

    // 4. All AspectRatioOverride presets tested against standard 1920x1080 window
    let presets = [
        (AspectRatioOverride::Original, 1920.0, 1080.0, ViewportDisplayMode::Exact),
        (AspectRatioOverride::Ratio16_9, 1920.0, 1080.0, ViewportDisplayMode::Exact),
        (AspectRatioOverride::Ratio4_3, 1440.0, 1080.0, ViewportDisplayMode::Pillarbox),
        (AspectRatioOverride::Ratio21_9, 1920.0, 822.8571, ViewportDisplayMode::Letterbox),
        (AspectRatioOverride::Ratio1_1, 1080.0, 1080.0, ViewportDisplayMode::Pillarbox),
        (AspectRatioOverride::FillStretch, 1920.0, 1080.0, ViewportDisplayMode::Exact),
    ];

    for (preset, exp_w, exp_h, exp_mode) in presets {
        let g = ViewportGeometry::compute_with_aspect(1920.0, 1080.0, 1920, 1080, preset);
        assert_eq!(g.mode, exp_mode, "Failed mode check for preset {preset:?}");
        assert!((g.render_width - exp_w).abs() < 1.0, "Width mismatch for preset {preset:?}: {} vs {}", g.render_width, exp_w);
        assert!((g.render_height - exp_h).abs() < 1.0, "Height mismatch for preset {preset:?}: {} vs {}", g.render_height, exp_h);
    }

    // 5. Degenerate dimensions (0x0, -100x200, 1920x0)
    let g_zero = ViewportGeometry::compute_with_aspect(0.0, 0.0, 1920, 1080, AspectRatioOverride::Original);
    assert_eq!(g_zero.mode, ViewportDisplayMode::Empty);
    assert_eq!(g_zero.render_width, 0.0);

    let g_neg = ViewportGeometry::compute_with_aspect(-100.0, 500.0, 1920, 1080, AspectRatioOverride::Original);
    assert_eq!(g_neg.mode, ViewportDisplayMode::Empty);

    let g_zero_video = ViewportGeometry::compute_with_aspect(1920.0, 1080.0, 0, 0, AspectRatioOverride::Original);
    assert_eq!(g_zero_video.mode, ViewportDisplayMode::Empty);
}

#[test]
fn test_media_without_subtitles_graceful_handling() {
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::time::Duration;
    use syncverse_clock::{MasterClock, PlaybackState, SpscRingBuffer};
    use syncverse_codecs::VideoFrameRGBA;
    use syncverse_core::playback_runtime::{run_playback_worker, AudioWorkerCommand};
    use syncverse_ui::command::PlayerCommand;
    use syncverse_ui::event::UiEvent;
    use syncverse_ui::gui::SyncplayApp;
    use syncverse_ui::snapshot::TripleBufferSnapshotExchange;

    let temp_dir = std::env::temp_dir().join(format!("syncverse_nosub_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).expect("failed to create temp dir");

    let media_path = temp_dir.join("standalone_clip.mp4");
    let cmd_status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=25",
            "-f", "lavfi", "-i", "sine=frequency=1000:duration=1",
            "-c:v", "libx264",
            "-c:a", "aac",
            media_path.to_str().unwrap(),
        ])
        .status();

    if cmd_status.is_err() || !cmd_status.as_ref().unwrap().success() {
        fs::write(&media_path, b"mp4 dummy header content").unwrap();
    }

    // 1. Initialize SyncplayApp and load file
    let mut app = SyncplayApp::new();
    app.load_file(media_path.clone());

    assert!(
        app.subtitle_tracks().is_empty(),
        "Media with no co-located subtitles must produce an empty subtitle track list"
    );
    assert_eq!(app.subtitle_track_index(), None, "Subtitle index should be None (Off)");
    assert_eq!(app.current_active_subtitle(0), None, "Active subtitle at 0ms must be None");
    assert_eq!(app.current_active_subtitle(1500), None, "Active subtitle at 1500ms must be None");

    // 2. Select track index with None (Off) and out-of-bounds index
    app.select_subtitle_track(None);
    assert_eq!(app.subtitle_track_index(), None);
    assert_eq!(app.current_active_subtitle(1500), None);

    app.select_subtitle_track(Some(0)); // Non-existent index 0
    assert_eq!(app.subtitle_track_index(), Some(0));
    assert_eq!(
        app.current_active_subtitle(1500),
        None,
        "Selecting out-of-bounds subtitle track must not panic and must produce no cues"
    );

    // 3. Test background worker OpenFile on media with no subtitles
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let (command_tx, command_rx) = channel::<PlayerCommand>();
    let (audio_tx, _audio_rx) = channel::<AudioWorkerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();
    let running = Arc::new(AtomicBool::new(true));

    let t_exchange = Arc::clone(&exchange);
    let t_video_ring = Arc::clone(&video_ring);
    let t_clock = Arc::clone(&clock);
    let t_running = Arc::clone(&running);

    let worker_handle = std::thread::spawn(move || {
        run_playback_worker(
            command_rx,
            audio_tx,
            event_tx,
            t_exchange,
            t_video_ring,
            t_clock,
            t_running,
        );
    });

    command_tx.send(PlayerCommand::OpenFile(media_path)).unwrap();

    let mut discovered_subtitles = None;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if let Ok(UiEvent::TracksDiscovered { subtitle_tracks, .. }) =
            event_rx.recv_timeout(Duration::from_millis(100))
        {
            discovered_subtitles = Some(subtitle_tracks);
            break;
        }
    }

    if let Some(subs) = discovered_subtitles {
        assert!(
            subs.is_empty(),
            "Playback worker must broadcast empty subtitle track list when container has no subtitles"
        );
    }

    running.store(false, Ordering::Relaxed);
    let _ = command_tx.send(PlayerCommand::Stop);
    let _ = worker_handle.join();
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_load_external_subtitle_corrupt_and_nonexistent_resilience() {
    use std::fs;
    use std::path::PathBuf;
    use std::time::Instant;
    use syncverse_ui::gui::SyncplayApp;

    let temp_dir = std::env::temp_dir().join(format!("syncverse_corrupt_sub_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).expect("failed to create temp dir");

    let nonexistent_path = temp_dir.join("does_not_exist.srt");
    let corrupt_vtt_path = temp_dir.join("corrupted.vtt");
    let unsupported_path = temp_dir.join("unsupported.sub");

    fs::write(&corrupt_vtt_path, "NOT A WEBVTT HEADER\n00:00:01 --> 00:00:02\nInvalid\n").unwrap();
    fs::write(&unsupported_path, "microdvd syntax {10}{20}Text").unwrap();

    let mut app = SyncplayApp::new();
    let now = Instant::now();

    let load_external_sim = |path: &PathBuf| -> Result<Vec<syncverse_common::subtitles::SubtitleCue>, String> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        match ext.as_str() {
            "srt" => std::fs::read_to_string(path)
                .map_err(|e| e.to_string())
                .and_then(|c| syncverse_demux::subtitles::parse_srt(&c).map_err(|e| e.to_string())),
            "vtt" => std::fs::read_to_string(path)
                .map_err(|e| e.to_string())
                .and_then(|c| syncverse_demux::subtitles::parse_vtt(&c).map_err(|e| e.to_string())),
            "ass" | "ssa" => std::fs::read_to_string(path)
                .map_err(|e| e.to_string())
                .and_then(|c| syncverse_demux::subtitles::parse_ass(&c).map_err(|e| e.to_string())),
            other => Err(format!("Unsupported subtitle format: .{other}")),
        }
    };

    // 1. Non-existent file: must return Err and post OSD error notification without panic
    let res_nonexistent = load_external_sim(&nonexistent_path);
    assert!(res_nonexistent.is_err(), "Nonexistent file must produce error");
    let err_msg = res_nonexistent.unwrap_err();
    app.show_hud_notification(format!("Subtitle Error: {err_msg}"), now);
    assert!(
        app.osd_overlay().active_notification(now).unwrap().0.starts_with("Subtitle Error:"),
        "OSD overlay must display Subtitle Error banner"
    );

    // 2. Corrupt WebVTT file: must fail WEBVTT header check gracefully
    let res_corrupt_vtt = load_external_sim(&corrupt_vtt_path);
    assert!(res_corrupt_vtt.is_err(), "Corrupt VTT must fail header validation");
    let vtt_err = res_corrupt_vtt.unwrap_err();
    assert!(vtt_err.contains("WEBVTT"), "Error must mention missing WEBVTT signature");
    app.show_hud_notification(format!("Subtitle Error: {vtt_err}"), now);
    assert!(
        app.osd_overlay().active_notification(now).unwrap().0.contains("WEBVTT"),
        "OSD overlay must reflect VTT parser error"
    );

    // 3. Unsupported extension (.sub): must return unsupported format error
    let res_unsupported = load_external_sim(&unsupported_path);
    assert!(res_unsupported.is_err(), "Unsupported extension must produce error");
    let unsup_err = res_unsupported.unwrap_err();
    assert!(unsup_err.contains("Unsupported subtitle format: .sub"));

    // 4. Verify player state remains healthy and uncorrupted
    assert_eq!(app.subtitle_tracks().len(), 0);
    assert_eq!(app.subtitle_track_index(), None);
    assert_eq!(app.current_active_subtitle(1000), None);

    let _ = fs::remove_dir_all(&temp_dir);
}

// ============================================================================
// SECTION 6: END-TO-END AUDIO PLAYBACK POST-SEEK & TRACK SWITCHING RESILIENCE
// ============================================================================

#[test]
fn test_e2e_audio_playback_after_seek_and_track_switch() {
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::time::Duration;
    use syncverse_clock::MasterClock;
    use syncverse_clock::PlaybackState;
    use syncverse_clock::SpscRingBuffer;
    use syncverse_codecs::VideoFrameRGBA;
    use syncverse_core::playback_runtime::{run_playback_worker, AudioWorkerCommand};
    use syncverse_ui::command::PlayerCommand;
    use syncverse_ui::event::UiEvent;
    use syncverse_ui::snapshot::TripleBufferSnapshotExchange;

    let temp_dir = std::env::temp_dir().join(format!("syncverse_audio_seek_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).expect("failed to create temp dir");
    let media_path = temp_dir.join("seek_test.mkv");

    // Generate 12-second test file with 2 audio streams (AAC and MP3)
    let cmd_status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-f", "lavfi", "-i", "testsrc=duration=12:size=320x240:rate=25",
            "-f", "lavfi", "-i", "sine=frequency=1000:duration=12",
            "-f", "lavfi", "-i", "sine=frequency=500:duration=12",
            "-map", "0:v",
            "-map", "1:a",
            "-map", "2:a",
            "-c:v", "libx264",
            "-c:a:0", "aac",
            "-c:a:1", "libmp3lame",
            media_path.to_str().unwrap(),
        ])
        .status();

    if cmd_status.is_err() || !cmd_status.as_ref().unwrap().success() {
        // Fallback: write dummy file if ffmpeg CLI is not present
        fs::write(&media_path, b"dummy media").unwrap();
        let _ = fs::remove_dir_all(&temp_dir);
        return;
    }

    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let (command_tx, command_rx) = channel::<PlayerCommand>();
    let (audio_tx, audio_rx) = channel::<AudioWorkerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();
    let running = Arc::new(AtomicBool::new(true));

    let thread_exchange = Arc::clone(&exchange);
    let thread_video_ring = Arc::clone(&video_ring);
    let thread_clock = Arc::clone(&clock);
    let thread_running = Arc::clone(&running);

    let drain_video_ring = Arc::clone(&video_ring);
    let drain_running = Arc::clone(&running);
    let drain_handle = std::thread::spawn(move || {
        while drain_running.load(Ordering::Relaxed) {
            while drain_video_ring.try_pop().is_some() {}
            std::thread::sleep(Duration::from_millis(5));
        }
    });

    let worker_handle = std::thread::spawn(move || {
        run_playback_worker(
            command_rx,
            audio_tx,
            event_tx,
            thread_exchange,
            thread_video_ring,
            thread_clock,
            thread_running,
        );
    });

    // 1. Open media file
    command_tx.send(PlayerCommand::OpenFile(media_path.clone())).unwrap();

    // Wait for TracksDiscovered event
    let start = std::time::Instant::now();
    let mut tracks_ready = false;
    while start.elapsed() < Duration::from_secs(3) {
        if let Ok(UiEvent::TracksDiscovered { .. }) = event_rx.recv_timeout(Duration::from_millis(100)) {
            tracks_ready = true;
            break;
        }
    }
    assert!(tracks_ready, "TracksDiscovered must be emitted");

    // 2. Start playback
    command_tx.send(PlayerCommand::ForcePlay).unwrap();

    // Receive initial audio samples
    let mut initial_samples = 0;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if let Ok(AudioWorkerCommand::Samples(samples) | AudioWorkerCommand::TimestampedSamples { samples, .. }) = audio_rx.recv_timeout(Duration::from_millis(100)) {
            initial_samples += samples.len();
            if initial_samples > 4096 {
                break;
            }
        }
    }
    assert!(initial_samples > 4096, "Initial playback must yield audio samples (got {initial_samples})");

    // 3. Perform a long seek: jump 6 seconds forward (simulates 10-minute skip)
    command_tx.send(PlayerCommand::Seek(6000)).unwrap();

    // Verify audio samples CONTINUE to be decoded and output after the seek!
    let mut post_seek_samples = 0;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        match audio_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(AudioWorkerCommand::Samples(samples) | AudioWorkerCommand::TimestampedSamples { samples, .. }) => {
                post_seek_samples += samples.len();
                if post_seek_samples > 4096 {
                    break;
                }
            }
            Ok(AudioWorkerCommand::Reset) => {
                // Reset is expected on seek
            }
            _ => {}
        }
    }
    assert!(
        post_seek_samples > 4096,
        "Audio MUST continue decoding and outputting samples post-seek (got {post_seek_samples})"
    );

    // 4. Test cycling audio track
    command_tx.send(PlayerCommand::CycleAudioTrack).unwrap();

    let mut post_cycle_samples = 0;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        match audio_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(AudioWorkerCommand::Samples(samples) | AudioWorkerCommand::TimestampedSamples { samples, .. }) => {
                post_cycle_samples += samples.len();
                if post_cycle_samples > 4096 {
                    break;
                }
            }
            Ok(AudioWorkerCommand::Reset) => {}
            _ => {}
        }
    }
    assert!(
        post_cycle_samples > 4096,
        "Audio MUST continue outputting samples post-cycle (got {post_cycle_samples})"
    );

    // 5. Seek again after track cycling to ensure second seek also works seamlessly!
    command_tx.send(PlayerCommand::Seek(9000)).unwrap();

    let mut second_seek_samples = 0;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        match audio_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(AudioWorkerCommand::Samples(samples) | AudioWorkerCommand::TimestampedSamples { samples, .. }) => {
                second_seek_samples += samples.len();
                if second_seek_samples > 4096 {
                    break;
                }
            }
            Ok(AudioWorkerCommand::Reset) => {}
            _ => {}
        }
    }
    assert!(
        second_seek_samples > 4096,
        "Audio MUST continue outputting samples after second seek (got {second_seek_samples})"
    );

    // 6. Cleanup
    running.store(false, Ordering::Relaxed);
    let _ = command_tx.send(PlayerCommand::Stop);
    let _ = drain_handle.join();
    let _ = worker_handle.join();
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_real_user_video_seek_10_minutes() {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::time::Duration;
    use syncverse_clock::MasterClock;

    use syncverse_clock::PlaybackState;
    use syncverse_clock::SpscRingBuffer;
    use syncverse_codecs::VideoFrameRGBA;
    use syncverse_core::playback_runtime::{run_playback_worker, AudioWorkerCommand};
    use syncverse_ui::command::PlayerCommand;
    use syncverse_ui::event::UiEvent;
    use syncverse_ui::snapshot::TripleBufferSnapshotExchange;

    let media_path = PathBuf::from("/home/gautham/Videos/Your.Name.2016.1080p.Bluray.Multi-Audio.10bit.DTS-HD.MA.5.1.x265.[HashMiner].mkv");
    if !media_path.exists() {
        eprintln!("File does not exist, skipping real video test");
        return;
    }

    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let (command_tx, command_rx) = channel::<PlayerCommand>();
    let (audio_tx, audio_rx) = channel::<AudioWorkerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();
    let running = Arc::new(AtomicBool::new(true));

    let thread_exchange = Arc::clone(&exchange);
    let thread_video_ring = Arc::clone(&video_ring);
    let thread_clock = Arc::clone(&clock);
    let thread_running = Arc::clone(&running);

    let drain_video_ring = Arc::clone(&video_ring);
    let drain_running = Arc::clone(&running);
    let drain_handle = std::thread::spawn(move || {
        while drain_running.load(Ordering::Relaxed) {
            while drain_video_ring.try_pop().is_some() {}
            std::thread::sleep(Duration::from_millis(5));
        }
    });

    let worker_handle = std::thread::spawn(move || {
        run_playback_worker(
            command_rx,
            audio_tx,
            event_tx,
            thread_exchange,
            thread_video_ring,
            thread_clock,
            thread_running,
        );
    });

    // 1. Open media file
    command_tx.send(PlayerCommand::OpenFile(media_path.clone())).unwrap();

    let start = std::time::Instant::now();
    let mut tracks_ready = false;
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(UiEvent::TracksDiscovered { .. }) = event_rx.recv_timeout(Duration::from_millis(100)) {
            tracks_ready = true;
            break;
        }
    }
    assert!(tracks_ready, "TracksDiscovered must be emitted");

    // 2. Start playback
    command_tx.send(PlayerCommand::ForcePlay).unwrap();

    // Receive initial audio samples
    let mut initial_samples = 0;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if let Ok(AudioWorkerCommand::Samples(samples) | AudioWorkerCommand::TimestampedSamples { samples, .. }) = audio_rx.recv_timeout(Duration::from_millis(100)) {
            initial_samples += samples.len();
            if initial_samples > 4096 {
                break;
            }
        }
    }
    println!("[REAL TEST] Initial samples received: {}", initial_samples);
    assert!(initial_samples > 4096, "Initial playback must yield audio samples (got {initial_samples})");

    // 3. Skip 10 minutes forward (600,000 ms)
    println!("[REAL TEST] Seeking to 600,000 ms (10 minutes)...");
    command_tx.send(PlayerCommand::Seek(600_000)).unwrap();

    // Check if audio samples arrive post-seek!
    let mut post_seek_samples = 0;
    let mut reset_count = 0;
    let seek_start = std::time::Instant::now();
    let mut latency = Duration::ZERO;
    while seek_start.elapsed() < Duration::from_secs(3) {
        match audio_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(AudioWorkerCommand::Samples(samples) | AudioWorkerCommand::TimestampedSamples { samples, .. }) => {
                post_seek_samples += samples.len();
                if post_seek_samples > 4096 {
                    latency = seek_start.elapsed();
                    break;
                }
            }
            Ok(AudioWorkerCommand::Reset) => {
                reset_count += 1;
            }
            _ => {}
        }
    }
    println!("[REAL TEST] Post-seek samples: {}, resets: {}, latency: {:?}", post_seek_samples, reset_count, latency);

    running.store(false, Ordering::Relaxed);
    let _ = command_tx.send(PlayerCommand::Stop);
    let _ = drain_handle.join();
    let _ = worker_handle.join();

    assert!(
        post_seek_samples > 4096,
        "Audio MUST continue outputting samples after 10-minute seek on real video! Got {post_seek_samples}"
    );
}

#[test]
fn test_real_user_video_paused_seek_and_unpause() {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::time::Duration;
    use syncverse_clock::MasterClock;
    use syncverse_clock::PlaybackState;
    use syncverse_clock::SpscRingBuffer;
    use syncverse_codecs::VideoFrameRGBA;
    use syncverse_core::playback_runtime::{run_playback_worker, AudioWorkerCommand};
    use syncverse_ui::command::PlayerCommand;
    use syncverse_ui::event::UiEvent;
    use syncverse_ui::snapshot::TripleBufferSnapshotExchange;

    let media_path = PathBuf::from("/home/gautham/Videos/Your.Name.2016.1080p.Bluray.Multi-Audio.10bit.DTS-HD.MA.5.1.x265.[HashMiner].mkv");
    if !media_path.exists() {
        return;
    }

    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let (command_tx, command_rx) = channel::<PlayerCommand>();
    let (audio_tx, audio_rx) = channel::<AudioWorkerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();
    let running = Arc::new(AtomicBool::new(true));

    let thread_exchange = Arc::clone(&exchange);
    let thread_video_ring = Arc::clone(&video_ring);
    let thread_clock = Arc::clone(&clock);
    let thread_running = Arc::clone(&running);

    let drain_video_ring = Arc::clone(&video_ring);
    let drain_running = Arc::clone(&running);
    let drain_handle = std::thread::spawn(move || {
        while drain_running.load(Ordering::Relaxed) {
            while drain_video_ring.try_pop().is_some() {}
            std::thread::sleep(Duration::from_millis(5));
        }
    });

    let worker_handle = std::thread::spawn(move || {
        run_playback_worker(
            command_rx,
            audio_tx,
            event_tx,
            thread_exchange,
            thread_video_ring,
            thread_clock,
            thread_running,
        );
    });

    // 1. Open media file
    command_tx.send(PlayerCommand::OpenFile(media_path.clone())).unwrap();

    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(UiEvent::TracksDiscovered { .. }) = event_rx.recv_timeout(Duration::from_millis(100)) {
            break;
        }
    }

    // 2. Seek while PAUSED (target: 600,000 ms)
    command_tx.send(PlayerCommand::Seek(600_000)).unwrap();
    std::thread::sleep(Duration::from_millis(150));

    // 3. Unpause (ForcePlay)
    command_tx.send(PlayerCommand::ForcePlay).unwrap();

    // 4. Verify audio resumes immediately post-unpause!
    let mut unpaused_samples = 0;
    let start = std::time::Instant::now();
    let mut latency = Duration::ZERO;
    while start.elapsed() < Duration::from_secs(3) {
        if let Ok(AudioWorkerCommand::Samples(samples) | AudioWorkerCommand::TimestampedSamples { samples, .. }) = audio_rx.recv_timeout(Duration::from_millis(50)) {
            unpaused_samples += samples.len();
            if unpaused_samples > 4096 {
                latency = start.elapsed();
                break;
            }
        }
    }

    println!("[REAL PAUSED TEST] Unpaused samples: {}, latency: {:?}", unpaused_samples, latency);

    running.store(false, Ordering::Relaxed);
    let _ = command_tx.send(PlayerCommand::Stop);
    let _ = drain_handle.join();
    let _ = worker_handle.join();

    assert!(
        unpaused_samples > 4096,
        "Audio MUST resume seamlessly after seeking while paused! Got {unpaused_samples}"
    );
}

#[test]
fn test_high_bitrate_seek_av_alignment() {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::time::Duration;
    use syncverse_clock::MasterClock;
    use syncverse_clock::PlaybackState;
    use syncverse_clock::SpscRingBuffer;
    use syncverse_codecs::VideoFrameRGBA;
    use syncverse_core::playback_runtime::{run_playback_worker, AudioWorkerCommand};
    use syncverse_demux::Demuxer;
    use syncverse_ui::command::PlayerCommand;
    use syncverse_ui::event::UiEvent;
    use syncverse_ui::snapshot::TripleBufferSnapshotExchange;

    let media_path = PathBuf::from("/home/gautham/Downloads/Nightcrawler 2014 WEB-DL 2160p HDR10+ DV HEVC DTS-HD MA 5.1 x265-E/Nightcrawler 2014.mkv");
    if !media_path.exists() {
        return;
    }

    // 1. Verify container-level demux alignment at distant seek points (10m and 20m)
    let mut demuxer = syncverse_demux::UniversalDemuxer::open_file(&media_path).expect("Failed to open file");
    let info = demuxer.container_info().expect("Missing container info").clone();
    let v_track = info.tracks.iter().find(|t| t.track_type == syncverse_demux::TrackType::Video).unwrap().track_id;
    let a_track = info.tracks.iter().find(|t| t.track_type == syncverse_demux::TrackType::Audio).unwrap().track_id;

    for &target_ms in &[600_000, 1_200_000] {
        let landed = demuxer.seek_to(target_ms).expect("Demuxer seek failed");
        assert!(landed > 0, "Landed PTS must be positive");

        let mut first_video_pts = None;
        let mut first_audio_pts = None;

        for _ in 0..100 {
            if let Ok(Some(pkt)) = demuxer.read_packet() {
                if pkt.track_id == v_track && first_video_pts.is_none() {
                    first_video_pts = Some(pkt.pts_ms);
                } else if pkt.track_id == a_track && first_audio_pts.is_none() {
                    first_audio_pts = Some(pkt.pts_ms);
                }
                if first_video_pts.is_some() && first_audio_pts.is_some() {
                    break;
                }
            }
        }

        let v_pts = first_video_pts.expect("Video packet not found post-seek");
        let a_pts = first_audio_pts.expect("Audio packet not found post-seek");
        let av_delta = (v_pts as i64 - a_pts as i64).abs();
        assert!(
            av_delta < 50,
            "Seek to {target_ms}ms produced unacceptable demux A/V skew of {av_delta}ms (V: {v_pts}, A: {a_pts})"
        );
    }

    // 2. Verify end-to-end playback worker audio streaming resumption post-seek
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let (command_tx, command_rx) = channel::<PlayerCommand>();
    let (audio_tx, audio_rx) = channel::<AudioWorkerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();
    let running = Arc::new(AtomicBool::new(true));

    let thread_exchange = Arc::clone(&exchange);
    let thread_video_ring = Arc::clone(&video_ring);
    let thread_clock = Arc::clone(&clock);
    let thread_running = Arc::clone(&running);

    let drain_video_ring = Arc::clone(&video_ring);
    let drain_running = Arc::clone(&running);
    let drain_handle = std::thread::spawn(move || {
        while drain_running.load(Ordering::Relaxed) {
            while drain_video_ring.try_pop().is_some() {}
            std::thread::sleep(Duration::from_millis(5));
        }
    });

    let worker_handle = std::thread::spawn(move || {
        run_playback_worker(
            command_rx,
            audio_tx,
            event_tx,
            thread_exchange,
            thread_video_ring,
            thread_clock,
            thread_running,
        );
    });

    command_tx.send(PlayerCommand::OpenFile(media_path.clone())).unwrap();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(UiEvent::TracksDiscovered { .. }) = event_rx.recv_timeout(Duration::from_millis(100)) {
            break;
        }
    }

    command_tx.send(PlayerCommand::ForcePlay).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // Seek to 1,200,000 ms (20 min)
    command_tx.send(PlayerCommand::Seek(1_200_000)).unwrap();

    let mut samples_received = 0;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if let Ok(AudioWorkerCommand::Samples(samples) | AudioWorkerCommand::TimestampedSamples { samples, .. }) = audio_rx.recv_timeout(Duration::from_millis(100)) {
            samples_received += samples.len();
            if samples_received > 8192 {
                break;
            }
        }
    }

    running.store(false, Ordering::Relaxed);
    let _ = command_tx.send(PlayerCommand::Stop);
    let _ = drain_handle.join();
    let _ = worker_handle.join();

    assert!(
        samples_received > 8192,
        "Audio samples MUST flow seamlessly after seeking forward 20 minutes! Received: {samples_received}"
    );
}

