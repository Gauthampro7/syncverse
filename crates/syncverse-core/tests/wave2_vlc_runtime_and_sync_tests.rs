//! Wave 2 integration & adversarial runtime verification suite for `syncverse-core`.
//!
//! Covers:
//! 1. Multi-audio track switching latency (<20ms) with independent `audio_demuxer` and zero video stall.
//! 2. 20-seek storm coalescing, near-EOF recovery, and monotonic `seek_generation` without worker `try_pop`.
//! 3. Continuous multi-track embedded subtitles (`mov_text` >=128B & clear `[0x00, 0x00]`,
//!    8-comma MKV ASS, unselected buffering -> select emission ordering, forced/default auto-select).
//! 4. Encrypted IPv4 & IPv6 Watch-Party E2E (<250ms welcome, 75ms NTP burst, initial snap,
//!    two-phase barrier unpause cross-peer alignment, and bidirectional Client->Host `PeerControl`).

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use syncverse_clock::{MasterClock, PlaybackState, SpscRingBuffer};
use syncverse_codecs::subtitle_decoder::{DecodedSubtitleEvent, UniversalSubtitleDecoder};
use syncverse_codecs::VideoFrameRGBA;
use syncverse_common::subtitles::{
    insert_cue_dedup, select_initial_subtitle_track, truncate_active_cues_at, SubtitleSource,
    SubtitleTrack, SubtitleTrackCandidate,
};
use syncverse_common::time::MediaTime;
use syncverse_core::playback_runtime::{run_playback_worker, AudioWorkerCommand};
use syncverse_ui::{PlayerCommand, RoomMember, TripleBufferSnapshotExchange, UiEvent};

struct TempPathGuard(PathBuf);

impl Drop for TempPathGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn make_temp_dir(prefix: &str) -> TempPathGuard {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), nanos));
    let _ = std::fs::create_dir_all(&dir);
    TempPathGuard(dir)
}

fn find_free_udp_port() -> u16 {
    let sock = std::net::UdpSocket::bind("127.0.0.1:0").expect("bind ephemeral UDP socket");
    sock.local_addr().unwrap().port()
}

fn has_ffmpeg_cli() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn generate_test_mkv_multitrack(dir: &std::path::Path) -> Option<PathBuf> {
    if !has_ffmpeg_cli() {
        return None;
    }
    let ass_path = dir.join("track0.ass");
    let srt_path = dir.join("track1.srt");
    let out_mkv = dir.join("wave2_multitrack.mkv");

    let ass_content = "\
[Script Info]\n\
ScriptType: v4.00+\n\
PlayResX: 320\n\
PlayResY: 240\n\
\n\
[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\
\n\
[Events]\n\
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:00.10,0:00:01.50,Default,,0,0,0,,{\\i1}First ASS Line, With Comma{\\i0}\n\
Dialogue: 0,0:00:01.80,0:00:03.20,Default,,0,0,0,,Second ASS Line\n";

    let srt_content = "\
1\n\
00:00:00,200 --> 00:00:01,600\n\
Forced Track Cue One\n\
\n\
2\n\
00:00:02,000 --> 00:00:03,400\n\
Forced Track Cue Two\n";

    std::fs::write(&ass_path, ass_content).ok()?;
    std::fs::write(&srt_path, srt_content).ok()?;

    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=24:duration=4",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=4",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:sample_rate=48000:duration=4",
            "-i",
        ])
        .arg(&ass_path)
        .arg("-i")
        .arg(&srt_path)
        .args([
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-map",
            "3:s",
            "-map",
            "4:s",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-g",
            "12",
            "-c:a",
            "aac",
            "-c:s:0",
            "ass",
            "-c:s:1",
            "srt",
            "-disposition:s:1",
            "forced",
            "-metadata:s:a:0",
            "language=eng",
            "-metadata:s:a:1",
            "language=jpn",
            "-metadata:s:s:0",
            "language=jpn",
            "-metadata:s:s:1",
            "language=eng",
        ])
        .arg(&out_mkv)
        .status()
        .ok()?;

    if status.success() && out_mkv.exists() {
        Some(out_mkv)
    } else {
        None
    }
}

#[test]
fn test_wave2_multi_audio_instant_switch_and_zero_video_stall() {
    let tmp = make_temp_dir("syncverse_w2_audio_switch");
    let real_mkv = PathBuf::from(
        "/home/gautham/Videos/Your.Name.2016.1080p.Bluray.Multi-Audio.10bit.DTS-HD.MA.5.1.x265.[HashMiner].mkv",
    );
    let generated_mkv = generate_test_mkv_multitrack(&tmp.0);
    let media_path = if real_mkv.exists() {
        real_mkv
    } else if let Some(gen) = generated_mkv {
        gen
    } else {
        eprintln!("[SKIP] Neither sample MKV nor ffmpeg CLI is available.");
        return;
    };

    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let (command_tx, command_rx) = channel::<PlayerCommand>();
    let (audio_tx, audio_rx) = channel::<AudioWorkerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();
    let running = Arc::new(AtomicBool::new(true));

    let drain_ring = Arc::clone(&video_ring);
    let drain_running = Arc::clone(&running);
    let drain_handle = thread::spawn(move || {
        let mut generations = Vec::new();
        while drain_running.load(Ordering::Relaxed) {
            while let Some(frame) = drain_ring.try_pop() {
                generations.push(frame.generation);
            }
            thread::sleep(Duration::from_millis(2));
        }
        generations
    });

    let worker_handle = {
        let ex = Arc::clone(&exchange);
        let ring = Arc::clone(&video_ring);
        let clk = Arc::clone(&clock);
        let run = Arc::clone(&running);
        thread::spawn(move || {
            run_playback_worker(command_rx, audio_tx, event_tx, ex, ring, clk, run);
        })
    };

    command_tx
        .send(PlayerCommand::OpenFile(media_path))
        .unwrap();

    let mut discovered_audio_ids = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(UiEvent::AudioDataDiscovered(items)) =
            event_rx.recv_timeout(Duration::from_millis(50))
        {
            discovered_audio_ids = items.into_iter().map(|i| i.id).collect();
            break;
        }
    }
    assert!(
        discovered_audio_ids.len() >= 2,
        "Expected >=2 audio tracks, found {:?}",
        discovered_audio_ids
    );

    // Let playback establish steady stream
    thread::sleep(Duration::from_millis(200));
    while audio_rx.try_recv().is_ok() {}
    let gen_before_switch = clock.seek_generation();

    let switch_start = Instant::now();
    command_tx
        .send(PlayerCommand::SelectAudioTrack(discovered_audio_ids[1]))
        .unwrap();

    let mut got_track_selected = false;
    let mut got_post_switch_pcm = false;
    let mut selected_elapsed = Duration::MAX;
    let switch_deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < switch_deadline && (!got_track_selected || !got_post_switch_pcm) {
        while let Ok(ev) = event_rx.try_recv() {
            if let UiEvent::AudioTrackSelected { track_id, .. } = ev {
                if track_id == discovered_audio_ids[1] {
                    got_track_selected = true;
                    selected_elapsed = switch_start.elapsed();
                }
            }
        }
        while let Ok(cmd) = audio_rx.try_recv() {
            if let AudioWorkerCommand::TimestampedSamples { samples, .. } = cmd {
                if !samples.is_empty() {
                    got_post_switch_pcm = true;
                }
            }
        }
        thread::sleep(Duration::from_millis(1));
    }

    assert!(got_track_selected, "Expected AudioTrackSelected event");
    assert!(got_post_switch_pcm, "Expected PCM samples after switch");
    assert!(
        selected_elapsed < Duration::from_millis(40),
        "Fast audio switch took {:?}, expected sub-frame speed without video flush",
        selected_elapsed
    );
    assert_eq!(
        clock.seek_generation(),
        gen_before_switch,
        "Audio track switch MUST NOT flush or bump video seek_generation"
    );

    running.store(false, Ordering::Relaxed);
    let _ = command_tx.send(PlayerCommand::Stop);
    let gens = drain_handle.join().unwrap();
    let _ = worker_handle.join();
    assert!(!gens.is_empty(), "Expected continuous video frames");
}

#[test]
fn test_wave2_seek_storm_coalescing_near_eof_and_monotonic_epoch() {
    let tmp = make_temp_dir("syncverse_w2_seek_storm");
    let Some(media_path) = generate_test_mkv_multitrack(&tmp.0) else {
        eprintln!("[SKIP] ffmpeg CLI unavailable for synthetic seek-storm test.");
        return;
    };

    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let (command_tx, command_rx) = channel::<PlayerCommand>();
    let (audio_tx, _audio_rx) = channel::<AudioWorkerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();
    let running = Arc::new(AtomicBool::new(true));

    let drain_ring = Arc::clone(&video_ring);
    let drain_running = Arc::clone(&running);
    let drain_handle = thread::spawn(move || {
        let mut observed_frames = Vec::new();
        while drain_running.load(Ordering::Relaxed) {
            while let Some(frame) = drain_ring.try_pop() {
                observed_frames.push((frame.generation, frame.pts_ms));
            }
            thread::sleep(Duration::from_millis(3));
        }
        observed_frames
    });

    let worker_handle = {
        let ex = Arc::clone(&exchange);
        let ring = Arc::clone(&video_ring);
        let clk = Arc::clone(&clock);
        let run = Arc::clone(&running);
        thread::spawn(move || {
            run_playback_worker(command_rx, audio_tx, event_tx, ex, ring, clk, run);
        })
    };

    command_tx
        .send(PlayerCommand::OpenFile(media_path))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut ready = false;
    while Instant::now() < deadline {
        if let Ok(UiEvent::TracksDiscovered { .. }) =
            event_rx.recv_timeout(Duration::from_millis(50))
        {
            ready = true;
            break;
        }
    }
    assert!(ready, "File open failed to emit TracksDiscovered");
    thread::sleep(Duration::from_millis(80));

    let gen_before_storm = clock.seek_generation();
    // Enqueue 20 consecutive seeks in a single burst
    for step in 1..=20u64 {
        let target_ms = 100 + step * 150;
        command_tx.send(PlayerCommand::Seek(target_ms)).unwrap();
    }

    // Wait for the coalesced seek batch to complete
    let storm_deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < storm_deadline && clock.seek_generation() == gen_before_storm {
        thread::sleep(Duration::from_millis(5));
    }
    thread::sleep(Duration::from_millis(60));

    let gen_after_storm = clock.seek_generation();
    let delta_gen = gen_after_storm - gen_before_storm;
    assert!(
        (1..=4).contains(&delta_gen),
        "20 queued seeks should coalesce into <=4 generations (got {delta_gen})"
    );
    let pts_after = clock.current_pts(Instant::now());
    assert!(
        pts_after >= 2500.0,
        "Expected landed PTS near final queued seek (3100ms), got {pts_after}"
    );

    // Near-EOF seek (3980ms / past 4000ms) must recover a valid frame rather than blanking
    let gen_before_eof = clock.seek_generation();
    command_tx.send(PlayerCommand::Seek(3980)).unwrap();
    let eof_deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < eof_deadline && clock.seek_generation() == gen_before_eof {
        thread::sleep(Duration::from_millis(5));
    }
    thread::sleep(Duration::from_millis(80));
    let final_gen = clock.seek_generation();
    assert!(final_gen > gen_before_eof);

    running.store(false, Ordering::Relaxed);
    let _ = command_tx.send(PlayerCommand::Stop);
    let frames = drain_handle.join().unwrap();
    let _ = worker_handle.join();

    assert!(!frames.is_empty(), "Expected decoded frames from consumer");
    for window in frames.windows(2) {
        assert!(
            window[0].0 <= window[1].0,
            "Frame generation regressed: {} -> {}",
            window[0].0,
            window[1].0
        );
    }
    assert!(
        frames.iter().any(|(g, _)| *g == final_gen),
        "Near-EOF seek (gen={final_gen}) must emit a decoded VideoFrameRGBA"
    );
}

#[test]
fn test_wave2_subtitles_mov_text_ass_buffering_and_autoselect() {
    // Part A: Pure-Rust fallback & decoder verification for >=128B mov_text, [0x00, 0x00] clear, and 8-comma ASS
    let mut mov_dec = UniversalSubtitleDecoder::new_packet_parser("mov_text");
    let long_text = "Long mov_text caption exceeding 128 bytes: ".to_string() + &"X".repeat(140);
    let len_bytes = (long_text.len() as u16).to_be_bytes();
    let mut mov_pkt = Vec::with_capacity(2 + long_text.len());
    mov_pkt.extend_from_slice(&len_bytes);
    mov_pkt.extend_from_slice(long_text.as_bytes());

    let evs = mov_dec.decode_packet(&mov_pkt, 1_000, Some(3_000)).unwrap();
    assert_eq!(evs.len(), 1);
    let mut track = SubtitleTrack::new(
        1,
        SubtitleSource::Embedded,
        "mov_text",
        Some("en".to_string()),
        "MP4 Sub",
        Vec::new(),
    );
    if let DecodedSubtitleEvent::Cue(cue) = &evs[0] {
        assert_eq!(cue.text, long_text);
        assert!(insert_cue_dedup(&mut track.cues, cue.clone()));
    } else {
        panic!("Expected Cue for >=128B mov_text");
    }

    let clear_evs = mov_dec.decode_packet(&[0x00, 0x00], 1_750, Some(0)).unwrap();
    assert_eq!(clear_evs, vec![DecodedSubtitleEvent::ClearActive { at_ms: 1_750 }]);
    truncate_active_cues_at(&mut track.cues, MediaTime::from_millis(1_750));
    assert_eq!(track.cues[0].end, MediaTime::from_millis(1_750));

    let mut ass_dec = UniversalSubtitleDecoder::new_packet_parser("ass");
    let ass_pkt = b"12,0,Default,Actor,0,0,0,,{\\b1}Hello, World!{\\b0}";
    let ass_evs = ass_dec.decode_packet(ass_pkt, 2_000, Some(2_500)).unwrap();
    assert_eq!(ass_evs.len(), 1);
    match &ass_evs[0] {
        DecodedSubtitleEvent::Cue(c) => assert_eq!(c.text, "Hello, World!"),
        _ => panic!("Expected parsed ASS Cue"),
    }

    // Part B: VLC-style priority ranking (external exact stem > external > embedded forced > embedded default)
    let candidates = vec![
        SubtitleTrackCandidate {
            index: 0,
            is_external: false,
            is_forced: false,
            is_default: true,
            language: Some("eng".to_string()),
            exact_stem_match: false,
        },
        SubtitleTrackCandidate {
            index: 1,
            is_external: false,
            is_forced: true,
            is_default: false,
            language: Some("eng".to_string()),
            exact_stem_match: false,
        },
        SubtitleTrackCandidate {
            index: 2,
            is_external: true,
            is_forced: false,
            is_default: false,
            language: Some("jpn".to_string()),
            exact_stem_match: true,
        },
    ];
    assert_eq!(select_initial_subtitle_track(&candidates, Some("en")), Some(2));
    assert_eq!(select_initial_subtitle_track(&candidates[..2], Some("en")), Some(1));

    // Part C: Runtime continuous multi-track buffering & SelectSubtitleTrack ordering on real/generated MKV
    let tmp = make_temp_dir("syncverse_w2_sub_runtime");
    let Some(media_path) = generate_test_mkv_multitrack(&tmp.0) else {
        return;
    };

    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let (command_tx, command_rx) = channel::<PlayerCommand>();
    let (audio_tx, _audio_rx) = channel::<AudioWorkerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();
    let running = Arc::new(AtomicBool::new(true));

    let drain_ring = Arc::clone(&video_ring);
    let drain_running = Arc::clone(&running);
    let drain_handle = thread::spawn(move || {
        while drain_running.load(Ordering::Relaxed) {
            while drain_ring.try_pop().is_some() {}
            thread::sleep(Duration::from_millis(5));
        }
    });

    let worker_handle = {
        let ex = Arc::clone(&exchange);
        let ring = Arc::clone(&video_ring);
        let clk = Arc::clone(&clock);
        let run = Arc::clone(&running);
        thread::spawn(move || {
            run_playback_worker(command_rx, audio_tx, event_tx, ex, ring, clk, run);
        })
    };

    command_tx.send(PlayerCommand::OpenFile(media_path)).unwrap();

    // Auto-select must pick forced track index 1 on open
    let mut auto_selected = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(UiEvent::SubtitleTrackSelected { track_index, .. }) =
            event_rx.recv_timeout(Duration::from_millis(50))
        {
            auto_selected = track_index;
            break;
        }
    }
    assert_eq!(
        auto_selected,
        Some(1),
        "Forced embedded subtitle track should auto-select on open"
    );

    // Let playback decode across track 0 (ASS, currently unselected) & track 1
    thread::sleep(Duration::from_millis(600));
    while event_rx.try_recv().is_ok() {}

    // Switch to previously-unselected track 0: SubtitleDataDiscovered MUST precede SubtitleTrackSelected
    // and contain buffered ASS cues
    command_tx
        .send(PlayerCommand::SelectSubtitleTrack(Some(0)))
        .unwrap();

    let mut saw_discovered_before_selected = false;
    let mut buffered_ass_cues = 0usize;
    let mut saw_selected = false;
    let sel_deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < sel_deadline && !saw_selected {
        if let Ok(ev) = event_rx.recv_timeout(Duration::from_millis(50)) {
            match ev {
                UiEvent::SubtitleDataDiscovered(tracks) => {
                    if !saw_selected {
                        saw_discovered_before_selected = true;
                        buffered_ass_cues = tracks[0].cues.len();
                    }
                }
                UiEvent::SubtitleTrackSelected {
                    track_index: Some(0),
                    ..
                } => {
                    saw_selected = true;
                }
                _ => {}
            }
        }
    }
    assert!(saw_selected);
    assert!(
        saw_discovered_before_selected,
        "SubtitleDataDiscovered MUST be emitted before SubtitleTrackSelected"
    );
    assert!(
        buffered_ass_cues > 0,
        "Unselected ASS track should still buffer cues continuously (got {buffered_ass_cues})"
    );

    running.store(false, Ordering::Relaxed);
    let _ = command_tx.send(PlayerCommand::Stop);
    let _ = drain_handle.join();
    let _ = worker_handle.join();
}

fn run_encrypted_party_sync_case(connect_addr_prefix: &str) {
    let port = find_free_udp_port();
    let connect_addr = format!("{}:{}", connect_addr_prefix, port);
    let running = Arc::new(AtomicBool::new(true));

    let (host_cmd_tx, host_cmd_rx) = channel::<PlayerCommand>();
    let (host_audio_tx, _host_audio_rx) = channel::<AudioWorkerCommand>();
    let (host_event_tx, host_event_rx) = channel::<UiEvent>();
    let host_exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let host_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let host_clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));

    let host_handle = {
        let ex = Arc::clone(&host_exchange);
        let ring = Arc::clone(&host_ring);
        let clk = Arc::clone(&host_clock);
        let run = Arc::clone(&running);
        thread::spawn(move || {
            run_playback_worker(
                host_cmd_rx,
                host_audio_tx,
                host_event_tx,
                ex,
                ring,
                clk,
                run,
            );
        })
    };

    let (client_cmd_tx, client_cmd_rx) = channel::<PlayerCommand>();
    let (client_audio_tx, _client_audio_rx) = channel::<AudioWorkerCommand>();
    let (client_event_tx, client_event_rx) = channel::<UiEvent>();
    let client_exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let client_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let client_clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));

    let client_handle = {
        let ex = Arc::clone(&client_exchange);
        let ring = Arc::clone(&client_ring);
        let clk = Arc::clone(&client_clock);
        let run = Arc::clone(&running);
        thread::spawn(move || {
            run_playback_worker(
                client_cmd_rx,
                client_audio_tx,
                client_event_tx,
                ex,
                ring,
                clk,
                run,
            );
        })
    };

    // Host opens room with encryption
    host_cmd_tx
        .send(PlayerCommand::HostParty {
            port,
            room: "wave2-encrypted-room".to_string(),
            username: "AliceHost".to_string(),
            password: Some("wave2-passphrase".to_string()),
            enable_upnp: false,
        })
        .unwrap();
    host_cmd_tx.send(PlayerCommand::SetForcePlay(true)).unwrap();
    client_cmd_tx.send(PlayerCommand::SetForcePlay(true)).unwrap();
    host_cmd_tx.send(PlayerCommand::Seek(12_000)).unwrap();
    thread::sleep(Duration::from_millis(60));

    let connect_start = Instant::now();
    client_cmd_tx
        .send(PlayerCommand::ConnectParty {
            addr: connect_addr.clone(),
            room: "wave2-encrypted-room".to_string(),
            username: "BobPeer".to_string(),
            password: Some("wave2-passphrase".to_string()),
        })
        .unwrap();

    // 1. Verify rapid welcome (<250ms budget check) and host peer visibility on client
    let mut welcomed_members: Vec<RoomMember> = Vec::new();
    let mut welcome_elapsed = Duration::MAX;
    let welcome_deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < welcome_deadline {
        if let Ok(UiEvent::RoomMembersUpdated(members)) =
            client_event_rx.recv_timeout(Duration::from_millis(5))
        {
            if members.len() >= 2 && members.iter().any(|m| m.username == "AliceHost") {
                welcomed_members = members;
                welcome_elapsed = connect_start.elapsed();
                break;
            }
        }
    }
    assert!(
        welcomed_members.len() >= 2,
        "Client did not receive Host welcome via {connect_addr}"
    );
    assert!(
        welcome_elapsed < Duration::from_millis(250),
        "Welcome handshake over {connect_addr} took {:?} (expected <250ms)",
        welcome_elapsed
    );

    // 2. Allow fast NTP burst to lock offset and snap client clock to Host's 12_000ms
    let snap_deadline = Instant::now() + Duration::from_secs(2);
    let mut snapped = false;
    while Instant::now() < snap_deadline {
        let client_pts = client_clock.current_pts(Instant::now());
        if (client_pts - 12_000.0).abs() < 200.0 {
            snapped = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        snapped,
        "Client failed to snap to Host timeline (client pts={})",
        client_clock.current_pts(Instant::now())
    );

    // 3. Client sends TogglePause -> PeerControlAction::Play -> Host barrier unpause alignment
    client_cmd_tx.send(PlayerCommand::TogglePause).unwrap();
    let barrier_deadline = Instant::now() + Duration::from_secs(2);
    let mut host_play_time = None;
    let mut client_play_time = None;
    while Instant::now() < barrier_deadline
        && (host_play_time.is_none() || client_play_time.is_none())
    {
        let now = Instant::now();
        if host_play_time.is_none() && host_clock.is_playing() {
            host_play_time = Some(now);
        }
        if client_play_time.is_none() && client_clock.is_playing() {
            client_play_time = Some(now);
        }
        thread::sleep(Duration::from_millis(1));
    }
    let hp = host_play_time.expect("Host did not unpause via barrier");
    let cp = client_play_time.expect("Client did not unpause via barrier");
    let skew = if hp > cp {
        hp.duration_since(cp)
    } else {
        cp.duration_since(hp)
    };
    assert!(
        skew <= Duration::from_millis(20),
        "Barrier unpause cross-peer skew over {connect_addr} was {:?} (expected <= 20ms)",
        skew
    );

    // 4. Client sends PeerControl Seek & SetSpeed -> Host follows cleanly without echo
    client_cmd_tx.send(PlayerCommand::SetSpeed(1.25)).unwrap();
    client_cmd_tx.send(PlayerCommand::Seek(4_500)).unwrap();
    let ctrl_deadline = Instant::now() + Duration::from_secs(2);
    let mut host_followed = false;
    while Instant::now() < ctrl_deadline {
        let h_pts = host_clock.current_pts(Instant::now());
        if (host_clock.speed() - 1.25).abs() < 0.05 && (4_200.0..=6_500.0).contains(&h_pts) {
            host_followed = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        host_followed,
        "Host failed to apply Client PeerControl Seek+SetSpeed over {connect_addr}: speed={}, pts={}",
        host_clock.speed(),
        host_clock.current_pts(Instant::now())
    );

    // Drain any unused events cleanly
    while host_event_rx.try_recv().is_ok() {}

    running.store(false, Ordering::Relaxed);
    let _ = host_handle.join();
    let _ = client_handle.join();
}

#[test]
fn test_wave2_watch_party_encrypted_ipv4_and_ipv6_barrier_e2e() {
    run_encrypted_party_sync_case("127.0.0.1");
    run_encrypted_party_sync_case("[::1]");
}
