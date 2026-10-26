# Dispatch: worker_p3_audio

## Identity
- Role: Implementation Worker (Milestone 2: Pure-Rust Audio Output & Resampling Engine)
- Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/worker_p3_audio
- Authoritative User Request: /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md
- Survey Architecture Blueprint: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_survey_1/survey_report.md
- Survey Handoff: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_survey_1/handoff.md

## Scope & Write Ownership
You exclusively own:
- `crates/syncplay-codecs/src/audio/resampler.rs` (Linear sample rate conversion with fractional phase accumulation and speed steering)
- `crates/syncplay-core/src/audio_sink.rs` (or `crates/syncplay-core/src/audio/`: `AudioSink` trait, `PipeAudioSink` streaming f32 PCM to `/usr/bin/pw-cat` / `/usr/bin/aplay`, `NullAudioSink` for headless CI)
- `crates/syncplay-clock/src/audio_pacer.rs` (Audio clock pacing and PTS synchronization with `MasterClock`)
- Integration with `SpscRingBuffer<AudioFramePCM, 64>` with zero-allocation silence scratch buffer underrun protection.
- Unit and integration tests in `crates/syncplay-core/tests/` and `crates/syncplay-codecs/tests/`.

## Mandatory Integrity Warning
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## Coding Standards & Invariants
1. Zero banned external dependencies (`./scripts/audit_dependencies.sh`).
2. Zero compiler warnings (`cargo check --workspace --all-targets`).
3. Zero clippy warnings (`cargo clippy --workspace --all-targets -- -D warnings`).
4. `#![deny(missing_docs)]` and `#![deny(unsafe_code)]`.
5. 100% Intent compliance (`python3 scripts/intent_harness.py`).
6. All tests pass (`cargo test --workspace`).

## Deliverables
Implement all Milestone 2 components, verify with builds and tests, write `handoff.md`, and notify orchestrator_3.

## 2026-10-25T16:31:37Z
[Message] timestamp=2026-10-25T16:31:37Z sender=ecd27bad-3488-46aa-ba26-8b0b7cecf0eb priority=MESSAGE_PRIORITY_HIGH
Task: Milestone 2 of Syncplay-Alt Phase 3 - Requirement R2: Pure-Rust Audio Output & Resampling Engine
- crates/syncplay-codecs/src/audio/resampler.rs: LinearResampler with fractional phase accumulation, speed ratio r in [0.96, 1.04], zero-allocation resample_into.
- crates/syncplay-core/src/audio_sink.rs: AudioSink trait, PipeAudioSink (pw-cat fallback aplay), NullAudioSink, AutoAudioSink.
- crates/syncplay-clock/src/audio_pacer.rs: Audio clock pacing synchronized with MasterClock, buffer underrun protection with pre-allocated silence scratch buffer.
- Comprehensive unit, integration, and underrun tests.
- Verification: scripts/audit_dependencies.sh, cargo check, cargo clippy, cargo test, python3 scripts/intent_harness.py.
