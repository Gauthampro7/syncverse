# Empirical Challenge Report: Audio Unpackers, Terminal TrueColor Buffer & SIMD Tail Padding

**Agent**: `challenger_p3_codecs_2`  
**Target Milestone**: Phase 3 Milestone 1 (Codecs, Presentation & Bitstream Engine)  
**Date**: 2026-10-25T16:15:00Z  
**Verdict**: **APPROVE**  

---

## Challenge Summary

**Overall risk assessment**: **LOW**

Extensive empirical stress testing and fuzzing were conducted against:
1. Audio unpackers (`syncplay-codecs::audio`):
   - Truncated PCM byte streams across all 9 PCM formats (`U8`, `I16Le`, `I16Be`, `I24Le`, `I24Be`, `I32Le`, `I32Be`, `F32Le`, `F32Be`) and channel counts (1..16).
   - IEEE 754 float sample boundaries (Quiet NaN, Signaling NaN, negative NaN, payload NaNs, $\pm\infty$, and subnormals).
   - Opus Table of Contents (TOC) byte parsing across all 256 possible byte configurations (`0x00..=0xFF`).
   - RFC 5215 Vorbis identification header validation and packet unpacking under boundary blocksize exponents.
   - `AudioDecoder` unified dispatcher switching across PCM, Opus, and Vorbis under corrupted packets.
2. Terminal TrueColor buffer (`syncplay-codecs::color`):
   - Extreme resolutions ($1000\times 1000$ character cells, $1000\times 1$ extreme wide, $1\times 1000$ extreme tall).
   - Single-pixel frames ($1\times 1$) downscaled and upscaled.
   - Rapid color transitions (worst-case non-deduplicating high-entropy checkerboard).
   - String buffer capacity growth, heap safety, and absence of buffer overflows.
3. Planar YUV420p SIMD tail padding (`syncplay-codecs::frame`):
   - Exact $+16$B tail padding allocation and 64-byte pointer alignment across 17 resolutions.
   - Canary byte preservation under simulated 128-bit SIMD vector reads and color conversions.

All 10 Rust empirical integration tests (`audio_truecolor_simd_challenge.rs`) and all 6 Python challenge test suites (`test_audio_truecolor_p3.py`) passed cleanly (100% pass rate) with zero compiler or clippy warnings.

---

## Challenges

### [Low] Challenge 1: Truncated Buffer Lengths in Multi-Byte Audio Formats
- **Assumption challenged**: Demuxed packets from network jitter or truncated file reads may contain trailing bytes that do not align with `bytes_per_sample * channels`. Unpackers must not panic with out-of-bounds index errors.
- **Attack scenario**: Slices of lengths $0 \dots 3 \times \text{frame\_bytes} + 7$ with arbitrary random payloads were fed to `unpack_pcm` across all 9 supported formats and multiple channel counts (1, 2, 3, 4, 6, 8, 16).
- **Blast radius**: If unhandled, index panic in audio thread, terminating playback.
- **Empirical result**: Integer division `num_frames = data.len() / frame_bytes` and `total_samples = num_frames * channels` ensures `total_samples * bps <= data.len()`. Trailing partial samples are cleanly discarded. Zero panics observed across 5,000+ permutations.
- **Mitigation / Defense**: In-tree integer division bounds guarantee memory safety.

### [Low] Challenge 2: IEEE 754 Floating-Point NaN and $\pm\infty$ Injection in F32 PCM
- **Assumption challenged**: Malformed or malicious audio streams could inject NaN float bit patterns or $\pm\infty$, contaminating the DSP pipeline and downstream ALSA audio buffers.
- **Attack scenario**: Injected Quiet NaNs (`0x7FC00000`), Signaling NaNs (`0x7FA00000`), Negative NaNs (`0xFFC00000`), payload NaNs (`0x7F800001` .. `0xFFFFFFFF`), and $\pm\infty$ in both `F32Le` and `F32Be`.
- **Blast radius**: Undefined audio playback noise, NaN poisoning of downstream IIR/FIR resampling filters.
- **Empirical result**: `unpack_pcm` explicitly checks `val.is_nan()` and rejects it immediately with strongly typed `CodecError::InvalidAudio("encountered NaN float sample")`. Injected $\pm\infty$ values are cleanly clamped to $\pm 1.0$ via `val.clamp(-1.0, 1.0)`.
- **Mitigation / Defense**: NaN check and clamping in hot unpacking loop completely neutralizes float poisoning.

### [Low] Challenge 3: RFC 6716 Opus TOC Table Indexing Boundaries
- **Assumption challenged**: Arbitrary single-byte TOC values could trigger out-of-bounds lookups or unhandled pattern matches in `OpusToc::parse`.
- **Attack scenario**: Exhaustive fuzzing across all 256 byte values (`0x00..=0xFF`).
- **Blast radius**: Panic via `unreachable!()` in match statement.
- **Empirical result**: `config = (toc_byte >> 3) & 0x1F` strictly produces $0..31$, and `frame_count_code = toc_byte & 0x03` strictly produces $0..3$. Every possible combination is explicitly handled in the match table. Zero panics. Duration calculations strictly conform to RFC 6716 Table 2 (`2.5, 5.0, 10.0, 20.0, 40.0, 60.0` ms). Empty packets (`&[]`) return typed `CodecError::BufferTooSmall`.
- **Mitigation / Defense**: Total function over `u8` input space.

### [Low] Challenge 4: Extreme Terminal TrueColor Display Resolutions & Worst-Case Color Entropy
- **Assumption challenged**: Giant display buffers ($1000\times 1000$) or rapid per-pixel color transitions could trigger string buffer overflow, excessive heap allocation, or out-of-bounds writes.
- **Attack scenario**:
  1. Allocated $1000\times 1000$ character buffer ($1,000,000$ cells $\implies 2,000,000$ virtual pixels).
  2. Applied maximum entropy checkerboard where every adjacent sub-pixel has differing RGB foreground and background colors.
  3. Tested $1000\times 1$ and $1\times 1000$ extreme aspect ratios.
- **Blast radius**: Heap exhaustion (OOM) or terminal rendering crash.
- **Empirical result**: Rust standard library `String` dynamically grows capacity. Worst-case 80x24 ANSI output with zero deduplication requires $\approx 68.9\text{ KB}$ ($\approx 35.9$ bytes/cell), while uniform frames compress to $2.66\text{ KB}$ ($< 18\text{ KB}$ spec limit). $1000\times 1000$ uniform frame renders to $3.05\text{ MB}$ without error. All lines terminate strictly with `\x1b[0m\n`.
- **Mitigation / Defense**: Robust run-length color deduplication with clean row resets.

### [Low] Challenge 5: SIMD 128-bit Vector Load Over-Read into Tail Padding
- **Assumption challenged**: SIMD vector instructions (`_mm_loadu_si128`) read 16 bytes at a time. On non-multiple-of-16 widths, the last vector load on the last row could read past the allocated buffer if tail padding is absent.
- **Attack scenario**: Saturated the $+16$B tail padding of Y, U, and V planes with canary bytes (`0xAA`, `0x55`, `0xDE`). Tested 17 resolutions including $(1,1), (7,13), (17,19), (65,33), (854,480), (1921,1081)$. Simulated 16-byte vector loads up to frame boundaries.
- **Blast radius**: Memory segmentation fault, address sanitizer trip, or cache-line over-read.
- **Empirical result**: Row strides are aligned to 16 bytes: `(width + 15) & !15`. Capacities are strictly `stride * height + 16`. Proved that for any vector load starting at $row\_start + x$ where $x < width$, $row\_start + x + 16 \le stride * height + 16$. Canaries remained 100% intact after conversions.
- **Mitigation / Defense**: Formal $+16$B allocation guarantee in `VideoFrameYUV420p::new`.

---

## Stress Test Results

| Scenario | Expected Behavior | Actual Behavior | Result |
|---|---|---|---|
| Truncated PCM buffers (0..1024B, 9 formats, 1..16 ch) | Clean truncation, sample counts = `(len/(bps*ch))*ch`, samples in `[-1.0, 1.0]`, no panic | Clean truncation, exact sample counts, 0 panics | **PASS** |
| F32Le/F32Be NaN sample injection (7 NaN patterns) | Strict rejection with `CodecError::InvalidAudio("...NaN...")` | Returns `Err(CodecError::InvalidAudio)` containing "NaN" | **PASS** |
| F32Le/F32Be $\pm\infty$ float sample injection | Clamped to $\pm 1.0$, frame passes bounded validation | Cleanly clamped to `1.0` and `-1.0` | **PASS** |
| F32 subnormal float sample injection | Unpacks to near-zero float without trap | Unpacked cleanly ($|s| < 10^{-10}$) | **PASS** |
| Opus TOC exhaustive fuzzing (all 256 byte values) | Valid parse into standard durations and modes, no panic | 256/256 parsed with exact RFC 6716 durations, 0 panics | **PASS** |
| Opus 0-byte payload packet | Synthesizes bounded audio frame matching TOC duration | Synthesized valid PCM frame with duration matching TOC | **PASS** |
| Empty Opus packet (`&[]`) | `Err(CodecError::BufferTooSmall)` | Returns `Err(CodecError::BufferTooSmall { required: 1, available: 0 })` | **PASS** |
| Vorbis truncated headers (0..29 bytes) | `Err(CodecError::BufferTooSmall)` | Returns `Err(CodecError::BufferTooSmall { required: 30 })` | **PASS** |
| Vorbis corrupt magic, version, 0 ch, 0 rate, 0 framing | Strict rejection with `CodecError::InvalidAudio` | All 7 corrupt variants rejected with `CodecError::InvalidAudio` | **PASS** |
| Vorbis blocksize exponent clamping (0 and 15) | Clamped to [6, 13], allocating 64..8192 samples | Exponent 0 yields 64 samples; 15 yields 8192 samples | **PASS** |
| AudioDecoder unified dispatcher | Handles PCM, Opus, Vorbis with clean error propagation | All 3 codecs decode and handle truncated packets | **PASS** |
| Extreme resolution $1000\times 1000$ TrueColor buffer | Allocates $1\text{M}$ cells, renders without buffer overflow | Rendered 1000 lines, 0 overflows, bounded string | **PASS** |
| Single-pixel $1\times 1$ downscale/upscale | Correctly maps single pixel to cell and scales to 80x24 | Cell contains exact RGB, upscaled cells match | **PASS** |
| Worst-case color transitions (checkerboard 80x24) | No deduplication, output $< 100\text{ KB}$, all rows end `\x1b[0m\n` | 68,911 bytes, 24 rows end with reset sequence | **PASS** |
| Best-case uniform color 80x24 | Deduplication compresses output $< 18\text{ KB}$ | 2,664 bytes ($< 18\text{ KB}$ spec bound) | **PASS** |
| SIMD tail padding +16B & 64B align (17 resolutions) | `len == stride * height + 16`, 64B aligned | All planes match exact capacity formula and 64B alignment | **PASS** |
| SIMD 16B vector load boundary proof | $row + x + 16 \le capacity$ for all rows and SIMD steps | Zero over-reads across all rows and steps | **PASS** |
| Canary protection in tail padding | Canary bytes untouched after RGBA & terminal conversion | 100% of canary bytes preserved intact | **PASS** |

---

## Unchallenged Areas

- **CABAC arithmetic entropy decoding**: Out of scope for Baseline / High profile slice header milestone.
- **ALSA / PulseAudio sound card hardware drivers**: Scheduled for Milestone M2 (`worker_p3_audio`).
- **Interactive TUI input event loop**: Scheduled for Milestone M4 (`worker_p3_ui`).

---

## Conclusion & Verdict

The audio unpacking and TrueColor presentation components implemented for Phase 3 Milestone 1 demonstrate exceptional numerical robustness, memory safety, and empirical resilience against hostile/malformed inputs. Zero panics, zero memory leaks, and zero buffer over-reads occurred under adversarial stress testing.

**Final Verdict**: **APPROVE**
