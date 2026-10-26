# Dispatch Instructions — auditor_p2_core

## Mission
Conduct a strict Forensic Integrity and Dependency Audit of `crates/syncplay-core` and the entire workspace:
1. Verify ZERO banned multimedia/codec/database engines (`ffmpeg`, `vlc`, `mpv`, `symphonia`, `dav1d`, `sqlite`, `rocksdb`, `tantivy`, etc.) via `./scripts/audit_dependencies.sh`.
2. Static analysis: Detect any hardcoded outputs, fake facade mocks, `todo!`, `unimplemented!`, or circumvented tasks.
3. Execution verification: Run `cargo check`, `cargo clippy -- -D warnings`, `cargo test --workspace`, `python3 scripts/intent_harness.py`, and `./scripts/verify_phase1.sh`.
4. Deliver an authoritative Forensic Audit Report with a binary verdict: **`CLEAN`** or **`INTEGRITY VIOLATION`**.

## Authoritative Reference
- Read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`
- Read `/home/gautham/syncplay-alt/AGENTS.md`
- Read `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_core_2/handoff.md`

## Output Requirements
- Write report to `/home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_core/handoff.md`.
- Send completion message to orchestrator with explicit verdict (**CLEAN** or **INTEGRITY VIOLATION**).

## 2026-10-25T14:25:15Z

You are auditor_p2_core, working in /home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_core/.
Read /home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_core/DISPATCH.md, /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md, and /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_core_2/handoff.md.
Conduct a strict Forensic Integrity and Dependency Audit across `crates/syncplay-core` and the entire workspace:
1. Verify ./scripts/audit_dependencies.sh reports 0 banned crates.
2. Check for hardcoded test outputs, dummy mock facades, todo!, or unimplemented!.
3. Run cargo check, cargo clippy (-D warnings), cargo test --workspace, python3 scripts/intent_harness.py, and ./scripts/verify_phase1.sh.
Deliver your report to /home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_core/handoff.md and report your explicit binary verdict (CLEAN or INTEGRITY VIOLATION).
