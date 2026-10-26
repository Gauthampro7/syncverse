# BRIEFING — 2026-10-25T21:41:00Z

## Mission
Implement Universal Video & Audio Compatibility Engine (Milestone 2 / R2) covering UniversalDemuxer, UniversalVideoDecoder, UniversalAudioDecoder, and PlayerCoordinator integration.

## 🔒 My Identity
- Archetype: worker
- Roles: implementer, qa, specialist
- Working directory: /home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m2
- Original parent: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
- Milestone: Milestone 2 (Universal Video & Audio Compatibility Engine)

## 🔒 Key Constraints
- Zero warnings policy: `#![deny(warnings)]`, cargo check and clippy must have 0 warnings.
- Mandatory documentation: `#![deny(missing_docs)]` on public items.
- Strongly typed errors with `thiserror`.
- Forbidden frameworks: NO mpv, vlc, sqlite, rocksdb, etc. Only ffmpeg-next / symphonia / cpal.
- Strict memory alignment: YUV420p 64-byte aligned planar buffers, 16-byte row strides ((width + 15) & !15), +16B SIMD tail padding.
- Audio: 48kHz stereo f32 in [-1.0, 1.0], emitting AudioFramePCM, AudioPacer underrun handling.
- Preserve existing EbmlDemuxer, IsoBmffDemuxer, and unit tests.
- DO NOT CHEAT: Genuine logic, real behavior, no facade/dummy implementations.

## Current Parent
- Conversation ID: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
- Updated: 2026-10-25T21:35:13Z

## Task Summary
- **What to build**: UniversalDemuxer (syncplay-demux), UniversalVideoDecoder (syncplay-codecs), UniversalAudioDecoder (syncplay-codecs), Coordinator & Playback Integration (syncplay-core).
- **Success criteria**: cargo check, clippy, test all pass. audit_dependencies clean. intent_harness 100%. verify_phase4.sh 8 stages pass.
- **Interface contracts**: /home/gautham/syncplay-alt/.agents/teamwork/orchestrator_4/PROJECT.md
- **Code layout**: crates/syncplay-{demux, codecs, clock, sync, chat, ui, core}

## Key Decisions Made
- Added `ffmpeg-next = "9.0.0"` to workspace root and member crates `syncplay-demux`, `syncplay-codecs`, `syncplay-core`.
- Preserved `EbmlDemuxer` and `IsoBmffDemuxer` for in-tree synthetic vector tests while delegating multi-container production parsing to `UniversalDemuxer`.
- Implemented `UniversalVideoDecoder` with 64-byte aligned plane buffers, 16-byte aligned row strides, and +16B SIMD tail padding.
- Implemented `UniversalAudioDecoder` standardizing all audio bitstreams to 48kHz stereo f32 PCM in `[-1.0, 1.0]` with empty layout normalization and delay-line flushing.
- Updated `PlayerCoordinator` and `run_presentation_pipeline` to leverage `SpscRingBuffer<VideoFrameRGBA, 32>`, `SpscRingBuffer<AudioFramePCM, 64>`, `AudioPacer`, and `AutoAudioSink`.
- Added `play_universal_media` supporting end-to-end demuxing, decoding, ring-buffering, and audio-paced rendering on real media containers.

## Artifact Index
- /home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m2/DISPATCH.md — Dispatch instructions
- /home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m2/progress.md — Progress heartbeat
- /home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m2/handoff.md — Final handoff report

## Change Tracker
- **Files modified**:
  - `Cargo.toml`: Added `ffmpeg-next = "9.0.0"` to `workspace.dependencies`.
  - `crates/syncplay-demux/Cargo.toml`: Added `ffmpeg-next = { workspace = true }`.
  - `crates/syncplay-demux/src/universal.rs`: Created `UniversalDemuxer`.
  - `crates/syncplay-demux/src/lib.rs`: Exported `UniversalDemuxer`.
  - `crates/syncplay-demux/tests/universal_demux_tests.rs`: Added 4 tests across MKV, WebM, MP4, MOV, AVI, TS, FLV, OGG.
  - `crates/syncplay-codecs/Cargo.toml`: Added `ffmpeg-next = { workspace = true }`.
  - `crates/syncplay-codecs/src/error.rs`: Added `Decoder` and `UnsupportedCodec` variants and `From<ffmpeg_next::Error>`.
  - `crates/syncplay-codecs/src/universal_video.rs`: Created `UniversalVideoDecoder` with strict memory alignments.
  - `crates/syncplay-codecs/src/audio/universal.rs`: Created `UniversalAudioDecoder` with libswresample resampling.
  - `crates/syncplay-codecs/src/audio/mod.rs`: Exported `UniversalAudioDecoder` and `UniversalAudioError`.
  - `crates/syncplay-codecs/src/lib.rs`: Re-exported video and audio decoders.
  - `crates/syncplay-core/Cargo.toml`: Added `ffmpeg-next = { workspace = true }`.
  - `crates/syncplay-core/src/error.rs`: Implemented `From<UniversalAudioError> for CoreError`.
  - `crates/syncplay-core/src/coordinator.rs`: Integrated `UniversalDemuxer`, `UniversalVideoDecoder`, `UniversalAudioDecoder`, `SpscRingBuffer<VideoFrameRGBA, 32>`, `SpscRingBuffer<AudioFramePCM, 64>`, `AudioPacer`, `AutoAudioSink`, and added `play_universal_media`.
- **Build status**: Pass (0 errors, 0 warnings across workspace)
- **Pending issues**: None

## Quality Status
- **Build/test result**: All unit and integration tests passing (`cargo test --workspace`).
- **Lint status**: 0 warnings under `cargo clippy --workspace --all-targets -- -D warnings`.
- **Tests added/modified**: `crates/syncplay-demux/tests/universal_demux_tests.rs`, `syncplay-codecs/src/universal_video.rs` (unit tests), `syncplay-codecs/src/audio/universal.rs` (unit tests), `syncplay-core/src/coordinator.rs` (`test_coordinator_play_universal_media`).

## Loaded Skills
- None requested in prompt.
