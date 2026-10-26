# Dispatch for challenger_p2_presentation_1

## Task
Empirical adversarial challenge testing of Milestone 2 Codecs and Clock subsystems (`crates/syncplay-codecs`, `crates/syncplay-clock`).

## Inputs
- `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`
- `/home/gautham/syncplay-alt/AGENTS.md`
- `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_presentation/handoff.md`
- `crates/syncplay-codecs/`
- `crates/syncplay-clock/`

## Adversarial Focus
1. Codecs:
   - Fuzz frame geometry with odd widths and heights (e.g. 1x1, 3x3, 17x19, 1921x1081). Verify zero buffer over-reads with 16-byte stride alignment and $+16$B SIMD tail padding.
   - Stress test Q8 fixed-point arithmetic under extreme luma/chroma values (0, 16, 128, 235, 240, 255) to verify absence of integer wrapping.
2. Clock:
   - Concurrency stress test: multi-threaded reader/writer on `MasterClock` SeqLock during rapid speed oscillations ($0.96 \to 1.04$ at 1 kHz). Verify zero torn reads and $C^0$ continuity.
   - SPSC ring buffer: high-throughput producer/consumer thread race testing capacity saturation and wrapping counters.
   - Bounded 16-frame reorder buffer: burst arrival of out-of-order frames, verifying max capacity clamping and strict ascending PTS eviction.
3. Issue formal verdict: `APPROVE` or `REQUEST_CHANGES`.

## Deliverables
- Challenge report at `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_presentation_1/challenge_report.md`
- Formal handoff report at `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_presentation_1/handoff.md`

## 2026-10-25T10:23:34Z
You are challenger_p2_presentation_1, a teamwork_preview_challenger for Milestone 2 of Syncplay-Alt Phase 2.
Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_presentation_1/
Workspace root: /home/gautham/syncplay-alt

MANDATORY INSTRUCTIONS:
1. You MUST read /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically ## 2026-10-25T09:09:06Z) and /home/gautham/syncplay-alt/AGENTS.md before starting.
2. Read your task dispatch at /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_presentation_1/DISPATCH.md.
3. Read the worker handoff report at /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_presentation/handoff.md.
4. Execute empirical adversarial challenge testing against `crates/syncplay-codecs/` and `crates/syncplay-clock/`:
   - Codecs: Frame geometry stress testing with odd dimensions (1x1, 3x3, 17x19, 1921x1081), verifying zero buffer over-reads with 16-byte stride alignment and +16B SIMD tail padding. Q8 fixed-point arithmetic under extreme boundary values.
   - Clock: Multi-threaded reader/writer race testing on MasterClock SeqLock during rapid speed oscillations (0.96 to 1.04 at 1 kHz), verifying C^0 continuity and zero torn reads. High-throughput producer/consumer SPSC ring buffer stress test. Bounded 16-frame reorder buffer capacity and sorting invariants.
5. Update your progress in /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_presentation_1/progress.md.
6. Write your challenge report to /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_presentation_1/challenge_report.md and formal handoff report to /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_presentation_1/handoff.md with a clear verdict: APPROVE or REQUEST_CHANGES.
7. Send a message to your parent orchestrator with your verdict and handoff path.
