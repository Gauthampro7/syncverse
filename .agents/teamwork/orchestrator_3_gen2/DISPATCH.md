# Dispatch Instructions: orchestrator_3_gen2

## 2026-10-25T20:06:00Z

You are the Successor Project Orchestrator (`orchestrator_3_gen2`) for Phase 3 of Syncplay-Alt.

## Your Identity & Workspace
- Identity: orchestrator_3_gen2
- Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/orchestrator_3_gen2
- Workspace root: /home/gautham/syncplay-alt
- Predecessor: orchestrator_3 (/home/gautham/syncplay-alt/.agents/teamwork/orchestrator_3)
- Authoritative User Request: /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (latest request under `## 2026-10-25T15:21:44Z`)

## Context & Current Phase State
Your predecessor `orchestrator_3` deployed 28 subagents and successfully drove:
1. **Phase 0 (Survey)**: COMPLETED by 3 explorers.
2. **Milestone 1 (Requirement R3: Pure-Rust Bitstream Decoders)**: GATE PASSED (Annex B H.264/AVC, SPS/PPS Exp-Golomb, PCM/Opus/Vorbis, TrueColor half-block renderer).
3. **Milestone 2 (Requirement R2: Pure-Rust Audio Output & Resampling)**: GATE PASSED (Linear resampler with corrected physical step formula, MasterClock audio pacer, PipeAudioSink targeting pw-cat/aplay, NullAudioSink).
4. **Milestone 3 (Requirement R4: Watch-Party Networking & In-Band Smart Chat)**:
   - Implementation completed by `worker_p3_network`.
   - Wire packet `0x08`, non-echo relaying, and `syncplay_chat.bin` bridging implemented and tested (`multi_client_watch_party_chat.rs`, `party_chat_integration.rs`).
   - `reviewer_p3_network_1` (APPROVE), `reviewer_p3_network_2` (APPROVE), and `auditor_p3_network` (CLEAN) delivered their handoffs.
   - Action: Close Milestone 3 Gate as PASS in `GATE_STATUS.md`.

## Remaining Milestones to Complete
### Milestone 4 (Requirement R1: Fedora Desktop Application & Interactive Terminal/OSD Player Launcher)
Transform `syncplay-alt` into a fully accessible standalone application:
- Create `~/.local/share/applications/syncplay-alt.desktop` with proper Exec bindings, MIME types (`video/x-matroska`, `video/mp4`, `video/webm`, `video/quicktime`), and modern icon integration.
- Implement interactive terminal/OSD presentation mode (`syncplay-alt play <FILE>`) featuring a live, 60fps responsive presentation loop:
  * Full keyboard navigation: `Space` (Pause/Resume with barrier sync), `Left`/`Right` (Keyframe seek ±5s), `Shift+Left`/`Right` (Fine seek ±1s), `[` / `]` (Continuous speed shift ±0.1x via SeqLock), `M` (Mute/Unmute), `Up`/`Down` (Volume ±5%), `T`/`C` (Toggle floating chat HUD), `S` (Toggle sync diagnostics HUD), `/` (Interactive slash-command bar), `Q`/`Esc` (Clean exit).
  * Sleek terminal aesthetic OSD with ANSI 24-bit TrueColor / box-drawing / half-block video rendering, real-time scrubber progress bar, timecode HUD (`format_timecode_hud`), tri-color sync health badge, and floating bullet comments.
  * Fix the timing race in `crates/syncplay-ui/src/snapshot.rs` test `test_triple_buffer_concurrency_stress` (ensure consumer loop reads at least once before checking `assert!(reads > 0)`).
- Execute standard iteration loop: Worker -> Reviewers (2) -> Challengers (2) -> Forensic Auditor.

### Milestone 5 (Requirement R5: System Integration, Intent Verification Harness & Stress Hardening)
- Integrate all subsystems under `crates/syncplay-core`.
- Verify full CLI commands:
  * `syncplay-alt play <FILE>`
  * `syncplay-alt party host --port <PORT> --file <MEDIA>`
  * `syncplay-alt party connect <HOST:PORT> --file <MEDIA>`
- Verify workspace compilation, clippy (`-D warnings`), all tests pass (`cargo test --workspace`), zero banned dependencies (`./scripts/audit_dependencies.sh`), and 100.0% Intent Compliance (`python3 scripts/intent_harness.py`).

### Milestone 6: Final Comprehensive Victory Audit & Attestation
- Run final validation checks across all empirical challenges and scripts.
- Ensure total subagents deployed across Phase 3 satisfies the user request of 35+ subagents (currently at 28, deploy specialists across M4, M5, M6 to reach 35+).
- Claim victory with a comprehensive handoff report to Sentinel.

## Key Constraints
- Pure Rust, zero banned dependencies (`ffmpeg`, `vlc`, `mpv`, `sqlite`, etc.).
- Zero compiler and clippy warnings.
- Keep `PATH` including `$HOME/.cargo/bin`.
- Do not write code directly — orchestrate specialists (workers, reviewers, challengers, auditors).
