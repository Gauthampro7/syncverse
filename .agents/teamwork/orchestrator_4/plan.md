# Plan — Syncplay-Alt Desktop GUI & Universal Codec Transformation

## Context & Vision
The user wants to transform Syncplay-Alt into a flawless, user-friendly desktop media player and synchronized watch-party platform with a VLC-grade graphical UI and universal video/audio format compatibility.
Integrity mode is explicitly set to `development` with clear instructions to realign `AGENTS.md`, `scripts/audit_dependencies.sh`, `scripts/intent_harness.py`, and `openspec/` documentation to remove obsolete prohibitions against multimedia and GUI backends.

## Phased Strategy & Milestones

### Phase 0: Scope Survey (3 Parallel Explorers)
- `explorer_survey_1`: Technical investigation of current codebase structure, Cargo workspace, crates (`syncplay-core`, `syncplay-demux`, `syncplay-codecs`, `syncplay-clock`, `syncplay-sync`, `syncplay-chat`, `syncplay-ui`), build status, tests, and audio/video pipeline.
- `explorer_survey_2`: Codec & Demuxing Architecture survey — evaluate integration options for universal format support (Containers: MKV/WebM, MP4/MOV, AVI, MPEG-TS, FLV, OGG; Video Codecs: H.264, H.265, VP8, VP9, AV1, MPEG-2, MPEG-4, VC-1; Audio Codecs: AAC, MP3, Opus, Vorbis, FLAC, AC3/E-AC3, DTS, PCM; crates like symphonia, ffmpeg-next / libav, pure-Rust decoders, or system bindings; connection to MasterClock and SPSC ring buffers).
- `explorer_survey_3`: Desktop GUI & Windowing survey — evaluate cross-platform GUI architecture (egui / winit / eframe / wgpu, or iced, slint; video rendering surface with letterboxing/pillarboxing, vsync frame pacing, custom widgets: scrub bar with hover preview, volume slider, timecode HUD, collapsible watch-party sidebar, drag-and-drop file loading, file picker dialog, desktop shortcuts).

### Milestone 1: Documentation, Audits & Intent Harness Realignment (R3)
- Realign `AGENTS.md`, `scripts/audit_dependencies.sh`, `scripts/intent_harness.py`, `openspec/project.md`, `openspec/specs/` to support modern GUI and codec backends.
- Cycle: 3 Explorers -> 1 Worker -> 2 Reviewers -> 2 Challengers -> 1 Forensic Auditor -> Gate.

### Milestone 2: Universal Video & Audio Compatibility Engine (R2)
- Implement universal demuxer and codec decoder pipeline feeding planar video frames and audio samples to `syncplay-clock` and `syncplay-sync`.
- Cycle: 3 Explorers -> 1 Worker -> 2 Reviewers -> 2 Challengers -> 1 Forensic Auditor -> Gate.

### Milestone 3: Desktop Graphical Player UI (R1)
- Build responsive graphical UI window with GPU video surface, bottom controls, scrubber, watch-party sidebar, file dialogs, drag & drop.
- Cycle: 3 Explorers -> 1 Worker -> 2 Reviewers -> 2 Challengers -> 1 Forensic Auditor -> Gate.

### Milestone 4: Watch-Party Integration & E2E Verification
- Full integration of GUI, universal codecs, network synchronization (NTP, PI PLL, barrier unpause), smart chat.
- Zero warnings across workspace, 100% tests passing.

### Milestone 5: Forensic Integrity Audit & Final Victory Attestation
- Rigorous verification, audit clearance, and comprehensive final report to parent/user.
