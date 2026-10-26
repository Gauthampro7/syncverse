# BRIEFING — 2026-10-25T22:04:00Z

## Mission
Investigate and design concrete fix strategies and draft code diffs for Milestone 2 issues: FLV seek(0) EPERM error, stream start-time offset normalization (MPEG-TS PCR offset), AVProbeData buffer zero-padding, and codec hardening (swscale cache invalidation, DTS alias mapping).

## 🔒 My Identity
- Archetype: explorer
- Roles: read-only investigation, analysis, synthesis, draft diffs
- Working directory: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_fix_1
- Original parent: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
- Milestone: phase4_milestone2_fixes

## 🔒 Key Constraints
- Read-only investigation — do NOT implement directly in source files
- Concrete evidence chain with file paths and line numbers
- Write findings and proposed diffs to handoff.md in working directory
- Send completion message to parent upon finishing

## Current Parent
- Conversation ID: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
- Updated: 2026-10-25T22:04:00Z

## Investigation State
- **Explored paths**:
  - `crates/syncplay-demux/src/universal.rs`
  - `crates/syncplay-demux/tests/universal_adversarial_challenge.rs`
  - `crates/syncplay-demux/tests/universal_demux_tests.rs`
  - `crates/syncplay-codecs/src/universal_video.rs`
  - `crates/syncplay-codecs/src/audio/universal.rs`
  - `crates/syncplay-codecs/tests/universal_codecs_adversarial_stress.rs`
  - `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p4_m2_1/handoff.md`
  - `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p4_m2_2/handoff.md`
  - `ffmpeg-next-9.0.0` format and input context implementations
- **Key findings**:
  - Confirmed FLV `seek_to(0)` `EPERM` root cause: `avformat_seek_file` with `max_ts = 0` cannot find keyframe when earliest keyframe is at 25ms due to priming delay. Fix: seek with `stream_target_us, ..` when target is at/before start, or fallback from `..stream_target_us` to `..`.
  - Confirmed MPEG-TS PCR offset root cause: `ictx.start_time()` is ignored, causing PTS to start at 1480ms instead of 0ms. Fix: extract `start_time_ms = (*ictx.as_ptr()).start_time / 1000`, subtract from `read_packet` PTS/DTS, add to `seek_to` target.
  - Confirmed `AVProbeData` buffer padding requirement: allocate `vec![0u8; 4096 + 64]` and read up to 4096, guaranteeing 64 bytes zero padding (exceeding FFmpeg's `AVPROBE_PADDING_SIZE` of 32 bytes).
  - Confirmed video `swscale` cache invalidation: `UniversalVideoDecoder` lacks tracking of scaler dimensions/format. Fix: record `last_scaler_format`, `last_scaler_width`, `last_scaler_height`, recreate scaler on change.
  - Confirmed audio DTS alias mapping: add `map_codec_name` to map `"dts"` / `"dca"` -> `Id::DTS`, `"pcm"` -> `Id::PCM_S16LE`.
- **Unexplored areas**: None. All 4 target areas completely investigated with verified solutions.

## Key Decisions Made
- Use two-tier seek strategy: forward/unbounded `..` for target 0 / stream start, and backward `..stream_target_us` with fallback to `..` for random seeking.
- Compute container start time via `(*ictx.as_ptr()).start_time` checked against `AV_NOPTS_VALUE` and $> 0$.
- Add `map_codec_name` to `UniversalAudioDecoder` mirroring `UniversalVideoDecoder`.

## Artifact Index
- /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_fix_1/DISPATCH.md — Dispatch instructions
- /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_fix_1/BRIEFING.md — Persistent state
- /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_fix_1/progress.md — Progress heartbeat
- /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_fix_1/handoff.md — Final handoff report
