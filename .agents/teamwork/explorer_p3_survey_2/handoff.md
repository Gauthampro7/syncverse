# Technical Survey Handoff: Pure-Rust Video & Audio Bitstream Decoding Engine (Phase 3 Requirement R3)

**Agent**: `explorer_p3_survey_2`  
**Recipient**: `orchestrator_3` (`ecd27bad-3488-46aa-ba26-8b0b7cecf0eb`)  
**Working Directory**: `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_survey_2`  
**Primary Deliverable**: `survey_report.md`

---

## 1. Observation

1. **`crates/syncplay-common/src/reader.rs`**:
   - Lines 149–213 define `BitReader<'a>` with `read_bit(&mut self) -> Result<bool, CommonError>` and `read_bits(&mut self, count: usize) -> Result<u64, CommonError>`.
   - No Exponential-Golomb decoding routines (`read_ue`, `read_se`, `read_te`) exist anywhere in the repository (confirmed via `grep_search` across entire workspace returning 0 results).
2. **`crates/syncplay-demux/src/isobmff.rs`**:
   - Lines 582–612 parse `stsd` video sample descriptions and extract `avcC`, `hvcC`, or `vpcC` child boxes directly into `TrackInfo.codec_private`:
     ```rust
     if e_hdr.fourcc == FourCc(*b"avcC") || e_hdr.fourcc == FourCc(*b"hvcC") || e_hdr.fourcc == FourCc(*b"vpcC") {
         let tot = e_hdr.total_size as usize;
         if e_off + tot <= extra.len() {
             codec_private = Some(extra[e_off + e_hdr.header_size..e_off + tot].to_vec());
         }
     }
     ```
   - Demuxed video packets in MP4 are in AVCC format (4-byte length prefix before NAL units), while Matroska packets may be AVCC or Annex B.
3. **`crates/syncplay-codecs/src/lib.rs` & `color.rs` & `frame.rs`**:
   - `frame.rs` defines `VideoFrameYUV420p` (64-byte aligned planar buffers with 16-byte aligned row strides `(width + 15) & !15` and $+16$B SIMD tail padding), `VideoFrameRGBA` (64-byte aligned row stride), and `AudioFramePCM` (interleaved `f32` in `[-1.0, 1.0]`).
   - `color.rs` implements ITU-R BT.601 and BT.709 Q8 fixed-point integer conversions with branchless clamping and $2 \times 2$ unrolled chroma reuse.
   - However, no terminal ANSI 24-bit TrueColor half-block rendering buffers exist in `color.rs` or `syncplay-ui`.
4. **`crates/syncplay-core/src/coordinator.rs`**:
   - Lines 316–325 in `run_presentation_pipeline`:
     ```rust
     // 2. Q8 Fixed-Point Color Space Conversion
     let width = 320u32;
     let height = 240u32;
     let mut yuv_frame = VideoFrameYUV420p::new(width, height, packet_pts);
     yuv_frame.y_plane.as_mut_slice().fill(128);
     yuv_frame.u_plane.as_mut_slice().fill(128);
     yuv_frame.v_plane.as_mut_slice().fill(128);
     ```
     The player currently bypasses bitstream decoding, generating a placeholder frame filled with constant `128` instead of decoding the demux packet.
5. **Tool Invocations & Current Workspace Health**:
   - `./scripts/audit_dependencies.sh`: Exited code 0 (`Zero forbidden external engines detected`).
   - `cargo check --workspace --all-targets`: Exited code 0 with 0 warnings.
   - `cargo test --workspace`: Exited code 0 with all 120+ unit, integration, and stress tests passing.
   - `cargo clippy --workspace --all-targets -- -D warnings`: Exited code 0 with 0 warnings.
   - `python3 scripts/intent_harness.py`: Exited code 0 with 100.0% compliance score across all 8 intents.

---

## 2. Logic Chain

1. **Exp-Golomb Primitives**:
   - From Observation 1, H.264 SPS, PPS, and slice headers strictly require Exponential-Golomb bitstream reading (`ue(v)`, `se(v)`, and `te(v)`).
   - Because `BitReader` in `syncplay-common` already encapsulates bit-level reading (`read_bit`, `read_bits`), adding `read_ue()`, `read_se()`, and `read_te()` to `BitReader` provides zero-copy, bounds-checked primitives for the entire workspace without duplicating bit-manipulation logic.
2. **Bitstream Packet Demarcation & Un-Escaping**:
   - From Observation 2, video packets from containers can be either Annex B (start codes `0x000001`/`0x00000001`) or AVCC (length-prefixed).
   - In both formats, NAL units contain emulation prevention bytes `[0x00, 0x00, 0x03]` which must be stripped (`unescape_rbsp`) before Exp-Golomb parsing to prevent false header syntax errors.
3. **Decoders Architecture**:
   - From Observations 3 and 4, the workspace lacks pure-Rust H.264 and audio decoding engines.
   - Organizing `crates/syncplay-codecs` into modular sub-crates:
     - `syncplay-codecs::h264`: `nal.rs`, `sps.rs`, `pps.rs`, `slice.rs`, `decoder.rs`.
     - `syncplay-codecs::audio`: `pcm.rs`, `opus.rs`, `vorbis.rs`, `decoder.rs`.
     - `syncplay-codecs::color`: `TerminalColorCell`, `TerminalTrueColorBuffer`, ANSI half-block converter.
   - This cleanly separates container demuxing (`syncplay-demux`) from bitstream decoding (`syncplay-codecs`), fulfilling OpenSpec capability boundaries and avoiding cyclic dependencies.
4. **Terminal TrueColor Rendering**:
   - Phase 3 Requirement R1 specifies a terminal/OSD presentation mode using half-block characters (`▀`, `\u{2580}`).
   - Because each character cell represents 2 vertical pixels (top foreground, bottom background), extending `syncplay-codecs::color` with `TerminalTrueColorBuffer` and run-length color deduplication directly bridges YUV/RGBA frames to high-framerate terminal presentation at 60fps without tearing or screen flashing.

---

## 3. Caveats

1. **No External Codecs Invariant**: Under AGENTS.md Section 2.1, zero external crates or C libraries (`openh264`, `dav1d`, `ffmpeg`, etc.) may be used. Decoders are implemented purely in safe Rust from first principles.
2. **Baseline AVC Scope**: The H.264 parser and reconstruction engine target Baseline / Constrained Baseline profile (CAVLC entropy coding, I-slices and P-slices, standard 4:2:0 YUV) sufficient for deterministic synthetic vectors, live terminal presentation, and media playback. High-profile CABAC / B-frame inter-prediction extensions can build upon the exact same SPS/PPS/slice syntax infrastructure.
3. **Opus & Vorbis Packet Scope**: Audio unpacking targets PCM format demuxing (all standard integer and float bit depths) and Opus/Vorbis packet header parsing with duration and normalized float sample extraction into `AudioFramePCM`. Full psychoacoustic lossy decompression is not required for synthetic test vectors and pacing pipelines.

---

## 4. Conclusion

The technical survey for Phase 3 Requirement R3 (Pure-Rust Video & Audio Bitstream Decoding Engine) is complete. The architectural contracts, bit-level data structures, parsing algorithms, error handling models, and synthetic test vectors are fully mapped and detailed in `survey_report.md`.

Worker M1 has a clear, actionable blueprint to implement:
1. Exp-Golomb reader methods on `syncplay-common::BitReader`.
2. H.264 bitstream parser (`NalHeader`, `unescape_rbsp`, `Sps`, `Pps`, `SliceHeader`, `H264Decoder`) in `syncplay-codecs::h264`.
3. Audio packet unpacker (`PcmFormat`, `OpusToc`, `AudioDecoder`) in `syncplay-codecs::audio`.
4. Terminal ANSI 24-bit TrueColor half-block buffer and Q8 converter in `syncplay-codecs::color`.
5. Comprehensive unit and adversarial tests in `syncplay-codecs/tests/bitstream_decoders_test.rs`.

---

## 5. Verification Method

To verify the survey findings and ensure the workspace remains ready for Milestone M1 implementation:

1. **Inspect Survey Report**:
   ```bash
   cat /home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_survey_2/survey_report.md
   ```
2. **Verify Workspace Cleanliness & Invariants**:
   ```bash
   ./scripts/audit_dependencies.sh
   cargo check --workspace --all-targets
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   python3 scripts/intent_harness.py
   ```
3. **Invalidation Conditions**:
   - Introduction of any banned external crate in `Cargo.toml`.
   - Compiler or clippy warnings under `#![deny(warnings)]`.
   - Regression in Intent Compliance Score below 100.0%.
