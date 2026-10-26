# Empirical Challenge Report: Milestone 2 — AudioPacer & AudioSink Stress Testing

**Agent**: `challenger_p3_audio_2`  
**Role**: Adversarial Challenger (critic, specialist)  
**Date**: 2026-10-25T17:05:00Z  
**Target Subsystems**: `crates/syncplay-clock` (`AudioPacer`, `SpscRingBuffer`), `crates/syncplay-core` (`PipeAudioSink`, `AutoAudioSink`, `NullAudioSink`, `MockAudioSink`)  
**Workspace**: `/home/gautham/syncplay-alt`  
**Verdict**: **APPROVE**

---

## 1. Executive Summary

Milestone 2 Requirement R2 (Pure-Rust Audio Output & Resampling Engine) was subjected to rigorous empirical challenge across four primary stress dimensions:
1. **Audio Underrun Starvation Storm (1,000 Bursts)**: Simulated 1,000 consecutive bursts of severe ring buffer starvation followed by valid audio recovery; verified silence injection, zero pops, and smooth 2ms linear ramp-in.
2. **Multithreaded SPSC Ring Buffer High-Throughput Concurrency (50,000 Frames)**: Streamed 50,000 full audio frames (102,400,000 float samples, ~409.6 MB) across concurrent producer and consumer threads; verified zero lost frames, zero data races, exact sequence timestamps, and bit-level deterministic integrity.
3. **PipeAudioSink Broken Pipe Resilience**: Spawned live `pw-cat` audio subprocess, simulated abrupt process death via `SIGKILL`, and verified instantaneous error propagation (`Broken pipe, os error 32`) without thread stalls or resource leaks.
4. **AutoAudioSink Headless & Daemon Fallback**: Simulated sound daemon / CLI tool unavailability via empty `PATH` subprocess execution; verified transparent fallback to `NullAudioSink` with full trait compliance and memory bounding.

**Overall Risk Assessment**: **LOW** (All empirical tests passed cleanly with 100% reproducibility across consecutive runs).

---

## 2. Adversarial Challenges & Empirical Results

### Challenge 1: Audio Underrun Starvation Storm (1,000 Bursts)
- **Assumption Challenged**: Under frequent, rapid alternations between buffer starvation and valid frames, `AudioPacer` might leak memory, lose tracking of underrun state, panic, or cause audible DC voltage steps (pops/clicks) on resumption.
- **Empirical Attack Harness**:
  - Executed in `test_audio_underrun_starvation_storm_1000_bursts`.
  - Configured 48 kHz stereo audio with 1024-sample frames (2048 f32 samples per frame).
  - Subjected `AudioPacer` to 1,000 distinct starvation cycles:
    1. *Starvation Phase*: Polled empty `SpscRingBuffer`. Verified `pull_frame` returns a silence frame where all 2048 samples are identically `0.0f32`, presentation timestamp matches `MasterClock.current_pts(now)`, and `is_in_underrun()` is `true`.
    2. *Resumption Phase*: Pushed 2 valid audio frames with test amplitude `0.75f32`.
    3. *Ramp-In Verification*: Inspected recovered Frame 0:
       - Sample 0 is exactly `0.0f32` (continuous transition from silence).
       - Leading 96 frames (2.0 ms ramp window at 48 kHz) are monotonically scaled: $g(f) = \frac{f}{96}$, with zero DC pop or overshoot.
       - Post-ramp frames (samples 96..1024) are at full $0.75$ amplitude.
    4. *Subsequent Frame*: Verified Frame 1 is NOT ramped ($samples[0] = 0.75f32$).
  - Subjected `AudioPacer` to a secondary 500-burst storm using `fill_samples` into a pre-allocated scratch buffer:
    - Verified zero reallocations (`scratch.capacity() == initial_cap` retained across all 500 iterations).
- **Results**:
  - Bursts Executed: 1,000 primary + 500 secondary = 1,500 total starvation cycles.
  - Underruns Recorded: Exactly 1,000.
  - Silence Injected: Exactly 1,000 frames.
  - Valid Frames Processed: Exactly 2,000 frames.
  - Panics / Pops / NaNs / Infs: **0**.
  - Status: **PASS**.

---

### Challenge 2: Multithreaded SPSC Ring Buffer Concurrency (50,000 Frames)
- **Assumption Challenged**: High-speed cross-thread transfer over a fixed-capacity (64-slot) ring buffer with 128-byte cache-aligned atomic head/tail could suffer from data races, counter wrapping errors, or lost frames under high contention.
- **Empirical Attack Harness**:
  - Executed in `test_multithreaded_spsc_ring_buffer_50k_audio_frames`.
  - Transferred 50,000 audio frames ($50,000 \times 2,048 = 102,400,000$ 32-bit float samples $\approx 409.6$ MB).
  - *Producer Thread*: Continuously pushed frames with monotonically increasing PTS ($i \times 21$) and deterministic sample values: $s_{left}(k) = ((i \pmod{1000}) \times 0.0005) + k \times 10^{-5}$, spinning on `BufferFullError`.
  - *Consumer Thread*: Continuously popped frames, validating sequential PTS, channel count, sample count, and deterministic payload values for every single frame.
- **Results**:
  - Total Frames Sent: 50,000.
  - Total Frames Received: 50,000.
  - Lost / Dropped Frames: **0**.
  - Data Races / Corrupted Samples: **0**.
  - Execution Time: 1.56s – 1.80s across runs.
  - Sustained Throughput: **28,000 – 32,000 fps** (~220 – 250 MB/s lock-free audio streaming).
  - Status: **PASS**.

---

### Challenge 3: PipeAudioSink Broken Pipe & Process Death Resilience
- **Assumption Challenged**: If the background audio daemon (`pw-cat` or `aplay`) crashes, is killed, or closes its stdin descriptor, writes to `PipeAudioSink` might deadlock or hang the calling presentation thread.
- **Empirical Attack Harness**:
  - Executed in `test_pipe_audio_sink_broken_pipe_resilience`.
  - Initialized `PipeAudioSink::new()` spawning active `pw-cat` on host Fedora Linux.
  - Successfully wrote initial frame to confirm live pipe.
  - Located child PID via `/proc/[pid]/stat` and terminated it immediately via `kill -9` (`SIGKILL`).
  - Executed subsequent `write_samples` calls with elapsed time measurement.
- **Results**:
  - First write after kernel cleanup immediately returned `Err(CoreError::Io(Broken pipe (os error 32)))`.
  - Call Elapsed Time: **< 15ms** (well below the 500ms upper bound; zero thread freezing).
  - Subsequent 10 consecutive writes failed immediately (< 1ms each).
  - `flush()` and `drop()` completed cleanly without hanging or panicking.
  - Status: **PASS**.

---

### Challenge 4: AutoAudioSink Daemon Unavailability & Fallback Safety
- **Assumption Challenged**: When audio daemons or system audio tools are unavailable (e.g. minimal Docker containers, CI test runners, headless environments), `AutoAudioSink` must cleanly fall back to `NullAudioSink` without crashing.
- **Empirical Attack Harness**:
  - Executed in `test_auto_audio_sink_fallback_and_headless_safety`.
  - Spawned isolated subprocess with cleared `PATH=""` to simulate complete absence of `pw-cat` and `aplay`.
  - Verified `AutoAudioSink::new()` selects `AutoAudioSink::Null`.
  - Verified `is_null() == true`, `backend_name() == "null"`.
  - Streamed 100,000 samples to the fallback sink; verified `write_samples`, `flush`, `pause`, and `resume` succeed with exact sample counting ($100,000$ samples written).
  - Tested `NullAudioSink` directly and `MockAudioSink` sample capture.
- **Results**:
  - Fallback Detection: 100% reliable under empty `PATH`.
  - Sample Accountability: Exactly 100,000 samples accounted for.
  - Status: **PASS**.

---

## 3. Findings & Engineering Recommendations

### Observation / Non-Blocking Finding: Parameter Validation in `PipeAudioSink::with_config`
- **Observation**:
  `PipeAudioSink::spawn_pw_cat` and `spawn_aplay` invoke `std::thread::sleep(Duration::from_millis(5))` to check if the child process exited immediately via `child.try_wait()`. While missing binaries (`NotFound`) fail synchronously at `spawn()`, invalid configurations (such as `sample_rate == 0` or `channels == 0`) spawn a child process that must be scheduled and terminate within 5ms. Under extreme thread contention, relying on 5ms for scheduling can occasionally create a race.
- **Recommendation**:
  Add an explicit precondition check at the top of `PipeAudioSink::with_config`:
  ```rust
  if sample_rate == 0 || channels == 0 {
      return Err(CoreError::Runtime("sample_rate and channels must be > 0".into()));
  }
  ```
  This matches the existing contract already enforced by `AudioPacer::new`, `AudioFramePCM::new`, and `LinearResampler::new`.

---

## 4. Verification Method

To independently reproduce all empirical challenge results:

```bash
export PATH="$HOME/.cargo/bin:$PATH"

# 1. Dependency Blacklist Audit
./scripts/audit_dependencies.sh

# 2. Rust Adversarial Test Suite (5 tests: starvation storm, SPSC stress, broken pipe, fallback, timing)
cargo test -p syncplay-core --test adversarial_audio_pacer_sink_stress -- --nocapture

# 3. Python Empirical Challenge Runner
python3 tests/empirical_challenge/test_audio_pacer_sink_resilience.py

# 4. Intent Verification Harness (100% compliance)
python3 scripts/intent_harness.py

# 5. Full Workspace Test Suite
cargo test --workspace
```

---

## 5. Final Verdict

**Verdict**: **APPROVE**  
`AudioPacer` and `AudioSink` implementations meet all requirements of Milestone 2 (Phase 3 Requirement R2) with mathematical precision, rock-solid multithreaded concurrency, and crash resilience.
