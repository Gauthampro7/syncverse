# DISPATCH — worker_p4_m3

## Mandatory Integrity Warning
> DO NOT CHEAT. All implementations must be genuine. DO NOT
> hardcode test results, create dummy/facade implementations, or
> circumvent the intended task. A teamwork_preview_auditor will independently
> verify your work. Integrity violations WILL be detected and your
> work WILL be rejected.

## Mission
You are `worker_p4_m3`, tasked with implementing Milestone 3: Flawless Desktop Graphical Player UI (R1).
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m3`.

You must read:
1. `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically `## 2026-10-25T20:12:42Z` R1 requirements)
2. `/home/gautham/syncplay-alt/.agents/teamwork/orchestrator_4/PROJECT.md`
3. Explorer Reports:
   - `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_1/handoff.md` (Windowing & GPU Video Viewport)
   - `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_2/handoff.md` (Controls Bar, Debounced Seekbar, Volume & HUD)
   - `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_3/handoff.md` (Sidebar, Chat, File Picker & CLI Entrypoint)

## Implementation Tasks

### 1. `crates/syncplay-ui`: Graphical Modules & Command Expansion
- In `crates/syncplay-ui/Cargo.toml`:
  Ensure `eframe = "0.37"`, `egui = "0.37"`, `rfd = "0.17"` are enabled.
- In `crates/syncplay-ui/src/command.rs`:
  Expand `PlayerCommand` to include all required variants:
  `TogglePause`, `Seek(u64)`, `SetSpeed(f64)`, `Stop`, `SetVolume(f32)`, `ToggleMute`, `OpenFile(PathBuf)`, `SendChatMessage(String)`, `ExecuteSlashCommand(String)`.
- In `crates/syncplay-ui/src/debouncer.rs`:
  Implement `SeekDebouncer` supporting 10Hz rate-limiting on drag and immediate seek on release.
- In `crates/syncplay-ui/src/gui/`:
  * `viewport.rs`: GPU video viewport with `ViewportGeometry::compute` (automatic aspect-ratio letterboxing/pillarboxing with black bars), texture updates from `VideoFrameRGBA` via `TextureHandle`, and F11/double-click fullscreen toggle.
  * `controls.rs`: Bottom docked bar (`#141417` sleek dark), Play/Pause button with Spacebar shortcut (suppressed when typing), Stop button (`⏹`), interactive seekbar with floating cursor hover tooltip (`HH:MM:SS`) and 10Hz debounced scrubbing, volume slider ($0-100\%$) with mute toggle ('M' shortcut), timecode HUD (`HH:MM:SS / HH:MM:SS`), and speed selector (`0.5x`, `0.75x`, `1.0x`, `1.25x`, `1.5x`, `2.0x`).
  * `sidebar.rs`: Collapsible right watch-party panel with room members list, connection RTT, sync health badge (Green $<15\text{ms}$, Amber $15-1500\text{ms}$, Red $>1500\text{ms}$), scrollable smart chat with dual timestamps ($t_{wall}, t_{media}$) and click-to-seek, and slash-command input (`/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`).
  * `file_dialog.rs`: Non-blocking native file picker via `rfd::FileDialog` and drag-and-drop file loading into the window.
  * `app.rs`: `SyncplayApp` (`eframe::App`) integrating viewport, controls bar, and sidebar, polling `PlayerUiSnapshot`, and dispatching `PlayerCommand`s.
- In `crates/syncplay-ui/src/lib.rs`:
  Export the GUI modules and `SyncplayApp`, `run_desktop_player`.

### 2. `crates/syncplay-core`: CLI & Desktop Integration
- In `crates/syncplay-core/src/main.rs`:
  * Running `syncplay-alt [FILE]` by default launches the desktop graphical window (`run_desktop_player`).
  * Preserve `--headless` and `--terminal` flags for automated testing and TTY environments.
  * Subcommands `party host` and `party connect` launch the desktop GUI with watch-party networking active.

### 3. Verification Commands
Before submitting handoff, you must run and verify:
1. `cargo check --workspace --all-targets` (Must have 0 warnings, 0 errors)
2. `cargo clippy --workspace --all-targets -- -D warnings` (Must have 0 warnings)
3. `cargo test --workspace` (All tests must pass)
4. `python3 tests/empirical_challenge/test_ui_renderer.py` (Must pass 100%)
5. `./scripts/audit_dependencies.sh` (Must pass with 0 violations)
6. `python3 scripts/intent_harness.py` (Must achieve 100.0% compliance)
7. `./scripts/verify_phase4.sh` (All 8 stages must pass cleanly)

## Output Requirements
Document all modified files, architectural details, and verification command outputs in `/home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m3/handoff.md`.
Send a completion message to the orchestrator.

## 2026-10-26T00:00:34Z
You are `worker_p4_m3`. Read your task assignment at `/home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m3/DISPATCH.md`.
Also read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically R1 requirements).
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Implement Milestone 3: Flawless Desktop Graphical Player UI (R1):
1. `crates/syncplay-ui`:
   - Expand `PlayerCommand` with all 9 variants.
   - Implement `SeekDebouncer` in `debouncer.rs`.
   - Implement `gui/` modules: `viewport.rs` (aspect ratio letterboxing/pillarboxing with black bars, texture uploads, F11/double-click fullscreen), `controls.rs` (bottom bar, Spacebar play/pause, Stop, seekbar with hover tooltip & 10Hz debounced scrubbing, volume slider & mute, timecode HUD, speed selector), `sidebar.rs` (members, RTT, sync badge, real-time chat with dual timestamps, slash commands), `file_dialog.rs` (non-blocking rfd native dialog, drag-and-drop, recent files), and `app.rs` (`SyncplayApp`).
   - Export `SyncplayApp` and `run_desktop_player` in `crates/syncplay-ui/src/lib.rs`.
2. `crates/syncplay-core/src/main.rs`:
   - Launch desktop GUI by default (`syncplay-alt [FILE]`), preserving `--headless` and `--terminal` flags.
3. Verification:
   - `cargo check --workspace --all-targets` (0 warnings)
   - `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings)
   - `cargo test --workspace` (100% passing)
   - `python3 tests/empirical_challenge/test_ui_renderer.py` (100% passing)
   - `./scripts/audit_dependencies.sh` (0 violations)
   - `python3 scripts/intent_harness.py` (100.0% compliance)
   - `./scripts/verify_phase4.sh` (all 8 stages passing)

Write your report to `/home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m3/handoff.md` and send a completion message to the orchestrator.
