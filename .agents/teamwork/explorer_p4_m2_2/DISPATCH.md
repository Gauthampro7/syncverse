# DISPATCH — explorer_p4_m2_2

## Mission
You are `explorer_p4_m2_2`, the Universal Video Decoding Specialist for Milestone 2.
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_2`.

You must read:
1. `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically `## 2026-10-25T20:12:42Z`)
2. `/home/gautham/syncplay-alt/.agents/teamwork/orchestrator_4/PROJECT.md`
3. Current `crates/syncplay-codecs/Cargo.toml` and `crates/syncplay-codecs/src/frame.rs`, `lib.rs`
4. `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_2/handoff.md`

## Scope & Objective
Formulate the exact architectural design for universal video decoding in `syncplay-codecs`:
1. Video codec coverage: H.264 (AVC), H.265 (HEVC), VP8, VP9, AV1, MPEG-2, MPEG-4, VC-1.
2. Add `ffmpeg-next = "9.0.0"` dependency to `crates/syncplay-codecs/Cargo.toml`.
3. Design `UniversalVideoDecoder`:
   - Initialize decoder context from codec parameters.
   - Decode video packets (`DemuxPacket`) and extract planar frame planes.
   - Copy or map planes into `VideoFrameYUV420p` adhering strictly to memory alignment invariants:
     * 64-byte aligned planar heap buffers (`y_plane`, `u_plane`, `v_plane`).
     * 16-byte aligned row strides (`(width + 15) & !15`).
     * $+16$B SIMD tail padding at the end of each plane buffer.
   - Provide conversion to packed `VideoFrameRGBA` via in-tree Q8 fixed-point color space conversion (`convert_yuv420p_to_rgba` for BT.601 / BT.709) or `swscale`.
4. Ensure zero compiler warnings and memory safety.
5. Provide code prototypes and verification commands.

## Output Requirements
Document your complete design, code diffs, and verification commands in `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_2/handoff.md`.
Send a completion message to the orchestrator when done.
## 2026-10-25T21:11:04Z
From: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
Content:
You are explorer_p4_m2_2.
Your working directory is: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_2
Read:
1. /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically ## 2026-10-25T20:12:42Z)
2. /home/gautham/syncplay-alt/.agents/teamwork/orchestrator_4/PROJECT.md
3. /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_2/DISPATCH.md
4. crates/syncplay-codecs/Cargo.toml, crates/syncplay-codecs/src/frame.rs, lib.rs
5. /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_survey_2/handoff.md

Task:
Formulate the exact architectural design for universal video decoding in syncplay-codecs:
- Add ffmpeg-next = "9.0.0" to crates/syncplay-codecs/Cargo.toml.
- Design UniversalVideoDecoder supporting H.264, H.265, VP8, VP9, AV1, MPEG-2, MPEG-4, VC-1.
- Map decoded frames into in-tree VideoFrameYUV420p adhering strictly to memory alignment invariants:
  * 64-byte aligned planar heap buffers (y_plane, u_plane, v_plane).
  * 16-byte aligned row strides ((width + 15) & !15).
  * +16B SIMD tail padding at end of each plane.
- Support conversion to VideoFrameRGBA via in-tree Q8 fixed-point converter or swscale.
- Provide code prototypes and verification commands.

Maintain progress.md in your working directory.
When done, write your structured report to /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_2/handoff.md and call send_message to report completion to the orchestrator.
## 2026-10-25T21:21:56Z
From: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
Content:
**Context**: Milestone 2 Universal Video Decoding Survey
**Content**: Status check on `explorer_p4_m2_2`. Please report your current progress on `UniversalVideoDecoder` design and memory alignment mapping for `VideoFrameYUV420p` / `VideoFrameRGBA`.
**Action**: Update `progress.md` and report current status.
