# Dispatch: Explorer Survey 2 (UI Sidebar, Playback Controls, & Unpause Gate)

## Objective
Investigate `crates/syncplay-ui`, `crates/syncplay-core`, and UI rendering loop:
1. Current desktop window, egui/eframe structure, sidebar rendering (room members, RTT, sync health).
2. How `PlayerUiSnapshot` is defined and passed from `syncplay-core` Telemetry Aggregator to `syncplay-ui`.
3. How playback controls (Play/Pause, spacebar, seek) communicate back to core/sync engine.
4. How to display:
   - Peer loaded media info (filename, formatted HH:MM:SS duration, or `[⏳ No file loaded]`).
   - File match diagnostic badge: `[Match ✔]` (duration ±2s / filename match) vs `[⚠️ Mismatch]`.
   - "Ready to Watch" local toggle checkbox and peer readiness badges: `[✓ Ready]` vs `[⏳ Not Ready]`.
   - On-screen alert/notification when unpause is blocked: `"⏸ Paused: Waiting for [Username(s)]..."`.
   - Host "Force Play" bypass button/toggle.
   - Update notification banner at top of UI with 1-click action and Dismiss button.
   - Menu bar "Check for Updates..." under Help menu.

## Input Information
- `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (read section `## 2026-10-26T11:12:06Z`)
- `/home/gautham/syncplay-alt/crates/syncplay-ui`
- `/home/gautham/syncplay-alt/crates/syncplay-core`
- `/home/gautham/syncplay-alt/crates/syncplay-clock`

## Output Requirements
Produce a comprehensive report in `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_2/handoff.md`.
Include:
- UI architecture and component breakdown
- `PlayerUiSnapshot` changes needed to convey peer media, readiness, update status, and gate alerts
- Input handling and command dispatch changes (unpause gate, force play, ready toggle)
- Update banner & Help menu UI design
- Potential pitfalls and responsiveness considerations


## 2026-10-26T11:15:20Z
You are Explorer 2 for Phase 5 Survey.
Your identity: explorer_survey_2
Your working directory: /home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_2/
Read your dispatch assignment in /home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_2/DISPATCH.md and the authoritative user request in /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (under section `## 2026-10-26T11:12:06Z`).

Investigate the UI architecture, playback controls, and unpause gate in crates/syncplay-ui, crates/syncplay-core, and crates/syncplay-clock.
Analyze:
1. How PlayerUiSnapshot conveys peer media, readiness, and gate alerts to egui.
2. Sidebar rendering: peer loaded file, formatted duration, [Match ✔] vs [⚠️ Mismatch] badges, ready badges.
3. Unpause gate enforcement, waiting notification, and Host "Force Play" bypass.
4. Auto-update notification banner and Help menu entry.

Produce your comprehensive handoff report at /home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_2/handoff.md and message back when complete.
