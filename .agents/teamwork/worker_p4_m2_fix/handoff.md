# Milestone 2 Fix Handoff Report: Universal Demuxing & Codec Remediation

**Agent**: `worker_p4_m2_fix`  
**Recipient**: `orchestrator_4` (`ee88cac4-07d4-4dad-bc7e-4c3f606ef03f`)  
**Working Directory**: `/home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m2_fix`  
**Milestone**: Milestone 2 Fixes (Phase 4 Universal Media Compatibility & Codec Hardening)  
**Date**: 2026-10-25T23:46:00Z  

---

## 1. Observation

### 1.1 Pre-Fix Defects Observed
1. **FLV `seek_to(0)` EPERM Failure**:
   - Location: `crates/syncplay-demux/src/universal.rs:324`.
   - In `crates/syncplay-demux/tests/universal_adversarial_challenge.rs`:
     ```
     [FAIL] seek(0) in flv_flv1 failed: I/O error: FFmpeg seek failure: Operation not permitted
     [FAIL] rapid seek(0) in flv_flv1 failed: I/O error: FFmpeg seek failure: Operation not permitted
     [FAIL] Seek to 0 after past-EOF in flv_flv1 failed: I/O error: FFmpeg seek failure: Operation not permitted
     ```
   - In FLV files with video priming delays (e.g., packet 0 at $T = 25\text{ms}$), requesting a seek backwards with `ictx.seek(0, ..0)` forced `avformat_seek_file` to search for keyframes with $T \le 0$. Since none exist before $25\text{ms}$, libavformat returned `AVERROR(EPERM)`.

2. **MPEG-TS Start-Time Offset Desynchronization**:
   - Location: `crates/syncplay-demux/src/universal.rs:268-283`.
   - In `timescale_90k.ts`, container PCR clock starts at `start_time = 1.456778s` (`1456778` $\mu\text{s}$) with video packet 0 at `1480ms`.
   - Packets were emitted with un-normalized timestamps ($[1480\text{ms}, 4400\text{ms}]$) rather than media-relative timestamps starting at 0ms. Seeks between $[0, 1480\text{ms}]$ clamped to $1480\text{ms}$, and the second half of the clip was unaddressable within the 3.023s duration.

3. **Missing `AVProbeData` Buffer Zero-Padding Margin**:
   - Location: `crates/syncplay-demux/src/universal.rs:169-187`.
   - Allocated `vec![0u8; 4096]` and read up to 4096 bytes into it. If $n = 4096$, trailing bytes after $n$ had 0 zero-padding margin, violating FFmpeg's `AVProbeData` contract requiring `AVPROBE_PADDING_SIZE` (32 bytes) of trailing zeros.

4. **Universal Video Decoder Scaler Invalidation**:
   - Location: `crates/syncplay-codecs/src/universal_video.rs:318-334`.
   - `UniversalVideoDecoder` cached `ScalerContext` under `if self.scaler.is_none()`, failing to track input pixel format, width, and height. Dynamic shifts (e.g., NV12 -> RGB24 or 320x240 -> 640x480) produced `Input changed` or `Output changed` swscale conversion errors.

5. **Audio Codec DTS / DCA Alias Mapping**:
   - Location: `crates/syncplay-codecs/src/audio/universal.rs:157`.
   - `UniversalAudioDecoder::from_codec_name` directly queried FFmpeg decoder name `"dts"`, failing with `DecoderNotFound("dts")` because libavcodec registers the DTS decoder as `"dca"`.

### 1.2 Implemented Remediations
1. **UniversalDemuxer Seeking Bounds & Multi-Tier Fallback** (`crates/syncplay-demux/src/universal.rs`):
   - Added container start time tracking: `start_time_us: i64`.
   - In `seek_to(target_pts_ms)`, converted target to microseconds with stream offset:
     `stream_target_us = (target_pts_ms as i64 * 1000) + self.start_time_us`.
   - Attempted seeking backwards: `ictx.seek(stream_target_us, ..stream_target_us)`.
   - On error, applied multi-tier forward fallback:
     `stream_target_us..` $\to$ `self.start_time_us..` $\to$ `0..` $\to$ `..`.
   - Normalized landed packet timestamps using `normalize_packet`.
2. **UniversalDemuxer Stream Start-Time Normalization** (`crates/syncplay-demux/src/universal.rs`):
   - In `init_from_context`, safely extracted format/stream start-time in microseconds (`(*raw).start_time`).
   - Added `normalize_packet(&self, packet: &ffmpeg::Packet) -> DemuxPacket` subtracting `start_time_us` from both PTS and DTS with saturating 0 floors:
     `norm_pts_us = (pts_us - self.start_time_us as i128).max(0)`.
   - Integrated `normalize_packet` across both `read_packet` and `seek_to` peek buffering.
   - Added public getter `pub fn start_time_us(&self) -> i64`.
3. **`AVProbeData` Buffer Zero-Padding Guarantee** (`crates/syncplay-demux/src/universal.rs`):
   - Allocated `vec![0u8; 4096 + 64]`, read up to 4096 bytes (`&mut buf[..4096]`), and explicitly filled `buf[n..n + 64].fill(0)`, guaranteeing 64 bytes of zero padding (exceeding FFmpeg's required 32 bytes).
4. **Video Scaler Dynamic Cache Invalidation** (`crates/syncplay-codecs/src/universal_video.rs`):
   - Added `last_scaler_format`, `last_scaler_width`, and `last_scaler_height` fields.
   - Recreated `ScalerContext` and emptied `self.intermediate_frame` whenever input format, width, or height shifted.
   - Reset scaler state in `flush(&mut self)`.
5. **Audio Codec Name / Alias Resolver** (`crates/syncplay-codecs/src/audio/universal.rs`):
   - Added `pub fn map_codec_name(name: &str) -> Option<Id>` resolving `"dts"` and `"dca"` to `Id::DTS`, `"pcm"` and `"pcm_s16le"` to `Id::PCM_S16LE`, etc.
   - Updated `from_codec_name` to route through `map_codec_name`.
6. **Hardened Adversarial Tests**:
   - `crates/syncplay-demux/tests/universal_adversarial_challenge.rs`: Converted seek checks from warning logs into hard assertions (`unwrap_or_else(|e| panic!(...))`).
   - `crates/syncplay-codecs/tests/universal_codecs_adversarial_stress.rs`: Updated format shift, resolution shift, and DTS/DCA alias tests to verify successful execution.
   - Added unit test in `crates/syncplay-codecs/src/audio/universal.rs` verifying `"dts"` and `"dca"` alias resolution.

---

## 2. Logic Chain

1. **Resolution of FLV `seek_to(0)`**:
   - `stream_target_us = (target_pts_ms as i64 * 1000) + self.start_time_us`.
   - When seeking to 0 on FLV with a 25ms priming delay, backward seek (`..stream_target_us`) fails with `EPERM`.
   - The forward fallback `ictx.seek(stream_target_us, stream_target_us..)` instructs libavformat to locate the earliest available keyframe with $T \ge 0\mu\text{s}$.
   - Libavformat selects keyframe 0 at $T = 25\text{ms}$.
   - Landed PTS is normalized against `start_time_us` and lands cleanly at $0\text{ms}$.
   - Verified: All 5 seek tests on `flv_flv1` passed without error.

2. **Resolution of MPEG-TS Timeline Alignment**:
   - MPEG-TS carries a PCR start offset (`start_time = 1456778` $\mu\text{s}$).
   - Subtracting `start_time_us` from stream PTS normalizes packet timestamps to media-relative times ($0\text{ms}$ to $2943\text{ms}$).
   - In `seek_to(target_pts_ms)`, adding `self.start_time_us` translates media targets (e.g. 0, 1000, 2000 ms) back into stream timebase microseconds.
   - Verified: Seeking across `mpegts_90k` at 0ms, 500ms, 1000ms, 1500ms, 2000ms, and 2200ms succeeds cleanly and lands within correct keyframe boundaries.

3. **Resolution of `AVProbeData` Padding**:
   - Buffer size of `4096 + 64` with `buf[n..n + 64].fill(0)` guarantees that `buf` has at least 64 zeroed bytes after index `n`.
   - `av_probe_input_format` reads up to `buf_size = n` with guaranteed trailing zeros, eliminating heap over-read risk.

4. **Resolution of Video Scaler Reconfiguration**:
   - Comparing `src.format()`, `src.width()`, and `src.height()` against `last_scaler_*` detects any dynamic shift in video frame geometry or pixel layout.
   - Re-allocating `ScalerContext` and clearing `intermediate_frame` allows seamless format conversion (e.g., NV12 to RGB24, 320x240 to 640x480) without `Input changed` or `Output changed` errors.

5. **Resolution of Audio DTS / DCA Aliases**:
   - `map_codec_name` maps both `"dts"` and `"dca"` to `Id::DTS`.
   - Calling `UniversalAudioDecoder::from_codec_name("dts", ...)` now successfully initializes libavcodec's `"dca"` decoder.

---

## 3. Caveats

- **No Caveats**: All 5 tasks outlined in DISPATCH.md are fully implemented, verified, and passing 100% of unit, integration, intent, and phase verification suites.

---

## 4. Conclusion

All reported Milestone 2 defects have been completely remediated:
1. FLV `seek_to(0)` succeeds cleanly on packet 0 without EPERM.
2. MPEG-TS stream start-time offsets are normalized to 0ms across both sequential demuxing and random seeking.
3. `AVProbeData` buffers are zero-padded with a 64-byte margin beyond `n`.
4. Video decoder dynamic format and dimension mutations re-initialize swscale cleanly.
5. Audio decoder recognizes `"dts"` and `"dca"` aliases for `Id::DTS`.

---

## 5. Verification Method

To independently execute and verify all remediations:

```bash
# 1. Run the demuxer adversarial challenge suite (verifies FLV seek(0) & MPEG-TS start-time normalization)
cargo test -p syncplay-demux --test universal_adversarial_challenge -- --nocapture

# 2. Run the codecs adversarial stress suite (verifies swscale invalidation & audio DTS/DCA alias mapping)
cargo test -p syncplay-codecs --test universal_codecs_adversarial_stress -- --nocapture

# 3. Workspace check and strict clippy linter check (-D warnings)
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings

# 4. Full workspace unit and integration test suite
cargo test --workspace

# 5. Dependency blacklist audit
./scripts/audit_dependencies.sh

# 6. Pi Framework Intent Verification Harness
python3 scripts/intent_harness.py

# 7. Phase 4 End-to-End Automated Verification Harness (All 8 Stages)
./scripts/verify_phase4.sh
```

### Invalidation Conditions
- Any panic or error during `seek_to(0)` on FLV or MPEG-TS fixtures.
- Any compiler warning or clippy violation under `#![deny(warnings)]`.
- Any dependency violation reported by `./scripts/audit_dependencies.sh`.
- Any compliance score $< 100.0\%$ in `python3 scripts/intent_harness.py`.
- Any failure across Stages 1–8 in `./scripts/verify_phase4.sh`.
