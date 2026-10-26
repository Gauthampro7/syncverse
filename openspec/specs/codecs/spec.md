# Capability Specification: Codecs & Pixel Pipeline (`syncplay-codecs`)

**Component**: `syncplay-codecs`  
**Standards Compliance**: ITU-R BT.601, ITU-R BT.709, YUV420p (I420), Packed RGBA32  
**Status**: Authoritative OpenSpec Specification  

---

## 1. Overview & Architectural Role

The `syncplay-codecs` crate encapsulates baseline video/audio frame representations, zero-allocation memory buffers, and pure-Rust color space transformations. In adherence to the zero-external-codec invariant, the color space conversion pipeline does not link against `libswscale`, `image`, or hardware C-FFI libraries.

Video presentation requires converting planar YUV420p ($Y'CbCr$ 4:2:0 chroma-subsampled) frames into packed 32-bit `RGBA` textures suitable for display rendering surfaces (Wayland `wl_shm`, X11 shared memory, or GPU framebuffer uploads). To ensure deterministic results across diverse CPU architectures and avoid floating-point rounding discrepancies or scalar FPU stalls, all color transformations are executed using 8-bit fractional fixed-point ($Q8$) integer arithmetic with branchless clamping.

---

## 2. Requirements & Scenarios

### Requirement: Planar YUV420p Frame Memory Layout
The codec subsystem SHALL represent uncompressed video frames in planar YUV420p (I420) format. The frame buffer MUST consist of three separate planes with row strides aligned to at least 16 bytes to prevent SIMD AVX2/NEON vector over-read:
1. **Luminance Plane ($Y$)**: $W \times H$ bytes with row stride $S_y \ge (\text{width} + 15) \ \& \ !15$.
2. **Chroma Blue-Difference Plane ($U / Cb$)**: $\lceil W/2 \rceil \times \lceil H/2 \rceil$ bytes with row stride $S_u \ge (\lceil W/2 \rceil + 15) \ \& \ !15$.
3. **Chroma Red-Difference Plane ($V / Cr$)**: $\lceil W/2 \rceil \times \lceil H/2 \rceil$ bytes with row stride $S_v \ge (\lceil W/2 \rceil + 15) \ \& \ !15$.

All plane buffer allocations MUST be aligned to 64-byte boundaries (`#[repr(align(64))]`) and include at least 16 bytes of end-of-buffer tail padding to permit aligned SIMD loads without reading past allocation boundaries on the final row.

#### Scenario: Allocate and Validate 1080p YUV420p Frame Layout
- **Given** video dimensions $W = 1920$ and $H = 1080$
- **When** a new `VideoFrameYUV420p` is initialized
- **Then** the Y-plane size is at least $1920 \times 1080 = 2,073,600\text{ bytes}$
- **And** the U-plane and V-plane sizes are each at least $960 \times 540 = 518,400\text{ bytes}$
- **And** the total frame payload footprint equals $1.5 \times W \times H = 3,110,400\text{ bytes}$
- **And** plane pointer memory addresses are 64-byte aligned.

#### Scenario: Allocate Non-Multiple-of-16 Frame with Padded Strides
- **Given** video dimensions $W = 854$ (480p) and $H = 480$
- **When** a new `VideoFrameYUV420p` is initialized
- **Then** the Y-plane stride $S_y$ is $(854 + 15) \ \& \ !15 = 864\text{ bytes}$
- **And** chroma width is $\lceil 854 / 2 \rceil = 427\text{ pixels}$
- **And** chroma strides $S_u, S_v$ are $(427 + 15) \ \& \ !15 = 432\text{ bytes}$
- **And** 16-pixel SIMD vector operations execute across all rows without out-of-bounds reads.

---

### Requirement: Packed RGBA32 Presentation Buffer Layout
The codec subsystem SHALL provide a packed RGBA32 frame structure (`VideoFrameRGBA`) for presentation. Pixels MUST be stored contiguously in memory as 4-octet tuples: `[R, G, B, A]`, where Alpha is fixed to `255` (`0xFF`). The row stride SHALL be padded to a multiple of 64 bytes to eliminate unaligned cache-line crossings during texture uploads.

#### Scenario: Validate RGBA Frame Buffer Geometry
- **Given** frame dimensions $W = 1920$ and $H = 1080$
- **When** `VideoFrameRGBA` is allocated with row alignment 64
- **Then** stride is $1920 \times 4 = 7680\text{ bytes}$ (already a multiple of 64)
- **And** total buffer capacity is $7680 \times 1080 = 8,294,400\text{ bytes}$
- **And** every fourth byte represents the Alpha channel initialized to `255`.

---

### Requirement: Pure-Rust Q8 Fixed-Point BT.601 Color Conversion
The color space conversion engine SHALL convert Studio Swing / Limited Range YUV420p ($Y \in [16, 235], U \in [16, 240], V \in [16, 240]$) to Full Range sRGB ($R, G, B \in [0, 255]$) conforming to ITU-R BT.601. The conversion MUST use integer arithmetic with $Q8$ scaling ($2^8 = 256$ multiplier) and rounding bias $+128$:
- $C = Y - 16$
- $D = U - 128$
- $E = V - 128$
- $R = \text{clamp}\left(\frac{298 \cdot C + 409 \cdot E + 128}{256}\right)$
- $G = \text{clamp}\left(\frac{298 \cdot C - 100 \cdot D - 208 \cdot E + 128}{256}\right)$
- $B = \text{clamp}\left(\frac{298 \cdot C + 516 \cdot D + 128}{256}\right)$

All intermediate computations MUST be executed in 32-bit signed integers (`i32`) to prevent arithmetic overflow. Clamping to $[0, 255]$ MUST be branchless using bitwise saturation.

#### Scenario: Convert Standard BT.601 Primary Reference Colors
- **Given** standard limited-range ITU-R BT.601 test vectors:
  - Reference Black: $(Y=16, U=128, V=128)$
  - Reference White: $(Y=235, U=128, V=128)$
  - Reference Saturated Red: $(Y=81, U=90, V=240)$
- **When** `yuv_to_rgb_bt601` is applied to each pixel
- **Then**:
  - Reference Black converts to $(R=0, G=0, B=0)$
  - Reference White converts to $(R=255, G=255, B=255)$
  - Reference Saturated Red converts to $(R \ge 250, G \le 2, B \le 2)$.

---

### Requirement: Pure-Rust Q8 Fixed-Point BT.709 Color Conversion
For High Definition (HDTV / 1080p / 4K) streams, the converter SHALL support ITU-R BT.709 colorimetry using $Q8$ fixed-point integer arithmetic:
- $C = Y - 16$
- $D = U - 128$
- $E = V - 128$
- $R = \text{clamp}\left(\frac{298 \cdot C + 459 \cdot E + 128}{256}\right)$
- $G = \text{clamp}\left(\frac{298 \cdot C - 55 \cdot D - 136 \cdot E + 128}{256}\right)$
- $B = \text{clamp}\left(\frac{298 \cdot C + 541 \cdot D + 128}{256}\right)$

The conversion error compared to 64-bit double-precision floating point MUST NOT exceed $\pm 1$ LSB ($< 0.4\%$ error) across the entire valid color gamut.

#### Scenario: Verify BT.709 High Definition Color Transform
- **Given** high-definition limited-range input: $(Y=235, U=128, V=128)$
- **When** `yuv_to_rgb_bt709` is executed
- **Then** output is $(R=255, G=255, B=255)$
- **Given** pure blue input $(Y=41, U=240, V=110)$
- **When** `yuv_to_rgb_bt709` is executed
- **Then** output produces blue dominance $(B \ge 250, R \le 2)$.

---

### Requirement: 4:2:0 Chroma Subsampling Spatial Reconstruction
The color converter SHALL correctly map the subsampled chroma planes ($U, V$) to luminance ($Y$). Because each chroma sample corresponds to a $2 \times 2$ block of luminance pixels, the converter MUST read:
$$U(x, y) = U_{\text{plane}}\left[\left\lfloor \frac{y}{2} \right\rfloor \cdot S_u + \left\lfloor \frac{x}{2} \right\rfloor\right]$$
$$V(x, y) = V_{\text{plane}}\left[\left\lfloor \frac{y}{2} \right\rfloor \cdot S_v + \left\lfloor \frac{x}{2} \right\rfloor\right]$$
The implementation SHALL process 2 horizontal pixels simultaneously or 16 pixels via SIMD vectorization without reading past buffer boundaries.

#### Scenario: Convert Full Frame Without Memory Leaks or Out-of-Bounds Access
- **Given** a planar YUV420p image of size $64 \times 64$
- **And** a target RGBA buffer of size $64 \times 64 \times 4 = 16,384\text{ bytes}$
- **When** `convert_yuv420p_to_rgba` executes
- **Then** exactly $4096$ pixels ($16,384$ bytes) are populated
- **And** zero memory allocations or reallocations occur during the call.

---

### Requirement: Interleaved 32-bit Floating-Point Audio Frame Representation
The codec subsystem SHALL represent decoded audio frames (`AudioFramePCM`) as normalized 32-bit floating-point samples (`f32`) interleaved across channels:
`[L0, R0, L1, R1, L2, R2, ...]`.
Each sample MUST be bounded within the linear amplitude range $[-1.0, 1.0]$. The frame structure MUST record the audio sample rate (nominally $48,000\text{ Hz}$ or $44,100\text{ Hz}$), channel count, and starting presentation timestamp (PTS).

#### Scenario: Verify Audio Frame Normalization and Duration
- **Given** an `AudioFramePCM` with sample rate $48,000\text{ Hz}$, 2 channels, and $960$ stereo sample pairs ($1920$ total `f32` values)
- **When** inspecting frame properties
- **Then** all sample values satisfy $-1.0 \le s \le 1.0$
- **And** the effective duration of the frame is computed as $\frac{960}{48000} = 0.020\text{ seconds}$ ($20\text{ ms}$).
