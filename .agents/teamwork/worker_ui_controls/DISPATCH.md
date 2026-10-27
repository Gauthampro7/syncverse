## 2026-10-27T19:03:53Z
You are worker_ui_controls, a teamwork_worker.
Your working directory is:
/home/gautham/syncverse/.agents/teamwork/worker_ui_controls/

MANDATORY FIRST STEP:
Read the authoritative request file at:
/home/gautham/syncverse/.agents/teamwork/ORIGINAL_REQUEST.md

Also read:
/home/gautham/syncverse/AGENTS.md
/home/gautham/syncverse/.agents/teamwork/explorer_audit_1/handoff.md

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

SCOPE & EXCLUSIVE FILE OWNERSHIP:
You own User-Facing Audio, Video, and Subtitle Configuration Menus/Modals & File Dialogs in `syncverse-ui`:
- In `crates/syncverse-ui/src/gui/dialogs.rs`:
  - Implement `AudioConfigDialog`:
    - Track dropdown / radio list to switch between discovered audio streams.
    - Volume slider [0% - 100%] with quick mute toggle.
    - Audio sync / delay offset slider [-5000 ms to +5000 ms] with Reset (0 ms) button.
  - Implement `VideoConfigDialog`:
    - Aspect ratio selection (Original, 16:9, 4:3, 21:9, 1:1, Fill/Stretch).
    - Playback speed selection (presets + custom slider).
    - Stream metadata display (resolution, aspect ratio, frame rate if available).
  - Implement `SubtitleConfigDialog`:
    - Track listing showing all detected subtitle tracks (both embedded MKV/MP4 tracks and external .srt/.vtt/.ass files).
    - "Off" (Disable) option at the top of the list.
    - "Load External Subtitle..." file picker button (using `rfd::FileDialog` to browse for .srt, .vtt, .ass).
    - Subtitle sync / delay offset slider [-5000 ms to +5000 ms].
- In `crates/syncverse-ui/src/gui/controls.rs`:
  - Add quick-access icons/buttons in the bottom controls bar for:
    - Audio Settings (opens `AudioConfigDialog`)
    - Subtitle Settings (opens `SubtitleConfigDialog`)
    - Video Settings (opens `VideoConfigDialog`)
- Ensure modal dialogs open cleanly, handle user input smoothly, and do not trap keyboard focus when closed.
- Verify `cargo check -p syncverse-ui` and `cargo test -p syncverse-ui` pass with zero warnings under `#![deny(warnings)]`.

Deliver your report to `/home/gautham/syncverse/.agents/teamwork/worker_ui_controls/handoff.md` and send a message when done.
