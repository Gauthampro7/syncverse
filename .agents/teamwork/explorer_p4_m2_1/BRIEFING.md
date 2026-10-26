# BRIEFING — 2026-10-25T21:23:30Z

## Mission
Formulate the exact architectural design, code prototypes, and verification commands for universal container demuxing in syncplay-demux.

## 🔒 My Identity
- Archetype: explorer
- Roles: investigation, architectural design, synthesis
- Working directory: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_1
- Original parent: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
- Milestone: milestone_2

## 🔒 Key Constraints
- Read-only investigation — do NOT implement in production crates directly; provide design, prototypes, and handoff report
- Integrate ffmpeg-next = "9.0.0" for universal demuxing
- Universal container coverage: MKV/WebM, MP4/MOV, AVI, MPEG-TS, FLV, OGG
- Exact rational timescale conversion: pts_ms = packet.pts() * tb.num * 1000 / tb.den
- Keyframe seeking seek_to(pts_ms)
- Preserve existing EbmlDemuxer and IsoBmffDemuxer and all existing unit tests

## Current Parent
- Conversation ID: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
- Updated: 2026-10-25T21:22:06Z

## Investigation State
- **Explored paths**:
  - `Cargo.toml`, `crates/syncplay-demux/Cargo.toml`, `crates/syncplay-demux/src/lib.rs`, `traits.rs`, `ebml.rs`, `isobmff.rs`
  - `/home/gautham/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ffmpeg-next-9.0.0/` format demuxing, StreamIo, Packet, seeking, and C probe APIs
  - Generated real test fixtures across all 8 container formats (MKV, WebM, MP4, MOV, AVI, MPEG-TS, FLV, OGG)
- **Key findings**:
  - `ffmpeg-next` 9.0.0 compiles cleanly with system `libav*` and user-space `.deps/env.sh`
  - Custom I/O via `StreamIo::from_read_seek(reader)` seamlessly supports `Box<dyn ReadSeek + Send>`
  - `av_probe_input_format` with fallback container heuristics correctly identifies 100% of required containers
  - Exact rational timescale conversion `pts_ms = ((raw_pts as i128 * num * 1000) / den) as u64` provides overflow-safe millisecond alignment
  - Keyframe seeking with peeked packet buffering guarantees zero packet loss and exact keyframe landing
  - All 38 existing + new unit and integration tests compile and pass with zero warnings and zero clippy warnings
- **Unexplored areas**: None for demuxing; video and audio decoding assigned to sibling explorers m2_2 and m2_3

## Key Decisions Made
- Implemented `UniversalDemuxer` implementing `Demuxer` trait and providing `open_file` convenience constructor
- Retained in-tree `EbmlDemuxer` and `IsoBmffDemuxer` to maintain 100% backward compatibility with synthetic test suites
- Created `proposed_universal.rs`, `proposed_universal_demux_tests.rs`, and `universal_demuxer.patch` (tested with `git apply --check`)

## Artifact Index
- `DISPATCH.md` — incoming dispatch instructions and status queries
- `BRIEFING.md` — persistent situational awareness
- `progress.md` — liveness heartbeat and milestone tracking
- `proposed_universal.rs` — full production-ready implementation of `UniversalDemuxer`
- `proposed_universal_demux_tests.rs` — comprehensive integration test suite for all 8 container types
- `universal_demuxer.patch` — machine-applicable unified git diff patch
- `handoff.md` — formal 5-component architectural handoff report
