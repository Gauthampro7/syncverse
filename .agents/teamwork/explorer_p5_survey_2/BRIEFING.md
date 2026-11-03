# BRIEFING — 2026-10-30T21:58:00Z

## Mission
Deep-dive technical audit of Syncverse core crates (`syncverse-demux`, `syncverse-codecs`, `syncverse-clock`, `syncverse-sync`): analyze current state, identify improvements, edge cases, and new features adhering strictly to AGENTS.md.

## 🔒 My Identity
- Archetype: teamwork_explorer
- Roles: explorer, investigator, synthesizer
- Working directory: /home/gautham/syncverse/.agents/teamwork/explorer_p5_survey_2
- Original parent: 3feda53a-5598-4b7c-86b8-98dbeb7556bd
- Milestone: P5 Technical Survey & Planning

## 🔒 Key Constraints
- Read-only investigation — do NOT implement in source crates
- Strict compliance with AGENTS.md (no banned crates, pure Rust/direct bindings, SIMD/cache alignments, no float precision loss)
- Deliver findings in /home/gautham/syncverse/.agents/teamwork/explorer_p5_survey_2/handoff.md

## Current Parent
- Conversation ID: 3feda53a-5598-4b7c-86b8-98dbeb7556bd
- Updated: 2026-10-30T21:49:49Z

## Investigation State
- **Explored paths**:
  - `crates/syncverse-demux/`: universal demuxer (`ffmpeg-next`), in-tree EBML/ISOBMFF, synthetic vectors, subtitle parsers (SRT, WebVTT, ASS/SSA), external scanner.
  - `crates/syncverse-codecs/`: universal video decoder (H.264/HEVC/VP8/VP9/AV1/MPEG-2/4/VC-1), universal audio decoder (AAC/MP3/Opus/Vorbis/FLAC/AC3/DTS/PCM), planar YUV420p / RGBA memory alignment & SIMD padding, `LinearResampler` micro-speed steering, Q8 fixed-point color space converters (BT.601 / BT.709).
  - `crates/syncverse-clock/`: `MasterClock` SeqLock optimistic snapshot, $C^0$ continuity re-anchoring, `evaluate_presentation_timing`, `AudioPacer` underrun protection & silence injection, `SpscRingBuffer` 128B cache alignment, `PresentationReorderBuffer`.
  - `crates/syncverse-sync/`: NTP 4-timestamp exchange, `Rfc5905Filter` (8-sample min RTT, dispersion floor $5.0\text{ms}$, watchdog), reference timeline equation, `PllSpeedController` discrete PI loop, two-phase barrier unpause, binary wire protocol (`SPNT`).
  - `crates/syncverse-core/`: `playback_runtime.rs`, `coordinator.rs`, audio worker thread, drift synchronization loops.
- **Key findings**:
  1. *Major Architectural Bug in PLL & Watch Party Non-1.0x Speeds*: `PllSpeedController` and client drift synchronization loop hardcode base speed 1.0x and clamp to $[0.96, 1.04]$, ignoring `room_state.r_target`. In Zone 1, client unconditionally resets speed to 1.0x. If host plays at 1.25x or 1.5x, client drifts, clobbers speed, and repeatedly triggers Zone 3 hard seeks every few seconds.
  2. *Timescale Integer Arithmetic Invariant Violation*: In `UniversalDemuxer::init_from_context`, chapter start/end timestamps are calculated using `f64` arithmetic (`ch.start() as f64 * tb_f64 * 1000.0`), violating AGENTS.md Section 4 Rule 6.
  3. *Audio Clipping on Multichannel Downmix*: Downmixing 5.1/7.1 to stereo hard-clamps samples with `s.clamp(-1.0, 1.0)`, causing square-wave distortion on loud sound effects. A soft-knee limiter (`tanh` or polynomial) would eliminate digital clipping buzz.
  4. *Audio Starvation Glitch in Runtime Worker*: `run_audio_sink_worker` in `playback_runtime.rs` sleeps when `pending_queue.is_empty()` rather than leveraging `AudioPacer` silence injection, exposing soundcards to driver underrun pops during heavy IO.
  5. *High-Value Feature Additions*:
     - Subdirectory subtitle scanning (`subtitles/`, `subs/`).
     - Media looping & A-B repeat segment markers (`[`, `]`, `\`).
     - Fine-grained playback speed steps [0.25x - 4.0x] with synchronized room tracking.
     - Enhanced sync diagnostics & jitter telemetry (window dispersion, probe rejection rate, RTT spread).
- **Unexplored areas**: None across assigned scope.

## Key Decisions Made
- Formulate precise, mathematically sound remediations and features adhering strictly to AGENTS.md.
- Maintain the 72-byte `PlayerUiSnapshot` memory invariant and zero-allocation hot loops.

## Artifact Index
- /home/gautham/syncverse/.agents/teamwork/explorer_p5_survey_2/DISPATCH.md — Incoming parent instructions
- /home/gautham/syncverse/.agents/teamwork/explorer_p5_survey_2/progress.md — Heartbeat and progress tracking
- /home/gautham/syncverse/.agents/teamwork/explorer_p5_survey_2/handoff.md — Final structured audit report
