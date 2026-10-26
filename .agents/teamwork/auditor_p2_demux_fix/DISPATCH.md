# Dispatch for auditor_p2_demux_fix

## Task
Forensic integrity audit of Milestone 1 EBML demuxer remediation (`crates/syncplay-demux/`).

## Inputs
- `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`
- `/home/gautham/syncplay-alt/AGENTS.md`
- `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux_fix/handoff.md`
- `crates/syncplay-demux/src/ebml.rs`

## Audit Checklist
1. Inspect `crates/syncplay-demux/src/ebml.rs`: verify genuine bug fix with zero cheats, zero hardcoded values.
2. Run `./scripts/audit_dependencies.sh`: verify zero banned external multimedia or database engines.
3. Check `#![deny(unsafe_code)]`, `#![deny(warnings)]`, `#![deny(missing_docs)]`.
4. Run `python3 scripts/intent_harness.py`: verify 100.0% compliance.
5. Run `./scripts/verify_phase1.sh`: verify all 8 stages pass cleanly.
6. Issue binary verdict: `CLEAN` or `INTEGRITY VIOLATION`.

## Deliverables
- Audit report at `/home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_demux_fix/audit_report.md`
- Formal handoff report at `/home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_demux_fix/handoff.md`

## 2026-10-25T10:04:24Z
You are auditor_p2_demux_fix, a teamwork_preview_auditor for Milestone 1 Remediation of Syncplay-Alt Phase 2.
Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_demux_fix/
Workspace root: /home/gautham/syncplay-alt

MANDATORY INSTRUCTIONS:
1. You MUST read /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically ## 2026-10-25T09:09:06Z) and /home/gautham/syncplay-alt/AGENTS.md before starting.
2. Read your task dispatch at /home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_demux_fix/DISPATCH.md.
3. Read the worker remediation report at /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux_fix/handoff.md.
4. Conduct a forensic integrity audit of the fixes in `crates/syncplay-demux/src/ebml.rs`:
   - Verify genuine fixes: zero cheating, zero hardcoding.
   - Run dependency whitelist audit: `./scripts/audit_dependencies.sh`.
   - Verify `#![deny(unsafe_code)]`, `#![deny(warnings)]`, `#![deny(missing_docs)]`.
   - Run `python3 scripts/intent_harness.py` (ensure 100.0% compliance).
   - Run `./scripts/verify_phase1.sh` (ensure all 8 stages pass).
5. Update progress in /home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_demux_fix/progress.md.
6. Write audit report to /home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_demux_fix/audit_report.md and formal handoff report to /home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_demux_fix/handoff.md with a binary verdict: CLEAN or INTEGRITY VIOLATION.
7. Send a message to your parent orchestrator with your verdict and handoff path.
