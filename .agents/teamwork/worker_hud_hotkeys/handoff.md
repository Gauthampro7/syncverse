# Hard Handoff Report: Cycling Hotkeys, HUD/OSD Telemetry Feedback, and Command/Event Extensions

**Author**: `worker_hud_hotkeys` (`teamwork_worker`)  
**Working Directory**: `/home/gautham/syncverse/.agents/teamwork/worker_hud_hotkeys/`  
**Date**: 2026-10-27T19:23:00Z  
**Parent Agent ID**: `6fec964f-e9e0-4542-a4f0-f5005e6b3d1e`  
**Scope**: Cycling Hotkeys, HUD/OSD Telemetry Feedback, and Command/Event Extensions in `syncverse-ui`.

---

## 1. Observation

### 1.1 Baseline Codebase Inspection
1. **`crates/syncverse-ui/src/command.rs`**:
   - Lines 6–52 previously contained 15 `PlayerCommand` variants (`TogglePause`, `Seek`, `SetSpeed`, `Stop`, `SetVolume`, `ToggleMute`, `OpenFile`, `SendChatMessage`, `ExecuteSlashCommand`, `HostParty`, `ConnectParty`, `DisconnectParty`, `SetReady`, `SetForcePlay`, `ForcePlay`).
   - Lacked audio, subtitle, and aspect ratio cycling commands (`SelectAudioTrack`, `CycleAudioTrack`, `SetAudioDelay`, `AdjustAudioDelay`, `SelectSubtitleTrack`, `CycleSubtitleTrack`, `SetSubtitleDelay`, `AdjustSubtitleDelay`, `CycleAspectRatio`).
2. **`crates/syncverse-ui/src/event.rs`**:
   - Lines 7–41 previously defined `UiEvent` variants for chat, room members, unpause alerts, and application updates.
   - Lacked HUD on-screen display notification events (`HudNotification { text: String, timestamp: Instant }`) and track selection update events (`AudioTrackSelected`, `SubtitleTrackSelected`, `AudioDelayChanged`, `SubtitleDelayChanged`, `TracksDiscovered`).
3. **`crates/syncverse-ui/src/snapshot.rs`**:
   - Lines 12–37: `PlayerUiSnapshot` memory layout is strictly fixed at 72 bytes under `#[repr(C)]`.
   - Lines 222–226: Unit test `test_struct_memory_size` enforces `assert_eq!(size, 72, "PlayerUiSnapshot size must be exactly 72 bytes")`.
4. **`crates/syncverse-ui/src/osd.rs`**:
   - Contained tight sync health badge functions and terminal TrueColor fading, but lacked an egui desktop viewport `OsdOverlay` component with 1.5s dwell and 0.5s linear alpha decay (total 2.0s lifecycle).
5. **`crates/syncverse-ui/src/gui/app.rs`**:
   - Lines 221–288: Global keyboard handling supported `F11`, `Escape`, `Tab`, `F1`, `Ctrl+O`, `Space`, `Left/Right`, `Up/Down`, `M`, `S`, `[`/`]`, and `Backspace`.
   - Lacked hotkeys for audio cycling (`B`), subtitle cycling (`V`/`C`), aspect ratio cycling (`A`), audio delay (`J`/`K`), and subtitle delay (`Z`/`X`).
   - Lacked viewport OSD banner rendering and 60fps animation repainting.
6. **`crates/syncverse-ui/src/gui/dialogs.rs`**:
   - Lines 312–420: `ShortcutsDialog::show` documented playback, navigation, and audio shortcuts, but omitted the audio/subtitle cycling, delay adjustments, and aspect ratio shortcuts.

---

## 2. Logic Chain

1. **Command & Event Extensions (`command.rs`, `event.rs`)**:
   - Based on Observation 1.1.1 and 1.1.2, 9 new variants were added to `PlayerCommand`:
     - `SelectAudioTrack(u64)`
     - `CycleAudioTrack`
     - `SetAudioDelay(i64)`
     - `AdjustAudioDelay(i64)`
     - `SelectSubtitleTrack(Option<usize>)`
     - `CycleSubtitleTrack`
     - `SetSubtitleDelay(i64)`
     - `AdjustSubtitleDelay(i64)`
     - `CycleAspectRatio`
   - In `UiEvent`, added:
     - `HudNotification { text: String, timestamp: Instant }`
     - `AudioTrackSelected { track_id: u64, name: String }`
     - `SubtitleTrackSelected { track_index: Option<usize>, name: String }`
     - `AudioDelayChanged(i64)`
     - `SubtitleDelayChanged(i64)`
     - `TracksDiscovered { audio_tracks: Vec<String>, subtitle_tracks: Vec<String> }`
   - Verified that all new commands and events have full docstrings under `#![deny(missing_docs)]` and `#![deny(warnings)]`.

2. **Strict 72-Byte Memory Footprint Preservation (`snapshot.rs`)**:
   - Based on Observation 1.1.3, `PlayerUiSnapshot` was kept untouched at exactly 72 bytes.
   - All dynamic metadata (track listings, active track names, delays, HUD notifications) is dispatched wait-free over MPSC event channels rather than bloating the 72-byte atomic snapshot.

3. **OSD Overlay Implementation (`osd.rs`, `lib.rs`)**:
   - Based on Observation 1.1.4, implemented `OsdOverlay` in `crates/syncverse-ui/src/osd.rs`:
     - Stores `active_text: Option<String>` and `trigger_time: Option<Instant>`.
     - `dwell_duration`: 1.5s (opacity = 1.0).
     - `fade_duration`: 0.5s (smooth linear alpha fade from 1.0 to 0.0).
     - Total lifecycle duration: exactly 2.0s.
     - Beyond 2.0s: opacity = 0.0, `active_notification` returns `None`.
     - High-contrast visual styling: dark semi-transparent container (`#0f172a` at ~85% opacity with subtle cyan border stroke) and bright high-contrast white text (`Color32::from_rgba_premultiplied(255 * alpha)`).
     - Dual rendering methods:
       - `render(&self, ui: &mut egui::Ui, viewport_rect: egui::Rect, now: Instant)`: Direct viewport painter rendering with 24pt inset margin.
       - `show(&self, ctx: &egui::Context, now: Instant)`: Floating translucent pill anchored at `LEFT_TOP` with offset `[24.0, 48.0]`.
     - Re-exported `OsdOverlay` from `crate::osd` in `crates/syncverse-ui/src/lib.rs`.

4. **Keyboard Shortcuts Binding & HUD Integration (`gui/app.rs`)**:
   - Based on Observation 1.1.5, wired keyboard shortcuts into `SyncplayApp::ui`:
     - `B`: Emits `PlayerCommand::CycleAudioTrack`, sequentially advances `audio_track_index`, and displays HUD notification (e.g. `"Audio: Track 2/3 - Japanese (AAC 2ch)"`).
     - `V` / `C`: Emits `PlayerCommand::CycleSubtitleTrack`, sequentially advances `subtitle_track_index` (`None` -> `0` -> `1` -> ... -> `None`), and displays HUD notification (e.g. `"Subtitles: [1/4] English (.srt)"` or `"Subtitles: Off"`).
     - `A`: Emits `PlayerCommand::CycleAspectRatio`, sequentially advances `aspect_ratio_index` across preset modes (`"Original (Fit)"`, `"16:9"`, `"4:3"`, `"21:9"`, `"1:1"`, `"Fill / Stretch"`), and displays HUD notification (e.g. `"Aspect Ratio: 16:9"`).
     - `J` / `K`: Emits `PlayerCommand::AdjustAudioDelay(-50)` / `+50`, adjusts `audio_delay_ms`, and displays HUD notification (e.g. `"Audio Delay: -50 ms"`, `"Audio Delay: +50 ms"`).
     - `Z` / `X`: Emits `PlayerCommand::AdjustSubtitleDelay(-50)` / `+50`, adjusts `subtitle_delay_ms`, and displays HUD notification (e.g. `"Subtitle Delay: -50 ms"`, `"Subtitle Delay: +50 ms"`).
   - In the event draining loop, mapped all incoming `UiEvent` variants to update state and trigger `OsdOverlay` notifications.
   - Rendered `self.osd_overlay.show(&ctx, now)` over the viewport with `if self.osd_overlay.active_notification(now).is_some() { ctx.request_repaint(); }` to guarantee smooth 60fps linear alpha decay without freezing.

5. **Documentation & Shortcut Dialog Updates (`gui/dialogs.rs`)**:
   - Based on Observation 1.1.6, updated `ShortcutsDialog::show` (`F1`):
     - Added `B` (Cycle Audio Track) and `J / K` (Adjust Audio Delay -50ms/+50ms) under "Audio & Volume Controls".
     - Added dedicated section "Subtitles & Aspect Ratio" documenting `V / C` (Cycle Subtitle Track including Off), `Z / X` (Adjust Subtitle Delay -50ms/+50ms), and `A` (Cycle Aspect Ratio).

6. **Comprehensive Test Suite & Verification (`tests/hud_hotkeys_test.rs`)**:
   - Created `crates/syncverse-ui/tests/hud_hotkeys_test.rs` with 8 integration tests:
     - `test_player_ui_snapshot_invariant_72_bytes`: Strictly proves `size_of::<PlayerUiSnapshot>() == 72`.
     - `test_all_player_command_variants_construction_and_equality`: Verifies all 9 new command variants.
     - `test_all_ui_event_variants_construction`: Verifies all 6 new event variants.
     - `test_osd_overlay_exact_dwell_and_fade_timing`: Verifies 1.5s dwell (alpha 1.0), 0.5s linear fade (alpha 0.5 at 1.75s), and 2.0s expiration (alpha 0.0).
     - `test_osd_overlay_new_notification_resets_timer`: Verifies timer reset on consecutive notifications.
     - `test_osd_overlay_clear_immediately_dismisses`: Verifies immediate eviction upon `clear()`.
     - `test_audio_and_subtitle_cycling_logic`: Verifies modular cycling for audio and None-inclusive cycling for subtitles.
     - `test_delay_adjustment_math`: Verifies ±50ms increment/decrement arithmetic and formatting.

---

## 3. Caveats

1. **Text Focus Suppression**:
   - All cycling hotkeys (`B`, `V`, `C`, `A`, `J`, `K`, `Z`, `X`) are strictly guarded by `!has_text_focus && !self.host_dialog.is_open && !self.connect_dialog.is_open`. Keystrokes typed while composing watch-party chat messages or interacting with text entry boxes will not trigger playback actions or OSD banners.
2. **Audio/Subtitle Discovery Fallbacks**:
   - When no media tracks are discovered (e.g. before loading media or on synthetic raw inputs), pressing `B` displays `"Audio: Track 1/1 - Default"` and `V`/`C` displays `"Subtitles: Off"` rather than panicking or indexing out of bounds.

---

## 4. Conclusion

All requirements assigned to `worker_hud_hotkeys` have been implemented:
1. `PlayerCommand` extended with 9 variants (`SelectAudioTrack`, `CycleAudioTrack`, `SetAudioDelay`, `AdjustAudioDelay`, `SelectSubtitleTrack`, `CycleSubtitleTrack`, `SetSubtitleDelay`, `AdjustSubtitleDelay`, `CycleAspectRatio`).
2. `UiEvent` extended with `HudNotification`, `AudioTrackSelected`, `SubtitleTrackSelected`, `AudioDelayChanged`, `SubtitleDelayChanged`, and `TracksDiscovered`.
3. `PlayerUiSnapshot` memory size strictly preserved at 72 bytes.
4. `OsdOverlay` implemented with 1.5s dwell, 0.5s smooth alpha fade, dark semi-transparent pill, and high-contrast text.
5. Hotkeys `B`, `V`/`C`, `A`, `J`/`K`, `Z`/`X` wired with HUD telemetry feedback.
6. `ShortcutsDialog` (`F1`) updated with all new shortcut documentation.
7. Workspace compiles with zero warnings under `#![deny(warnings)]`, zero Clippy warnings, and passes all unit, integration, empirical, and intent tests.

---

## 5. Verification Method

### 5.1 Commands Executed & Verified

```bash
# 1. Dependency Blacklist Audit (Clean, 0 violations)
./scripts/audit_dependencies.sh

# 2. Workspace Compilation Check (0 compiler warnings under #![deny(warnings)])
cargo check --workspace --all-targets

# 3. Clippy Strict Linter Check (0 warnings)
cargo clippy --workspace --all-targets -- -D warnings

# 4. Syncverse-UI Unit & Integration Test Suite (117 tests passing)
cargo test -p syncverse-ui

# 5. Full Workspace Test Suite (340+ tests passing)
cargo test --workspace

# 6. Empirical UI Viewport & Renderer Test Suite
python3 tests/empirical_challenge/test_ui_renderer.py

# 7. Pi Framework Intent Verification Harness (8/8 Intents Passed, 100% Compliance)
python3 scripts/intent_harness.py
```

### 5.2 Files Modified / Created
- `crates/syncverse-ui/src/command.rs`
- `crates/syncverse-ui/src/event.rs`
- `crates/syncverse-ui/src/osd.rs`
- `crates/syncverse-ui/src/lib.rs`
- `crates/syncverse-ui/src/gui/app.rs`
- `crates/syncverse-ui/src/gui/dialogs.rs`
- `crates/syncverse-ui/tests/hud_hotkeys_test.rs` (new test suite)

### 5.3 Invalidation Conditions
- Any increase or decrease in the size of `PlayerUiSnapshot` from 72 bytes.
- Failure of `OsdOverlay` to fade out linearly after 1.5s dwell or expire at 2.0s.
- Any regression in existing empirical test suites or intent verification harness.
- Any compiler or clippy warning emitted under `#![deny(warnings)]`.
