# DISPATCH — explorer_p4_survey_3

## Mission
You are `explorer_p4_survey_3`, a read-only exploration and UI architecture specialist.
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_3`.
You must read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (especially section `## 2026-10-25T20:12:42Z`).

## Scope & Objective
Investigate Requirement R1 (Flawless Desktop Graphical Player UI) and R3 (Documentation & Specification Realignment):
1. Review graphical desktop UI requirements:
   - Window management: Native desktop window (Wayland/X11 on Fedora, Windows), title bar, min/max, resize, clean close, fullscreen toggle (F11/double-click).
   - Video viewport: GPU-accelerated video rendering surface preserving aspect ratio, letterboxing/pillarboxing, vsync frame pacing.
   - Playback controls: Bottom control bar with Play/Pause, Stop, interactive seekbar / timeline slider with mouse hover timestamp preview & smooth drag-scrubbing, volume slider with mute toggle, timecode display (HH:MM:SS / HH:MM:SS), playback speed selector.
   - In-player watch-party sidebar / overlay: Toggleable sidebar displaying room members, connection RTT, sync health indicator, real-time smart chat with text input for chat and slash-commands (/seek, /clip, /poll).
   - File management: File open dialog, drag-and-drop file loading directly into the window, recent media history.
2. Evaluate Rust GUI frameworks: `egui` / `eframe` (with custom paint callback or texture updating for video), `iced`, `slint`, or `winit` + `wgpu`. Compare developer ergonomics, compile time, Wayland/X11 compatibility on Fedora, and performance. Check `rfd` (rusty file dialog) for native file pickers.
3. Review `syncplay-ui` crate: how `PlayerUiSnapshot`, `ScrubberDebouncer`, and OSD state are currently modeled, and how they can be adapted for a native GUI renderer.
4. Review R3 requirements: what specific updates are needed in `AGENTS.md`, `openspec/project.md`, `openspec/specs/`, `scripts/audit_dependencies.sh`, and `scripts/intent_harness.py`.

## Output Requirements
Document all findings, GUI framework evaluation, UI component design, and documentation update plan in `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_3/handoff.md`.
Send a message to the orchestrator when complete.


## 2026-10-25T20:16:47Z
You are explorer_p4_survey_3.
Your working directory is: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_3
Read /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically the latest request under ## 2026-10-25T20:12:42Z) and your assignment in /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_3/DISPATCH.md.

Task:
Investigate Requirement R1 (Flawless Desktop Graphical Player UI) and R3 (Documentation & Specification Realignment):
1. Review graphical desktop UI requirements:
   - Window management: Native desktop window (Wayland/X11 on Fedora Linux, Windows), title bar, min/max, resize, clean close, fullscreen toggle (F11/double-click).
   - Video viewport: GPU-accelerated video rendering surface preserving aspect ratio, letterboxing/pillarboxing, vsync frame pacing.
   - Playback controls: Bottom control bar with Play/Pause, Stop, interactive seekbar / timeline slider with mouse hover timestamp preview & smooth drag-scrubbing, volume slider with mute toggle, timecode display (HH:MM:SS / HH:MM:SS), playback speed selector.
   - In-player watch-party sidebar / overlay: Toggleable sidebar displaying room members, connection RTT, sync health indicator, real-time smart chat with text input for chat and slash-commands (/seek, /clip, /poll).
   - File management: File open dialog, drag-and-drop file loading directly into the window, recent media history.
2. Evaluate Rust GUI frameworks: egui / eframe (with wgpu custom rendering or video texture update), iced, slint, or winit + wgpu. Compare developer ergonomics, compile time, Wayland/X11 compatibility on Fedora, and performance. Check rfd (rusty file dialog) for native file pickers.
3. Review syncplay-ui crate: how PlayerUiSnapshot, ScrubberDebouncer, and OSD state are currently modeled, and how they can be adapted for a native GUI renderer.
4. Review R3 requirements: what specific updates are needed in AGENTS.md, openspec/project.md, openspec/specs/, scripts/audit_dependencies.sh, and scripts/intent_harness.py.

Maintain progress.md in your working directory.
When done, write your structured report to /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_3/handoff.md and call send_message to report completion to the orchestrator.
