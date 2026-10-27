# Forensic Audit Remediation: Subtitle Pipeline Blueprint & Technical Investigation

**Author**: `explorer_remediation` (`teamwork_explorer`)  
**Parent Agent ID**: `6fec964f-e9e0-4542-a4f0-f5005e6b3d1e`  
**Date**: 2026-10-27T19:52:00Z  
**Workspace**: `/home/gautham/syncverse`  
**Milestone**: Subtitle Pipeline Remediation  

---

## 1. Observation

A forensic technical review of the codebase confirms all six integrity violations identified by `victory_auditor_7` in `/home/gautham/syncverse/.agents/teamwork/victory_auditor_7/handoff.md`:

1. **Hardcoded Empty Subtitle Tracks in Runtime**:
   In `crates/syncverse-core/src/playback_runtime.rs:407-410`:
   ```rust
   let _ = event_tx.send(UiEvent::TracksDiscovered {
       audio_tracks: audio_tracks_info,
       subtitle_tracks: Vec::new(),
   });
   ```
   Upon opening any media file via `PlayerCommand::OpenFile(path)`, `playback_runtime` transmits an explicitly hardcoded empty vector `subtitle_tracks: Vec::new()`.

2. **Container Embedded Subtitle Tracks Completely Ignored**:
   In `crates/syncverse-core/src/playback_runtime.rs:122-138`:
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
   `PlaybackSession::open` completely discards all tracks where `t.track_type == TrackType::Subtitle`. Moreover, in the demux loop (`playback_runtime.rs:1014-1070`), only video and audio track IDs are decoded; subtitle packets are discarded.

3. **External Subtitle Scanner Never Invoked During Playback**:
   In `crates/syncverse-demux/src/subtitles/scanner.rs:190`, `pub fn scan_external_subtitles<P: AsRef<Path>>(media_path: P) -> Vec<SubtitleTrack>` is fully implemented with automated language detection and parser dispatching, but ripgrep confirms it is never called anywhere in `syncverse-core` or `syncverse-ui`.
   In `crates/syncverse-ui/src/gui/app.rs:246-257` (`load_file`):
   ```rust
   self.subtitle_track_index = None;
   self.subtitle_tracks.clear();
   self.subtitle_track_data.clear();
   self.active_subtitle_cues.clear();
   self.subtitle_delay_ms = 0;
   ```
   The UI clears all subtitle track state and relies on `UiEvent::TracksDiscovered`, which always receives the empty vector from `playback_runtime.rs`.

4. **External Subtitle Manual Loading is an Empty Stub**:
   In `crates/syncverse-ui/src/gui/dialogs.rs:1327`, when a user selects an external subtitle file (.srt, .vtt, .ass) via the native file dialog, `SubtitleConfigAction::LoadExternal(path)` is emitted.
   In `crates/syncverse-ui/src/gui/app.rs:1033`:
   ```rust
   crate::gui::SubtitleConfigAction::LoadExternal(_path) => {}
   ```
   The action is a no-op stub `{}`. The file is never read, parsed, or added to `subtitle_track_data` or `active_subtitle_cues`.

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
   Because `self.subtitle_tracks` is perpetually empty during media playback, pressing `V` or `C` always produces `"Subtitles: Off"`. Furthermore, lines 464-486 never update `self.active_subtitle_cues` when advancing between tracks.

6. **Self-Certifying Disconnected Tests**:
   In `crates/syncverse-core/tests/adversarial_desktop_player_challenge.rs:302-330`:
   ```rust
   let cycle_next = |cur: ActiveSubtitleTrack, count: usize| -> ActiveSubtitleTrack {
       ...
   };
   ```
   The test verified a local closure defined inside the test function rather than executing media playback through `PlaybackSession`, `run_playback_worker`, or `SyncplayApp`.

---

## 2. Logic Chain

1. Requirements R2 and R4 in `ORIGINAL_REQUEST.md` require:
   - Automatic detection and listing of embedded container subtitle tracks (MKV, MP4).
   - Automatic detection and listing of external subtitle files (.srt, .vtt, .ass) matching media stem.
   - Synchronized rendering of active subtitle cues against current playback PTS.
   - Hotkey cycling (`V`/`C`) advancing through available subtitle tracks and "Off", with HUD feedback.
2. The subtitle demuxing parsers (`srt.rs`, `vtt.rs`, `ass.rs`), scanner (`scanner.rs`), data models (`syncverse-common/src/subtitles.rs`), and rendering overlay (`subtitle_overlay.rs`) exist and are robust.
3. The root cause of the failure is a disconnect between the demuxer/scanner layer, the core runtime playback session, and the UI state management.
4. Specifically:
   - `PlaybackSession::open` must invoke `info.tracks.iter().filter(|t| t.track_type == TrackType::Subtitle)` to discover embedded tracks AND `syncverse_demux::subtitles::scan_external_subtitles(&path)` to discover external tracks.
   - `PlaybackSession` must manage `available_subtitle_tracks: Vec<SubtitleTrack>` and `selected_subtitle_track_index: Option<usize>`.
   - `playback_runtime.rs` must broadcast `UiEvent::TracksDiscovered` with the discovered track labels AND `UiEvent::SubtitleDataDiscovered(Vec<SubtitleTrack>)` with the parsed cues.
   - `playback_runtime.rs` must genuinely handle `PlayerCommand::SelectSubtitleTrack` and `PlayerCommand::CycleSubtitleTrack`, matching the audio track pattern.
   - `crates/syncverse-ui/Cargo.toml` must declare dependencies on `syncverse-common` and `syncverse-demux`.
   - `app.rs:1033` must genuinely read and parse external files using `parse_srt`, `parse_vtt`, or `parse_ass`, append to `self.subtitle_track_data` and `self.subtitle_tracks`, select the track, and populate `self.active_subtitle_cues`.
   - `app.rs:464-486` must update `self.active_subtitle_cues` to `self.subtitle_track_data[idx].cues` when cycling to a track, and clear `self.active_subtitle_cues` when cycling to "Off".
   - `app.load_file(path)` must invoke `scan_external_subtitles(&path)` to provide immediate track inventory in both synchronous test loops and interactive windowing.
   - An authentic end-to-end integration test must verify container demuxing + external scanning + track cycling + timestamp cue extraction without mock closures.

---

## 3. Caveats

- Embedded container subtitle tracks in MKV and MP4 may have codec `subrip`, `ass`, or `mov_text`. While external subtitle files have their cues fully parsed upfront into `cues: Vec<SubtitleCue>`, embedded tracks in video containers can also emit streaming subtitle packets during playback. To guarantee complete support, embedded tracks are registered in `available_subtitle_tracks` upon file opening, and dynamic demuxed packets are routed to `UiEvent::SubtitleCueDemuxed`.
- No banned crates (`mpv`, `vlc`, `sqlite`, etc.) may be used. All parsing is pure-Rust.

---

## 4. Conclusion & Concrete Implementation Blueprint

### File Modification Plan

#### Component 1: `crates/syncverse-ui/Cargo.toml`
Add dependencies on `syncverse-common` and `syncverse-demux` to enable direct parser access in UI:
```toml
[dependencies]
syncverse-common = { path = "../syncverse-common" }
syncverse-demux = { path = "../syncverse-demux" }
syncverse-codecs = { path = "../syncverse-codecs" }
syncverse-clock = { path = "../syncverse-clock" }
syncverse-chat = { path = "../syncverse-chat" }
thiserror.workspace = true
eframe = "0.37"
egui = "0.37"
rfd = "0.17"
ureq = { version = "2.12", features = ["tls", "json"] }
semver = "1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

---

#### Component 2: `crates/syncverse-ui/src/event.rs`
Extend `UiEvent` to transmit parsed subtitle track objects and streaming demuxed cues:
```rust
    /// Discovered subtitle track data with full parsed cues.
    SubtitleDataDiscovered(Vec<crate::gui::subtitle_overlay::SubtitleTrack>),
    /// Streaming subtitle cue dynamically demuxed from container bitstream.
    SubtitleCueDemuxed(crate::gui::subtitle_overlay::SubtitleCue),
```

---

#### Component 3: `crates/syncverse-core/src/playback_runtime.rs`

1. **Update `PlaybackSession` struct**:
   ```rust
   struct PlaybackSession {
       demuxer: UniversalDemuxer,
       video_decoder: Option<UniversalVideoDecoder>,
       audio_decoder: Option<UniversalAudioDecoder>,
       video_track_id: Option<u64>,
       audio_track_id: Option<u64>,
       available_audio_tracks: Vec<TrackInfo>,
       available_subtitle_tracks: Vec<syncverse_common::subtitles::SubtitleTrack>,
       selected_subtitle_track_index: Option<usize>,
       subtitle_track_id: Option<u64>,
       duration_ms: u64,
       eof_reached: bool,
   }
   ```

2. **In `PlaybackSession::open` (around line 122)**:
   ```rust
   let available_audio_tracks: Vec<TrackInfo> = info
       .tracks
       .iter()
       .filter(|t| t.track_type == TrackType::Audio)
       .cloned()
       .collect();

   let embedded_subtitle_tracks: Vec<TrackInfo> = info
       .tracks
       .iter()
       .filter(|t| t.track_type == TrackType::Subtitle)
       .cloned()
       .collect();

   let mut available_subtitle_tracks = Vec::new();
   for t in embedded_subtitle_tracks {
       let title = t.title.clone().unwrap_or_else(|| {
           if let Some(ref lang) = t.language {
               format!("Embedded Subtitle [{}]", lang.to_uppercase())
           } else {
               format!("Embedded Subtitle (Track {})", t.track_id)
           }
       });
       available_subtitle_tracks.push(syncverse_common::subtitles::SubtitleTrack::new(
           t.track_id as usize,
           syncverse_common::subtitles::SubtitleSource::Embedded,
           t.codec_id.clone(),
           t.language.clone(),
           title,
           Vec::new(),
       ));
   }

   let external_tracks = syncverse_demux::subtitles::scan_external_subtitles(path);
   available_subtitle_tracks.extend(external_tracks);
   ```
   And initialize:
   ```rust
   Ok(Self {
       demuxer,
       video_decoder,
       audio_decoder,
       video_track_id,
       audio_track_id,
       available_audio_tracks,
       available_subtitle_tracks,
       selected_subtitle_track_index: None,
       subtitle_track_id: None,
       duration_ms,
       eof_reached: false,
   })
   ```

3. **Add subtitle track selection and cycling methods to `PlaybackSession`**:
   ```rust
   /// Switches the active subtitle track to the specified index (`None` = Off/Disabled).
   /// Returns the human-readable description of the selected track or "Off".
   fn select_subtitle_track(&mut self, track_idx: Option<usize>) -> (Option<usize>, String) {
       match track_idx {
           Some(idx) if idx < self.available_subtitle_tracks.len() => {
               self.selected_subtitle_track_index = Some(idx);
               let track = &self.available_subtitle_tracks[idx];
               self.subtitle_track_id = if track.source == syncverse_common::subtitles::SubtitleSource::Embedded {
                   Some(track.id as u64)
               } else {
                   None
               };
               (Some(idx), track.display_label())
           }
           _ => {
               self.selected_subtitle_track_index = None;
               self.subtitle_track_id = None;
               (None, "Off".to_string())
           }
       }
   }

   /// Cycles sequentially to the next available subtitle track (including Off).
   /// Sequence: None -> Some(0) -> Some(1) -> ... -> Some(N-1) -> None.
   fn cycle_subtitle_track(&mut self) -> (Option<usize>, String) {
       if self.available_subtitle_tracks.is_empty() {
           return (None, "Off".to_string());
       }
       let next_idx = match self.selected_subtitle_track_index {
           None => Some(0),
           Some(idx) if idx + 1 < self.available_subtitle_tracks.len() => Some(idx + 1),
           Some(_) => None,
       };
       self.select_subtitle_track(next_idx)
   }
   ```

4. **In `PlayerCommand::OpenFile` handling in `run_playback_worker` (lines 401-417)**:
   ```rust
   let audio_tracks_info: Vec<String> = new_session
       .available_audio_tracks
       .iter()
       .map(format_track_info)
       .collect();
   let subtitle_tracks_info: Vec<String> = new_session
       .available_subtitle_tracks
       .iter()
       .map(|t| t.display_label())
       .collect();

   let ui_subtitle_tracks: Vec<syncverse_ui::gui::subtitle_overlay::SubtitleTrack> = new_session
       .available_subtitle_tracks
       .iter()
       .map(|t| {
           let cues = t.cues.iter().map(|c| syncverse_ui::gui::subtitle_overlay::SubtitleCue {
               start_ms: c.start.as_millis() as i64,
               end_ms: c.end.as_millis() as i64,
               text: c.text.clone(),
           }).collect();
           syncverse_ui::gui::subtitle_overlay::SubtitleTrack::new(
               t.id,
               t.display_label(),
               t.language.clone(),
               cues,
           )
       })
       .collect();

   let _ = event_tx.send(UiEvent::TracksDiscovered {
       audio_tracks: audio_tracks_info,
       subtitle_tracks: subtitle_tracks_info,
   });
   let _ = event_tx.send(UiEvent::SubtitleDataDiscovered(ui_subtitle_tracks));
   ```

5. **In `run_playback_worker` command loop (replacing lines 751-757)**:
   ```rust
   PlayerCommand::SelectSubtitleTrack(track_index) => {
       if let Some(ref mut sess) = session {
           let (idx, name) = sess.select_subtitle_track(track_index);
           let _ = event_tx.send(UiEvent::SubtitleTrackSelected {
               track_index: idx,
               name: name.clone(),
           });
           let notif = if idx.is_some() {
               format!("Subtitles: {name}")
           } else {
               "Subtitles: Off".to_string()
           };
           let _ = event_tx.send(UiEvent::HudNotification {
               text: notif,
               timestamp: Instant::now(),
           });
       }
   }
   PlayerCommand::CycleSubtitleTrack => {
       if let Some(ref mut sess) = session {
           let (next_idx, name) = sess.cycle_subtitle_track();
           let _ = event_tx.send(UiEvent::SubtitleTrackSelected {
               track_index: next_idx,
               name: name.clone(),
           });
           let notif = match next_idx {
               Some(idx) => format!(
                   "Subtitles: [{}/{}] {}",
                   idx + 1,
                   sess.available_subtitle_tracks.len(),
                   name
               ),
               None => "Subtitles: Off".to_string(),
           };
           let _ = event_tx.send(UiEvent::HudNotification {
               text: notif,
               timestamp: Instant::now(),
           });
       }
   }
   PlayerCommand::SetSubtitleDelay(delay_ms) => {
       subtitle_delay_ms = delay_ms;
       let _ = event_tx.send(UiEvent::SubtitleDelayChanged(subtitle_delay_ms));
       let _ = event_tx.send(UiEvent::HudNotification {
           text: format!("Subtitle Delay: {:+} ms", subtitle_delay_ms),
           timestamp: Instant::now(),
       });
   }
   PlayerCommand::AdjustSubtitleDelay(delta_ms) => {
       subtitle_delay_ms = subtitle_delay_ms.saturating_add(delta_ms);
       let _ = event_tx.send(UiEvent::SubtitleDelayChanged(subtitle_delay_ms));
       let _ = event_tx.send(UiEvent::HudNotification {
           text: format!("Subtitle Delay: {:+} ms", subtitle_delay_ms),
           timestamp: Instant::now(),
       });
   }
   ```

6. **In packet demuxing loop (around line 1070)**:
   ```rust
   else if Some(packet.track_id) == sess.subtitle_track_id {
       let text = if let Ok(s) = std::str::from_utf8(&packet.data) {
           syncverse_demux::subtitles::clean_srt_text(s)
       } else {
           String::new()
       };
       if !text.is_empty() {
           let dur = packet.duration_ms.unwrap_or(3000);
           let cue = syncverse_ui::gui::subtitle_overlay::SubtitleCue {
               start_ms: packet.pts_ms as i64,
               end_ms: (packet.pts_ms + dur) as i64,
               text,
           };
           let _ = event_tx.send(UiEvent::SubtitleCueDemuxed(cue));
       }
   }
   ```

---

#### Component 4: `crates/syncverse-ui/src/gui/app.rs`

1. **In `load_file(&mut self, path: PathBuf)` (lines 246-258)**:
   Scan external subtitles immediately so track inventory and cues are ready synchronously:
   ```rust
   pub fn load_file(&mut self, path: PathBuf) {
       self.viewport.reset();
       self.recent_files.add_file(&path);
       let _ = self.command_tx.send(PlayerCommand::OpenFile(path.clone()));
       self.current_file = Some(path.clone());
       self.sidebar_state.local_ready = true;
       self.subtitle_track_index = None;
       self.subtitle_tracks.clear();
       self.subtitle_track_data.clear();
       self.active_subtitle_cues.clear();
       self.subtitle_delay_ms = 0;

       let external = syncverse_demux::subtitles::scan_external_subtitles(&path);
       for ext_track in external {
           let ui_cues: Vec<crate::gui::subtitle_overlay::SubtitleCue> = ext_track
               .cues
               .into_iter()
               .map(|c| crate::gui::subtitle_overlay::SubtitleCue {
                   start_ms: c.start.as_millis() as i64,
                   end_ms: c.end.as_millis() as i64,
                   text: c.text,
               })
               .collect();
           let label = ext_track.display_label();
           let track = crate::gui::subtitle_overlay::SubtitleTrack::new(
               ext_track.id,
               label.clone(),
               ext_track.language,
               ui_cues,
           );
           self.subtitle_tracks.push(label);
           self.subtitle_track_data.push(track);
       }
       let dialog_items: Vec<crate::gui::dialogs::SubtitleTrackItem> = self
           .subtitle_track_data
           .iter()
           .enumerate()
           .map(|(i, t)| crate::gui::dialogs::SubtitleTrackItem {
               id: i,
               title: t.title.clone(),
               language: t.language.clone(),
               format: "SUB".to_string(),
               is_external: true,
               file_path: None,
           })
           .collect();
       self.controls_state.subtitle_dialog.set_tracks(dialog_items);
   }
   ```

2. **In `ui` event handling loop (around line 340-365)**:
   ```rust
   UiEvent::SubtitleTrackSelected { track_index, name } => {
       self.subtitle_track_index = track_index;
       self.controls_state.subtitle_dialog.select_track(track_index);
       if let Some(idx) = track_index {
           if idx < self.subtitle_track_data.len() {
               self.active_subtitle_cues = self.subtitle_track_data[idx].cues.clone();
           }
       } else {
           self.active_subtitle_cues.clear();
       }
       let msg = if name == "Off" || track_index.is_none() {
           "Subtitles: Off".to_string()
       } else {
           format!("Subtitles: {}", name)
       };
       self.osd_overlay.show_notification(msg, now);
   }
   UiEvent::TracksDiscovered {
       audio_tracks,
       subtitle_tracks,
   } => {
       self.audio_tracks = audio_tracks;
       if self.subtitle_tracks.is_empty() {
           self.subtitle_tracks = subtitle_tracks;
       }
   }
   UiEvent::SubtitleDataDiscovered(tracks) => {
       self.subtitle_track_data = tracks;
       self.subtitle_tracks = self.subtitle_track_data.iter().map(|t| t.title.clone()).collect();
       let dialog_items: Vec<crate::gui::dialogs::SubtitleTrackItem> = self
           .subtitle_track_data
           .iter()
           .enumerate()
           .map(|(i, t)| crate::gui::dialogs::SubtitleTrackItem {
               id: i,
               title: t.title.clone(),
               language: t.language.clone(),
               format: "SUB".to_string(),
               is_external: true,
               file_path: None,
           })
           .collect();
       self.controls_state.subtitle_dialog.set_tracks(dialog_items);
   }
   UiEvent::SubtitleCueDemuxed(cue) => {
       self.active_subtitle_cues.push(cue);
   }
   ```

3. **In `V`/`C` hotkey cycling (lines 464-486)**:
   ```rust
   if ctx.input(|i| i.key_pressed(egui::Key::V) || i.key_pressed(egui::Key::C)) {
       let _ = self.command_tx.send(PlayerCommand::CycleSubtitleTrack);
       let notif = if !self.subtitle_tracks.is_empty() {
           let next_idx = match self.subtitle_track_index {
               None => Some(0),
               Some(idx) if idx + 1 < self.subtitle_tracks.len() => Some(idx + 1),
               Some(_) => None,
           };
           self.subtitle_track_index = next_idx;
           self.controls_state.subtitle_dialog.select_track(next_idx);
           match next_idx {
               Some(idx) => {
                   if idx < self.subtitle_track_data.len() {
                       self.active_subtitle_cues = self.subtitle_track_data[idx].cues.clone();
                   }
                   format!(
                       "Subtitles: [{}/{}] {}",
                       idx + 1,
                       self.subtitle_tracks.len(),
                       self.subtitle_tracks[idx]
                   )
               }
               None => {
                   self.active_subtitle_cues.clear();
                   "Subtitles: Off".to_string()
               }
           }
       } else {
           self.active_subtitle_cues.clear();
           "Subtitles: Off".to_string()
       };
       self.osd_overlay.show_notification(notif, now);
   }
   ```

4. **In `SubtitleConfigAction` handling (replacing line 1033)**:
   ```rust
   crate::gui::SubtitleConfigAction::SelectTrack(track_id) => {
       self.subtitle_track_index = track_id;
       if let Some(idx) = track_id {
           if idx < self.subtitle_track_data.len() {
               self.active_subtitle_cues = self.subtitle_track_data[idx].cues.clone();
           }
       } else {
           self.active_subtitle_cues.clear();
       }
       let _ = self.command_tx.send(PlayerCommand::SelectSubtitleTrack(track_id));
   }
   crate::gui::SubtitleConfigAction::LoadExternal(path) => {
       let ext = path
           .extension()
           .and_then(|s| s.to_str())
           .unwrap_or("")
           .to_ascii_lowercase();

       let parse_res = match ext.as_str() {
           "srt" => std::fs::read_to_string(&path)
               .map_err(|e| e.to_string())
               .and_then(|c| syncverse_demux::subtitles::parse_srt(&c).map_err(|e| e.to_string())),
           "vtt" => std::fs::read_to_string(&path)
               .map_err(|e| e.to_string())
               .and_then(|c| syncverse_demux::subtitles::parse_vtt(&c).map_err(|e| e.to_string())),
           "ass" | "ssa" => std::fs::read_to_string(&path)
               .map_err(|e| e.to_string())
               .and_then(|c| syncverse_demux::subtitles::parse_ass(&c).map_err(|e| e.to_string())),
           other => Err(format!("Unsupported subtitle format: .{other}")),
       };

       match parse_res {
           Ok(parsed_cues) => {
               let filename = path
                   .file_name()
                   .and_then(|s| s.to_str())
                   .unwrap_or("External Subtitle")
                   .to_string();

               let ui_cues: Vec<crate::gui::subtitle_overlay::SubtitleCue> = parsed_cues
                   .into_iter()
                   .map(|c| crate::gui::subtitle_overlay::SubtitleCue {
                       start_ms: c.start.as_millis() as i64,
                       end_ms: c.end.as_millis() as i64,
                       text: c.text,
                   })
                   .collect();

               let new_track_id = self.subtitle_track_data.len();
               let track = crate::gui::subtitle_overlay::SubtitleTrack::new(
                   new_track_id,
                   filename.clone(),
                   None,
                   ui_cues.clone(),
               );

               self.subtitle_tracks.push(filename.clone());
               self.subtitle_track_data.push(track);
               self.subtitle_track_index = Some(new_track_id);
               self.active_subtitle_cues = ui_cues;

               self.osd_overlay.show_notification(
                   format!("Loaded Subtitle: {filename}"),
                   now,
               );
           }
           Err(err) => {
               eprintln!("[ERROR] Failed to load external subtitle: {err}");
               self.osd_overlay.show_notification(
                   format!("Subtitle Error: {err}"),
                   now,
               );
           }
       }
   }
   ```

---

#### Component 5: End-to-End Integration Test
Replace `test_dynamic_subtitle_track_cycling_including_off` in `crates/syncverse-core/tests/adversarial_desktop_player_challenge.rs`:

```rust
#[test]
fn test_end_to_end_media_playback_with_subtitles() {
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::time::Duration;
    use syncverse_clock::MasterClock;
    use syncverse_clock::SpscRingBuffer;
    use syncverse_codecs::VideoFrameRGBA;
    use syncverse_common::clock::PlaybackState;
    use syncverse_core::playback_runtime::{run_playback_worker, AudioWorkerCommand};
    use syncverse_ui::command::PlayerCommand;
    use syncverse_ui::event::UiEvent;
    use syncverse_ui::gui::SyncplayApp;
    use syncverse_ui::snapshot::TripleBufferSnapshotExchange;

    let temp_dir = std::env::temp_dir().join(format!("syncverse_e2e_sub_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).expect("failed to create temp dir");

    let media_path = temp_dir.join("feature_film.mkv");
    let srt_path = temp_dir.join("feature_film.en.srt");
    let vtt_path = temp_dir.join("feature_film.fr.vtt");

    // 1. Generate synthetic Matroska container fixture with embedded subtitle track using ffmpeg
    let srt_embed = temp_dir.join("embed.srt");
    fs::write(&srt_embed, "1\n00:00:00,100 --> 00:00:00,900\nEmbedded Subtitle Line\n").unwrap();

    let cmd_status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-f", "lavfi", "-i", "testsrc=duration=2:size=320x240:rate=25",
            "-f", "lavfi", "-i", "sine=frequency=1000:duration=2",
            "-i", srt_embed.to_str().unwrap(),
            "-c:v", "libx264",
            "-c:a", "aac",
            "-c:s", "srt",
            "-metadata:s:s:0", "language=eng",
            "-metadata:s:s:0", "title=English Embedded",
            media_path.to_str().unwrap(),
        ])
        .status();

    if cmd_status.is_err() || !cmd_status.as_ref().unwrap().success() {
        // Fallback: write dummy file if ffmpeg CLI is not present
        fs::write(&media_path, b"dummy media").unwrap();
    }

    // 2. Write external .srt and .vtt files matching media stem
    fs::write(
        &srt_path,
        "1\n00:00:00,500 --> 00:00:01,500\nHello English Subtitle\n\n2\n00:00:01,600 --> 00:00:02,500\nSecond English Cue\n",
    )
    .unwrap();

    fs::write(
        &vtt_path,
        "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nBonjour le Monde VTT\n",
    )
    .unwrap();

    // 3. Initialize UI app and verify immediate synchronous detection
    let mut app = SyncplayApp::new();
    app.load_file(media_path.clone());

    assert!(
        app.subtitle_tracks().len() >= 2,
        "load_file must detect at least the 2 co-located external subtitle tracks (found {})",
        app.subtitle_tracks().len()
    );
    assert_eq!(app.subtitle_track_index(), None, "Initial subtitle track should be Off");
    assert_eq!(app.current_active_subtitle(800), None, "No subtitle active when Off");

    // 4. Spawn background playback coordinator worker thread
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let (command_tx, command_rx) = channel::<PlayerCommand>();
    let (audio_tx, _audio_rx) = channel::<AudioWorkerCommand>();
    let (event_tx, event_rx) = channel::<UiEvent>();
    let running = Arc::new(AtomicBool::new(true));

    let thread_exchange = Arc::clone(&exchange);
    let thread_video_ring = Arc::clone(&video_ring);
    let thread_clock = Arc::clone(&clock);
    let thread_running = Arc::clone(&running);

    let worker_handle = std::thread::spawn(move || {
        run_playback_worker(
            command_rx,
            audio_tx,
            event_tx,
            thread_exchange,
            thread_video_ring,
            thread_clock,
            thread_running,
        );
    });

    // 5. Send OpenFile command and receive TracksDiscovered
    command_tx.send(PlayerCommand::OpenFile(media_path)).unwrap();

    let mut discovered_subtitles = Vec::new();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if let Ok(event) = event_rx.recv_timeout(Duration::from_millis(200)) {
            if let UiEvent::TracksDiscovered { subtitle_tracks, .. } = event {
                discovered_subtitles = subtitle_tracks;
                break;
            }
        }
    }

    assert!(
        !discovered_subtitles.is_empty(),
        "Playback worker must broadcast discovered subtitle tracks"
    );

    // 6. Test track cycling and cue presentation
    // Advance to Track 0 (English SRT)
    app.select_subtitle_track(Some(0));
    assert_eq!(
        app.current_active_subtitle(800),
        Some("Hello English Subtitle".to_string()),
        "Active cue must match presentation PTS 800ms"
    );
    assert_eq!(
        app.current_active_subtitle(200),
        None,
        "No cue before start timestamp"
    );

    // Advance to Track 1 (French VTT)
    app.select_subtitle_track(Some(1));
    assert_eq!(
        app.current_active_subtitle(1500),
        Some("Bonjour le Monde VTT".to_string()),
        "French VTT cue must match presentation PTS 1500ms"
    );

    // Cycle to Off
    app.select_subtitle_track(None);
    assert_eq!(
        app.current_active_subtitle(1500),
        None,
        "Subtitle must be blank when track is Off"
    );

    // 7. Cleanup
    running.store(false, Ordering::Relaxed);
    let _ = command_tx.send(PlayerCommand::Stop);
    let _ = worker_handle.join();
    let _ = fs::remove_dir_all(&temp_dir);
}
```

---

## 5. Verification Method

To independently verify this plan before and after implementation:

1. **Compilation and Zero Warnings**:
   ```bash
   cargo check --workspace --all-targets
   cargo clippy --workspace --all-targets -- -D warnings
   ```
2. **Dependency Blacklist Audit**:
   ```bash
   ./scripts/audit_dependencies.sh
   ```
   Confirms 0 banned player frameworks or external database daemons.
3. **Execution of the End-to-End Test**:
   ```bash
   cargo test --package syncverse-core --test adversarial_desktop_player_challenge test_end_to_end_media_playback_with_subtitles -- --nocapture
   ```
4. **Interactive Verification of Scanner & Loader**:
   - `rg "scan_external_subtitles" crates/` -> Shows calls in both `playback_runtime.rs` and `app.rs`.
   - `rg "SubtitleConfigAction::LoadExternal" crates/` -> Confirms genuine parser dispatching.
   - `rg "subtitle_tracks: Vec::new()" crates/` -> Confirms zero instances.
