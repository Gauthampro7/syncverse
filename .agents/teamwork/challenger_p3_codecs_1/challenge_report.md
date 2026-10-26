# Adversarial Challenge Report: Pure-Rust H.264 Bitstream Parser & Decoders

**Agent**: `challenger_p3_codecs_1`  
**Milestone**: Phase 3 Milestone 1 (Pure-Rust Video & Audio Bitstream Decoding Engine)  
**Target Crates**: `crates/syncplay-codecs`, `crates/syncplay-common`  
**Date**: 2026-10-25T16:12:00Z  
**Verdict**: **REQUEST_CHANGES**

---

## Challenge Summary

**Overall risk assessment**: **CRITICAL**

Through empirical fuzzing and systematic boundary condition stress-testing in `crates/syncplay-codecs/tests/adversarial_bitstream_fuzz_test.rs`, we uncovered **1 CRITICAL memory out-of-bounds panic**, **3 HIGH-severity arithmetic overflow panics** in SPS/PPS parsing and dimension calculation, and **1 MEDIUM resource exhaustion vulnerability**.

While the foundational Exp-Golomb decoding, NAL unit demarcations, emulation prevention byte stripping, and AVCC length parsing demonstrate robust defensive checks against truncated and malformed inputs, the decoder and parameter set models allow panics and out-of-bounds memory accesses when handling untrusted streams.

---

## Detailed Challenges & Vulnerabilities Found

### [CRITICAL] Challenge 1: Memory Out-of-Bounds Panic on Dynamic Resolution Change with Non-Zero `first_mb_in_slice`

- **Assumption challenged**: Assumed `self.last_frame` has identical dimensions and stride as the current frame when copying preceding macroblocks in multi-slice pictures.
- **Attack scenario**:
  In `crates/syncplay-codecs/src/h264/decoder.rs` (lines 195–214), `reconstruct_slice_macroblocks` handles macroblocks where `mb_index < slice_header.first_mb_in_slice` by copying pixel data from `self.last_frame`:
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
  `height` and `width` bounds are checked solely against the *current* frame's dimensions (`frame.height`, `frame.width`).
  When an H.264 stream transitions from a smaller resolution (e.g., 16x16, or 720p) to a larger resolution (e.g., 64x64, or 1080p), if the first slice in the packet starts at `first_mb_in_slice > 0` (e.g., `first_mb_in_slice = 5`), the loop computes `y = 17` and `x = 0`.
  In `prev` (16x16), `prev.stride_y = 16` and `prev_y.len() = 272` (valid indices 0..271).
  The expression `y * prev.stride_y + x` evaluates to `17 * 16 + 0 = 272`, which directly indexes out-of-bounds into `prev_y[272]`.
- **Empirical confirmation**:
  Reproduced in `test_h264_decoder_dimension_change_prev_frame_out_of_bounds`:
  ```
  thread 'test_h264_decoder_dimension_change_prev_frame_out_of_bounds' panicked at crates/syncplay-codecs/src/h264/decoder.rs:206:69:
  index out of bounds: the len is 272 but the index is 272
  ```
- **Blast radius**: Immediate process crash and Remote Denial of Service (DoS) of the media player upon resolution changes or multi-slice keyframes.
- **Mitigation**:
  In `reconstruct_slice_macroblocks`, check `prev.width == frame.width && prev.height == frame.height` before copying from `self.last_frame`. If dimensions differ, fill preceding macroblocks with neutral luma (`128`) instead of indexing into `prev`.

---

### [HIGH] Challenge 2: Arithmetic Addition & Multiplication Overflow Panics in `Sps::visible_dimensions()`

- **Assumption challenged**: Assumed `pic_width_in_mbs_minus1`, `pic_height_in_map_units_minus1`, and frame crop offsets will not cause integer overflow in 32-bit arithmetic.
- **Attack scenario**:
  In `crates/syncplay-codecs/src/h264/sps.rs`:
  1. Line 212:
     `let raw_width = (self.pic_width_in_mbs_minus1 + 1) * 16;`
     - If `pic_width_in_mbs_minus1 == u32::MAX`, panics on `+ 1` (`attempt to add with overflow`).
     - If `pic_width_in_mbs_minus1 >= 0x1000_0000` (e.g. `268435456`), `(268435456 + 1) * 16 = 4,294,967,312 > u32::MAX`, panicking on `* 16` (`attempt to multiply with overflow`).
  2. Line 214:
     `let raw_height = (self.pic_height_in_map_units_minus1 + 1) * 16 * sub_height_factor;`
     - When `frame_mbs_only_flag == false` (`sub_height_factor = 2`), for any `pic_height_in_map_units_minus1 >= 0x0800_0000`, multiplication overflows `u32`.
  3. Lines 233–234:
     `let crop_x = (self.frame_crop_left_offset + self.frame_crop_right_offset) * crop_unit_x;`
     `let crop_y = (self.frame_crop_top_offset + self.frame_crop_bottom_offset) * crop_unit_y;`
     - If `frame_crop_left_offset + frame_crop_right_offset > u32::MAX`, panics on `+`.
     - If `(left + right) * crop_unit_x > u32::MAX`, panics on `*`.
- **Empirical confirmation**:
  Reproduced in `test_sps_visible_dimensions_integer_overflow_vulnerabilities`:
  ```
  thread panicked at crates/syncplay-codecs/src/h264/sps.rs:212:25: attempt to multiply with overflow
  thread panicked at crates/syncplay-codecs/src/h264/sps.rs:212:25: attempt to add with overflow
  thread panicked at crates/syncplay-codecs/src/h264/sps.rs:233:22: attempt to add with overflow
  ```
- **Blast radius**: Complete crash of any caller requesting visible display dimensions from a parsed SPS on untrusted video input.
- **Mitigation**:
  1. Enforce bounds checks in `Sps::parse`: validate that `pic_width_in_mbs_minus1` and `pic_height_in_map_units_minus1` do not exceed standard level maximums (e.g. `pic_width_in_mbs_minus1 < 1024` and `pic_height_in_map_units_minus1 < 1024`, corresponding to resolutions $\le 16384 \times 16384$), returning `CodecError::InvalidSps("macroblock dimensions exceed supported limit")`.
  2. In `Sps::visible_dimensions()`, use 64-bit integer arithmetic (`u64`) or checked math (`checked_add`, `checked_mul`), clamping the output safely to `u32`.

---

### [HIGH] Challenge 3: Integer Addition Overflow Panic in `Sps::parse` Scaling Lists on Large `delta_scale`

- **Assumption challenged**: Assumed `last_scale + delta_scale + 256` fits within `i32`.
- **Attack scenario**:
  In `crates/syncplay-codecs/src/h264/sps.rs` (lines 114–115):
  ```rust
  let delta_scale = reader.read_se()?;
  next_scale = (last_scale + delta_scale + 256) % 256;
  ```
  `delta_scale` is an `i32` read from `reader.read_se()?`. If a malicious or malformed bitstream provides `delta_scale = i32::MAX` (`2147483647`), evaluating `8 + 2147483647 + 256` causes an addition overflow in 32-bit signed integer arithmetic.
- **Empirical confirmation**:
  Reproduced in `test_scaling_matrix_delta_scale_overflow`:
  ```
  thread 'test_scaling_matrix_delta_scale_overflow' panicked at crates/syncplay-codecs/src/h264/sps.rs:115:47:
  attempt to add with overflow
  ```
- **Blast radius**: Process crash when decoding High Profile SPS scaling matrix.
- **Mitigation**:
  Perform the addition in `i64`:
  `next_scale = ((last_scale as i64 + delta_scale as i64 + 256).rem_euclid(256)) as i32;`

---

### [HIGH] Challenge 4: Integer Addition Overflow Panic in `Pps::parse` Scaling Lists on Large `delta_scale`

- **Assumption challenged**: Assumed `last_scale + delta_scale + 256` fits within `i32`.
- **Attack scenario**:
  In `crates/syncplay-codecs/src/h264/pps.rs` (lines 116–117):
  ```rust
  let delta_scale = reader.read_se()?;
  next_scale = (last_scale + delta_scale + 256) % 256;
  ```
  Identical mechanism as Challenge 3. When `pic_scaling_matrix_present_flag` is set in PPS and `delta_scale` is large, it panics on addition overflow.
- **Empirical confirmation**:
  Reproduced in `test_pps_scaling_matrix_delta_scale_overflow`:
  ```
  thread 'test_pps_scaling_matrix_delta_scale_overflow' panicked at crates/syncplay-codecs/src/h264/pps.rs:117:47:
  attempt to add with overflow
  ```
- **Blast radius**: Process crash when decoding Picture Parameter Sets with scaling lists.
- **Mitigation**:
  Perform addition in `i64`:
  `next_scale = ((last_scale as i64 + delta_scale as i64 + 256).rem_euclid(256)) as i32;`

---

### [MEDIUM] Challenge 5: Unbounded Heap Allocation in `VideoFrameYUV420p::new` on Huge Visible Dimensions

- **Assumption challenged**: Assumed dimensions passed to `VideoFrameYUV420p::new` will not exhaust memory.
- **Attack scenario**:
  In `crates/syncplay-codecs/src/frame.rs` (lines 133–156):
  `y_capacity = stride_y * (height as usize) + 16`.
  If an SPS with dimension 50,000 x 50,000 passes to `VideoFrameYUV420p::new`, it calculates ~2.5 GB buffer requirements per frame. If allocation fails, `AlignedBuffer::new` invokes `std::alloc::handle_alloc_error`, terminating the process immediately.
- **Blast radius**: OOM denial of service.
- **Mitigation**:
  Validate maximum display dimensions (e.g. width $\le 8192$, height $\le 4320$) in `Sps::parse` or `H264Decoder::decode_packet`, returning `CodecError::InvalidSps("dimensions exceed maximum supported 8K resolution")`.

---

## Stress Test Results Matrix

| Test Suite / Area | Tested Input / Scenario | Expected Behavior | Actual Behavior | Result |
|---|---|---|---|---|
| **Exp-Golomb Leading Zeros** | 32, 64, 1000 leading zeros | Return `CommonError::InvalidData` | Returned `CommonError::InvalidData` | **PASS** |
| **Exp-Golomb Truncation** | Truncation during zeros & info bits | Return `CommonError::UnexpectedEof` | Returned `CommonError::UnexpectedEof` | **PASS** |
| **`read_te` Bounds** | `max == 0` | Return `Ok(0)`, 0 bits consumed | Returned `Ok(0)`, 0 bits consumed | **PASS** |
| **`read_te` Bounds** | `max == 1`, bit `1` vs `0` | Bit 1 -> 0, Bit 0 -> 1 | Correctly inverted bits | **PASS** |
| **`read_se` Boundaries** | `i32::MAX` and `-2147483647` | Correct signed values mapped | Correct signed values returned | **PASS** |
| **Annex B Scanner** | Corrupt start codes, 4096 zeros, EOF | No panics, clean empty returns | Clean empty returns, no hangs | **PASS** |
| **Annex B Trailing Zeros** | Trailing zeros at EOF & between NALs | Trailing zeros trimmed | Trimmed without underflow | **PASS** |
| **AVCC Length Headers** | Invalid sizes (0, 3, 5, 8), length > buffer | Return `CodecError::InvalidNal` | Returned `CodecError::InvalidNal` | **PASS** |
| **Emulation Prevention** | `0x000003` at buffer boundary & cascaded | Discard `0x03` without over-reads | Stripped cleanly | **PASS** |
| **Crop Saturation** | Crop offsets exceeding dimensions | Saturate to 0 width/height | Correctly saturated to 0 | **PASS** |
| **Dynamic Resolution Switch** | Prev frame smaller than current with `first_mb > 0` | Safe copy or fallback | **Panicked with `index out of bounds`** | **FAIL** |
| **SPS Visible Dimensions** | Huge macroblock counts / crop offsets | Safe error or clamped dimension | **Panicked with `attempt to multiply with overflow` / `attempt to add with overflow`** | **FAIL** |
| **SPS Scaling Matrix** | `delta_scale == i32::MAX` | Safe modular arithmetic in `[0, 255]` | **Panicked with `attempt to add with overflow`** | **FAIL** |
| **PPS Scaling Matrix** | `delta_scale == i32::MAX` | Safe modular arithmetic in `[0, 255]` | **Panicked with `attempt to add with overflow`** | **FAIL** |
| **Fuzzing Harness** | 2,000 pseudo-random malformed packets | Zero unhandled panics | No unhandled panics outside the above | **PASS** |

---

## Verdict & Recommendation

**Verdict**: **`REQUEST_CHANGES`**

The implementation in `crates/syncplay-codecs` must be hardened before Milestone 1 can be signed off:
1. Fix `crates/syncplay-codecs/src/h264/decoder.rs:198-211`: check `prev.width == frame.width && prev.height == frame.height` before copying from `self.last_frame`.
2. Fix `crates/syncplay-codecs/src/h264/sps.rs:212, 233`: validate macroblock count limits in `Sps::parse` ($\le 1024$) and use `u64` arithmetic in `visible_dimensions()`.
3. Fix `crates/syncplay-codecs/src/h264/sps.rs:115` and `crates/syncplay-codecs/src/h264/pps.rs:117`: use `i64` for `(last_scale as i64 + delta_scale as i64 + 256).rem_euclid(256) as i32`.
4. Validate max supported resolution in `H264Decoder::decode_packet` (e.g. $\le 8192 \times 4320$) to prevent OOM panics.
