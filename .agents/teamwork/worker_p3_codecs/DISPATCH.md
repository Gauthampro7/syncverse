# Dispatch: worker_p3_codecs

## Identity
- Role: Implementation Worker (Milestone 1: Pure-Rust Video & Audio Bitstream Decoding Engine)
- Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/worker_p3_codecs
- Authoritative User Request: /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md
- Survey Architecture Blueprint: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_survey_2/survey_report.md
- Handoff Report: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_survey_2/handoff.md

## Scope & Write Ownership
You exclusively own:
- `crates/syncplay-common/src/reader.rs` (Exp-Golomb `read_ue`, `read_se`, `read_te`)
- `crates/syncplay-codecs/src/` (H.264 bitstream parser, SPS/PPS/slice header decoding, YUV420p frame reconstruction, Audio packet demuxer/unpacker for PCM and Vorbis/Opus into `AudioFramePCM`, Q8 color converter & ANSI TrueColor half-block buffer)
- `crates/syncplay-codecs/tests/` (unit and integration tests for bitstream decoders)
- `crates/syncplay-demux/` (if minor glue adjustments needed for packet extraction)

## Mandatory Integrity Warning
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## Coding Standards & Invariants
1. Zero compiler warnings (`cargo check --workspace --all-targets`).
2. Zero clippy warnings (`cargo clippy --workspace --all-targets -- -D warnings`).
3. Mandatory documentation (`#![deny(missing_docs)]`).
4. `#![deny(unsafe_code)]` unless justified with `// SAFETY:` comments.
5. Zero banned external dependencies (`./scripts/audit_dependencies.sh`).
6. All tests pass (`cargo test --workspace`).
7. 100% Intent compliance score (`python3 scripts/intent_harness.py`).

## Deliverables
- Implement all components according to `survey_report.md`.
- Run tests and linters to verify.
- Write `handoff.md` in your working directory and notify orchestrator_3.

## 2026-10-25T15:39:56Z
From: ecd27bad-3488-46aa-ba26-8b0b7cecf0eb
Content:
You are worker_p3_codecs, the implementation worker for Milestone 1 of Syncplay-Alt Phase 3.
Your working directory is /home/gautham/syncplay-alt/.agents/teamwork/worker_p3_codecs.
The workspace root is /home/gautham/syncplay-alt.

Read your dispatch instructions in:
/home/gautham/syncplay-alt/.agents/teamwork/worker_p3_codecs/DISPATCH.md
the authoritative user request in:
/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically the latest request under ## 2026-10-25T15:21:44Z),
and the architecture & implementation blueprints in:
/home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_survey_2/survey_report.md
/home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_survey_2/handoff.md

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Your task is to implement Requirement R3: Pure-Rust Video & Audio Bitstream Decoding Engine:
1. `syncplay-common::BitReader`: Add Exp-Golomb decoding routines (`read_ue`, `read_se`, `read_te`).
2. `syncplay-codecs`:
   - H.264 / AVC bitstream parser:
     * Annex B NAL unit parsing (`0x000001` / `0x00000001` start codes, AVCC length prefix support, emulation prevention un-escaping `0x000003` -> `unescape_rbsp`).
     * SPS parameter decoding (profile_idc, level_idc, pic_width_in_mbs_minus1, pic_height_in_map_units_minus1, frame_mbs_only_flag, frame_cropping offsets).
     * PPS parameter decoding (pic_parameter_set_id, seq_parameter_set_id, entropy_coding_mode_flag).
     * Slice header parsing (first_mb_in_slice, slice_type, pic_parameter_set_id, frame_num, pic_order_cnt_lsb).
     * Planar `VideoFrameYUV420p` reconstruction (16-byte row stride alignment `(width + 15) & !15`, 64-byte aligned planes, +16B SIMD tail padding).
   - Audio packet demuxing and payload unpacking:
     * PCM formats (8, 16, 24, 32-bit int, 32-bit float) and Vorbis/Opus packet demuxer and payload unpacker yielding standard interleaved or planar `AudioFramePCM` float samples in `[-1.0, 1.0]`.
   - Q8 Fixed-point color converter & ANSI 24-bit TrueColor rendering:
     * `TerminalTrueColorBuffer` converting frames to ANSI 24-bit TrueColor upper-half-block (`▀` `U+2580`) cells with color-change run-length deduplication (< 18KB/frame).
3. Unit and integration tests in `crates/syncplay-codecs/tests/` verifying all decoders against synthetic test vectors.
4. Verify using:
   - `./scripts/audit_dependencies.sh`
   - `cargo check --workspace --all-targets`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace`
   - `python3 scripts/intent_harness.py`

Write a comprehensive `handoff.md` in your working directory and notify orchestrator_3 via send_message (my conversation ID is ecd27bad-3488-46aa-ba26-8b0b7cecf0eb).
