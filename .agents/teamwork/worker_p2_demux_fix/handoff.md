# Handoff Report: Remediation of EBML Demuxer Frame Lacing Defect

**Agent**: `worker_p2_demux_fix` (teamwork_preview_worker / implementer / qa)  
**Date**: 2026-10-25T10:06:00Z  
**Target**: `crates/syncplay-demux/src/ebml.rs` and `crates/syncplay-demux/tests/ebml_adversarial_challenge.rs`  
**Verdict**: **`APPROVED / REMEDIATED`**  
**Type**: Hard Handoff  

---

## 1. Observation

1. **Defect 1 — EBML Lacing Signed Delta `parse_vint(..., true)` Rejection**:
   - Location: `crates/syncplay-demux/src/ebml.rs:172` (pre-remediation).
   - Code before fix:
     ```rust
     let (raw_vint, _, _) = parse_vint(&payload[offset..offset + w], true)?;
     ```
   - In `crates/syncplay-demux/src/vint.rs:49-56`, `is_element_id = true` triggers the check `(raw_val & data_mask) == data_mask`, returning `Err(VintError::ReservedElementId)`.
   - When a valid EBML signed delta VINT had all data bits equal to 1 (such as `0xFF` representing $+64$ in 1 octet, or `0x7FFF` representing $+8192$ in 2 octets), `parse_vint` rejected it with `VintError::ReservedElementId`.
   - Code after fix (`crates/syncplay-demux/src/ebml.rs:175`):
     ```rust
     let (raw_vint, _, _) = parse_vint(&payload[offset..offset + w], false)?;
     ```
   - Execution result: `0xFF` delta and `0x7FFF` deltas now decode cleanly into $+64$ and $+8192$ frame size differences without error.

2. **Defect 2 — EBML Lacing with `frame_count == 1` Produces Phantom Extra Frame**:
   - Location: `crates/syncplay-demux/src/ebml.rs:141-154` (pre-remediation).
   - Code before fix:
     ```rust
     let frame_count = (payload[0] as usize) + 1;
     let mut offset = 1;
     let mut sizes = Vec::with_capacity(frame_count);
     ...
     let (_, first_size_opt, vint_len) = parse_vint(&payload[offset..], false)?;
     let mut prev_size = ...;
     sizes.push(prev_size);
     for _ in 1..frame_count - 1 { ... } // 1..0, skipped for frame_count = 1
     let last_sz = payload.len() - offset - sum_sizes;
     sizes.push(last_sz);
     ```
   - When `payload[0] == 0` (`frame_count = 1`), `sizes` accumulated both `prev_size` and `last_sz`, returning 2 frames (`[frame_data, vec![]]`).
   - Code after fix (`crates/syncplay-demux/src/ebml.rs:141-144`):
     ```rust
     let frame_count = (payload[0] as usize) + 1;
     if frame_count == 1 {
         return Ok(vec![payload[1..].to_vec()]);
     }
     ```
   - Execution result: Blocks with `frame_count == 1` return exactly 1 frame `payload[1..]` without appending phantom empty frames.

3. **Empirical Adversarial Test Suite Results**:
   - `cargo test -p syncplay-demux -- --nocapture`:
     - `src/lib.rs`: 8 passed, 0 failed.
     - `tests/demux_tests.rs`: 5 passed, 0 failed.
     - `tests/ebml_adversarial_challenge.rs`: 15 passed, 0 failed.
     - `tests/isobmff_adversarial_challenge.rs`: 6 passed, 0 failed.
     - Total: 34 passed, 0 failed.
   - `python3 tests/empirical_challenge/test_ebml_adversarial.py`:
     - Frame lacing adversarial stress tests: [PASS]
     - Cues seeking boundary tests: [PASS]
     - Multi-track queue invariant tests: [PASS]
   - `cargo clippy -p syncplay-demux --all-targets -- -D warnings`: Finished cleanly with 0 warnings.
   - `./scripts/audit_dependencies.sh`: Zero banned dependencies.
   - `python3 scripts/intent_harness.py`: 8/8 intents passed, 100.0% compliance score.
   - `./scripts/verify_phase1.sh`: All 8 stages passed cleanly.

---

## 2. Logic Chain

1. **RFC 8794 & EBML Signed Delta Semantics**:
   - In EBML lacing, delta values represent frame size differences encoded as biased signed integers, not Element IDs.
   - An Element ID must not have all data bits set to 1 per RFC 8794 §4.1 (`ReservedElementId`). However, a signed delta integer may legitimately have all data bits set to 1 (representing maximum positive delta, e.g. $+64$ for 1-octet VINT or $+8192$ for 2-octet VINT).
   - Changing `parse_vint(..., true)` to `parse_vint(..., false)` ensures `parse_vint` skips the `ReservedElementId` check while still correctly decoding `raw_val` and `octet_width`.

2. **Matroska Lacing Header Invariant for Single-Frame Lace**:
   - In Matroska SimpleBlock/Block lacing, the last frame in any lace is never explicitly sized; its length is deduced from the remaining payload bytes.
   - When a block specifies 1 frame (`frame_count = 1`), the only frame is the last frame. Therefore, zero frame sizes are coded in the lacing header; the single frame consists of the entire remainder `payload[1..]`.
   - Adding `if frame_count == 1 { return Ok(vec![payload[1..].to_vec()]); }` eliminates the erroneous parsing of an initial size and trailing subtraction, guaranteeing that exactly 1 frame is returned.

3. **Non-Regression & System Safety**:
   - The changes are strictly localized to `unpack_lacing` in `crates/syncplay-demux/src/ebml.rs` (minimal change principle).
   - All existing tests (multi-frame EBML lacing, Xiph lacing, Fixed lacing, Cues seeking, multi-track interleaving) continue to pass.
   - The intent harness confirms 0 drift across all 8 core intents.

---

## 3. Caveats

- **No Caveats**: Both defects were thoroughly reproduced, isolated, remediated, and verified using both Rust integration tests and external Python challenge harnesses.

---

## 4. Conclusion

Both defects reported by `challenger_p2_demux_1` in `crates/syncplay-demux/src/ebml.rs` have been completely resolved:
1. Signed delta VINTs with all data bits set to 1 are cleanly accepted and correctly biased.
2. Single-frame laced blocks (`frame_count == 1`) emit exactly 1 frame without trailing empty packets.
3. All unit, integration, clippy, dependency audit, Pi intent harness, and Phase 1 verification tests pass with 0 errors and 0 warnings.

---

## 5. Verification Method

To independently verify this remediation, run the following commands:

```bash
# 1. Ensure Rust toolchain is in PATH
export PATH="$HOME/.cargo/bin:$PATH"

# 2. Run syncplay-demux test suite including adversarial challenges
cargo test -p syncplay-demux -- --nocapture

# 3. Specifically verify the two remediated challenge tests
cargo test -p syncplay-demux --test ebml_adversarial_challenge -- test_lacing_mode_3_ebml_defect_reserved_element_id_rejection --nocapture
cargo test -p syncplay-demux --test ebml_adversarial_challenge -- test_lacing_mode_3_ebml_defect_frame_count_one_generates_two_frames --nocapture

# 4. Run empirical python challenge harness
python3 tests/empirical_challenge/test_ebml_adversarial.py

# 5. Check strict clippy compliance
cargo clippy -p syncplay-demux --all-targets -- -D warnings

# 6. Verify zero banned dependencies
./scripts/audit_dependencies.sh

# 7. Verify Pi Intent Harness (100% compliance)
python3 scripts/intent_harness.py

# 8. Run full Phase 1 verification script
./scripts/verify_phase1.sh
```

### Invalidation Conditions:
- If `unpack_lacing(3, &[2, 0x8A, 0xFF, ...])` fails with `ReservedElementId` or fails to return 3 valid frames.
- If `unpack_lacing(3, &[0, ...])` returns anything other than exactly 1 frame.
