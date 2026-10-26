# Dispatch for explorer_p2_survey_1

## Task
Deep technical survey of R1: Full Container Demuxing & Packet Processing (`001-demuxer-bitstream`) under `crates/syncplay-demux`.

## Inputs
- `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`
- `/home/gautham/syncplay-alt/AGENTS.md`
- `crates/syncplay-demux/`
- `crates/syncplay-common/`
- `openspec/specs/demuxer/spec.md`
- `openspec/changes/001-demuxer-bitstream.md`
- `tests/empirical_challenge/test_demux_codecs_clock.py`

## Deliverables
- Survey report at `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p2_survey_1/survey_report.md`
- Handoff report at `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p2_survey_1/handoff.md`
- Liveness updates in `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p2_survey_1/progress.md`


## 2026-10-25T09:14:01Z
You are explorer_p2_survey_1, a teamwork_preview_explorer for Phase 2 of Syncplay-Alt.
Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p2_survey_1/
Workspace root: /home/gautham/syncplay-alt

MANDATORY INSTRUCTIONS:
1. You MUST read /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically ## 2026-10-25T09:09:06Z) and /home/gautham/syncplay-alt/AGENTS.md before starting your investigation.
2. Read your task dispatch at /home/gautham/syncplay-alt/.agents/teamwork/explorer_p2_survey_1/DISPATCH.md.
3. Conduct a deep technical survey of R1: Full Container Demuxing & Packet Processing (`001-demuxer-bitstream`) under `crates/syncplay-demux`:
   - Inspect `crates/syncplay-demux/`, `crates/syncplay-common/`, `openspec/specs/demuxer/spec.md`, `openspec/changes/001-demuxer-bitstream.md`, and `tests/empirical_challenge/test_demux_codecs_clock.py`.
   - Analyze the current state of MKV/EBML and MP4/ISOBMFF parsing.
   - Detail what needs to be implemented for:
     a) Extracting sequential `DemuxPacket` payloads for both audio and video streams with validated PTS/DTS.
     b) Keyframe index lookup and random-access seeking (`seek_to(pts)`).
     c) Multi-track stream parsing (video, audio, timed text).
     d) Synthetic test vectors and edge cases.
   - Map exact data structures, trait methods, error types, and concrete recommendations for Worker implementation.
4. Update your liveness heartbeat in /home/gautham/syncplay-alt/.agents/teamwork/explorer_p2_survey_1/progress.md.
5. Write your comprehensive technical survey report to /home/gautham/syncplay-alt/.agents/teamwork/explorer_p2_survey_1/survey_report.md and your formal handoff report to /home/gautham/syncplay-alt/.agents/teamwork/explorer_p2_survey_1/handoff.md.
6. Send a completion message via send_message to your parent orchestrator with a summary of findings and the path to your handoff report.
