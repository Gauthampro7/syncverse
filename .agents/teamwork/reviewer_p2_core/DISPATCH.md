# Dispatch Instructions — reviewer_p2_core

## Mission
Perform high-reliability code review and adversarial challenge of `crates/syncplay-core`:
- Review `crates/syncplay-core/src/main.rs`, `coordinator.rs`, `aggregator.rs`, `lib.rs`, and `tests/cli_and_coordinator_integration.rs`.
- Verify all CLI subcommands (`inspect`, `play`, `party host|connect`, `chat demo|log|query`, `verify`).
- Check zero compiler warnings, zero clippy warnings (`-D warnings`), zero banned crates.
- Run tests and deliver an objective handoff report with explicit verdict (**APPROVE** or **REQUEST_CHANGES**).

## Authoritative Reference
- Read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`
- Read `/home/gautham/syncplay-alt/AGENTS.md`
- Read `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_core_2/handoff.md`

## Output Requirements
- Write report to `/home/gautham/syncplay-alt/.agents/teamwork/reviewer_p2_core/handoff.md` with:
  1. Observation
  2. Logic Chain
  3. Caveats
  4. Conclusion (Explicit Verdict: APPROVE or REQUEST_CHANGES)
  5. Verification Method
- Send completion message to orchestrator.

## 2026-10-25T14:25:03Z
You are reviewer_p2_core, working in /home/gautham/syncplay-alt/.agents/teamwork/reviewer_p2_core/.
Read /home/gautham/syncplay-alt/.agents/teamwork/reviewer_p2_core/DISPATCH.md, /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md, and /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_core_2/handoff.md.
Review `crates/syncplay-core` (CLI subcommands, coordinator, telemetry aggregator, and integration tests).
Verify zero compiler warnings, zero clippy warnings (-D warnings), zero banned dependencies, and 100% test pass rate.
Deliver your handoff report to /home/gautham/syncplay-alt/.agents/teamwork/reviewer_p2_core/handoff.md and report your explicit verdict (APPROVE or REQUEST_CHANGES).
