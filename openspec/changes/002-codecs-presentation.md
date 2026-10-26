# Change Proposal: 002-codecs-presentation

**Change ID**: `002-codecs-presentation`  
**Title**: Baseline Frame Representations & Pure-Rust Fixed-Point Color Space Conversion  
**Status**: Ready for Implementation  
**Target Milestone**: Phase 1 Foundational Primitives  

---

## 1. Context & Motivation

Presentation surfaces (Wayland SHM buffers, X11 shared memory, GPU framebuffers) require packed 32-bit `RGBA` textures, whereas video decoders output planar YUV420p ($Y'CbCr$ 4:2:0). To prevent floating-point rounding divergence and eliminate dependencies on `libswscale`, this change implements high-efficiency fixed-point integer color transforms in pure safe Rust.

---

## 2. Dependency Graph

```
[001-demuxer-bitstream]
         │
         ▼
[002-codecs-presentation] (syncplay-codecs)
         │
         ▼
[003-clock-pipeline]
```

- **Dependencies**: `001-demuxer-bitstream`.
- **Blocks**: `003-clock-pipeline`.

---

## 3. Affected Crates & Components

- **`crates/syncplay-codecs`**:
  - `src/frame/video.rs`: 64-byte cache-aligned `VideoFrameYUV420p` with 16-byte row stride padding and packed `VideoFrameRGBA`.
  - `src/frame/audio.rs`: Normalized interleaved `AudioFramePCM` (`f32` samples in $[-1.0, 1.0]$).
  - `src/color/bt601.rs`: ITU-R BT.601 limited-range Q8 integer conversion.
  - `src/color/bt709.rs`: ITU-R BT.709 high-definition Q8 integer conversion.
  - `src/color/mod.rs`: Trait `ColorSpaceConverter` and plane traversal.

---

## 4. Deliverables & Implementation Plan

1. **Memory-Aligned Frame Buffers**:
   - `VideoFrameYUV420p` with separate 64-byte aligned luminance ($Y$) and chroma ($U, V$) planes, 16-byte aligned row strides (`(width + 15) & !15`), and $+16$B SIMD tail padding.
   - `VideoFrameRGBA` with 64-byte row stride padding for zero-copy presentation.
2. **Q8 Fixed-Point Color Conversion**:
   - $Q8$ integer arithmetic (scaling factor $256$) with rounding offset $+128$:
     - $C = Y - 16, \quad D = U - 128, \quad E = V - 128$
     - $R = \text{clamp}((298 \cdot C + 409 \cdot E + 128) \gg 8)$
     - $G = \text{clamp}((298 \cdot C - 100 \cdot D - 208 \cdot E + 128) \gg 8)$
     - $B = \text{clamp}((298 \cdot C + 516 \cdot D + 128) \gg 8)$
   - Branchless clamping via `clamp(0, 255) as u8`.
3. **Chroma 4:2:0 Spatial Expansion**:
   - Mapping each $U, V$ chroma sample to a $2 \times 2$ block of luminance pixels.
   - Zero dynamic heap allocation in the conversion loop.

---

## 5. Verification Commands

Execute the following commands from the repository root:

```bash
# 1. Type and borrow check affected crate
cargo check -p syncplay-codecs

# 2. Run unit tests including color accuracy matrices
cargo test -p syncplay-codecs -- --nocapture

# 3. Verify zero banned dependencies
cargo tree -p syncplay-codecs --edges no-dev
```

Expected output: Color conversion tests for Reference Black $(16,128,128) \to (0,0,0)$ and Reference White $(235,128,128) \to (255,255,255)$ pass with zero warnings.
