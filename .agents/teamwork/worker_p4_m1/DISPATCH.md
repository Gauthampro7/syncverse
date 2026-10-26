## 2026-10-25T20:37:04Z

# DISPATCH — worker_p4_m1

## Mission
You are `worker_p4_m1`, the implementation worker for Milestone 1: Specification, Audits & Intent Harness Realignment (R3).
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m1`.

You must read:
1. `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically `## 2026-10-25T20:12:42Z`)
2. `/home/gautham/syncplay-alt/.agents/teamwork/orchestrator_4/PROJECT.md`
3. Explorer handoffs:
   - `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_1/handoff.md`
   - `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_2/handoff.md`
   - `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_3/handoff.md`

## Mandatory Integrity Warning
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## Tasks & Implementation Steps
1. Apply specification realignment from `explorer_p4_m1_1`:
   - Apply `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_1/specification_realignment.patch` (or copy `proposed_AGENTS.md` to `AGENTS.md`, `proposed_openspec_project.md` to `openspec/project.md`, `proposed_ui_renderer_spec.md` to `openspec/specs/ui-renderer/spec.md`).
2. Apply dependency audit realignment & setup script from `explorer_p4_m1_2`:
   - Apply `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_2/audit_dependencies.patch` (or copy `proposed_audit_dependencies.sh` to `scripts/audit_dependencies.sh`).
   - Copy `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_2/proposed_setup_deps.sh` to `scripts/setup_deps.sh`.
   - Make scripts executable: `chmod +x scripts/*.sh`.
   - Run `./scripts/setup_deps.sh` to ensure non-root sysroot (`.deps/`) is initialized and `.cargo/config.toml` is configured for `ffmpeg-next`.
3. Apply intent harness realignment from `explorer_p4_m1_3`:
   - Apply patches from `explorer_p4_m1_3`:
     * `intent_harness.patch` (or copy `proposed_intent_harness.py` to `scripts/intent_harness.py`)
     * `intent_matrix.patch` (or copy `proposed_intent_matrix.json` to `.pi/harness/intent_matrix.json`)
     * `test_ui_renderer.patch` (or copy `proposed_test_ui_renderer.py` to `tests/empirical_challenge/test_ui_renderer.py`)
     * `intent_guardian.patch` (or copy `proposed_intent_guardian.md` to `.pi/agents/intent_guardian.md`)
     * `skill_intent_harness.patch` (or copy `proposed_skill_intent_harness.md` to `.pi/skills/intent_verification_harness.md`)
     * Copy `proposed_verify_phase4.sh` to `scripts/verify_phase4.sh` and make executable (`chmod +x scripts/verify_phase4.sh`).
4. Execute full verification suite:
   - `./scripts/audit_dependencies.sh` (must exit 0)
   - `python3 scripts/intent_harness.py` (must score 100.0% compliance across all 8 intents)
   - `python3 tests/empirical_challenge/test_ui_renderer.py` (must pass all tests including Test 9)
   - `cargo check --workspace --all-targets` (must compile with 0 warnings)
   - `cargo test --workspace` (must pass 100% of tests)
   - `cargo clippy --workspace --all-targets -- -D warnings` (must exit 0 with 0 warnings)
   - `./scripts/verify_phase4.sh` (all stages pass)

## Output Requirements
Document all actions taken, files modified, and exact command outputs in `/home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m1/handoff.md`.
Send a completion message to the orchestrator when finished.
