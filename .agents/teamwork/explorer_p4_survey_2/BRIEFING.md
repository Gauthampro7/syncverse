# BRIEFING — 2026-10-25T20:25:30Z

## Mission
Investigate and architect Universal Video & Audio Compatibility (R2) for Syncplay-Alt Phase 4: container and codec requirements, evaluation of Rust libraries/frameworks (ffmpeg-next, symphonia, cpal, rodio), Fedora system packages, and integration architecture with MasterClock, SPSC ring buffers, and syncplay-sync.

## 🔒 My Identity
- Archetype: explorer
- Roles: explorer, surveyor, architect
- Working directory: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_2
- Original parent: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
- Milestone: Phase 4 Universal Video & Audio Compatibility Survey

## 🔒 Key Constraints
- Read-only investigation — do NOT implement
- Zero direct code modification outside .agents/teamwork/explorer_p4_survey_2
- Investigate R2 Universal Compatibility across containers and codecs
- Check system libraries on Gautham's Fedora Linux
- Formulate concrete architecture integrating with syncplay-clock, syncplay-codecs, syncplay-sync

## Current Parent
- Conversation ID: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
- Updated: 2026-10-25T20:25:30Z

## Investigation State
- **Explored paths**:
  - Fedora 44 system packages: `ffmpeg-libs 8.1.3` is installed in `/usr/lib64/libav*.so.62`.
  - Non-root extraction: `ffmpeg-devel` and `clang-libs` RPMs can be unpacked locally in < 2s without sudo.
  - Empirically verified: Built, linked, and executed `ffmpeg-next 9.0.0` successfully against Fedora 44's FFmpeg 8.1.3 runtime with zero errors.
  - Symphonia analysis: purely audio/demux, lacks all video codecs and lacks AVI, MPEG-TS, FLV containers.
  - Audio output analysis: PipeWire (`pw-cat`) and ALSA (`aplay`) already supported in `PipeAudioSink` with zero dependencies; `cpal 0.18.2` also evaluated and buildable with local `alsa.pc`.
  - Architectural pipeline: Mapped packet flow into `VideoFrameYUV420p` / `VideoFrameRGBA` and `AudioFramePCM`, feeding SPSC ring buffers, `MasterClock`, `AudioPacer`, and `syncplay-sync`.
- **Key findings**:
  - `ffmpeg-next 9.0.0` is the clear, optimal foundation for R2 universal compatibility covering 100% of required containers and codecs.
  - Zero-privilege setup can provide headers locally or user can install system packages.
  - Existing `VideoFrameYUV420p`, `VideoFrameRGBA`, and `AudioFramePCM` models in `syncplay-codecs` align perfectly with FFmpeg's decoded planes and sample layouts.
- **Unexplored areas**: None for survey scope.

## Key Decisions Made
- Recommend `ffmpeg-next 9.0.0` as primary universal demuxing and decoding backend.
- Recommend keeping existing `syncplay-demux` synthetic test vectors and pure-Rust decoders as fallback/test fixtures.
- Recommend updating `AGENTS.md` and `scripts/audit_dependencies.sh` to allow multimedia frameworks while preserving lock-free mathematical sync invariants.

## Artifact Index
- /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_2/DISPATCH.md — Dispatch instructions
- /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_2/BRIEFING.md — Persistent context
- /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_2/progress.md — Heartbeat and status
- /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_2/handoff.md — Final investigation report
