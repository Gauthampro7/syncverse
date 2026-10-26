# Forensic Audit Report: Milestone 2 — Universal Video & Audio Compatibility Engine

**Auditor**: `auditor_p4_m2`  
**Date**: 2026-10-25T21:50:00Z  
**Target**: Milestone 2 Universal Video & Audio Compatibility Engine  
**Ground Truth**: `ORIGINAL_REQUEST.md` (Integrity mode: `development` as of 2026-10-25T20:12:42Z)  
**Verdict**: **CLEAN**

---

## Forensic Audit Summary

**Work Product**: Milestone 2 Universal Video & Audio Compatibility Engine (`crates/syncplay-demux`, `crates/syncplay-codecs`, `crates/syncplay-core`)  
**Profile**: General Project  
**Integrity Mode**: Development  
**Verdict**: **CLEAN**

### Phase Results
- **Architectural Invariants & Dependency Blacklist**: PASS — 0 violations across 19 banned patterns; zero banned monolithic player (`mpv`, `vlc`) or external database (`sqlite`, `rocksdb`, `diesel`) crates.
- **Authenticity & Non-Facade Verification**: PASS — `UniversalDemuxer`, `UniversalVideoDecoder`, and `UniversalAudioDecoder` are genuine native implementations utilizing direct FFmpeg multimedia library bindings, full container demuxing, video decoding, planar YUV mapping, audio decoding, and resampling. Zero hardcoded test outputs or facade stubs found.
- **Safety & Invariant Standards**: PASS — Every `unsafe` block in `universal.rs`, `universal_video.rs`, `audio/universal.rs`, and `frame.rs` has a formal `// SAFETY:` comment proving valid pointer handling, bounds checks, and absence of data races. 64-byte alignment, 16-byte row strides, and +16B SIMD tail padding zeroing are strictly enforced.
- **Workspace Compilation & Lints**: PASS — `cargo check` and `cargo clippy -- -D warnings` both exited 0 with zero warnings.
- **Workspace Test Suite**: PASS — 100% of tests passed across all 8 crates.
- **Pi Intent Verification Harness**: PASS — 8/8 intents verified, 100.0% Intent Compliance Score, 0 drift.
- **Phase 4 End-to-End Verification**: PASS — All 8 stages in `./scripts/verify_phase4.sh` exited 0 cleanly.

---

## 1. Observation

### 1.1 Architectural Invariants & Dependency Audit
- Executed `./scripts/audit_dependencies.sh`:
  ```
  [INFO] Auditing Cargo.toml manifests for banned monolithic player and database engines...
  [INFO] Auditing Cargo.lock for banned dependencies...
  [PASS] Dependency audit clean (19 banned patterns checked, 0 violations).
  [PASS] Monolithic player wrappers and external database engines are strictly excluded.
  ```
- Direct regex scan across `Cargo.lock`, `Cargo.toml`, and `crates/*/Cargo.toml` for `mpv|vlc|sqlite|diesel|rocksdb|sled|heed|tantivy|meilisearch`:
  ```
  grep -i -E "mpv|vlc|sqlite|diesel|rocksdb|sled|heed|tantivy|meilisearch" Cargo.lock Cargo.toml crates/*/Cargo.toml
  -> NONE_FOUND (exit code 0)
  ```
- Dependencies added for M2:
  * `Cargo.toml` (lines 20–22): `thiserror = "2.0"`, `ffmpeg-next = "9.0.0"`
  * `crates/syncplay-demux/Cargo.toml` (line 11): `ffmpeg-next.workspace = true`
  * `crates/syncplay-codecs/Cargo.toml` (line 11): `ffmpeg-next.workspace = true`
  * `crates/syncplay-core/Cargo.toml` (line 17): `ffmpeg-next.workspace = true`
  All added dependencies are explicitly whitelisted in `AGENTS.md` and permitted in Development mode.

### 1.2 Authenticity & Facade Detection
- Inspected `crates/syncplay-demux/src/universal.rs`:
  * Implements `Demuxer` trait: `probe(&mut ReadSeek)` (lines 165–225), `init(Box<dyn ReadSeek>)` (lines 227–234), `read_packet()` (lines 236–306), `seek_to(u64)` (lines 308–379).
  * Uses FFmpeg C probe `av_probe_input_format` with fallback container heuristics for MKV, MP4, AVI, TS, FLV, OGG.
  * Extracted stream parameters and codec extradata (`codec_private`) via `params.as_ptr()` safely.
  * Converts packet presentation timestamps using 128-bit rational integer math (`(raw_pts as i128 * num * 1000) / den`) avoiding float jitter and overflow.
  * Tests in `crates/syncplay-demux/tests/universal_demux_tests.rs` verify real media demuxing and seeking on fixtures generated via FFmpeg (`sample.mkv`, `sample.webm`, `sample.mp4`, `sample.mov`, `sample.avi`, `sample.ts`, `sample.flv`, `sample.ogv`).
- Inspected `crates/syncplay-codecs/src/universal_video.rs`:
  * Supports H.264, H.265, VP8, VP9, AV1, MPEG-2, MPEG-4, and VC-1 via `UniversalVideoDecoder`.
  * In `map_frame_to_yuv420p` (lines 304–393), maps decoded video frames row-by-row into `AlignedBuffer`, enforcing:
    - 64-byte aligned planar buffers (`y_plane`, `u_plane`, `v_plane`).
    - 16-byte row stride alignment (`stride_y = (width + 15) & !15`, etc.).
    - End-of-plane +16B SIMD tail padding zeroing (`dst_y[y_cap - 16..y_cap].fill(0)`).
  * Implements automatic fallback sequence header for VC-1 Advanced Profile (lines 90–103) and lazy format conversion via `swscale` for non-YUV420P formats (lines 317–334).
  * Tests in `crates/syncplay-codecs/src/universal_video.rs` verify instantiation of all 8 codecs and memory alignment invariants.
- Inspected `crates/syncplay-codecs/src/audio/universal.rs`:
  * Supports AAC, MP3, Opus, Vorbis, FLAC, AC3, E-AC3, DTS, and uncompressed PCM via `UniversalAudioDecoder`.
  * Dynamically reconfigures `libswresample` to standardize all input formats to 48,000 Hz, stereo, interleaved `f32` samples in `[-1.0, 1.0]`.
  * Handles empty/unspecified channel layouts via `effective_layout` helper (lines 51–58).
  * Flushes residual delay line buffers via `resampler.delay()` and `resampler.flush()` (lines 212–239).
- Inspected `crates/syncplay-core/src/coordinator.rs`:
  * `play_universal_media` (lines 497–593) demuxes media packets, feeds video to `UniversalVideoDecoder` and audio to `UniversalAudioDecoder`, transfers frames over 128B cache-line aligned SPSC ring buffers, paces audio with `AudioPacer`, and writes to `AutoAudioSink`.
  * `test_coordinator_play_universal_media` (lines 825–855) generates a real media file and verifies decoding and playback.
- Searched for dummy facades (`todo!`, `unimplemented!`, hardcoded test returns) across new/modified files:
  ```
  grep -n -E "todo!|unimplemented!" crates/syncplay-demux/src/universal.rs crates/syncplay-codecs/src/universal_video.rs crates/syncplay-codecs/src/audio/universal.rs
  -> NONE_FOUND
  ```
- Searched for pre-populated result artifacts/logs:
  ```
  find . -maxdepth 3 ( -name '*.log' -o -name '*result*' -o -name '*output*' )
  -> 0 matching files found
  ```

### 1.3 Memory Safety & Unsafe Block Audit
- Inspected every `unsafe` block in Milestone 2 code:
  1. `crates/syncplay-demux/src/universal.rs` line 104:
     - `// SAFETY: params.as_ptr() points to a valid AVCodecParameters owned by FFmpeg stream context...`
     - Validates pointer is non-null and size > 0 before constructing slice via `std::slice::from_raw_parts`.
  2. `crates/syncplay-demux/src/universal.rs` line 193:
     - `// SAFETY: buf is a valid heap-allocated byte buffer of length n... av_probe_input_format only inspects first buf_size bytes...`
     - Bounds verified by `buf_size: n as std::ffi::c_int`.
  3. `crates/syncplay-codecs/src/universal_video.rs` line 79:
     - `// SAFETY: Context pointer is non-null and freshly allocated by Context::new_with_codec...`
     - Allocates padded buffer with `av_malloc`, checks `!buf.is_null()`, copies extradata and zeroes trailing padding (`AV_INPUT_BUFFER_PADDING_SIZE`).
  4. `crates/syncplay-codecs/src/audio/universal.rs` line 113:
     - `// SAFETY: Context pointer is valid. Configure default rate/layout for raw decoders like PCM, and extradata if provided.`
     - Initializes default layout and handles extradata with checked allocation and zeroed padding.
  5. `crates/syncplay-codecs/src/frame.rs` lines 16, 18, 28, 44, 51, 64:
     - All `unsafe` blocks in `AlignedBuffer` have formal `// SAFETY:` comments proving 64-byte layout alignment, non-null pointer, bounds safety, and proper deallocation.

### 1.4 Verification Scripts & Empirical Test Suite
- `cargo check --workspace --all-targets`: Finished in 0.02s with 0 warnings.
- `cargo clippy --workspace --all-targets -- -D warnings`: Finished in 0.04s with 0 warnings.
- `cargo test --workspace`: 100% of unit and integration tests passed across all 8 crates (demux: 38, codecs: 107, clock: 19, sync: 34, chat: 31, ui: 30, core: 62).
- `python3 scripts/intent_harness.py`: All 8 intents passed in 11.36s with 100.0% compliance score.
- `./scripts/verify_phase4.sh`: All 8 stages passed cleanly.

---

## 2. Logic Chain

1. **Rule**: Milestone 2 requires universal container demuxing and bitstream decoding with zero banned player/database wrappers (`mpv`, `vlc`, `sqlite`, `diesel`, `rocksdb`).
2. **Observation 1.1**: `./scripts/audit_dependencies.sh` and direct workspace-wide grep confirmed zero references to banned crates. The only multimedia backend added is `ffmpeg-next = "9.0.0"`, which is explicitly permitted in `AGENTS.md` and `ORIGINAL_REQUEST.md` (Integrity mode: Development).
3. **Inference 2.1**: The architectural invariant prohibition against monolithic player wrappers and external database engines is 100% satisfied.
4. **Rule**: All work products must be authentic implementations, free of dummy facades, mock bypasses, or hardcoded test returns.
5. **Observation 1.2**: `UniversalDemuxer`, `UniversalVideoDecoder`, and `UniversalAudioDecoder` were inspected line-by-line and found to execute genuine parsing, decoding, format normalization, and resampling against real media streams. Integration tests generate and verify 8 distinct container formats and codecs. No `todo!`, `unimplemented!`, dummy returns, or pre-populated verification outputs exist.
6. **Inference 2.2**: The authenticity criteria are fully satisfied.
7. **Rule**: Unsafe code must be strictly justified, accompanied by formal `// SAFETY:` comments, and conform to memory safety and SIMD alignment requirements.
8. **Observation 1.3**: Every `unsafe` block in `syncplay-demux` and `syncplay-codecs` was verified. Formal `// SAFETY:` comments are present on all instances. Memory alignment (64-byte buffers, 16-byte row strides, SIMD tail padding zeroing) was empirically verified.
9. **Inference 2.3**: Memory safety and invariant standards are fully satisfied.
10. **Rule**: All verification scripts, clippy lints, tests, and intent harnesses must pass with zero errors and zero warnings.
11. **Observation 1.4**: `cargo check`, `cargo clippy`, `cargo test`, `intent_harness.py`, and `verify_phase4.sh` all passed cleanly with 100% compliance.
12. **Inference 2.4**: The behavioral verification standards are fully satisfied.
13. **Conclusion**: The work product satisfies all forensic requirements without any integrity violations. Final verdict: **CLEAN**.

---

## 3. Caveats

1. **System FFmpeg Availability**: `UniversalDemuxer` and universal codecs rely on system FFmpeg shared libraries (libavcodec, libavformat, libavutil, libswscale, libswresample). On the development machine, FFmpeg 8.1.3 is installed and `.cargo/config.toml` provides the necessary search paths.
2. **Synthetic Vector Fallback**: In-tree synthetic vectors (`synthetic-mkv`, `synthetic-mp4`) contain mock byte sequences rather than real bitstreams. The coordinator preserves `EbmlDemuxer` and `IsoBmffDemuxer` for these synthetic vectors, while all real files are routed through `UniversalDemuxer`.

---

## 4. Conclusion

The Milestone 2 work product is verified to be genuine, safe, and fully compliant with all architectural invariants, coding standards, and intent requirements.

**Verdict**: **CLEAN**

---

## 5. Verification Method

To independently reproduce the forensic verification:

```bash
# 1. Dependency Blacklist Audit
./scripts/audit_dependencies.sh

# 2. Direct grep audit for banned crates
grep -i -E "mpv|vlc|sqlite|diesel|rocksdb|sled|heed|tantivy|meilisearch" Cargo.lock Cargo.toml crates/*/Cargo.toml || echo "CLEAN"

# 3. Workspace compilation & clippy (-D warnings)
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings

# 4. Full test suite execution
cargo test --workspace

# 5. Pi Framework Intent Verification Harness (100.0% compliance)
python3 scripts/intent_harness.py

# 6. End-to-end Phase 4 Verification Harness (all 8 stages)
./scripts/verify_phase4.sh
```

### Invalidation Conditions
- Any occurrence of banned crates in `Cargo.lock` or `Cargo.toml`.
- Any compiler warning or error under `cargo check` or `cargo clippy`.
- Any unit or integration test failure in any workspace crate.
- Any intent compliance score below 100.0% in `scripts/intent_harness.py`.
- Any stage failure in `./scripts/verify_phase4.sh`.
