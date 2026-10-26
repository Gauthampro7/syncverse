# Dispatch: challenger_p3_audio_1

## Identity
- Role: Adversarial Challenger (Milestone 2 Resampler Stress & Precision Fuzzing)
- Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/challenger_p3_audio_1
- Authoritative User Request: /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md
- Worker Handoff: /home/gautham/syncplay-alt/.agents/teamwork/worker_p3_audio/handoff.md

## Mission
Design and execute empirical stress tests against `LinearResampler`:
1. Resampling ratio boundaries: test 8kHz -> 192kHz, 192kHz -> 8kHz, 44.1kHz -> 48kHz, 48kHz -> 44.1kHz.
2. Speed ratio sweeps: sweep speed ratio across $r \in [0.96, 1.04]$ and extreme ratios ($0.5x, 2.0x$).
3. Irregular chunk boundaries: test 1-sample chunks, prime-sized chunks (3, 7, 13, 31, 1024), verify zero phase discontinuities or audio clicks.
4. Hot loop allocation verification: verify `resample_into` performs 0 heap allocations when given pre-allocated output vector.
5. Empty and corrupt inputs: verify no divide-by-zero or panics on zero rates or empty slices.

## Deliverables
Write test harness, run tests, document results in `challenge_report.md` and complete with `handoff.md` with explicit APPROVE or REQUEST_CHANGES verdict, and report back to orchestrator_3.

## 2026-10-25T16:47:39Z
You are challenger_p3_audio_1, a code-executing adversarial challenger for Milestone 2 of Syncplay-Alt Phase 3.
Your working directory is /home/gautham/syncplay-alt/.agents/teamwork/challenger_p3_audio_1.
The workspace root is /home/gautham/syncplay-alt.

Read your dispatch instructions in:
/home/gautham/syncplay-alt/.agents/teamwork/challenger_p3_audio_1/DISPATCH.md
the authoritative user request in:
/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically the latest request under ## 2026-10-25T15:21:44Z),
and the worker handoff in:
/home/gautham/syncplay-alt/.agents/teamwork/worker_p3_audio/handoff.md.

Design and execute empirical stress tests against `LinearResampler`:
1. Resampling ratio boundaries: test 8kHz -> 192kHz, 192kHz -> 8kHz, 44.1kHz -> 48kHz, 48kHz -> 44.1kHz.
2. Speed ratio sweeps: sweep speed ratio across $r \in [0.96, 1.04]$ and extreme ratios ($0.5x, 2.0x$).
3. Irregular chunk boundaries: test 1-sample chunks, prime-sized chunks (3, 7, 13, 31, 1024), verify zero phase discontinuities or audio clicks.
4. Hot loop allocation verification: verify `resample_into` performs 0 heap allocations when given pre-allocated output vector.
5. Empty and corrupt inputs: verify no divide-by-zero or panics on zero rates or empty slices.

Write test harness, run tests, document results in `challenge_report.md` and complete with `handoff.md` with explicit APPROVE or REQUEST_CHANGES verdict, and report back to orchestrator_3 (my conversation ID is ecd27bad-3488-46aa-ba26-8b0b7cecf0eb).
