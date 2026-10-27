## 2026-10-27T19:37:58Z

You are explorer_remediation, a teamwork_explorer.
Your working directory is:
/home/gautham/syncverse/.agents/teamwork/explorer_remediation/

MANDATORY FIRST STEP:
Read the authoritative request file at:
/home/gautham/syncverse/.agents/teamwork/ORIGINAL_REQUEST.md

CRITICAL: FORENSIC AUDIT FAILURE REMEDIATION:
The previous milestone claim was REJECTED by the independent Victory Auditor due to integrity violations and unconnected/facade implementations.
You MUST read the FULL auditor report at:
/home/gautham/syncverse/.agents/teamwork/victory_auditor_7/handoff.md

Here is the exact auditor finding and evidence:
1. In `crates/syncverse-core/src/playback_runtime.rs:407-410`, when a media file is opened, the runtime hardcodes `subtitle_tracks: Vec::new()`.
2. Embedded container subtitle tracks in MKV and MP4 are completely ignored by `PlaybackSession::open` (lines 122–138) and are never extracted or forwarded to the UI.
3. `scan_external_subtitles` in `crates/syncverse-demux/src/subtitles/scanner.rs` is never invoked during media loading or active playback.
4. In `crates/syncverse-ui/src/gui/app.rs:1033`, manual external subtitle loading via the file picker is an empty stub: `SubtitleConfigAction::LoadExternal(_path) => {}`.
5. In `crates/syncverse-ui/src/gui/app.rs:466-484`, pressing the subtitle cycling hotkeys (V/C) always produces "Subtitles: Off" because the UI track inventory is permanently empty.
6. Unit and empirical challenge tests evaluated local closures and mock dictionaries rather than testing end-to-end media playback with subtitles.

TASK:
Perform a deep technical investigation of how to genuinely wire and connect the entire subtitle pipeline from end to end:
1. In `crates/syncverse-core/src/playback_runtime.rs`:
   - How should `PlaybackSession::open` discover both embedded container subtitle tracks (`info.tracks.iter().filter(|t| t.track_type == TrackType::Subtitle)`) AND external subtitle tracks using `syncverse_demux::subtitles::scanner::scan_external_subtitles(&path)`?
   - How should `PlaybackSession` manage `available_subtitle_tracks: Vec<SubtitleTrack>`?
   - How should `UiEvent::TracksDiscovered` or a new event send the discovered subtitle tracks (with titles and language tags) to the UI instead of hardcoding `Vec::new()`?
   - How should subtitle track selection (`PlayerCommand::SelectSubtitleTrack`) and cycling (`PlayerCommand::CycleSubtitleTrack`) be handled in `PlaybackSession`?
2. In `crates/syncverse-ui/src/gui/app.rs`:
   - When `UiEvent::TracksDiscovered` or subtitle events arrive, how should `app.rs` populate `self.subtitle_tracks` (for the OSD/cycling) AND `self.subtitle_track_data` (the actual parsed `SubtitleTrack` objects containing cues)?
   - In `SubtitleConfigAction::LoadExternal(path)` (line 1033), implement genuine file loading:
     - Determine extension (.srt, .vtt, .ass).
     - Read and parse file using `parse_srt`, `parse_vtt`, or `parse_ass`.
     - Construct a `SubtitleTrack` and append to `self.subtitle_track_data` and `self.subtitle_tracks`.
     - Select it as active (`self.subtitle_track_index = Some(...)`), set `self.active_subtitle_cues = track.cues`, and show HUD notification.
   - When pressing `V` or `C` (cycling hotkeys), advance `self.subtitle_track_index`:
     - If advancing to a track, set `self.active_subtitle_cues` to that track's cues and show HUD banner with track title.
     - If advancing to "Off", clear `self.active_subtitle_cues` and show "Subtitles: Off".
3. End-to-End Integration Testing:
   - Design an authentic integration test in `crates/syncverse-core/tests/` or `crates/syncverse-ui/tests/` that:
     - Creates a synthetic media file with an external `.srt` and `.vtt` file matching its base stem.
     - Opens the media file through the player coordinator.
     - Verifies that `TracksDiscovered` contains the external and embedded tracks.
     - Simulates pressing `V`/`C` hotkey or sending `CycleSubtitleTrack` and verifies active track transitions.
     - Verifies that at the cue's timestamp, the active subtitle cue text is extracted and presented.
4. Output:
   - Formulate a precise, line-by-line implementation blueprint and file modification plan.
   - Deliver your full findings to `/home/gautham/syncverse/.agents/teamwork/explorer_remediation/handoff.md`.
   - Update `progress.md` and send a message when complete.
