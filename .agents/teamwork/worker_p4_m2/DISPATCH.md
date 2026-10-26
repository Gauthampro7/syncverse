# DISPATCH — worker_p4_m2

## Mission
You are `worker_p4_m2`, the implementation worker for Milestone 2: Universal Video & Audio Compatibility Engine (R2).
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m2`.

You must read:
1. `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically `## 2026-10-25T20:12:42Z`)
2. `/home/gautham/syncplay-alt/.agents/teamwork/orchestrator_4/PROJECT.md`
3. Explorer handoffs and prototypes:
   - `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_1/handoff.md` and `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_1/proposed_universal.rs`
   - `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_2/handoff.md` and `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_2/proposed_universal_video.rs`
   - `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_3/handoff.md` and `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_3/universal_audio_prototype.rs`

## Mandatory Integrity Warning
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## Tasks & Implementation Steps
1. **Universal Demuxer (`syncplay-demux`)**:
   - Add `ffmpeg-next = "9.0.0"` to `crates/syncplay-demux/Cargo.toml` and workspace `Cargo.toml`.
   - Implement `UniversalDemuxer` in `crates/syncplay-demux/src/universal.rs` from `explorer_p4_m2_1/proposed_universal.rs`.
   - Export `UniversalDemuxer` in `crates/syncplay-demux/src/lib.rs`.
   - Preserve existing `EbmlDemuxer` and `IsoBmffDemuxer` and all existing unit tests.
2. **Universal Video Decoder (`syncplay-codecs`)**:
   - Add `ffmpeg-next = "9.0.0"` to `crates/syncplay-codecs/Cargo.toml`.
   - Implement `UniversalVideoDecoder` in `crates/syncplay-codecs/src/universal_video.rs` (or `video/universal.rs`) from `explorer_p4_m2_2/proposed_universal_video.rs`.
   - Export `UniversalVideoDecoder` in `crates/syncplay-codecs/src/lib.rs`.
   - Ensure strict memory alignment invariants: 64-byte aligned planar buffers, 16-byte aligned row strides (`(width + 15) & !15`), +16B SIMD tail padding.
3. **Universal Audio Decoder & Resampler (`syncplay-codecs`)**:
   - Implement `UniversalAudioDecoder` in `crates/syncplay-codecs/src/audio/universal.rs` from `explorer_p4_m2_3/universal_audio_prototype.rs`.
   - Export `UniversalAudioDecoder` in `crates/syncplay-codecs/src/lib.rs`.
   - Ensure real-time resampling via `libswresample` to standardized 48kHz stereo `f32` in `[-1.0, 1.0]`, emitting `AudioFramePCM`.
4. **Integration in `syncplay-core`**:
   - Update `PlayerCoordinator` and playback loop in `crates/syncplay-core/src/coordinator.rs` to leverage `UniversalDemuxer`, `UniversalVideoDecoder`, and `UniversalAudioDecoder` for media playback.
   - Connect decoded video frames to `SpscRingBuffer<VideoFrameRGBA, 32>` and audio frames to `SpscRingBuffer<AudioFramePCM, 64>`.
   - Connect to `AudioPacer` and `AudioSink`.
5. **Run Full Verification Suite**:
   - `cargo check --workspace --all-targets` (must pass with 0 warnings)
   - `cargo clippy --workspace --all-targets -- -D warnings` (must pass with 0 warnings)
   - `cargo test --workspace` (all tests pass)
   - `./scripts/audit_dependencies.sh` (clean)
   - `python3 scripts/intent_harness.py` (100.0% compliance score)
   - `./scripts/verify_phase4.sh` (all 8 stages pass cleanly)

## Output Requirements
Document all actions taken, files modified, and exact command outputs in `/home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m2/handoff.md`.
Send a completion message to the orchestrator when finished.


## 2026-10-25T21:24:40Z
[Message] timestamp=2026-10-25T21:24:40Z sender=ee88cac4-07d4-4dad-bc7e-4c3f606ef03f priority=MESSAGE_PRIORITY_HIGH content=You are worker_p4_m2.
Tasks:
1. Universal Demuxer (syncplay-demux):
   - Add ffmpeg-next = "9.0.0" to crates/syncplay-demux/Cargo.toml and root Cargo.toml.
   - Implement UniversalDemuxer in crates/syncplay-demux/src/universal.rs from explorer_p4_m2_1/proposed_universal.rs.
   - Export UniversalDemuxer in crates/syncplay-demux/src/lib.rs.
   - Preserve existing EbmlDemuxer and IsoBmffDemuxer and existing unit tests.
2. Universal Video Decoder (syncplay-codecs):
   - Add ffmpeg-next = "9.0.0" to crates/syncplay-codecs/Cargo.toml.
   - Implement UniversalVideoDecoder in crates/syncplay-codecs/src/universal_video.rs from explorer_p4_m2_2/proposed_universal_video.rs.
   - Export UniversalVideoDecoder in crates/syncplay-codecs/src/lib.rs.
   - Ensure strict memory alignment: 64B aligned planar buffers, 16B row strides ((width + 15) & !15), +16B SIMD tail padding.
3. Universal Audio Decoder & Resampler (syncplay-codecs):
   - Implement UniversalAudioDecoder in crates/syncplay-codecs/src/audio/universal.rs from explorer_p4_m2_3/universal_audio_prototype.rs.
   - Export UniversalAudioDecoder in crates/syncplay-codecs/src/lib.rs.
   - Resample via libswresample to standard 48kHz stereo f32 in [-1.0, 1.0], emitting AudioFramePCM.
4. Coordinator & Playback Integration (syncplay-core):
   - Add UniversalDemuxer, UniversalVideoDecoder, UniversalAudioDecoder to syncplay-core/Cargo.toml and crates/syncplay-core/src/coordinator.rs.
   - Connect decoded video frames to SpscRingBuffer<VideoFrameRGBA, 32> and audio frames to SpscRingBuffer<AudioFramePCM, 64>, AudioPacer, and AudioSink.
5. Verification Suite:
   - cargo check --workspace --all-targets (0 warnings)
   - cargo clippy --workspace --all-targets -- -D warnings (0 warnings)
   - cargo test --workspace (all tests pass)
   - ./scripts/audit_dependencies.sh (clean)
   - python3 scripts/intent_harness.py (100.0% compliance)
   - ./scripts/verify_phase4.sh (all 8 stages pass)


## 2026-10-25T21:35:13Z
**Context**: Milestone 2 Implementation
**Content**: Status check on `worker_p4_m2`. `universal.rs`, `universal_video.rs`, and `audio/universal.rs` are in place. Please report on your current verification progress (`cargo check`, `cargo test`, `cargo clippy`, `verify_phase4.sh`).
**Action**: Update `progress.md` and report when verification is complete.
