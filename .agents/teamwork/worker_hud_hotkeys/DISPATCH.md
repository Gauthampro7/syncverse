## 2026-10-27T19:04:01Z
You are worker_hud_hotkeys, a teamwork_worker.
Your working directory is:
/home/gautham/syncverse/.agents/teamwork/worker_hud_hotkeys/

MANDATORY FIRST STEP:
Read the authoritative request file at:
/home/gautham/syncverse/.agents/teamwork/ORIGINAL_REQUEST.md

Also read:
/home/gautham/syncverse/AGENTS.md
/home/gautham/syncverse/.agents/teamwork/explorer_audit_1/handoff.md

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

SCOPE & EXCLUSIVE FILE OWNERSHIP:
You own Cycling Hotkeys, HUD/OSD Telemetry Feedback, and Command/Event extensions:
- In `crates/syncverse-ui/src/command.rs`:
  - Add player command variants:
    - `SelectAudioTrack(u64)`
    - `CycleAudioTrack`
    - `SetAudioDelay(i64)`
    - `AdjustAudioDelay(i64)`
    - `SelectSubtitleTrack(Option<usize>)`
    - `CycleSubtitleTrack`
    - `SetSubtitleDelay(i64)`
    - `AdjustSubtitleDelay(i64)`
    - `CycleAspectRatio`
- In `crates/syncverse-ui/src/event.rs`:
  - Add `UiEvent::HudNotification { text: String, timestamp: Instant }` or equivalent HUD banner event.
  - Add events for track selection updates.
  - CRITICAL: Remember `PlayerUiSnapshot` MUST REMAIN EXACTLY 72 BYTES! Do not add fields to `PlayerUiSnapshot`.
- In `crates/syncverse-ui/src/osd.rs`:
  - Implement `OsdOverlay`:
    - Stores active notification text and trigger timestamp.
    - Floating translucent pill/banner positioned at the top-left or top-center of the video viewport.
    - Dwell time: 1.5s visible, 0.5s smooth alpha fade-out (total 2.0s).
    - High-contrast text with dark semi-transparent background.
- In `crates/syncverse-ui/src/gui/app.rs`:
  - Bind keyboard shortcuts:
    - `B`: Cycle audio track sequentially (emits command + displays HUD notification e.g. "Audio: Track 2/3 - Japanese (AAC 2ch)")
    - `V` or `C`: Cycle subtitle track sequentially (including "Subtitles: Off" state, displays HUD notification e.g. "Subtitles: [1/4] English (.srt)")
    - `A`: Cycle aspect ratio (displays HUD notification e.g. "Aspect Ratio: 16:9")
    - `J` / `K`: Audio delay adjustment -50ms / +50ms (displays HUD notification e.g. "Audio Delay: -100 ms")
    - `Z` / `X`: Subtitle delay adjustment -50ms / +50ms (displays HUD notification e.g. "Subtitle Delay: +50 ms")
  - Update `ShortcutsDialog` (`F1`) to list all new hotkeys.
- Verify `cargo test -p syncverse-ui` passes with zero warnings under `#![deny(warnings)]`.

Deliver your report to `/home/gautham/syncverse/.agents/teamwork/worker_hud_hotkeys/handoff.md` and send a message when done.
