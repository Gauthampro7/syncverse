# Project: Syncplay-Alt Phase 4 (Desktop GUI & Universal Media Compatibility)

## Architecture
Syncplay-Alt is transformed into a flawless, user-friendly desktop media player and synchronized watch-party platform with a VLC-grade graphical UI and universal video/audio format compatibility:
- **Presentation & UI Layer**: Native cross-platform desktop graphical window via `eframe` / `egui` (with `wgpu` video texture surface), running natively on Fedora Linux (Wayland/X11) and Windows. Decoupled from media threads via lock-free SPSC 72-byte `PlayerUiSnapshot` triple-buffering (`TripleBufferSnapshotExchange`), 10Hz `ScrubberDebouncer`, and native file selection via `rfd` (XDG Desktop Portal).
- **Universal Codec & Demuxing Pipeline**: High-efficiency universal container demuxing (MKV/WebM, MP4/MOV, AVI, MPEG-TS, FLV, OGG) and video/audio bitstream decoding (H.264, H.265, VP8, VP9, AV1, MPEG-2, MPEG-4, VC-1; AAC, MP3, Opus, Vorbis, FLAC, AC3/E-AC3, DTS, uncompressed PCM) via `ffmpeg-next` and system `libav*` libraries, feeding into existing planar `VideoFrameYUV420p`, packed `VideoFrameRGBA`, and `AudioFramePCM` models.
- **Master Monotonic Clock & Audio Pacing**: `MasterClock` continuous presentation loop with atomic SeqLock rate re-anchoring guaranteeing $C^0$ mathematical continuity, audio buffer underrun protection with silence frame injection, and audio playback sinks.
- **Deterministic WAN Watch-Party Synchronization**: Real UDP transport, RFC 5905 NTP 4-timestamp sliding-window filtering with dispersion floor $D_{\text{effective}} \ge 5.0\text{ms}$, discrete PI PLL micro-speed controller ($r \in [0.96, 1.04]$), and two-phase barrier unpause ($<10\text{ms}$ cross-peer alignment).
- **Dual-Temporal Smart Chat & Slash Commands**: Append-only binary log `syncplay_chat.bin` with `0xAA55` framing, dual timestamps ($t_{\text{wall}}, t_{\text{media}}$), CRC32 checks, crash-resilient scanner, in-memory inverted index FTS, and interactive slash-commands (`/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`).
- **Specification & Invariant Standards**: Realigned `AGENTS.md`, `scripts/audit_dependencies.sh`, `scripts/intent_harness.py`, and `openspec/` documentation recognizing universal multimedia backends while strictly prohibiting monolithic player IPC wrappers (`mpv`, `vlc`) and external database engines (`sqlite`).

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | AGENTS.md Realignment | Remove obsolete bans on multimedia backends; whitelist eframe, egui, wgpu, rfd, ffmpeg-next, cpal | M1 (DONE) | ORIGINAL_REQUEST R3 |
| 2 | Dependency Audit Realignment | Update scripts/audit_dependencies.sh to reflect whitelisted crates while keeping player/DB wrapper bans | M1 (DONE) | ORIGINAL_REQUEST R3 |
| 3 | Pi Intent Harness Realignment | Update scripts/intent_harness.py and intent_matrix.json for universal media and desktop GUI intents | M1 (DONE) | ORIGINAL_REQUEST R3 |
| 4 | OpenSpec Documentation Realignment | Update openspec/project.md and openspec/specs/ to document Phase 4 architecture and GUI specifications | M1 (DONE) | ORIGINAL_REQUEST R3 |
| 5 | Universal Container Demuxing | Support MKV/WebM, MP4/MOV, AVI, MPEG-TS, FLV, OGG via UniversalDemuxer in syncplay-demux | M2 | ORIGINAL_REQUEST R2 |
| 6 | Universal Video Decoding | Decode H.264, H.265, VP8, VP9, AV1, MPEG-2, MPEG-4, VC-1 into VideoFrameYUV420p / VideoFrameRGBA | M2 | ORIGINAL_REQUEST R2 |
| 7 | Universal Audio Decoding | Decode AAC, MP3, Opus, Vorbis, FLAC, AC3/E-AC3, DTS, PCM into AudioFramePCM (48kHz f32 stereo) | M2 | ORIGINAL_REQUEST R2 |
| 8 | Clock & Pacer Integration | Queue universal decoded frames into 128B SPSC ring buffers, synchronized by MasterClock & AudioPacer | M2 | ORIGINAL_REQUEST R2 |
| 9 | Native Desktop Windowing | Desktop window (Wayland/X11, Windows), title bar, min/max/close, resize, and F11 fullscreen toggle | M3 | ORIGINAL_REQUEST R1 |
| 10 | GPU Video Viewport | GPU texture surface preserving aspect ratio with letterboxing/pillarboxing and vsync pacing | M3 | ORIGINAL_REQUEST R1 |
| 11 | Playback Controls Bar | Bottom docked bar with Play/Pause, Stop, volume slider, mute toggle, timecode HUD, speed selector | M3 | ORIGINAL_REQUEST R1 |
| 12 | Interactive Seekbar & Debouncer | Timeline seekbar with mouse hover preview tooltip and 10Hz debounced drag-scrubbing | M3 | ORIGINAL_REQUEST R1 |
| 13 | Watch-Party Sidebar & Chat | Collapsible right panel with room members, RTT, sync health badge, real-time chat, and slash-commands | M3 | ORIGINAL_REQUEST R1 |
| 14 | Native File Selection & Drag-and-Drop | File picker via rfd (XDG portal on Fedora GNOME), drag-and-drop loading, and recent media history | M3 | ORIGINAL_REQUEST R1 |
| 15 | Headless / Terminal Fallback | Maintain CLI flags (--headless, --terminal) for headless automated tests and terminal usage | M3 | ORIGINAL_REQUEST R1 |
| 16 | System Integration & Watch-Party Verification | End-to-end integration of GUI, universal codecs, UDP networking, NTP/PLL sync, and chat storage | M4 | ORIGINAL_REQUEST R1/R2 |
| 17 | Zero-Warning Workspace & Audit Verification | Complete cargo check, cargo test, clippy, audit_dependencies.sh, and intent_harness.py passing | M4 | ORIGINAL_REQUEST R3 |

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| 1 | Specification, Audits & Intent Harness Realignment | Update AGENTS.md, audit_dependencies.sh, intent_harness.py, openspec/ specs, and setup_deps.sh | none | DONE |
| 2 | Universal Video & Audio Compatibility Engine | UniversalDemuxer & UniversalDecoder for all required containers and video/audio codecs | M1 | DONE |
| 3 | Desktop Graphical Player UI | egui/eframe desktop window, GPU viewport, bottom controls, debounced seekbar, sidebar, rfd file picker | M1 | DONE |
| 4 | Full System Integration & E2E Validation | Integrate GUI with universal codecs, watch-party networking, zero warnings check, all tests passing | M2, M3 | DONE |




## Interface Contracts
### syncplay-demux ↔ syncplay-codecs
- `Demuxer` trait produces `DemuxPacket` containing stream index, payload bytes (`data: Vec<u8>`), presentation timestamp (`pts_ms: u64`), decode timestamp (`dts_ms: Option<u64>`), and keyframe flag (`is_keyframe: bool`).
- Video decoder consumes `DemuxPacket` and emits `VideoFrameYUV420p` (64B aligned planar buffers, 16B row stride, SIMD padding) or `VideoFrameRGBA` (64B aligned row stride).
- Audio decoder consumes `DemuxPacket` and emits `AudioFramePCM` (interleaved normalized `f32` in `[-1.0, 1.0]`, 48kHz, stereo).

### syncplay-codecs ↔ syncplay-clock ↔ syncplay-ui
- `SpscRingBuffer<VideoFrameRGBA, 32>`: 128-byte cache-line aligned lock-free buffer transferring decoded RGBA frames to the presentation loop.
- `SpscRingBuffer<AudioFramePCM, 64>`: transfers audio frames to `AudioPacer` and `AudioSink`.
- `TripleBufferSnapshotExchange`: 72-byte `PlayerUiSnapshot` published by `TelemetryAggregator` and polled wait-free by `SyncplayApp` (`egui`) each frame.
- `PlayerCommand`: Sent from UI thread to `PlayerCoordinator` (`TogglePause`, `Seek(u64)`, `SetSpeed(f64)`, `Stop`, `SetVolume(f32)`, `ToggleMute`, `OpenFile(PathBuf)`, `SendChatMessage(String)`, `ExecuteSlashCommand(String)`).
