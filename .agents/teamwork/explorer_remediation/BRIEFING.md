# BRIEFING — 2026-10-27T19:50:00Z

## Mission
Investigate and design the end-to-end remediation plan for the subtitle pipeline failure, connecting container demuxing, external subtitle scanning/parsing, playback session management, UI event propagation, OSD/cycling hotkeys, and integration testing.

## 🔒 My Identity
- Archetype: teamwork_explorer
- Roles: explorer, synthesis
- Working directory: /home/gautham/syncverse/.agents/teamwork/explorer_remediation
- Original parent: 6fec964f-e9e0-4542-a4f0-f5005e6b3d1e
- Milestone: Subtitle Pipeline Remediation

## 🔒 Key Constraints
- Read-only investigation — do NOT implement
- Adhere strictly to AGENTS.md zero-warning, no-banned-crates, strongly-typed errors, and architectural invariants
- Concrete line-by-line blueprint with exact code changes and verification methods

## Current Parent
- Conversation ID: 6fec964f-e9e0-4542-a4f0-f5005e6b3d1e
- Updated: 2026-10-27T19:50:00Z

## Investigation State
- **Explored paths**:
  - `crates/syncverse-core/src/playback_runtime.rs`
  - `crates/syncverse-ui/src/gui/app.rs`
  - `crates/syncverse-ui/src/event.rs`
  - `crates/syncverse-ui/src/command.rs`
  - `crates/syncverse-ui/src/gui/subtitle_overlay.rs`
  - `crates/syncverse-ui/src/gui/dialogs.rs`
  - `crates/syncverse-demux/src/subtitles/scanner.rs`
  - `crates/syncverse-demux/src/subtitles/srt.rs`
  - `crates/syncverse-demux/src/subtitles/vtt.rs`
  - `crates/syncverse-demux/src/subtitles/ass.rs`
  - `crates/syncverse-core/tests/adversarial_desktop_player_challenge.rs`
- **Key findings**:
  - `PlaybackSession::open` ignored embedded subtitle tracks and never called `scan_external_subtitles`.
  - `playback_runtime.rs` hardcoded `UiEvent::TracksDiscovered { subtitle_tracks: Vec::new() }`.
  - `PlaybackSession` ignored `PlayerCommand::SelectSubtitleTrack` and `PlayerCommand::CycleSubtitleTrack`.
  - `app.rs:1033` had an empty stub for `SubtitleConfigAction::LoadExternal(_path) => {}`.
  - `app.rs:466-484` hotkey cycling had no tracks to cycle through, always showing "Subtitles: Off" and never updating `active_subtitle_cues`.
  - Integration test in `adversarial_desktop_player_challenge.rs` tested an in-test closure instead of end-to-end player.
- **Unexplored areas**: None. Blueprint covers all 4 crates, UI, runtime, and end-to-end testing.

## Key Decisions Made
- Symmetrical design for subtitles matching audio track switching and cycling in `PlaybackSession`.
- SPSC/Channel event `UiEvent::SubtitleDataDiscovered` to transport full parsed track cues from session to UI.
- Immediate external scanner invocation in `app.load_file` and `PlaybackSession::open` for seamless headless and GUI operation.
- Complete implementation of `SubtitleConfigAction::LoadExternal` using pure-Rust parsers (`parse_srt`, `parse_vtt`, `parse_ass`).
- Replace mock closure in `adversarial_desktop_player_challenge.rs` with authentic end-to-end worker integration test.

## Artifact Index
- DISPATCH.md — Received dispatch instructions
- BRIEFING.md — Persistent context & state
- progress.md — Liveness & progress tracking
- handoff.md — Final investigation report
