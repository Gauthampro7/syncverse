# BRIEFING — 2026-10-27T19:22:00Z

## Mission
Implement cycling hotkeys, HUD/OSD telemetry feedback, and command/event extensions in syncverse-ui while strictly maintaining PlayerUiSnapshot size at exactly 72 bytes and zero warnings.

## 🔒 My Identity
- Archetype: teamwork_worker
- Roles: implementer, qa, specialist
- Working directory: /home/gautham/syncverse/.agents/teamwork/worker_hud_hotkeys
- Original parent: 6fec964f-e9e0-4542-a4f0-f5005e6b3d1e
- Milestone: UI Hotkeys, HUD/OSD & Telemetry Feedback

## 🔒 Key Constraints
- In `crates/syncverse-ui/src/command.rs`:
  Add player command variants:
  `SelectAudioTrack(u64)`, `CycleAudioTrack`, `SetAudioDelay(i64)`, `AdjustAudioDelay(i64)`,
  `SelectSubtitleTrack(Option<usize>)`, `CycleSubtitleTrack`, `SetSubtitleDelay(i64)`, `AdjustSubtitleDelay(i64)`,
  `CycleAspectRatio`
- In `crates/syncverse-ui/src/event.rs`:
  Add `UiEvent::HudNotification { text: String, timestamp: Instant }` or equivalent HUD banner event.
  Add events for track selection updates.
  CRITICAL: `PlayerUiSnapshot` MUST REMAIN EXACTLY 72 BYTES! Do not add fields to `PlayerUiSnapshot`.
- In `crates/syncverse-ui/src/osd.rs`:
  Implement `OsdOverlay` (floating translucent pill/banner at top-left or top-center of video viewport, dwell time 1.5s visible, 0.5s smooth alpha fade-out, total 2.0s, high-contrast text with dark semi-transparent background).
- In `crates/syncverse-ui/src/gui/app.rs`:
  Bind keyboard shortcuts:
  `B`: Cycle audio track sequentially
  `V` or `C`: Cycle subtitle track sequentially (including "Subtitles: Off")
  `A`: Cycle aspect ratio
  `J` / `K`: Audio delay adjustment -50ms / +50ms
  `Z` / `X`: Subtitle delay adjustment -50ms / +50ms
  Update `ShortcutsDialog` (`F1`) to list all new hotkeys.
- Verify `cargo test -p syncverse-ui` passes with zero warnings under `#![deny(warnings)]`.

## Current Parent
- Conversation ID: 6fec964f-e9e0-4542-a4f0-f5005e6b3d1e
- Updated: not yet

## Task Summary
- **What to build**: Hotkeys, HUD/OSD telemetry feedback, commands and events in syncverse-ui.
- **Success criteria**: All commands and events added, OsdOverlay implemented and integrated into viewport rendering, hotkeys wired and shortcuts dialog updated, tests passing, PlayerUiSnapshot unchanged at 72 bytes, zero warnings.
- **Interface contracts**: crates/syncverse-ui/src/{command.rs, event.rs, osd.rs, gui/app.rs}
- **Code layout**: crates/syncverse-ui

## Key Decisions Made
- Implemented `OsdOverlay` in `osd.rs` with 1.5s visible dwell and 0.5s smooth linear alpha fade-out (total 2.0s duration), using dark semi-transparent pill container (`#0f172a` at ~85-88% opacity) and high-contrast white text.
- Added dual rendering support (`render` in viewport rect and `show` via egui::Area), with `ctx.request_repaint()` during active animations ensuring smooth 60fps fading.
- Added all 9 requested `PlayerCommand` variants with full docstrings.
- Added all requested `UiEvent` variants (HUD banner and track selection updates).
- Maintained exact 72-byte size of `PlayerUiSnapshot` verified by unit and integration tests.
- Bound hotkeys `B`, `V`/`C`, `A`, `J`/`K`, `Z`/`X` in `SyncplayApp::ui`, triggering transient OSD feedback banners.
- Updated `ShortcutsDialog` (`F1`) with complete documentation for all new keyboard shortcuts.
- Created `crates/syncverse-ui/tests/hud_hotkeys_test.rs` with 8 integration tests covering command variants, event variants, OSD lifecycle timing, hotkey state math, and snapshot size.

## Artifact Index
- DISPATCH.md
- progress.md
- BRIEFING.md
- handoff.md

## Change Tracker
- **Files modified**:
  - `crates/syncverse-ui/src/command.rs`: Added 9 player command variants and unit tests
  - `crates/syncverse-ui/src/event.rs`: Added HUD and track update event variants and unit tests
  - `crates/syncverse-ui/src/osd.rs`: Implemented OsdOverlay with 1.5s dwell + 0.5s fade, rendering, and unit tests
  - `crates/syncverse-ui/src/lib.rs`: Re-exported OsdOverlay
  - `crates/syncverse-ui/src/gui/app.rs`: Bound hotkeys, handled new events, integrated OSD overlay, and added accessors
  - `crates/syncverse-ui/src/gui/dialogs.rs`: Updated ShortcutsDialog with all new hotkeys and cleaned imports
  - `crates/syncverse-ui/tests/hud_hotkeys_test.rs`: Added 8 integration tests
- **Build status**: PASS (117 tests passing in syncverse-ui, 340+ in workspace)
- **Pending issues**: None

## Quality Status
- **Build/test result**: PASS (zero test failures, zero compilation warnings under `#![deny(warnings)]`)
- **Lint status**: PASS (zero Clippy warnings)
- **Tests added/modified**: `crates/syncverse-ui/tests/hud_hotkeys_test.rs` (8 tests) + unit tests in `command.rs`, `event.rs`, `osd.rs`

## Loaded Skills
- None specified
