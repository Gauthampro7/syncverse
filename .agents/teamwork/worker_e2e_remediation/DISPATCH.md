## 2026-10-27T19:49:23Z

SCOPE & TASK:
You will implement the complete End-to-End Subtitle Pipeline remediation blueprint designed in `/home/gautham/syncverse/.agents/teamwork/explorer_remediation/handoff.md`:

1. In `crates/syncverse-ui/Cargo.toml`:
   - Add dependencies on `syncverse-common` and `syncverse-demux` so that `syncverse-ui` has direct access to subtitle data models and parsers.

2. In `crates/syncverse-ui/src/event.rs`:
   - Add variants to `UiEvent`:
     ```rust
     SubtitleDataDiscovered(Vec<crate::gui::subtitle_overlay::SubtitleTrack>),
     SubtitleCueDemuxed(crate::gui::subtitle_overlay::SubtitleCue),
     ```
   - Verify `PlayerUiSnapshot` remains strictly 72 bytes.

3. In `crates/syncverse-core/src/playback_runtime.rs`:
   - In `PlaybackSession`:
     - Add `available_subtitle_tracks: Vec<syncverse_common::subtitles::SubtitleTrack>`, `selected_subtitle_track_index: Option<usize>`, `subtitle_track_id: Option<u64>`.
     - In `PlaybackSession::open`: discover embedded subtitle tracks (`t.track_type == TrackType::Subtitle`) AND scan co-located external subtitle files with `syncverse_demux::subtitles::scan_external_subtitles(path)`.
     - Add `select_subtitle_track(&mut self, track_idx: Option<usize>) -> (Option<usize>, String)` and `cycle_subtitle_track(&mut self) -> (Option<usize>, String)`.
   - In `run_playback_worker`:
     - On `PlayerCommand::OpenFile`: broadcast `UiEvent::TracksDiscovered` with real subtitle track labels (ELIMINATE `subtitle_tracks: Vec::new()`), and broadcast `UiEvent::SubtitleDataDiscovered`.
     - Handle `PlayerCommand::SelectSubtitleTrack(idx)` and `PlayerCommand::CycleSubtitleTrack`, broadcasting `UiEvent::SubtitleTrackSelected` and `UiEvent::HudNotification`.
     - In the packet demuxing loop, route demuxed subtitle packets (`Some(packet.track_id) == sess.subtitle_track_id`) to `UiEvent::SubtitleCueDemuxed`.

4. In `crates/syncverse-ui/src/gui/app.rs`:
   - In `load_file(&mut self, path: PathBuf)`: call `syncverse_demux::subtitles::scan_external_subtitles(&path)` to populate `self.subtitle_tracks`, `self.subtitle_track_data`, and `self.controls_state.subtitle_dialog.set_tracks(...)` synchronously.
   - In `ui` event draining loop: handle `UiEvent::SubtitleDataDiscovered`, `UiEvent::SubtitleTrackSelected`, and `UiEvent::SubtitleCueDemuxed`.
   - In `SubtitleConfigAction::LoadExternal(path)` (replacing line 1033): implement genuine external file loading by reading the file, parsing with `parse_srt`, `parse_vtt`, or `parse_ass` based on extension, constructing a `SubtitleTrack`, appending to `self.subtitle_track_data` and `self.subtitle_tracks`, selecting it as active, setting `self.active_subtitle_cues`, and triggering an OSD notification.
   - In `V` / `C` keyboard shortcut handling: update `self.active_subtitle_cues` to the active track's cues when cycling to a track, and clear `self.active_subtitle_cues` when cycling to "Off".
   - Add public helper getters on `SyncplayApp`: `pub fn subtitle_tracks(&self) -> &[String]`, `pub fn subtitle_track_index(&self) -> Option<usize>`, and `pub fn select_subtitle_track(&mut self, track_idx: Option<usize>)`.

5. In `crates/syncverse-core/tests/adversarial_desktop_player_challenge.rs`:
   - Replace the mock closure test with the authentic integration test `test_end_to_end_media_playback_with_subtitles` (from Component 5 of `explorer_remediation/handoff.md`), verifying that opening media with external `.srt` and `.vtt` populates tracks, cycling advances active tracks, and presentation timestamps yield active cues.

6. Verification:
   - Run `cargo check --workspace --all-targets` (0 warnings).
   - Run `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings).
   - Run `cargo test --workspace` (all tests pass).
   - Run `./scripts/audit_dependencies.sh` (0 violations).
   - Run `python3 scripts/intent_harness.py` (8/8 intents pass).

Write your handoff report to `/home/gautham/syncverse/.agents/teamwork/worker_e2e_remediation/handoff.md` and send a message when complete.
