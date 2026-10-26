# Adversarial Challenge Report: Remediation of EBML Lacing Defects

**Target**: `crates/syncplay-demux/src/ebml.rs`  
**Challenger**: `challenger_p2_demux_fix` (teamwork_preview_challenger / critic)  
**Date**: 2026-10-25T10:12:00Z  
**Verdict**: **APPROVE**  

---

## Challenge Summary

**Overall risk assessment**: **LOW**

The two defects identified by `challenger_p2_demux_1` in `crates/syncplay-demux/src/ebml.rs` (falsely rejecting valid signed delta VINTs with all data bits set to 1 as `ReservedElementId`, and synthesizing trailing phantom empty frames when `frame_count == 1`) have been thoroughly, cleanly, and completely remediated by `worker_p2_demux_fix`. All 34 tests in `syncplay-demux` (including 15 adversarial stress tests in `ebml_adversarial_challenge.rs`), all 3 suites in `tests/empirical_challenge/test_ebml_adversarial.py`, all workspace compilation and clippy checks, dependency audits, the Pi Intent Harness (8/8, 100%), and the full Phase 1 end-to-end verification script execute with 0 errors and 0 warnings.

---

## Challenges Evaluated

### Challenge 1: Signed Delta VINT Decoding with All-Ones Data Mask

- **Assumption challenged**: That EBML signed delta VINTs can be safely parsed using Element ID semantics (`parse_vint(..., true)`).
- **Pre-remediation failure**: When an EBML block contained a signed delta where all data bits were 1 (e.g. `0xFF` representing $+64$ in 1 octet, or `0x7FFF` representing $+8192$ in 2 octets), `parse_vint` triggered `VintError::ReservedElementId`, failing valid media blocks.
- **Remediation verified**: In `crates/syncplay-demux/src/ebml.rs:175`, `parse_vint` is called with `is_element_id = false`. Both `0xFF` (+64) and `0x7FFF` (+8192) decode without raising `ReservedElementId`, properly biasing and adding to the frame size.
- **Empirical test**: `test_lacing_mode_3_ebml_defect_reserved_element_id_rejection` directly supplies `[2, 0x8A, 0xFF, ...]` and `[2, 0x8A, 0x7F, 0xFF, ...]`. Both pass cleanly, producing frames of lengths [10, 74, 5] and [10, 8202, 4].
- **Status**: **RESOLVED & VERIFIED**.

### Challenge 2: Single-Frame Lacing Phantom Frame Synthesis

- **Assumption challenged**: That lacing unpacking logic requiring at least two frames (`first_size` + delta loop + `last_sz`) can handle `frame_count == 1` without special casing.
- **Pre-remediation failure**: When `payload[0] == 0` (`frame_count = 1`), `prev_size` was parsed from the payload, the delta loop was skipped, and `last_sz = payload.len() - offset - sum_sizes` (0) was appended, yielding 2 frames: `[frame0, vec![]]`. This injected empty packets into demuxing.
- **Remediation verified**: In `crates/syncplay-demux/src/ebml.rs:142-144`, an immediate guard `if frame_count == 1 { return Ok(vec![payload[1..].to_vec()]); }` returns exactly the single frame without parsing delta or appending `last_sz`.
- **Empirical test**: `test_lacing_mode_3_ebml_defect_frame_count_one_generates_two_frames` verifies 1-frame laced blocks with regular data, prefix-like data, and empty remaining payload all return exactly 1 frame.
- **Status**: **RESOLVED & VERIFIED**.

### Challenge 3: Negative Frame Size and Truncation Invariants

- **Assumption challenged**: That signed deltas could cause negative frame sizes or payload underflows without being detected.
- **Attack scenario**: Supplying a delta that exceeds `prev_size` in the negative direction, or truncating the delta VINT halfway through.
- **Observed behavior**: Negative sizes are caught by `if new_size < 0` returning `DemuxError::CorruptedBlock("negative EBML laced frame size ...")`. Truncated VINTs are caught by bounds checking returning `DemuxError::UnexpectedEof` or `DemuxError::CorruptedBlock`.
- **Status**: **PASS (ROBUST)**.

### Challenge 4: Memory Safety and Fuzzing Under Corrupted Inputs

- **Assumption challenged**: That malformed, zero-prefix, or random payloads could cause buffer panics or out-of-bounds slice accesses.
- **Attack scenario**: 100,000+ pseudo-random bytes across lacing modes 0, 1, 2, 3, and 4.
- **Observed behavior**: Zero panics observed. All inputs return either `Ok(...)` or `Err(...)`.
- **Status**: **PASS (ROBUST)**.

---

## Stress Test Results

| Scenario | Expected Behavior | Actual Behavior | Result |
|---|---|---|---|
| EBML Lacing Delta `0xFF` (+64) | Decode 3 frames: [10, 74, 5] | Returned 3 frames: [10, 74, 5] | **PASS** |
| EBML Lacing Delta `0x7FFF` (+8192) | Decode 3 frames: [10, 8202, 4] | Returned 3 frames: [10, 8202, 4] | **PASS** |
| EBML Lacing `frame_count == 1` with 5-byte data | Return exactly 1 frame (5 bytes) | Returned 1 frame (5 bytes) | **PASS** |
| EBML Lacing `frame_count == 1` with empty data | Return exactly 1 frame (0 bytes) | Returned 1 frame (0 bytes) | **PASS** |
| EBML Lacing negative size delta (`5 + (-10) = -5`) | Reject with `CorruptedBlock` | Rejected with `CorruptedBlock` | **PASS** |
| EBML Lacing truncated delta header | Reject with `CorruptedBlock` | Rejected with `CorruptedBlock` | **PASS** |
| EBML Lacing zero prefix byte in delta | Reject with `InvalidZeroPrefix` | Rejected with `InvalidZeroPrefix` | **PASS** |
| Cues seeking (0ms, between cues, on cue, beyond last cue) | Correct keyframe clamping | Keyframe clamped accurately | **PASS** |
| Cues seeking with 0 cues | Fallback to 0ms | Clamped to 0ms | **PASS** |
| Multi-track sequential reading | Interleaved FIFO track isolation | Perfect packet sequence | **PASS** |
| Packet queue flushing on seek | Discard unread packets from prior PTS | Discarded, next packet from seek PTS | **PASS** |
| Pseudo-random fuzzing (modes 0..=4, lengths 0..128) | Zero panics | Zero panics (all Ok/Err) | **PASS** |
| Full workspace compilation & lints | Zero warnings (`-D warnings`) | 0 warnings | **PASS** |
| Dependency Blacklist Audit | 0 banned crates in Cargo.toml / Cargo.lock | 0 banned crates detected | **PASS** |
| Pi Framework Intent Verification Harness | 8/8 intents passed, 100.0% score | 8/8 passed, 100.0% score | **PASS** |
| End-to-End `./scripts/verify_phase1.sh` | All 8 stages passed | All 8 stages passed cleanly | **PASS** |

---

## Unchallenged Areas

- **Extremely large files (> 4 GB)**: Not stress-tested with real disk files due to sandbox disk constraints; however, 64-bit integer arithmetic for timescale calculations and offsets was verified against overflow.
