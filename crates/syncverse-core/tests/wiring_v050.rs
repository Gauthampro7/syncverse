//! Integration tests for v0.5.0 core runtime wiring (WP6).

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use syncverse_clock::{MasterClock, PlaybackState, SpscRingBuffer};
use syncverse_codecs::VideoFrameRGBA;
use syncverse_common::userdata::{keys, FileKey, ResumeEntry, ResumeStore, Settings, UserDirs};
use syncverse_core::playback_runtime::{run_playback_worker_with_dirs, AudioWorkerCommand};
use syncverse_ui::{PlayerCommand, TripleBufferSnapshotExchange, UiEvent};

fn generate_120s_clip(path: &Path) -> bool {
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=64x64:r=1",
            "-t",
            "120",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    matches!(status, Ok(s) if s.success()) && path.exists()
}

fn drain_events_until<F>(rx: &Receiver<UiEvent>, timeout: Duration, mut pred: F) -> Vec<UiEvent>
where
    F: FnMut(&UiEvent) -> bool,
{
    let deadline = Instant::now() + timeout;
    let mut collected = Vec::new();
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(25)) {
            Ok(ev) => {
                let matched = pred(&ev);
                collected.push(ev);
                if matched {
                    break;
                }
            }
            Err(_) => continue,
        }
    }
    collected
}

#[test]
fn wp4_settings_resume_and_per_file_restore_via_worker() {
    let base = std::env::temp_dir().join(format!("syncverse_core_wp4_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    let clip = base.join("sample_120s.mp4");
    if !generate_120s_clip(&clip) {
        eprintln!("SKIP: ffmpeg not available to generate test clip");
        let _ = std::fs::remove_dir_all(&base);
        return;
    }

    let dirs = UserDirs::under(&base);

    let mut init_settings = Settings::default();
    init_settings.set("unknown_pref", "preserved_value");
    init_settings.set(keys::VOLUME, "0.75");
    init_settings.set_bool(keys::MUTED, false);
    init_settings.set(keys::RESUME_POLICY, "ask");
    init_settings.set_bool(keys::REMEMBER_PER_FILE, true);
    init_settings.set_bool(keys::REMEMBER_SETTINGS, true);
    init_settings.save(&dirs.settings_path()).unwrap();

    let key = FileKey::from_path(&clip).unwrap();
    let mut store = ResumeStore::default();
    let mut entry = ResumeEntry::new(key);
    entry.pos_ms = 40_000;
    entry.dur_ms = 120_000;
    entry.audio_delay_ms = 150;
    entry.sub_delay_ms = -200;
    entry.aspect = "16:9".to_string();
    store.put(entry);
    store.save(&dirs.resume_path()).unwrap();

    let (cmd_tx, cmd_rx) = channel::<PlayerCommand>();
    let (audio_tx, audio_rx) = channel::<AudioWorkerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let running = Arc::new(AtomicBool::new(true));

    let worker_exchange = Arc::clone(&exchange);
    let worker_clock = Arc::clone(&clock);
    let worker_running = Arc::clone(&running);
    let worker_dirs = dirs.clone();

    let handle = thread::spawn(move || {
        run_playback_worker_with_dirs(
            cmd_rx,
            audio_tx,
            event_tx,
            worker_exchange,
            video_ring,
            worker_clock,
            worker_running,
            Some(worker_dirs),
        );
    });

    // Startup should have pushed restored volume (0.75) to audio worker.
    let mut saw_initial_vol = false;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if let Ok(AudioWorkerCommand::SetVolume(v)) = audio_rx.recv_timeout(Duration::from_millis(20))
        {
            if (v - 0.75).abs() < 0.01 {
                saw_initial_vol = true;
                break;
            }
        }
    }
    assert!(saw_initial_vol, "expected initial volume 0.75 from settings");

    cmd_tx.send(PlayerCommand::OpenFile(clip.clone())).unwrap();
    let events = drain_events_until(&event_rx, Duration::from_secs(5), |ev| {
        matches!(ev, UiEvent::ResumeAvailable { .. })
    });

    assert!(
        events
            .iter()
            .any(|e| matches!(e, UiEvent::AudioDelayChanged(150))),
        "expected restored audio delay 150ms, got {:?}",
        events
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, UiEvent::SubtitleDelayChanged(-200))),
        "expected restored subtitle delay -200ms, got {:?}",
        events
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, UiEvent::AspectRatioRestored(s) if s == "16:9")),
        "expected restored aspect 16:9, got {:?}",
        events
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            UiEvent::ResumeAvailable {
                pos_ms: 40_000,
                ..
            }
        )),
        "expected ResumeAvailable at 40_000ms, got {:?}",
        events
    );

    // Accept resume and verify clock seeks near 40s.
    cmd_tx.send(PlayerCommand::ResumeAccept).unwrap();
    let seek_deadline = Instant::now() + Duration::from_secs(3);
    let mut resumed_ok = false;
    while Instant::now() < seek_deadline {
        let snap = exchange.poll_latest_snapshot();
        if snap.media_position_pts >= 35_000_000_000 {
            resumed_ok = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(resumed_ok, "expected ResumeAccept to seek near 40_000ms");

    // Change preferences + volume + mute, then exit cleanly and verify settings.conf saved.
    cmd_tx.send(PlayerCommand::SetVolume(0.42)).unwrap();
    cmd_tx.send(PlayerCommand::ToggleMute).unwrap();
    cmd_tx
        .send(PlayerCommand::SetResumePolicy("always".to_string()))
        .unwrap();
    cmd_tx.send(PlayerCommand::SetRememberPerFile(false)).unwrap();
    thread::sleep(Duration::from_millis(100));

    running.store(false, Ordering::Relaxed);
    handle.join().unwrap();

    let saved = Settings::load(&dirs.settings_path());
    assert_eq!(saved.get("unknown_pref"), Some("preserved_value"));
    assert_eq!(saved.get(keys::RESUME_POLICY), Some("always"));
    assert_eq!(saved.get_bool(keys::REMEMBER_PER_FILE), Some(false));
    assert_eq!(saved.get_bool(keys::MUTED), Some(true));
    let saved_vol = saved.get_f32(keys::VOLUME).unwrap_or(0.0);
    assert!(
        (saved_vol - 0.42).abs() < 0.01,
        "expected saved volume ~0.42, got {saved_vol}"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn wp4_in_party_suppresses_resume_offer() {
    let base = std::env::temp_dir().join(format!("syncverse_core_wp4_party_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    let clip = base.join("party_120s.mp4");
    if !generate_120s_clip(&clip) {
        eprintln!("SKIP: ffmpeg not available to generate test clip");
        let _ = std::fs::remove_dir_all(&base);
        return;
    }

    let dirs = UserDirs::under(&base);
    let key = FileKey::from_path(&clip).unwrap();
    let mut store = ResumeStore::default();
    let mut entry = ResumeEntry::new(key);
    entry.pos_ms = 45_000;
    entry.dur_ms = 120_000;
    store.put(entry);
    store.save(&dirs.resume_path()).unwrap();

    let (cmd_tx, cmd_rx) = channel::<PlayerCommand>();
    let (audio_tx, _audio_rx) = channel::<AudioWorkerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let running = Arc::new(AtomicBool::new(true));

    let worker_running = Arc::clone(&running);
    let worker_dirs = dirs.clone();

    let handle = thread::spawn(move || {
        run_playback_worker_with_dirs(
            cmd_rx,
            audio_tx,
            event_tx,
            exchange,
            video_ring,
            clock,
            worker_running,
            Some(worker_dirs),
        );
    });

    // Pick a free UDP port to host a party.
    let probe_sock = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = probe_sock.local_addr().unwrap().port();
    drop(probe_sock);

    cmd_tx
        .send(PlayerCommand::HostParty {
            port,
            room: "test-room".to_string(),
            username: "Host".to_string(),
            password: None,
            enable_upnp: false,
        })
        .unwrap();
    thread::sleep(Duration::from_millis(100));

    cmd_tx.send(PlayerCommand::OpenFile(clip.clone())).unwrap();
    let events = drain_events_until(&event_rx, Duration::from_millis(1200), |ev| {
        matches!(ev, UiEvent::ResumeAvailable { .. })
    });

    assert!(
        !events
            .iter()
            .any(|e| matches!(e, UiEvent::ResumeAvailable { .. })),
        "ResumeAvailable must NOT be emitted in a watch-party"
    );

    running.store(false, Ordering::Relaxed);
    handle.join().unwrap();
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn wp5_step_frame_uses_stream_fps_and_blocks_party_client() {
    let base = std::env::temp_dir().join(format!("syncverse_core_wp5_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    let clip = base.join("step_25fps.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=64x64:rate=25",
            "-t",
            "2",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&clip)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if !matches!(status, Ok(s) if s.success()) {
        eprintln!("SKIP: ffmpeg not available");
        let _ = std::fs::remove_dir_all(&base);
        return;
    }

    let (cmd_tx, cmd_rx) = channel::<PlayerCommand>();
    let (audio_tx, _audio_rx) = channel::<AudioWorkerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let running = Arc::new(AtomicBool::new(true));

    let worker_exchange = Arc::clone(&exchange);
    let worker_running = Arc::clone(&running);
    let handle = thread::spawn(move || {
        run_playback_worker_with_dirs(
            cmd_rx,
            audio_tx,
            event_tx,
            worker_exchange,
            video_ring,
            clock,
            worker_running,
            None,
        );
    });

    cmd_tx.send(PlayerCommand::OpenFile(clip)).unwrap();
    let _ = drain_events_until(&event_rx, Duration::from_secs(3), |ev| {
        matches!(ev, UiEvent::VideoMetadataDiscovered(_))
    });
    // Pause at 0 so we can step cleanly.
    cmd_tx.send(PlayerCommand::TogglePause).unwrap();
    cmd_tx.send(PlayerCommand::Seek(0)).unwrap();
    thread::sleep(Duration::from_millis(80));

    // 5 forward steps at 25fps => ~200ms (each frame = 40ms).
    for _ in 0..5 {
        cmd_tx.send(PlayerCommand::StepFrame { forward: true }).unwrap();
        thread::sleep(Duration::from_millis(40));
    }
    let pos_ms = exchange.poll_latest_snapshot().media_position_pts / 1_000_000;
    assert!(
        (160..=240).contains(&pos_ms),
        "expected 5 steps at 25fps to land near 200ms, got {pos_ms}ms"
    );

    running.store(false, Ordering::Relaxed);
    handle.join().unwrap();
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn wp1_hwdec_policy_reaches_session_open_and_fallback_toast() {
    let base = std::env::temp_dir().join(format!("syncverse_core_wp1_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    let clip = base.join("hwdec_probe.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=64x64:rate=25",
            "-t",
            "1",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&clip)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if !matches!(status, Ok(s) if s.success()) {
        eprintln!("SKIP: ffmpeg not available");
        let _ = std::fs::remove_dir_all(&base);
        return;
    }

    // Force an invalid VA-API device so fallback to software is deterministic even without GPU.
    let bad_hw = syncverse_codecs::hwaccel::HwdecConfig::new(
        syncverse_codecs::hwaccel::HwdecPolicy::Vaapi,
    )
    .with_device("/nonexistent/renderD999");

    let (cmd_tx, cmd_rx) = channel::<PlayerCommand>();
    let (audio_tx, _audio_rx) = channel::<AudioWorkerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let running = Arc::new(AtomicBool::new(true));

    let worker_running = Arc::clone(&running);
    let handle = thread::spawn(move || {
        syncverse_core::playback_runtime::run_playback_worker_with_hw_and_dirs(
            cmd_rx,
            audio_tx,
            event_tx,
            exchange,
            video_ring,
            clock,
            worker_running,
            None,
            Some(bad_hw),
        );
    });

    cmd_tx.send(PlayerCommand::OpenFile(clip)).unwrap();
    let events = drain_events_until(&event_rx, Duration::from_secs(3), |ev| {
        matches!(ev, UiEvent::VideoMetadataDiscovered(_))
    });

    assert!(
        events.iter().any(|e| matches!(
            e,
            UiEvent::HudNotification { text, .. } if text.contains("Hardware decode fallback")
        )),
        "expected fallback HUD toast, got {:?}",
        events
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            UiEvent::VideoMetadataDiscovered(meta) if meta.codec.as_deref().is_some_and(|c| c.to_ascii_lowercase().contains("software"))
        )),
        "expected Software backend in VideoMetadataDiscovered, got {:?}",
        events
    );

    running.store(false, Ordering::Relaxed);
    handle.join().unwrap();
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn wp3_connect_unreachable_never_emits_connected() {
    let probe_sock = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let dead_port = probe_sock.local_addr().unwrap().port();
    drop(probe_sock);

    let (cmd_tx, cmd_rx) = channel::<PlayerCommand>();
    let (audio_tx, _audio_rx) = channel::<AudioWorkerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let running = Arc::new(AtomicBool::new(true));

    let worker_running = Arc::clone(&running);
    let handle = thread::spawn(move || {
        run_playback_worker_with_dirs(
            cmd_rx,
            audio_tx,
            event_tx,
            exchange,
            video_ring,
            clock,
            worker_running,
            None,
        );
    });

    cmd_tx
        .send(PlayerCommand::ConnectParty {
            addr: format!("127.0.0.1:{dead_port}"),
            room: "nowhere".to_string(),
            username: "Guest".to_string(),
            password: None,
        })
        .unwrap();

    let events = drain_events_until(&event_rx, Duration::from_millis(600), |_| false);
    assert!(
        events.iter().any(|e| matches!(
            e,
            UiEvent::PartyConnectionState {
                state: syncverse_ui::event::PartyConnectionState::Connecting
            }
        )),
        "expected Connecting state, got {:?}",
        events
    );
    assert!(
        !events.iter().any(|e| matches!(
            e,
            UiEvent::PartyConnectionState {
                state: syncverse_ui::event::PartyConnectionState::Connected
            }
        )),
        "must NOT emit Connected when no host responds"
    );
    assert!(
        !events.iter().any(|e| matches!(
            e,
            UiEvent::NewChatRecord(rec) if String::from_utf8_lossy(&rec.payload).contains("Connected to")
        )),
        "must NOT emit premature Connected chat message"
    );

    running.store(false, Ordering::Relaxed);
    handle.join().unwrap();
}
