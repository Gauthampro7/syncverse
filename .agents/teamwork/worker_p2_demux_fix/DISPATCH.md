# Dispatch for worker_p2_demux_fix

## Task
Remediate the two EBML frame lacing defects identified by `challenger_p2_demux_1` in `crates/syncplay-demux/src/ebml.rs`.

## Exclusive Write Ownership
- `crates/syncplay-demux/src/ebml.rs`
- `crates/syncplay-demux/tests/`

## Inputs
- `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`
- `/home/gautham/syncplay-alt/AGENTS.md`
- `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_1/handoff.md`
- `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_1/challenge_report.md`
- `crates/syncplay-demux/tests/ebml_adversarial_challenge.rs`

## Required Fixes in `crates/syncplay-demux/src/ebml.rs`
1. **EBML Signed Delta VINT Decoding (Line 172)**:
   - In `unpack_lacing`, signed delta VINTs are decoded with `parse_vint(&payload[offset..offset + w], true)?`.
   - The second parameter `true` signifies `is_element_id`, which rejects any VINT whose data bits are all 1 as `VintError::ReservedElementId`.
   - In EBML lacing, delta values are signed integers, NOT element IDs. An all-ones value like `0xFF` represents $+64$ (valid maximum 1-octet delta).
   - Change `parse_vint(&payload[offset..offset + w], true)` to `parse_vint(&payload[offset..offset + w], false)` so that all data bit combinations are accepted.
2. **Lacing with `frame_count == 1` (Lines 141-209)**:
   - When `payload[0] == 0` (`frame_count = 1`), `unpack_lacing` currently parses `prev_size`, skips the empty delta loop, and then computes and appends `last_sz = payload.len() - offset - sum_sizes`, pushing 2 frames (the first frame + an empty 0-byte frame).
   - For `frame_count == 1`, either:
     - Check if `frame_count == 1`: return `vec![payload[1..].to_vec()]` immediately (or validate `frame_count >= 2` and return `Err(DemuxError::CorruptedBlock("Laced block must contain at least 2 frames"))` if single-frame lacing is disallowed). Note: in Matroska, if lacing flags are set but frame count is 1, emitting exactly the 1 frame without phantom trailing frames is correct.
3. **Verification**:
   - Run `cargo test -p syncplay-demux -- --nocapture` (all unit and integration tests including `ebml_adversarial_challenge.rs` must pass).
   - Run `python3 tests/empirical_challenge/test_ebml_adversarial.py`.
   - Run `cargo clippy -p syncplay-demux --all-targets -- -D warnings`.
   - Run `python3 scripts/intent_harness.py`.
   - Run `./scripts/verify_phase1.sh`.

## Deliverables
- Fix applied in `crates/syncplay-demux/src/ebml.rs`
- Progress updates in `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux_fix/progress.md`
- Formal handoff report at `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux_fix/handoff.md`


## 2026-10-25T09:57:34Z
You are worker_p2_demux_fix, a teamwork_preview_worker for Milestone 1 Remediation of Syncplay-Alt Phase 2.
Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux_fix/
Workspace root: /home/gautham/syncplay-alt

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

MANDATORY INSTRUCTIONS:
1. You MUST read /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically ## 2026-10-25T09:09:06Z) and /home/gautham/syncplay-alt/AGENTS.md before starting.
2. Read your task dispatch at /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux_fix/DISPATCH.md.
3. Read the challenger report at /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_1/handoff.md and /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_1/challenge_report.md.
4. Apply the two fixes to `crates/syncplay-demux/src/ebml.rs`:
   - Change `parse_vint(&payload[offset..offset + w], true)` to `false` for signed delta VINTs in `unpack_lacing` so deltas with all data bits = 1 (like 0xFF for +64) are not rejected.
   - Fix single-frame lacing (`frame_count == 1`) so it does not append an empty trailing frame.
5. Verification:
   - Run `export PATH="$HOME/.cargo/bin:$PATH"`
   - Run `cargo test -p syncplay-demux -- --nocapture` (including `ebml_adversarial_challenge.rs`)
   - Run `python3 tests/empirical_challenge/test_ebml_adversarial.py`
   - Run `cargo clippy -p syncplay-demux --all-targets -- -D warnings`
   - Run `./scripts/audit_dependencies.sh`
   - Run `python3 scripts/intent_harness.py`
   - Run `./scripts/verify_phase1.sh`
6. Update progress in /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux_fix/progress.md.
7. Write formal handoff report to /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux_fix/handoff.md.
8. Send a message to your parent orchestrator with the verification results and handoff path.
