# BRIEFING — 2026-10-25T16:50:00Z

## Mission
Implement Milestone 2: Pure-Rust Audio Output & Resampling Engine (Requirement R2) for Phase 3 of Syncplay-Alt.

## 🔒 My Identity
- Archetype: worker
- Roles: implementer, qa
- Working directory: /home/gautham/syncplay-alt/.agents/teamwork/worker_p3_audio
- Original parent: ecd27bad-3488-46aa-ba26-8b0b7cecf0eb
- Milestone: Milestone 2: Pure-Rust Audio Output & Resampling Engine

## 🔒 Key Constraints
- Zero external codec/audio/database frameworks (no symphonia, hound, alsa-sys, etc.)
- Pure-Rust LinearResampler with fractional phase accumulation and speed steering r in [0.96, 1.04]
- Zero allocation in the hot loop (resample_into)
- AudioSink trait + PipeAudioSink (pw-cat / aplay) + NullAudioSink + AutoAudioSink
- Audio clock pacing synchronized with MasterClock and SpscRingBuffer<AudioFramePCM, 64>
- Pre-allocated silence scratch buffer underrun protection
- Zero compiler warnings (#![deny(warnings)]), zero clippy warnings, deny(missing_docs), deny(unsafe_code)
- 100% Intent compliance score

## Current Parent
- Conversation ID: ecd27bad-3488-46aa-ba26-8b0b7cecf0eb
- Updated: 2026-10-25T16:50:00Z

## Task Summary
- **What to build**: Pure-Rust Audio Output & Resampling Engine:
  1. `crates/syncplay-codecs/src/audio/resampler.rs`: Pure-Rust `LinearResampler` with fractional phase accumulation, speed ratio r in [0.96, 1.04], zero allocation in `resample_into`.
  2. `crates/syncplay-core/src/audio_sink.rs`: `AudioSink` trait, `PipeAudioSink`, `NullAudioSink`, `AutoAudioSink`, `MockAudioSink`.
  3. `crates/syncplay-clock/src/audio_pacer.rs`: Audio clock pacing with `MasterClock`, buffer underrun protection with pre-allocated silence scratch buffer transferred over `SpscRingBuffer<AudioFramePCM, 64>`.
  4. Comprehensive unit, integration, and underrun tests.
- **Success criteria**: All tests pass, intent harness passes 100%, zero warnings, clean audio playback & resampling.
- **Interface contracts**: AGENTS.md, openspec/, survey blueprints.
- **Code layout**: In-tree crates `syncplay-codecs`, `syncplay-core`, `syncplay-clock`.

## Key Decisions Made
- `LinearResampler`: exact linear interpolation with fractional phase tracking across chunk boundaries (`step = (in_rate as f64) / ((out_rate as f64) * speed_ratio)`), maintaining coordinate continuity without losing samples or accumulating drift.
- `AudioPacer`: synchronizes audio presentation with `MasterClock` using `AudioTimingAction` (`PlayNow`, `Wait`, `Late`). On buffer underrun, injects pre-allocated silence frames without thread stalls or memory allocation, and applies a smooth 2ms linear ramp-in upon stream recovery to eliminate pops/glitches.
- `PipeAudioSink`: streams IEEE 754 f32 little-endian PCM directly to `/usr/bin/pw-cat` (PipeWire) on Fedora 44 with automatic fallback to `/usr/bin/aplay` (ALSA).
- `AutoAudioSink`: probes `PipeAudioSink` and falls back cleanly to `NullAudioSink` in headless/CI environments.

## Artifact Index
- `.agents/teamwork/worker_p3_audio/DISPATCH.md` — Dispatch prompt and instructions
- `.agents/teamwork/worker_p3_audio/BRIEFING.md` — Persistent situational awareness
- `.agents/teamwork/worker_p3_audio/progress.md` — Liveness and progress heartbeat
- `.agents/teamwork/worker_p3_audio/handoff.md` — 5-component handoff report

## Change Tracker
- **Files modified**:
  - `crates/syncplay-codecs/src/audio/resampler.rs`: Pure-Rust `LinearResampler` with phase accumulation and micro-speed steering
  - `crates/syncplay-codecs/src/audio/mod.rs`: Exported `LinearResampler`
  - `crates/syncplay-codecs/src/lib.rs`: Re-exported `LinearResampler`
  - `crates/syncplay-codecs/tests/audio_resampler_tests.rs`: Comprehensive integration test suite for `LinearResampler`
  - `crates/syncplay-clock/Cargo.toml`: Added `syncplay-codecs` intra-workspace dependency
  - `crates/syncplay-clock/src/error.rs`: Added `AudioPacing` and `Codec` error variants
  - `crates/syncplay-clock/src/audio_pacer.rs`: `AudioPacer`, `AudioPacerStats`, `AudioTimingAction`, silence underrun protection
  - `crates/syncplay-clock/src/lib.rs`: Re-exported `AudioPacer`, `AudioPacerStats`, `AudioTimingAction`
  - `crates/syncplay-core/src/audio_sink.rs`: `AudioSink` trait, `PipeAudioSink`, `NullAudioSink`, `AutoAudioSink`, `MockAudioSink`
  - `crates/syncplay-core/src/lib.rs`: Re-exported audio sink types
  - `crates/syncplay-core/tests/audio_pipeline_and_underrun_tests.rs`: Integration tests for audio pipeline and underruns
- **Build status**: Pass (100% tests pass, zero warnings)
- **Pending issues**: None

## Quality Status
- **Build/test result**: Pass (Workspace tests pass, all 8 intent contracts satisfied)
- **Lint status**: 0 warnings across `cargo check` and `cargo clippy --all-targets -- -D warnings`
- **Tests added/modified**:
  - `crates/syncplay-codecs/src/audio/resampler.rs` unit tests (4 tests)
  - `crates/syncplay-codecs/tests/audio_resampler_tests.rs` (6 tests)
  - `crates/syncplay-clock/src/audio_pacer.rs` unit tests (4 tests)
  - `crates/syncplay-core/src/audio_sink.rs` unit tests (3 tests)
  - `crates/syncplay-core/tests/audio_pipeline_and_underrun_tests.rs` (4 tests)

## Loaded Skills
None
