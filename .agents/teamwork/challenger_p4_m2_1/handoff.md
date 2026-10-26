# Empirical Challenge Report: UniversalDemuxer Compatibility & Seeking

**Agent**: `challenger_p4_m2_1`  
**Milestone**: Milestone 2 (Phase 4 Universal Media Compatibility)  
**Verdict**: **`REQUEST_CHANGES`**

---

## 1. Observation

Direct observations from empirical execution of the adversarial challenge suite against `UniversalDemuxer` (`crates/syncplay-demux/src/universal.rs`):

### 1.1 FLV `seek_to(0)` Fails with `Operation not permitted` (EPERM)
- In `crates/syncplay-demux/tests/universal_adversarial_challenge.rs`:
  ```
  --> Running keyframe seeking on: flv_flv1
  [FAIL] seek(0) in flv_flv1 failed: I/O error: FFmpeg seek failure: Operation not permitted
  ...
  [FAIL] rapid seek(0) in flv_flv1 failed: I/O error: FFmpeg seek failure: Operation not permitted
  ...
  [FAIL] Seek to 0 after past-EOF in flv_flv1 failed: I/O error: FFmpeg seek failure: Operation not permitted
  ```
- File inspected: `multi_flv.flv` (encoded with FLV1 video + MP3 audio via FFmpeg).
- `ffprobe -show_packets` confirms video packet 0 has presentation timestamp `pts=25` (25ms).
- In `crates/syncplay-demux/src/universal.rs:324`:
  ```rust
  let target_us = (target_pts_ms as i64) * 1000;
  ictx.seek(target_us, ..target_us)
      .map_err(|e| DemuxError::Io(format!("FFmpeg seek failure: {}", e)))?;
  ```
  When `target_pts_ms == 0`, `target_us == 0`, so `..target_us` instructs FFmpeg to find a keyframe $T$ satisfying $T \le 0\mu\text{s}$. Because all frames have $T \ge 25,000\mu\text{s}$, libavformat returns `-1` (`AVERROR(EPERM)`).

### 1.2 MPEG-TS PCR Start Offset Causes Seek Clamping & Timeline Desynchronization
- In `crates/syncplay-demux/tests/universal_adversarial_challenge.rs`:
  ```
  === Timescale Test [mpegts_90k] ===
    Track count: 2
    Video packets demuxed: 75
    Initial video PTS: 1480 ms
    Final video PTS: 4400 ms
    Average frame delta: 40.00 ms
  === Seeking Test [mpegts_90k] ===
    seek(0) in mpegts_90k -> landed: 1480 ms (pkt PTS: Some(1480))
    seek(1000) in mpegts_90k -> landed: 1480 ms (pkt PTS: Some(1480))
    seek(2000) in mpegts_90k -> landed: 2000 ms (pkt PTS: Some(2000))
    seek(500) in mpegts_90k -> landed: 1480 ms (pkt PTS: Some(1480))
  ```
- File inspected: `timescale_90k.ts`.
- `ffprobe` confirms `start_time=1.456778` (format), `start_time=1.480000` (video).
- `UniversalDemuxer` reports `duration_ms = 3023` ms (`ictx.duration() / 1000`).
- Because `UniversalDemuxer` does not store or compensate for `start_time`, seeking within $[0, 1480\text{ms}]$ clamps to $1480\text{ms}$. Furthermore, seeking within $[1480, 3023\text{ms}]$ addresses the first half of the clip ($0\text{ms}$ to $1543\text{ms}$ relative to media start). The second half of the clip ($1543\text{ms}$ to $3023\text{ms}$) requires stream PTS $> 3023\text{ms}$, which cannot be requested from a seekbar bounded by `duration_ms` (3023ms).

### 1.3 `AVProbeData` Missing Mandated Buffer Zero-Padding
- In `crates/syncplay-demux/src/universal.rs:169-194`:
  ```rust
  let mut buf = vec![0u8; 4096];
  let n = reader.read(&mut buf)...
  let probe_data = ffmpeg::ffi::AVProbeData {
      filename: std::ptr::null(),
      buf: buf.as_mut_ptr(),
      buf_size: n as std::ffi::c_int,
      mime_type: std::ptr::null(),
  };
  let fmt = unsafe { ffmpeg::ffi::av_probe_input_format(&probe_data, 1) };
  ```
- FFmpeg API contract: `AVProbeData.buf` MUST have `AVPROBE_PADDING_SIZE` (32 bytes) of zero padding at the end of the buffer. If $n = 4096$, `buf_size == 4096`, leaving 0 padding bytes and creating a heap buffer over-read risk.

### 1.4 Verified Robust Functionality
- **Multi-Container Format Support**: Container probing and track metadata extraction passed cleanly across all 8 formats: Matroska (`.mkv`), WebM (`.webm`), ISOBMFF (`.mp4`), QuickTime (`.mov`), AVI (`.avi`), MPEG-TS (`.ts`), Flash Video (`.flv`), and Ogg (`.ogv`).
- **Timescale Conversions**:
  * 1/90000 (MPEG-TS): average frame delta = 40.00 ms (25 fps)
  * 1/12800 (MP4 custom): average frame delta = 40.00 ms (25 fps)
  * 1/1000 (MKV standard): average frame delta = 40.00 ms (25 fps)
  * 1/25 (AVI fixed): average frame delta = 40.54 ms
  * VP9 / Opus WebM: average frame delta = 40.00 ms
  * Theora / Vorbis Ogg: average frame delta = 40.00 ms
- **In-Memory Streaming**: Initializing `UniversalDemuxer` via `Box<dyn ReadSeek + Send>` using `Cursor<Vec<u8>>` succeeded for probe, track extraction, sequential packet reading, and seeking on in-memory buffers.
- **Probe Adversarial Rejections**: Truncated buffers ($<4$ bytes), all-zero, all-0xFF, pseudo-random LCG noise, and HTTP text payloads were all cleanly rejected (`Ok(false)`).
- **Pure-Rust Demuxers**: `EbmlDemuxer` and `IsoBmffDemuxer` tests passed 100% without regressions.

---

## 2. Logic Chain

1. **Root Cause of FLV `seek_to(0)` Failure**:
   - `UniversalDemuxer::seek_to(target_pts_ms)` computes `target_us = target_pts_ms * 1000`.
   - It calls `ictx.seek(target_us, ..target_us)`.
   - In `ffmpeg-next`, `..target_us` converts to `avformat_seek_file(ctx, -1, i64::MIN, target_us, target_us, 0)`.
   - The fifth parameter (`max_ts`) is set to `target_us`.
   - When `target_pts_ms == 0`, `max_ts = 0`.
   - `avformat_seek_file` searches for a keyframe $T$ where $\text{min\_ts} \le T \le \text{max\_ts}$.
   - In FLV (and many real-world files where audio/video has an encoder priming delay), the earliest keyframe timestamp is $T = 25\text{ms} = 25,000\mu\text{s} > 0$.
   - Because no keyframe exists with $T \le 0$, `avformat_seek_file` fails and returns `EPERM`.
   - Therefore, calling `seek_to(0)` on FLV fails.

2. **Root Cause of MPEG-TS Timeline Truncation & Seek Clamping**:
   - In MPEG-TS, transport streams carry a 33-bit PCR clock that does not start at 0 (FFmpeg defaults to $\sim 1.48\text{s}$ to prevent buffer underflow).
   - `UniversalDemuxer::init_from_context` extracts duration ($3.023\text{s}$) but ignores `ictx.start_time()` ($1.456\text{s}$).
   - `UniversalDemuxer::read_packet` scales raw PTS without subtracting `start_time`, yielding packets timestamped $[1480\text{ms}, 4400\text{ms}]$.
   - When the user seeks to `target_pts_ms` in $[0, 1480\text{ms}]$, `target_us` is before the stream start, clamping to $1480\text{ms}$.
   - When the user seeks to $[1480, 3023\text{ms}]$, the demuxer seeks to the first half of the video. The second half of the video (stream timestamps $[3023, 4400\text{ms}]$) is completely unreachable from the seekbar.

3. **Impact on User Experience & Watch-Party Synchronization**:
   - Gautham cannot rewind an FLV file to timestamp 0 (pressing 'Home' or seeking to start will throw an I/O error).
   - In MPEG-TS watch-parties, barrier unpauses and seek synchronizations will fail or desynchronize because the timeline presentation time is offset by 1.48 seconds.

---

## 3. Caveats

- **Network Streams**: Tests used local file fixtures and in-memory `Cursor` buffers; live non-seekable multicast MPEG-TS streams over UDP sockets were not tested for seeking (as network live streams are inherently non-seekable).
- **Subtitle Tracks**: The test fixtures focused on video and audio streams; timed text / subtitle packet demuxing was not part of the Milestone 2 container compatibility scope.
- **Pure-Rust Demuxers**: `EbmlDemuxer` and `IsoBmffDemuxer` remain completely unaffected by these FFmpeg issues and continue to pass all in-tree tests.

---

## 4. Conclusion

**Verdict**: **`REQUEST_CHANGES`**

`UniversalDemuxer` successfully demonstrates universal container demuxing, metadata extraction, and timescale conversion across all 8 required formats. However, it cannot be approved for Milestone 2 completion until the following three defects are resolved:

1. **Fix FLV `seek_to(0)` error**: Adjust seek bounds in `seek_to` so that `max_ts` does not reject the first keyframe when `target_pts_ms` is smaller than the stream's first keyframe (e.g. use `range = ..` or clamp `target_us` to `start_time`).
2. **Normalize stream start time**: Record `start_time` in `UniversalDemuxer` and adjust `pts_ms` and `target_us` so presentation timestamps are normalized to start at $0\text{ms}$ relative to media playback.
3. **Add buffer padding in `probe`**: Allocate `vec![0u8; 4096 + 64]` and read up to 4096 bytes to guarantee the mandated 32-byte zero padding for `AVProbeData`.

---

## 5. Verification Method

To independently reproduce all empirical findings:

```bash
# Run the adversarial challenge test suite:
cargo test -p syncplay-demux --test universal_adversarial_challenge -- --nocapture
```

Look for the logged failure outputs:
- `[FAIL] seek(0) in flv_flv1 failed: I/O error: FFmpeg seek failure: Operation not permitted`
- `=== Seeking Test [mpegts_90k] ===`: `seek(0) -> landed: 1480 ms`, `seek(1000) -> landed: 1480 ms`, `seek(500) -> landed: 1480 ms`
