## 2026-10-27T19:03:27Z
You are worker_subtitles, a teamwork_worker.
Your working directory is:
/home/gautham/syncverse/.agents/teamwork/worker_subtitles/

MANDATORY FIRST STEP:
Read the authoritative request file at:
/home/gautham/syncverse/.agents/teamwork/ORIGINAL_REQUEST.md

Also read:
/home/gautham/syncverse/AGENTS.md
/home/gautham/syncverse/.agents/teamwork/explorer_audit_1/handoff.md

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

SCOPE & EXCLUSIVE FILE OWNERSHIP:
You own the Subtitle Data Models, Parsers, Container Demuxing & External Scanner:
- Create `crates/syncverse-common/src/subtitles.rs` defining:
  - `SubtitleCue` (start: MediaTime, end: MediaTime, text: String)
  - `SubtitleSource` (Embedded, External)
  - `SubtitleTrack` (id: usize, source: SubtitleSource, format: String, language: Option<String>, title: String, cues: Vec<SubtitleCue>)
  Re-export in `crates/syncverse-common/src/lib.rs`.
- In `crates/syncverse-demux/src/traits.rs`:
  - Add `pub language: Option<String>` and `pub title: Option<String>` to `TrackInfo`.
  - Add `pub duration_ms: Option<u64>` to `DemuxPacket`.
- In `crates/syncverse-demux/src/universal.rs`:
  - Populate stream metadata (`language`, `title`) from ffmpeg stream tags.
  - Expose demuxed subtitle packets if present or handle subtitle stream identification.
- In `crates/syncverse-demux/src/subtitles/`:
  - `mod.rs`: module declaration and public re-exports
  - `srt.rs`: Pure-Rust parser for SubRip (.srt) cues with timestamps `HH:MM:SS,mmm` -> `MediaTime`
  - `vtt.rs`: Pure-Rust parser for WebVTT (.vtt) cues with timestamps `HH:MM:SS.mmm` -> `MediaTime`
  - `ass.rs`: Pure-Rust parser for SubStation Alpha (.ass / .ssa) dialogue events with timestamps `H:MM:SS.cs` -> `MediaTime`
  - `scanner.rs`: Auto-scanner in media file directory matching `media_stem.*` (.srt, .vtt, .ass, .ssa) and detecting language codes (e.g. `video.en.srt` -> language "en", title "English").
  Re-export in `crates/syncverse-demux/src/lib.rs`.
- Comprehensive Unit Tests in `crates/syncverse-demux/src/subtitles/` testing parsing of SRT, VTT, and ASS files (including edge cases: BOM, multiline, timestamps, corrupted entries) and directory scanning.
- Verify `cargo test -p syncverse-common -p syncverse-demux` passes with zero warnings under `#![deny(warnings)]`.

Deliver your report to `/home/gautham/syncverse/.agents/teamwork/worker_subtitles/handoff.md` and send a message when done.
