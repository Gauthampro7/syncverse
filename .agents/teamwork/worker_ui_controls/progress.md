# Progress — worker_ui_controls

Last visited: 2026-10-27T19:21:00Z

## Status
- [x] Initialized DISPATCH.md and BRIEFING.md
- [x] Read ORIGINAL_REQUEST.md, AGENTS.md, and explorer_audit_1/handoff.md
- [x] Inspected existing `syncverse-ui` codebase (`gui/dialogs.rs`, `gui/controls.rs`, `gui/app.rs`)
- [x] Implemented `AudioConfigDialog`:
  - Discovered audio stream tracking and radio selection list (`AudioTrackItem`)
  - Volume slider [0% - 100%] with quick mute toggle
  - Audio sync / delay offset slider [-5000 ms to +5000 ms] with quick step buttons and Reset (0 ms) button
- [x] Implemented `VideoConfigDialog`:
  - Aspect ratio selection (Original, 16:9, 4:3, 21:9, 1:1, Fill/Stretch)
  - Playback speed selection (presets + custom slider [0.25x - 4.0x])
  - Video stream metadata display (resolution, aspect ratio, frame rate, codec/pixel format)
- [x] Implemented `SubtitleConfigDialog`:
  - Track listing showing detected subtitle tracks (`SubtitleTrackItem` with embedded vs external badges)
  - "Off" (Disable Subtitles) option at the top of the list
  - "Load External Subtitle..." file picker button with non-blocking `rfd::FileDialog` thread
  - Subtitle sync / delay offset slider [-5000 ms to +5000 ms] with quick step buttons and Reset (0 ms) button
- [x] Implemented bottom controls bar quick-access buttons in `gui/controls.rs`:
  - Video Settings button ("🎬") opening `VideoConfigDialog`
  - Subtitle Settings button ("CC") opening `SubtitleConfigDialog`
  - Audio Settings button ("🎚") opening `AudioConfigDialog`
- [x] Added `ControlsBarAction::OpenAudioSettings`, `OpenSubtitleSettings`, `OpenVideoSettings`
- [x] Added `show_dialogs(&mut self, ctx: &Context)` helper on `ControlsBarState`
- [x] Wired modal dialogs rendering and action dispatch into `app.rs`
- [x] Re-exported all new dialog items in `gui/mod.rs`
- [x] Added comprehensive unit tests in `gui/dialogs.rs` and `gui/controls.rs`
- [x] Verified `cargo check -p syncverse-ui` (zero warnings)
- [x] Verified `cargo test -p syncverse-ui` (all 109 tests passed)
- [x] Verified `cargo clippy -p syncverse-ui -- -D warnings` (zero warnings)
- [x] Verified `./scripts/audit_dependencies.sh` (clean)
- [x] Verified intent harness (8/8 intents passed)
