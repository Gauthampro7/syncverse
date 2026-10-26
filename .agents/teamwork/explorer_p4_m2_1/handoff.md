# Handoff Report: Universal Container Demuxing Architecture (`syncplay-demux`)

**Agent**: `explorer_p4_m2_1`  
**Date**: 2026-10-25T21:24:00Z  
**Context**: Phase 4 Transformation — Universal Container Demuxing (Milestone 2, Feature 5)  
**Destination**: `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_1/handoff.md`  

---

## 1. Observation

### 1.1 Existing Demuxer Architecture & Crate Contracts
- In `crates/syncplay-demux/Cargo.toml` (lines 8–10):
  ```toml
  [dependencies]
  syncplay-common = { path = "../syncplay-common" }
  thiserror.workspace = true
  ```
- In `crates/syncplay-demux/src/traits.rs` (lines 54–81):
  ```rust
  pub struct DemuxPacket {
      pub track_id: u64,
      pub pts_ms: u64,
      pub dts_ms: Option<u64>,
      pub is_keyframe: bool,
      pub data: Vec<u8>,
  }

  pub trait Demuxer: Send {
      fn probe(reader: &mut (dyn ReadSeek + Send)) -> Result<bool, DemuxError>;
      fn init(&mut self, reader: Box<dyn ReadSeek + Send>) -> Result<ContainerInfo, DemuxError>;
      fn read_packet(&mut self) -> Result<Option<DemuxPacket>, DemuxError>;
      fn seek_to(&mut self, target_pts_ms: u64) -> Result<u64, DemuxError>;
  }
  ```
- In `crates/syncplay-demux/src/lib.rs` (lines 3–5):
  ```rust
  #![deny(warnings)]
  #![deny(missing_docs)]
  #![deny(unsafe_code)]
  ```
- In `crates/syncplay-demux/tests/demux_tests.rs`: Existing tests validate `EbmlDemuxer` and `IsoBmffDemuxer` on in-tree synthetic byte vectors (`build_synthetic_full_mkv`, `build_synthetic_full_mp4`).

### 1.2 FFmpeg Bindings & Host Environment Probing
- In `/home/gautham/syncplay-alt/.deps/env.sh`, environment variables supply user-space development headers and libraries:
  ```bash
  export LIBCLANG_PATH="/home/gautham/syncplay-alt/.deps/usr/lib64"
  export PKG_CONFIG_PATH="/home/gautham/syncplay-alt/.deps/usr/lib64/pkgconfig:${PKG_CONFIG_PATH:-}"
  export BINDGEN_EXTRA_CLANG_ARGS="-I/usr/lib/gcc/x86_64-redhat-linux/16/include"
  export RUSTFLAGS="-L /home/gautham/syncplay-alt/.deps/usr/lib64 ${RUSTFLAGS:-}"
  ```
- `ffmpeg-next = "9.0.0"` compiles cleanly in the Fedora 44 environment against system `libav*` (libavcodec 62, libavformat 62, libavutil 60).
- `ffmpeg-next::format::context::StreamIo::from_read_seek(reader)` directly wraps any `Box<dyn ReadSeek + Send>` for in-memory stream demuxing via `ffmpeg-next::format::input_from_stream`.
- Probing via `ffmpeg::ffi::av_probe_input_format(&probe_data, 1)` accurately identifies container formats from the first 4096 bytes.
- Synthetic full MP4 fixtures (`build_synthetic_full_mp4()`) fail under libavformat stream info detection (`ffmpeg::Error(1094995529: Invalid data found when processing input)`) because sample payloads are synthetic dummy bytes (`0xAA`), proving that existing in-tree demuxers (`EbmlDemuxer`, `IsoBmffDemuxer`) must remain intact for synthetic unit test suites while `UniversalDemuxer` serves universal media formats.

### 1.3 Empirical Verification Across All 8 Container Fixtures
We generated test fixtures using system `ffmpeg`:
1. Matroska (`sample.mkv`, H.264 + AAC)
2. WebM (`sample.webm`, VP8 + Vorbis)
3. MP4 (`sample.mp4`, H.264 + AAC)
4. QuickTime (`sample.mov`, H.264 + AAC)
5. AVI (`sample.avi`, MPEG-4 + MP3)
6. MPEG-TS (`sample.ts`, H.264 + AAC)
7. Flash Video (`sample.flv`, FLV1 + MP3)
8. Ogg (`sample.ogv`, Theora + Vorbis)

Execution of our `UniversalDemuxer` prototype on all 8 fixtures yielded:
- **Probing**: 100% matched (`probe = true`). Non-media and truncated buffers rejected (`probe = false`).
- **Metadata**: Exact stream identification (video width/height, audio sample rate/channels, duration in ms).
- **Sequential Demuxing**: Continuous extraction of sequential `DemuxPacket` payloads with validated timestamps.
- **Keyframe Seeking**: `seek_to(500)` seeks backwards to the preceding keyframe, sets `peeked_packet`, and resumes packet reading with zero packet drops.

---

## 2. Logic Chain

1. **Premise 1**: The user request and Milestone 2 require universal container compatibility across Matroska (`.mkv`, `.webm`), ISOBMFF (`.mp4`, `.m4v`, `.mov`), AVI (`.avi`), MPEG-TS (`.ts`), FLV (`.flv`), and OGG (`.ogg`, `.ogv`).
2. **Premise 2**: Pure-Rust container parsers for MPEG-TS, FLV, and complex AVI formats either do not exist or lack random-access seek tables and codec parameter extraction.
3. **Inference 2.1**: Delegating universal demuxing to `ffmpeg-next = "9.0.0"` provides comprehensive, battle-tested container demuxing, stream probing, and keyframe indexing across 100% of the required formats.
4. **Premise 3**: Downstream codecs (`syncplay-codecs`) and the master clock pipeline (`syncplay-clock`, `syncplay-sync`) expect `DemuxPacket` with exact presentation timestamps in integer milliseconds (`pts_ms: u64`).
5. **Inference 2.2**: Rational timescale conversion using 128-bit integer arithmetic avoids float precision loss and prevents 64-bit overflow:
   $$\text{pts}_{\text{ms}} = \max\left(0, \frac{\text{raw\_pts} \times \text{tb.num} \times 1000}{\text{tb.den}}\right)$$
6. **Premise 4**: Random-access seeking (`seek_to(target_pts_ms)`) must land on the nearest keyframe preceding or at the target timestamp, and the demuxer must return the exact landed PTS while guaranteeing that the subsequent `read_packet()` returns that keyframe.
7. **Inference 2.3**: By invoking `ictx.seek(target_us, ..target_us)` (where `target_us = target_pts_ms * 1000`) and immediately pre-reading the first packet into `peeked_packet: Option<DemuxPacket>`, `UniversalDemuxer` determines the exact landed PTS and returns it directly, ensuring zero packet loss on subsequent reads.
8. **Premise 5**: Existing unit tests depend on synthetic vectors (`SYNTHETIC_EBML_VECTOR`, `SYNTHETIC_MP4_VECTOR`) which contain synthetic dummy payload bytes not recognizable by FFmpeg's strict codec bitstream parsers.
9. **Inference 2.4**: Retaining `EbmlDemuxer` and `IsoBmffDemuxer` side-by-side with `UniversalDemuxer` preserves 100% backward compatibility for all existing tests while adding universal production demuxing.

---

## 3. Caveats

1. **Build Environment Prerequisite**: Building `ffmpeg-next` requires development sysroot environment variables (`LIBCLANG_PATH`, `PKG_CONFIG_PATH`, `BINDGEN_EXTRA_CLANG_ARGS`, `RUSTFLAGS`), which are already provided in `.deps/env.sh`. Build scripts and CI commands must source `.deps/env.sh`.
2. **Unsafe Block in C Probing**: Probing via `av_probe_input_format` requires one localized `unsafe` call to pass `*const AVProbeData`. In accordance with `AGENTS.md`, this call is documented with a formal `// SAFETY:` rationale, and `#![deny(unsafe_code)]` is removed from `crates/syncplay-demux/src/lib.rs`.
3. **Subsequent Codec Decoding Scope**: `DemuxPacket` contains compressed bitstream payloads (`Vec<u8>`). Video and audio decoding into planar `VideoFrameYUV420p` and normalized `AudioFramePCM` is scoped to sibling specialists `explorer_p4_m2_2` and `explorer_p4_m2_3`.

---

## 4. Conclusion

1. **Architecture Formulation**: Universal container demuxing in `syncplay-demux` is achieved via `UniversalDemuxer` backed by `ffmpeg-next = "9.0.0"`.
2. **Trait Compliance**: `UniversalDemuxer` fully implements the `Demuxer` trait (`probe`, `init`, `read_packet`, `seek_to`), accepts any `Box<dyn ReadSeek + Send>` via `StreamIo::from_read_seek`, and provides a convenience `UniversalDemuxer::open_file<P: AsRef<Path>>(path: P)` constructor.
3. **Timescale Arithmetic**: Exactly converts container stream timescales to integer milliseconds using 128-bit rational math: `pts_ms = (raw_pts as i128 * tb.num * 1000) / tb.den`.
4. **Keyframe Seeking**: Employs backwards-bounded microsecond seeking (`ictx.seek(target_us, ..target_us)`) with peeked-packet buffering, guaranteeing exact landed keyframe PTS reporting and zero packet loss.
5. **Artifacts Delivered in Working Directory**:
   - `universal_demuxer.patch`: Machine-applicable unified diff patch tested with `git apply --check` (exited 0).
   - `proposed_universal.rs`: Complete drop-in source for `crates/syncplay-demux/src/universal.rs`.
   - `proposed_universal_demux_tests.rs`: Complete drop-in source for `crates/syncplay-demux/tests/universal_demux_tests.rs`.

---

## 5. Verification Method

To independently verify the implementation:

### 5.1 Verification Commands
```bash
# 1. Source user-space dependency environment
source /home/gautham/syncplay-alt/.deps/env.sh

# 2. Check patch applicability without modifying repository
git apply --check /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_1/universal_demuxer.patch

# 3. Apply patch (when implementing in worker stage)
git apply /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_1/universal_demuxer.patch

# 4. Verify compilation with zero compiler warnings
cargo check --workspace --all-targets

# 5. Execute all syncplay-demux unit and integration tests (38 tests: 8 lib + 5 demux + 15 ebml + 6 isobmff + 4 universal)
cargo test -p syncplay-demux -- --nocapture

# 6. Verify zero clippy warnings under strict deny rules
cargo clippy -p syncplay-demux --all-targets -- -D warnings
```

### 5.2 Invalidation Conditions
- Any test failure among the 38 unit and integration tests under `cargo test -p syncplay-demux`.
- Any compiler warning or Clippy warning under `cargo clippy -p syncplay-demux --all-targets -- -D warnings`.
- `git apply --check` failing to match target files.
- `UniversalDemuxer::probe` returning false for any of the 8 standard container formats.
- Packet PTS drift or miscalculation where `pts_ms != (raw_pts * tb.num * 1000 / tb.den)`.
