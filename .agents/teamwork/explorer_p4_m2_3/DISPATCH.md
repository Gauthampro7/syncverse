# DISPATCH — explorer_p4_m2_3

## Mission
You are `explorer_p4_m2_3`, the Universal Audio Decoding & Clock Pacing Specialist for Milestone 2.
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_3`.

You must read:
1. `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically `## 2026-10-25T20:12:42Z`)
2. `/home/gautham/syncplay-alt/.agents/teamwork/orchestrator_4/PROJECT.md`
3. Current `crates/syncplay-codecs/src/audio/`, `crates/syncplay-clock/src/audio_pacer.rs`, `crates/syncplay-core/src/audio_sink.rs`
4. `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_2/handoff.md`

## Scope & Objective
Formulate the exact architectural design for universal audio decoding and clock pacing:
1. Audio codec coverage: AAC, MP3, Opus, Vorbis, FLAC, AC3/E-AC3, DTS, uncompressed PCM.
2. Design `UniversalAudioDecoder`:
   - Initialize audio decoder context from stream parameters.
   - Resample decoded audio in real time via `ffmpeg_next::software::resampling::Context` to standard:
     * Format: Interleaved 32-bit floating point (`f32`, normalized `[-1.0, 1.0]`).
     * Sampling rate: 48,000 Hz.
     * Channels: 2 (Stereo).
   - Package decoded samples into `AudioFramePCM::new(48_000, 2, pts_ms, samples)`.
3. Verify connection to `AudioPacer` and `AudioSink`:
   - Audio frames transferred via 128B cache-line aligned `SpscRingBuffer<AudioFramePCM, 64>`.
   - `AudioPacer` pacing against `MasterClock` presentation time with silence injection on underruns.
   - Audio sink playback via `PipeAudioSink` (pw-cat/aplay) and/or `cpal`.
4. Provide code prototypes and verification commands.

## Output Requirements
Document your complete design, code diffs, and verification commands in `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_3/handoff.md`.
Send a completion message to the orchestrator when done.

## 2026-10-25T21:11:11Z
You are explorer_p4_m2_3.
Your working directory is: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_3
Read:
1. /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically ## 2026-10-25T20:12:42Z)
2. /home/gautham/syncplay-alt/.agents/teamwork/orchestrator_4/PROJECT.md
3. /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_3/DISPATCH.md
4. crates/syncplay-codecs/src/audio/, crates/syncplay-clock/src/audio_pacer.rs, crates/syncplay-core/src/audio_sink.rs
5. /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_2/handoff.md

Task:
Formulate the exact architectural design for universal audio decoding and clock pacing:
- Design UniversalAudioDecoder supporting AAC, MP3, Opus, Vorbis, FLAC, AC3/E-AC3, DTS, uncompressed PCM.
- Resample in real time via ffmpeg_next::software::resampling::Context to standardized 48kHz stereo f32 in [-1.0, 1.0].
- Package into AudioFramePCM::new(48_000, 2, pts_ms, samples).
- Connect to AudioPacer and AudioSink (pw-cat/aplay/cpal) via 128B SPSC ring buffers.
- Provide code prototypes and verification commands.

Maintain progress.md in your working directory.
When done, write your structured report to /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_3/handoff.md and call send_message to report completion to the orchestrator.
