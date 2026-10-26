# Code Review & Adversarial Challenge Report

**Reviewer**: `reviewer_p3_audio_1`  
**Milestone**: Milestone 2 — Pure-Rust Audio Output & Resampling Engine (Requirement R2)  
**Target Files**:
- `crates/syncplay-codecs/src/audio/resampler.rs`
- `crates/syncplay-codecs/tests/audio_resampler_tests.rs`
- `crates/syncplay-clock/src/audio_pacer.rs`
- `crates/syncplay-core/src/audio_sink.rs`  
**Date**: 2026-10-25T16:56:00Z  

---

## 1. Review Summary

**Verdict**: **REQUEST_CHANGES**

While the core phase tracking across chunk boundaries and zero-allocation buffer reuse in `LinearResampler` are cleanly designed and compile with zero warnings under pure-Rust constraints, **a critical mathematical defect was discovered in the micro-speed steering ratio calculation ($r \in [0.96, 1.04]$)**:
The step size formula:
$$\text{step} = \frac{f_{\text{in}}}{f_{\text{out}} \times r}$$
places the playback speed multiplier $r$ in the denominator instead of the numerator. This **inverts the speed steering multiplier ($r \mapsto 1/r$)**, causing commanded speed-up ($r > 1$) to decelerate playback, and commanded slow-down ($r < 1$) to accelerate playback. In a closed-loop network clock synchronization environment, this transforms the discrete PI Phase-Locked Loop (PLL) negative feedback into destabilizing positive feedback, driving phase error into divergent oscillation and forced hard seeks.

Furthermore, unit and integration tests were self-certifying: tests in `resampler.rs` and `audio_resampler_tests.rs` asserted that $1.04\times$ speed should yield more output frames (`expected_frames = 4800.0 * ratio`), validating the implementation against the faulty formula rather than physical sample rate relations.

---

## 2. Integrity Violation & Forensic Audit

| Check Category | Status | Assessment |
|:---|:---:|:---|
| **Hardcoded Test Outputs in Source** | **CLEAN** | No hardcoded outputs or lookup tables embedded in source logic. |
| **Dummy / Facade Implementations** | **CLEAN** | Subprocess pipe streaming and linear fractional interpolation logic are genuinely implemented. |
| **Bypassing Core Task via Foreign Crates** | **CLEAN** | `./scripts/audit_dependencies.sh` passes; zero foreign codec/media crates used. |
| **Fabricated Verification Logs** | **CLEAN** | Verification logs reproduce commands authentically. |
| **Self-Certifying Work / Facade Tests** | **FLAGGED** | **Self-certifying test logic**: Unit test line 326 of `resampler.rs` and line 126 of `audio_resampler_tests.rs` asserted `out_frames = in_frames * ratio`, asserting that $1.04\times$ speed generates *more* output frames. When played on a fixed-rate sound DAC, more output frames take *longer* to play ($1.04\text{s}$ wall time per $1.0\text{s}$ audio), decelerating playback. Tests tested consistency with a flawed formula rather than physical reality. |

---

## 3. Findings & Detailed Defect Analysis

### [Critical] Finding 1: Inverted Micro-Speed Steering Formula in `LinearResampler` ($r \mapsto 1/r$)

- **What**: In `crates/syncplay-codecs/src/audio/resampler.rs:138`, the fractional step size is calculated as:
  ```rust
  let step = (self.in_rate as f64) / ((self.out_rate as f64) * speed_ratio);
  ```
- **Where**: `crates/syncplay-codecs/src/audio/resampler.rs:138` and tests in lines 326–335 and `tests/audio_resampler_tests.rs:126–135`.
- **Why**: 
  1. **Dimensional & Physical Proof**:
     - Let $f_{\text{in}}$ be input sample rate (e.g. 48,000 Hz) and $f_{\text{out}}$ be DAC output sample rate (48,000 Hz).
     - In `LinearResampler::resample_into`, `t` tracks input coordinates. Each iteration pushes **1 output sample** and advances `t` by `step`.
     - Therefore, the output DAC consumes $f_{\text{out}}$ output samples per wall-clock second.
     - The quantity of input audio consumed per wall-clock second is:
       $$\text{Input audio consumed per second} = f_{\text{out}} \times \text{step}$$
     - With the code's formula:
       $$\text{Input audio consumed per second} = f_{\text{out}} \times \left( \frac{f_{\text{in}}}{f_{\text{out}} \times r} \right) = \frac{f_{\text{in}}}{r}$$
     - If the network PLL commands $r = 1.04$ (speed up by 4% to eliminate a positive local lag $e > 0$):
       $$\text{Input audio consumed per second} = \frac{48,000}{1.04} \approx 46,154 \text{ samples/sec}$$
       The resampler consumes **4% LESS input audio per second**, causing local media playback to fall further behind!
     - Conversely, if $r = 0.96$ (slow down by 4% to eliminate a negative local lead $e < 0$):
       $$\text{Input audio consumed per second} = \frac{48,000}{0.96} = 50,000 \text{ samples/sec}$$
       The resampler consumes **4.17% MORE input audio per second**, causing local media playback to pull further ahead!
  2. **Control Loop Blast Radius**:
     - In `syncplay-sync` PI PLL, $\Delta r = K_p e + K_i \int e \, dt$.
     - Because the resampler inverts $r$, positive phase error causes slower consumption, which increases phase error $\dot{e} > 0$.
     - This turns the negative feedback control loop into **destabilizing positive feedback**, causing the PLL to runaway until it trips Zone 3 hard seeks ($|e| > 1500\text{ms}$).
- **Suggestion**:
  1. Fix the step calculation in `crates/syncplay-codecs/src/audio/resampler.rs:138`:
     ```rust
     let step = ((self.in_rate as f64) * speed_ratio) / (self.out_rate as f64);
     ```
  2. Update unit test `test_speed_ratio_micro_steering` in `resampler.rs:326–335`:
     ```rust
     // At 1.04x speed, step is larger (48000 * 1.04 / 48000 = 1.04)
     // so input is consumed 1.04x faster, generating fewer output frames (4800 / 1.04)
     let out_frames = output.len() / 2;
     let expected_frames = (4800.0f64 / 1.04).round() as usize;
     assert!(
         (out_frames as isize - expected_frames as isize).abs() <= 2,
         "Expected ~{} frames, got {}",
         expected_frames,
         out_frames
     );
     ```
  3. Update integration test in `tests/audio_resampler_tests.rs:126–135`:
     ```rust
     let out = resampler.resample(&input).unwrap();
     let expected_frames = (4800.0 / ratio).round() as usize;
     let actual_frames = out.len() / 2;
     assert!(
         (actual_frames as isize - expected_frames as isize).abs() <= 2,
         "Ratio {}: expected ~{} frames, got {}",
         ratio,
         expected_frames,
         actual_frames
     );
     ```

---

### [Minor] Finding 2: Unverified 6-Channel (5.1 Surround) Interleaving in Test Suite

- **What**: The header of `crates/syncplay-codecs/tests/audio_resampler_tests.rs` explicitly advertises:
  `//! - Multi-channel interleaving (mono, stereo, 6-channel).`
  However, the test suite only instantiates 1-channel and 2-channel tests; zero 6-channel integration tests exist.
- **Where**: `crates/syncplay-codecs/tests/audio_resampler_tests.rs:8`
- **Why**: While `resampler.rs` uses generic `0..ch` channel loops, omitting multi-channel tests leaves 5.1 surround sound interleaving unverified across chunk boundaries.
- **Suggestion**: Add a dedicated integration test `test_resampler_5_1_surround_interleaving` testing 6 channels across prime chunk splits.

---

### [Minor] Finding 3: Potential Panic on NaN in `AudioPacer::evaluate_timing`

- **What**: If uninitialized or corrupted timestamps result in `delta_ms = NaN`, `delta_ms > 20.0` and `delta_ms >= -20.0` both evaluate to false. The execution falls through to `AudioTimingAction::Late(Duration::from_secs_f64(late_ms / 1000.0))`, where `Duration::from_secs_f64(NaN)` panics in `std`.
- **Where**: `crates/syncplay-clock/src/audio_pacer.rs:173–181`
- **Why**: Defensive guard needed to prevent crashes on adversarial inputs.
- **Suggestion**:
  ```rust
  if delta_ms.is_nan() {
      return AudioTimingAction::PlayNow;
  }
  ```

---

## 4. Adversarial Challenges & Stress-Testing

### Challenge 1: Micro-Speed Directionality in Closed-Loop Playback
- **Assumption challenged**: That dividing by `speed_ratio` accelerates audio consumption.
- **Attack scenario**: Simulate 10 seconds of 48 kHz stereo audio at $r = 1.04$. At 48 kHz, 10 seconds of audio should be consumed in $10 / 1.04 = 9.615$ seconds of DAC playback time.
- **Result with current implementation**: 10 seconds of audio generates 10.4 seconds of DAC output. The audio plays for 10.4 seconds, meaning it was slowed down by 4%.
- **Verdict**: **FAIL**. Proves inversion bug.

### Challenge 2: Long-Term Phase Accumulator Stability
- **Assumption challenged**: That `state.phase: f64` maintains precision over 24+ hours of streaming without float64 precision degradation.
- **Stress test**: At the end of every chunk of $N$ frames, `state.phase` is updated to $t - N$. Because $t$ is bounded within $[N, N + \text{step}]$, `state.phase` is strictly reset into the interval $[0, \text{step})$. It never accumulates unbounded growth.
- **Result**: `state.phase` remains bounded within $[0, 2.0]$ indefinitely, preserving 53 bits of IEEE 754 precision forever.
- **Verdict**: **PASS**.

### Challenge 3: Arbitrary Chunk Boundary Splitting Down to 1-Sample Chunks
- **Assumption challenged**: That 1-sample chunk streams do not introduce DC bias, drift, or sample drops.
- **Stress test**: Processed 2,000 samples in chunks of `[1]*100 + [5]*20 + [17, 33, 1024, 500]`.
- **Result**: Output length and sample values matched monolithic reference within $3.16 \times 10^{-12}$ float64 error.
- **Verdict**: **PASS**.

---

## 5. Verified Claims

| Claim | Method | Result |
|:---|:---|:---:|
| Zero foreign codec/media crates | `./scripts/audit_dependencies.sh` | **PASS** |
| Zero compiler & clippy warnings | `cargo check --workspace` & `cargo clippy -- -D warnings` | **PASS** |
| Zero allocation in hot loop | `resample_into` capacity retention over 100 iterations | **PASS** |
| Phase continuity across chunk splits | Prime chunk splitting comparison against monolithic reference | **PASS** |
| Micro-speed steering ratio directionality | Dimensional analysis of consumption rate $f_{\text{out}} \times \text{step}$ | **FAIL** (Inverted) |

---

## 6. Coverage Gaps & Unverified Items

- **Hardware Sound Output under High CPU Pressure**: Audio buffer underruns under 100% core load cannot be tested without a desktop presentation loop, scheduled for Phase 3 integration testing. Risk: **LOW**.

---

## 7. Required Fixes Before Approval

1. **Invert Step Formula**: In `crates/syncplay-codecs/src/audio/resampler.rs:138`, change `step = in_rate / (out_rate * speed_ratio)` to `step = (in_rate * speed_ratio) / out_rate`.
2. **Update Tests**: Correct the expected frame calculations in `crates/syncplay-codecs/src/audio/resampler.rs:329` and `crates/syncplay-codecs/tests/audio_resampler_tests.rs:126` to `expected_frames = (in_frames / ratio)`.
3. **Add 5.1 Test**: Add a 6-channel test to `crates/syncplay-codecs/tests/audio_resampler_tests.rs` to validate surround sound interleaving.
