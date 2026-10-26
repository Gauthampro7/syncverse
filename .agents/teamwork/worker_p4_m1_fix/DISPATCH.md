# DISPATCH — worker_p4_m1_fix

## 2026-10-25T21:02:59Z

## Mission
You are `worker_p4_m1_fix`, the remediation worker for Milestone 1.
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m1_fix`.

You must read:
1. `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically `## 2026-10-25T20:12:42Z`)
2. `/home/gautham/syncplay-alt/.agents/teamwork/orchestrator_4/PROJECT.md`
3. Fix reports and patches:
   - `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_fix_1/handoff.md` and `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_fix_1/cli_and_coordinator_integration.patch`
   - `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_fix_2/handoff.md` and `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_fix_2/test_ui_renderer_fix.patch`
   - `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_fix_3/handoff.md`

## Mandatory Integrity Warning
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## Tasks & Implementation Steps
1. Apply Clippy fix to `crates/syncplay-core/tests/cli_and_coordinator_integration.rs`:
   - Apply `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_fix_1/cli_and_coordinator_integration.patch` (or hoist `normalize_target_path` to lines 15–17 and change lines 461 and 527 to `use syncplay_core::InteractivePlayer;`).
2. Apply Viewport 0x0 division guard & edge case assertions to `tests/empirical_challenge/test_ui_renderer.py`:
   - Apply `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_fix_2/test_ui_renderer_fix.patch` (or copy `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_fix_2/proposed_test_ui_renderer.py` to `tests/empirical_challenge/test_ui_renderer.py`).
3. Update `INTENT-08` in `.pi/harness/intent_matrix.json`:
   - Ensure the verification command includes clippy warning check:
     `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -- --nocapture`
4. Run full verification suite:
   - `cargo clippy --workspace --all-targets -- -D warnings` (must exit 0 with 0 warnings)
   - `cargo check --workspace --all-targets` (must exit 0 with 0 warnings)
   - `cargo test --workspace` (must pass 100% of tests)
   - `python3 tests/empirical_challenge/test_ui_renderer.py` (must pass with 0 errors)
   - `python3 scripts/intent_harness.py` (must score 100.0% compliance across all 8 intents)
   - `./scripts/audit_dependencies.sh` (clean, 0 violations)
   - `./scripts/verify_phase4.sh` (all 8 stages pass cleanly)

## Output Requirements
Document all actions taken, files modified, and exact command outputs in `/home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m1_fix/handoff.md`.
Send a completion message to the orchestrator when finished.
