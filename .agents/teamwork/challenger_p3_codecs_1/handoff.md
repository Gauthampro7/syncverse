# Milestone 1 Adversarial Challenge Handoff: H.264 Bitstream Parser & Decoders

**Agent**: `challenger_p3_codecs_1`  
**Recipient**: `orchestrator_3` (`ecd27bad-3488-46aa-ba26-8b0b7cecf0eb`)  
**Working Directory**: `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p3_codecs_1`  
**Date**: 2026-10-25T16:15:00Z  
**Target Milestone**: Phase 3 Milestone 1 (Pure-Rust Video & Audio Bitstream Decoding Engine)  
**Verdict**: **`REQUEST_CHANGES`**

---

## 1. Observation

1. **Memory Index Out-of-Bounds on Resolution Transition**:
   - Location: `crates/syncplay-codecs/src/h264/decoder.rs`, lines 198–211:
     ```rust
     if let Some(ref prev) = self.last_frame {
         let prev_y = prev.y_plane.as_slice();
         for py in 0..16 {
             let y = mb_y * 16 + py;
             if y < height {
                 for px in 0..16 {
                     let x = mb_x * 16 + px;
                     if x < width {
                         y_plane[y * stride_y + x] = prev_y[y * prev.stride_y + x];
                     }
                 }
             }
         }
     }
     ```
   - Tool Command & Verbatim Output:
     `cargo test --test adversarial_bitstream_fuzz_test test_h264_decoder_dimension_change_prev_frame_out_of_bounds -- --nocapture`
     ```
     thread 'test_h264_decoder_dimension_change_prev_frame_out_of_bounds' panicked at crates/syncplay-codecs/src/h264/decoder.rs:206:69:
     index out of bounds: the len is 272 but the index is 272
     ```

2. **Arithmetic Addition & Multiplication Overflows in `Sps::visible_dimensions()`**:
   - Location: `crates/syncplay-codecs/src/h264/sps.rs`, lines 212–214 and 233–234:
     ```rust
     let raw_width = (self.pic_width_in_mbs_minus1 + 1) * 16;
     let sub_height_factor = if self.frame_mbs_only_flag { 1 } else { 2 };
     let raw_height = (self.pic_height_in_map_units_minus1 + 1) * 16 * sub_height_factor;
     ...
     let crop_x = (self.frame_crop_left_offset + self.frame_crop_right_offset) * crop_unit_x;
     let crop_y = (self.frame_crop_top_offset + self.frame_crop_bottom_offset) * crop_unit_y;
     ```
   - Tool Command & Verbatim Output:
     `cargo test --test adversarial_bitstream_fuzz_test test_sps_visible_dimensions_integer_overflow_vulnerabilities -- --nocapture`
     ```
     thread panicked at crates/syncplay-codecs/src/h264/sps.rs:212:25: attempt to multiply with overflow
     thread panicked at crates/syncplay-codecs/src/h264/sps.rs:212:25: attempt to add with overflow
     thread panicked at crates/syncplay-codecs/src/h264/sps.rs:233:22: attempt to add with overflow
     ```

3. **Integer Addition Overflow in `Sps::parse` Scaling Lists**:
   - Location: `crates/syncplay-codecs/src/h264/sps.rs`, line 115:
     ```rust
     next_scale = (last_scale + delta_scale + 256) % 256;
     ```
   - Tool Command & Verbatim Output:
     `cargo test --test adversarial_bitstream_fuzz_test test_scaling_matrix_delta_scale_overflow -- --nocapture`
     ```
     thread 'test_scaling_matrix_delta_scale_overflow' panicked at crates/syncplay-codecs/src/h264/sps.rs:115:47:
     attempt to add with overflow
     ```

4. **Integer Addition Overflow in `Pps::parse` Scaling Lists**:
   - Location: `crates/syncplay-codecs/src/h264/pps.rs`, line 117:
     ```rust
     next_scale = (last_scale + delta_scale + 256) % 256;
     ```
   - Tool Command & Verbatim Output:
     `cargo test --test adversarial_bitstream_fuzz_test test_pps_scaling_matrix_delta_scale_overflow -- --nocapture`
     ```
     thread 'test_pps_scaling_matrix_delta_scale_overflow' panicked at crates/syncplay-codecs/src/h264/pps.rs:117:47:
     attempt to add with overflow
     ```

5. **Exp-Golomb, NAL Parser, and AVCC Edge Cases Verified Passing**:
   - Exp-Golomb leading zeros > 31 returns `Err(CommonError::InvalidData)`.
   - Truncated bitstreams return `Err(CommonError::UnexpectedEof)`.
   - `read_te(0)` returns `Ok(0)` with 0 consumed bits; `read_te(1)` correctly parses inverted bit values.
   - `read_se` correctly parses up to `i32::MAX` and down to `-2147483647`.
   - Annex B start code scanner trims trailing zeros and handles corrupt inputs without hanging.
   - Emulation prevention byte un-escaping strips `0x03` across buffer boundaries without buffer over-reads.
   - AVCC length parsing validates sizes and bounds checks packet buffers.

---

## 2. Logic Chain

1. **Vulnerability 1 Logic (Memory Safety)**:
   - Observation 1 demonstrates that in `reconstruct_slice_macroblocks`, `prev_y` from `self.last_frame` is indexed using coordinates `y` and `x` derived from the *current* frame's macroblock grid without verifying `prev.width == frame.width && prev.height == frame.height`.
   - When a video stream switches from a smaller resolution (e.g. 16x16) to a larger resolution (e.g. 64x64), if `first_mb_in_slice > 0`, `y * prev.stride_y + x` computes an index larger than `prev_y.len()`, causing an out-of-bounds index panic.
   - Therefore, video playback crashes immediately when encountering a resolution change on multi-slice pictures.

2. **Vulnerability 2 Logic (Integer Overflow in Dimensions)**:
   - Observation 2 demonstrates that `Sps::visible_dimensions()` performs unchecked addition `self.pic_width_in_mbs_minus1 + 1` and unchecked multiplication `* 16` in `u32`.
   - Since `Sps::parse` decodes `pic_width_in_mbs_minus1` via `read_ue()?` without bounding it, an attacker or corrupted bitstream with `pic_width_in_mbs_minus1 >= 0x1000_0000` causes an arithmetic multiplication overflow panic.
   - Crop offsets similarly overflow `u32` when added or multiplied by `crop_unit_x`.
   - Therefore, dimension calculation on untrusted SPS data crashes the process.

3. **Vulnerability 3 & 4 Logic (Integer Overflow in Scaling Lists)**:
   - Observations 3 and 4 demonstrate that in both SPS and PPS scaling matrix parsers, `(last_scale + delta_scale + 256)` is calculated in signed 32-bit integer arithmetic.
   - When `delta_scale` is decoded from `read_se()?` with a large positive integer (e.g. `i32::MAX = 2147483647`), adding `last_scale (8)` and `256` overflows `i32::MAX`.
   - Therefore, parsing validly encoded or fuzzed High Profile streams with large scaling list deltas triggers an immediate panic.

---

## 3. Caveats

1. The test suite does not implement a full CABAC syntax arithmetic decoder (out of scope for Milestone 1, which focuses on SPS, PPS, slice headers, CAVLC, and planar reconstruction).
2. The UI concurrency test race condition noted by `worker_p3_codecs` in `crates/syncplay-ui/src/snapshot.rs` was not modified per strict Write Ownership rules.

---

## 4. Conclusion

**Verdict**: **`REQUEST_CHANGES`**

Milestone 1 is functional and well-architected in its core primitives, but contains critical denial-of-service and memory indexing panics on untrusted input that violate Requirement R3 and safety contracts.

The following changes must be implemented by `worker_p3_codecs`:
1. **Resolution Change Guard**: In `crates/syncplay-codecs/src/h264/decoder.rs:198`, add a check ensuring `prev.width == frame.width && prev.height == frame.height` before copying from `self.last_frame`; if dimensions differ, fill previous macroblocks with neutral luma (`128`) instead.
2. **Dimension Bounding & Checked Math**: In `crates/syncplay-codecs/src/h264/sps.rs`, validate in `Sps::parse` that `pic_width_in_mbs_minus1 <= 1024` and `pic_height_in_map_units_minus1 <= 1024`, and calculate `visible_dimensions()` using `u64` arithmetic clamped to `u32`.
3. **Scaling Matrix Cast to `i64`**: In `crates/syncplay-codecs/src/h264/sps.rs:115` and `crates/syncplay-codecs/src/h264/pps.rs:117`, compute `next_scale = ((last_scale as i64 + delta_scale as i64 + 256).rem_euclid(256)) as i32;`.
4. **Resolution Cap in Frame Allocation**: In `H264Decoder::decode_packet`, verify that `(width, height)` does not exceed maximum supported limits (e.g. $8192 \times 4320$) before invoking `VideoFrameYUV420p::new`.

---

## 5. Verification Method

To independently reproduce all empirical findings and test results:

```bash
# 1. Run the empirical challenge & fuzzing suite
cargo test --test adversarial_bitstream_fuzz_test -- --nocapture

# 2. Run all codecs tests
cargo test -p syncplay-codecs

# 3. Verify strict Clippy zero-warnings compliance
cargo clippy --workspace --all-targets -- -D warnings

# 4. Verify Pi Framework Intent Verification Harness
python3 scripts/intent_harness.py
```
