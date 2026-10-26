# DISPATCH — explorer_p4_survey_2

## Mission
You are `explorer_p4_survey_2`, a read-only exploration and architecture specialist.
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_2`.
You must read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (especially section `## 2026-10-25T20:12:42Z`).

## Scope & Objective
Investigate Requirement R2: Universal Video & Audio Compatibility:
1. Examine container support requirements: MKV/WebM, MP4/MOV, AVI, MPEG-TS, FLV, OGG.
2. Examine video codec requirements: H.264 (AVC), H.265 (HEVC), VP8, VP9, AV1, MPEG-2, MPEG-4, VC-1.
3. Examine audio codec requirements: AAC, MP3, Opus, Vorbis, FLAC, AC3/E-AC3, DTS, uncompressed PCM.
4. Note user instruction in R3: "Update AGENTS.md to remove obsolete prohibitions against multimedia backends and define the new GUI and codec standards."
5. Evaluate best Rust libraries/frameworks to fulfill these requirements (e.g. `ffmpeg-next` / system FFmpeg, `symphonia` for audio/containers, pure-Rust decoders, `cpal` / `rodio` for audio playback). Check system libraries available on Gautham's Fedora Linux (`pkg-config`, ffmpeg libraries, etc.).
6. Propose a concrete architecture for demuxing and decoding that converts packets into planar `VideoFrameYUV420p` / `VideoFrameRGBA` and `AudioFramePCM` float/i16 samples, integrating seamlessly with `syncplay-clock` MasterClock, SPSC ring buffers, and `syncplay-sync`.

## Output Requirements
Document all findings, evaluated options, crate dependencies, and concrete architectural recommendations in `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_2/handoff.md`.
Send a message to the orchestrator when complete.


## 2026-10-25T20:16:40Z
You are explorer_p4_survey_2.
Your working directory is: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_2
Read /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically the latest request under ## 2026-10-25T20:12:42Z) and your assignment in /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_2/DISPATCH.md.

Task:
Investigate Requirement R2: Universal Video & Audio Compatibility:
1. Examine container support requirements: MKV/WebM, MP4/MOV, AVI, MPEG-TS, FLV, OGG.
2. Examine video codec requirements: H.264 (AVC), H.265 (HEVC), VP8, VP9, AV1, MPEG-2, MPEG-4, VC-1.
3. Examine audio codec requirements: AAC, MP3, Opus, Vorbis, FLAC, AC3/E-AC3, DTS, uncompressed PCM.
4. Note user instruction in R3: "Update AGENTS.md to remove obsolete prohibitions against multimedia backends and define the new GUI and codec standards."
5. Evaluate best Rust libraries/frameworks to fulfill these requirements (e.g. ffmpeg-next / system FFmpeg, symphonia for audio/containers, pure-Rust decoders, cpal / rodio for audio playback). Check system libraries available on Gautham's Fedora Linux (e.g. pkg-config, ffmpeg-devel / libav* libraries).
6. Propose a concrete architecture for demuxing and decoding that converts packets into planar VideoFrameYUV420p / VideoFrameRGBA and AudioFramePCM float/i16 samples, integrating seamlessly with syncplay-clock MasterClock, SPSC ring buffers, and syncplay-sync.
