## 2026-10-27T18:53:27Z
You are a teamwork_explorer performing a deep feature audit and architectural discovery for Syncverse.

Your working directory is:
/home/gautham/syncverse/.agents/teamwork/explorer_audit_1/

MANDATORY FIRST STEP:
Read the authoritative request file at:
/home/gautham/syncverse/.agents/teamwork/ORIGINAL_REQUEST.md

Also read:
/home/gautham/syncverse/AGENTS.md
/home/gautham/syncverse/.agents/teamwork/orchestrator/plan.md

TASK:
Perform a comprehensive audit of the Syncverse codebase to identify all missing desktop video player features and map the exact architecture and interfaces needed to implement:
1. Subtitle Detection and Track Selection:
   - Embedded subtitle tracks within media containers (MKV, MP4, etc.) via `syncverse-demux` / `ffmpeg-next`.
   - External subtitle files (.srt, .vtt, .ass) residing in the same directory as the media file with matching base filenames (e.g. `video.mkv` -> `video.srt`, `video.en.srt`, `video.vtt`, `video.ass`).
   - Pure-Rust subtitle parsers for SRT, WebVTT, and ASS cues (extracting start/end timestamps and text).
   - Subtitle track representation, active track selection, and 'Off'/disable state.
   - Synchronized rendering overlay in `syncverse-ui` video viewport using `MediaTime`.
2. Audio and Video Configuration Controls:
   - Audio: volume/mute control, audio delay/sync offset adjustment (e.g. in milliseconds), and multi-audio track selection / switching during active playback.
   - Video: aspect ratio selection/override (Original, 16:9, 4:3, 21:9, Fill/Stretch), playback speed control, video presentation settings.
   - User-facing UI dialogs/controls (menus, sliders, hotkeys).
3. Audio and Subtitle Cycling Hotkeys:
   - Dedicated keyboard shortcuts to cycle sequentially through available audio tracks and subtitle tracks during active playback.
   - On-screen visual HUD / telemetry status overlay displaying the newly selected track name/index upon cycling (with a brief fade-out timer).
4. Codebase Mapping:
   - Inspect existing crates:
     - `crates/syncverse-common`: timestamp types, shared models
     - `crates/syncverse-demux`: `UniversalDemuxer`, stream discovery, track indices, packet dispatch
     - `crates/syncverse-codecs`: video and audio decoding, format conversion
     - `crates/syncverse-clock`: master clock, `AudioPacer`, audio timing offset
     - `crates/syncverse-core`: player coordinator, stream control, `PlayerUiSnapshot`
     - `crates/syncverse-ui`: `eframe`/`egui`/`wgpu` viewport, playback controls, hotkeys, sidebar
   - Detail the exact files, structs, methods, channels/events, and snapshot fields to add or modify.
5. Invariants & Tests:
   - Ensure zero banned player wrappers/IPC (`mpv`, `vlc`) and zero external databases (`sqlite`, `rocksdb`).
   - Check existing tests in `tests/empirical_challenge/` and `cargo test`.
6. Deliverables:
   - Write a detailed audit and implementation specification to:
     `/home/gautham/syncverse/.agents/teamwork/explorer_audit_1/handoff.md`
   - Include a complete checklist of standard desktop player features.
   - Formulate a precise work breakdown for 10 parallel subagents (Workers, Reviewers, Challengers, Auditor) to implement and verify everything cleanly.
   - Update your `progress.md` with your status.
   - Send a message when complete.
