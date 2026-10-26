# BRIEFING — 2026-10-25T21:24:00Z

## Mission
Formulate the exact architectural design for universal video decoding in syncplay-codecs (H.264, H.265, VP8, VP9, AV1, MPEG-2, MPEG-4, VC-1) with aligned VideoFrameYUV420p mapping and RGBA conversion.

## 🔒 My Identity
- Archetype: explorer
- Roles: investigator, analyzer, synthesizer, reporter
- Working directory: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_2
- Original parent: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
- Milestone: milestone_2

## 🔒 Key Constraints
- Read-only investigation — do NOT modify source code files in repository (provide code prototypes and designs for implementer)
- Add ffmpeg-next = "9.0.0" to crates/syncplay-codecs/Cargo.toml
- Video codec coverage: H.264, H.265, VP8, VP9, AV1, MPEG-2, MPEG-4, VC-1
- Strict memory alignment invariants for VideoFrameYUV420p:
  * 64-byte aligned planar heap buffers (y_plane, u_plane, v_plane)
  * 16-byte aligned row strides ((width + 15) & !15)
  * +16B SIMD tail padding at end of each plane
- Support conversion to VideoFrameRGBA via in-tree Q8 fixed-point converter or swscale
- Enforce AGENTS.md rules: no banned crates (no mpv, vlc, sqlite, etc.), #![deny(warnings)], safety comments for unsafe blocks

## Current Parent
- Conversation ID: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
- Updated: 2026-10-25T21:21:56Z

## Investigation State
- **Explored paths**: `crates/syncplay-codecs/Cargo.toml`, `src/frame.rs`, `src/color.rs`, `src/error.rs`, `src/h264/`, `crates/syncplay-demux/src/traits.rs`, `ffmpeg-next-9.0.0` crate internals, `/usr/lib64/` FFmpeg shared libraries, sysroot `.deps/`.
- **Key findings**:
  1. All 8 video decoders (H.264, H.265/HEVC, VP8, VP9, AV1 via libdav1d, MPEG-2, MPEG-4, VC-1) are present and functional in Fedora's libavcodec.
  2. VC-1 requires extradata or global sequence header to initialize in FFmpeg; a 16-byte fallback header ensures safe initialization even if container omits extradata.
  3. Planar row mapping into `VideoFrameYUV420p` strictly preserves 64B heap buffer alignment, 16B row strides (`(width + 15) & !15`), and +16B SIMD tail padding.
  4. Non-YUV420P formats (NV12, P010, RGB) are normalized via `swscale`.
  5. RGBA presentation frames are generated either via in-tree Q8 fixed-point converter (`convert_yuv420p_to_rgba`) or `swscale`.
  6. End-to-end packet decoding with draining at EOF verified with 100% assertions passing.
- **Unexplored areas**: None within universal video decoding scope.

## Key Decisions Made
- `UniversalVideoDecoder` wraps `ffmpeg_next::decoder::Video` with lazy `swscale` context.
- Constructors support `from_parameters(&Parameters)`, `from_codec_id(Id, ...)`, and `from_codec_name(&str, ...)`.
- Multi-threading enabled via `ffmpeg_next::threading::Type::Frame` for optimal CPU core utilization.

## Artifact Index
- DISPATCH.md — Task dispatches and requests
- progress.md — Liveness heartbeat and step tracking
- BRIEFING.md — Persistent context index
- proposed_universal_video.rs — Complete Rust prototype for UniversalVideoDecoder
- syncplay_codecs_video.patch — Precise unified diff patch for crates/syncplay-codecs
- handoff.md — 5-component structured investigation and design report
