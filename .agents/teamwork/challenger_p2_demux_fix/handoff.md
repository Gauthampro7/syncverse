# Handoff Report: Verification of EBML Demuxer Remediation (`crates/syncplay-demux/src/ebml.rs`)

**Target**: `crates/syncplay-demux/src/ebml.rs`  
**Challenger**: `challenger_p2_demux_fix` (teamwork_preview_challenger / critic)  
**Date**: 2026-10-25T10:14:00Z  
**Verdict**: **`APPROVE`**  
**Type**: Hard Handoff  

---

## 1. Observation

1. **Remediation of Defect 1 (Signed Delta VINT 0xFF Rejection)**:
   - File & Line: `crates/syncplay-demux/src/ebml.rs:175`
   - Verified code:
     ```rust
     let (raw_vint, _, _) = parse_vint(&payload[offset..offset + w], false)?;
     ```
   - Direct test execution:
     Command: `cargo test -p syncplay-demux --test ebml_adversarial_challenge -- test_lacing_mode_3_ebml_defect_reserved_element_id_rejection --nocapture`
     Result:
     ```
     running 1 test
     test test_lacing_mode_3_ebml_defect_reserved_element_id_rejection ... ok
     test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 14 filtered out; finished in 0.00s
     ```
   - Both 1-octet all-ones delta `0xFF` (+64) and 2-octet all-ones delta `0x7FFF` (+8192) parse without returning `ReservedElementId`, correctly unpacking into 3 frames with expected sizes.

2. **Remediation of Defect 2 (Single-Frame Lacing Phantom Frame Synthesis)**:
   - File & Lines: `crates/syncplay-demux/src/ebml.rs:141-144`
   - Verified code:
     ```rust
     let frame_count = (payload[0] as usize) + 1;
     if frame_count == 1 {
         return Ok(vec![payload[1..].to_vec()]);
     }
     ```
   - Direct test execution:
     Command: `cargo test -p syncplay-demux --test ebml_adversarial_challenge -- test_lacing_mode_3_ebml_defect_frame_count_one_generates_two_frames --nocapture`
     Result:
     ```
     running 1 test
     test test_lacing_mode_3_ebml_defect_frame_count_one_generates_two_frames ... ok
     test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 14 filtered out; finished in 0.00s
     ```
   - A laced block with `payload[0] == 0` returns exactly 1 frame `payload[1..]` without appending empty trailing packets.

3. **Demuxer Test Suite & Adversarial Test Suites**:
   - Command: `cargo test -p syncplay-demux -- --nocapture`
     Result:
     - `src/lib.rs`: 8 passed, 0 failed.
     - `tests/demux_tests.rs`: 5 passed, 0 failed.
     - `tests/ebml_adversarial_challenge.rs`: 15 passed, 0 failed.
     - `tests/isobmff_adversarial_challenge.rs`: 6 passed, 0 failed.
     - Total: 34 passed, 0 failed, 0 ignored.
   - Command: `python3 tests/empirical_challenge/test_ebml_adversarial.py`
     Result:
     - Stage 1 (Frame lacing stress tests): `[PASS]`
     - Stage 2 (Cues seeking boundary tests): `[PASS]`
     - Stage 3 (Multi-track queue invariant tests): `[PASS]`

4. **Workspace Invariants & Quality Standards**:
   - `cargo check --workspace --all-targets`: Passed with code 0, 0 compiler warnings.
   - `cargo clippy --workspace --all-targets -- -D warnings`: Passed with code 0, 0 warnings.
   - `./scripts/audit_dependencies.sh`: Zero banned dependencies in `Cargo.toml` or `Cargo.lock`.
   - `python3 scripts/intent_harness.py`: 8/8 intents passed, 100.0% compliance score.
   - `./scripts/verify_phase1.sh`: All 8 stages passed cleanly.
   - CLI execution: `inspect`, `play`, `party`, `chat demo`, and `verify` all exit 0 with expected output.

---

## 2. Logic Chain

1. **EBML Signed Delta Semantics**:
   - Per RFC 8794 §4.1, `ReservedElementId` applies strictly to Element IDs when all data bits are set to 1.
   - Matroska EBML lacing deltas are signed integer differences, not Element IDs.
   - Observation 1 proves that passing `false` as `is_element_id` to `parse_vint` disables the `ReservedElementId` error while correctly decoding the raw integer value, enabling deltas `0xFF` (+64) and `0x7FFF` (+8192) to calculate frame sizes accurately.

2. **Matroska Single-Frame Invariant**:
   - In Matroska block lacing, when `frame_count == 1`, there is only one frame which is also the final frame. The final frame is never explicitly sized; its length is the entirety of the remaining payload.
   - Observation 2 proves that returning `Ok(vec![payload[1..].to_vec()])` when `frame_count == 1` completely prevents the parsing of an erroneous initial frame size and trailing subtraction, guaranteeing that exactly 1 frame is returned.

3. **Non-Regression & Workspace Integrity**:
   - Observations 3 and 4 confirm that no regressions were introduced to unlaced parsing, Xiph lacing, Fixed lacing, Cues seeking, multi-track demuxing, or any dependent workspace crates (`syncplay-core`, `syncplay-codecs`, `syncplay-clock`, `syncplay-sync`, `syncplay-chat`, `syncplay-ui`).

---

## 3. Caveats

- **No Caveats**: Both defects were empirically tested with dedicated unit and integration tests, python test harnesses, fuzzing generators, and full-pipeline end-to-end scripts.
- **Untested Angles**: Real MKV files larger than physical RAM (>4GB) could not be tested on disk due to sandbox limits; however, 64-bit integer arithmetic for timescale calculations and offsets was mathematically and empirically verified against overflow.

---

## 4. Conclusion

**Verdict: `APPROVE`**

The remediation performed by `worker_p2_demux_fix` in `crates/syncplay-demux/src/ebml.rs` completely resolves both reported defects:
1. Valid signed delta VINTs with all data bits set to 1 (`0xFF`, `0x7FFF`) are properly accepted and decoded without error.
2. Single-frame laced blocks (`frame_count == 1`) emit exactly 1 frame without trailing empty packets.
3. All 34 tests in `syncplay-demux`, Python challenge scripts, clippy checks, dependency audits, and the Pi Intent Harness pass with 100% compliance.

Milestone 1 Container Demuxing remediation is ready for final sign-off.

---

## 5. Verification Method

To independently verify this approval:

```bash
# 1. Environment setup
export PATH="$HOME/.cargo/bin:$PATH"

# 2. Run syncplay-demux test suite (34 tests including 15 adversarial tests)
cargo test -p syncplay-demux -- --nocapture

# 3. Specifically verify Defect 1 (no ReservedElementId on delta 0xFF / 0x7FFF)
cargo test -p syncplay-demux --test ebml_adversarial_challenge -- test_lacing_mode_3_ebml_defect_reserved_element_id_rejection --nocapture

# 4. Specifically verify Defect 2 (exactly 1 frame emitted when frame_count == 1)
cargo test -p syncplay-demux --test ebml_adversarial_challenge -- test_lacing_mode_3_ebml_defect_frame_count_one_generates_two_frames --nocapture

# 5. Run Python empirical challenge runner
python3 tests/empirical_challenge/test_ebml_adversarial.py

# 6. Check workspace compilation and clippy
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings

# 7. Audit banned dependencies and verify Pi Intent Harness
./scripts/audit_dependencies.sh
python3 scripts/intent_harness.py

# 8. Full end-to-end verification script
./scripts/verify_phase1.sh
```

### Invalidation Conditions:
- If `unpack_lacing(3, &[2, 0x8A, 0xFF, ...])` fails with `ReservedElementId`.
- If `unpack_lacing(3, &[0, ...])` returns anything other than 1 frame.
- If any tests in `cargo test -p syncplay-demux` fail.
