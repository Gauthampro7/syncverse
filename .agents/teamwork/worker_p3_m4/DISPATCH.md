## 2026-10-25T20:16:52Z
You are worker_p3_m4, the implementation specialist for Milestone 4 (Requirement R1: Fedora Desktop Application & Interactive Terminal/OSD Player Launcher).
Your working directory is: /home/gautham/syncplay-alt/.agents/teamwork/worker_p3_m4

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Read the authoritative documents:
1. Authoritative User Request: /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically ## 2026-10-25T15:21:44Z)
2. Project Plan: /home/gautham/syncplay-alt/.agents/teamwork/orchestrator_3_gen2/PROJECT.md
3. Architecture Invariants: /home/gautham/syncplay-alt/AGENTS.md
4. Explorer Reports:
   - /home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_m4_1/handoff.md
   - /home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_m4_2/handoff.md
   - /home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_m4_3/handoff.md

Your tasks:
1. Fix the snapshot concurrency test timing race:
   - In `crates/syncplay-ui/src/snapshot.rs:181-221` (`test_triple_buffer_concurrency_stress`) and `crates/syncplay-core/tests/adversarial_crates_5_to_8.rs:477-529`:
   - Use `std::sync::Barrier::new(2)` for producer/consumer synchronized startup and a post-tested loop (`loop { ... if stop_flag.load(Ordering::Acquire) { break; } }`) with post-stop drain check to guarantee `reads >= 1` and zero torn reads.
2. Implement OSD rendering and formatting in `crates/syncplay-ui`:
   - Create `crates/syncplay-ui/src/osd.rs` and `crates/syncplay-ui/src/render.rs` (and re-export in `lib.rs`):
     * Scrubber progress bar with custom box-drawing characters (`━`, `╺`, `─`, `█`, `●`)
     * Timecode HUD formatting (`format_timecode_hud`)
     * Tri-color sync health badge (`evaluate_tight_sync_badge`: Green <15ms, Yellow 15-50ms, Red >50ms, plus OpenSpec zone mapping)
     * Floating bullet comments with 24-bit TrueColor alpha blending (4.0s dwell + 1.0s linear fade-out)
     * ANSI half-block video rendering support
3. Implement `InteractivePlayer` in `crates/syncplay-core/src/interactive.rs`:
   - Fulfill the `PROJECT.md:40` contract: `InteractivePlayer: fn run_presentation_loop(&mut self) -> Result<(), CoreError>;`
   - Live 60fps presentation loop with non-blocking raw mode via `TerminalGuard`
   - Throttled terminal size querying (~300ms) to avoid process fork overhead
   - Flicker-free cursor-homed rendering (`\x1b[1;1H`) with single-pass stdout flush
   - Full keyboard navigation:
     * `Space`: Pause/Resume
     * `Left`/`Right`: Keyframe seek ±5s
     * `Shift+Left`/`Right`: Fine seek ±1s
     * `[`/`]`: Continuous speed shift ±0.1x via SeqLock
     * `M`: Mute/Unmute
     * `Up`/`Down`: Volume ±5%
     * `T`/`C`: Toggle floating chat HUD
     * `S`: Toggle sync diagnostics HUD
     * `/`: Interactive slash-command prompt (`/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`)
     * `Q`/`Esc`: Clean exit restoring terminal state
   - `max_ticks: Option<usize>` support for headless testing
   - Wire `InteractivePlayer` into `crates/syncplay-core/src/lib.rs` and `main.rs` (`run_interactive_player`)
4. Fedora Desktop Application Packaging:
   - Update `assets/syncplay-alt.desktop` and install to `~/.local/share/applications/syncplay-alt.desktop` per XDG Desktop Entry Spec v1.5 recommendations from `explorer_p3_m4_3`.
   - Update `scripts/install_linux.sh` to validate with `desktop-file-validate` and install desktop entry and icons.
   - Add empirical test `tests/empirical_challenge/test_desktop_packaging.py`.
   - Add unit/integration tests in `crates/syncplay-core/tests/cli_and_coordinator_integration.rs`.
5. Run verification commands:
   - `./scripts/audit_dependencies.sh`
   - `cargo check --workspace --all-targets`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace`
   - `python3 tests/empirical_challenge/test_desktop_packaging.py`
   - `python3 tests/empirical_challenge/test_ui_renderer.py`
   - `python3 scripts/intent_harness.py`
6. Write detailed handoff report in `/home/gautham/syncplay-alt/.agents/teamwork/worker_p3_m4/handoff.md` and message orchestrator when complete.
