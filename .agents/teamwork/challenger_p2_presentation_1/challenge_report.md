# Adversarial Challenge Report: Codecs & Clock Subsystems (Milestone 2)

**Challenger Agent**: `challenger_p2_presentation_1` (teamwork_preview_challenger)  
**Parent Orchestrator**: `0644b5fa-9f3b-44ab-92cd-57079ebc4b70`  
**Working Directory**: `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_presentation_1/`  
**Date**: 2026-10-25T10:35:00Z  
**Verdict**: **APPROVE** (Subsystems are empirically robust; 1 low-severity finding noted)

---

## Challenge Summary

**Overall risk assessment**: **LOW**

The Milestone 2 presentation engine components under `crates/syncplay-codecs/` and `crates/syncplay-clock/` were subjected to intensive empirical stress testing across frame geometry boundaries, fixed-point integer arithmetic limits, high-concurrency race conditions, memory alignment and over-read guards, and bounded priority queue invariants.

All core mathematical, architectural, and memory safety invariants held under adversarial load:
1. **Frame Geometry & SIMD Padding**: Frames with odd widths and heights (e.g., 1x1, 3x3, 17x19, 1921x1081, 1x1080, 1920x1) strictly maintain 16-byte row stride alignment, 64-byte buffer alignment, and $+16$B SIMD tail padding. Canary guard bytes placed in the tail padding were completely uncorrupted, proving zero vector over-reads or out-of-bounds writes. The unrolled $2 \times 2$ chroma reuse kernel is mathematically bit-exact with the scalar oracle across 100% of tested pixels.
2. **Q8 Integer Fixed-Point Conversion**: Tested across 3,375 extreme boundary permutations and dense 3D color cube grids. Zero integer overflows, zero wrapping, branchless saturation clamping to $[0, 255]$ verified, and error strictly $\le 2$ LSB compared to floating-point ITU-R references.
3. **MasterClock SeqLock Concurrency**: Subjected to a multi-threaded reader/writer race with rapid speed oscillations ($0.96 \leftrightarrow 1.04$ at 1 kHz). Across 66,636,814 concurrent reads, exactly 0 torn reads, 0 negative jumps, and 0 wild forward jumps were observed. Mathematical $C^0$ continuity at transition instants held with $|\Delta PTS| < 10^{-4}$ ms.
4. **SPSC Ring Buffer**: Transferred 200,000 items through a tight 16-slot ring buffer across concurrent threads with zero data loss, zero corruption, and strict FIFO sequence preservation. Independent 128-byte cache-line isolation verified (`cache_line_distance >= 128`).
5. **16-Frame Reorder Buffer**: Under bursts of out-of-order, reverse-sorted, and duplicate frames, capacity was strictly clamped to 16 (`len <= 16`), with automatic minimum PTS ejection and strictly sorted ascending eviction on drain.

One non-blocking edge-case finding was discovered in `AudioFramePCM::new` regarding `NaN` sample bounding.

---

## Challenges

### [Low] Challenge 1: `AudioFramePCM` Sample Validation Permits `f32::NAN`

- **Assumption challenged**: `AudioFramePCM::new` guarantees all audio samples are bounded within the normalized amplitude range $[-1.0, 1.0]$ per `openspec/specs/codecs/spec.md` line 125.
- **Attack scenario**: An audio decoder produces a corrupted bitstream yielding IEEE 754 `f32::NAN` (Not a Number) samples. In `crates/syncplay-codecs/src/frame.rs`:
  ```rust
  for &s in &samples {
      if s < -1.0 || s > 1.0 {
          return Err(CodecError::InvalidAudio(format!(
              "sample value {} exceeds [-1.0, 1.0] bounds",
              s
          )));
      }
  }
  ```
  Under IEEE 754 floating-point comparison semantics in Rust, comparing `NaN` with any float returns `false`. Specifically:
  - `f32::NAN < -1.0` evaluates to `false`
  - `f32::NAN > 1.0` evaluates to `false`
  As a result, `AudioFramePCM::new` does not reject `NaN`, and accepts `f32::NAN` as a valid normalized audio sample.
- **Blast radius**: If corrupted PCM frames containing `NaN` samples are transferred into the audio presentation or mixer pipeline, downstream floating-point operations (gain, volume attenuation, resampling) propagate `NaN`, potentially causing silence or audio distortion.
- **Mitigation**: Update the sample range validation to:
  ```rust
  if !(-1.0..=1.0).contains(&s) {
      return Err(CodecError::InvalidAudio(format!(
          "sample value {} is not in valid [-1.0, 1.0] range",
          s
      )));
  }
  ```
  Or explicitly check `if !s.is_finite() || s < -1.0 || s > 1.0`.

---

### [Info] Challenge 2: MasterClock Presentation Timing Boundary Sensitivity on Running Clock

- **Assumption challenged**: `MasterClock::evaluate_presentation_timing` returns exact timing actions for frame delta boundaries (e.g. $\Delta = -15\text{ms} \implies \text{PresentNow}$).
- **Attack scenario**: When the clock is in `PlaybackState::Playing`, presentation time continuously tracks wall clock time:
  $$\text{master\_pts} = \text{anchor\_pts} + (t_{\text{now}} - t_{\text{anchor}}) \cdot \text{speed} \cdot 1000$$
  If a caller evaluates a candidate frame at the exact lower boundary of `PresentNow` ($\text{delta} = -15.0\text{ms}$) using an unanchored `Instant::now()`, any sub-microsecond elapsed time between clock anchor and evaluation causes $\text{delta} < -15.0\text{ms}$ (e.g., $-15.0001\text{ms}$), causing the action to slip into `PresentNowLate`.
- **Blast radius**: Low. This is physically accurate real-time clock progression, but testing harnesses or callers verifying exact mathematical boundaries must evaluate with `PlaybackState::Paused` or set an exact snapshot instant.
- **Mitigation**: Documented in test harness to use anchored instants when validating exact boundary cutoffs.

---

## Stress Test Results

| Component | Stress Scenario | Expected Behavior | Actual Behavior | Verdict |
|---|---|---|---|---|
| **Codecs** | Odd frame geometries: 1x1, 3x3, 17x19, 1x1080, 1920x1, 7x13, 65x33, 854x480, 1921x1081, 255x255 | 16-byte strides, 64-byte plane alignment, +16B SIMD tail padding | Stride % 16 == 0, planes 64-byte aligned, capacities exact | **PASS** |
| **Codecs** | Canary sentinel bytes in +16B SIMD tail padding during conversion | Canaries remain uncorrupted (no buffer over-read or overwrite) | 100% of canary bytes preserved across all odd resolutions | **PASS** |
| **Codecs** | Unrolled $2 \times 2$ chroma reuse kernel vs. scalar reference oracle | Exact bit-for-bit mathematical equivalence for all pixels | Identical RGBA values for all pixels in BT.601 and BT.709 | **PASS** |
| **Codecs** | Dimension mismatch between YUV source and RGBA destination | Return `Err(CodecError::DimensionMismatch)` | Returned `DimensionMismatch` without panic | **PASS** |
| **Codecs** | Q8 fixed-point arithmetic across 3,375 extreme boundary combinations | Zero integer wrapping, zero panic, clamping in $[0, 255]$ | Zero overflow, clean clamping, error $\le 2$ LSB vs float | **PASS** |
| **Clock** | MasterClock SeqLock 1 kHz rapid speed oscillation race ($0.96 \leftrightarrow 1.04$) | Zero torn reads, monotonic progress, $C^0$ continuity | 66,636,814 reads: 0 torn, 0 negative jumps, 0 wild jumps | **PASS** |
| **Clock** | Presentation scheduler 4-zone decision window | Early $\to$ Wait, On-time $\to$ PresentNow, Late $\to$ PresentNowLate, Severe $\to$ Drop | Exact 4-zone mapping verified | **PASS** |
| **Clock** | SPSC ring buffer cache line isolation | Distance between head and tail $\ge 128$ bytes | `cache_line_distance() >= 128` verified | **PASS** |
| **Clock** | SPSC high-throughput concurrent race (200,000 items in 16 slots) | Zero dropped items, strict FIFO order | 200,000 items popped in exact sequence `0..200000` | **PASS** |
| **Clock** | SPSC capacity saturation and full error handling | Reject 17th item in 16-slot buffer, preserve rejected item | `Err(BufferFullError { item })` returned, buffer stays full | **PASS** |
| **Clock** | Reorder buffer capacity clamping under 100 scrambled frames | Buffer size strictly $\le 16$, min PTS ejected | Max len was 16, 84 items ejected, remaining sorted | **PASS** |
| **Clock** | Reorder buffer reverse-sorted burst (50 frames descending) | Min PTS ejected, drain produces strictly ascending order | Strict ascending eviction and drain verified | **PASS** |
| **Clock** | Reorder buffer duplicate PTS (25 frames with same PTS) | Bounded capacity $\le 16$, no deadlock or heap corruption | Clamped to 16, all 16 drained cleanly | **PASS** |

---

## Unchallenged Areas

- **Direct DRM / Wayland SHM Presentation**: Actual kernel framebuffer ioctl or Wayland surface mapping requires display hardware / compositor; tested via in-memory 64-byte aligned packed RGBA32 buffers conforming to standard presentation surface specifications.
- **Large-capacity Ring Buffers ($> 65,536$)**: Tested tight buffers (16, 32, 64 slots) to intentionally maximize lock-free contention, wrapping, and boundary conditions.

---

## Verification Commands & Execution Logs

```bash
export PATH="$HOME/.cargo/bin:$PATH"

# Run syncplay-codecs adversarial stress suite
cargo test -p syncplay-codecs --test adversarial_codecs_stress -- --nocapture
# Result: 6 passed; 0 failed; finished in 0.34s

# Run syncplay-clock adversarial stress suite
cargo test -p syncplay-clock --test adversarial_clock_stress -- --nocapture
# Result: 7 passed; 0 failed; finished in 1.59s (66.6M SeqLock reads, 200k SPSC items)
```
