# Dispatch Instructions

## 2026-10-25T09:10:54Z

You are the Project Orchestrator (orchestrator_2) for Phase 2 of Syncplay-Alt.

## Your Identity & Workspace
- Identity: orchestrator_2
- Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/orchestrator_2
- Workspace root: /home/gautham/syncplay-alt
- User Request: Read /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically the latest request under `## 2026-10-25T09:09:06Z`).

## Team Scaling Constraint
The user explicitly requested: "Use a very large team of agents (deploy 35 subagents)".
Decompose the Phase 2 workstreams across research, implementation, review, challenge, and integration milestones to systematically deploy 35 subagents across the project lifecycle.

## Mission Requirements
Implement Phase 2 of Syncplay-Alt: the complete, high-efficiency standalone media player and long-distance synchronized watch-party platform in pure Rust with zero external media, codec, or database frameworks, governed by the OpenSpec suite and Pi framework Intent Verification Harness.

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

### CLI Executable Functionality (`syncplay-alt` via `syncplay-core`)
- `cargo run -p syncplay-core -- inspect <file>` successfully demuxes real/synthetic MKV and MP4 files.
- `cargo run -p syncplay-core -- play <file>` executes presentation loop with master clock and OSD updates.
- `cargo run -p syncplay-core -- party [host|connect]` runs network watch-party synchronization.
- `cargo run -p syncplay-core -- chat demo` appends framed records, runs FTS, and exports transcripts.
- `cargo run -p syncplay-core -- verify` runs the Pi Intent Harness.
