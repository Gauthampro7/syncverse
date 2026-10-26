# Dispatch for challenger_p2_demux_2

## Task
Empirical adversarial challenge of Milestone 1 ISOBMFF/MP4 parser (`crates/syncplay-demux/src/isobmff.rs`).

## Inputs
- `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`
- `/home/gautham/syncplay-alt/AGENTS.md`
- `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux/handoff.md`
- `crates/syncplay-demux/src/isobmff.rs`

## Adversarial Focus
1. Construct fuzz/stress tests for ISOBMFF box parsing:
   - 64-bit largesize (`size == 1`), 64-bit chunk offsets (`co64`), truncated headers, invalid chunk-to-sample runs.
2. Stress test multi-track interleaved reading:
   - Multi-track mdat interleaving where sample offsets alternate across tracks.
3. Stress test `seek_to(pts)`:
   - Stream with no sync sample box (`stss`) vs stream with sparse `stss`.
   - Seeking past end of stream, seeking to 0, seek alignment across video and audio tracks.
4. Issue formal verdict: `APPROVE` or `REQUEST_CHANGES`.

## Deliverables
- Challenge report at `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_2/challenge_report.md`
- Handoff report at `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_2/handoff.md`

## 2026-10-25T09:42:32Z
You are challenger_p2_demux_2, a teamwork_preview_challenger for Milestone 1 of Syncplay-Alt Phase 2.
Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_2/
Workspace root: /home/gautham/syncplay-alt

MANDATORY INSTRUCTIONS:
1. You MUST read /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically ## 2026-10-25T09:09:06Z) and /home/gautham/syncplay-alt/AGENTS.md before starting.
2. Read your task dispatch at /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_2/DISPATCH.md.
3. Read the worker handoff report at /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux/handoff.md.
4. Execute empirical adversarial challenge testing against `crates/syncplay-demux/src/isobmff.rs`:
   - Fuzz/stress test ISOBMFF box parser: 64-bit largesize bounds, co64 64-bit offsets, truncated headers, corrupted stsc runs.
   - Stress test multi-track interleaved reading across active tracks from mdat.
   - Stress test keyframe seeking with stss and fallback cases.
5. Update your progress in /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_2/progress.md.
6. Write your challenge report to /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_2/challenge_report.md and formal handoff report to /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_2/handoff.md with a clear verdict: APPROVE or REQUEST_CHANGES.
7. Send a message to your parent orchestrator with your verdict and handoff path.

## 2026-10-25T09:52:34Z
[Message] timestamp=2026-10-25T09:52:34Z sender=0644b5fa-9f3b-44ab-92cd-57079ebc4b70 priority=MESSAGE_PRIORITY_HIGH content=**Context**: Milestone 1 ISOBMFF empirical adversarial challenge.
**Content**: Checking in on your progress with the adversarial test suite for ISOBMFF.
**Action**: Please update your progress and let us know your status.

## 2026-10-25T09:56:30Z
[Message] timestamp=2026-10-25T09:56:30Z sender=0644b5fa-9f3b-44ab-92cd-57079ebc4b70 priority=MESSAGE_PRIORITY_HIGH content=**Context**: ISOBMFF empirical adversarial challenge.
**Content**: We observed your extensive adversarial test suite `isobmff_adversarial_challenge.rs`. Are the test runs complete, and are you compiling your report?
**Action**: Please deliver your challenge report and handoff report.
