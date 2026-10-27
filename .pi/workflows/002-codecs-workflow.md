# Workflow: 002-codecs-workflow

## 1. Metadata & OpenSpec Mapping
- **Workflow ID**: `002-codecs-workflow`
- **Target Change Proposal**: `openspec/changes/002-codecs-presentation.md`
- **Capability Specification**: `openspec/specs/codecs/spec.md`
- **Target Crates**: `syncverse-codecs`
- **Primary Agents**: `spec_implementer`
- **Required Skills**: `openspec-change-mapping`, `zero-dependency-audit`

---

## 2. Prerequisites & Dependencies
- **Upstream Dependencies**: `001-demuxer-workflow` (Needs `MediaTime` and packet primitives).
- **Blocks**: `003-clock-workflow`.
- **Pre-flight Checks**:
  1. Confirm `syncverse-common` and `syncverse-demux` tests pass cleanly.
  2. Confirm zero external codec dependencies (`libswscale`, `dav1d`, etc.).

---

## 3. Step-by-Step Execution Plan

### Step 1: Memory-Aligned Frame Data Models (`syncverse-codecs/src/frame/`)
1. Implement `VideoFrameYUV420p` (`video.rs`):
   - Separate 64-byte aligned luminance ($Y$) and chroma ($U, V$) planes.
   - Enforce 16-byte row stride alignment: `stride = (width + 15) & !15`.
   - Allocate $+16$B SIMD tail padding at the end of each plane buffer to prevent out-of-bounds over-reads during vectorized loads.
2. Implement `VideoFrameRGBA`:
   - Packed 32-bit RGBA texture buffer with 64-byte row alignment for zero-copy presentation handoff.
3. Implement `AudioFramePCM` (`audio.rs`):
   - Interleaved 32-bit floating-point audio samples normalized in range $[-1.0, 1.0]$.

### Step 2: Q8 Fixed-Point Color Space Conversion (`syncverse-codecs/src/color/`)
1. Implement ITU-R BT.601 limited-range conversion (`bt601.rs`):
   - $Q8$ integer arithmetic with $+128$ rounding offset:
     $$C = Y - 16, \quad D = U - 128, \quad E = V - 128$$
     $$R = \text{clamp}((298 \cdot C + 409 \cdot E + 128) \gg 8)$$
     $$G = \text{clamp}((298 \cdot C - 100 \cdot D - 208 \cdot E + 128) \gg 8)$$
     $$B = \text{clamp}((298 \cdot C + 516 \cdot D + 128) \gg 8)$$
2. Implement ITU-R BT.709 high-definition conversion (`bt709.rs`).
3. Enforce branchless saturation via `clamp(0, 255) as u8`.
4. Ensure zero heap allocations inside the pixel conversion loop.

### Step 3: Chroma 4:2:0 Spatial Expansion
1. Implement spatial upsampling mapping each $(U, V)$ sample pair to a $2 \times 2$ block of luminance pixels.
2. Verify stride indexing across odd and even video heights.

---

## 4. Verification & Acceptance Criteria
Execute the automated test suite:
```bash
# Crate unit test verification
cargo test -p syncverse-codecs -- --nocapture

# Adversarial stride and SIMD alignment verification
python3 tests/empirical_challenge/test_demux_codecs_clock.py

# Lint inspection
cargo clippy -p syncverse-codecs --all-targets -- -D warnings
```
Criteria for success:
- 100% test pass rate with 0 errors.
- Verified absence of buffer over-reads across odd/even video dimensions.
- 0 compiler warnings.
