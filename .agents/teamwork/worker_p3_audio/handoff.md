# Handoff Report: Milestone 2 — Pure-Rust Audio Output & Resampling Engine

**Agent**: `worker_p3_audio`  
**Role**: Implementation Worker (implementer, qa)  
**Date**: 2026-10-25T16:55:00Z  
**Recipient**: `orchestrator_3` (`ecd27bad-3488-46aa-ba26-8b0b7cecf0eb`)  
**Working Directory**: `/home/gautham/syncplay-alt/.agents/teamwork/worker_p3_audio`  
**Milestone**: Milestone 2: Requirement R2 (Pure-Rust Audio Output & Resampling Engine)

---

## 1. Observation

1. **Dependency Audit Invariant Compliance**:
   - Inspected `scripts/audit_dependencies.sh` enforcing zero foreign codec/media/database crates.
   - Executed `./scripts/audit_dependencies.sh`:
     ```
     [INFO] Auditing Cargo.toml manifests for banned dependencies...
     [INFO] Auditing Cargo.lock for banned dependencies...
     [PASS] Dependency audit clean. Zero forbidden external engines detected.
     ```

2. **Component Implementation**:
   - `crates/syncplay-codecs/src/audio/resampler.rs` (340 lines): Implemented pure-Rust `LinearResampler` with exact fractional phase accumulation (`step = (in_rate as f64) / ((out_rate as f64) * speed_ratio)`), continuous phase tracking across arbitrary chunk splits, micro-speed steering ratio $r \in [0.96, 1.04]$, multi-channel support, `resample_into(&self, input: &[f32], output: &mut Vec<f32>) -> Result<usize, CodecError>`, `resample(&self, input: &[f32]) -> Result<Vec<f32>, CodecError>`, and `resample_frame(&self, frame: &AudioFramePCM) -> Result<AudioFramePCM, CodecError>`.
   - `crates/syncplay-codecs/src/audio/mod.rs` (15 lines) & `crates/syncplay-codecs/src/lib.rs` (27 lines): Re-exported `LinearResampler`.
   - `crates/syncplay-clock/Cargo.toml` (13 lines) & `crates/syncplay-clock/src/error.rs` (39 lines): Added intra-workspace dependency on `syncplay-codecs` and error variants `AudioPacing(String)` and `Codec(syncplay_codecs::CodecError)`.
   - `crates/syncplay-clock/src/audio_pacer.rs` (365 lines): Implemented `AudioPacer`, `AudioPacerStats`, and `AudioTimingAction` (`PlayNow`, `Wait`, `Late`). Synchronized audio presentation against `MasterClock` presentation time. Implemented buffer underrun protection via pre-allocated silence scratch buffer transferred over `SpscRingBuffer<AudioFramePCM, 64>`, zero-allocation `fill_samples`, producer-side `inject_silence_frame`, and smooth 2ms linear ramp-in recovery on first frame after underrun.
   - `crates/syncplay-clock/src/lib.rs` (17 lines): Re-exported `AudioPacer`, `AudioPacerStats`, and `AudioTimingAction`.
   - `crates/syncplay-core/src/audio_sink.rs` (380 lines): Implemented `AudioSink` trait (`write_samples`, `flush`, `sample_rate`, `channels`, `pause`, `resume`), `PipeAudioSink` streaming IEEE 754 32-bit float PCM (`f32`, 48 kHz stereo) to `/usr/bin/pw-cat` (PipeWire on Fedora 44) with fallback to `/usr/bin/aplay` (ALSA), `NullAudioSink` for headless/CI environments, `AutoAudioSink` with auto-detection and fallback, and `MockAudioSink` for test capture.
   - `crates/syncplay-core/src/lib.rs` (17 lines): Re-exported audio sink types.

3. **Test Enhancements**:
   - `crates/syncplay-codecs/tests/audio_resampler_tests.rs` (203 lines): 6 comprehensive integration tests validating 44.1 kHz <-> 48 kHz sine tone accuracy, arbitrary prime/irregular chunk splitting phase continuity, continuous micro-speed steering across $r \in [0.96, 1.04]$, zero-allocation capacity retention in hot loop, `AudioFramePCM` resampling, and adversarial input validation.
   - `crates/syncplay-core/tests/audio_pipeline_and_underrun_tests.rs` (206 lines): 4 comprehensive integration tests validating multi-tier `AudioSink` implementations, audio clock pacing decision windows, multithreaded SPSC audio starvation and silence underrun recovery with smooth ramp-in, and end-to-end 44.1 kHz -> 48 kHz resampling into `MasterClock`-paced `AudioSink`.

4. **Verification Outputs**:
   - `cargo check --workspace --all-targets`: Passed with 0 warnings.
   - `cargo clippy --workspace --all-targets -- -D warnings`: Passed with 0 warnings.
   - `cargo test --workspace`: 100% of unit and integration tests passed.
   - `python3 scripts/intent_harness.py`: 8/8 intents passed, 100.0% compliance score.
   - `./scripts/verify_phase1.sh`: All 8 stages passed cleanly.

---

## 2. Logic Chain

1. **Pure-Rust Resampling & Phase Continuity**:
   - Different audio sources deliver varying sample rates (e.g. 44.1 kHz, 48.0 kHz). Because modern sound daemons on Linux (PipeWire / ALSA) expect 48.0 kHz stereo float PCM, an in-tree resampler is required.
   - Naive per-chunk resampling discards fractional phase remainders at chunk boundaries, causing high-frequency clicks and phase drift.
   - `LinearResampler` tracks `phase: f64` and caches `prev_frame: Vec<f32>` across chunks. It maps chunk samples such that boundary samples interpolate smoothly between the trailing frame of chunk $K$ and the leading frame of chunk $K+1$.
   - Observation 3 confirms `test_arbitrary_chunk_splitting_phase_continuity` yields identical sample values ($< 10^{-4}$ deviation) regardless of whether input is processed in a single chunk or split across irregular prime chunks (31, 127, 256, 503, 1024).
   - In `resample_into`, `output.clear()` followed by `output.reserve(...)` reuses existing capacity, ensuring zero heap allocations in the hot loop.

2. **Micro-Speed Steering Integration**:
   - When network clock synchronization (`syncplay-sync` PI PLL) requires speed adjustments ($r \in [0.96, 1.04]$), `resampler.set_speed_ratio(r)` dynamically adjusts `step = in_rate / (out_rate * r)`.
   - Observation 3 confirms `test_micro_speed_ratio_sweep` accurately adjusts consumption rates without timeline jitter or buffer corruption.

3. **Audio Clock Pacing & Underrun Defense**:
   - In a decoupled threading architecture, network jitter or decoding delays can cause the audio thread to starve (`SpscRingBuffer.try_pop() == None`).
   - If an audio sink starves, PipeWire/ALSA sound daemons emit pipe broken errors (`-EPIPE`), causing audible speaker pops or thread lockups.
   - `AudioPacer` catches buffer starvation, increments `underrun_count`, and supplies a pre-allocated silence scratch frame with timestamps anchored to `MasterClock.current_pts(now)`.
   - When valid audio resumes, `AudioPacer` detects the transition from underrun and applies a 2ms linear ramp-in on the leading samples, preventing discontinuous voltage steps and pops.
   - Observation 3 confirms `test_multithreaded_pipeline_with_underrun_starvation_and_recovery` recovers cleanly and maintains uninterrupted streaming.

4. **Multi-Tier Audio Sinks**:
   - `PipeAudioSink` uses `std::process::Command` to launch `/usr/bin/pw-cat` (primary PipeWire tool on Fedora 44) or `/usr/bin/aplay` (ALSA fallback) with `Stdio::piped()`, writing raw IEEE 754 32-bit float PCM bytes to standard input without requiring any external C audio libraries or crates.
   - `NullAudioSink` and `AutoAudioSink` ensure headless CI, automated tests, and minimal containers execute without requiring an active audio daemon or sound card.

---

## 3. Caveats

1. **System Tool Availability on Host**: `PipeAudioSink` relies on either `pw-cat` (PipeWire) or `aplay` (ALSA) installed on the Linux system. On Fedora 44 Workstation, both are standard system utilities. When running in minimal Docker containers or CI without either tool, `AutoAudioSink` transparently falls back to `NullAudioSink`.
2. **Resampling Algorithm**: Linear interpolation provides computationally light, zero-allocation sample rate conversion suitable for real-time video playback and micro-speed steering. For extreme downsampling (e.g. 192 kHz to 8 kHz), a sinc/polyphase filter with low-pass antialiasing could be added in a future phase if needed, but for standard 44.1 kHz <-> 48 kHz media playback, linear interpolation is optimal.

---

## 4. Conclusion

Milestone 2 (Requirement R2: Pure-Rust Audio Output & Resampling Engine) is complete and verified:
- `LinearResampler` provides seamless 44.1 kHz <-> 48.0 kHz sample rate conversion and micro-speed steering $r \in [0.96, 1.04]$ with zero hot-loop allocations and continuous phase tracking.
- `AudioPacer` synchronizes audio presentation with `MasterClock` and provides pre-allocated silence underrun protection with smooth 2ms linear ramp-in recovery.
- `PipeAudioSink`, `NullAudioSink`, and `AutoAudioSink` provide zero-external-dependency audio streaming to PipeWire (`pw-cat`) and ALSA (`aplay`) with headless fallback.
- All workspace checks, clippy lints, intent harness tests (100%), and end-to-end Phase 1 tests pass cleanly with zero warnings and zero foreign codec/media crates.

---

## 5. Verification Method

To independently reproduce and verify this milestone:

```bash
# 1. Ensure Rust compiler is on PATH
export PATH="$HOME/.cargo/bin:$PATH"

# 2. Verify zero forbidden multimedia/database engines
./scripts/audit_dependencies.sh

# 3. Verify workspace compilation with zero warnings
cargo check --workspace --all-targets

# 4. Verify strict Clippy linter with zero warnings
cargo clippy --workspace --all-targets -- -D warnings

# 5. Run the new resampler and audio pipeline integration test suites
cargo test -p syncplay-codecs --test audio_resampler_tests -- --nocapture
cargo test -p syncplay-core --test audio_pipeline_and_underrun_tests -- --nocapture

# 6. Run full workspace test suite
cargo test --workspace

# 7. Run Pi Framework Intent Verification Harness (100.0% compliance)
python3 scripts/intent_harness.py

# 8. Run end-to-end automated verification script
./scripts/verify_phase1.sh
```
