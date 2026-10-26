# Dispatch for reviewer_p2_demux_1

## Task
Review Milestone 1 Matroska/EBML container demuxing implementation under `crates/syncplay-demux/src/ebml.rs`.

## Inputs
- `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`
- `/home/gautham/syncplay-alt/AGENTS.md`
- `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux/handoff.md`
- `crates/syncplay-demux/src/ebml.rs`
- `crates/syncplay-demux/src/synthetic.rs`
- `crates/syncplay-demux/tests/demux_tests.rs`

## Review Checklist
1. Correctness of EBML element hierarchy parsing (`Segment/Info`, `Segment/Tracks`, `Segment/Cues`).
2. Correctness of frame lacing unpacker across all 4 modes (No lacing, Xiph, Fixed-size, EBML signed delta VINTs).
3. Correctness of `seek_to(pts)` binary search over `CueEntry`s and queue invalidation.
4. Conformance to zero-warning `#![deny(warnings)]` and `#![deny(missing_docs)]`.
5. Run workspace build, unit tests, clippy, and intent harness.
6. Issue formal verdict: `APPROVE` or `REQUEST_CHANGES`.

## Deliverables
- Review report at `/home/gautham/syncplay-alt/.agents/teamwork/reviewer_p2_demux_1/review_report.md`
- Handoff report at `/home/gautham/syncplay-alt/.agents/teamwork/reviewer_p2_demux_1/handoff.md`

## 2026-10-25T09:42:11Z
From: 0644b5fa-9f3b-44ab-92cd-57079ebc4b70
Content:
You are reviewer_p2_demux_1, a teamwork_preview_reviewer for Milestone 1 of Syncplay-Alt Phase 2.
Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/reviewer_p2_demux_1/
Workspace root: /home/gautham/syncplay-alt

MANDATORY INSTRUCTIONS:
1. You MUST read /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically ## 2026-10-25T09:09:06Z) and /home/gautham/syncplay-alt/AGENTS.md before starting.
2. Read your task dispatch at /home/gautham/syncplay-alt/.agents/teamwork/reviewer_p2_demux_1/DISPATCH.md.
3. Read the worker handoff report at /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux/handoff.md.
4. Review the Matroska/EBML container demuxing implementation under `crates/syncplay-demux/src/ebml.rs`:
   - Inspect element hierarchy parsing, 4-mode frame lacing unpacker (No lacing, Xiph, Fixed, EBML signed delta VINTs), internal packet buffering, and Cues-based keyframe seeking.
   - Run verification commands: `cargo check --workspace`, `cargo clippy -p syncplay-demux --all-targets -- -D warnings`, `cargo test -p syncplay-demux -- --nocapture`.
5. Update your progress in /home/gautham/syncplay-alt/.agents/teamwork/reviewer_p2_demux_1/progress.md.
6. Write your review report to /home/gautham/syncplay-alt/.agents/teamwork/reviewer_p2_demux_1/review_report.md and formal handoff report to /home/gautham/syncplay-alt/.agents/teamwork/reviewer_p2_demux_1/handoff.md with a clear verdict: APPROVE or REQUEST_CHANGES.
7. Send a message to your parent orchestrator with your verdict and handoff path.
