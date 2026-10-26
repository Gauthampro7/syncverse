# Dispatch Instructions — challenger_p2_core

## Mission
Perform empirical adversarial testing of `crates/syncplay-core`:
- Stress-test the executable CLI binary (`target/debug/syncplay-alt` or `cargo run -p syncplay-core -- ...`) across all subcommands (`inspect`, `play`, `party`, `chat demo|log|query`, `verify`).
- Test edge cases: non-existent files, empty files, malformed args, simulated network conditions.
- Test `PlayerCoordinator` and `TelemetryAggregator` under rapid command sequences and seek operations.
- Test `ChatStore` CLI integration with binary persistence.
- Run `intent_harness.py` and `verify_phase1.sh`.
- Deliver an empirical challenge report with explicit verdict (**APPROVE** or **REQUEST_CHANGES**).

## Authoritative Reference
- Read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`
- Read `/home/gautham/syncplay-alt/AGENTS.md`
- Read `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_core_2/handoff.md`

## Output Requirements
- Write report to `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_core/handoff.md` with:
  1. Observation
  2. Logic Chain
  3. Caveats
  4. Conclusion (Explicit Verdict: APPROVE or REQUEST_CHANGES)
  5. Verification Method
- Send completion message to orchestrator.


## 2026-10-25T14:25:09Z
You are challenger_p2_core, working in /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_core/.
Read /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_core/DISPATCH.md, /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md, and /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_core_2/handoff.md.
Perform adversarial and empirical testing on `crates/syncplay-core` CLI subcommands, error handling on bad inputs, coordinator pipeline under rapid seeks, and chat persistence.
Run the Intent Verification Harness and verify_phase1.sh.
Deliver your report to /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_core/handoff.md and report your explicit verdict (APPROVE or REQUEST_CHANGES).


## 2026-10-25T14:32:40Z
**Context**: Milestone 5 Gate Verification
**Content**: Status query on your empirical and adversarial testing of `crates/syncplay-core` CLI subcommands and coordinator. Both `auditor_p2_core` (CLEAN) and `reviewer_p2_core` (APPROVE) have completed their evaluations.
**Action**: Please provide an update on your progress or deliver your handoff report and explicit verdict.
