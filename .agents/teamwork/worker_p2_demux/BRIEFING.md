# BRIEFING — 2026-10-25T09:41:00Z

## Mission
Implement Milestone 1 in `crates/syncplay-demux`: full container demuxing, multi-track extraction, frame lacing, sample table resolving, and deterministic keyframe seeking for Matroska (MKV) and ISOBMFF (MP4).

## 🔒 My Identity
- Archetype: teamwork_preview_worker
- Roles: implementer, qa, specialist
- Working directory: /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_demux
- Original parent: 0644b5fa-9f3b-44ab-92cd-57079ebc4b70
- Milestone: Milestone 1: Full Container Demuxing & Packet Processing (`001-demuxer-bitstream`)

## 🔒 Key Constraints
- Zero-External-Codec Invariant: No external media, codec, or database frameworks (`ffmpeg`, `vlc`, `mpv`, `nom-ebml`, `symphonia`, `sqlite`, etc.).
- Exclusive write ownership: `crates/syncplay-demux/` (and `.agents/teamwork/worker_p2_demux/`).
- MUST preserve existing `SYNTHETIC_EBML_VECTOR` (48B) and `SYNTHETIC_MP4_VECTOR` (132B) unmodified.
- Code standards: `#![deny(warnings)]`, `#![deny(missing_docs)]`, `#![deny(unsafe_code)]`.
- Mandatory verification: 100% Intent Compliance, zero compiler/clippy warnings, all unit/integration tests passing.

## Current Parent
- Conversation ID: 0644b5fa-9f3b-44ab-92cd-57079ebc4b70
- Updated: 2026-10-25T09:35:10Z

## Task Summary
- **What to build**: End-to-end container demuxer for MKV (`ebml.rs`) and MP4 (`isobmff.rs`):
  1. Matroska: Parse Segment/Info (timescale, duration), Segment/Tracks (multi-track TrackInfo), SimpleBlock & BlockGroup with Xiph/Fixed/EBML frame lacing and `VecDeque<DemuxPacket>` buffer, and Segment/Cues keyframe seek_to(pts).
  2. ISOBMFF/MP4: Parse moov/trak hierarchies, wire per-track `SampleTableResolver` (stts, ctts, stss, stsc, stsz, stco/co64), interleaved sample packet reading from mdat, and keyframe seek_to(pts).
  3. Synthetic multi-track vectors: `SYNTHETIC_FULL_MKV` and `SYNTHETIC_FULL_MP4`.
  4. Integration tests in `crates/syncplay-demux/tests/demux_tests.rs`.
- **Success criteria**: All cargo tests pass, clippy passes with zero warnings, intent harness passes at 100.0%.
- **Interface contracts**: `openspec/specs/demuxer/spec.md`, `crates/syncplay-demux/src/traits.rs`.
- **Code layout**: `crates/syncplay-demux/src/`, `crates/syncplay-demux/tests/`.

## Key Decisions Made
- Preserved `SYNTHETIC_EBML_VECTOR` (48B) and `SYNTHETIC_MP4_VECTOR` (132B) byte-for-byte to maintain Intent 07 and Phase 1 compatibility.
- Implemented `parse_tracks_data`, `parse_cues_data`, and `parse_info_data` in `EbmlDemuxer`.
- Implemented `unpack_lacing` handling No lacing (0b00), Xiph (0b01), Fixed-size (0b10), and EBML signed delta VINTs (0b11).
- Implemented `parse_trak` inside `IsoBmffDemuxer`, resolving `stsd`, `stts`, `ctts`, `stss`, `stsc`, `stsz`, `stco`, and `co64`.
- Extracted interleaved sample packets across tracks using minimum sample file offset.
- Implemented deterministic keyframe seeking mapping target PTS to preceding sync samples and repositioning the stream reader.
- Created `build_synthetic_full_mkv` and `build_synthetic_full_mp4` in `synthetic.rs` with `OnceLock` static access.

## Artifact Index
- `crates/syncplay-demux/src/traits.rs` — Updated `TrackInfo` with `codec_private: Option<Vec<u8>>`.
- `crates/syncplay-demux/src/error.rs` — Added `DemuxError::CorruptedBlock(String)`.
- `crates/syncplay-demux/src/ebml.rs` — Multi-track EBML demuxer, lacing unpacker, Cues seek, and `CueEntry`.
- `crates/syncplay-demux/src/isobmff.rs` — Multi-track ISOBMFF demuxer, SampleTableResolver, interleaved reading, seek.
- `crates/syncplay-demux/src/synthetic.rs` — Full multi-track MKV and MP4 test vectors.
- `crates/syncplay-demux/src/lib.rs` — Exported synthetic module and new types.
- `crates/syncplay-demux/tests/demux_tests.rs` — Integration test suite.

## Change Tracker
- **Files modified**:
  - `crates/syncplay-demux/src/traits.rs`: Added `codec_private` field to `TrackInfo`.
  - `crates/syncplay-demux/src/error.rs`: Added `CorruptedBlock` variant to `DemuxError`.
  - `crates/syncplay-demux/src/ebml.rs`: Implemented multi-track parsing, lacing unpacker, Cues seeking.
  - `crates/syncplay-demux/src/isobmff.rs`: Implemented moov/trak parsing, sample table wire-up, interleaved reading, seeking.
  - `crates/syncplay-demux/src/synthetic.rs`: Created multi-track synthetic generators and OnceLock getters.
  - `crates/syncplay-demux/src/lib.rs`: Re-exported synthetic module and public types.
  - `crates/syncplay-demux/tests/demux_tests.rs`: Added comprehensive unit and integration tests.
- **Build status**: All checks, tests, clippy, and intent harness passing cleanly.
- **Pending issues**: None.

## Quality Status
- **Build/test result**: Pass (100% of workspace tests passing).
- **Lint status**: 0 warnings (`cargo clippy --workspace --all-targets -- -D warnings`).
- **Tests added/modified**: 5 new integration tests in `crates/syncplay-demux/tests/demux_tests.rs`, 1 new unit test in `src/ebml.rs`.

## Loaded Skills
None requested.
