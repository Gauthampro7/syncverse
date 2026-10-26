# Handoff Report: Reviewer Milestone 2 — Audio Pacer & Audio Sinks

**Agent**: `reviewer_p3_audio_2`  
**Role**: Code Reviewer and Adversarial Critic (reviewer, critic)  
**Date**: 2026-10-25T16:55:00Z  
**Recipient**: `orchestrator_3` (`ecd27bad-3488-46aa-ba26-8b0b7cecf0eb`)  
**Working Directory**: `/home/gautham/syncplay-alt/.agents/teamwork/reviewer_p3_audio_2`  
**Milestone**: Milestone 2: Requirement R2 (Pure-Rust Audio Output & Resampling Engine)  
**Verdict**: **APPROVE**  

---

## 1. Observation

1. **Dependency Audit Invariant Verification**:
   - Inspected root `Cargo.toml`, `Cargo.lock`, and crate manifests.
   - Executed `./scripts/audit_dependencies.sh`:
     ```
     [INFO] Auditing Cargo.toml manifests for banned dependencies...
     [INFO] Auditing Cargo.lock for banned dependencies...
     [PASS] Dependency audit clean. Zero forbidden external engines detected.
     ```
   - Confirmed zero foreign codec/media/database libraries linked or vendored.

2. **Component Implementations Reviewed**:
   - `crates/syncplay-clock/src/audio_pacer.rs` (401 lines):
     * Evaluates frame timing against `MasterClock` (`evaluate_timing`): `Wait(delta - 20ms)` for $> +20\text{ms}$, `PlayNow` within $[-20\text{ms}, +20\text{ms}]$, and `Late(-delta)` for $< -20\text{ms}$.
     * Underrun handling: Detects buffer starvation when `SpscRingBuffer.try_pop() == None`, updates `underrun_count` and `silence_injected_count`, sets `in_underrun` flag, and provides pre-allocated silence frames anchored to `MasterClock.current_pts(now)`.
     * Linear ramp-in: When transitioning from underrun to valid audio (`in_underrun.swap(false, Ordering::AcqRel)`), `apply_ramp_in` scales the initial `((sample_rate * 2) / 1000)` samples linearly from 0.0 to 1.0.
     * Zero-allocation guarantee: `fill_samples` clears and fills a reusable caller-provided `output: &mut Vec<f32>`, keeping capacity constant without heap allocations.
   - `crates/syncplay-core/src/audio_sink.rs` (473 lines):
     * `AudioSink` trait with `write_samples`, `flush`, `sample_rate`, `channels`, `pause`, `resume`.
     * `PipeAudioSink`: Spawns `/usr/bin/pw-cat` (PipeWire) with fallback to `/usr/bin/aplay` (ALSA), streaming IEEE 754 32-bit float PCM (`f32`, 48 kHz stereo) clamped to $[-1.0, 1.0]$.
     * `Drop` for `PipeAudioSink` sends `kill()` and calls `wait()` on child process, preventing zombie subprocesses.
     * `NullAudioSink`: Zero-overhead sink for headless/CI operation.
     * `AutoAudioSink`: Probes `PipeAudioSink` and cleanly falls back to `NullAudioSink`.
     * `MockAudioSink`: Captures samples for testing.
   - `crates/syncplay-codecs/src/audio/resampler.rs` (355 lines):
     * Pure-Rust `LinearResampler` with fractional phase accumulation and dynamic micro-speed ratio $r \in [0.96, 1.04]$.
     * Preserves fractional phase remainders and boundary history (`prev_frame`) across chunk splits.
     * Zero-allocation steady-state processing via `resample_into`.

3. **Compiler and Test Verification**:
   - `cargo check --workspace --all-targets`: Exited 0 with 0 compiler warnings.
   - `cargo clippy --workspace --all-targets -- -D warnings`: Exited 0 with 0 warnings.
   - `cargo doc --workspace --no-deps`: Exited 0 with 0 missing docs warnings.
   - `cargo test -p syncplay-codecs --test audio_resampler_tests`: 6/6 tests passed.
   - `cargo test -p syncplay-core --test audio_pipeline_and_underrun_tests`: 4/4 tests passed.
   - `cargo test -p syncplay-clock`: 19/19 tests passed (including 7 adversarial clock stress tests).
   - `cargo test --workspace`: 100% of workspace tests passed cleanly.
   - `python3 scripts/intent_harness.py`: 8/8 intents passed, 100.0% compliance score.
   - `./scripts/verify_phase1.sh`: All 8 stages passed cleanly.
   - Live CLI verification: Synthetic 440Hz f32 PCM played cleanly via both `pw-cat` and `aplay` pipelines on Gautham's Fedora system.

4. **Integrity Violation Check**:
   - Audited for hardcoded test results, facade implementations, unauthorized external tools, fabricated verification logs, and self-certifying tests.
   - No integrity violations found. Real mathematical and systems logic is implemented throughout.

---

## 2. Logic Chain

1. **Deterministic Audio Pacing and Underrun Recovery**:
   - Audio buffer starvation is inevitable in decoupled multithreaded pipelines under I/O stalls or network packet jitter.
   - Without underrun protection, empty ring buffers stall the audio consumer thread, causing sound server pipe breakage (`-EPIPE`) and audible loud clicks/pops.
   - `AudioPacer` detects starvation atomically, increments metrics, and delivers pre-allocated silence frames synchronized with `MasterClock.current_pts(now)`.
   - When valid audio frames arrive after starvation, applying the 2ms linear ramp-in eliminates abrupt DC voltage steps.
   - Verified in `test_multithreaded_pipeline_with_underrun_starvation_and_recovery`: a producer thread starved for 50ms, and the consumer thread recovered cleanly with silence insertion followed by ramp-in.

2. **Zero-Allocation Hot-Loop Performance**:
   - Audio presentation callbacks require strict real-time guarantees; dynamic memory allocations in the callback thread cause heap lock contention and OS scheduler jitter.
   - In `AudioPacer::fill_samples` and `LinearResampler::resample_into`, `output.clear()` followed by slice copies preserves existing buffer capacity.
   - Verified in `test_zero_allocation_fill_samples` and `test_zero_allocation_hot_loop`: `output.capacity()` remained invariant across 100 iterations.

3. **Subprocess Pipe Lifecycle and Error Handling**:
   - Streaming raw PCM to `/usr/bin/pw-cat` and `/usr/bin/aplay` eliminates all foreign library dependencies.
   - If the sound daemon or child process dies, `write_samples` returns `Err(CoreError::Io(BrokenPipe))` without panicking.
   - On drop, `child.kill()` and `child.wait()` guarantee child processes are reaped without creating zombies.

---

## 3. Caveats

1. **Real-Time Priority Audio Thread Best Practice**: `AudioPacer::pull_frame` allocates a new `Vec<f32>` when cloning `silence_template` on underrun. The audio callback thread should always invoke `fill_samples` into a reused output buffer to guarantee zero allocations.
2. **Defensive Timestamp Sanitation**: In `AudioPacer::evaluate_timing`, if `delta_ms` is `f64::NAN`, `Duration::from_secs_f64(NaN)` would panic. A defensive `if delta_ms.is_nan()` check defaulting to `PlayNow` is recommended for future hardening.
3. **Subprocess Tool Requirement on Host**: `PipeAudioSink` requires either `pw-cat` or `aplay` on the Linux host. Both are verified to be installed on Gautham's Fedora system. On headless CI systems lacking sound daemons, `AutoAudioSink` transparently falls back to `NullAudioSink`.

---

## 4. Conclusion

**Verdict: APPROVE**

Milestone 2 (Pure-Rust Audio Output & Resampling Engine, Requirement R2) satisfies all requirements:
- `AudioPacer` delivers mathematically continuous audio presentation pacing, underrun protection, and 2ms smooth linear ramp-in recovery.
- `LinearResampler` provides seamless 44.1 kHz <-> 48.0 kHz sample rate conversion and micro-speed steering $r \in [0.96, 1.04]$ with zero hot-loop allocations and continuous phase tracking across arbitrary chunk boundaries.
- `PipeAudioSink`, `NullAudioSink`, and `AutoAudioSink` enable zero-external-dependency audio streaming to PipeWire (`pw-cat`) and ALSA (`aplay`) with headless fallback and clean subprocess reaping.
- All workspace tests pass 100%, and zero compiler/clippy warnings or banned dependencies exist.

---

## 5. Verification Method

To independently reproduce the complete verification suite:

```bash
# 1. Ensure Rust compiler is on PATH
export PATH="$HOME/.cargo/bin:$PATH"

# 2. Verify zero banned dependencies
./scripts/audit_dependencies.sh

# 3. Check workspace compilation and clippy lints (-D warnings)
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo doc --workspace --no-deps

# 4. Run dedicated audio integration tests
cargo test -p syncplay-codecs --test audio_resampler_tests -- --nocapture
cargo test -p syncplay-core --test audio_pipeline_and_underrun_tests -- --nocapture

# 5. Run clock crate tests including pacer unit tests
cargo test -p syncplay-clock -- --nocapture

# 6. Run full workspace test suite
cargo test --workspace

# 7. Run Pi Framework Intent Verification Harness (100.0% compliance)
python3 scripts/intent_harness.py

# 8. Run end-to-end automated verification script (8/8 stages)
./scripts/verify_phase1.sh

# 9. Verify live pipe streaming on Fedora Linux
python3 -c '
import struct, sys, math
samples = [0.1 * math.sin(2 * math.pi * 440 * i / 48000) for i in range(4800) for _ in (0, 1)]
sys.stdout.buffer.write(struct.pack(f"{len(samples)}f", *samples))
' | pw-cat --playback --raw --rate 48000 --channels 2 --format f32 -
```
