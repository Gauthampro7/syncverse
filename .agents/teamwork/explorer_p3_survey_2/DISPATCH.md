# Dispatch: explorer_p3_survey_2

## Identity
- Role: Technical Explorer (Survey R3)
- Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_survey_2
- Authoritative User Request: /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md

## Mission
Conduct an exhaustive technical survey of:
1. Pure-Rust H.264 / AVC Bitstream Parsing: Annex B NAL parsing (`0x000001` / `0x00000001`), SPS parameter decoding (profile, level, pic dimensions, crop offsets), PPS decoding, slice header parsing, and frame reconstruction into planar `VideoFrameYUV420p`.
2. Audio packet demuxing and payload unpacking: PCM and Vorbis/Opus packet extraction from MKV/MP4 yielding standard float samples in `AudioFramePCM`.
3. High-performance Q8 fixed-point integer color space converter ($Y'CbCr \to R'G'B'$) outputting to 32-bit packed RGBA and terminal TrueColor rendering buffers.

## Output
Write your findings to `survey_report.md` and complete with `handoff.md` in your working directory.

## 2026-10-25T15:26:33Z
You are explorer_p3_survey_2, a technical explorer for Phase 3 of Syncplay-Alt.
Your working directory is /home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_survey_2.
The workspace root is /home/gautham/syncplay-alt.

Read your dispatch instructions in:
/home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_survey_2/DISPATCH.md
and the authoritative user request in:
/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically the latest request under ## 2026-10-25T15:21:44Z).

Your mission is to explore and survey:
R3: Pure-Rust Video & Audio Bitstream Decoding Engine
1. H.264 / AVC bitstream parser in pure safe Rust (zero external C/C++ or crates):
   - Annex B NAL unit parsing (`0x000001` / `0x00000001` start codes, emulation prevention byte `0x000003` handling).
   - SPS (Sequence Parameter Set) parameter decoding using Exp-Golomb bitstream reading (profile_idc, level_idc, pic_width_in_mbs, pic_height_in_map_units, frame_mbs_only_flag, frame_cropping offsets).
   - PPS (Picture Parameter Set) decoding (pic_parameter_set_id, seq_parameter_set_id, entropy_coding_mode_flag).
   - Slice header parsing (first_mb_in_slice, slice_type, pic_parameter_set_id, frame_num, pic_order_cnt_lsb).
   - Frame reconstruction into planar `VideoFrameYUV420p`.
2. Audio packet demuxing and payload unpacking:
   - PCM and Vorbis/Opus packet demuxer and payload unpacker yielding standard interleaved or planar `AudioFramePCM` float samples (`Vec<f32>` normalized to [-1.0, 1.0]).
3. High-performance Q8 fixed-point integer color space converter ($Y'CbCr \to R'G'B'$):
   - Outputting to 32-bit packed RGBA and terminal truecolor rendering buffers.

Inspect existing code in `crates/syncplay-demux`, `crates/syncplay-codecs`, `crates/syncplay-common`.
Write a comprehensive report to `survey_report.md` in your working directory and deliver a complete `handoff.md`.
Communicate back with send_message to orchestrator_3 (my conversation ID is ecd27bad-3488-46aa-ba26-8b0b7cecf0eb).
