# BRIEFING — 2026-10-25T21:22:00Z

## Mission
Architect the universal audio decoding and clock pacing subsystem for Milestone 2: UniversalAudioDecoder (AAC, MP3, Opus, Vorbis, FLAC, AC3/E-AC3, DTS, PCM), SwResample 48kHz stereo f32 conversion, AudioFramePCM packaging, and SPSC/AudioPacer/AudioSink pacing integration.

## 🔒 My Identity
- Archetype: explorer
- Roles: investigator, architect, synthesizer
- Working directory: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_3
- Original parent: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
- Milestone: M2 (Universal Video & Audio Compatibility Engine)

## 🔒 Key Constraints
- Read-only investigation — do NOT directly modify source code (deliver prototypes and reports in working directory)
- Strict adherence to AGENTS.md, PROJECT.md, and OpenSpec
- Standardized audio output: 48,000 Hz, 2 channels (Stereo), interleaved f32 in [-1.0, 1.0]
- Zero buffer underrun crackle, silence injection, lock-free 128B cache-line aligned SPSC ring buffer

## Current Parent
- Conversation ID: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
- Updated: 2026-10-25T21:22:00Z

## Investigation State
- **Explored paths**:
  - `crates/syncplay-codecs/src/audio/` (decoder.rs, resampler.rs, pcm.rs, opus.rs, vorbis.rs)
  - `crates/syncplay-clock/src/audio_pacer.rs` (MasterClock sync, 128B SPSC, silence injection, ramp-in)
  - `crates/syncplay-core/src/audio_sink.rs` (PipeAudioSink pw-cat/aplay, NullAudioSink, AutoAudioSink)
  - `ffmpeg-next-9.0.0` internals (decoder::Audio, ResamplerContext, Audio frame, Packet)
  - Empirical verification test suite across all 9 target codecs in `/tmp/test_audio`
- **Key findings**:
  - Full decoder support verified for all 9 codecs (AAC, MP3, Opus, Vorbis, FLAC, AC3, E-AC3, DTS, PCM).
  - SwResample context converts any channel/rate/sample format to 48kHz stereo interleaved f32.
  - Channel layout normalization required: fall back to `ChannelLayout::default(channels)` if layout mask is empty.
  - Resampler flush requires pre-allocating output frame with `delay.output` samples to prevent `Output changed` panic.
  - End-to-end integration verified: decoded frames wrap into `AudioFramePCM`, queue into 128B SPSC, pace via `AudioPacer`, with silence injection on underruns and zero pop/crackle.
- **Unexplored areas**: None within M2 audio scope.

## Key Decisions Made
- `UniversalAudioDecoder` designed as unified engine wrapping `ffmpeg_next::decoder::Audio` and lazy `ResamplerContext`.
- Output format strictly normalized: `48_000` Hz, 2 channels (Stereo), interleaved `f32` in `[-1.0, 1.0]`.
- Integration with `AudioPacer` and `AudioSink` via 128B cache-line aligned `SpscRingBuffer<AudioFramePCM, 64>`.
- Prototype delivered in `universal_audio_prototype.rs`.

## Artifact Index
- DISPATCH.md — Task assignment from orchestrator
- BRIEFING.md — Persistent working memory
- progress.md — Liveness heartbeat and progress log
- universal_audio_prototype.rs — Complete working implementation prototype of UniversalAudioDecoder
- handoff.md — Final 5-component handoff report
