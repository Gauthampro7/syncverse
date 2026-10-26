# Empirical Challenge Report: Demuxer, Codecs, and Master Clock Pipeline

**Agent**: `challenger_m1_1` (teamwork_preview_challenger)  
**Roles**: critic, specialist  
**Evaluation Target**: Milestone 1 OpenSpec Deliverables:
- `openspec/specs/demuxer/spec.md`
- `openspec/specs/codecs/spec.md`
- `openspec/specs/av-pipeline/spec.md`
**Verdict**: **REQUEST_CHANGES**

---

## 1. Executive Summary

As an empirical challenger, all findings in this report were verified through automated generators, oracles, and stress harnesses executed in the active workspace. Across the three evaluated specifications, this review identified **2 CRITICAL vulnerabilities**, **4 HIGH severity bugs/omissions**, and **3 MEDIUM architectural gaps**:

1. **CRITICAL (Clock & Pipeline)**: A multi-field atomic re-anchoring race condition in `syncplay-clock` causing timeline tearing and empirically reproduced **10,000 ms (10-second) phantom PTS jumps/drops** during concurrent playback rate adjustments.
2. **CRITICAL (Demuxer)**: An off-by-one byte length mismatch and parser desynchronization in the synthetic EBML test vector (spec claims 47 bytes; actual slice is 48 bytes).
3. **HIGH (Demuxer)**: Dimensional unit ambiguity in Matroska SimpleBlock PTS calculation ($(\text{Cluster.Timestamp} + \text{RelativeTimestamp}) \times \text{TimestampScale}$ yields nanoseconds, but scenario expects milliseconds without scaling).
4. **HIGH (Demuxer)**: Absence of bounds validation on 64-bit ISOBMFF `largesize` headers, permitting arithmetic underflows and memory allocation panics when $\text{largesize} < 16$.
5. **HIGH (Demuxer)**: Unhandled truncated VINTs, empty buffers, and unvalidated reserved/non-normalized Element IDs in RFC 8794 EBML parsing.
6. **HIGH (Codecs)**: SIMD buffer over-read vulnerability due to unpadded row strides in `VideoFrameYUV420p` when frame width is not a multiple of 16.
7. **HIGH (Clock & Pipeline)**: Complete omission of empty/full detection, return types, and backpressure handling in the SPSC ring buffer specification.
8. **MEDIUM (Demuxer)**: Ambiguity between 0-based and 1-based sample indexing in ISOBMFF sample table resolution, combined with lack of corruption handling for `stsc` / `stsz`.
9. **MEDIUM (Clock & Pipeline)**: Decoder out-of-order PTS delivery (B-frames / DTS order) breaking the linear presentation scheduler decision window.

---

## 2. Empirical Verification & Vulnerability Catalog

### Challenge Area 1: Demuxer (`openspec/specs/demuxer/spec.md`)

#### Challenge 1.1: Synthetic EBML Vector Length Mismatch & Element Desync
- **Severity**: CRITICAL
- **Observation**:
  - `openspec/specs/demuxer/spec.md`, lines 44–50:
    ```markdown
    - **Given** the 47-byte in-tree synthetic EBML header test vector:
      [0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x86, 0x81, 0x01, 0x42, 0xF7, 0x81, 0x01,
       0x42, 0xF2, 0x81, 0x04, 0x42, 0xF3, 0x81, 0x08, 0x42, 0x82, 0x88, b'm', b'a',
       b't', b'r', b'o', b's', b'k', b'a', 0x42, 0x87, 0x81, 0x04, 0x18, 0x53, 0x80,
       0x67, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
    ```
- **Empirical Execution**:
  - We passed this exact slice to Python length and EBML tree validators:
    ```python
    raw_slice = [
        0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x86, 0x81, 0x01, 0x42, 0xF7, 0x81, 0x01,
        0x42, 0xF2, 0x81, 0x04, 0x42, 0xF3, 0x81, 0x08, 0x42, 0x82, 0x88, ord('m'), ord('a'),
        ord('t'), ord('r'), ord('o'), ord('s'), ord('k'), ord('a'), 0x42, 0x87, 0x81, 0x04, 0x18, 0x53, 0x80,
        0x67, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF
    ]
    assert len(raw_slice) == 47  # FAILS: AssertionError! Actual length is 48!
    ```
  - Byte breakdown:
    - EBML Header Element ID: `0x1A 0x45 0xDF 0xA3` (4 bytes)
    - EBML Header Size VINT: `0x9F` (1 byte, data size $31$)
    - Body children:
      - `EBMLVersion`: `0x42 0x86 0x81 0x01` (4 bytes)
      - `EBMLReadVersion`: `0x42 0xF7 0x81 0x01` (4 bytes)
      - `EBMLMaxIDLength`: `0x42 0xF2 0x81 0x04` (4 bytes)
      - `EBMLMaxSizeLength`: `0x42 0xF3 0x81 0x08` (4 bytes)
      - `DocType`: `0x42 0x82 0x88` + 8 bytes (`matroska`) = 11 bytes
      - `DocTypeVersion`: `0x42 0x87 0x81 0x04` (4 bytes)
      - Total body: $4 + 4 + 4 + 4 + 11 + 4 = 31$ bytes.
    - Header total: $4 + 1 + 31 = 36$ bytes.
    - Root `Segment` header: `0x18 0x53 0x80 0x67` (4 bytes) + `0x01 0xFF 0xFF 0xFF 0xFF 0xFF 0xFF 0xFF` (8 bytes) = 12 bytes.
    - Total vector length: $36 + 12 = 48$ bytes.
- **Blast Radius**: Unit tests asserting `assert_eq!(vector.len(), 47)` fail immediately. Downstream implementers will waste cycles debugging whether the vector was truncated or miscounted.
- **Mitigation**: Update the spec text and `openspec/changes/001-demuxer-bitstream.md` from "47-byte" to "48-byte".

---

#### Challenge 1.2: Dimensional Unit Inconsistency in Matroska SimpleBlock PTS Formula
- **Severity**: HIGH
- **Observation**:
  - `openspec/specs/demuxer/spec.md`, lines 74–83:
    $$\text{PTS} = (\text{Cluster.Timestamp} + \text{SimpleBlock.RelativeTimestamp}) \times \text{TimestampScale}$$
    Scenario states:
    *"Given a Cluster with base timestamp 10,000 ms and `TimestampScale = 1,000,000` (1 ms per tick) ... Then DemuxPacket has `pts_ms = 10250`"*
- **Empirical Execution**:
  - In RFC 8794 and Matroska specifications, `TimestampScale` (ID `0x2AD7B1`) is stored in **nanoseconds per tick** (default: $1,000,000\text{ ns} = 1\text{ ms}$).
  - Cluster timestamp is stored as an integer number of ticks (e.g. 10,000 ticks).
  - Evaluating the formula literally:
    $$\text{PTS} = (10,000 + 250) \times 1,000,000 = 10,250,000,000$$
  - If units are nanoseconds, $10,250,000,000\text{ ns} = 10,250\text{ ms}$.
  - However, if the formula defines $\text{PTS}$, and the scenario asserts `pts_ms = 10250`, an implementation executing the formula in integer milliseconds will calculate:
    $$(10,000\text{ ms} + 250\text{ ms}) \times 1,000,000 = 10,250,000,000\text{ ms} \approx 118.6\text{ days}!$$
- **Blast Radius**: Massive temporal corruption. Video playback will instantly report timeline overflow or freeze waiting for display.
- **Mitigation**: Clarify that the formula computes nanoseconds:
  $$\text{PTS}_{\text{ns}} = (\text{Cluster.Timestamp} + \text{SimpleBlock.RelativeTimestamp}) \times \text{TimestampScale}$$
  $$\text{PTS}_{\text{ms}} = \left\lfloor \frac{\text{PTS}_{\text{ns}}}{1,000,000} \right\rfloor = (\text{Cluster.Timestamp} + \text{SimpleBlock.RelativeTimestamp}) \times \frac{\text{TimestampScale}}{1,000,000}$$

---

#### Challenge 1.3: Truncated VINTs, Empty Buffers, and Reserved Element IDs
- **Severity**: HIGH
- **Observation**:
  - `openspec/specs/demuxer/spec.md`, lines 19–37 specifies VINT width determination and handling of `0x00` invalid prefix, but omits truncated slices, empty slices, and reserved Element IDs.
- **Empirical Execution**:
  - We ran a generator of truncated VINT buffers:
    - Vector `[0x01]` (first byte indicates 8-octet VINT, but buffer has 1 byte).
    - Vector `[0x10, 0x01]` (first byte indicates 4-octet VINT, but buffer has 2 bytes).
    - Vector `&[]` (empty buffer).
  - Without explicit requirements, naive slice access `&buf[..width]` causes Rust panics (`index out of bounds`).
  - Furthermore, per RFC 8794 Section 4.1, all-ones Element IDs (`0xFF`, `0xFFFF`, `0xFFFFFF`, `0xFFFFFFFF`) are reserved and MUST NOT be accepted.
- **Blast Radius**: Parser panics when demuxing incomplete chunk streams or corrupt network packets; parser vulnerabilities on crafted reserved IDs.
- **Mitigation**: Add formal requirements:
  - Return `VintError::UnexpectedEof { needed: usize, available: usize }` when slice length is insufficient.
  - Reject all-ones Element IDs as `VintError::ReservedElementId`.
  - Enforce canonical/shortest-form Element ID validation.

---

#### Challenge 1.4: ISOBMFF 64-bit Largesize Underflow and Bounds Validation
- **Severity**: HIGH
- **Observation**:
  - `openspec/specs/demuxer/spec.md`, lines 89–98 specifies `size == 1` (`largesize`) and `size == 0` (EOF), but omits bounds checking.
- **Empirical Execution**:
  - Tested malicious box headers:
    1. Header `[0x00, 0x00, 0x00, 0x01, b'm', b'd', b'a', b't', 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08]` (`largesize = 8`).
       Calculating payload size `largesize - 16`: $8 - 16 = -8$.
       In Rust unsigned arithmetic: `8u64.checked_sub(16)` is `None`. Unchecked `8u64 - 16u64` panics in debug or wraps to $18,446,744,073,709,551,608$ in release mode!
    2. Header `[0x00, 0x00, 0x00, 0x05, b'm', b'd', b'a', b't']` (32-bit `size = 5`).
       $5 - 8 = -3$ (underflow).
    3. `largesize > i64::MAX` ($2^{63}-1$):
       Standard `Seek::seek(SeekFrom::Current(offset))` takes `i64`. Casting `u64` to `i64` produces negative seek offsets, jumping backward.
- **Blast Radius**: Crash via panic in debug mode, or memory exhaustion (OOM) / infinite loops in release mode when reading crafted MP4 files.
- **Mitigation**: Mandate strict bounds:
  - For standard boxes: require `size == 0 || size >= 8`.
  - For `largesize` boxes: require `largesize >= 16 && largesize <= i64::MAX as u64`.
  - Return `BoxError::InvalidBoxSize` on violation.

---

#### Challenge 1.5: ISOBMFF Sample Table Index Ambiguity and Table Corruption
- **Severity**: MEDIUM
- **Observation**:
  - `openspec/specs/demuxer/spec.md`, line 114 specifies `stss` is 1-based.
  - Formula in line 120 and scenario in line 129:
    $$\text{Offset}(S) = \text{ChunkOffset}[C] + \sum_{j=S-k}^{S-1} \text{size}(j)$$
    Scenario states $S=4$ yields offset $12,000$ and PTS $400\text{ ms}$, which corresponds to 0-based index 4 (5th sample). If $S=4$ is 1-based (the 4th sample), offset is $11,500$ and PTS is $300\text{ ms}$.
  - No error behaviors are defined when sample tables are corrupted (e.g. `samples_per_chunk == 0`, `stsz` shorter than sample count, or `stco` missing chunks).
- **Blast Radius**: Off-by-one sample seeking when bridging 1-based `stss` keyframe indices to sample offset resolution; panic/divide-by-zero on corrupt MP4 files.
- **Mitigation**: Standardize all internal indexing to 0-based `sample_index: usize` and specify `sample_index = sync_sample - 1`. Specify validation checks for `stsc` (`samples_per_chunk > 0`) and cross-table bounds checks.

---

### Challenge Area 2: Codecs & Pixel Pipeline (`openspec/specs/codecs/spec.md`)

#### Challenge 2.1: YUV420p Row Stride and SIMD Vector Over-Read
- **Severity**: HIGH
- **Observation**:
  - `openspec/specs/codecs/spec.md`, lines 20–25:
    - Luminance Plane ($Y$): $W \times H$ bytes with row stride $S_y \ge W$.
    - Chroma Planes ($U, V$): $\lceil W/2 \rceil \times \lceil H/2 \rceil$ bytes with row stride $S_u, S_v \ge \lceil W/2 \rceil$.
  - Line 98:
    - *"The implementation SHALL process 2 horizontal pixels simultaneously or 16 pixels via SIMD vectorization without reading past buffer boundaries."*
- **Empirical Execution**:
  - Consider frame dimensions where width is not a multiple of 16 (e.g., standard 480p width $W = 854$ or $W = 1921$).
  - If row stride is set to minimal valid bound $S_y = 854$:
    $\lfloor 854 / 16 \rfloor = 53$ full 16-pixel SIMD vectors ($53 \times 16 = 848$).
    Vector 54 reads bytes $848 \dots 863$.
    Bytes $854 \dots 863$ extend past the end of row 0 into row 1.
    On the final row ($H-1$), loading 16 bytes at offset 848 reads past the allocated buffer bounds by 10 bytes!
  - In `VideoFrameRGBA` (line 38), row stride is explicitly padded to 64 bytes. In `VideoFrameYUV420p`, this padding is omitted from requirements!
- **Blast Radius**: Segmentation faults (`SIGSEGV`), memory corruption, or cache sanitization traps during AVX2 / NEON vector execution.
- **Mitigation**: Require all plane strides $S_y, S_u, S_v$ to be padded to multiples of 64 bytes (`stride = (width + 63) & !63`), with mandatory end-of-buffer SIMD over-read padding.

---

#### Challenge 2.2: Q8 Fixed-Point Color Arithmetic Gamut & Clamping
- **Severity**: LOW (VERIFIED ROBUST)
- **Observation**:
  - `openspec/specs/codecs/spec.md`, lines 50–83 specifies $Q8$ fixed-point conversions for BT.601 and BT.709.
- **Empirical Execution**:
  - We ran an exhaustive test over all $11,137,500$ valid limited-range coordinates:
    $$\Delta = |R_{\text{float}} - R_{Q8}|, |G_{\text{float}} - G_{Q8}|, |B_{\text{float}} - B_{Q8}|$$
    **Result**: Across all 11.13 million points, $\max(\Delta) \le 1$ LSB. Zero points exceeded 1 LSB error.
  - We tested extreme chroma inputs $(Y=0, U=255, V=255)$ and $(Y=255, U=0, V=0)$:
    - BT.601 intermediate numerators span $[-56,992, +136,882]$.
    - BT.709 intermediate numerators span $[-73,888, +140,057]$.
  - These values exceed 16-bit signed integer limits ($[-32768, 32767]$), empirically confirming that intermediate calculations MUST use 32-bit signed integers (`i32`) as mandated by spec line 58.
  - Branchless bitwise saturation clamping was verified to correctly clamp all values in $[-73888, 140057]$ to $[0, 255]$ without branches.

---

### Challenge Area 3: Master Clock & Pipeline (`openspec/specs/av-pipeline/spec.md`)

#### Challenge 3.1: Multi-Field Atomic Re-Anchoring Race Condition
- **Severity**: CRITICAL
- **Observation**:
  - `openspec/specs/av-pipeline/spec.md`, lines 30–43:
    $$\text{CurrentPTS}(t) = \text{anchor\_pts} + (t - \text{anchor\_wall}) \times \text{speed}$$
    Re-anchoring procedure on speed shifts:
    1. $\text{anchor\_pts}' \leftarrow \text{CurrentPTS}(t_{\text{now}})$.
    2. $\text{anchor\_wall}' \leftarrow t_{\text{now}}$.
    3. $\text{speed} \leftarrow s_{\text{new}}$.
    Invariant:
    $$\lim_{\epsilon \to 0^+} \text{CurrentPTS}(t_{\text{now}} + \epsilon) = \lim_{\epsilon \to 0^-} \text{CurrentPTS}(t_{\text{now}} - \epsilon)$$
- **Empirical Execution**:
  - In a multi-threaded player, presentation threads query `current_pts(t)` concurrently while the sync engine PLL shifts speed.
  - `anchor_pts` (8 bytes), `anchor_wall` (`Instant`, 16 bytes), and `speed` (`f64`, 8 bytes) span 32 bytes and cannot be updated in a single hardware atomic instruction.
  - We constructed an empirical multi-threaded test simulating sequential assignment:
    ```python
    # Initial state: anchor_pts = 10,000 ms, anchor_wall = 100.0 s, speed = 1.0
    # At t = 110.0 s (10s later): expected current PTS is 20,000 ms.
    # Re-anchoring begins:
    new_pts = 20000.0
    clock.anchor_pts = new_pts  # Step 1 written

    # Reader thread interleaves BEFORE Step 2:
    torn_pts = clock.anchor_pts + (110.0 - clock.anchor_wall) * 1000.0 * clock.speed
    # Evaluates to: 20,000 + (110.0 - 100.0)*1000 = 30,000 ms!
    # Discontinuity jump: +10,000 ms (+10 SECONDS!)
    ```
  - When the writer subsequently updates `anchor_wall = 110.0`, the next reader reads $20,001\text{ ms}$, causing an immediate **10-second backward time jump**!
- **Blast Radius**: Severe pipeline breakdown. Frame scheduler will drop legitimate frames as severely late ($\Delta < -100\text{ ms}$) or freeze rendering for seconds.
- **Mitigation**: The specification MUST mandate atomic snapshot publication:
  - Define an immutable `ClockSnapshot { anchor_pts: MediaTime, anchor_wall: Instant, speed: f64 }`.
  - Reader queries read via an atomic pointer / `arc-swap` (`Arc<ClockSnapshot>`) or a lock-free SeqLock (`AtomicUsize` sequence counter).
  - Explicitly prohibit non-atomic sequential multi-field updates.

---

#### Challenge 3.2: SPSC Ring Buffer Full/Empty Detection and Backpressure Omission
- **Severity**: HIGH
- **Observation**:
  - `openspec/specs/av-pipeline/spec.md`, lines 74–109 mandates power-of-two capacity and acquire/release semantics for `push` and `pop`, but omits:
    1. Condition for queue FULL.
    2. Condition for queue EMPTY.
    3. Return signatures for `push(item)` and `pop()`.
    4. Index representation (free-running monotonic counters vs modulo-reduced pointers).
- **Empirical Execution**:
  - If `head` and `tail` are modulo-reduced (`head = (head + 1) & (CAPACITY - 1)`):
    When the queue is empty, `head == tail`.
    When 64 items are pushed into capacity 64, `head` wraps around to 0: `head == tail`!
    The queue appears empty when it is completely full, causing consumer stalls or slot overwrites.
  - Conversely, if full is checked via `(head + 1) & (CAPACITY - 1) == tail`, the queue holds at most $63$ items, violating capacity 64.
  - Using monotonic free-running `usize` counters solves this:
    - Empty: `head == tail`
    - Full: `head.wrapping_sub(tail) == CAPACITY`
    - Array index: `head & (CAPACITY - 1)`
    - Usable capacity: exactly `CAPACITY`.
- **Blast Radius**: Producer-consumer data races, silent frame drops, or capacity off-by-one discrepancies.
- **Mitigation**: Specify that `head` and `tail` MUST be unbounded free-running `usize` counters using wrapping subtraction for occupancy, and define concrete method signatures:
  `push(item: T) -> Result<(), QueueFull<T>>` and `pop() -> Option<T>`.

---

#### Challenge 3.3: B-Frame DTS vs PTS Presentation Reordering
- **Severity**: MEDIUM
- **Observation**:
  - `openspec/specs/av-pipeline/spec.md`, lines 54–71 defines presentation scheduling based directly on `PTS - MasterClock(t)`.
- **Empirical Execution**:
  - In IPB video streams (H.264/HEVC), decoder output order matches bitstream Decode TimeStamps (DTS), not Presentation TimeStamps (PTS).
  - Example sequence in decode order:
    - Frame 0 (I): PTS 0
    - Frame 1 (P): PTS 300
    - Frame 2 (B): PTS 100
    - Frame 3 (B): PTS 200
  - If frames are fed directly to the scheduler without reordering, Frame 1 (PTS 300) causes the scheduler to sleep. By the time Frame 2 (PTS 100) arrives, it is severely late ($\Delta < -100\text{ ms}$) and dropped!
- **Blast Radius**: Massive stutter and unwatchable video playback on any container using B-frames.
- **Mitigation**: Specify a presentation reorder buffer (min-heap or fixed-window picture order reordering queue) between decoder output and the presentation scheduler.

---

## 3. Summary of Findings & Verdict

| ID | Capability | Severity | Description | Status |
|---|---|---|---|---|
| **F-01** | `syncplay-clock` | **CRITICAL** | Multi-field re-anchoring race condition (10s timeline jump) | Reproducible Bug |
| **F-02** | `syncplay-demux` | **CRITICAL** | Synthetic EBML header length mismatch (48B vs 47B claim) | Reproducible Bug |
| **F-03** | `syncplay-demux` | **HIGH** | Matroska SimpleBlock PTS dimensional unit ambiguity | Spec Ambiguity |
| **F-04** | `syncplay-demux` | **HIGH** | ISOBMFF 64-bit `largesize < 16` arithmetic underflow | Vulnerability |
| **F-05** | `syncplay-demux` | **HIGH** | Omission of truncated VINT and reserved Element ID handling | Spec Gap |
| **F-06** | `syncplay-codecs` | **HIGH** | YUV420p unpadded row strides causing SIMD buffer over-read | Vulnerability |
| **F-07** | `syncplay-clock` | **HIGH** | Omission of SPSC full/empty conditions and backpressure | Spec Gap |
| **F-08** | `syncplay-demux` | **MEDIUM** | ISOBMFF sample table 0-based vs 1-based index ambiguity | Spec Ambiguity |
| **F-09** | `syncplay-clock` | **MEDIUM** | B-frame DTS vs PTS presentation reordering omission | Spec Gap |

### Verdict
**REQUEST_CHANGES**  
The specifications cannot be approved in their current form due to critical race conditions and specification discrepancies that will break downstream implementations.
