# DISPATCH — explorer_p4_m1_1

## Mission
You are `explorer_p4_m1_1`, an exploration and specification specialist for Milestone 1.
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_1`.
You must read:
1. `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically `## 2026-10-25T20:12:42Z`)
2. `/home/gautham/syncplay-alt/.agents/teamwork/orchestrator_4/PROJECT.md`
3. Current `/home/gautham/syncplay-alt/AGENTS.md`
4. Current `/home/gautham/syncplay-alt/openspec/project.md` and `/home/gautham/syncplay-alt/openspec/specs/ui-renderer/spec.md`

## Scope & Objective
Formulate the precise, line-by-line specification realignment strategy for `AGENTS.md`, `openspec/project.md`, and capability specs:
1. Identify all obsolete prohibitions in `AGENTS.md` §2.1 that must be updated to permit `ffmpeg-next`, `ffmpeg-sys-next`, `symphonia`, `eframe`, `egui`, `wgpu`, `winit`, `rfd`, `cpal`, `rodio`.
2. Explicitly define what prohibitions MUST be preserved: no monolithic player script wrappers (`mpv`, `vlc`), no external database engines (`sqlite`, `diesel`, etc.).
3. Formulate updates for `openspec/project.md` and `openspec/specs/ui-renderer/spec.md` defining RFC-2119 requirements for the desktop graphical UI (Wayland/X11 windowing, GPU video viewport, aspect ratio preservation, 10Hz debounced seekbar, watch-party sidebar, native file dialogs).

## Output Requirements
Document the full analysis, exact text diff proposals, and verification commands in `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_1/handoff.md`.
Send a completion message to the orchestrator when done.

## 2026-10-25T20:26:59Z
[Message] timestamp=2026-10-25T20:26:59Z sender=ee88cac4-07d4-4dad-bc7e-4c3f606ef03f priority=MESSAGE_PRIORITY_HIGH content=You are explorer_p4_m1_1.
Your working directory is: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_1
Read:
1. /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically the latest request under ## 2026-10-25T20:12:42Z)
2. /home/gautham/syncplay-alt/.agents/teamwork/orchestrator_4/PROJECT.md
3. /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_1/DISPATCH.md
4. /home/gautham/syncplay-alt/AGENTS.md
5. /home/gautham/syncplay-alt/openspec/project.md and /home/gautham/syncplay-alt/openspec/specs/ui-renderer/spec.md

Task:
Formulate the precise, line-by-line specification realignment strategy for AGENTS.md, openspec/project.md, and capability specs:
1. Identify all obsolete prohibitions in AGENTS.md §2.1 that must be updated to permit ffmpeg-next, ffmpeg-sys-next, symphonia, eframe, egui, wgpu, winit, rfd, cpal, rodio.
2. Explicitly define what prohibitions MUST be preserved: no monolithic player script wrappers (mpv, vlc), no external database engines (sqlite, diesel, etc.).
3. Formulate updates for openspec/project.md and openspec/specs/ui-renderer/spec.md defining RFC-2119 requirements for the desktop graphical UI (Wayland/X11 windowing, GPU video viewport, aspect ratio preservation, 10Hz debounced seekbar, watch-party sidebar, native file dialogs).

Maintain progress.md in your working directory.
When done, write your structured report to /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_1/handoff.md and call send_message to report completion to the orchestrator.
