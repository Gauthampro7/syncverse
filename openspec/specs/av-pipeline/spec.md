# Capability Specification: Master A/V Clock & Pipeline (`syncverse-clock`)

**Component**: `syncverse-clock`  
**Standard Compliance**: POSIX `CLOCK_MONOTONIC`, C11/Rust Atomic Memory Model  
**Status**: Authoritative OpenSpec Specification  

---

## 1. Overview & Architectural Role

The `syncverse-clock` crate governs media presentation timing, continuous PTS progression across variable playback speeds, and thread-decoupling via lock-free ring buffers. Presentation pacing requires an absolute monotonic reference that is immune to system wall-clock discontinuities (e.g., NTP adjustments or daylight saving shifts).

Furthermore, media playback requires transferring decoded video and audio frames across threads (Demuxer $\to$ Decoder $\to$ Presentation) without thread blocking, mutex locks, or priority inversions. This is achieved via a 128-byte cache-line aligned Single-Producer Single-Consumer (SPSC) lock-free ring buffer that eliminates CPU core false sharing.

---

## 2. Requirements & Scenarios

### Requirement: Master Monotonic Time Base
The master clock SHALL maintain presentation time using a monotonic hardware clock source (`std::time::Instant` backed by `clock_gettime(CLOCK_MONOTONIC)` on Linux). The master clock MUST NOT query or depend on `CLOCK_REALTIME` or system wall-clock time for frame pacing or drift evaluation.

#### Scenario: Verify Monotonic Clock Invariant Across System Steps
- **Given** an active media playback session with monotonic anchor $t_0$
- **When** the operating system wall time is artificially stepped backward or forward by 1 hour
- **Then** the master clock elapsed time continues advancing monotonically without jumping, reversing, or stalling.

---

### Requirement: Continuous PTS Progression Across Speed Adjustments
The presentation timestamp at wall instant $t$ SHALL be calculated according to:
$$\text{CurrentPTS}(t) = \begin{cases}
\text{anchor\_pts} + (t - \text{anchor\_wall}) \times \text{speed}, & \text{if Playing} \\
\text{anchor\_pts}, & \text{if Paused}
\end{cases}$$
When the playback speed shifts from $s_{\text{old}}$ to $s_{\text{new}}$ at instant $t_{\text{now}}$ (e.g. commanded by the sync engine's PLL drift controller):
1. The new anchor PTS MUST be computed as $\text{anchor\_pts}' \leftarrow \text{CurrentPTS}(t_{\text{now}})$.
2. The clock state ($\text{anchor\_pts}'$, $\text{anchor\_wall}' = t_{\text{now}}$, $\text{speed} = s_{\text{new}}$, and playback state) MUST be published as an **atomic snapshot** using a lock-free Sequence Lock (`SeqLock`) or atomic 128-bit snapshot (`AtomicU128`).
3. The implementation MUST NOT perform disjoint, non-atomic multi-field sequential writes to `anchor_pts`, `anchor_wall`, and `speed`.

The clock implementation MUST guarantee $C^0$ continuity across rate transitions:
$$\lim_{\epsilon \to 0^+} \text{CurrentPTS}(t_{\text{now}} + \epsilon) = \lim_{\epsilon \to 0^-} \text{CurrentPTS}(t_{\text{now}} - \epsilon)$$
with zero discontinuous jumps, frame drops, or timeline tearing observable by concurrent reader threads.

#### Scenario: Shift Playback Speed from 1.0x to 1.04x Under Continuous Playback
- **Given** an active playback session playing at $1.0\times$ speed with anchor PTS $10,000\text{ ms}$ at anchor instant $t_0$
- **When** $500\text{ ms}$ of wall time elapses, bringing current PTS to $10,500\text{ ms}$
- **And** the sync engine commands a micro-speed shift to $1.04\times$
- **Then** the clock re-anchors to PTS $10,500\text{ ms}$ at instant $t_0 + 500\text{ ms}$
- **And** immediately after the transition, the computed PTS remains exactly $10,500\text{ ms}$ ($\Delta = 0\text{ ms}$)
- **And** after an additional $1,000\text{ ms}$ of wall time, the computed PTS is $10,500 + (1000 \times 1.04) = 11,540\text{ ms}$.

---

### Requirement: Frame Presentation Decision Window
The presentation scheduler SHALL evaluate the timing delta $\Delta$ for every candidate frame with timestamp $\text{PTS}_{\text{frame}}$ against current master clock time:
$$\Delta = \text{PTS}_{\text{frame}} - \text{MasterClock}(t_{\text{now}})$$
The scheduler MUST execute one of four deterministic actions:
1. **Early ($\Delta > +10\text{ ms}$)**: Yield/sleep until $\Delta \le +10\text{ ms}$ before rendering.
2. **On-Time ($-15\text{ ms} \le \Delta \le +10\text{ ms}$)**: Submit immediately to the display compositor / Wayland surface.
3. **Slightly Late ($-100\text{ ms} < \Delta < -15\text{ ms}$)**: Render immediately without waiting, recording a late-frame metric.
4. **Severely Late ($\Delta \le -100\text{ ms}$)**: **Drop the frame immediately** without RGB conversion or rendering to clear pipeline backpressure.

Between decoder frame emission and the presentation decision window, the pipeline MUST maintain a bounded presentation reorder buffer (min-heap or fixed-capacity priority queue ordered by ascending PTS) of capacity $K$ (nominally $K = 16$ frames). Decoded frames emitted in decode order (DTS) MUST be inserted into the reorder buffer, and the frame with the minimum PTS MUST be yielded to the presentation scheduler once the reorder window is populated or upon container stream drain.

#### Scenario: Schedule Frames Under Varying Timing Deltas
- **Given** current master clock time $20,000\text{ ms}$
- **When** evaluating Frame A with PTS $20,050\text{ ms}$ ($\Delta = +50\text{ ms}$)
- **Then** action is `PresentationAction::Wait(40ms)`
- **When** evaluating Frame B with PTS $19,995\text{ ms}$ ($\Delta = -5\text{ ms}$)
- **Then** action is `PresentationAction::PresentNow`
- **When** evaluating Frame C with PTS $19,850\text{ ms}$ ($\Delta = -150\text{ ms}$)
- **Then** action is `PresentationAction::DropLateFrame`.

---

### Requirement: Lock-Free Single-Producer Single-Consumer (SPSC) Ring Buffer
Inter-thread communication across pipeline stages MUST use a lock-free Single-Producer Single-Consumer (SPSC) circular ring buffer (`SpscRingBuffer<T, CAPACITY>`). The buffer CAPACITY MUST be a power of two ($2^N$) to permit fast modulo wrapping via bitwise AND (`slot = index & (CAPACITY - 1)`).

#### Scenario: Enforce Power-of-Two Capacity Invariant
- **Given** an attempt to construct `SpscRingBuffer` with capacity $100$
- **When** the constructor validates capacity
- **Then** construction fails with an assertion or error requiring a power of two
- **Given** a valid capacity $128$
- **When** constructing `SpscRingBuffer`
- **Then** construction succeeds.

---

### Requirement: False-Sharing Elimination via 128-Byte Cache Alignment
The producer's `head` atomic index and the consumer's `tail` atomic index MUST be placed on separate 128-byte cache lines (`#[repr(align(128))]`). This prevents cache-line ping-pong between producer and consumer CPU cores on modern x86_64 / aarch64 processors.

#### Scenario: Verify Cache-Line Separation of Head and Tail
- **Given** an instantiated `SpscRingBuffer`
- **When** inspecting memory offsets of `head` and `tail`
- **Then** the byte offset difference between `head` and `tail` is $\ge 128\text{ bytes}$
- **And** updates to `head` by the producer thread do not invalidate the CPU cache line holding `tail` on the consumer core.

---

### Requirement: SPSC Monotonic Indexing, Full/Empty Conditions, and Signatures
The `SpscRingBuffer<T, CAPACITY>` implementation MUST provide the following explicit method signatures:
```rust
pub fn try_push(&self, item: T) -> Result<(), BufferFullError<T>>;
pub fn try_pop(&self) -> Option<T>;
```
The implementation MUST maintain producer `head` and consumer `tail` as unbounded, free-running monotonic `AtomicUsize` counters:
1. **Buffer Empty**: The queue is empty when `head.load(Ordering::Acquire) == tail.load(Ordering::Relaxed)`. In this state, `try_pop()` MUST return `None`.
2. **Buffer Full**: The queue is full when `head.load(Ordering::Relaxed).wrapping_sub(tail.load(Ordering::Acquire)) == CAPACITY`. In this state, `try_push(item)` MUST return `Err(BufferFullError(item))`.
3. **Slot Mapping**: Buffer slot index MUST be derived exclusively via bitwise masking: `slot_index = index & (CAPACITY - 1)`.
4. **Memory Ordering**:
   - `try_push`: Loads `tail` with `Ordering::Acquire`. Writes item. Stores incremented `head` with `Ordering::Release`.
   - `try_pop`: Loads `head` with `Ordering::Acquire`. Reads item. Stores incremented `tail` with `Ordering::Release`.
   - `Ordering::SeqCst` MUST NOT be used.

#### Scenario: Concurrent Producer-Consumer Verification
- **Given** an `SpscRingBuffer` of capacity 64 running across two dedicated OS threads
- **When** the producer pushes 1,000,000 sequentially incrementing integers and the consumer pops them
- **Then** exactly 1,000,000 integers are received in strictly increasing order
- **And** zero data corruption, zero lost items, and zero deadlocks occur.
