# Progress: worker_hud_hotkeys

Last visited: 2026-10-27T19:22:30Z

## Status
- [x] Initialized DISPATCH.md, BRIEFING.md, and progress.md
- [x] Read ORIGINAL_REQUEST.md, AGENTS.md, and explorer_audit_1/handoff.md
- [x] Inspect existing `crates/syncverse-ui/src/{command.rs, event.rs, osd.rs, gui/app.rs}` and related modules
- [x] Implement command variants in `command.rs`
  - `SelectAudioTrack(u64)`
  - `CycleAudioTrack`
  - `SetAudioDelay(i64)`
  - `AdjustAudioDelay(i64)`
  - `SelectSubtitleTrack(Option<usize>)`
  - `CycleSubtitleTrack`
  - `SetSubtitleDelay(i64)`
  - `AdjustSubtitleDelay(i64)`
  - `CycleAspectRatio`
- [x] Implement event variants in `event.rs`
  - `HudNotification { text: String, timestamp: Instant }`
  - `AudioTrackSelected { track_id: u64, name: String }`
  - `SubtitleTrackSelected { track_index: Option<usize>, name: String }`
  - `AudioDelayChanged(i64)`
  - `SubtitleDelayChanged(i64)`
  - `TracksDiscovered { audio_tracks: Vec<String>, subtitle_tracks: Vec<String> }`
- [x] Verified `PlayerUiSnapshot` size = exactly 72 bytes
- [x] Implement `OsdOverlay` in `osd.rs`
  - Dwell duration: 1.5s
  - Smooth linear alpha fade: 0.5s (total 2.0s)
  - High-contrast text on dark semi-transparent pill background
  - Dual rendering via painter inside viewport and Area popup
- [x] Wire up keyboard shortcuts and OSD notifications in `gui/app.rs`
  - `B`: Cycle audio tracks sequentially + HUD notification
  - `V` / `C`: Cycle subtitle tracks sequentially (including Off) + HUD notification
  - `A`: Cycle aspect ratio presets + HUD notification
  - `J` / `K`: Audio delay adjustment -50ms / +50ms + HUD notification
  - `Z` / `X`: Subtitle delay adjustment -50ms / +50ms + HUD notification
  - Overlay rendering and 60fps repaint request during active animations
- [x] Update `ShortcutsDialog` (`F1`) in `crates/syncverse-ui/src/gui/dialogs.rs` with all new hotkeys
- [x] Add comprehensive integration and unit test suite in `tests/hud_hotkeys_test.rs`
- [x] Run `cargo check --workspace --all-targets` (clean, 0 warnings)
- [x] Run `cargo clippy --workspace --all-targets -- -D warnings` (clean, 0 warnings)
- [x] Run `cargo test --workspace` (all 340+ tests pass)
- [x] Run `./scripts/audit_dependencies.sh` (clean, 0 banned dependencies)
- [x] Run `python3 tests/empirical_challenge/test_ui_renderer.py` (all tests pass)
- [x] Run `python3 scripts/intent_harness.py` (8/8 intents pass, 100% compliance)
- [x] Write handoff report `handoff.md`
