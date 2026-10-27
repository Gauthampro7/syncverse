# Plan: Desktop Video Player Standard Features Implementation

## Architecture & Scope
Implement missing standard desktop video player features in Syncverse:
- Subtitle detection & track selection (embedded container tracks MKV/MP4 + external .srt, .vtt, .ass)
- Subtitle parsing and synchronized rendering in UI viewport
- Audio configuration: volume/mute, audio delay/sync offset, audio track selection
- Video configuration: aspect ratio selection/override, playback speed control, video adjustments
- Audio & Subtitle cycling hotkeys with visual HUD / status telemetry notifications
- Strict compliance with AGENTS.md invariants (pure Rust / ffmpeg-next, zero banned player wrappers/IPC, zero external database daemons)

## Execution Structure

### Phase 1: Feature Audit & Architecture Discovery (1 Subagent)
- **Agent**: `explorer_audit_1` (`teamwork_explorer`)
- **Mission**:
  1. Inspect `crates/syncverse-demux`, `crates/syncverse-codecs`, `crates/syncverse-core`, `crates/syncverse-ui`, `crates/syncverse-clock`, `crates/syncverse-common` for current track management, playback loop, UI controls, input handling, and rendering.
  2. Map out how container audio & subtitle streams are exposed by `UniversalDemuxer` / `ffmpeg-next`.
  3. Map out external subtitle scanning in media directory (.srt, .vtt, .ass).
  4. Identify exact data structures needed in `syncverse-common` / `syncverse-demux` / `syncverse-codecs` / `syncverse-core` / `syncverse-ui` (e.g., `SubtitleTrack`, `SubtitleCue`, `AudioTrackInfo`, `VideoConfig`, `AudioConfig`, `PlayerUiSnapshot` fields).
  5. Formulate complete checklist of standard desktop player features and interface contracts.
  6. Deliver comprehensive audit report to `.agents/teamwork/explorer_audit_1/handoff.md`.

### Phase 2: Parallel Implementation & Verification Panel (10 Subagents in Parallel)
- **Worker 1 (`worker_subtitles`)**: Implement subtitle detection in demuxer (embedded MKV/MP4 tracks), external subtitle scanner (.srt, .vtt, .ass matching media file base name), and pure-Rust subtitle parser (SRT/WebVTT/ASS text cue extraction with start/end timestamps).
- **Worker 2 (`worker_audio_mgmt`)**: Implement multi-audio track demuxing & decoder switching in demuxer/codecs/core, audio delay / sync offset adjustment, volume scaling and mute in audio sink / pacer.
- **Worker 3 (`worker_video_controls`)**: Implement aspect ratio selection/override (16:9, 4:3, 21:9, original, fill/crop), playback speed adjustment controls, and video presentation configuration.
- **Worker 4 (`worker_ui_controls`)**: Implement user-facing UI controls in `syncverse-ui`: Audio Configuration modal/menu (track selector, volume slider, sync offset slider), Video Configuration modal/menu (aspect ratio dropdown, speed selector), Subtitle selector menu (track listing with 'Off' option).
- **Worker 5 (`worker_hud_hotkeys`)**: Implement keyboard shortcuts (audio cycle hotkey, subtitle cycle hotkey, volume, mute, delay adjust) and on-screen HUD / status notification banner rendering in viewport with fading duration.
- **Worker 6 (`worker_subtitle_rendering`)**: Implement synchronized subtitle rendering overlay in `syncverse-ui` video viewport using current presentation timestamp `MediaTime`.
- **Reviewer 1 (`reviewer_core_codecs`)**: Review backend crates (`common`, `demux`, `codecs`, `core`, `clock`), checking compilation `#![deny(warnings)]`, clippy cleanliness, unit tests, and performance.
- **Reviewer 2 (`reviewer_ui_hud`)**: Review UI crates (`ui`, `core`), checking UX controls, hotkeys, HUD telemetry, aspect ratio calculations, subtitle display, and error handling.
- **Challenger 1 (`challenger_empirical`)**: Run and verify existing empirical test suites (`test_demux_codecs_clock.py`, `test_sync_engine.py`, `test_smart_chat.py`, `test_ui_renderer.py`), add test cases for track switching, subtitle sync, audio offset, and verify regressions.
- **Auditor 1 (`auditor_integrity`)**: Perform forensic audit: check against dependency blacklist (`./scripts/audit_dependencies.sh`), verify no hardcoded dummy data or facade implementations, verify pure Rust / ffmpeg-next compliance.

### Phase 3: Gate Evaluation & Synthesis
- Evaluate Gate Matrix across all 10 agents.
- Ensure all criteria pass.
- Synthesize findings and report to user/parent.
