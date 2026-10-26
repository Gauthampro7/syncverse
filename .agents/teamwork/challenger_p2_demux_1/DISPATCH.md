# Dispatch for challenger_p2_demux_1

## Task
Empirical adversarial challenge of Milestone 1 Matroska/EBML parser (`crates/syncplay-demux/src/ebml.rs`).

## Inputs
- `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`
- `/home/gautham/syncplay-alt/AGENTS.md`
- `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux/handoff.md`
- `crates/syncplay-demux/src/ebml.rs`

## Adversarial Focus
1. Construct fuzz/stress tests for EBML frame lacing unpacking:
   - Malformed/truncated laced packets in No-lacing, Xiph, Fixed-size, and EBML delta modes.
   - VINT length overflows and negative signed delta edge cases.
2. Stress test `seek_to(pts)`:
   - Seeking before first cue, between cues, exactly on a cue, after last cue, and seeking in streams with 0 cues.
3. Multi-track interleaved packet queue invariants:
   - Ensure queue ordering, track ID separation, and absence of memory leaks or infinite loops on corrupted simpleblocks.
4. Issue formal verdict: `APPROVE` or `REQUEST_CHANGES`.

## Deliverables
- Challenge report at `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_1/challenge_report.md`
- Handoff report at `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_1/handoff.md`


## 2026-10-25T09:42:25Z
You are challenger_p2_demux_1, a teamwork_preview_challenger for Milestone 1 of Syncplay-Alt Phase 2.
Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_1/
Workspace root: /home/gautham/syncplay-alt

MANDATORY INSTRUCTIONS:
1. You MUST read /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically ## 2026-10-25T09:09:06Z) and /home/gautham/syncplay-alt/AGENTS.md before starting.
2. Read your task dispatch at /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_1/DISPATCH.md.
3. Read the worker handoff report at /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux/handoff.md.
4. Execute empirical adversarial challenge testing against `crates/syncplay-demux/src/ebml.rs`:
   - Fuzz/stress test frame lacing unpacking with malformed payloads, truncated buffers, and VINT overflows.
   - Stress test Cues-based seeking across boundary conditions (before first cue, after last cue, between cues, 0 cues).
   - Verify multi-track interleaved packet queue invariants.
5. Update your progress in /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_1/progress.md.
6. Write your challenge report to /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_1/challenge_report.md and formal handoff report to /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_1/handoff.md with a clear verdict: APPROVE or REQUEST_CHANGES.
7. Send a message to your parent orchestrator with your verdict and handoff path.
