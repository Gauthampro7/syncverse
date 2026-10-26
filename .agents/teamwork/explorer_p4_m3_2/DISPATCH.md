# DISPATCH — explorer_p4_m3_2

## Mission
You are `explorer_p4_m3_2`, exploring the Playback Controls Bar, Seekbar & HUD for Milestone 3 (Flawless Desktop Graphical Player UI).
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_2`.

You must read:
1. `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically `## 2026-10-25T20:12:42Z` R1 requirements)
2. `/home/gautham/syncplay-alt/.agents/teamwork/orchestrator_4/PROJECT.md`
3. Codebase files:
   - `crates/syncplay-ui/Cargo.toml`
   - `crates/syncplay-ui/src/lib.rs`
   - `crates/syncplay-ui/src/debouncer.rs`
   - `crates/syncplay-ui/src/renderer.rs`
   - `crates/syncplay-core/src/coordinator.rs`

## Investigation Scope
1. **Bottom Docked Controls Bar Layout**:
   - Modern, sleek dark theme (`egui::Visuals::dark()`, custom padding, rounded button aesthetics).
   - Play / Pause button with icon (`▶` / `⏸`) and Spacebar toggle.
   - Stop button (`⏹`).
2. **Interactive Seekbar**:
   - Time range: `0` to `media_duration_ms`.
   - Hover timestamp tooltip: showing `HH:MM:SS` (or `MM:SS`) dynamically computed from mouse X coordinate relative to seekbar bounding rect.
   - 10Hz debounced drag-scrubbing:
     * How to utilize or integrate `crates/syncplay-ui/src/debouncer.rs` (`SeekDebouncer`) so drag operations only emit seek commands at $\le 10\text{Hz}$ (every $\ge 100\text{ms}$), plus immediate seek on mouse release.
   - Click-to-seek: immediate seek on click.
3. **Volume & Mute Controls**:
   - Volume slider ($0.0$ to $1.0$, displayed as $0\%$ to $100\%$).
   - Mute toggle button (`🔊` / `🔇`) and 'M' keyboard shortcut.
4. **Timecode HUD**:
   - Current presentation time / total duration: `HH:MM:SS / HH:MM:SS`.
5. **Playback Speed Selector**:
   - Speed buttons or dropdown: `0.5x`, `0.75x`, `1.0x`, `1.25x`, `1.5x`, `2.0x`.
   - Sends `PlayerCommand::SetSpeed(rate)` to coordinator.

## Output Requirements
Document your component design, state management, and draft implementation in `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_2/handoff.md`.
Send a completion message to the orchestrator.

## 2026-10-25T23:51:35Z
[Message] timestamp=2026-10-25T23:51:35Z sender=ee88cac4-07d4-4dad-bc7e-4c3f606ef03f priority=MESSAGE_PRIORITY_HIGH content=You are `explorer_p4_m3_2`. Read `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_2/DISPATCH.md`.
Also read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically R1 requirements).
Investigate and design:
1. Bottom docked playback controls bar with sleek dark aesthetics: Play/Pause toggle with Spacebar shortcut, Stop button.
2. Interactive seekbar with hover timestamp tooltip (`HH:MM:SS`), 10Hz debounced drag-scrubbing via `SeekDebouncer`, and click-to-seek.
3. Volume slider ($0-100\%$) with mute toggle ('M' shortcut).
4. Timecode HUD: `HH:MM:SS / HH:MM:SS`.
5. Playback speed selector (`0.5x`, `0.75x`, `1.0x`, `1.25x`, `1.5x`, `2.0x`).
Write your component design and prototype code to `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_2/handoff.md` and send a message to the orchestrator.
