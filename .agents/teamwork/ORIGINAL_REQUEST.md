# Original User Request

## 2026-10-24T23:00:07Z

Architect, specify, and scaffold an open-source, high-efficiency standalone media player and long-distance synchronized watch-party platform in Rust (`no pre-built media/codec frameworks` — container demuxers, baseline video/audio bitstream decoders, A/V clock pipeline, network sync engine, and local-persisted smart chat built from the ground up). This Phase 1 deliverable establishes the exhaustive OpenSpec specifications, the Pi agent-driven development framework (`.pi/`, `AGENTS.md`, skills, and workflows), formal mathematical protocols, and a compiling, testable Rust workspace scaffold with working foundational primitives and verification tests.

Working directory: /home/gautham/syncplay-alt
Integrity mode: demo

## Requirements

### R1. OpenSpec Specification Suite (`openspec/`)
Create a complete, production-grade OpenSpec specification hierarchy covering the entire system before full feature implementation begins. It must include project context (`openspec/project.md`), master architecture roadmap, and detailed capability specs (`openspec/specs/<capability>/spec.md` with formal RFC-2119 MUST/SHALL requirements and testable Given/When/Then scenarios) for every subsystem:
1. **Zero-dependency container demuxing** (MKV/EBML, MP4/ISOBMFF) and raw bitstream parsing.
2. **From-scratch baseline video & audio codec decoding**, color-space conversion, and hardware/GPU presentation pipeline.
3. **Master monotonic A/V synchronization clock** and lock-free ring-buffer pipeline.
4. **Long-distance high-RTT synchronization protocol** (NTP-style clock offset estimation with outlier filtering, deterministic reference timeline equation, Phase-Locked Loop micro-speed drift correction without hard-seek stutter, and barrier-synchronized ready/commit unpause).
5. **Locally persisted smart in-player chat system** featuring:
   - Timestamp-anchored messages and reactions tied directly to the media timeline (timeline markers / overlay annotations).
   - Custom zero-dependency local binary/indexed storage engine with instant full-text search and room history export.
   - Smart slash-commands and interactive media control triggers (`/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`) synchronized across the party transport.
6. **Native player UI & OSD overlay architecture** with VLC-grade responsiveness and minimal memory footprint.

### R2. Pi Framework Agent-Driven Development Setup (`.pi/` & `AGENTS.md`)
Set up a complete Pi coding-agent framework inside the repository so autonomous agent runs can iteratively build, verify, and benchmark each module from the OpenSpec specs. This includes:
- Root `AGENTS.md` defining architecture invariants, strict "no external codec/player library" rules, coding standards, and verification commands.
- `.pi/` directory containing specialized agent prompts, skills, task workflows, and phase-by-phase execution plans mapping directly to OpenSpec change proposals (`openspec/changes/`).

### R3. Pure-Rust Multi-Crate Workspace Scaffold & Working Foundational Primitives
Initialize a clean, compiling Rust workspace in `/home/gautham/syncplay-alt` where core functionality does not rely on pre-built media, codec, or sync frameworks. Each subsystem must be isolated into a dedicated crate with concrete trait boundaries, core data structures, binary wire format definitions, working foundational primitives (EBML/ISOBMFF header parsers, NTP clock filter + PLL speed controller math, custom binary chat log + inverted index search + slash-command parser), and executable unit tests.

### R4. Self-Contained Verification Harness & Deterministic Simulation Suite
Provide an automated verification script (`scripts/verify_phase1.sh`) that validates OpenSpec completeness, Pi framework integrity, zero forbidden external media dependencies in `Cargo.lock`, workspace compilation (`cargo test --workspace`), synthetic binary test vector parsing, and a deterministic high-RTT network simulation proving the clock-sync + PLL math converges within target error bounds across simulated 300ms+ jittery links.

## Acceptance Criteria

### OpenSpec & Pi Framework Completeness
- [ ] `openspec/project.md` and capability specs under `openspec/specs/` exist for at least 6 core subsystems (`demuxer`, `codecs`, `av-pipeline`, `sync-engine`, `smart-chat`, `ui-renderer`), each containing formal `### Requirement:` blocks and `#### Scenario:` Given/When/Then verification cases.
- [ ] `openspec/changes/` contains an ordered, dependency-graphed multi-phase implementation roadmap breaking the from-scratch player and sync engine into agent-executable milestones.
- [ ] `AGENTS.md` and `.pi/` directory exist with concrete agent skills, prompts, and workflows for spec-driven implementation, bitstream testing, and sync simulation.

### Pure From-Scratch Integrity & Compilation
- [ ] `cargo check --workspace` and `cargo test --workspace` exit with code 0 with zero warnings.
- [ ] Programmatic inspection of `Cargo.toml` and `Cargo.lock` across the workspace confirms zero bindings or dependencies on external media/codec/database engines (`ffmpeg`, `libav*`, `gstreamer`, `mpv`, `vlc`, `symphonia`, `dav1d`, `sqlite`, `rusqlite`, `sqlx`).

### Mathematical & Architectural Proof Tests
- [ ] Automated unit tests execute and pass for the high-RTT synchronization math: across a simulated 250–400ms RTT link with ±30ms jitter, the NTP clock filter + PLL speed controller converges media phase error to `< 15ms` without issuing a hard seek for drifts under `1.5s`, and barrier-sync unpause aligns playback start across simulated peers within `< 10ms`.
- [ ] Automated unit tests execute and pass for foundational binary container parser primitives (EBML variable-length integer & element IDs, MP4/ISOBMFF box headers) against synthetic test vectors generated in-tree.
- [ ] Automated unit tests execute and pass for the custom smart-chat subsystem: persisting timestamp-anchored messages to the custom binary log, executing instant full-text search via the custom index, exporting history, and parsing/dispatching `/seek`, `/bookmark`, `/clip`, `/poll`, and `/sync-status` commands.
- [ ] `scripts/verify_phase1.sh` runs all checks end-to-end and exits 0.

## 2026-10-25T09:09:06Z

Use a very large team of agents (deploy 35 subagents) to implement Phase 2 of Syncplay-Alt: the complete, high-efficiency standalone media player and long-distance synchronized watch-party platform in pure Rust with zero external media, codec, or database frameworks, governed by the OpenSpec suite and Pi framework Intent Verification Harness.

Working directory: /home/gautham/syncplay-alt
Integrity mode: demo
Requested team: Deploy 35 subagents (very large team of agents)

## Requirements

### R1. Full Container Demuxing & Packet Processing (`001-demuxer-bitstream`)
Implement end-to-end container demuxing for Matroska (MKV/WebM) and ISOBMFF (MP4/MOV) in pure safe Rust under `crates/syncplay-demux`:
- Extract sequential audio and video `DemuxPacket` bitstream payloads with validated presentation timestamps (PTS/DTS).
- Implement keyframe index lookup and random-access seeking (`seek_to(pts)`).
- Handle multi-track stream parsing (video, audio, timed text).

### R2. Video & Audio Presentation Engine (`002-codecs-presentation`, `003-clock-pipeline`, `006-ui-osd-renderer`)
Flesh out the presentation pipeline across `crates/syncplay-codecs`, `crates/syncplay-clock`, and `crates/syncplay-ui`:
- High-throughput Q8 fixed-point integer color space conversion ($Y'CbCr \to R'G'B'$) for BT.601 and BT.709 with 16-byte SIMD row alignment.
- Lock-free 128-byte cache-aligned SPSC ring buffer for inter-thread audio/video sample transfers.
- Master monotonic clock continuous presentation loop driving playback and OSD state updates.
- 10Hz scrubber debouncer and memory-bounded OSD state rendering ($\le 15\text{MB}$ UI RAM budget).

### R3. Real-Time High-RTT Network Watch-Party Transport (`004-network-sync-protocol`)
Build the real-time networking engine in `crates/syncplay-sync`:
- Real UDP peer-to-peer and client-server datagram transport.
- RFC 5905 NTP 4-timestamp probe exchange maintaining continuous clock offset ($\hat{\theta}$) and RTT tracking with $D_{\text{effective}} \ge 5.0\text{ms}$ floor.
- Critically damped discrete PI PLL micro-speed controller ($K_p = 0.17, K_i = 0.0072$) maintaining $|e| < 15\text{ms}$ phase alignment across $250\text{--}400\text{ms}$ jittery WAN links.
- Two-phase barrier unpause protocol ensuring cross-peer playback resume alignment $< 10\text{ms}$.

### R4. Local Binary Smart Chat & Media Slash-Commands (`005-smart-chat-storage`)
Finalize the smart chat engine in `crates/syncplay-chat`:
- Append-only binary log `syncplay_chat.bin` with `0xAA55` framing, dual timestamps ($t_{wall}, t_{media}$), UUID sender tracking, and IEEE 802.3 CRC32 checksums.
- Crash-recovery forward scanner recovering from corrupted bytes and false-sync candidate patterns without data loss.
- In-memory inverted index FTS with Unicode case folding, CJK unigram/bigram segmentation, and single ideograph indexing.
- Interactive media slash-command parser and dispatcher (`/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`).

### R5. Pi Framework Intent Verification Harness & Zero-Drift Defense
Guarantee that all implementations strictly honor architectural invariants:
- Zero forbidden external multimedia or database engines (`ffmpeg`, `vlc`, `mpv`, `sqlite`, etc.), enforced by `./scripts/audit_dependencies.sh`.
- Zero compiler warnings and zero clippy warnings (`cargo check --workspace --all-targets` and `cargo clippy --workspace --all-targets -- -D warnings`).
- 100% compliance across all 8 intents in `./scripts/verify_intent.sh` (`python3 scripts/intent_harness.py`).
- Full end-to-end passing of `./scripts/verify_phase1.sh`.

## Acceptance Criteria

### Workspace Compilation & Zero Warnings
- [ ] `cargo check --workspace --all-targets` exits 0 with 0 compiler warnings.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` exits 0 with 0 warnings.
- [ ] Mandatory public documentation (`#![deny(missing_docs)]`) and safe Rust standards (`#![deny(unsafe_code)]` outside justified SIMD/SPSC primitives).

### Zero-Dependency Invariant
- [ ] `./scripts/audit_dependencies.sh` exits 0 with zero banned dependencies detected in `Cargo.toml`, `Cargo.lock`, and build scripts.

### Test & Intent Harness Verification
- [ ] All unit, integration, and adversarial tests pass cleanly (`cargo test --workspace`).
- [ ] Pi Framework Intent Verification Harness (`./scripts/verify_intent.sh`) executes and achieves 100.0% Intent Compliance Score with 0 drift.
- [ ] Full end-to-end verification script `./scripts/verify_phase1.sh` passes all 8 stages cleanly.

### Executable Binary Functionality (`syncplay-alt`)
- [ ] `cargo run -p syncplay-core -- inspect <file>` successfully demuxes real/synthetic MKV and MP4 files.
- [ ] `cargo run -p syncplay-core -- play <file>` executes presentation loop with master clock and OSD updates.
- [ ] `cargo run -p syncplay-core -- party [host|connect]` runs network watch-party synchronization.
- [ ] `cargo run -p syncplay-core -- chat demo` appends framed records, runs FTS, and exports transcripts.
- [ ] `cargo run -p syncplay-core -- verify` runs the Pi Intent Harness.


## 2026-10-25T15:21:44Z

Deploy a very large team of agents (35+ subagents) to implement Phase 3 of Syncplay-Alt: transform the engine into a VLC-grade standalone interactive media player and synchronized watch-party desktop application for Fedora Linux in pure, idiomatic Rust with zero external multimedia, codec, or database frameworks, governed by the OpenSpec suite and Pi Framework Intent Verification Harness.

Working directory: /home/gautham/syncplay-alt
Integrity mode: demo

## Requirements

### R1. Fedora Desktop Application & Interactive Player Launcher
Transform `syncplay-alt` into a fully accessible standalone application that Gautham can launch directly from the GNOME application grid or CLI:
- Create `~/.local/share/applications/syncplay-alt.desktop` with proper Exec bindings, MIME types (`video/x-matroska`, `video/mp4`, `video/webm`, `video/quicktime`), and modern icon integration.
- Implement an interactive terminal/OSD presentation mode (`syncplay-alt play <FILE>`) featuring a live, 60fps responsive presentation loop:
  * Full keyboard navigation: `Space` (Pause/Resume with barrier sync), `Left`/`Right` (Keyframe seek ±5s), `Shift+Left`/`Right` (Fine seek ±1s), `[` / `]` (Continuous speed shift ±0.1x via SeqLock), `M` (Mute/Unmute), `Up`/`Down` (Volume ±5%), `T`/`C` (Toggle floating chat HUD), `S` (Toggle sync diagnostics HUD), `/` (Interactive slash-command bar), `Q`/`Esc` (Clean exit).
  * Sleek terminal aesthetic OSD with ANSI 24-bit TrueColor / box-drawing / half-block video rendering, real-time scrubber progress bar, timecode HUD (`format_timecode_hud`), tri-color sync health badge, and floating bullet comments.

### R2. Pure-Rust Audio Output & Resampling Engine
Connect `syncplay-codecs` `AudioFramePCM` and `syncplay-clock` 128-byte cache-line aligned SPSC ring buffer to audible playback:
- Zero-external-dependency audio sink for Linux (native ALSA/sound PCM interface or unbuffered raw PCM audio pipe to standard output/sound server).
- Audio clock pacing synchronized with `MasterClock` presentation time.
- Zero-allocation audio buffer underrun protection and linear sample rate conversion (e.g. 44.1 kHz <-> 48 kHz).

### R3. Pure-Rust Video & Audio Bitstream Decoding Engine
Extend `syncplay-demux` and `syncplay-codecs` with pure safe Rust bitstream decoders (zero C/C++ or external crates):
- H.264 / AVC bitstream parser: Annex B NAL unit parsing (`0x000001` / `0x00000001` start codes), SPS/PPS parameter set decoding (profile, level, picture dimensions, crop offsets), slice header parsing, and frame reconstruction into planar `VideoFrameYUV420p`.
- PCM and Vorbis/Opus packet demuxer and payload unpacker yielding standard interleaved or planar `AudioFramePCM` float samples.
- High-performance Q8 fixed-point integer color space converter ($Y'CbCr \to R'G'B'$) outputting to 32-bit packed RGBA and terminal truecolor rendering buffers.

### R4. Full Interactive Multi-Peer Watch-Party Networking
Complete the distributed watch-party networking capabilities:
- `syncplay-alt party host --port <PORT> --file <MEDIA>`: Interactive host server broadcasting room state, answering NTP 4-timestamp probes, and orchestrating barrier unpause agreements.
- `syncplay-alt party connect <HOST:PORT> --file <MEDIA>`: Client peer connecting to host, computing clock offset $\hat{\theta}$ and dispersion-filtered RTT, driving discrete PI PLL speed steering, and participating in barrier unpauses.
- Live in-band smart chat messaging: typed chat messages are framed, sent over UDP, persisted to `syncplay_chat.bin`, indexed in the inverted index, and displayed as floating on-screen comments in real time.

### R5. Pi Framework Intent Verification Harness & Zero-Drift Defense
Guarantee that all changes strictly comply with workspace invariants:
- Zero banned dependencies (`./scripts/audit_dependencies.sh`).
- Zero compiler warnings and clippy warnings (`cargo check --workspace --all-targets` and `cargo clippy --workspace --all-targets -- -D warnings`).
- 100.0% Intent Compliance score in `./scripts/verify_intent.sh`.
- Full end-to-end verification passing across all empirical challenge test suites.

## Acceptance Criteria

### Desktop Integration & Usability
- [ ] `syncplay-alt.desktop` installed to `~/.local/share/applications/` and validated.
- [ ] Running `syncplay-alt play <media>` launches interactive real-time player with responsive keyboard controls (`Space`, arrows, `[`, `]`, `T`, `S`, `/`, `Q`).
- [ ] Running `syncplay-alt party host` and `syncplay-alt party connect` demonstrates real UDP multi-peer synchronization.

### Audio & Video Presentation
- [ ] Audible or verified PCM audio frame generation with zero buffer underruns.
- [ ] Planar YUV420p -> RGBA conversion with 16-byte SIMD stride and Q8 fixed-point arithmetic verified.
- [ ] Master Clock continuous rate shifts maintain mathematical $C^0$ continuity during playback adjustments.

### Smart Chat & Persistence
- [ ] Crash-resilient `syncplay_chat.bin` binary log persists and recovers messages with `0xAA55` framing and CRC32 checks.
- [ ] Slash commands (`/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`) dispatch and update playback state.

### Quality, Lints & Intent Compliance
- [ ] `./scripts/audit_dependencies.sh` exits 0 with zero banned dependencies.
- [ ] `cargo test --workspace` passes 100% of unit and integration tests.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` exits 0 with 0 warnings.
- [ ] `python3 scripts/intent_harness.py` exits 0 with 100.0% compliance.

## 2026-10-25T20:12:42Z

Use a very large team of agents. Transform Syncplay-Alt into a flawless, user-friendly desktop media player and synchronized watch-party platform with a VLC-grade graphical UI and universal video/audio format compatibility.

Working directory: /home/gautham/syncplay-alt
Integrity mode: development

## Requirements

### R1. Flawless Desktop Graphical Player UI
Provide a modern, responsive, user-friendly graphical desktop application window (supporting Linux Wayland/X11 and Windows):
- Window management: Native desktop window with title bar, minimize/maximize, window resizing, clean close, and smooth fullscreen toggle (F11 / double-click).
- Video viewport: GPU-accelerated video rendering surface preserving aspect ratio, letterboxing/pillarboxing, and smooth vsync frame pacing.
- Playback controls: Intuitive bottom control bar with Play/Pause, Stop, interactive seekbar / timeline slider with mouse hover timestamp preview and smooth drag-scrubbing, volume slider with mute toggle, timecode display (HH:MM:SS / HH:MM:SS), and playback speed selector.
- In-player watch-party sidebar / overlay: Toggleable sidebar displaying room members, connection RTT, sync health indicator, and real-time smart chat with text input for chat and slash-commands (/seek, /clip, /poll).
- File management: File open dialog, drag-and-drop file loading directly into the window, and recent media history.

### R2. Full VLC-Level Video & Audio Compatibility
Integrate comprehensive media decoding and demuxing to play all standard and modern media formats:
- Containers: Matroska (.mkv, .webm), MP4 / MOV (.mp4, .m4v, .mov), AVI (.avi), MPEG-TS (.ts), FLV (.flv), OGG (.ogg, .ogv).
- Video Codecs: H.264 (AVC), H.265 (HEVC), VP8, VP9, AV1, MPEG-2, MPEG-4, VC-1.
- Audio Codecs: AAC, MP3, Opus, Vorbis, FLAC, AC3/E-AC3, DTS, uncompressed PCM.
- Connect decoded video frames and audio samples to the existing MasterClock, discrete PI PLL synchronization engine, and audio output sinks for synchronized watch-party playback.

### R3. Comprehensive Documentation & Specification Realignment
Revise and update all repository documentation and verification specifications to reflect the new architecture:
- Update AGENTS.md to remove obsolete prohibitions against multimedia backends and define the new GUI and codec standards.
- Update openspec/project.md and capability specifications in openspec/specs/ to document the graphical UI architecture, windowing lifecycle, and codec integration.
- Update scripts/audit_dependencies.sh and the Pi Intent Harness (scripts/intent_harness.py) to align dependency checks with the updated design.
- Ensure all automated unit and integration tests compile with zero warnings and pass cleanly.

## Acceptance Criteria

### Graphical UI & User Experience
- [ ] Launching syncplay-alt without arguments or clicking the desktop entry opens a polished graphical desktop window (not a terminal instance).
- [ ] Dropping a video file or selecting via file picker begins playback with correct aspect ratio and smooth rendering.
- [ ] Hovering over the seekbar shows timestamp tooltip; clicking or dragging scrubs the timeline smoothly.
- [ ] Spacebar toggles play/pause; arrow keys seek; volume slider and hotkeys adjust audio smoothly.
- [ ] Watch-party chat panel allows typing and viewing messages with real-time room updates.

### Video & Audio Playback
- [ ] Plays MKV, MP4, WebM, and AVI files containing H.264, H.265, VP9, and AV1 video streams.
- [ ] Outputs synchronized audio across AAC, MP3, Opus, and Vorbis streams without underrun or crackling.
- [ ] Watch-party network synchronization (NTP filter, PI PLL speed nudging, barrier unpause) functions accurately with the graphical player.

### Build & Verification
- [ ] Workspace compiles with zero compiler warnings (cargo check --workspace --all-targets).
- [ ] All tests pass under cargo test --workspace.
- [ ] Updated dependency audit and intent verification scripts succeed without errors.
