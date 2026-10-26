# Dispatch Log

## 2026-10-24T23:02:56Z
From: 1a21f3c9-f57a-4ad4-b306-97b4ba0e698e (parent / orchestrator_1)
Priority: MESSAGE_PRIORITY_HIGH

You are explorer_survey_1 (teamwork_preview_explorer).
Your Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_1/
Project Root: /home/gautham/syncplay-alt
Authoritative User Request: /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md

### Objective
Conduct an exhaustive technical exploration and survey for the first foundational subsystems of Syncplay-Alt:
1. Zero-dependency container demuxing (MKV/EBML, MP4/ISOBMFF) and raw bitstream parsing.
2. From-scratch baseline video & audio codec decoding primitives, color-space conversion (YUV420p to RGB), and frame presentation buffer models.
3. Master monotonic A/V synchronization clock and lock-free ring-buffer pipeline architecture.

### Scope & Constraints
- Read-only technical investigation. Do NOT write or modify application source code.
- Write your working files exclusively in your working directory `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_1/`.
- Prioritize authoritative standards (EBML RFC 8794, Matroska specifications, ISO/IEC 14496-12 ISOBMFF box specs) and mathematical clock synchronization models.
- ZERO external media/codec libraries allowed (`ffmpeg`, `libav*`, `gstreamer`, `mpv`, `vlc`, `symphonia`, `dav1d`).

### Detailed Requirements to Survey & Specify
1. Container Demuxing:
   - EBML Variable-Length Integer (VINT) octet-width encoding and decoding rules, mask handling, and leading zero counts.
   - EBML Element ID representation and known Matroska elements (EBML Header, Segment, SeekHead, Info, Tracks, TrackEntry, Video/Audio settings, Cluster, Timecode/Timestamp, SimpleBlock, BlockGroup, Block flags like keyframe/lacing).
   - ISOBMFF Box structure: standard 32-bit `size` and 4-byte `type` (FourCC), 64-bit `largesize` when `size == 1`, Box headers (`ftyp`, `moov`, `mvhd`, `trak`, `tkhd`, `mdia`, `mdhd`, `hdlr`, `minf`, `stbl`, `stsd`, `stts`, `stsc`, `stsz`, `stco`, `co64`, `mdat`).
   - Synthetic test vector design: exact byte layout for valid synthetic EBML header and MP4 `ftyp`/`moov` headers for automated unit testing without needing multi-megabyte media files.
2. Codec Primitives & A/V Clock Ring Buffer:
   - Baseline audio/video frame representations: planar YUV420p, packed RGB24/RGBA32, PCM audio frames (stereo, 48kHz/44.1kHz, f32/i16).
   - Pure-Rust integer/fixed-point YUV420p to RGB conversion formula and SIMD-friendly layout.
   - Master Monotonic Clock design: high-resolution monotonic time source, PTS (Presentation TimeStamp) tracking, drift calculation, audio-master vs video-slave vs external network clock modes.
   - Lock-free / atomic ring-buffer design for decoupling container demuxing, decoding, and presentation threads.

### Deliverables
Write a comprehensive report to `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_1/analysis.md` and complete a structured handoff report in `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_1/handoff.md`.
Notify the orchestrator via `send_message` when complete.
