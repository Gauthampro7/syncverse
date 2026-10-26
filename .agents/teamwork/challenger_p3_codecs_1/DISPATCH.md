# Dispatch: challenger_p3_codecs_1

## Identity
- Role: Adversarial Challenger (M1 Bitstream & Exp-Golomb Fuzzing)
- Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/challenger_p3_codecs_1
- Authoritative User Request: /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md
- Worker Handoff: /home/gautham/syncplay-alt/.agents/teamwork/worker_p3_codecs/handoff.md

## Mission
Design empirical stress and fuzzing tests against the H.264 bitstream parser:
1. Exp-Golomb bitstream edge cases: extreme leading zero counts (> 31), bitstream truncation, `read_te` with `max <= 1`, negative `read_se` boundaries.
2. NAL parser fuzzing: corrupt start codes, trailing zeros, malformed AVCC length headers, emulation prevention bytes `0x000003` at buffer boundaries.
3. SPS/PPS fuzzing: malformed crop offsets, huge macroblock width/height (check for integer overflow in visible dimensions), unknown NAL types.
4. Verify memory bounds and safety (no panics, returns strongly typed `CodecError`).

## Deliverables
Write test harness, run it, document results in `challenge_report.md` and `handoff.md` with explicit APPROVE or REQUEST_CHANGES verdict, and report back to orchestrator_3.

## 2026-10-25T15:59:19Z
You are challenger_p3_codecs_1, a code-executing adversarial challenger for Milestone 1 of Syncplay-Alt Phase 3.
Your working directory is /home/gautham/syncplay-alt/.agents/teamwork/challenger_p3_codecs_1.
The workspace root is /home/gautham/syncplay-alt.

Read your dispatch instructions in:
/home/gautham/syncplay-alt/.agents/teamwork/challenger_p3_codecs_1/DISPATCH.md
the authoritative user request in:
/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically the latest request under ## 2026-10-25T15:21:44Z),
and the worker handoff in:
/home/gautham/syncplay-alt/.agents/teamwork/worker_p3_codecs/handoff.md.

Design and execute empirical stress tests and fuzzing against the H.264 bitstream parser:
1. Exp-Golomb bitstream edge cases: extreme leading zero counts (> 31), bitstream truncation, `read_te` with `max <= 1`, negative `read_se` boundaries.
2. NAL parser fuzzing: corrupt start codes, trailing zeros, malformed AVCC length headers, emulation prevention bytes `0x000003` at buffer boundaries.
3. SPS/PPS fuzzing: malformed crop offsets, huge macroblock width/height (check for integer overflow in visible dimensions), unknown NAL types.
4. Verify memory bounds and safety (no panics, returns strongly typed `CodecError`).

Write test harness, run it, document results in `challenge_report.md` and complete with `handoff.md` with an explicit verdict: APPROVE or REQUEST_CHANGES.
Report back to orchestrator_3 (my conversation ID is ecd27bad-3488-46aa-ba26-8b0b7cecf0eb).
