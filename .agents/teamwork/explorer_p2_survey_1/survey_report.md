# Phase 2 Technical Survey: Full Container Demuxing & Packet Processing (`001-demuxer-bitstream`)

**Component**: `crates/syncplay-demux` & `crates/syncplay-common`  
**Author**: `explorer_p2_survey_1` (Teamwork Preview Explorer)  
**Date**: 2026-10-25T09:18:00Z  
**Status**: Comprehensive Technical Survey Report  

---

## 1. Executive Summary & Architectural Objectives

Syncplay-Alt is an open-source, ultra-efficient standalone media player and long-distance synchronized watch-party platform written in pure, idiomatic Rust. Under Section 2 of `AGENTS.md`, the platform enforces the **Zero-External-Codec Invariant**: no crate in the workspace may depend on or vendor external multimedia or container engines (`ffmpeg`, `libav*`, `gstreamer`, `nom-ebml`, `symphonia`, etc.).

During Phase 1, foundational binary parsing primitives and mathematical invariants were established:
- RFC 8794 EBML VINT decoder (`src/vint.rs`).
- Synthetic 48-byte EBML header vector (`SYNTHETIC_EBML_VECTOR`) and 24-hour integer PTS arithmetic (`compute_simpleblock_pts`).
- ISOBMFF 32-bit and 64-bit box header bounds validator (`BoxHeader::parse`) and synthetic 132-byte MP4 header vector (`SYNTHETIC_MP4_VECTOR`).
- Foundational `SampleTableResolver` for MP4 sample-to-chunk index math.

### Phase 2 Mandate (R1: Container Demuxing & Packet Processing)
Phase 2 transitions `syncplay-demux` from static header probing into an **end-to-end operational stream demuxer**:
1. **Sequential Packet Extraction**: Emitting interleaved `DemuxPacket` streams (audio, video, timed text) with validated PTS and DTS from both Matroska (MKV/WebM) and ISOBMFF (MP4/MOV) byte streams.
2. **Keyframe Indexing & Seeking**: Implementing deterministic `seek_to(pts)` mapping to nearest preceding keyframes via Matroska `Cues` (`0x1C53BB6B`) and ISOBMFF `stss`/`stts` tables, returning exact landed presentation timestamps for pre-roll decoding.
3. **Multi-Track Stream Parsing**: Parsing all active track entries (`Tracks`/`TrackEntry` in MKV, `moov/trak` in MP4) with full codec metadata (`TrackInfo`: width, height, sample rate, channels, codec ID strings).
4. **Synthetic Test Vectors & Adversarial Edge Cases**: Providing self-contained, in-tree synthetic full MKV and MP4 test vectors enabling end-to-end packet reading, seeking, and zero-drift verification under `scripts/intent_harness.py`.

---

## 2. Current State Analysis (Phase 1 Baseline vs Phase 2 Requirements)

An exhaustive code audit of `crates/syncplay-demux` reveals the exact current implementation state and gaps:

### 2.1 Matroska / EBML Demuxer (`src/ebml.rs`)
| Component | Current State (Phase 1) | Phase 2 Requirement | Gap / Deficiency |
| :--- | :--- | :--- | :--- |
| **EBML Header** | `parse_ebml_header` validates `0x1A45DFA3`, `DocType` (`"matroska"` or `"webm"`), and versions. | Keep unchanged. | Fully compliant. |
| **Container Info & Metadata** | Hardcoded in `EbmlDemuxer::init` (lines 168–180): emits fixed dummy VP9 video track ($1920\times 1080$), duration `None`, default timescale. | Must traverse `Segment` (`0x18538067`) and parse `Info` (`0x1549A966`) for `TimestampScale` (`0x2AD7B1`) and `Duration` (`0x4489`). | Real container metadata and timescale are ignored. |
| **Track Inventory** | Hardcoded single track; does not parse `Tracks` (`0x1654AE6B`) or `TrackEntry` (`0xAE`). | Must parse repeated `TrackEntry` elements, detecting `TrackNumber`, `TrackType` (1=Video, 2=Audio, 17=Subtitle), `CodecID`, `VideoSettings`, and `AudioSettings`. | Audio and subtitle tracks cannot be recognized. |
| **SimpleBlock & Cluster** | Reads `Cluster` (`0x1F43B675`), `Timecode` (`0xE7`), and `SimpleBlock` (`0xA3`). | Must support `SimpleBlock` AND `BlockGroup` (`0xA0`) / `Block` (`0xA1`), plus frame lacing. | Lacing (Xiph, Fixed, EBML) is currently ignored; concatenated audio frames are treated as a single corrupt payload. |
| **Seeking (`seek_to`)** | Stubbed at line 306: `fn seek_to(&mut self, target_pts_ms: u64) -> Result<u64, DemuxError> { Ok(target_pts_ms) }`. | Must parse `Cues` (`0x1C53BB6B`) / `CuePoint` (`0xBB`), locate preceding keyframe cluster offset, seek reader, and reset cluster state. | Seeking does not move the file pointer or land on keyframes. |

### 2.2 ISOBMFF / MP4 Demuxer (`src/isobmff.rs`)
| Component | Current State (Phase 1) | Phase 2 Requirement | Gap / Deficiency |
| :--- | :--- | :--- | :--- |
| **BoxHeader Parsing** | Fully compliant in `BoxHeader::parse`: handles 32-bit boxes ($size \ge 8$ or $0$), 64-bit `largesize` ($16 \le largesize \le i64::MAX$), and bounds checks. | Keep unchanged. | Fully compliant. |
| **Container Info & Metadata** | `parse_headers_from_slice` reads `moov/mvhd` timescale and duration; tracks are hardcoded to a single dummy `avc1` track. | Must parse child `trak` boxes inside `moov`, reading `tkhd`, `mdia/mdhd`, `mdia/hdlr`, `minf/stbl/stsd`. | Track properties, secondary tracks, and audio/subtitles are not parsed from containers. |
| **Sample Table Resolver** | `SampleTableResolver` exists and has unit tests for `stsc`, `stsz`, `stco`, and `stss` mapping. | Connect `SampleTableResolver` to `IsoBmffDemuxer` per track. | `SampleTableResolver` is currently an isolated struct not instantiated or used by `IsoBmffDemuxer`. |
| **Packet Extraction (`read_packet`)** | Stubbed at line 396: `fn read_packet(&mut self) -> Result<Option<DemuxPacket>, DemuxError> { Ok(None) }`. | Must read interleaved samples from `mdat` across tracks, computing PTS and DTS, and emitting `DemuxPacket`s. | MP4 demuxing emits zero packets. |
| **Seeking (`seek_to`)** | Stubbed at line 400: returns `Ok(target_pts_ms)`. | Must resolve keyframe sample index in `stss`, align audio track sample index, seek reader to chunk offset, and return landed PTS. | Seeking is completely inoperative. |

---

## 3. Deep Technical Survey: Container Demuxing & Bitstream Processing (R1)

### 3.1 Sequential `DemuxPacket` Payloads & Validated Timestamps (PTS/DTS)

#### A. Matroska Cluster & Block Demuxing
In Matroska, media payloads are stored in `Cluster` (`0x1F43B675`) master elements.
Each Cluster begins with a `Timecode` (`0xE7`) element:
```
Cluster (0x1F43B675)
├── Timecode (0xE7) -> Base cluster timecode in TimestampScale ticks (u64)
├── SimpleBlock (0xA3) [or BlockGroup (0xA0)]
└── ...
```

##### 1. SimpleBlock Wire Format
A `SimpleBlock` (`0xA3`) wire payload has the following byte structure:
1. `TrackNumber`: EBML VINT (1–8 octets) decoded via `parse_vint(slice, false)`.
2. `Timecode`: Signed 16-bit big-endian integer (`i16`) representing relative ticks from `ClusterTimecode`.
3. `Flags`: 1 octet:
   - `Bit 7` (`0x80`): Keyframe flag (`1` = keyframe, `0` = inter-frame).
   - `Bits 6..4` (`0x70`): Reserved (must be 0).
   - `Bit 3` (`0x08`): Invisible / discardable.
   - `Bits 2..1` (`0x06`): Lacing flag:
     - `0b00` (0): No lacing (payload contains exactly 1 frame).
     - `0b01` (1): Xiph lacing.
     - `0b10` (2): Fixed-size lacing.
     - `0b11` (3): EBML lacing.
   - `Bit 0` (`0x01`): Discardable.
4. `Payload`: Compressed bitstream frame(s).

##### 2. BlockGroup Wire Format
A `BlockGroup` (`0xA0`) wraps a `Block` (`0xA1`) alongside metadata:
- `Block` (`0xA1`): Format identical to `SimpleBlock`, but Bit 7 is reserved.
- `ReferenceBlock` (`0xFB`): Relative timestamp of reference frame (`i16`). **Critical invariant**: If `ReferenceBlock` is absent inside `BlockGroup`, the block is an intra-frame / keyframe (`is_keyframe = true`). If present, `is_keyframe = false`.
- `BlockDuration` (`0x8B`): Duration in ticks.

##### 3. Lacing Algorithms (Required for Audio Stream Unpacking)
When lacing bits are non-zero, the payload begins with a 1-octet frame count: `frame_count = payload[0] + 1` ($N \ge 2$ frames).
- **No Lacing (`00`)**: The entire remaining slice is a single frame.
- **Xiph Lacing (`01`)**:
  - For frames $0 \dots N-2$: Read bytes summing values equal to 255 until encountering a value $< 255$.
    $$\text{size} = \sum 255 + \text{last\_byte}$$
  - Final frame ($N-1$): Consumes all remaining bytes in the block.
- **Fixed-Size Lacing (`10`)**:
  - The remaining payload length must be divisible by $N$.
  - Each frame has length $\text{size} = \text{remaining} / N$.
- **EBML Lacing (`11`)**:
  - Frame 0 length: Decoded as standard unsigned EBML VINT.
  - Frames $1 \dots N-2$: Encoded as signed EBML VINT deltas relative to previous frame length:
    - Delta VINT uses bias $2^{(7 \cdot w - 1)} - 1$ where $w$ is octet width.
    $$\text{size}_i = \text{size}_{i-1} + \text{delta}_i$$
  - Final frame ($N-1$): All remaining bytes.

##### 4. Internal Packet Queue Architecture
Because a single laced `SimpleBlock` yields $N \ge 2$ frames with identical or interpolated timestamps, `EbmlDemuxer` MUST maintain an internal buffer:
```rust
packet_queue: VecDeque<DemuxPacket>,
```
When `read_packet()` is invoked:
1. If `packet_queue` has queued packets, pop and return immediately.
2. Otherwise, parse the next block from the reader, unpack laced frames into `packet_queue`, and pop the first.

##### 5. Timestamp Calculation & Invariants
Presentation timestamps MUST be computed using the verified integer arithmetic:
$$\text{PTS}_{\text{ns}} = (\text{ClusterTimecode} + \text{BlockTimecode}) \times \text{TimestampScale}$$
$$\text{PTS}_{\text{ms}} = \lfloor \text{PTS}_{\text{ns}} / 1,000,000 \rfloor$$
Computed via `i64` arithmetic using `checked_add` and `checked_mul`. DTS in Matroska defaults to `None` (identical to PTS for non-reordered bitstreams).

---

#### B. ISOBMFF / MP4 Interleaved Sample Demuxing
In ISOBMFF containers, media bitstream samples are located in the `mdat` (Media Data) box. Their byte offsets, sizes, and presentation times are completely described by the Sample Table (`stbl`) boxes inside each track's `moov/trak/mdia/minf/stbl`.

##### 1. Multi-Track State Tracking
For each track parsed from `moov/trak`, `IsoBmffDemuxer` maintains an active track tracker:
```rust
pub struct Mp4TrackState {
    pub track_id: u64,
    pub track_type: TrackType,
    pub timescale: u64,
    pub resolver: SampleTableResolver,
    pub current_sample: usize,
    pub total_samples: usize,
    pub sample_dts: Vec<u64>,     // Cumulative DTS in track timescale ticks
    pub sample_pts: Vec<u64>,     // Cumulative PTS in track timescale ticks (DTS + CTTS)
}
```

##### 2. Interleaved Sequential Reading Algorithm
In an MP4 container, chunks from video and audio tracks are interleaved on disk to minimize drive seek head thrashing. To achieve optimal linear streaming:
1. For each track where `current_sample < total_samples`:
   Compute the file offset of its next sample using `resolver.compute_sample_offset(current_sample)`.
2. Select the track with the **minimum absolute file offset**:
   $$T^* = \arg\min_{T} \left( \text{sample\_offset}(T, T.\text{current\_sample}) \right)$$
3. Seek `reader` to `sample_offset`.
4. Read `sample_size = resolver.get_sample_size(current_sample)` bytes directly from the stream.
5. Compute timestamps:
   $$\text{DTS}_{\text{ms}} = \left\lfloor \frac{\text{sample\_dts}[current\_sample] \times 1000}{\text{timescale}} \right\rfloor$$
   $$\text{PTS}_{\text{ms}} = \left\lfloor \frac{\text{sample\_pts}[current\_sample] \times 1000}{\text{timescale}} \right\rfloor$$
6. Determine keyframe status:
   - If `resolver.sync_samples` is empty (e.g. audio track or intra-only video): `is_keyframe = true`.
   - Else, check if `resolver.sync_samples.binary_search(&current_sample).is_ok()`.
7. Increment $T^*.\text{current\_sample} += 1$.
8. Emit `DemuxPacket { track_id, pts_ms, dts_ms: Some(dts_ms), is_keyframe, data }`.
9. When all tracks satisfy `current_sample == total_samples`, return `Ok(None)`.

---

### 3.2 Keyframe Index Lookup & Random-Access Seeking (`seek_to(pts)`)

Random-access seeking requires locating the nearest keyframe preceding or at `target_pts_ms`, landing the reader exactly at the keyframe block or chunk, and resetting demuxer state so that subsequent calls to `read_packet()` emit cleanly starting from that keyframe.

#### A. Matroska `Cues` Architecture
Matroska files index keyframes using the top-level `Cues` (`0x1C53BB6B`) element.
```
Cues (0x1C53BB6B)
└── CuePoint (0xBB)
    ├── CueTime (0xB3) -> Timestamp in unscaled ticks (u64)
    └── CueTrackPositions (0xB7)
        ├── CueTrack (0xF7) -> Track number (u64)
        ├── CueClusterPosition (0xF1) -> Offset of target Cluster from Segment data start (u64)
        └── CueRelativePosition (0xF0) -> Optional offset of Block within Cluster (u64)
```

##### In-Memory Seek Table:
```rust
#[derive(Debug, Clone)]
pub struct CueEntry {
    pub pts_ms: u64,
    pub track_id: u64,
    pub cluster_file_offset: u64,
}
```

##### Seeking Algorithm in `EbmlDemuxer::seek_to`:
1. Find Cue: In `cues`, filter for video track (or primary track). Perform binary search:
   Find the largest entry where `entry.pts_ms <= target_pts_ms`.
   If none found (target before first cue), select `cues[0]`.
2. Position Reader:
   Seek `reader` to `entry.cluster_file_offset`.
3. Flush State:
   - Clear `packet_queue`.
   - Reset `current_cluster_timecode = 0` (it will be parsed from the landed cluster).
4. Return:
   `Ok(entry.pts_ms)` — exact landed keyframe presentation time in ms.
5. **Fallback for Unindexed MKV (Missing `Cues`)**:
   If `cues` is empty (e.g. live recording or unindexed stream), perform a bounded cluster skip:
   - Seek to beginning of `Segment`.
   - Loop through Clusters reading `ClusterTimecode` until reaching a cluster whose timecode $> target\_pts\_ms$.
   - Revert to previous cluster, scan SimpleBlocks to the nearest keyframe $\le target\_pts\_ms$, and return its PTS.

---

#### B. ISOBMFF `stss` & `stts` Seeking
In MP4, keyframes are explicitly recorded in `stss` (Sync Sample box) as 1-based sample numbers, converted to 0-based indices in `SampleTableResolver.sync_samples`.

##### Seeking Algorithm in `IsoBmffDemuxer::seek_to`:
1. Identify primary video track $V$.
2. In track $V$'s precomputed `sample_pts` array, find sample index $S_{\text{target}}$ such that $\text{PTS}(S_{\text{target}}) \approx target\_pts\_ms$.
3. Keyframe Resolution:
   - If $V.\text{sync\_samples}$ is empty (every frame is keyframe): $S_{\text{key}} = S_{\text{target}}$.
   - Else, binary search in $V.\text{sync\_samples}$ for the largest keyframe index $S_{\text{key}} \le S_{\text{target}}$. If none $\le S_{\text{target}}$, take $S_{\text{key}} = V.\text{sync\_samples}[0]$.
4. Audio Alignment:
   - For each audio track $A$, find sample index $S_A$ such that $\text{PTS}_A(S_A)$ is the closest sample $\le \text{PTS}_V(S_{\text{key}})$.
   - Set $A.\text{current\_sample} = S_A$.
5. Update Video State:
   - Set $V.\text{current\_sample} = S_{\text{key}}$.
6. Reposition Reader:
   - Compute byte offsets for both tracks at their new indices.
   - Seek `reader` to $\min(\text{offset}_V, \text{offset}_A)$.
7. Return:
   `Ok(pts_ms(S_{key}))`.

---

### 3.3 Multi-Track Stream Parsing (Video, Audio, Timed Text)

The platform requires simultaneous identification and decoding of multi-track streams.

#### A. Matroska Tracks Element Specification
```
Tracks (0x1654AE6B)
└── TrackEntry (0xAE)
    ├── TrackNumber (0xD7) -> u64 (1-based ID)
    ├── TrackUID (0x73C5) -> u64
    ├── TrackType (0x83) -> u8:
    │     1: Video
    │     2: Audio
    │     17: Subtitle
    ├── CodecID (0x86) -> ASCII string ("V_VP9", "V_MPEG4/ISO/AVC", "A_OPUS", "A_AAC", "S_TEXT/UTF8")
    ├── Video (0xE0)
    │     ├── PixelWidth (0xB0) -> u32
    │     ├── PixelHeight (0xBA) -> u32
    │     ├── DisplayWidth (0x54B0) -> u32
    │     └── DisplayHeight (0x54BA) -> u32
    ├── Audio (0xE1)
    │     ├── SamplingFrequency (0xB5) -> f32 / f64 (Hz)
    │     ├── Channels (0x9F) -> u32
    │     └── BitDepth (0x88) -> u32
    └── CodecPrivate (0x63A2) -> Binary extradata
```

`EbmlDemuxer` MUST iterate over all `TrackEntry` children, populating `TrackInfo` structures inside `ContainerInfo.tracks`:
```rust
let track_type = match track_type_num {
    1 => TrackType::Video,
    2 => TrackType::Audio,
    17 => TrackType::Subtitle,
    _ => continue,
};
```

---

#### B. ISOBMFF Movie (`moov`) Box Hierarchy
Inside `moov`, each stream is declared in an independent `trak` box:
```
moov (0x6D6F6F76)
├── mvhd (0x6D766864) -> Global timescale and duration
└── trak (0x7472616B) (repeated per track)
    ├── tkhd (0x746B6864) -> Track ID, dimensions (fixed 16.16)
    └── mdia (0x6D646961)
        ├── mdhd (0x6D646864) -> Track media timescale (Hz) & duration
        ├── hdlr (0x68646C72) -> Handler type: 'vide', 'soun', 'subt', 'sbtl'
        └── minf (0x6D696E66)
            └── stbl (0x7374626C)
                ├── stsd (0x73747364) -> Sample descriptions: 'avc1', 'vp09', 'mp4a', 'opus'
                ├── stts (0x73747473) -> Time-to-Sample run-length table (DTS)
                ├── ctts (0x63747473) -> Composition Time-to-Sample offsets (PTS = DTS + CTTS)
                ├── stss (0x73747373) -> Sync Sample keyframe table
                ├── stsc (0x73747363) -> Sample-to-Chunk run-length table
                ├── stsz (0x7374737A) -> Sample sizes (uniform or per-sample)
                └── stco / co64       -> 32-bit or 64-bit absolute chunk file offsets
```

`IsoBmffDemuxer` MUST:
1. Parse every `trak` box in `moov`.
2. Extract the handler type from `hdlr` to classify `TrackType`.
3. Extract codec FourCC from `stsd` (`avc1`, `vp09`, `mp4a`, `opus`, `tx3g`).
4. Read video pixel dimensions or audio channels/sample rate from the sample entry body.
5. Populate `SampleTableResolver` and compute cumulative DTS/PTS tables for that track.

---

### 3.4 Synthetic Test Vectors & Edge Case Specifications

#### A. Synthetic Test Vectors
To guarantee hermetic, zero-dependency CI verification:

1. **Preserve Phase 1 Vectors (Mandatory Invariant)**:
   - `SYNTHETIC_EBML_VECTOR` (48 bytes): EBML Header (36 bytes) + Segment Header (12 bytes).
   - `SYNTHETIC_MP4_VECTOR` (132 bytes): `ftyp` (16 bytes) + `moov/mvhd` (116 bytes).
   - Existing unit tests in `crates/syncplay-demux/src/ebml.rs` and `crates/syncplay-core/tests/adversarial_crates_1_to_4.rs` depend on their exact byte lengths.

2. **Add Extended Phase 2 Test Vectors (`src/synthetic.rs`)**:
   - `SYNTHETIC_FULL_MKV`: A byte-exact valid Matroska container containing:
     - EBML Header (`matroska`, version 4)
     - Segment Header with unknown size (`0x01FFFFFFFFFFFFFF`)
     - `Info` element (`TimestampScale = 1,000,000`, `Duration = 1000.0`)
     - `Tracks` element with Track 1 (Video, `V_VP9`, $1920\times 1080$) and Track 2 (Audio, `A_OPUS`, 48000Hz, 2ch)
     - `Cues` element with 2 seek points ($t = 0\text{ms}$ and $t = 500\text{ms}$)
     - Cluster 1 at $t=0$: Video Keyframe SimpleBlock ($t=0$, 8 bytes) + Audio SimpleBlock ($t=0$, 6 bytes)
     - Cluster 2 at $t=500$: Video Keyframe SimpleBlock ($t=500$, 8 bytes) + Audio SimpleBlock ($t=500$, 6 bytes)
   - `SYNTHETIC_FULL_MP4`: A byte-exact valid MP4 container containing:
     - `ftyp` box (`isom`, 16 bytes)
     - `moov` box with `mvhd` (timescale 1000, duration 1000ms)
     - `trak` 1 (Video, `avc1`, 1920x1080) with `stts`, `stss` (sample 0 keyframe), `stsc`, `stsz`, `stco`
     - `trak` 2 (Audio, `mp4a`, 48000Hz, 2ch) with `stts`, `stsc`, `stsz`, `stco`
     - `mdat` box containing the sample payloads referenced by `stco`

#### B. Adversarial Edge Case Matrix
The demuxer MUST be validated against the following edge cases:
1. **Truncated Buffers**: Buffer ends before VINT or BoxHeader completes $\implies$ `DemuxError::UnexpectedEof`.
2. **Malformed Box Sizes**: ISOBMFF $size \in [1..7]$, $largesize < 16$, or $largesize > i64::MAX \implies$ `DemuxError::MalformedBox`.
3. **Reserved EBML Element IDs**: All data bits set to 1 $\implies$ `VintError::ReservedElementId`.
4. **Invalid Zero-Prefix VINT**: First byte $0x00 \implies$ `VintError::InvalidZeroPrefix`.
5. **Corrupted Sample Table**: $samples\_per\_chunk == 0 \implies$ `DemuxError::CorruptSampleTable`.
6. **24-Hour Timescale Headroom**: Cluster and SimpleBlock tick arithmetic across 24h playback ($86,400,000\text{ ms}$) must never overflow `i64`.
7. **Clean Stream Termination**: When reader reaches EOF, demuxer returns `Ok(None)` with zero panics.

---

## 4. Data Structures, Trait Method Signatures & Error Enums Mapping

### 4.1 Enhanced Error Enums (`crates/syncplay-demux/src/error.rs`)
```rust
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum DemuxError {
    #[error("unexpected EOF: needed {needed} bytes, available {available}")]
    UnexpectedEof { needed: usize, available: usize },

    #[error("malformed box: {0}")]
    MalformedBox(String),

    #[error("corrupt sample table: {0}")]
    CorruptSampleTable(String),

    #[error("EBML VINT error: {0}")]
    Vint(#[from] VintError),

    #[error("integer overflow occurred")]
    IntegerOverflow,

    #[error("invalid container header: {0}")]
    InvalidHeader(String),

    #[error("I/O error: {0}")]
    Io(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("corrupted block framing: {0}")]
    CorruptedBlock(String),
}
```

### 4.2 Enhanced Data Models (`crates/syncplay-demux/src/traits.rs`)
Ensure `DemuxPacket` and `TrackInfo` provide complete information for codec initialization:
```rust
/// A demuxed raw media packet extracted from a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemuxPacket {
    /// ID of the track this packet belongs to.
    pub track_id: u64,
    /// Presentation timestamp in milliseconds.
    pub pts_ms: u64,
    /// Decode timestamp in milliseconds, if distinct from PTS.
    pub dts_ms: Option<u64>,
    /// Whether this packet is an independent random-access keyframe.
    pub is_keyframe: bool,
    /// Raw compressed bitstream payload.
    pub data: Vec<u8>,
}

/// Information describing an individual track within a container.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackInfo {
    pub track_id: u64,
    pub track_type: TrackType,
    pub codec_id: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub sample_rate: Option<f64>,
    pub channels: Option<u32>,
    /// Optional binary extradata (AVCDecoderConfigurationRecord, OpusHead, etc.)
    pub codec_private: Option<Vec<u8>>,
}
```

### 4.3 Trait Method Contracts (`crates/syncplay-demux/src/traits.rs`)
```rust
pub trait Demuxer: Send {
    /// Probes a byte stream to determine whether it matches this container format.
    fn probe(reader: &mut (dyn ReadSeek + Send)) -> Result<bool, DemuxError>;

    /// Initializes the demuxer, parses headers, and extracts container metadata and all tracks.
    fn init(&mut self, reader: Box<dyn ReadSeek + Send>) -> Result<ContainerInfo, DemuxError>;

    /// Reads the next sequential media packet from the container (interleaving tracks).
    fn read_packet(&mut self) -> Result<Option<DemuxPacket>, DemuxError>;

    /// Seeks to the nearest keyframe preceding or at `target_pts_ms`, returning the landed PTS.
    fn seek_to(&mut self, target_pts_ms: u64) -> Result<u64, DemuxError>;
}
```

---

## 5. Worker Task Breakdown & Implementation Roadmap

To execute Phase 2 R1 with zero drift and zero merge conflicts, the work is organized into 3 concrete, isolated tasks:

```
┌────────────────────────────────────────────────────────────────────────┐
│                   PHASE 2 R1 IMPLEMENTATION ROADMAP                    │
└────────────────────────────────────────────────────────────────────────┘
                                    │
         ┌──────────────────────────┴──────────────────────────┐
         ▼                                                     ▼
┌─────────────────────────────────┐   ┌─────────────────────────────────┐
│ Worker R1-A: Matroska/EBML      │   │ Worker R1-B: ISOBMFF/MP4        │
│ (`crates/syncplay-demux/ebml.rs`)│   │ (`crates/syncplay-demux/isobmff`)│
│ - Segment & Info parser         │   │ - Complete moov/trak parser     │
│ - Tracks / TrackEntry parser    │   │ - Multi-track state manager     │
│ - SimpleBlock/BlockGroup lacing │   │ - Interleaved mdat packet reader│
│ - Cues index parser & seeking   │   │ - stss/stts seeking             │
└─────────────────────────────────┘   └─────────────────────────────────┘
         │                                                     │
         └──────────────────────────┬──────────────────────────┘
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│ Worker R1-C: Synthetic Vectors & End-to-End Verification Harness       │
│ - In-tree full MKV (SYNTHETIC_FULL_MKV) & MP4 (SYNTHETIC_FULL_MP4)     │
│ - Comprehensive unit & integration tests in syncplay-demux            │
│ - CLI integration test (`syncplay-alt inspect`)                        │
│ - Verify zero warnings, intent compliance (100%), and phase 1 script   │
└────────────────────────────────────────────────────────────────────────┘
```

### Detailed Worker Task Specifications:

#### Worker R1-A: Matroska/EBML Engine Extension
- **Target File**: `crates/syncplay-demux/src/ebml.rs`
- **Key Tasks**:
  1. Implement `parse_segment_info`: reads `TimestampScale` (`0x2AD7B1`) and `Duration` (`0x4489`).
  2. Implement `parse_tracks`: loops over `TrackEntry` (`0xAE`), populating video (`0xE0`), audio (`0xE1`), and subtitle tracks.
  3. Implement `parse_cues`: parses `CuePoint` (`0xBB`) into `Vec<CueEntry>` for seeking.
  4. Extend `read_packet`:
     - Support `SimpleBlock` (`0xA3`) and `BlockGroup` (`0xA0`) with `ReferenceBlock` keyframe deduction.
     - Implement lacing unpacker (Xiph, Fixed, EBML) buffering into `VecDeque<DemuxPacket>`.
  5. Implement `seek_to`: binary search on `CueEntry`, reposition reader to cluster, flush `packet_queue`, return landed PTS.

#### Worker R1-B: ISOBMFF/MP4 Engine Extension
- **Target File**: `crates/syncplay-demux/src/isobmff.rs`
- **Key Tasks**:
  1. Implement complete `moov` parser:
     - Iterates through all child `trak` boxes.
     - Parses `tkhd`, `mdia/mdhd`, `mdia/hdlr`, and `minf/stbl`.
     - Populates per-track `SampleTableResolver` (`stts`, `ctts`, `stss`, `stsc`, `stsz`, `stco`, `co64`).
  2. Implement `Mp4TrackState` and precompute cumulative DTS/PTS arrays.
  3. Implement `read_packet`:
     - Select track with minimum next sample file offset (`compute_sample_offset`).
     - Seek reader and read payload from `mdat`.
     - Compute PTS/DTS in milliseconds and keyframe flag.
     - Advance track sample pointer.
  4. Implement `seek_to`:
     - Resolve keyframe in `stss` on primary video track.
     - Align audio tracks to preceding sample PTS.
     - Seek reader to lowest chunk offset.
     - Return landed keyframe PTS.

#### Worker R1-C: Synthetic Test Vectors & Verification Harness
- **Target Files**: `crates/syncplay-demux/src/synthetic.rs`, `crates/syncplay-demux/tests/demux_tests.rs`, `crates/syncplay-demux/src/lib.rs`
- **Key Tasks**:
  1. Generate `SYNTHETIC_FULL_MKV` and `SYNTHETIC_FULL_MP4` byte constants containing valid multi-track streams.
  2. Write unit tests verifying:
     - Demuxing all video and audio packets sequentially with ascending PTS.
     - Keyframe seeking landing on target keyframe and resuming packet reads.
     - Error handling on corrupted and truncated boxes.
  3. Verify workspace compilation:
     - `cargo check --workspace --all-targets` (0 warnings).
     - `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings).
     - `cargo test --workspace` (100% pass rate).
     - `python3 scripts/intent_harness.py` (100.0% Intent Compliance).

---

## 6. Empirical Verification & Anti-Drift Invariants

Before any change is accepted for R1, all of the following verification commands MUST pass cleanly:

```bash
# 1. Dependency Blacklist Audit (Strict zero external media/database engines)
./scripts/audit_dependencies.sh

# 2. Workspace Compilation & Lint Checks (Zero warnings)
export PATH="$HOME/.cargo/bin:$PATH"
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings

# 3. Demuxer Unit & Integration Tests
cargo test -p syncplay-demux -- --nocapture

# 4. Empirical Challenge Test Suite
python3 tests/empirical_challenge/test_demux_codecs_clock.py

# 5. Pi Framework Intent Verification Harness (100% Compliance Guard)
python3 scripts/intent_harness.py

# 6. Standalone CLI Verification
cargo run -p syncplay-core -- inspect synthetic-mkv
cargo run -p syncplay-core -- inspect synthetic-mp4
```

---

## 7. Conclusion

The Phase 1 foundation established clean wire abstractions, bounds validation, and mathematical proof models. Implementing Phase 2 R1 container demuxing requires transforming `EbmlDemuxer` and `IsoBmffDemuxer` from static metadata inspectors into full multi-track packet extractors with frame lacing, sample table traversal, and keyframe seeking. By following the modular worker specifications outlined in this survey, the engineering team can deliver a zero-dependency, desktop-grade container demuxer conforming strictly to `AGENTS.md` and OpenSpec specifications.
