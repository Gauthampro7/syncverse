# Dispatch for worker_p2_demux

## Task
Implement Milestone 1: Full Container Demuxing & Packet Processing (`001-demuxer-bitstream`) under `crates/syncplay-demux`.

## Exclusive Write Ownership
You have exclusive write ownership of:
- `crates/syncplay-demux/`

Do NOT modify any files in other crates unless strictly required for trait contracts in `crates/syncplay-common/`.

## Inputs & Specifications
- `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`
- `/home/gautham/syncplay-alt/AGENTS.md`
- `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p2_survey_1/survey_report.md`
- `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p2_survey_1/handoff.md`
- `openspec/specs/demuxer/spec.md`
- `openspec/changes/001-demuxer-bitstream.md`

## Detailed Requirements
1. **Matroska / EBML Demuxer (`crates/syncplay-demux/src/ebml.rs`)**:
   - Parse `Segment/Info` element (`0x1549A966`) to extract `TimestampScale` (`0x2AD7B1`) and `Duration` (`0x4489`).
   - Parse `Segment/Tracks` element (`0x1654AE6B`) and child `TrackEntry`s (`0xAE`) to extract track number, track type (video, audio, subtitle), codec ID, video dimensions (`PixelWidth`, `PixelHeight`), audio params (`SamplingFrequency`, `Channels`), and `CodecPrivate`.
   - In `read_packet`: Parse `SimpleBlock` (`0xA3`) and `BlockGroup` (`0xA0`) / `Block` (`0xA1`). Implement frame lacing unpacker (Xiph lacing, Fixed-size lacing, and EBML lacing) and internal packet queue `VecDeque<DemuxPacket>`.
   - In `seek_to(target_pts_ms)`: Parse `Segment/Cues` (`0x1C53BB6B`) / `CuePoint` (`0xBB`) or perform cluster search to locate preceding keyframe, seek the reader, reset queue, and return exact landed keyframe PTS.
2. **ISOBMFF / MP4 Demuxer (`crates/syncplay-demux/src/isobmff.rs`)**:
   - In `parse_headers_from_slice`: Traverse child `trak` boxes inside `moov`, extracting `tkhd`, `mdia/mdhd`, `mdia/hdlr`, and `mdia/minf/stbl`.
   - Wire `SampleTableResolver` for each track to map `stts`, `stsc`, `stsz`, `stco`/`co64`, and `stss` (sync samples).
   - In `read_packet`: Extract interleaved sample packets from `mdat` across active tracks based on minimum physical file offset.
   - In `seek_to(target_pts_ms)`: Use `stss` and `stts` to find preceding sync sample, seek the reader, and return exact landed PTS.
3. **Synthetic Vectors & Invariant Preservation**:
   - MUST preserve existing `SYNTHETIC_EBML_VECTOR` (48B) and `SYNTHETIC_MP4_VECTOR` (132B) unmodified to maintain Intent 07.
   - Add new multi-track synthetic vectors (`SYNTHETIC_FULL_MKV` and `SYNTHETIC_FULL_MP4`) in `crates/syncplay-demux/src/synthetic.rs` or tests for testing sequential interleaved packet demuxing and seeking.
4. **Zero-Warning & Zero-Dependency Invariant**:
   - `#![deny(warnings)]`, `#![deny(missing_docs)]`, `#![deny(unsafe_code)]`.
   - Zero forbidden external dependencies.
   - Run verification commands and include results in handoff report.

## Deliverables
- Implementation in `crates/syncplay-demux/`
- Progress updates in `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux/progress.md`
- Formal handoff report at `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux/handoff.md`


## 2026-10-25T09:22:22Z
You are worker_p2_demux, a teamwork_preview_worker for Phase 2 of Syncplay-Alt.
Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux/
Workspace root: /home/gautham/syncplay-alt

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

MANDATORY INSTRUCTIONS:
1. You MUST read /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically ## 2026-10-25T09:09:06Z) and /home/gautham/syncplay-alt/AGENTS.md before starting.
2. Read your task dispatch at /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux/DISPATCH.md.
3. Read the technical survey and architecture roadmap prepared by explorer_p2_survey_1 at:
   - /home/gautham/syncplay-alt/.agents/teamwork/explorer_p2_survey_1/survey_report.md
   - /home/gautham/syncplay-alt/.agents/teamwork/explorer_p2_survey_1/handoff.md
4. Implement Milestone 1 in `crates/syncplay-demux/`:
   - Matroska / EBML demuxer extension (`src/ebml.rs`): Parse Segment/Info, Segment/Tracks, multi-track TrackInfo, SimpleBlock & BlockGroup frame lacing unpacker with VecDeque packet buffer, and Segment/Cues keyframe seek_to(pts).
   - ISOBMFF / MP4 demuxer extension (`src/isobmff.rs`): Parse moov/trak hierarchies, wire per-track SampleTableResolver instances, extract interleaved sample packets from mdat, and keyframe seek_to(pts) using stss/stts.
   - Preserve existing SYNTHETIC_EBML_VECTOR (48B) and SYNTHETIC_MP4_VECTOR (132B) untouched. Add new multi-track synthetic vectors (SYNTHETIC_FULL_MKV, SYNTHETIC_FULL_MP4) and comprehensive unit/integration tests in `crates/syncplay-demux/tests/`.
   - Ensure `#![deny(warnings)]`, `#![deny(missing_docs)]`, and `#![deny(unsafe_code)]` compliance.
5. Verification:
   - Run `export PATH="$HOME/.cargo/bin:$PATH"`
   - Run `cargo check --workspace --all-targets`
   - Run `cargo clippy -p syncplay-demux --all-targets -- -D warnings`
   - Run `cargo test -p syncplay-demux -- --nocapture`
   - Run `./scripts/audit_dependencies.sh`
   - Run `python3 scripts/intent_harness.py`
6. Update your progress in /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux/progress.md.
7. Write your handoff report to /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux/handoff.md.
8. Send a completion message via send_message to your parent orchestrator with the verification results and handoff path.

## 2026-10-25T09:35:10Z
**Context**: Milestone 1 implementation of container demuxing (`crates/syncplay-demux/`).
**Content**: Checking in on your progress. What step are you currently working on, and do you need any assistance?
**Action**: Please provide a brief status update and update your `progress.md`.
