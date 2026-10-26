# Dispatch for auditor_p2_presentation

## Task
Forensic integrity audit of Milestone 2 (`crates/syncplay-codecs`, `crates/syncplay-clock`, `crates/syncplay-ui`).

## Inputs
- `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`
- `/home/gautham/syncplay-alt/AGENTS.md`
- `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_presentation/handoff.md`
- `crates/syncplay-codecs/`
- `crates/syncplay-clock/`
- `crates/syncplay-ui/`

## Forensic Audit Checklist
1. Inspect code changes: verify genuine logic with zero hardcoded results, zero dummy stubs, zero shortcuts.
2. Run `./scripts/audit_dependencies.sh`: verify zero banned external multimedia or database engines.
3. Check safety invariants: `#![deny(unsafe_code)]` outside justified SIMD/SPSC primitives with `// SAFETY:` proofs.
4. Verify `#![deny(warnings)]` and `#![deny(missing_docs)]`.
5. Run `python3 scripts/intent_harness.py`: verify 100.0% compliance.
6. Run `./scripts/verify_phase1.sh`: verify all 8 stages pass cleanly.
7. Issue binary verdict: `CLEAN` or `INTEGRITY VIOLATION`.

## Deliverables
- Audit report at `/home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_presentation/audit_report.md`
- Formal handoff report at `/home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_presentation/handoff.md`

## 2026-10-25T10:23:51Z
You are auditor_p2_presentation, a teamwork_preview_auditor for Milestone 2 of Syncplay-Alt Phase 2.
Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_presentation/
Workspace root: /home/gautham/syncplay-alt

MANDATORY INSTRUCTIONS:
1. You MUST read /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically ## 2026-10-25T09:09:06Z) and /home/gautham/syncplay-alt/AGENTS.md before starting.
2. Read your task dispatch at /home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_presentation/DISPATCH.md.
3. Read the worker handoff report at /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_presentation/handoff.md.
4. Conduct a forensic integrity audit across `crates/syncplay-codecs`, `crates/syncplay-clock`, and `crates/syncplay-ui`:
   - Static analysis: verify genuine Q8 color conversion unrolling, SPSC atomics, SeqLock re-anchoring, and debouncer state machines. Zero hardcoded outputs, zero facade implementations.
   - Run dependency whitelist audit: `./scripts/audit_dependencies.sh`. Zero banned external media/codec/database engines in Cargo.toml or Cargo.lock.
   - Check strict safety invariants: verify any `unsafe` blocks in SIMD/SPSC have formal `// SAFETY:` proof comments.
   - Verify `#![deny(warnings)]` and `#![deny(missing_docs)]`.
   - Run `python3 scripts/intent_harness.py`: verify 100.0% compliance score across 8/8 core intents.
   - Run full verification harness: `./scripts/verify_phase1.sh`.
5. Update your progress in /home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_presentation/progress.md.
6. Write your audit report to /home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_presentation/audit_report.md and formal handoff report to /home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_presentation/handoff.md with a binary verdict: CLEAN or INTEGRITY VIOLATION.
7. Send a message to your parent orchestrator with your verdict and handoff path.
