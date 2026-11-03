## 2026-10-30T22:10:11Z

You are worker_p5_ui, a teamwork_worker.

Your assigned working directory is:
/home/gautham/syncverse/.agents/teamwork/worker_p5_ui/

MANDATORY FIRST STEP:
Read /home/gautham/syncverse/.agents/teamwork/ORIGINAL_REQUEST.md and /home/gautham/syncverse/AGENTS.md before starting any work.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

EXCLUSIVE WRITE OWNERSHIP:
You EXCLUSIVELY own files within `crates/syncverse-ui/`. DO NOT modify files in other crates (another worker is operating in parallel on non-UI crates).

DETAILED SCOPE:
Review the survey report at:
/home/gautham/syncverse/.agents/teamwork/explorer_p5_survey_3/handoff.md
Implement the following genuine features and improvements in `crates/syncverse-ui/`:

1. `src/snapshot.rs`:
   - Add status bitflags in unused bits of `playback_state: u64`:
     ```rust
     /// Status bitflag indicating audio output is currently muted.
     pub const SNAPSHOT_FLAG_MUTED: u64 = 0x1000;
     /// Status bitflag indicating loop / repeat playback mode is enabled.
     pub const SNAPSHOT_FLAG_LOOP_ENABLED: u64 = 0x2000;
     ```
   - Add helper methods `is_muted(&self) -> bool` and `is_loop_enabled(&self) -> bool`.
   - CRITICAL INVARIANT: `std::mem::size_of::<PlayerUiSnapshot>()` MUST REMAIN EXACTLY 72 BYTES! Ensure all tests verify this.

2. `src/command.rs`:
   - Ensure `PlayerCommand` includes `StepFrame { forward: bool }` and `ToggleLoop` (or `SetLoop(bool)`).

3. `src/gui/app.rs`:
   - Frame Stepping: Bind `,` (comma) to step 1 frame backward (~42ms) and `.` (period) to step 1 frame forward (~42ms) while paused. Update OSD notification with step feedback.
   - Hierarchical Quick Seeks: Left/Right arrows (±5s), Shift+Arrows (±30s), Ctrl+Left/Right (±60s). Add menu items for these jumps in the Playback menu.
   - Loop Playback Toggle: Bind `L` (or `Ctrl+L`) to toggle loop playback mode. Show OSD notification ("Loop: Enabled" / "Loop: Disabled").
   - Canonical Struct Naming: Rename `SyncplayApp` struct to `SyncverseApp`. Provide backward-compatible alias `pub type SyncplayApp = SyncverseApp;`.
   - Clipboard Path Loading: In `Ctrl+V` (when not editing text), check `ctx.input(|i| i.raw.clipboard_text)` and load media if valid file path/URI.

4. `src/gui/dialogs.rs`:
   - In `SubtitleConfigDialog`, expose appearance styling controls: font scale slider (50% to 200%), text color edit button, background fill opacity, outline shadow toggle. Hook into `SubtitleOverlayConfig`.

5. `src/gui/recent_files.rs`:
   - Add `APPDATA` / `USERPROFILE` fallback in `config_path()` for native Windows support.

6. `src/gui/sidebar.rs`:
   - Add a collapsible Search Bar at the top of the chat area in the watch-party sidebar, allowing users to search chat records using the existing `InvertedIndexFts` engine in real time.

7. `src/gui/controls.rs`:
   - Add responsive layout guard: dynamically fold or collapse speed / aspect combo boxes when controls bar width is $< 700\text{px}$ to prevent visual overflow on compact windows.

STANDARDS & VERIFICATION:
- Must compile cleanly with zero warnings under `#![deny(warnings)]`.
- Must satisfy `#![deny(missing_docs)]` with thorough docstrings on all new/modified public items.
- Run:
  1. `cargo check --package syncverse-ui --all-targets`
  2. `cargo clippy --package syncverse-ui --all-targets -- -D warnings`
  3. `cargo test --package syncverse-ui -- --nocapture`
- Write completion report to:
  /home/gautham/syncverse/.agents/teamwork/worker_p5_ui/handoff.md
- When finished, send a message to parent.
