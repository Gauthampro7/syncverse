//! Empirical challenge verification suite for Milestone 3 (Part 2):
//! 1. `PlayerCommand` dispatching across all 9 variants:
//!    - `TogglePause`, `Seek`, `SetSpeed`, `Stop`, `SetVolume`, `ToggleMute`, `OpenFile`, `SendChatMessage`, `ExecuteSlashCommand`.
//! 2. Smart chat slash-command execution:
//!    - `/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`.
//! 3. CLI defaults and command-line execution:
//!    - Default `syncplay-alt` launches GUI (or graceful fallback when non-interactive).
//!    - `--headless` runs simulation without GUI.
//!    - `--terminal` launches interactive terminal player.
//!    - Subcommands: `inspect`, `party`, `chat`, `verify`, `version`, `--help`.

#![deny(warnings)]

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::thread;
use std::time::Instant;

use syncverse_chat::{
    DispatchAction, ParsedSlashCommand, SlashCommandDispatcher, SlashCommandParser,
    SyncDiagnostics,
};
use syncverse_clock::PlaybackState;
use syncverse_core::PlayerCoordinator;
use syncverse_ui::PlayerCommand;

// ============================================================================
// PART 1: PLAYERCOMMAND DISPATCHING ACROSS ALL 9 VARIANTS
// ============================================================================

#[test]
fn test_player_command_dispatching_all_nine_variants() {
    let mut coordinator = PlayerCoordinator::new();
    let now = Instant::now();

    // Initial state invariant: Paused at PTS 0.0 with speed 1.0
    assert_eq!(coordinator.clock.state(), PlaybackState::Paused);
    assert_eq!(coordinator.current_pts(now), 0.0);
    assert_eq!(coordinator.speed(), 1.0);

    // 1. Variant 1: TogglePause (Paused -> Playing)
    coordinator
        .handle_command(PlayerCommand::TogglePause, now)
        .expect("TogglePause Paused->Playing failed");
    assert_eq!(coordinator.clock.state(), PlaybackState::Playing);
    assert!(coordinator.is_playing());
    let snap1 = coordinator.tick(now);
    assert_eq!(snap1.playback_state, 1, "Snapshot state must be 1 (Playing)");

    // 2. Variant 1: TogglePause (Playing -> Paused)
    coordinator
        .handle_command(PlayerCommand::TogglePause, now)
        .expect("TogglePause Playing->Paused failed");
    assert_eq!(coordinator.clock.state(), PlaybackState::Paused);
    assert!(!coordinator.is_playing());
    let snap2 = coordinator.tick(now);
    assert_eq!(snap2.playback_state, 0, "Snapshot state must be 0 (Paused)");

    // 3. Variant 2: Seek (Jump to 42,000 ms)
    coordinator
        .handle_command(PlayerCommand::Seek(42_000), now)
        .expect("Seek command failed");
    let snap3 = coordinator.tick(now);
    assert_eq!(
        snap3.media_position_pts,
        42_000 * 1_000_000,
        "Seek must position clock to exact PTS in nanoseconds"
    );

    // 4. Variant 3: SetSpeed (Rate shift to 1.5x)
    coordinator
        .handle_command(PlayerCommand::SetSpeed(1.5), now)
        .expect("SetSpeed command failed");
    assert!((coordinator.speed() - 1.5).abs() < 1e-6);
    let snap4 = coordinator.tick(now);
    assert!((snap4.pll_speed_ratio_q16 as f64 / 65536.0 - 1.5).abs() < 0.01);

    // 5. Variant 4: Stop (Reset PTS to 0.0, set state to Paused)
    // First resume playing at current PTS
    coordinator
        .handle_command(PlayerCommand::TogglePause, now)
        .expect("TogglePause to Playing failed");
    assert_eq!(coordinator.clock.state(), PlaybackState::Playing);

    coordinator
        .handle_command(PlayerCommand::Stop, now)
        .expect("Stop command failed");
    assert_eq!(coordinator.clock.state(), PlaybackState::Paused);
    let snap5 = coordinator.tick(now);
    assert_eq!(snap5.media_position_pts, 0);
    assert_eq!(snap5.playback_state, 0);

    // 6. Variant 5: SetVolume (Normalized range [0.0, 1.0])
    for vol in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
        coordinator
            .handle_command(PlayerCommand::SetVolume(vol), now)
            .expect("SetVolume failed");
    }

    // 7. Variant 6: ToggleMute
    coordinator
        .handle_command(PlayerCommand::ToggleMute, now)
        .expect("ToggleMute failed");
    coordinator
        .handle_command(PlayerCommand::ToggleMute, now)
        .expect("ToggleMute second toggle failed");

    // 8. Variant 7: OpenFile (Load synthetic media vector)
    coordinator
        .handle_command(
            PlayerCommand::OpenFile(PathBuf::from("synthetic-mkv")),
            now,
        )
        .expect("OpenFile synthetic-mkv failed");
    coordinator
        .handle_command(
            PlayerCommand::OpenFile(PathBuf::from("synthetic-mp4")),
            now,
        )
        .expect("OpenFile synthetic-mp4 failed");

    // 9. Variant 8: SendChatMessage
    coordinator
        .handle_command(
            PlayerCommand::SendChatMessage("Watch party sync is awesome!".to_string()),
            now,
        )
        .expect("SendChatMessage failed");

    // 10. Variant 9: ExecuteSlashCommand
    coordinator
        .handle_command(
            PlayerCommand::ExecuteSlashCommand("/seek 01:30".to_string()),
            now,
        )
        .expect("ExecuteSlashCommand failed");
    coordinator
        .handle_command(
            PlayerCommand::ExecuteSlashCommand("/bookmark Opening Theme".to_string()),
            now,
        )
        .expect("ExecuteSlashCommand /bookmark failed");
    coordinator
        .handle_command(
            PlayerCommand::ExecuteSlashCommand("/poll \"Vote?\" \"Yes\" \"No\"".to_string()),
            now,
        )
        .expect("ExecuteSlashCommand /poll failed");
}

#[test]
fn test_player_command_channel_concurrency_stress() {
    let (tx, rx) = channel::<PlayerCommand>();
    let num_senders = 4;
    let commands_per_sender = 250;

    let mut handles = Vec::new();
    for t in 0..num_senders {
        let tx_clone = tx.clone();
        handles.push(thread::spawn(move || {
            for i in 0..commands_per_sender {
                let cmd = match (t + i) % 9 {
                    0 => PlayerCommand::TogglePause,
                    1 => PlayerCommand::Seek((i * 1000) as u64),
                    2 => PlayerCommand::SetSpeed(0.95 + ((i % 10) as f64) * 0.01),
                    3 => PlayerCommand::Stop,
                    4 => PlayerCommand::SetVolume((i % 100) as f32 / 100.0),
                    5 => PlayerCommand::ToggleMute,
                    6 => PlayerCommand::OpenFile(PathBuf::from(format!("/media/clip_{}.mkv", i))),
                    7 => PlayerCommand::SendChatMessage(format!("Message {} from thread {}", i, t)),
                    8 => PlayerCommand::ExecuteSlashCommand(format!("/seek 00:{:02}", i % 60)),
                    _ => unreachable!(),
                };
                tx_clone.send(cmd).unwrap();
            }
        }));
    }
    drop(tx); // Close original producer

    for h in handles {
        h.join().unwrap();
    }

    // Consumer processes all (num_senders * commands_per_sender) commands
    let mut coordinator = PlayerCoordinator::new();
    let mut received = 0;
    let now = Instant::now();
    while let Ok(cmd) = rx.recv() {
        coordinator.handle_command(cmd, now).unwrap();
        received += 1;
    }

    assert_eq!(
        received,
        num_senders * commands_per_sender,
        "All commands must be safely received and processed without loss"
    );
}

// ============================================================================
// PART 2: SMART CHAT SLASH-COMMAND EXECUTION & DISPATCHING
// ============================================================================

#[test]
fn test_slash_command_seek_syntax_and_offsets() {
    // 1. Seconds format: /seek 120 or /seek 12.5
    let cmd1 = SlashCommandParser::parse("/seek 120", None).unwrap();
    assert_eq!(cmd1, ParsedSlashCommand::Seek { target_pts_ms: 120_000 });

    let cmd1b = SlashCommandParser::parse("/seek 12.5", None).unwrap();
    assert_eq!(cmd1b, ParsedSlashCommand::Seek { target_pts_ms: 12_500 });

    // 2. MM:SS and MM:SS.mmm
    let cmd2 = SlashCommandParser::parse("/seek 03:45", None).unwrap();
    assert_eq!(cmd2, ParsedSlashCommand::Seek { target_pts_ms: 225_000 });

    let cmd2b = SlashCommandParser::parse("/seek 03:45.500", None).unwrap();
    assert_eq!(cmd2b, ParsedSlashCommand::Seek { target_pts_ms: 225_500 });

    // 3. HH:MM:SS and HH:MM:SS.mmm
    let cmd3 = SlashCommandParser::parse("/seek 01:15:30", None).unwrap();
    assert_eq!(cmd3, ParsedSlashCommand::Seek { target_pts_ms: 4_530_000 });

    let cmd3b = SlashCommandParser::parse("/seek 02:00:00.250", None).unwrap();
    assert_eq!(cmd3b, ParsedSlashCommand::Seek { target_pts_ms: 7_200_250 });

    // 4. Relative positive: /seek +15s at PTS 60,000ms
    let cmd4 = SlashCommandParser::parse("/seek +15s", Some(60_000)).unwrap();
    assert_eq!(cmd4, ParsedSlashCommand::Seek { target_pts_ms: 75_000 });

    // 5. Relative negative: /seek -20s at PTS 60,000ms
    let cmd5 = SlashCommandParser::parse("/seek -20s", Some(60_000)).unwrap();
    assert_eq!(cmd5, ParsedSlashCommand::Seek { target_pts_ms: 40_000 });

    // 6. Relative negative saturating underflow: /seek -100s at PTS 20,000ms
    let cmd6 = SlashCommandParser::parse("/seek -100s", Some(20_000)).unwrap();
    assert_eq!(cmd6, ParsedSlashCommand::Seek { target_pts_ms: 0 });

    // 7. Error handling
    assert!(SlashCommandParser::parse("/seek", None).is_err());
    assert!(SlashCommandParser::parse("/seek not_a_time", None).is_err());
    assert!(SlashCommandParser::parse("/seek 00:61", None).is_err()); // seconds >= 60
    assert!(SlashCommandParser::parse("/seek 01:60:00", None).is_err()); // minutes >= 60
    assert!(SlashCommandParser::parse("/seek ++10s", None).is_err());
}

#[test]
fn test_slash_command_bookmark_and_clip() {
    // 1. Bookmark without label
    let bm1 = SlashCommandParser::parse("/bookmark", Some(15_000)).unwrap();
    assert_eq!(
        bm1,
        ParsedSlashCommand::Bookmark {
            label: None,
            pts_ms: 15_000,
        }
    );

    // 2. Bookmark with plain label
    let bm2 = SlashCommandParser::parse("/bookmark Epic Guitar Solo", Some(80_000)).unwrap();
    assert_eq!(
        bm2,
        ParsedSlashCommand::Bookmark {
            label: Some("Epic Guitar Solo".to_string()),
            pts_ms: 80_000,
        }
    );

    // 3. Bookmark with Unicode/CJK label
    let bm3 = SlashCommandParser::parse("/bookmark 精彩独奏片段", Some(95_000)).unwrap();
    assert_eq!(
        bm3,
        ParsedSlashCommand::Bookmark {
            label: Some("精彩独奏片段".to_string()),
            pts_ms: 95_000,
        }
    );

    // 4. Clip with valid interval
    let clip1 = SlashCommandParser::parse("/clip 00:10 00:30", None).unwrap();
    assert_eq!(
        clip1,
        ParsedSlashCommand::Clip {
            start_pts_ms: 10_000,
            end_pts_ms: 30_000,
            label: None,
        }
    );

    // 5. Clip with title
    let clip2 = SlashCommandParser::parse("/clip 01:00 01:45 \"Opening Credits\"", None).unwrap();
    assert_eq!(
        clip2,
        ParsedSlashCommand::Clip {
            start_pts_ms: 60_000,
            end_pts_ms: 105_000,
            label: Some("Opening Credits".to_string()),
        }
    );

    // 6. Clip error when start > end
    let err_clip = SlashCommandParser::parse("/clip 01:00 00:30", None);
    assert!(err_clip.is_err(), "Clip with start > end must fail validation");

    // 7. Clip error when missing end
    assert!(SlashCommandParser::parse("/clip 01:00", None).is_err());
    assert!(SlashCommandParser::parse("/clip", None).is_err());
}

#[test]
fn test_slash_command_poll_and_sync_status() {
    // 1. Quoted Poll syntax
    let poll1 = SlashCommandParser::parse(
        "/poll \"Best video codec?\" \"AV1\" \"H.265\" \"VP9\"",
        None,
    )
    .unwrap();
    match poll1 {
        ParsedSlashCommand::Poll { question, options } => {
            assert_eq!(question, "Best video codec?");
            assert_eq!(options, vec!["AV1", "H.265", "VP9"]);
        }
        _ => panic!("Expected ParsedSlashCommand::Poll"),
    }

    // 2. Pipe Poll syntax
    let poll2 = SlashCommandParser::parse(
        "/poll Watch next? | Interstellar | Blade Runner 2049 | Arrival",
        None,
    )
    .unwrap();
    match poll2 {
        ParsedSlashCommand::Poll { question, options } => {
            assert_eq!(question, "Watch next?");
            assert_eq!(options, vec!["Interstellar", "Blade Runner 2049", "Arrival"]);
        }
        _ => panic!("Expected ParsedSlashCommand::Poll"),
    }

    // 3. Poll errors
    assert!(SlashCommandParser::parse("/poll", None).is_err());
    assert!(SlashCommandParser::parse("/poll \"Single question with no options\"", None).is_err());
    assert!(SlashCommandParser::parse("/poll SingleQuestion |", None).is_err());

    // 4. Sync-status
    let status_cmd = SlashCommandParser::parse("/sync-status", None).unwrap();
    assert_eq!(status_cmd, ParsedSlashCommand::SyncStatus);

    // 5. Unrecognized command
    assert!(SlashCommandParser::parse("/unknown_command_xyz", None).is_err());
    assert!(SlashCommandParser::parse("no_slash_at_start", None).is_err());
}

#[test]
fn test_slash_command_dispatcher_actions_and_state() {
    let mut dispatcher = SlashCommandDispatcher::new();

    // Set diagnostics
    dispatcher.set_sync_diagnostics(SyncDiagnostics {
        rtt_ms: 320.0,
        clock_offset_ms: -14.5,
        speed_multiplier: 0.985,
        peer_count: 4,
        current_pts_ms: 125_000,
        is_paused: false,
        pll_zone: "MicroSpeed".to_string(),
    });

    // 1. Dispatch /seek
    let act_seek = dispatcher.dispatch("/seek 02:15", None).unwrap();
    assert_eq!(act_seek, DispatchAction::Seek { target_pts_ms: 135_000 });

    // 2. Dispatch /bookmark
    let act_bm = dispatcher.dispatch("/bookmark Highlight", Some(125_000)).unwrap();
    match act_bm {
        DispatchAction::Bookmark(bm) => {
            assert_eq!(bm.label, Some("Highlight".to_string()));
            assert_eq!(bm.pts_ms, 125_000);
        }
        _ => panic!("Expected DispatchAction::Bookmark"),
    }
    assert_eq!(dispatcher.bookmarks().len(), 1);

    // 3. Dispatch /clip
    let act_clip = dispatcher.dispatch("/clip 00:30 01:00 Chorus", None).unwrap();
    match act_clip {
        DispatchAction::Clip(clip) => {
            assert_eq!(clip.start_pts_ms, 30_000);
            assert_eq!(clip.end_pts_ms, 60_000);
            assert_eq!(clip.title, Some("Chorus".to_string()));
        }
        _ => panic!("Expected DispatchAction::Clip"),
    }
    assert_eq!(dispatcher.clips().len(), 1);

    // 4. Dispatch /poll
    let act_poll = dispatcher
        .dispatch("/poll \"Ready?\" \"Yes\" \"Wait\"", Some(125_000))
        .unwrap();
    match act_poll {
        DispatchAction::Poll(poll) => {
            assert_eq!(poll.poll_id, 1);
            assert_eq!(poll.question, "Ready?");
            assert_eq!(poll.options.len(), 2);
            assert_eq!(poll.votes, vec![0, 0]);
        }
        _ => panic!("Expected DispatchAction::Poll"),
    }
    assert!(dispatcher.active_poll().is_some());

    // 5. Dispatch /sync-status
    let act_status = dispatcher.dispatch("/sync-status", Some(125_000)).unwrap();
    match act_status {
        DispatchAction::SyncStatus { report, diagnostics } => {
            assert!(report.contains("Syncverse Sync Status"));
            assert!(report.contains("Round-Trip RTT: 320.00 ms"));
            assert!(report.contains("Speed Steering: 0.9850x"));
            assert!(report.contains("PLL Zone:       MicroSpeed"));
            assert_eq!(diagnostics.peer_count, 4);
        }
        _ => panic!("Expected DispatchAction::SyncStatus"),
    }
}

// ============================================================================
// PART 3: CLI DEFAULTS & SUBCOMMAND EXECUTION VERIFICATION
// ============================================================================

fn resolve_binary_path() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // Remove test binary name
    if path.ends_with("deps") {
        path.pop(); // Remove deps
    }
    path.push("syncverse");
    if !path.exists() {
        // Fallback to debug build
        path = PathBuf::from("target/debug/syncverse");
    }
    path
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut in_esc = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_esc = true;
        } else if in_esc {
            if c.is_ascii_alphabetic() {
                in_esc = false;
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn test_cli_version_and_help() {
    let bin = resolve_binary_path();
    assert!(bin.exists(), "Binary syncverse must exist at {:?}", bin);

    // 1. version
    let out_ver = Command::new(&bin)
        .arg("version")
        .output()
        .expect("failed to run syncverse version");
    assert!(out_ver.status.success());
    let stdout_ver = strip_ansi(&String::from_utf8_lossy(&out_ver.stdout));
    assert!(stdout_ver.contains("syncverse v0.1.1"));

    // 2. --help
    let out_help = Command::new(&bin)
        .arg("--help")
        .output()
        .expect("failed to run syncverse --help");
    assert!(out_help.status.success());
    let stdout_help = strip_ansi(&String::from_utf8_lossy(&out_help.stdout));
    assert!(stdout_help.contains("SYNCVERSE"));
    assert!(stdout_help.contains("USAGE:"));
    assert!(stdout_help.contains("COMMANDS:"));
}

#[test]
fn test_cli_inspect_synthetic_mkv_and_mp4() {
    let bin = resolve_binary_path();

    // 1. inspect synthetic-mkv
    let out_mkv = Command::new(&bin)
        .args(["inspect", "synthetic-mkv"])
        .output()
        .expect("inspect synthetic-mkv failed");
    assert!(out_mkv.status.success());
    let stdout_mkv = strip_ansi(&String::from_utf8_lossy(&out_mkv.stdout));
    assert!(stdout_mkv.contains("Probing container: synthetic-mkv"));
    assert!(stdout_mkv.contains("EBML/MKV probe match:  YES"));

    // 2. inspect synthetic-mp4
    let out_mp4 = Command::new(&bin)
        .args(["inspect", "synthetic-mp4"])
        .output()
        .expect("inspect synthetic-mp4 failed");
    assert!(out_mp4.status.success());
    let stdout_mp4 = strip_ansi(&String::from_utf8_lossy(&out_mp4.stdout));
    assert!(stdout_mp4.contains("Probing container: synthetic-mp4"));
    assert!(stdout_mp4.contains("ISOBMFF/MP4 probe match: YES"));
}

#[test]
fn test_cli_play_headless_and_ticks() {
    let bin = resolve_binary_path();

    // 1. play synthetic-mkv --headless
    let out_headless = Command::new(&bin)
        .args(["play", "synthetic-mkv", "--headless"])
        .output()
        .expect("play --headless failed");
    assert!(out_headless.status.success());
    let stdout_h = strip_ansi(&String::from_utf8_lossy(&out_headless.stdout));
    assert!(stdout_h.contains("[PLAYBACK] Initializing playback runtime for: synthetic-mkv"));
    assert!(stdout_h.contains("Simulating 5 consecutive ticks with live OSD Telemetry:"));
    assert!(stdout_h.contains("[DONE] Playback runtime verified with lock-free atomic snapshots."));

    // 2. play synthetic-mkv --ticks 3
    let out_ticks = Command::new(&bin)
        .args(["play", "synthetic-mkv", "--ticks", "3"])
        .output()
        .expect("play --ticks failed");
    assert!(out_ticks.status.success());
    let stdout_t = strip_ansi(&String::from_utf8_lossy(&out_ticks.stdout));
    assert!(stdout_t.contains("Simulating 3 consecutive ticks"));
}

#[test]
fn test_cli_play_terminal_fallback_when_piped() {
    let bin = resolve_binary_path();

    // Piped stdin and stdout (non-terminal)
    let child = Command::new(&bin)
        .args(["play", "synthetic-mkv", "--terminal"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn terminal play");

    let out = child.wait_with_output().expect("wait failed");
    assert!(out.status.success());
    let stdout_str = strip_ansi(&String::from_utf8_lossy(&out.stdout));
    // Non-terminal fallback executes bounded ticks without crashing
    assert!(stdout_str.contains("[PLAYBACK] Initializing playback runtime for: synthetic-mkv"));
    assert!(stdout_str.contains("Simulating 5 consecutive ticks"));
}

#[test]
fn test_cli_chat_demo_and_party_demo() {
    let bin = resolve_binary_path();

    // 1. chat demo
    let out_chat = Command::new(&bin)
        .args(["chat", "demo"])
        .output()
        .expect("chat demo failed");
    assert!(out_chat.status.success());
    let stdout_chat = strip_ansi(&String::from_utf8_lossy(&out_chat.stdout));
    assert!(stdout_chat.contains("Full-Text Search Tests:"));
    assert!(stdout_chat.contains("Search 'intro':"));
    assert!(stdout_chat.contains("Generated Markdown Transcript:"));

    // 2. party demo
    let out_party = Command::new(&bin)
        .args(["party", "demo"])
        .output()
        .expect("party demo failed");
    assert!(out_party.status.success());
    let stdout_party = strip_ansi(&String::from_utf8_lossy(&out_party.stdout));
    assert!(stdout_party.contains("Executing real UDP loopback"));
    assert!(stdout_party.contains("Simulating Transpacific High-RTT Link"));
    assert!(stdout_party.contains("Watch-party sync convergence verified"));
}

#[test]
fn test_cli_default_launch_headless_fallback() {
    let bin = resolve_binary_path();

    // In environment with no display (WAYLAND_DISPLAY="" DISPLAY="") and piped stdin:
    // Default launch attempts launch_desktop_gui(None), detects lack of graphical context,
    // and cleanly falls back to playback simulation.
    let child = Command::new(&bin)
        .env("WAYLAND_DISPLAY", "")
        .env("DISPLAY", "")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn default launch");

    let out = child.wait_with_output().expect("wait failed");
    assert!(out.status.success(), "Default launch must exit cleanly");
    let stdout = strip_ansi(&String::from_utf8_lossy(&out.stdout));
    let stderr = strip_ansi(&String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("[GUI] Launching Syncverse Desktop Graphical Player..."));
    assert!(stderr.contains("Falling back to terminal presentation mode..."));
    assert!(stdout.contains("[DONE] Playback runtime verified with lock-free atomic snapshots."));
}

#[test]
fn test_cli_unknown_command_returns_error() {
    let bin = resolve_binary_path();

    let out = Command::new(&bin)
        .arg("--unknown-flag-12345")
        .output()
        .expect("unknown flag failed");
    assert!(!out.status.success(), "Unknown flag must return non-zero exit code");
    let stderr = strip_ansi(&String::from_utf8_lossy(&out.stderr));
    assert!(stderr.contains("[ERROR] Unknown command"));
}
