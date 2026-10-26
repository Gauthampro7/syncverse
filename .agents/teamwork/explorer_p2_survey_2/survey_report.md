# Technical Survey Report: Video & Audio Presentation Engine (R2)

**Component Scope**: `crates/syncplay-codecs`, `crates/syncplay-clock`, `crates/syncplay-ui`, `crates/syncplay-core`  
**OpenSpec Artifacts**: `openspec/specs/codecs/spec.md`, `openspec/specs/av-pipeline/spec.md`, `openspec/specs/ui-renderer/spec.md`, `openspec/changes/002-codecs-presentation.md`, `openspec/changes/003-clock-pipeline.md`, `openspec/changes/006-ui-osd-renderer.md`  
**Author**: `explorer_p2_survey_2` (Teamwork Preview Explorer)  
**Date**: 2026-10-25  

---

## 1. Executive Summary

This survey establishes the complete technical blueprint, data models, concurrency invariants, and mathematical algorithms for **Requirement 2 (R2): Video & Audio Presentation Engine** of Phase 2 of Syncplay-Alt.

In strict adherence to the **Zero-External-Codec Invariant** (AGENTS.md § 2.1), Syncplay-Alt avoids all third-party media libraries (`libswscale`, `ffmpeg`, `image`, `vlc`, `mpv`, `symphonia`). All video frame representations, audio PCM buffering, color space transformations, presentation pacing, lock-free thread decoupling, and native user interface state handling are engineered from first principles in pure idiomatic Rust.

### Core Survey Findings:
1. **Color Space Conversion**: The current Q8 fixed-point integer color space converter under `crates/syncplay-codecs` accurately maps Studio Swing / Limited Range YUV420p to full-range sRGB for ITU-R BT.601 and BT.709 with zero heap allocations. It enforces 16-byte row stride alignment and +16-byte SIMD tail padding on 64-byte aligned buffers. For Worker implementation, hot-loop throughput can be doubled by sharing $(U, V)$ chroma calculations across horizontal pixel pairs $(x, x+1)$ in 4:2:0 subsampling, standardizing branchless clamping, and providing a clean `ColorSpaceConverter` trait.
2. **Lock-Free Inter-Thread Transfers**: `crates/syncplay-clock/src/spsc.rs` implements a power-of-two circular SPSC ring buffer with head and tail counters isolated on independent 128-byte cache lines (`#[repr(align(128))]`). Memory ordering uses pure `Acquire`/`Release` pairs with monotonic wrapping arithmetic, mathematically eliminating false sharing and mutex stalls.
3. **Master Presentation Clock**: `crates/syncplay-clock/src/master_clock.rs` implements a continuous monotonic time base using an optimistic `SeqLock` protocol. Re-anchoring speed adjustments guarantees $C^0$ timeline continuity ($\lim_{\epsilon \to 0} |\text{PTS}(t+\epsilon) - \text{PTS}(t-\epsilon)| = 0$) with zero phantom jumps. The 4-zone presentation decision window (Wait, PresentNow, PresentNowLate, DropLateFrame) and 16-frame bounded `PresentationReorderBuffer` correctly resolve frame pacing.
4. **UI Decoupling & Memory Bounding**: `crates/syncplay-ui` implements a 72-byte `PlayerUiSnapshot` published through an SPSC triple-buffer exchange where the centralized `TelemetryAggregator` is the sole authorized writer. The 10 Hz scrubber debouncer emits exactly 5 intermediate seeks + 1 release seek for a 500ms drag at 120 Hz. UI RAM is strictly capped under the 15.0 MB ceiling using a single-channel Alpha8 (`R8_UNORM`) font glyph atlas (4.0 MB for $2048 \times 2048$) and a 5-bubble capped chat overlay with 5.0s lifecycle.

---

## 2. Codebase & Specification Inventory

| Subsystem / Crate | OpenSpec Spec | OpenSpec Change | Key Source Files | Status |
| :--- | :--- | :--- | :--- | :--- |
| **`syncplay-codecs`** | `specs/codecs/spec.md` | `002-codecs-presentation.md` | `src/frame.rs`<br>`src/color.rs`<br>`src/error.rs` | Foundational primitives complete. Needs Chroma $2\times 2$ reuse optimization, trait modularization (`ColorSpaceConverter`), and explicit batch SIMD paths. |
| **`syncplay-clock`** | `specs/av-pipeline/spec.md` | `003-clock-pipeline.md` | `src/master_clock.rs`<br>`src/spsc.rs`<br>`src/reorder.rs` | Complete and verified. Monotonic SeqLock re-anchoring and 128B SPSC passing all concurrency stress tests. Ready for multi-threaded presentation loop orchestration in `syncplay-core`. |
| **`syncplay-ui`** | `specs/ui-renderer/spec.md` | `006-ui-osd-renderer.md` | `src/snapshot.rs`<br>`src/scrubber.rs`<br>`src/overlay.rs`<br>`src/command.rs` | Snapshot triple-buffer, 10Hz debouncer, Alpha8 font atlas, and bounded chat overlay verified. OSD HUD compositing structures and command types ready for event loop integration. |
| **`syncplay-core`** | `specs/av-pipeline/spec.md`<br>`specs/ui-renderer/spec.md` | `003-clock-pipeline.md`<br>`006-ui-osd-renderer.md` | `src/coordinator.rs`<br>`src/aggregator.rs`<br>`src/main.rs` | Telemetry Aggregator and coordinator ticks verified. Needs live cross-thread presentation runner coupling decoder SPSC queues with display surfaces. |

---

## 3. Deep Domain Analysis

### 3.1 Domain A: High-Throughput Q8 Fixed-Point Color Space Conversion (`syncplay-codecs`)

#### 3.1.1 Colorimetry & Integer Scaling Mathematics
Video demuxers output planar YUV420p (I420) where color samples are represented in Studio Swing / Limited Range:
- Luminance $Y \in [16, 235]$
- Chrominance Blue-Difference $U (Cb) \in [16, 240]$
- Chrominance Red-Difference $V (Cr) \in [16, 240]$

Direct presentation surfaces require full-range packed 32-bit `RGBA` ($R, G, B \in [0, 255]$ with Alpha $= 255$). To avoid floating-point non-determinism, CPU FPU context switching, and external C-FFI libraries (`libswscale`), color conversion is performed via 8-bit fractional fixed-point ($Q8$, multiplier $2^8 = 256$) integer arithmetic with rounding offset $+128$.

**Inputs**:
$$C = Y - 16, \quad D = U - 128, \quad E = V - 128$$

**ITU-R BT.601 (Standard Definition / SDTV)**:
$$R = \text{clamp}\left( (298 \cdot C + 409 \cdot E + 128) \gg 8 \right)$$
$$G = \text{clamp}\left( (298 \cdot C - 100 \cdot D - 208 \cdot E + 128) \gg 8 \right)$$
$$B = \text{clamp}\left( (298 \cdot C + 516 \cdot D + 128) \gg 8 \right)$$

**ITU-R BT.709 (High Definition / HDTV / 1080p)**:
$$R = \text{clamp}\left( (298 \cdot C + 459 \cdot E + 128) \gg 8 \right)$$
$$G = \text{clamp}\left( (298 \cdot C - 55 \cdot D - 136 \cdot E + 128) \gg 8 \right)$$
$$B = \text{clamp}\left( (298 \cdot C + 541 \cdot D + 128) \gg 8 \right)$$

All intermediate arithmetic is performed in 32-bit signed integers (`i32`) to guarantee absence of overflow.

#### 3.1.2 Memory Geometry & SIMD Alignment Guarantees
In `syncplay-codecs/src/frame.rs`:
- `VideoFrameYUV420p` allocates three discrete planes via `AlignedBuffer`:
  - $Y$ plane: Row stride $S_y = ((W + 15) \ \& \ !15)$, capacity $= S_y \times H + 16$ bytes.
  - $U, V$ planes: Width $\lceil W/2 \rceil$, height $\lceil H/2 \rceil$. Row stride $S_u = S_v = ((\lceil W/2 \rceil + 15) \ \& \ !15)$, capacity $= S_u \times \lceil H/2 \rceil + 16$ bytes.
  - All plane allocations use `std::alloc::alloc_zeroed` with 64-byte alignment (`Layout::from_size_align(cap, 64)`).
  - The $+16$ bytes SIMD tail padding guarantees that 128-bit vector loads (e.g. 16 bytes via SSE/AVX2/NEON) executed on the final row will never cross into unmapped virtual memory or fault on unaligned trailing bytes.
- `VideoFrameRGBA`:
  - Row stride $S_{\text{rgba}} = (((W \times 4) + 63) \ \& \ !63)$ bytes (aligned to 64 bytes).
  - Eliminates unaligned cache-line crossings during GPU / compositor texture upload.
  - Pre-initializes the 4th octet (Alpha) of each pixel to `255` (`0xFF`).

#### 3.1.3 Throughput Optimization Opportunities for Worker Implementation
1. **Chroma 4:2:0 Spatial Reuse**:
   In 4:2:0 subsampling, each $(U, V)$ chroma sample is shared across a $2 \times 2$ block of luminance pixels. The current implementation in `crates/syncplay-codecs/src/color.rs` executes scalar per-pixel lookups:
   ```rust
   let u_val = u_plane[uv_row_offset + (x >> 1)];
   let v_val = v_plane[(y >> 1) * stride_v + (x >> 1)];
   ```
   **Worker Optimization**: Process horizontal pixel pairs $(x, x+1)$ in an unrolled inner loop. The chroma terms:
   $$R_{\text{chroma}} = 409 \cdot E + 128$$
   $$G_{\text{chroma}} = -100 \cdot D - 208 \cdot E + 128$$
   $$B_{\text{chroma}} = 516 \cdot D + 128$$
   are computed **only once per 2 luminance pixels**. This eliminates 50% of chroma memory loads, additions, and multiplications in the conversion hot loop.
2. **Branchless Clamping**:
   Ensure clamping utilizes branchless saturation:
   ```rust
   #[inline(always)]
   pub fn clamp_u8(val: i32) -> u8 {
       val.clamp(0, 255) as u8
   }
   ```
   Compiles into `packuswb` or conditional moves (`cmov`) on x86_64, preventing branch misprediction penalties.
3. **Trait Architecture**:
   To fulfill `002-codecs-presentation.md` § 3, introduce `ColorSpaceConverter` trait:
   ```rust
   pub trait ColorSpaceConverter: Send + Sync {
       fn convert_frame(&self, src: &VideoFrameYUV420p, dst: &mut VideoFrameRGBA) -> Result<(), CodecError>;
       fn standard(&self) -> ColorStandard;
   }
   ```
   implemented by `Bt601Converter` and `Bt709Converter`.

---

### 3.2 Domain B: Lock-Free 128-Byte Cache-Aligned SPSC Ring Buffer (`syncplay-clock`)

#### 3.2.1 SPSC Architecture & Memory Model
Inter-thread communication across decoding, presentation, and user interface stages cannot use mutexes (`std::sync::Mutex`) or channel abstractions that allocate or block:
- **Topology**: Strictly Single-Producer Single-Consumer (SPSC).
- **Structure**: `SpscRingBuffer<T, const CAPACITY: usize>` in `crates/syncplay-clock/src/spsc.rs`.
- **Power-of-Two Invariant**: `CAPACITY.is_power_of_two()` checked at construction.
- **Bitwise Masking**: Slot index is derived via `slot = index & (CAPACITY - 1)`.

#### 3.2.2 False Sharing Elimination via 128-Byte Cache Line Alignment
On multi-core CPUs, adjacent variables placed within the same 64-byte or 128-byte cache line cause continuous cache invalidations ("ping-ponging") between producer and consumer cores.
- `syncplay-clock` defines `CacheAligned<T>` with `#[repr(align(128))]`.
- `head: CacheAligned<AtomicUsize>` and `tail: CacheAligned<AtomicUsize>` are physically separated by $\ge 128$ bytes (`cache_line_distance() >= 128`).
- Producer writes to `head` never trigger Request-For-Ownership (RFO) invalidation of the consumer's cached line containing `tail`.

#### 3.2.3 Monotonic Wrapping Counter Logic & Acquire-Release Semantics
Both `head` and `tail` are unbounded, free-running monotonic `AtomicUsize` counters:
- **Full Condition**: `head.load(Relaxed).wrapping_sub(tail.load(Acquire)) == CAPACITY`.
- **Empty Condition**: `head.load(Acquire) == tail.load(Relaxed)`.
- **Producer (`try_push`)**:
  ```rust
  let head = self.head.0.load(Ordering::Relaxed);
  let tail = self.tail.0.load(Ordering::Acquire);
  if head.wrapping_sub(tail) == CAPACITY {
      return Err(BufferFullError::new(item));
  }
  let slot_idx = head & (CAPACITY - 1);
  unsafe { (*self.slots[slot_idx].get()).write(item); }
  self.head.0.store(head.wrapping_add(1), Ordering::Release);
  ```
- **Consumer (`try_pop`)**:
  ```rust
  let tail = self.tail.0.load(Ordering::Relaxed);
  let head = self.head.0.load(Ordering::Acquire);
  if head == tail {
      return None;
  }
  let slot_idx = tail & (CAPACITY - 1);
  let item = unsafe { (*self.slots[slot_idx].get()).assume_init_read() };
  self.tail.0.store(tail.wrapping_add(1), Ordering::Release);
  ```
- Zero `SeqCst` overhead. All operations execute lock-free in $O(1)$ time.

---

### 3.3 Domain C: Master Monotonic Clock Continuous Presentation Loop (`syncplay-clock` & `syncplay-core`)

#### 3.3.1 Monotonic Time Base & Continuous Rate Re-Anchoring
Media presentation pacing requires absolute immunity against OS wall-clock adjustments. The master clock relies exclusively on `std::time::Instant` (`CLOCK_MONOTONIC`).

The presentation timestamp at instant $t$ is defined as:
$$\text{CurrentPTS}(t) = \begin{cases}
\text{anchor\_pts} + (t - \text{anchor\_wall}) \times \text{speed} \times 1000.0, & \text{if Playing} \\
\text{anchor\_pts}, & \text{if Paused / Seeking}
\end{cases}$$

When the Sync Engine commands a micro-speed slew ($r \in [0.96, 1.04]$):
1. Compute transition point: $\text{anchor\_pts}' \leftarrow \text{CurrentPTS}(t_{\text{now}})$.
2. Update anchor instant: $\text{anchor\_wall}' \leftarrow t_{\text{now}}$.
3. Assign new speed: $\text{speed}' \leftarrow s_{\text{new}}$.

This guarantees mathematical $C^0$ continuity:
$$\lim_{\epsilon \to 0^+} \text{CurrentPTS}(t_{\text{now}} + \epsilon) = \lim_{\epsilon \to 0^-} \text{CurrentPTS}(t_{\text{now}} - \epsilon)$$
eliminating phantom jumps, dropped frames, and visual stutter.

#### 3.3.2 SeqLock Optimistic Read Protection
Multi-field atomic updates to `(anchor_pts, anchor_wall, speed, state)` are synchronized using a sequence counter (`seq: AtomicUsize`):
- **Writers** increment `seq` from even to odd before writing, and from odd to even upon completion:
  $$\text{seq} \to \text{seq} + 1 \implies \text{write fields} \implies \text{seq} \to \text{seq} + 1$$
- **Readers** optimistically read fields only when `seq` is even, re-checking `seq` after reading:
  If $s_1 == s_2$ and $s_1$ is even, the read is torn-free and valid. If a concurrent write occurred, the reader spins and re-reads.
- Concurrency verification in `test_demux_codecs_clock.py` confirmed 0 torn reads and max step $< 4\text{ ms}$, whereas disjoint writes caused $10,000\text{ ms}$ errors.

#### 3.3.3 Presentation Decision Window
For candidate frame timestamp $\text{PTS}_{\text{frame}}$ against current master clock:
$$\Delta = \text{PTS}_{\text{frame}} - \text{MasterClock}(t_{\text{now}})$$
The presentation scheduler in `crates/syncplay-clock/src/master_clock.rs` enforces:
1. **Early ($\Delta > +10\text{ ms}$)**: `PresentationAction::Wait(Duration)` (yield/sleep for $\Delta - 10\text{ ms}$).
2. **On-Time ($-15\text{ ms} \le \Delta \le +10\text{ ms}$)**: `PresentationAction::PresentNow` (submit immediately to display compositor).
3. **Slightly Late ($-100\text{ ms} < \Delta < -15\text{ ms}$)**: `PresentationAction::PresentNowLate` (render immediately, increment late metric).
4. **Severely Late ($\Delta \le -100\text{ ms}$)**: `PresentationAction::DropLateFrame` (discard frame immediately without RGB conversion to clear backpressure).

#### 3.3.4 Bounded Presentation Reorder Buffer
In `crates/syncplay-clock/src/reorder.rs`, `PresentationReorderBuffer<T>` wraps a `BinaryHeap` configured as a min-heap with capacity $K = 16$. Decoded frames emitted in decode order (DTS) are inserted, and the frame with the lowest PTS is popped and passed to the presentation scheduler. On seek or container EOF, `drain_all()` flushes remaining frames in strictly increasing PTS order.

---

### 3.4 Domain D: 10Hz Scrubber Debouncer & Memory-Bounded OSD State Rendering (`syncplay-ui`)

#### 3.4.1 Scrubber Debouncing Dynamics
During interactive timeline dragging at display refresh rates (60–144 Hz):
1. **Visual Path**: Visual thumb position and tooltip timecode update at full event frequency ($< 16\text{ ms}$ response).
2. **Seek Path**: Network and demuxer seek packets are throttled to a maximum frequency of $10\text{ Hz}$ ($100\text{ ms}$ minimum interval).
3. **Release Authoritative Seek**: Upon mouse release, the debouncer immediately emits a final authoritative seek command.
- Verified in `test_ui_renderer.py`: A 500ms drag at 120 Hz generates 61 visual updates, exactly 5 intermediate seeks, and 1 final release seek (total = 6 seek commands).

#### 3.4.2 Lock-Free Triple-Buffered Atomic State Snapshot
The player state is encapsulated in `PlayerUiSnapshot` (72 bytes):
```rust
#[repr(C)]
pub struct PlayerUiSnapshot {
    pub monotonic_timestamp: u64,
    pub media_position_pts: u64,
    pub media_duration_pts: u64,
    pub playback_state: u64,
    pub network_rtt_nanos: u64,
    pub clock_offset_nanos: i64,
    pub phase_error_nanos: i64,
    pub pll_speed_ratio_q16: u32,
    pub peer_count: u16,
    pub peers_ready: u16,
    pub room_sequence: u64,
}
```
- Fits within a single 128-byte cache line pair ($72 \le 128$).
- Published exclusively by the central `TelemetryAggregator` in `syncplay-core` via `TripleBufferSnapshotExchange`.
- The UI event thread polls `poll_latest_snapshot()` in $O(1)$ time ($< 50\text{ ns}$) with zero mutex contention or thread blocking.

#### 3.4.3 Memory Budget Verification ($\le 15.0\text{ MB}$ UI RAM)
The UI renderer memory footprint is strictly bounded:
1. **Font Glyph Atlas**: Single-channel Alpha8 (`R8_UNORM`, 1 byte/texel).
   - $2048 \times 2048 \times 1\text{ B} = 4.0\text{ MB}$ (or $1024 \times 1024 = 1.0\text{ MB}$).
   - Banned 32-bit RGBA glyph textures ($16.0\text{ MB}$).
2. **OSD Overlay Framebuffer**: Dirty-rect compositing bounded to $\le 8.5\text{ MB}$.
3. **Telemetry Buffers & SPSC Queues**: $\le 1.0\text{ MB}$.
4. **Chat Overlay Display List**: `BoundedChatOverlay` caps visible bubbles at 5 (4.0s dwell, 1.0s fade). Under a burst of 1,000 incoming messages in 1 second, memory consumption is restricted to $1.2\text{ KB}$ (995 messages evicted).
- **Total UI RAM Footprint**:
  $$4.0\text{ MB} (\text{Atlas}) + 8.5\text{ MB} (\text{FB}) + 1.0\text{ MB} (\text{State}) = 13.50\text{ MB} \le 15.0\text{ MB}$$

#### 3.4.4 OSD Overlay Compositing Elements
The OSD layer defines four HUD components:
1. **Transport State Glyphs**: $96 \times 96\text{ px}$ indicators (Play `▶`, Pause `❚❚`, Seek `◄◄ / ►►`) decaying exponentially over $600\text{ ms}$:
   $$\alpha(t) = 0.8 \cdot e^{-5t}$$
2. **Monospace Timecode HUD**: Bottom-left corner display `HH:MM:SS / HH:MM:SS` using `syncplay_common::time::MediaTime`.
3. **Sync Health Diagnostic Badge**: Top-right corner HUD displaying live RTT, phase drift, PLL multiplier, and a tri-color status indicator:
   - Green (`#22C55E`): Phase error $|e(t)| < 15\text{ ms}$ (Deadband in-sync).
   - Amber (`#F59E0B`): Phase error $15\text{ ms} \le |e(t)| \le 1500\text{ ms}$ (Active PLL micro-speed slew).
   - Red (`#EF4444`): Phase error $|e(t)| > 1500\text{ ms}$ or packet loss $> 10\%$ (Desynchronized / Hard seek).
4. **Floating In-Player Chat Bubbles**: Semi-transparent rounded pills stacking upward at bottom-left ($60\text{ px}$ above scrubber).

---

## 4. Architectural Data Structures, Traits & Synchronization Primitives

### 4.1 Data Structures Matrix

| Struct / Enum | Crate | Memory Layout / Alignment | Primary Responsibility |
| :--- | :--- | :--- | :--- |
| `AlignedBuffer` | `syncplay-codecs` | 64-byte heap aligned | Owns contiguous memory with +16B SIMD tail padding. |
| `VideoFrameYUV420p` | `syncplay-codecs` | 16-byte row stride, 64-byte planes | Luminance ($Y$) and chroma ($U, V$) planar video representation. |
| `VideoFrameRGBA` | `syncplay-codecs` | 64-byte row stride, Alpha=255 | Packed 32-bit RGBA presentation surface buffer. |
| `AudioFramePCM` | `syncplay-codecs` | Interleaved `f32` in $[-1.0, 1.0]$ | Normalized stereo audio PCM frame with sample rate and PTS. |
| `CacheAligned<T>` | `syncplay-clock` | `#[repr(align(128))]` | Isolates inner atomic variables onto dedicated 128-byte cache lines. |
| `SpscRingBuffer<T, N>`| `syncplay-clock` | Lock-free, power-of-two $N$ | Lock-free circular ring buffer with Acquire-Release ordering. |
| `MasterClock` | `syncplay-clock` | SeqLock atomic snapshot | Monotonic presentation clock with $C^0$ rate re-anchoring. |
| `PresentationReorderBuffer<T>` | `syncplay-clock` | Min-heap, capacity $K=16$ | Orders decode-order frames into presentation timestamp order. |
| `PlayerUiSnapshot` | `syncplay-ui` | 72 bytes, `#[repr(C)]` | Immutable atomic telemetry snapshot read by native UI thread. |
| `TripleBufferSnapshotExchange`| `syncplay-ui` | SPSC triple buffer | Atomic index swapping between Telemetry Aggregator and UI thread. |
| `ScrubberDebouncer` | `syncplay-ui` | $10\text{ Hz}$ rate limiter | Decouples 120 Hz visual drag feedback from 10 Hz seek commands. |
| `BoundedChatOverlay`| `syncplay-ui` | Cap: 5 visible bubbles | Manages 5.0s bubble lifecycle and prevents memory growth. |
| `FontAtlasAlpha8` | `syncplay-ui` | Single-channel `R8_UNORM` | 4.0 MB font texture atlas adhering to 15MB UI budget. |
| `PlayerCommand` | `syncplay-ui` | Lightweight enum | User UI commands dispatched to core coordinator. |

### 4.2 Synchronization Primitives & Thread Topology

```
                       ┌─────────────────────────┐
                       │   Demuxer Thread        │
                       └────────────┬────────────┘
                                    │ SpscRingBuffer<DemuxPacket, 32>
                                    ▼
                       ┌─────────────────────────┐
                       │   Video Decoder Thread  │
                       └────────────┬────────────┘
                                    │ SpscRingBuffer<VideoFrameYUV420p, 16>
                                    ▼
┌────────────────────────────────────────────────────────┐
│             Presentation Thread / Coordinator          │
│                                                        │
│  - PresentationReorderBuffer<VideoFrameYUV420p> (K=16) │
│  - MasterClock (SeqLock, monotonic Instant)            │
│  - ColorSpaceConverter (BT.601 / BT.709 Q8)            │
│  - TelemetryAggregator (Compiles SubsystemMetrics)     │
└───────────┬────────────────────────────────────────────┘
            │
            │ SPSC Triple-Buffer Exchange (PlayerUiSnapshot, 72B)
            ▼
┌────────────────────────────────────────────────────────┐
│                  Native UI / OSD Thread                │
│                                                        │
│  - Polls poll_latest_snapshot() (< 50ns, lock-free)    │
│  - ScrubberDebouncer (10 Hz rate limiter)              │
│  - OSD Compositor (Transport glyphs, Sync badge, Chat) │
│  - Dispatches PlayerCommand via SPSC Command Queue     │
└────────────────────────────────────────────────────────┘
```

---

## 5. Worker Implementation Recommendations

For the Worker subagents assigned to Phase 2 R2 implementation, the following concrete tasks and guidelines are established:

### Task 1: Color Space Converter Optimization (`crates/syncplay-codecs`)
- Refactor `syncplay-codecs/src/color.rs` into a module `src/color/` defining:
  - `mod.rs`: `ColorSpaceConverter` trait and `ColorStandard` enum.
  - `bt601.rs`: `Bt601Converter` implementing the trait.
  - `bt709.rs`: `Bt709Converter` implementing the trait.
- Implement 2-pixel unrolled horizontal loop sharing $(U, V)$ chroma computations.
- Verify zero dynamic memory allocations in `convert_frame()`.
- Add unit tests verifying conversion accuracy for primary colors and extreme gamut bounds.

### Task 2: Presentation Loop Runner (`crates/syncplay-core`)
- Implement `PresentationRunner` in `crates/syncplay-core`:
  - Consumes decoded `VideoFrameYUV420p` from `SpscRingBuffer`.
  - Pushes into `PresentationReorderBuffer<VideoFrameYUV420p>`.
  - Peeks/pops the minimum PTS frame and checks `MasterClock::evaluate_presentation_timing()`.
  - On `Wait(d)`: uses precise sleeping/yielding.
  - On `PresentNow` / `PresentNowLate`: converts to RGBA and submits to display buffer.
  - On `DropLateFrame`: increments late-dropped frame counter in `SubsystemMetrics`.

### Task 3: OSD Visual Rendering Components (`crates/syncplay-ui`)
- Implement OSD rendering helpers:
  - Transport state exponential fade calculator: $\alpha(t) = 0.8 \cdot e^{-5t}$.
  - Monospace timecode string builder leveraging `MediaTime::to_timecode_string()`.
  - Tri-color status dot evaluator:
    - Green: $|e| < 15\text{ ms}$
    - Amber: $15\text{ ms} \le |e| \le 1500\text{ ms}$
    - Red: $|e| > 1500\text{ ms}$ or loss $> 10\%$
  - Wire `PlayerCommand` queue for user keyboard shortcuts (Space, Arrow keys).

---

## 6. Verification & Compliance Matrix

| Intent / Requirement | Verification Command | Expected Target |
| :--- | :--- | :--- |
| Zero External Codecs | `./scripts/audit_dependencies.sh` | Clean exit 0, zero banned media/db libraries |
| Lock-free SPSC & UI RAM | `python3 tests/empirical_challenge/test_ui_renderer.py` | SPSC triple-buffer 0 torn reads, 10Hz scrubber 6 seeks, UI RAM $\le 15\text{MB}$ |
| Q8 Color & Master Clock | `python3 tests/empirical_challenge/test_demux_codecs_clock.py` | 16-byte stride safe, SeqLock 0 torn reads, 24h timescale no overflow |
| Workspace Unit Tests | `cargo test --workspace -- --nocapture` | All unit and integration tests passing |
| Intent Compliance | `python3 scripts/intent_harness.py` | 100.0% Intent Compliance Score across 8 intents |
