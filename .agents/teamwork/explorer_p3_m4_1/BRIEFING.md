# BRIEFING — 2026-10-25T20:30:00Z

## Mission
Investigate the Terminal Input & Presentation Loop Architecture in `syncplay-core` (`src/coordinator.rs`, `src/main.rs`) and pure Rust terminal handling for Milestone 4 Requirement R1.

## 🔒 My Identity
- Archetype: explorer
- Roles: technical exploration specialist
- Working directory: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_m4_1
- Original parent: a21e166c-91fa-462c-b129-2d84da07e92f
- Milestone: Milestone 4 (Requirement R1: Fedora Desktop Application & Interactive Terminal/OSD Player Launcher)

## 🔒 Key Constraints
- Read-only investigation — do NOT implement
- Zero forbidden multimedia/database engines (AGENTS.md)
- Pure Rust terminal handling without banned external frameworks
- Pure Rust architecture invariants

## Current Parent
- Conversation ID: a21e166c-91fa-462c-b129-2d84da07e92f
- Updated: 2026-10-25T20:30:00Z

## Investigation State
- **Explored paths**:
  - `crates/syncplay-core/src/main.rs`, `coordinator.rs`, `aggregator.rs`, `audio_sink.rs`, `lib.rs`
  - `crates/syncplay-ui/src/terminal.rs`, `overlay.rs`, `snapshot.rs`, `command.rs`
  - `crates/syncplay-clock/src/master_clock.rs`, `audio_pacer.rs`
  - `crates/syncplay-codecs/src/color.rs`, `h264/decoder.rs`
  - `crates/syncplay-chat/src/commands.rs`, `record.rs`, `store.rs`
  - `crates/syncplay-sync/src/barrier.rs`, `host.rs`, `client.rs`
  - `tests/empirical_challenge/test_ui_renderer.py`, `scripts/intent_harness.py`, `scripts/audit_dependencies.sh`
- **Key findings**:
  - `run_interactive_player` currently throttles at 33ms (~30fps) instead of 16.6ms (~60fps).
  - Terminal raw mode via `TerminalGuard` with `stty min 0 time 0` is pure standard library Rust and fully compliant.
  - Size polling on every frame forks `stty` 60x/sec; must be throttled to ~300ms intervals to prevent frame jitter.
  - `KeyCommand::ToggleSyncHud` and `KeyCommand::OpenCommandPrompt` were stubbed out and need full implementation.
  - `PROJECT.md` specifies interface contract `InteractivePlayer: fn run_presentation_loop(&mut self) -> Result<(), CoreError>;` which should be implemented in `crates/syncplay-core/src/interactive.rs`.
  - Live telemetry from `TripleBufferSnapshotExchange` must replace hardcoded values in HUD.
- **Unexplored areas**: None for M4 R1 terminal investigation.

## Key Decisions Made
- Recommended architectural decomposition: create `InteractivePlayer` in `syncplay-core/src/interactive.rs` with `PlayerInputMode::{Normal, CommandBar}`, keyframe seek snapping, live telemetry aggregator coupling, throttled terminal size polling, and bounded tick testing support.
- Documented full findings in `handoff.md`.

## Artifact Index
- `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_m4_1/DISPATCH.md` — Initial dispatch message
- `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_m4_1/BRIEFING.md` — Persistent working memory
- `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_m4_1/progress.md` — Liveness heartbeat and progress tracking
- `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_m4_1/handoff.md` — Comprehensive technical handoff report
