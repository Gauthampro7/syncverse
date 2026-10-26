# Dispatch: Worker Fix (Phase 5 Challenger 1 Remediation)

## Objective
Remediate the integer subtraction overflow defect identified by Challenger 1 in `evaluate_file_match`:
1. In `crates/syncplay-ui/src/gui/sidebar.rs:144-147`:
   Replace:
   ```rust
   let duration_match = match (local_duration_ms, peer_duration_ms) {
       (Some(loc), Some(peer)) => (loc as i64 - peer as i64).abs() <= 2000,
       _ => false,
   };
   ```
   with:
   ```rust
   let duration_match = match (local_duration_ms, peer_duration_ms) {
       (Some(loc), Some(peer)) => loc.abs_diff(peer) <= 2000,
       _ => false,
   };
   ```
2. In `crates/syncplay-core/tests/adversarial_phase5_challenge.rs:376-387`:
   Update `test_file_match_cross_platform_path_basenames`:
   Change the overflow test so that instead of expecting a panic via `catch_unwind`, it asserts that `evaluate_file_match` handles `peer = 1u64 << 63` safely without panic and returns `FileMatchStatus::Mismatch`.
   Also add a test verifying `loc = u64::MAX` and `peer = 0` returns `FileMatchStatus::Mismatch`.
3. Run verification:
   - `cargo check --workspace --all-targets`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --test adversarial_phase5_challenge`
   - `cargo test --workspace`
   - `cargo test --test e2e_phase5`
   - `./scripts/audit_dependencies.sh`
   - `python3 scripts/intent_harness.py`

## References
- Challenger 1 handoff report: `/home/gautham/syncplay-alt/.agents/teamwork/challenger_1/handoff.md`
- Code file: `/home/gautham/syncplay-alt/crates/syncplay-ui/src/gui/sidebar.rs`
- Test file: `/home/gautham/syncplay-alt/crates/syncplay-core/tests/adversarial_phase5_challenge.rs`

## Invariants & Warnings
- Zero compiler warnings (`#![deny(warnings)]`).
- Clippy clean: `cargo clippy --workspace --all-targets -- -D warnings`.
- Zero banned dependencies (no `mpv`, `vlc`, `sqlite`, `rocksdb`).

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## Completion Criteria
All tests pass. Write completion report in `/home/gautham/syncplay-alt/.agents/teamwork/worker_fix/handoff.md` and message back.


## 2026-10-26T12:30:00Z
You are Worker Fix for Phase 5 Gate Remediation.
Your identity: worker_fix
Your working directory: /home/gautham/syncplay-alt/.agents/teamwork/worker_fix/
Read your dispatch assignment in /home/gautham/syncplay-alt/.agents/teamwork/worker_fix/DISPATCH.md and Challenger 1's handoff report in /home/gautham/syncplay-alt/.agents/teamwork/challenger_1/handoff.md.

Remediate the integer subtraction overflow defect in `crates/syncplay-ui/src/gui/sidebar.rs:145`:
- Replace `(loc as i64 - peer as i64).abs() <= 2000` with `loc.abs_diff(peer) <= 2000`.
- Update `crates/syncplay-core/tests/adversarial_phase5_challenge.rs:376` to verify it now safely returns `FileMatchStatus::Mismatch` instead of panicking on `peer = 1 << 63` and `loc = u64::MAX`.
- Run verification: `cargo check --workspace --all-targets`, `cargo test --test adversarial_phase5_challenge`, `cargo test --test e2e_phase5`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings`.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Produce your handoff report at /home/gautham/syncplay-alt/.agents/teamwork/worker_fix/handoff.md and message back when complete.
