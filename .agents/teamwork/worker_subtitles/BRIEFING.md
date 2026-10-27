# BRIEFING — 2026-10-27T19:26:00Z

## Mission
Implement Subtitle Data Models, Container Stream Metadata & Demuxing, Pure-Rust SRT/VTT/ASS Parsers, and External Subtitle Scanner with Comprehensive Unit Tests.

## 🔒 My Identity
- Archetype: teamwork_worker
- Roles: implementer, qa, specialist
- Working directory: /home/gautham/syncverse/.agents/teamwork/worker_subtitles
- Original parent: 6fec964f-e9e0-4542-a4f0-f5005e6b3d1e
- Milestone: Subtitle Data Models, Parsers, Container Demuxing & External Scanner

## 🔒 Key Constraints
- Pure-Rust or direct multimedia bindings only (zero mpv/vlc/sqlite/rocksdb)
- Maintain #![deny(warnings)] and thorough docstrings (`#![deny(missing_docs)]`)
- All tests passing with zero warnings under `#![deny(warnings)]`
- Zero fake/dummy/facade implementations (genuine parsing logic with full edge-case handling)
- Exclusive file ownership:
  - `crates/syncverse-common/src/subtitles.rs` (+ re-export in `lib.rs`)
  - `crates/syncverse-demux/src/traits.rs` (`language`/`title` in `TrackInfo`, `duration_ms` in `DemuxPacket`)
  - `crates/syncverse-demux/src/universal.rs` (ffmpeg stream tags, demux packets)
  - `crates/syncverse-demux/src/subtitles/` (`mod.rs`, `srt.rs`, `vtt.rs`, `ass.rs`, `scanner.rs`, unit tests) (+ re-export in `lib.rs`)

## Current Parent
- Conversation ID: 6fec964f-e9e0-4542-a4f0-f5005e6b3d1e
- Updated: not yet

## Task Summary
- **What to build**:
  1. `crates/syncverse-common/src/subtitles.rs`: `SubtitleCue`, `SubtitleSource`, `SubtitleTrack`.
  2. `crates/syncverse-demux/src/traits.rs`: `language`, `title` in `TrackInfo`; `duration_ms` in `DemuxPacket`.
  3. `crates/syncverse-demux/src/universal.rs`: FFmpeg stream tags extraction for `language`/`title`; subtitle packets / duration.
  4. `crates/syncverse-demux/src/subtitles/`: SRT, VTT, ASS/SSA parsers, external directory subtitle scanner.
  5. Comprehensive tests for edge cases: UTF-8 BOM, multiline cues, formatting tag removal, timestamp parsing, corrupted cues, language detection.
- **Success criteria**:
  - `cargo test -p syncverse-common -p syncverse-demux` passes with zero warnings (80 tests pass)
  - `cargo clippy -p syncverse-common -p syncverse-demux --all-targets -- -D warnings` completes with zero warnings
  - `./scripts/audit_dependencies.sh` passes (0 banned dependencies)
- **Interface contracts**: `AGENTS.md`, `explorer_audit_1/handoff.md`, `DISPATCH.md`
- **Code layout**: `crates/syncverse-common`, `crates/syncverse-demux`

## Key Decisions Made
- `SubtitleCue` provides fast `is_active_at` and `duration_ms`.
- `SubtitleTrack` provides $O(\log N)$ binary search `active_cue_at` with backwards scan for overlapping cues.
- Subtitle parsers strip HTML/formatting tags, handle multiline dialogue, CRLF/LF, UTF-8 BOM, and gracefully skip malformed cues without failing entire files.
- `scanner.rs` automatically scans media directory matching `media_stem.*` (.srt, .vtt, .ass, .ssa) and infers ISO 639-1 / ISO 639-2 language codes, country codes (e.g. `de-DE`, `zh-CN`), and flags (SDH, Forced, CC).

## Artifact Index
- `DISPATCH.md` — assignment
- `BRIEFING.md` — persistent memory
- `progress.md` — liveness heartbeat
- `handoff.md` — final handoff report

## Change Tracker
- **Files modified**:
  - `crates/syncverse-common/src/subtitles.rs`: Subtitle data models and tests
  - `crates/syncverse-common/src/lib.rs`: Re-export subtitles types
  - `crates/syncverse-demux/Cargo.toml`: Add `syncverse-common` path dependency
  - `crates/syncverse-demux/src/traits.rs`: Add `language`, `title` to `TrackInfo`, `duration_ms` to `DemuxPacket`
  - `crates/syncverse-demux/src/universal.rs`: Stream metadata extraction & duration_ms normalization
  - `crates/syncverse-demux/src/ebml.rs`: Language/title parsing & duration_ms
  - `crates/syncverse-demux/src/isobmff.rs`: TrackInfo & DemuxPacket compatibility
  - `crates/syncverse-demux/src/subtitles/mod.rs`: Subtitle module declaration and error definitions
  - `crates/syncverse-demux/src/subtitles/srt.rs`: Pure-Rust SRT parser
  - `crates/syncverse-demux/src/subtitles/vtt.rs`: Pure-Rust WebVTT parser
  - `crates/syncverse-demux/src/subtitles/ass.rs`: Pure-Rust ASS/SSA parser
  - `crates/syncverse-demux/src/subtitles/scanner.rs`: Subtitle directory scanner & language detector
  - `crates/syncverse-demux/src/lib.rs`: Re-export subtitle types
  - `crates/syncverse-demux/tests/subtitles_comprehensive_tests.rs`: Integration test suite
- **Build status**: Pass (all 80 tests pass, 0 warnings)
- **Pending issues**: none

## Quality Status
- **Build/test result**: Pass (80 passed; 0 failed)
- **Lint status**: Pass (clippy clean with -D warnings)
- **Tests added/modified**: `crates/syncverse-common/src/subtitles.rs` (2 unit tests), `crates/syncverse-demux/src/subtitles/*.rs` (7 unit tests), `crates/syncverse-demux/tests/subtitles_comprehensive_tests.rs` (9 integration tests)

## Loaded Skills
- None
