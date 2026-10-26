# Remediation Strategy & Technical Analysis: Milestone 1 OpenSpec Corrections

**Author**: `explorer_m1_fix_1` (teamwork_preview_explorer)  
**Roles**: investigation, synthesis  
**Target Milestone**: Milestone 1 Fix (`syncplay-demux`, `syncplay-codecs`, `syncplay-clock`)  
**Working Directory**: `/home/gautham/syncplay-alt/.agents/teamwork/explorer_m1_fix_1/`  
**Input Sources**:
- Challenger 1 Challenge Report (`.agents/teamwork/challenger_m1_1/challenge_report.md`)
- Challenger 1 Handoff Report (`.agents/teamwork/challenger_m1_1/handoff.md`)
- Authoritative Specifications (`openspec/specs/demuxer/spec.md`, `codecs/spec.md`, `av-pipeline/spec.md`)
- Change Proposals (`openspec/changes/001-demuxer-bitstream.md`, `002-codecs-presentation.md`, `003-clock-pipeline.md`)

---

## 1. Executive Summary

Empirical review of Challenger 1's findings across the demuxer, codecs, and master clock pipeline specifications confirms **6 primary vulnerabilities and ambiguities** that directly threaten implementation safety, temporal accuracy, and memory stability:

1. **Multi-Field Atomic Race Condition (`syncplay-clock`)**: Disjoint updates to `anchor_pts`, `anchor_wall`, and `speed` during PLL rate shifts permit concurrent readers to observe torn intermediate states, empirically producing **10,000 ms (10-second) phantom timeline jumps**.
2. **Synthetic EBML Test Vector Length Mismatch (`syncplay-demux`)**: The raw slice in the scenario contains **48 bytes**, but the spec text and change proposal label it as "47-byte", causing immediate unit test assertion failures.
3. **SimpleBlock PTS Timescale Conversion (`syncplay-demux`)**: The Matroska PTS equation $(\text{Cluster.Timestamp} + \text{Block.RelativeTimestamp}) \times \text{TimestampScale}$ yields **nanoseconds**, but the scenario asserts output in **milliseconds** without dividing by $1,000,000$, risking a $1,000,000\times$ temporal magnification error.
4. **ISOBMFF 64-bit `largesize` Underflow (`syncplay-demux`)**: Absence of bounds validation allows boxes with `size == 1` and $\text{largesize} < 16$ or $size < 8$ (except 0) to trigger unsigned integer underflows and denial of service.
5. **YUV420p Planar Row Stride Padding (`syncplay-codecs`)**: Specifying row stride as merely $S_y \ge W$ without 16-byte alignment enables 16-pixel SIMD AVX2/NEON vector loads to over-read scanlines and buffer boundaries on non-multiple-of-16 widths.
6. **SPSC Ring Buffer Ambiguity & Signatures (`syncplay-clock`)**: The SPSC circular buffer specification omits full/empty detection criteria, monotonic index conventions, backpressure, and concrete method signatures (`try_push` / `try_pop`), risking full/empty pointer aliasing ($64 = 0$).

This document provides a line-by-line, drop-in remediation strategy for the implementation worker.

---

## 2. Issue Mapping & Remediation Matrix

| ID | Issue | Target Specification | Target Change Proposal | Severity | Proposed Fix |
|---|---|---|---|---|---|
| **R-01** | Multi-field atomic re-anchoring race | `openspec/specs/av-pipeline/spec.md` (lines 29–52) | `openspec/changes/003-clock-pipeline.md` (lines 43–48) | **CRITICAL** | Mandate atomic state snapshot via lock-free `SeqLock` or 128-bit atomic `AtomicU128` |
| **R-02** | Synthetic EBML header length mismatch | `openspec/specs/demuxer/spec.md` (lines 43–55) | `openspec/changes/001-demuxer-bitstream.md` (lines 42, 60) | **CRITICAL** | Reconcile label to "48-byte" and document exact 36B + 12B byte layout |
| **R-03** | SimpleBlock PTS timescale conversion | `openspec/specs/demuxer/spec.md` (lines 72–87) | `openspec/changes/001-demuxer-bitstream.md` (lines 54–56) | **HIGH** | Formulate integer arithmetic: $\text{PTS}_{\text{ms}} = \lfloor \text{PTS}_{\text{ns}} / 1\,000\,000 \rfloor$ |
| **R-04** | ISOBMFF `largesize < 16` underflow | `openspec/specs/demuxer/spec.md` (lines 89–106) | `openspec/changes/001-demuxer-bitstream.md` (line 57) | **HIGH** | Validate `if size == 1 && largesize < 16 => Err(MalformedBox)` and `size < 8 && size != 0` |
| **R-05** | YUV420p unpadded stride SIMD over-read | `openspec/specs/codecs/spec.md` (lines 19–35, 94–106) | `openspec/changes/002-codecs-presentation.md` (lines 36, 47) | **HIGH** | Mandate 16-byte aligned row stride `(width + 15) & !15` and $+16$B SIMD tail padding |
| **R-06** | SPSC ring buffer full/empty & signatures | `openspec/specs/av-pipeline/spec.md` (lines 74–116) | `openspec/changes/003-clock-pipeline.md` (lines 55–59) | **HIGH** | Specify `try_push` / `try_pop`, wrapping counters, and full/empty conditions |
| **R-07** | Truncated VINTs & Reserved IDs | `openspec/specs/demuxer/spec.md` (lines 19–37) | `openspec/changes/001-demuxer-bitstream.md` (lines 48–51) | **HIGH** | Add `UnexpectedEof` and `ReservedElementId` error specifications |
| **R-08** | ISOBMFF 0-based vs 1-based index clarity | `openspec/specs/demuxer/spec.md` (lines 109–134) | `openspec/changes/001-demuxer-bitstream.md` (line 58) | **MEDIUM** | Standardize internal 0-based indexing; map 1-based `stss` via $S = sync - 1$; validate `samples_per_chunk > 0` |
| **R-09** | B-Frame DTS vs PTS reordering | `openspec/specs/av-pipeline/spec.md` (lines 54–72) | `openspec/changes/003-clock-pipeline.md` (lines 49–54) | **MEDIUM** | Specify presentation reordering queue between decoder and presentation scheduler |

---

## 3. Concrete Line-by-Line Remediation Strategy

---

### Remediation 1: Multi-Field Atomic Re-Anchoring Race Condition in Clock

#### Problem Diagnosis
In `openspec/specs/av-pipeline/spec.md` (lines 35–39), the spec prescribes:
```text
1. The new anchor PTS MUST be computed as anchor_pts' <- CurrentPTS(t_now).
2. The new anchor wall instant MUST be re-anchored: anchor_wall' <- t_now.
3. The speed MUST be updated: speed <- s_new.
```
Because `anchor_pts` (8 bytes), `anchor_wall` (`Instant` / monotonic timestamp, 8–16 bytes), and `speed` (`f64`, 8 bytes) reside in separate memory locations, non-atomic sequential stores allow reader threads (such as the presentation scheduler calling `current_pts(t)`) to interleave between steps 1 and 2:
$$\text{TornPTS} = \text{anchor\_pts}' + (t_{\text{now}} - \text{anchor\_wall}_{\text{old}}) \times \text{speed}_{\text{old}}$$
At $t_{\text{now}} - \text{anchor\_wall}_{\text{old}} = 10.0\text{ s}$, this produces a **10,000 ms phantom forward jump**, immediately followed by a backward jump once `anchor_wall` is updated.

#### Architectural Specification: Lock-Free State Snapshot Protocol
The master clock must encapsulate all state fields into an atomic snapshot guaranteeing that `anchor_instant`, `anchor_pts`, `speed`, and `state` cannot be read across disjoint atomic boundaries.

##### Option A: Lock-Free SeqLock (`SeqLock<ClockSnapshot>`) [Recommended for Zero Overhead & Portability]
```rust
#[derive(Copy, Clone, Debug)]
pub struct ClockSnapshot {
    pub anchor_pts: i64,          // Media presentation time in milliseconds (or MediaTime)
    pub anchor_instant_ns: u64,   // Monotonic hardware time in nanoseconds since anchor base
    pub speed: f64,               // Playback rate multiplier (e.g. 1.04)
    pub paused: bool,             // Playback state flag
}

pub struct MasterClock {
    base_instant: std::time::Instant,
    sequence: std::sync::atomic::AtomicUsize,
    snapshot: std::cell::UnsafeCell<ClockSnapshot>,
}

// Single-writer thread (Sync Engine / Transport):
impl MasterClock {
    pub fn update_anchor(&self, new_pts: i64, instant: std::time::Instant, speed: f64, paused: bool) {
        let elapsed_ns = instant.duration_since(self.base_instant).as_nanos() as u64;
        let new_snapshot = ClockSnapshot {
            anchor_pts: new_pts,
            anchor_instant_ns: elapsed_ns,
            speed,
            paused,
        };
        // 1. Begin write: increment sequence to ODD number with Release
        let seq = self.sequence.load(std::sync::atomic::Ordering::Relaxed);
        self.sequence.store(seq.wrapping_add(1), std::sync::atomic::Ordering::Release);
        std::sync::atomic::fence(std::sync::atomic::Ordering::Release);

        // 2. Write snapshot data into slot
        unsafe { *self.snapshot.get() = new_snapshot; }

        // 3. End write: increment sequence to EVEN number with Release
        self.sequence.store(seq.wrapping_add(2), std::sync::atomic::Ordering::Release);
    }

    // Concurrent reader threads (Presentation Scheduler / UI):
    pub fn read_snapshot(&self) -> ClockSnapshot {
        loop {
            // 1. Load sequence counter before reading
            let s1 = self.sequence.load(std::sync::atomic::Ordering::Acquire);
            if s1 & 1 != 0 {
                // Writer is currently mutating; spin
                std::hint::spin_loop();
                continue;
            }
            // 2. Read snapshot data
            let snapshot = unsafe { std::ptr::read_volatile(self.snapshot.get()) };
            std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);

            // 3. Verify sequence has not changed
            let s2 = self.sequence.load(std::sync::atomic::Ordering::Acquire);
            if s1 == s2 {
                return snapshot;
            }
            std::hint::spin_loop();
        }
    }
}
```

##### Option B: Atomic 128-bit Packed State Snapshot (`AtomicU128`)
If the platform supports lock-free 128-bit atomic loads (`std::sync::atomic::AtomicU128`), the state can be packed into a single `u128`:
- Bits `0..=47` (48 bits): Monotonic anchor wall time in microseconds since clock epoch ($\approx 8,925$ years).
- Bits `48..=95` (48 bits): Anchor PTS in milliseconds ($\approx 8,925$ years).
- Bits `96..=126` (31 bits): Playback speed stored as fixed-point $Q16.15$ or parts-per-million ($1.000000 = 1\,000\,000$).
- Bit `127` (1 bit): Paused flag ($0 = \text{Playing}, 1 = \text{Paused}$).

#### Line-by-Line Spec Changes for `openspec/specs/av-pipeline/spec.md`
**Target Lines**: 29–43

**Replace lines 35–43 with**:
```markdown
When the playback speed shifts from $s_{\text{old}}$ to $s_{\text{new}}$ at instant $t_{\text{now}}$ (e.g. commanded by the sync engine's PLL drift controller):
1. The new anchor PTS MUST be computed as $\text{anchor\_pts}' \leftarrow \text{CurrentPTS}(t_{\text{now}})$.
2. The clock state ($\text{anchor\_pts}'$, $\text{anchor\_wall}' = t_{\text{now}}$, $\text{speed} = s_{\text{new}}$, and playback state) MUST be published as an **atomic snapshot** using a lock-free Sequence Lock (`SeqLock`) or atomic 128-bit snapshot (`AtomicU128`).
3. The implementation MUST NOT perform disjoint, non-atomic multi-field sequential writes to `anchor_pts`, `anchor_wall`, and `speed`.

The clock implementation MUST guarantee $C^0$ continuity across rate transitions:
$$\lim_{\epsilon \to 0^+} \text{CurrentPTS}(t_{\text{now}} + \epsilon) = \lim_{\epsilon \to 0^-} \text{CurrentPTS}(t_{\text{now}} - \epsilon)$$
with zero discontinuous jumps, frame drops, or timeline tearing observable by concurrent reader threads.
```

**Target Lines in `openspec/changes/003-clock-pipeline.md`**:
**Replace lines 45–48 with**:
```markdown
   - Atomic state snapshot publication on rate shifts using a lock-free `SeqLock` or `AtomicU128`:
     $$\text{PublishSnapshot}(\text{anchor\_pts}' \leftarrow \text{CurrentPTS}(t_{\text{now}}), \quad \text{anchor\_wall}' \leftarrow t_{\text{now}}, \quad \text{speed} \leftarrow s_{\text{new}})$$
     guaranteeing disjoint multi-field atomic read race conditions are mathematically eliminated:
     $$\lim_{\epsilon \to 0} |\text{CurrentPTS}(t_{\text{now}} + \epsilon) - \text{CurrentPTS}(t_{\text{now}} - \epsilon)| = 0$$
```

---

### Remediation 2: Synthetic EBML Header Length Mismatch (48B vs 47B)

#### Problem Diagnosis
In `openspec/specs/demuxer/spec.md`, line 44 states:
`- **Given** the 47-byte in-tree synthetic EBML header test vector:`
Followed by the byte array in lines 45–50.
In `openspec/changes/001-demuxer-bitstream.md`, lines 42 and 60 refer to "47B" and "47-byte".

#### Verification & Byte Map
The byte slice contains exactly 48 bytes:
```text
EBML Header:
0x1A, 0x45, 0xDF, 0xA3   -> [4B] EBML Header Element ID (0x1A45DFA3)
0x9F                     -> [1B] VINT Data Size: 31 bytes (0x1F)
0x42, 0x86, 0x81, 0x01   -> [4B] EBMLVersion: ID 0x4286, size 0x81 (1B), val 0x01
0x42, 0xF7, 0x81, 0x01   -> [4B] EBMLReadVersion: ID 0x42F7, size 0x81 (1B), val 0x01
0x42, 0xF2, 0x81, 0x04   -> [4B] EBMLMaxIDLength: ID 0x42F2, size 0x81 (1B), val 0x04
0x42, 0xF3, 0x81, 0x08   -> [4B] EBMLMaxSizeLength: ID 0x42F3, size 0x81 (1B), val 0x08
0x42, 0x82, 0x88,        -> [3B] DocType: ID 0x4282, size 0x88 (8B)
b'm', b'a', b't', b'r',
b'o', b's', b'k', b'a'   -> [8B] DocType string payload ("matroska")
0x42, 0x87, 0x81, 0x04   -> [4B] DocTypeVersion: ID 0x4287, size 0x81 (1B), val 0x04
-- Subtotal EBML Header: 4 + 1 + 31 = 36 bytes.

Root Segment Header:
0x18, 0x53, 0x80, 0x67   -> [4B] Segment Element ID (0x18538067)
0x01, 0xFF, 0xFF, 0xFF,
0xFF, 0xFF, 0xFF, 0xFF   -> [8B] 8-octet VINT Unknown Size (0x01FFFFFFFFFFFFFF)
-- Subtotal Segment Header: 4 + 8 = 12 bytes.

Total Byte Count = 36 + 12 = 48 bytes.
```

#### Line-by-Line Spec Changes
1. **`openspec/specs/demuxer/spec.md`**:
   - **Line 44**: Replace:
     `- **Given** the 47-byte in-tree synthetic EBML header test vector:`
     with:
     `- **Given** the 48-byte in-tree synthetic EBML header test vector (36-byte EBML Header + 12-byte Segment Header):`
   - **Line 55**: Add assertion:
     `- **And** the total length of the synthetic vector is verified to be exactly 48 bytes.`

2. **`openspec/changes/001-demuxer-bitstream.md`**:
   - **Line 42**: Replace:
     `- src/synthetic.rs: Byte-exact in-tree synthetic EBML (47B) and MP4 (132B) test vectors.`
     with:
     `- src/synthetic.rs: Byte-exact in-tree synthetic EBML (48B) and MP4 (132B) test vectors.`
   - **Line 60**: Replace:
     `- 47-byte synthetic EBML header vector.`
     with:
     `- 48-byte synthetic EBML header vector.`

---

### Remediation 3: SimpleBlock PTS Timescale Conversion

#### Problem Diagnosis
In `openspec/specs/demuxer/spec.md`, line 74 defines:
$$\text{PTS} = (\text{Cluster.Timestamp} + \text{SimpleBlock.RelativeTimestamp}) \times \text{TimestampScale}$$
In Matroska (RFC 8794 / Matroska specification), `TimestampScale` (`0x2AD7B1`) represents **nanoseconds per tick** (default: $1,000,000\text{ ns} = 1\text{ ms}$).
Cluster and SimpleBlock timestamps are measured in ticks. Multiplying ticks by `TimestampScale` yields nanoseconds ($\text{PTS}_{\text{ns}}$), NOT milliseconds ($\text{pts\_ms}$).

#### Mathematical & Code Formulation
The specification must mandate unambiguous integer arithmetic converting nanosecond-scale `TimestampScale` to millisecond presentation timestamps:
$$\text{PTS}_{\text{ns}} = (\text{ClusterTimecode} + \text{BlockTimecode}) \times \text{TimestampScale}$$
$$\text{PTS}_{\text{ms}} = \left\lfloor \frac{(\text{ClusterTimecode} + \text{BlockTimecode}) \times \text{TimestampScale}}{1\,000\,000} \right\rfloor$$

In Rust integer arithmetic:
```rust
pub fn calculate_simpleblock_pts_ms(
    cluster_timecode: u64,
    block_relative_timecode: i16,
    timestamp_scale_ns: u64,
) -> Result<i64, DemuxError> {
    let ticks = (cluster_timecode as i64)
        .checked_add(block_relative_timecode as i64)
        .ok_or(DemuxError::TimestampOverflow)?;

    let pts_ns = ticks
        .checked_mul(timestamp_scale_ns as i64)
        .ok_or(DemuxError::TimestampOverflow)?;

    // Truncated integer division to milliseconds:
    Ok(pts_ns / 1_000_000)
}
```

#### Line-by-Line Spec Changes
1. **`openspec/specs/demuxer/spec.md`**:
   - **Replace lines 73–75 with**:
     ```markdown
     The demuxer SHALL parse `Cluster` (`0x1F43B675`) elements and extract packets from `SimpleBlock` (`0xA3`) payloads. The parser MUST calculate the presentation timestamp in nanoseconds ($\text{PTS}_{\text{ns}}$) and convert it to milliseconds ($\text{PTS}_{\text{ms}}$) using integer arithmetic:
     $$\text{PTS}_{\text{ns}} = (\text{ClusterTimecode} + \text{BlockTimecode}) \times \text{TimestampScale}$$
     $$\text{PTS}_{\text{ms}} = \left\lfloor \frac{\text{PTS}_{\text{ns}}}{1\,000\,000} \right\rfloor = \left\lfloor \frac{(\text{ClusterTimecode} + \text{BlockTimecode}) \times \text{TimestampScale}}{1\,000\,000} \right\rfloor$$
     where $\text{ClusterTimecode}$ is `u64` cast to `i64`, $\text{BlockTimecode}$ is the 16-bit signed relative timestamp (`i16` sign-extended to `i64`), and $\text{TimestampScale}$ is nanoseconds per tick (`u64`, default $1,000,000$).
     ```
   - **Update lines 78–83**:
     ```markdown
     #### Scenario: Extract Keyframe SimpleBlock
     - **Given** a Cluster with base timecode $10,000$ ticks and `TimestampScale = 1,000,000` ($1,000,000\text{ ns} = 1\text{ ms}$ per tick)
     - **And** a SimpleBlock with Track VINT `0x81` (Track 1), relative timecode `0x00FA` (+250 ticks), and flags `0x80` (Keyframe flag set, no lacing)
     - **When** `EbmlDemuxer::next_packet` reads the block
     - **Then** $\text{PTS}_{\text{ns}} = (10,000 + 250) \times 1,000,000 = 10,250,000,000\text{ ns}$
     - **And** the returned `DemuxPacket` has:
       - `track_id = 1`
       - `pts_ms = 10250`
       - `is_keyframe = true`
     ```

2. **`openspec/changes/001-demuxer-bitstream.md`**:
   - **Replace line 55 with**:
     ```markdown
          $$\text{PTS}_{\text{ms}} = \left\lfloor \frac{(\text{ClusterTimecode} + \text{BlockTimecode}) \times \text{TimestampScale}}{1\,000\,000} \right\rfloor$$
     ```

---

### Remediation 4: ISOBMFF 64-bit `largesize` and 32-bit `size` Bounds Validation

#### Problem Diagnosis
In `openspec/specs/demuxer/spec.md`, lines 89–98 state:
`The demuxer SHALL parse ISOBMFF box headers supporting both standard 32-bit box lengths and 64-bit largesize boxes (size == 1). If size == 0, the parser SHALL treat the box as extending to the end of the input stream.`
If a crafted or corrupted file specifies `size == 1` and $\text{largesize} < 16$ (e.g. $\text{largesize} = 8$), calculating payload size `largesize - 16` causes an unsigned integer underflow in Rust ($8\text{u64} - 16\text{u64}$ panics in debug mode or wraps to $18,446,744,073,709,551,608$ in release mode).
Similarly, if standard `size < 8` and `size != 0`, computing `size - 8` underflows.
Furthermore, if $\text{largesize} > \text{i64::MAX as u64}$, seeking via `SeekFrom::Current(offset as i64)` interprets the offset as negative, seeking backward.

#### Validation Rule Specification
```rust
pub fn parse_box_header(input: &[u8]) -> Result<BoxHeader, DemuxError> {
    if input.len() < 8 {
        return Err(DemuxError::UnexpectedEof { needed: 8, available: input.len() });
    }
    let size = u32::from_be_bytes(input[0..4].try_into().unwrap());
    let box_type = [input[4], input[5], input[6], input[7]];

    let (header_size, total_size) = if size == 1 {
        if input.len() < 16 {
            return Err(DemuxError::UnexpectedEof { needed: 16, available: input.len() });
        }
        let largesize = u64::from_be_bytes(input[8..16].try_into().unwrap());
        if largesize < 16 {
            return Err(DemuxError::MalformedBox("largesize must be >= 16 bytes"));
        }
        if largesize > i64::MAX as u64 {
            return Err(DemuxError::MalformedBox("largesize exceeds i64::MAX"));
        }
        (16usize, BoxSize::Known(largesize))
    } else if size == 0 {
        (8usize, BoxSize::UntilEof)
    } else if size < 8 {
        return Err(DemuxError::MalformedBox("box size must be >= 8 bytes or 0"));
    } else {
        (8usize, BoxSize::Known(size as u64))
    };

    Ok(BoxHeader { box_type, header_size, total_size })
}
```

#### Line-by-Line Spec Changes for `openspec/specs/demuxer/spec.md`
**Target Lines**: 89–98

**Replace lines 90–98 with**:
```markdown
The demuxer SHALL parse ISOBMFF box headers supporting both standard 32-bit box lengths and 64-bit `largesize` boxes (`size == 1`). If `size == 0`, the parser SHALL treat the box as extending to the end of the input stream.
The parser MUST enforce strict bounds validation:
1. For 64-bit `largesize` boxes (`size == 1`): `if size == 1 && largesize < 16 => Err(DemuxError::MalformedBox)`. Furthermore, `largesize` MUST NOT exceed `i64::MAX as u64` ($2^{63}-1$).
2. For standard 32-bit boxes: `if size != 0 && size < 8 => Err(DemuxError::MalformedBox)`.
3. If input buffer length is less than `header_size` (8 bytes for standard, 16 bytes for `largesize`), the parser MUST return `Err(DemuxError::UnexpectedEof)`.
For `FullBox` structures, the parser MUST read the 1-byte version and 3-byte flags following the box header.

#### Scenario: Reject Corrupted and Malformed Box Header Sizes
- **Given** an 8-byte buffer `[0x00, 0x00, 0x00, 0x05, b'm', b'd', b'a', b't']` (`size = 5`)
- **When** `BoxHeader::parse` is invoked
- **Then** the parser returns `Err(DemuxError::MalformedBox)`
- **Given** a 16-byte buffer `[0x00, 0x00, 0x00, 0x01, b'm', b'd', b'a', b't', 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08]` (`largesize = 8`)
- **When** `BoxHeader::parse` is invoked
- **Then** the parser returns `Err(DemuxError::MalformedBox)`.
```

---

### Remediation 5: YUV420p Planar Row Stride Padding for SIMD AVX2/NEON Alignment

#### Problem Diagnosis
In `openspec/specs/codecs/spec.md`, lines 20–23 define:
- Luminance Plane ($Y$): $W \times H$ bytes with row stride $S_y \ge W$.
- Chroma Planes ($U, V$): $\lceil W/2 \rceil \times \lceil H/2 \rceil$ bytes with row stride $S_u, S_v \ge \lceil W/2 \rceil$.
And line 98 requires:
`The implementation SHALL process 2 horizontal pixels simultaneously or 16 pixels via SIMD vectorization without reading past buffer boundaries.`
When frame width is not a multiple of 16 (e.g. 480p width $W = 854$ or $W = 1921$), setting $S_y = W$ causes the 54th 16-pixel SIMD vector (bytes $848 \dots 863$) to read 10 bytes past the end of row 0 into row 1. On the final scanline ($H-1$), this reads 10 bytes past the end of the allocated plane buffer, causing page faults, `SIGSEGV`, or cache corruption under AVX2 / NEON.

#### Alignment & Allocation Formula
The specification must mandate:
1. **Row Stride 16-Byte Alignment**:
   $$\text{stride}_y = (\text{width} + 15) \ \& \ !15$$
   $$\text{chroma\_width} = \left\lceil \frac{\text{width}}{2} \right\rceil = \left\lfloor \frac{\text{width} + 1}{2} \right\rfloor$$
   $$\text{stride}_u = \text{stride}_v = (\text{chroma\_width} + 15) \ \& \ !15$$
   *(Note: Implementations MAY pad to 64 bytes `(width + 63) & !63` for direct GPU DMA or AVX-512, but row stride MUST at minimum be 16-byte aligned).*
2. **Buffer Capacity with SIMD Tail Over-Read Padding**:
   To ensure that a 16-byte or 32-byte SIMD load beginning at the start of the final block of a row never over-reads the buffer end:
   $$\text{capacity}_y \ge \text{stride}_y \times \text{height} + 16$$
   $$\text{capacity}_u \ge \text{stride}_u \times \left\lceil \frac{\text{height}}{2} \right\rceil + 16$$
   $$\text{capacity}_v \ge \text{stride}_v \times \left\lceil \frac{\text{height}}{2} \right\rceil + 16$$

#### Line-by-Line Spec Changes for `openspec/specs/codecs/spec.md`
**Target Lines**: 19–35

**Replace lines 20–25 with**:
```markdown
The codec subsystem SHALL represent uncompressed video frames in planar YUV420p (I420) format. The frame buffer MUST consist of three separate planes with row strides aligned to at least 16 bytes to prevent SIMD AVX2/NEON vector over-read:
1. **Luminance Plane ($Y$)**: $W \times H$ bytes with row stride $S_y \ge (\text{width} + 15) \ \& \ !15$.
2. **Chroma Blue-Difference Plane ($U / Cb$)**: $\lceil W/2 \rceil \times \lceil H/2 \rceil$ bytes with row stride $S_u \ge (\lceil W/2 \rceil + 15) \ \& \ !15$.
3. **Chroma Red-Difference Plane ($V / Cr$)**: $\lceil W/2 \rceil \times \lceil H/2 \rceil$ bytes with row stride $S_v \ge (\lceil W/2 \rceil + 15) \ \& \ !15$.

All plane buffer allocations MUST be aligned to 64-byte boundaries (`#[repr(align(64))]`) and include at least 16 bytes of end-of-buffer tail padding to permit aligned SIMD loads without reading past allocation boundaries on the final row.
```

**Add Scenario under line 34**:
```markdown
#### Scenario: Allocate Non-Multiple-of-16 Frame with Padded Strides
- **Given** video dimensions $W = 854$ (480p) and $H = 480$
- **When** a new `VideoFrameYUV420p` is initialized
- **Then** the Y-plane stride $S_y$ is $(854 + 15) \ \& \ !15 = 864\text{ bytes}$
- **And** chroma width is $\lceil 854 / 2 \rceil = 427\text{ pixels}$
- **And** chroma strides $S_u, S_v$ are $(427 + 15) \ \& \ !15 = 432\text{ bytes}$
- **And** 16-pixel SIMD vector operations execute across all rows without out-of-bounds reads.
```

---

### Remediation 6: SPSC Ring Buffer Full/Empty Detection, Monotonic Indexing, and Signatures

#### Problem Diagnosis
In `openspec/specs/av-pipeline/spec.md`, lines 74–109 specify capacity and acquire/release ordering, but omit:
1. Concrete method signatures and return error types for `push` and `pop`.
2. Exact mathematical full and empty detection conditions.
3. Monotonic unbounded index representation vs modulo-reduced pointers (which trigger the $64 = 0$ full/empty aliasing hazard).

#### Formal Design & Method Signatures
The ring buffer MUST use free-running unbounded `usize` atomic counters (`head` and `tail`). Masking with `(CAPACITY - 1)` is performed only when dereferencing buffer slots.

##### Method Signatures
```rust
#[derive(Debug, PartialEq, Eq)]
pub struct BufferFullError<T>(pub T);

pub struct SpscRingBuffer<T, const CAPACITY: usize> {
    // Producer cache line
    head: CachePadded<AtomicUsize>,
    // Consumer cache line
    tail: CachePadded<AtomicUsize>,
    // Storage
    slots: Box<[UnsafeCell<Option<T>>]>,
}

impl<T, const CAPACITY: usize> SpscRingBuffer<T, CAPACITY> {
    pub fn new() -> Self;
    pub fn try_push(&self, item: T) -> Result<(), BufferFullError<T>>;
    pub fn try_pop(&self) -> Option<T>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn is_full(&self) -> bool;
}
```

##### Full and Empty Mathematical Conditions
- **Mask**: `CAPACITY` is a power of two ($2^N$); `MASK = CAPACITY - 1`.
- **Slot Index**: `slot = counter & MASK`.
- **Buffer Empty Condition (Consumer)**:
  $$\text{head.load(Acquire)} == \text{tail.load(Relaxed)}$$
  When `head == tail`, `try_pop()` returns `None`.
- **Buffer Full Condition (Producer)**:
  $$\text{head.load(Relaxed).wrapping\_sub(tail.load(Acquire))} == \text{CAPACITY}$$
  When full, `try_push(item)` returns `Err(BufferFullError(item))`, preserving item ownership for caller backpressure handling.

#### Line-by-Line Spec Changes for `openspec/specs/av-pipeline/spec.md`
**Target Lines**: 74–109

**Replace lines 98–110 with**:
```markdown
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
```

---

### Remediation 7: Truncated VINTs and Reserved Element IDs (Challenger F-05)

#### Problem Diagnosis
In `openspec/specs/demuxer/spec.md` (lines 19–37), the parser handles `0x00` invalid prefix, but omits truncated slices and RFC 8794 reserved Element IDs (`0xFF`, `0xFFFF`, `0xFFFFFF`, `0xFFFFFFFF`). Unchecked slicing `&buf[..width]` causes Rust index panics.

#### Line-by-Line Spec Changes for `openspec/specs/demuxer/spec.md`
**Target Lines**: 19–37

**Add to Requirement text (line 21)**:
```markdown
If the available input slice length is less than the octet length indicated by the leading zeros, the parser MUST return `Err(VintError::UnexpectedEof { needed: usize, available: usize })`. Per RFC 8794 Section 4.1, if all data bits of an Element ID are set to 1 (e.g. `0xFF`, `0xFFFF`, `0xFFFFFF`, `0xFFFFFFFF`), the parser MUST return `Err(VintError::ReservedElementId)`.
```

**Add Scenario under line 37**:
```markdown
#### Scenario: Handle Truncated VINT and Reserved Element ID
- **Given** a 1-byte buffer `[0x01]` (indicating an 8-octet VINT)
- **When** the VINT parser parses the buffer
- **Then** it returns `Err(VintError::UnexpectedEof { needed: 8, available: 1 })`
- **Given** an Element ID buffer `[0xFF]` or `[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]`
- **When** parsing an Element ID
- **Then** the parser returns `Err(VintError::ReservedElementId)`.
```

---

### Remediation 8: ISOBMFF Sample Table Index Ambiguity and Corruption Checks (Challenger F-08)

#### Problem Diagnosis
In `openspec/specs/demuxer/spec.md` (lines 109–134), `stss` uses 1-based indexing per ISOBMFF, but the formula $\text{Offset}(S)$ uses sample $S$. When resolving $S=4$, it is ambiguous whether this represents the 4th sample (index 3) or 5th sample (index 4). Furthermore, corrupt `stsc` tables with `samples_per_chunk == 0` cause divide-by-zero panics.

#### Line-by-Line Spec Changes for `openspec/specs/demuxer/spec.md`
**Target Lines**: 114–122

**Replace lines 114–122 with**:
```markdown
4. `stss`: Sync Sample Table identifying 1-based keyframe sample numbers ($sync\_sample \in [1, N]$). The demuxer MUST convert these to 0-based sample indices: $sample\_index = sync\_sample - 1$.
5. `stsc`: Sample-to-Chunk mapping run-length entries $(first\_chunk, samples\_per\_chunk, sample_description_index)$. The demuxer MUST validate that $samples\_per\_chunk \ge 1$; if $samples\_per\_chunk == 0$, the parser MUST return `Err(DemuxError::CorruptSampleTable)`.
6. `stsz`: Uniform or per-sample byte lengths.
7. `stco` / `co64`: 32-bit or 64-bit absolute chunk offsets in the file.

All internal sample table indexing MUST be 0-based ($S \in [0, N-1]$). The parser MUST compute sample $S$ byte offset as:
$$\text{Offset}(S) = \text{ChunkOffset}[C] + \sum_{j=S-k}^{S-1} \text{size}(j)$$
where chunk $C$ and index within chunk $k$ ($k \in [0, samples\_per\_chunk - 1]$) are derived from `stsc`.
```

---

### Remediation 9: B-Frame DTS vs PTS Presentation Reordering (Challenger F-09)

#### Problem Diagnosis
In `openspec/specs/av-pipeline/spec.md` (lines 54–72), the presentation decision window evaluates $\text{PTS} - \text{MasterClock}(t)$. However, decoders emit video frames in Decode TimeStamp (DTS) order. In streams with B-frames, later presentation frames can be emitted before earlier presentation frames (e.g. Frame 1 with PTS 300 arrives before Frame 2 with PTS 100). Feeding these directly to the scheduler causes Frame 2 to be dropped as severely late.

#### Line-by-Line Spec Changes for `openspec/specs/av-pipeline/spec.md`
**Target Lines**: 54–72

**Add to Requirement text (line 62)**:
```markdown
Between decoder frame emission and the presentation decision window, the pipeline MUST maintain a bounded presentation reorder buffer (min-heap or fixed-capacity priority queue ordered by ascending PTS) of capacity $K$ (nominally $K = 16$ frames). Decoded frames emitted in decode order (DTS) MUST be inserted into the reorder buffer, and the frame with the minimum PTS MUST be yielded to the presentation scheduler once the reorder window is populated or upon container stream drain.
```

---

## 4. Implementation Worker Action Plan

The implementation worker should apply the above changes following this sequence:

1. **Step 1: Update `openspec/specs/demuxer/spec.md`**:
   - Update line 44 from "47-byte" to "48-byte" and add 48-byte length verification.
   - Update line 74 and scenario with explicit nanosecond-to-millisecond conversion formula $\lfloor \text{PTS}_{\text{ns}} / 1,000,000 \rfloor$.
   - Add bounds checks for `size == 1 && largesize < 16` and standard `size < 8`.
   - Add `UnexpectedEof` and `ReservedElementId` error handling to VINT specification.
   - Standardize sample table indexing to 0-based and mandate $samples\_per\_chunk \ge 1$.

2. **Step 2: Update `openspec/specs/codecs/spec.md`**:
   - Mandate 16-byte aligned row strides `(width + 15) & !15` and $+16$B SIMD tail padding.
   - Add test scenario verifying non-multiple-of-16 frame allocation (e.g. $854 \times 480$).

3. **Step 3: Update `openspec/specs/av-pipeline/spec.md`**:
   - Mandate atomic snapshot publishing for clock re-anchoring (`SeqLock` or `AtomicU128`), prohibiting disjoint multi-field writes.
   - Specify explicit SPSC signatures `try_push(&self, item: T) -> Result<(), BufferFullError<T>>` and `try_pop(&self) -> Option<T>`.
   - Specify monotonic free-running wrapping counter full/empty conditions.
   - Specify bounded B-frame presentation reorder buffer.

4. **Step 4: Update Change Proposals**:
   - `openspec/changes/001-demuxer-bitstream.md`: update 47B references to 48B, update PTS formula.
   - `openspec/changes/002-codecs-presentation.md`: update stride alignment to 16B.
   - `openspec/changes/003-clock-pipeline.md`: update atomic snapshot re-anchoring and SPSC signatures.

---

## 5. Summary Conclusion

By implementing this line-by-line strategy, the implementation worker will completely resolve all 2 CRITICAL, 4 HIGH, and 3 MEDIUM vulnerabilities identified by Challenger 1, ensuring the OpenSpec specifications are mathematically watertight, memory-safe, and ready for crate implementation.
