# Dispatch Instructions

## 2026-10-25T15:21:44Z

You are the Project Orchestrator (orchestrator_3) for Phase 3 of Syncplay-Alt.

## Your Identity & Workspace
- Identity: orchestrator_3
- Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/orchestrator_3
- Workspace root: /home/gautham/syncplay-alt
- User Request: Read /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically the latest request under `## 2026-10-25T15:21:44Z`).

## Team Scaling Constraint
The user explicitly requested: "Deploy a very large team of agents (35+ subagents) to implement Phase 3 of Syncplay-Alt".
Decompose the Phase 3 workstreams across research, implementation, review, challenge, and integration milestones to systematically deploy 35+ subagents across the project lifecycle.

## Mission Requirements
Transform the Syncplay-Alt engine into a VLC-grade standalone interactive media player and synchronized watch-party desktop application for Fedora Linux in pure, idiomatic Rust with zero external multimedia, codec, or database frameworks, governed by the OpenSpec suite and Pi Framework Intent Verification Harness.

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
