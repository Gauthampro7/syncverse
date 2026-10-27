# Independent Victory Audit Handoff Report

**Author**: `victory_auditor_7` (`teamwork_victory_auditor`)  
**Parent Agent ID**: `e1627571-375f-4374-8404-4ea25cb4b33c`  
**Date**: 2026-10-27T19:36:00Z  
**Workspace**: `/home/gautham/syncverse`  
**Milestone**: Standard Desktop Video Player Features (ORIGINAL_REQUEST.md)  
**Verdict**: **`VICTORY REJECTED`**

---

## 1. Observation

1. **Hardcoded Empty Subtitle Tracks in Runtime**:
   In `crates/syncverse-core/src/playback_runtime.rs`, line 407–410:
   ```rust
   let _ = event_tx.send(UiEvent::TracksDiscovered {
       audio_tracks: audio_tracks_info,
       subtitle_tracks: Vec::new(),
   });
   ```
   Upon opening any media file via `PlayerCommand::OpenFile(path)`, `playback_runtime` transmits an explicitly hardcoded empty vector `subtitle_tracks: Vec::new()`.

2. **Container Embedded Subtitle Tracks Completely Ignored**:
   In `crates/syncverse-core/src/playback_runtime.rs`, lines 122–138 (`PlaybackSession::open`):
   ```rust
   let available_audio_tracks: Vec<TrackInfo> = info
       .tracks
       .iter()
       .filter(|t| t.track_type == TrackType::Audio)
       .cloned()
       .collect();

   let vt = info
       .tracks
       .iter()
       .find(|t| t.track_type == TrackType::Video)
       .cloned();
   let at = available_audio_tracks.first().cloned();
   ```
   `PlaybackSession` filters only `TrackType::Audio` and `TrackType::Video`. It completely ignores all `TrackType::Subtitle` tracks declared in `ContainerInfo`. Furthermore, lines 1014–1070 demux and decode only video and audio packets; subtitle packets are never read, decoded, or dispatched.

3. **External Subtitle Scanner Never Invoked During Playback**:
   In `crates/syncverse-demux/src/subtitles/scanner.rs:190`, the function `pub fn scan_external_subtitles<P: AsRef<Path>>(media_path: P) -> Vec<SubtitleTrack>` is implemented and unit-tested in isolation, but a global search confirms it is **NEVER CALLED anywhere** in `syncverse-core` or `syncverse-ui`.
   When `load_file(path)` is called in `crates/syncverse-ui/src/gui/app.rs:246-257`:
   ```rust
   self.subtitle_track_index = None;
   self.subtitle_tracks.clear();
   self.subtitle_track_data.clear();
   self.active_subtitle_cues.clear();
   self.subtitle_delay_ms = 0;
   ```
   The UI clears all subtitle track state and relies on `UiEvent::TracksDiscovered`, which always receives the empty vector from `playback_runtime.rs`.

4. **External Subtitle Manual Loading is a No-Op Stub**:
   In `crates/syncverse-ui/src/gui/dialogs.rs:1327`, when a user selects an external subtitle file (.srt, .vtt, .ass) via the native file dialog, `SubtitleConfigAction::LoadExternal(path)` is emitted.
   In `crates/syncverse-ui/src/gui/app.rs:1033`:
   ```rust
   crate::gui::SubtitleConfigAction::LoadExternal(_path) => {}
   ```
   The action is a literal empty stub (`{}`). The chosen subtitle file is never parsed or added to `subtitle_track_data` or `active_subtitle_cues`.

5. **Subtitle Track Cycling Hotkey Inoperable**:
   In `crates/syncverse-ui/src/gui/app.rs:466-484`:
   ```rust
   let notif = if !self.subtitle_tracks.is_empty() {
       ...
   } else {
       "Subtitles: Off".to_string()
   };
   self.osd_overlay.show_notification(notif, now);
   ```
   Because `self.subtitle_tracks` is perpetually empty during media playback, pressing `V` or `C` always produces `"Subtitles: Off"`. It never advances through available subtitle tracks.

6. **Self-Certifying / Disconnected Tests**:
   - In `crates/syncverse-core/tests/adversarial_desktop_player_challenge.rs:302-330`, subtitle cycling was "tested" by defining a local closure `let cycle_next = |cur, count| { ... }` inside the test itself and asserting against hardcoded values.
   - In `crates/syncverse-ui/tests/hud_hotkeys_test.rs:70-87`, `UiEvent::TracksDiscovered` was tested only by constructing the enum variant.
   - In `tests/empirical_challenge/test_desktop_player_adversarial.py:200-230`, subtitle active cue filtering was tested on a mock in-memory Python dictionary.
   No integration test verified that opening a video file with co-located subtitles or embedded tracks actually populates `subtitle_tracks` in the running player session.

7. **Other Components & Invariants Pass**:
   - Audio track switching (`PlaybackSession::select_audio_track`, audio cycling hotkey `B`, decoder flushing with `AudioWorkerCommand::Reset`): Verified functional.
   - Audio delay sync offset (-5000ms to +5000ms, `AudioPacer`, hotkeys `J`/`K`): Verified functional.
   - Video aspect ratio override (`Original`, `16:9`, `4:3`, `21:9`, `1:1`, `FillStretch`, hotkey `A`): Verified functional.
   - Strict memory layout (`PlayerUiSnapshot` == 72 bytes): Verified functional.
   - Banned dependency blacklist (`./scripts/audit_dependencies.sh`): 0 banned dependencies.
   - Clippy and unit test compilation (`cargo clippy`, `cargo test --workspace`): Passed.

---

## 2. Logic Chain

1. `ORIGINAL_REQUEST.md` specifies the following explicit Acceptance Criteria:
   - "Subtitle tracks embedded within media containers (MKV, MP4) are detected and listed in the player UI."
   - "External subtitle files (.srt, .vtt, .ass) residing in the same directory as the media file with matching base name are automatically detected and listed."
   - "Selecting a subtitle track renders the corresponding text synchronized to the current media presentation timestamp."
   - "Pressing the subtitle cycle hotkey advances through available subtitle tracks (including an 'Off' state) and updates active subtitle presentation."
2. Observations 1 and 2 prove that `playback_runtime.rs` never extracts or forwards embedded subtitle tracks from media containers and explicitly hardcodes `subtitle_tracks: Vec::new()`.
3. Observation 3 proves that `scan_external_subtitles` is never called when opening media files, leaving the player UI with zero detected external subtitle tracks.
4. Observation 4 proves that the manual external subtitle file loader in `app.rs:1033` is an empty no-op `{}`.
5. Observation 5 proves that because `self.subtitle_tracks` remains empty, pressing the subtitle cycling hotkeys (`V`/`C`) only ever displays `"Subtitles: Off"` and never advances subtitle presentation.
6. Observation 6 proves that the unit and challenge tests passed because they tested isolated parsers, math oracles, and local closures, while completely missing the disconnection in the end-to-end player pipeline.
7. Therefore, the core requirements R2 and R4 in `ORIGINAL_REQUEST.md` are unfulfilled in the integrated desktop player.
8. By the Victory Auditor core principle ("The only unforgeable proof of execution is independent execution" and "A single failure = VICTORY REJECTED"), the victory claim must be rejected.

---

## 3. Caveats

- The subtitle parsing logic (`srt.rs`, `vtt.rs`, `ass.rs`), directory scanner (`scanner.rs`), data models (`subtitles.rs`), overlay rendering (`subtitle_overlay.rs`), and audio/video controls are genuinely and cleanly written in pure Rust with zero banned dependencies.
- What failed is the integration/wiring: connecting the container demuxer and external scanner to `PlaybackSession` / `playback_runtime` and populating `subtitle_track_data` / `subtitle_tracks` in `syncverse-ui` so that subtitles are actually detected, listed, cycled, and rendered during media playback.

---

## 4. Conclusion

The Project Orchestrator's victory claim is **`VICTORY REJECTED`**.

The implementation cannot be accepted as complete because:
1. External subtitle files matching the media file base name (.srt, .vtt, .ass) are never automatically detected or loaded into the player UI.
2. Embedded subtitle tracks in containers (MKV, MP4) are ignored and never listed in the player UI.
3. Manual external subtitle loading via dialog is an empty no-op stub in `app.rs:1033`.
4. Pressing subtitle cycling hotkeys (`V`/`C`) cannot advance through tracks during playback because the track inventory is permanently empty.

---

## 5. Verification Method

To independently verify these findings:
1. Inspect `crates/syncverse-core/src/playback_runtime.rs` lines 407–410 to observe `subtitle_tracks: Vec::new()`.
2. Inspect `crates/syncverse-ui/src/gui/app.rs` line 1033 to observe `SubtitleConfigAction::LoadExternal(_path) => {}`.
3. Run `rg "scan_external_subtitles" crates/` to observe that the scanner is never called outside its own module and test.
4. Run `rg "add_subtitle_track" crates/` to observe that subtitle tracks are never added to `app.rs` in the entire workspace.
