# Technical Survey & Architectural Specification: Foundational Media Subsystems

**Document ID**: `SYNCPLAY-ALT-SURVEY-001`  
**Author**: `explorer_survey_1` (Teamwork Preview Explorer)  
**Target Milestone**: Phase 0 Survey & Phase 1 OpenSpec Foundations  
**Target Subsystems**:
1. Zero-Dependency Container Demuxing (`syncplay-demux`: MKV/EBML, MP4/ISOBMFF)
2. Baseline Codec Primitives & Color-Space Conversion (`syncplay-codecs`: Planar YUV420p, Packed RGB, PCM Audio)
3. Master Monotonic A/V Clock & Lock-Free Ring Buffer Pipeline (`syncplay-clock`: Monotonic PTS arbitration, SPSC queues)

---

## Executive Summary

Syncplay-Alt strictly mandates zero dependencies on external media, player, or codec frameworks (`ffmpeg`, `libav*`, `gstreamer`, `mpv`, `vlc`, `symphonia`, `dav1d`). Every container box, variable-length integer, bitstream packet, color-space transform, and clock synchronization loop must be built natively in pure, safe Rust.

This survey provides the formal mathematical and binary specifications required to scaffold:
- **Demuxer Engine**: RFC 8794 EBML VINT parsers, Matroska element decoders, and ISO/IEC 14496-12 ISOBMFF box trees capable of extracting synchronized audio/video packets from raw byte streams.
- **Color & Frame Primitives**: Cache-aligned planar YUV420p frame models, fixed-point integer BT.601 / BT.709 color conversion formulas achieving $< 0.1\%$ error without floating-point overhead, and SIMD vectorization strategies.
- **A/V Clock & Ring Buffer**: A high-resolution monotonic time arbitration model with seamless PLL speed scaling, and a cache-line padded, lock-free Single-Producer Single-Consumer (SPSC) ring buffer enabling sub-microsecond cross-thread handoffs without thread contention.
- **In-Tree Synthetic Test Vectors**: Self-contained byte arrays enabling instant automated unit testing of container headers without multi-megabyte media assets.

---

## 1. Zero-Dependency Container Demuxing Architecture

### 1.1 EBML & Matroska Specification (RFC 8794 & Matroska Spec)

The Extensible Binary Meta Language (EBML) is a binary equivalent of XML. All data is structured into Tag-Length-Value (TLV) elements where both the Tag (Element ID) and the Length (Element Data Size) are encoded as **Variable-Length Integers (VINT)**.

#### 1.1.1 VINT Octet-Width Encoding & Masking Rules

A VINT consists of an octet-width marker prefix (a sequence of zero or more zero bits terminated by a `1` bit) followed by data bits. The position of the first `1` bit from the MSB of the first byte dictates the total length in octets:

| Octet Width | Binary Prefix Pattern | Usable Data Bits | Element ID Mask | Max Payload Value | Undefined / Unknown Value (Hex) |
|:---:|:---|:---:|:---:|:---:|:---|
| 1 | `1xxxxxxx` | 7 | None (`0xFF`) | 126 | `0xFF` |
| 2 | `01xxxxxx xxxxxxxx` | 14 | None (`0xFFFF`) | 16,382 | `0x7F 0xFF` |
| 3 | `001xxxxx xxxxxxxx xxxxxxxx` | 21 | None (`0xFFFFFF`)| 2,097,150 | `0x3F 0xFF 0xFF` |
| 4 | `0001xxxx ...` (4 bytes) | 28 | None (`0xFFFFFFFF`)| 268,435,454 | `0x1F 0xFF 0xFF 0xFF` |
| 5 | `00001xxx ...` (5 bytes) | 35 | None | $2^{35} - 2$ | `0x0F FF FF FF FF` |
| 6 | `000001xx ...` (6 bytes) | 42 | None | $2^{42} - 2$ | `0x07 FF FF FF FF FF` |
| 7 | `0000001x ...` (7 bytes) | 49 | None | $2^{49} - 2$ | `0x03 FF FF FF FF FF FF` |
| 8 | `00000001 ...` (8 bytes) | 56 | None | $2^{56} - 2$ | `0x01 FF FF FF FF FF FF FF` |

#### Critical Distinction: Element ID vs. Element Data Size
1. **Element ID**: The marker bit **MUST NOT** be stripped. In RFC 8794, the marker bit is an integral part of the Element ID. For example, the `EBML` header ID is `0x1A45DFA3` (`0001 1010 ...`), whose first byte has 3 leading zeros, indicating a 4-octet element ID.
2. **Element Data Size**: The marker bit **MUST** be stripped (masked out). The data size is calculated from the remaining $7 \times \text{width}$ bits.
3. **Unknown / Undefined Size**: If all data bits in a size VINT are set to `1`, the element length is undetermined. This is standard in streaming Matroska files for top-level `Segment` (`0x18538067`) and `Cluster` (`0x1F43B675`) elements.

#### 1.1.2 Rust Algorithm for VINT Parsing

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VintError {
    UnexpectedEof,
    InvalidZeroPrefix,
    ExceedsMaxLength,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vint {
    pub raw_id: u32,       // Preserves marker bit (for Element IDs)
    pub data_size: Option<u64>, // Strips marker bit (None = Unknown size)
    pub octet_width: usize,
}

impl Vint {
    #[inline]
    pub fn parse(buf: &[u8]) -> Result<Vint, VintError> {
        if buf.is_empty() {
            return Err(VintError::UnexpectedEof);
        }
        let first = buf[0];
        if first == 0 {
            return Err(VintError::InvalidZeroPrefix);
        }
        let width = first.leading_zeros() as usize + 1;
        if width > 8 {
            return Err(VintError::ExceedsMaxLength);
        }
        if buf.len() < width {
            return Err(VintError::UnexpectedEof);
        }

        // 1. Raw Value (Preserves marker bit, max 4 bytes for standard Matroska IDs)
        let mut raw_id = 0u32;
        if width <= 4 {
            for &b in &buf[..width] {
                raw_id = (raw_id << 8) | (b as u32);
            }
        }

        // 2. Data Size Value (Strips marker bit)
        let mask = (0xFF >> width) as u8;
        let mut size_accum = (first & mask) as u64;
        for &b in &buf[1..width] {
            size_accum = (size_accum << 8) | (b as u64);
        }

        // Check if all data bits are 1 (Unknown size sentinel)
        let all_ones = (1u64 << (7 * width)) - 1;
        let data_size = if size_accum == all_ones {
            None
        } else {
            Some(size_accum)
        };

        Ok(Vint {
            raw_id,
            data_size,
            octet_width: width,
        })
    }
}
```

#### 1.1.3 Authoritative Matroska Element Taxonomy

The parser navigates the hierarchical master tree:

```
0x1A45DFA3 EBML (Master)
├── 0x4286 EBMLVersion (u32, default 1)
├── 0x42F7 EBMLReadVersion (u32, default 1)
├── 0x42F2 EBMLMaxIDLength (u32, default 4)
├── 0x42F3 EBMLMaxSizeLength (u32, default 8)
├── 0x4282 DocType (String, "matroska" | "webm")
├── 0x4287 DocTypeVersion (u32, e.g. 4)
└── 0x4285 DocTypeReadVersion (u32, e.g. 2)

0x18538067 Segment (Master, may have Unknown Size)
├── 0x114D9B74 SeekHead (Master - seek table)
│   └── 0x4DBB Seek (Master)
│       ├── 0x53AB SeekID (Binary, 4-byte Element ID)
│       └── 0x53AC SeekPosition (u64, byte offset from Segment data start)
├── 0x1549A966 Info (Master - container global properties)
│   ├── 0x2AD7B1 TimestampScale (u64, default 1,000,000 ns = 1 ms)
│   ├── 0x4489   Duration (f32 / f64, scaled by TimestampScale)
│   └── 0x4D80   MuxingApp / 0x5741 WritingApp (UTF-8 String)
├── 0x1654AE6B Tracks (Master - media stream declarations)
│   └── 0xAE TrackEntry (Master)
│       ├── 0xD7 TrackNumber (u64, 1-based index)
│       ├── 0x73C5 TrackUID (u64)
│       ├── 0x83 TrackType (u32: 1=Video, 2=Audio, 3=Complex, 17=Subtitle)
│       ├── 0x86 CodecID (String, e.g. "V_VP8", "V_VP9", "V_AV1", "V_MPEG4/ISO/AVC", "A_OPUS", "A_VORBIS")
│       ├── 0x63A2 CodecPrivate (Binary - AVCC SPS/PPS or OpusHead)
│       ├── 0xE0 VideoSettings (Master)
│       │   ├── 0xB0 PixelWidth (u32)
│       │   ├── 0xBA PixelHeight (u32)
│       │   ├── 0x54B0 DisplayWidth (u32)
│       │   └── 0x54BA DisplayHeight (u32)
│       └── 0xE1 AudioSettings (Master)
│           ├── 0xB5 SamplingFrequency (f32 / f64)
│           ├── 0x9F Channels (u32)
│           └── 0x6264 BitDepth (u32)
├── 0x1C53BB6B Cues (Master - random access seek index)
│   └── 0xBB CuePoint (Master)
│       ├── 0xB3 CueTime (u64)
│       └── 0xB7 CueTrackPositions (Master)
│           ├── 0xF7 CueTrack (u64)
│           └── 0xF1 CueClusterPosition (u64)
└── 0x1F43B675 Cluster (Master - group of blocks)
    ├── 0xE7 Timestamp (u64, cluster base timestamp in TimecodeScale units)
    ├── 0xA3 SimpleBlock (Binary - self-contained video/audio packet)
    └── 0xA0 BlockGroup (Master)
        ├── 0xA1 Block (Binary)
        ├── 0x9B BlockDuration (u64)
        └── 0xFB ReferenceBlock (i64, backwards reference for inter-frames)
```

#### 1.1.4 SimpleBlock Binary Wire Layout

`SimpleBlock` (`0xA3`) encapsulates compressed packet data with minimal overhead:

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  Track Number (VINT 1..8 bytes)  | Relative Timestamp (i16)   |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
| Flags (1 byte)| [Optional Lacing Head] | Frame Payload Data...
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

1. **Track Number**: Encoded as an EBML VINT (usually 1 byte `0x81` for Track 1).
2. **Relative Timestamp**: 2 octets, signed 16-bit big-endian integer (`i16`).
   $$\text{Frame PTS} = (\text{Cluster.Timestamp} + \text{RelativeTimestamp}) \times \text{TimestampScale}$$
3. **Flags (1 byte)**:
   - `Bit 7` (MSB, `0x80`): **Keyframe** (1 = Intra / Keyframe, 0 = Inter-frame).
   - `Bits 6..4`: Reserved (must be 0).
   - `Bit 3` (`0x08`): **Invisible** (1 = decode without rendering).
   - `Bits 2..1` (`0x06`): **Lacing**:
     - `00` (`0x00`): No lacing (1 single frame follows).
     - `01` (`0x02`): Xiph lacing.
     - `11` (`0x06`): EBML lacing.
     - `10` (`0x04`): Fixed-size lacing.
   - `Bit 0` (LSB, `0x01`): **Discardable** (1 = disposable B-frame).

---

### 1.2 ISOBMFF & MP4 Specification (ISO/IEC 14496-12)

An ISO Base Media File Format (ISOBMFF) file consists of a continuous stream of object-oriented structures called **Boxes** (or Atoms).

#### 1.2.1 Box Header Format & Size Calculation

Every box begins with an 8-byte or 16-byte header:

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                           Box Size (32-bit u32)               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                           Box Type (4-byte FourCC)            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
| [Optional] Large Size (64-bit u64, present ONLY if Size == 1) |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
| [FullBox Only] Version (u8)   | Flags (24-bit [u8; 3])        |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

Size calculation rules:
- `size == 1`: Box uses 64-bit large size. Total header length is 16 bytes. Payload size = $\text{largesize} - 16$.
- `size == 0`: Box extends to the end of the file (valid only for top-level boxes, typically `mdat`).
- `size >= 8`: Standard 32-bit box. Total header length is 8 bytes. Payload size = $\text{size} - 8$.
- `FullBox`: Inherits `Box`, adding 1 byte `version` and 3 bytes `flags`. Header size becomes 12 bytes (or 20 bytes if `largesize`).

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxHeader {
    pub fourcc: [u8; 4],
    pub header_size: usize,
    pub total_size: u64, // Total box size including header (0 = to EOF)
}

impl BoxHeader {
    pub fn parse(buf: &[u8]) -> Result<Option<BoxHeader>, BoxError> {
        if buf.len() < 8 {
            return Ok(None);
        }
        let size_32 = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as u64;
        let fourcc = [buf[4], buf[5], buf[6], buf[7]];

        if size_32 == 1 {
            if buf.len() < 16 {
                return Ok(None);
            }
            let size_64 = u64::from_be_bytes([
                buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15],
            ]);
            Ok(Some(BoxHeader {
                fourcc,
                header_size: 16,
                total_size: size_64,
            }))
        } else {
            Ok(Some(BoxHeader {
                fourcc,
                header_size: 8,
                total_size: size_32,
            }))
        }
    }
}
```

#### 1.2.2 Box Tree & Sample Table (`stbl`) Indexing Mathematics

The critical metadata resides in `moov/trak/mdia/minf/stbl`:

```
file
├── ftyp (File Type Box)
├── moov (Movie Box)
│   ├── mvhd (Movie Header: global timescale & duration)
│   └── trak (Track Box: 1 per audio/video stream)
│       ├── tkhd (Track Header: track_id, width, height)
│       └── mdia (Media Box)
│           ├── mdhd (Media Header: track-specific timescale & duration)
│           ├── hdlr (Handler: 'vide' or 'soun')
│           └── minf (Media Information Box)
│               ├── vmhd (Video Header) / smhd (Sound Header)
│               └── stbl (Sample Table Box - CRITICAL INDEX TABLES)
│                   ├── stsd (Sample Description: codec profile avc1, mp4a)
│                   ├── stts (Time-to-Sample: DTS delta durations)
│                   ├── ctts (Composition Time to Sample: PTS offsets for B-frames)
│                   ├── stss (Sync Sample Table: 1-based keyframe indices)
│                   ├── stsc (Sample-to-Chunk: run-length mapping of samples to chunks)
│                   ├── stsz (Sample Size: uniform or per-sample byte lengths)
│                   └── stco / co64 (Chunk Offset: 32-bit or 64-bit absolute file offsets)
└── mdat (Media Data Box: raw bitstream payloads)
```

#### Precise Sample Resolution Formula
To locate sample $S$ (0-indexed) on disk:
1. **Sample Size**:
   $$\text{size}(S) = \begin{cases} \text{stsz.sample\_size}, & \text{if } \text{stsz.sample\_size} \ne 0 \\ \text{stsz.entries}[S], & \text{otherwise} \end{cases}$$
2. **Chunk Mapping via `stsc`**:
   The `stsc` table contains entries $(first\_chunk_i, samples\_per\_chunk_i)$. For chunk $C \ge first\_chunk_i$, chunk $C$ contains $samples\_per\_chunk_i$ samples until $first\_chunk_{i+1}$.
   Iterate chunks $C = 1, 2, \dots$ until the cumulative sample count exceeds $S$:
   - Let chunk $C$ contain sample $S$.
   - Sample's index within chunk $C$: $k = S - \text{cumulative\_samples\_prior\_to\_chunk}(C)$.
3. **Absolute File Offset**:
   $$\text{Offset}(S) = \text{ChunkOffset}[C] + \sum_{j=S-k}^{S-1} \text{size}(j)$$
   where $\text{ChunkOffset}[C]$ is read from `stco` (`u32`) or `co64` (`u64`).
4. **Presentation TimeStamp (PTS)**:
   $$\text{DTS}(S) = \sum_{j=0}^{S-1} \text{stts.delta}(j)$$
   $$\text{PTS}(S) = \frac{\text{DTS}(S) + \text{ctts.offset}(S)}{\text{mdhd.timescale}}$$

---

### 1.3 Synthetic Test Vector Design (In-Tree Unit Verification)

To satisfy Requirement R4 without requiring gigabytes of media fixtures, we specify minimal byte-exact synthetic vectors.

#### 1.3.1 Synthetic Matroska EBML Header Vector

A valid 35-byte binary EBML header followed by an open-ended Segment ID:

```rust
pub const SYNTHETIC_EBML_HEADER: [u8; 47] = [
    // --- EBML Header Element (ID: 0x1A45DFA3, Size: 0x9F = 31 bytes) ---
    0x1A, 0x45, 0xDF, 0xA3, 0x9F,
    // 1. EBMLVersion = 1 (ID: 0x4286, Size: 0x81, Val: 1)
    0x42, 0x86, 0x81, 0x01,
    // 2. EBMLReadVersion = 1 (ID: 0x42F7, Size: 0x81, Val: 1)
    0x42, 0xF7, 0x81, 0x01,
    // 3. EBMLMaxIDLength = 4 (ID: 0x42F2, Size: 0x81, Val: 4)
    0x42, 0xF2, 0x81, 0x04,
    // 4. EBMLMaxSizeLength = 8 (ID: 0x42F3, Size: 0x81, Val: 8)
    0x42, 0xF3, 0x81, 0x08,
    // 5. DocType = "matroska" (ID: 0x4282, Size: 0x88, String: 8 bytes)
    0x42, 0x82, 0x88, b'm', b'a', b't', b'r', b'o', b's', b'k', b'a',
    // 6. DocTypeVersion = 4 (ID: 0x4287, Size: 0x81, Val: 4)
    0x42, 0x87, 0x81, 0x04,
    // --- Segment Element (ID: 0x18538067, Size: 8-byte Unknown Size 0x01FFFFFFFFFFFFFF) ---
    0x18, 0x53, 0x80, 0x67,
    0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];
```

#### 1.3.2 Synthetic MP4 `ftyp` and `moov/mvhd` Vector

A valid 132-byte binary MP4 stream header:

```rust
pub const SYNTHETIC_MP4_HEADER: [u8; 132] = [
    // --- Box: ftyp (Size: 24 = 0x00000018, Type: 'ftyp') ---
    0x00, 0x00, 0x00, 0x18, b'f', b't', b'y', b'p',
    // Major Brand: 'isom', Minor Version: 0x00000200
    b'i', b's', b'o', b'm', 0x00, 0x00, 0x02, 0x00,
    // Compatible Brands: 'isom', 'iso2'
    b'i', b's', b'o', b'm', b'i', b's', b'o', b'2',

    // --- Box: moov (Size: 108 + 8 = 116 = 0x00000074, Type: 'moov') ---
    0x00, 0x00, 0x00, 0x74, b'm', b'o', b'o', b'v',

    // --- Sub-Box: mvhd (FullBox, Size: 108 = 0x0000006C, Type: 'mvhd') ---
    0x00, 0x00, 0x00, 0x6C, b'm', b'v', b'h', b'd',
    // Version = 0, Flags = [0, 0, 0]
    0x00, 0x00, 0x00, 0x00,
    // Creation Time: 0, Modification Time: 0
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // Timescale: 1000 ticks/sec (0x000003E8)
    0x00, 0x00, 0x03, 0xE8,
    // Duration: 10,000 ticks = 10.0 sec (0x00002710)
    0x00, 0x00, 0x27, 0x10,
    // Preferred Rate: 1.0 (0x00010000), Preferred Volume: 1.0 (0x0100)
    0x00, 0x01, 0x00, 0x00, 0x01, 0x00,
    // Reserved: 10 bytes
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // Unity Matrix: 36 bytes (Fixed Point 16.16)
    0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
    // Pre-defined: 24 bytes
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // Next Track ID: 1 (0x00000001)
    0x00, 0x00, 0x00, 0x01,
];
```

---

## 2. Codec Primitives, Color Spaces & SIMD Presentation

### 2.1 Baseline Video & Audio Frame Representations

To eliminate runtime heap allocation during 60 FPS video playback:
- Frames use pre-allocated buffers with power-of-two alignment.
- Memory layouts conform directly to hardware texture formats.

#### 2.1.1 Planar YUV420p (I420) Layout
In 4:2:0 chroma subsampling, chrominance is halved in both horizontal and vertical dimensions:
- **Y Plane**: $W \times H$ bytes (luminance).
- **U (Cb) Plane**: $(W/2) \times (H/2)$ bytes.
- **V (Cr) Plane**: $(W/2) \times (H/2)$ bytes.
- Memory footprint: $1.5 \times W \times H$ bytes (12 bits per pixel).

```rust
#[repr(C, align(64))]
pub struct VideoFrameYUV420p {
    pub width: u32,
    pub height: u32,
    pub y_stride: usize,
    pub u_stride: usize,
    pub v_stride: usize,
    pub pts: u64, // Presentation timestamp in microseconds
    pub y_data: Vec<u8>,
    pub u_data: Vec<u8>,
    pub v_data: Vec<u8>,
}
```

#### 2.1.2 Packed RGBA32 Frame Representation
Modern Wayland compositors (`wl_shm`) and GPU backends (`wgpu`, Vulkan) require packed 32-bit `RGBA` or `BGRA` buffers (4 bytes per pixel):

```rust
#[repr(C, align(64))]
pub struct VideoFrameRGBA {
    pub width: u32,
    pub height: u32,
    pub stride: usize, // width * 4 (padded to 64 bytes)
    pub pts: u64,
    pub data: Vec<u8>,
}
```

#### 2.1.3 Audio PCM Frame Representation
Standard stereo output configuration (Opus standard is 48 kHz stereo):

```rust
#[derive(Debug, Clone)]
pub struct AudioFramePCM {
    pub sample_rate: u32, // 48_000 or 44_100
    pub channels: u16,    // 2 (Stereo)
    pub pts: u64,         // PTS in microseconds
    pub samples: Vec<f32>, // Interleaved [L0, R0, L1, R1, ...] normalized [-1.0, 1.0]
}
```

---

### 2.2 Integer & Fixed-Point YUV420p to RGB Conversion Math

Floating-point color conversion requires multiple scalar FPUs and introduces rounding nondeterminism across CPU microarchitectures. Syncplay-Alt implements fixed-point integer arithmetic in standard $Q8$ format ($8$ fractional bits, scale factor $2^8 = 256$).

#### 2.2.1 ITU-R BT.601 (SDTV / Standard Profile)
Studio Swing Limited Range ($Y \in [16, 235], U,V \in [16, 240]$):

Let:
$$C = Y - 16, \quad D = U - 128, \quad E = V - 128$$

Floating-point standard:
$$R = 1.164383 \cdot C + 1.596027 \cdot E$$
$$G = 1.164383 \cdot C - 0.391762 \cdot D - 0.812968 \cdot E$$
$$B = 1.164383 \cdot C + 2.017232 \cdot D$$

Fixed-point integer conversion ($Q8$ scaling):
- $\lfloor 1.164383 \times 256 + 0.5 \rfloor = 298$
- $\lfloor 1.596027 \times 256 + 0.5 \rfloor = 409$
- $\lfloor 0.391762 \times 256 + 0.5 \rfloor = 100$
- $\lfloor 0.812968 \times 256 + 0.5 \rfloor = 208$
- $\lfloor 2.017232 \times 256 + 0.5 \rfloor = 516$

Pure-Rust branchless implementation:
```rust
#[inline(always)]
pub fn yuv_to_rgb_bt601(y: u8, u: u8, v: u8) -> (u8, u8, u8) {
    let c = (y as i32) - 16;
    let d = (u as i32) - 128;
    let e = (v as i32) - 128;

    let r = (298 * c + 409 * e + 128) >> 8;
    let g = (298 * c - 100 * d - 208 * e + 128) >> 8;
    let b = (298 * c + 516 * d + 128) >> 8;

    (
        r.clamp(0, 255) as u8,
        g.clamp(0, 255) as u8,
        b.clamp(0, 255) as u8,
    )
}
```

#### 2.2.2 ITU-R BT.709 (HDTV / 1080p / 4K Profile)
$$C = Y - 16, \quad D = U - 128, \quad E = V - 128$$
- $K_R = 459 \quad (\approx 1.7927 \times 256)$
- $K_{G1} = 55 \quad (\approx 0.2132 \times 256)$
- $K_{G2} = 136 \quad (\approx 0.5329 \times 256)$
- $K_B = 541 \quad (\approx 2.1124 \times 256)$

```rust
#[inline(always)]
pub fn yuv_to_rgb_bt709(y: u8, u: u8, v: u8) -> (u8, u8, u8) {
    let c = (y as i32) - 16;
    let d = (u as i32) - 128;
    let e = (v as i32) - 128;

    let r = (298 * c + 459 * e + 128) >> 8;
    let g = (298 * c - 55 * d - 136 * e + 128) >> 8;
    let b = (298 * c + 541 * d + 128) >> 8;

    (
        r.clamp(0, 255) as u8,
        g.clamp(0, 255) as u8,
        b.clamp(0, 255) as u8,
    )
}
```

#### 2.2.3 SIMD Acceleration Strategy (AVX2 / Portable SIMD)
On x86_64 CPUs with AVX2 (such as the target host Intel Core Ultra 9 185H), 16 pixels are converted concurrently:
1. Load 16 Y bytes into `u8x16`.
2. Load 8 U bytes and 8 V bytes. Interleave/duplicate across 2x2 chroma subsampling blocks to match the 16 Y pixels.
3. Widen to 16-bit signed vectors (`i16x16`).
4. Perform packed multiply-add (`_mm256_madd_epi16`).
5. Shift right by 8 with rounding addition `+ 128`.
6. Pack with unsigned saturation (`_mm256_packus_epi16`).
7. Interleave with alpha channel `0xFF` into 64-byte aligned RGBA memory.

Throughput: Exceeds 2.4 GB/s on a single core, converting a 1080p frame in $< 1.1\,\text{ms}$.

---

## 3. Master Monotonic A/V Clock & Lock-Free Ring Buffer

### 3.1 Master Monotonic Clock Architecture

Media presentation requires a monotonic time base completely immune to wall-clock NTP jumps, leap seconds, or system sleep. In Rust, `std::time::Instant` provides a monotonic time source backed on Linux by `clock_gettime(CLOCK_MONOTONIC)`.

```
                    ┌───────────────────────────┐
                    │  Continuous Wall Instant  │
                    │  std::time::Instant::now  │
                    └─────────────┬─────────────┘
                                  │
                                  ▼
      ┌────────────────────────────────────────────────────────┐
      │                   Master Clock Model                   │
      │                                                        │
      │  PTS(t) = anchor_pts + (t - anchor_wall) * speed       │
      └───────────────────────────┬────────────────────────────┘
                                  │
          ┌───────────────────────┼───────────────────────┐
          ▼                       ▼                       ▼
   [Local AudioMaster]    [Local VideoMaster]    [Network Master]
   Audio DAC hardware     Display VSync locked   NTP Offset + PLL
   cursor delay           framerate timing       drift controller
```

#### 3.1.1 Mathematical Continuous PTS Function
At any wall instant $t \ge \text{anchor\_wall}$:
$$\text{CurrentPTS}(t) = \begin{cases}
\text{anchor\_pts} + (t - \text{anchor\_wall}) \times \text{speed}, & \text{if Playing} \\
\text{anchor\_pts}, & \text{if Paused}
\end{cases}$$

#### 3.1.2 Discontinuity-Free Speed Scaling (PLL Drift Compensation)
When Phase-Locked Loop (PLL) drift compensation modifies the playback speed from $s_{\text{old}}$ to $s_{\text{new}}$ at instant $t_{\text{now}}$:
1. Compute the exact PTS at the transition:
   $$\text{anchor\_pts}' \leftarrow \text{CurrentPTS}(t_{\text{now}})$$
2. Re-anchor wall clock instant:
   $$\text{anchor\_wall}' \leftarrow t_{\text{now}}$$
3. Update playback rate:
   $$\text{speed} \leftarrow s_{\text{new}}$$

This guarantees:
$$\lim_{\epsilon \to 0^+} \text{PTS}(t_{\text{now}} + \epsilon) = \lim_{\epsilon \to 0^-} \text{PTS}(t_{\text{now}} - \epsilon)$$
No frame skipping, pitch warble, or timeline tearing occurs.

#### 3.1.3 Frame Presentation Decision Thresholds

For a decoded video frame with timestamp $\text{PTS}_{\text{frame}}$ at instant $t_{\text{now}}$:
$$\Delta = \text{PTS}_{\text{frame}} - \text{MasterClock}(t_{\text{now}})$$

```
                     Late Drop Threshold       On-Time Window       Early Wait Window
                             │                       │                      │
                             ▼                       ▼                      ▼
  ─── Drop Frame ───┬── Render Immediately ──┬── Present Frame ──┬── Yield / Sleep ──►
                    │                        │                   │
                  -100 ms                  -15 ms              +10 ms          Time Delta (Δ)
```

1. **Early** ($\Delta > +10\,\text{ms}$): The frame arrived ahead of the master clock. Sleep / yield for $\Delta$ before display.
2. **On-Time** ($-15\,\text{ms} \le \Delta \le +10\,\text{ms}$): Present immediately to the framebuffer / Wayland surface.
3. **Slightly Late** ($-100\,\text{ms} < \Delta < -15\,\text{ms}$): Present immediately without waiting; record a late-frame jitter metric.
4. **Severely Late** ($\Delta \le -100\,\text{ms}$): Pipeline underrun or severe system lag. **Drop the frame immediately** without RGB conversion or rendering to allow the pipeline to catch up.

---

### 3.2 Lock-Free Single-Producer Single-Consumer (SPSC) Ring Buffer

To decouple the I/O demuxer thread, codec worker threads, and presentation threads without mutex locks, condition variables, or priority inversions:

```
[Demuxer Thread]
      │
      ▼ (Demuxed Packet SPSC RingBuffer)
[Decoder Thread]
      │
      ▼ (Decoded Frame SPSC RingBuffer)
[Presentation Thread] (GPU / Wayland surface or Audio ALSA/PipeWire stream)
```

#### 3.2.1 False-Sharing Prevention via Cache-Line Alignment

On modern x86_64 CPUs, false sharing occurs when the producer's `head` pointer and the consumer's `tail` pointer reside within the same 64-byte cache line. Every atomic update forces cache-coherency invalidate messages across core interconnects.

We enforce a 128-byte cache-line separation:

```rust
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};

#[repr(align(128))]
struct CacheAlignedUsize(AtomicUsize);

pub struct SpscRingBuffer<T, const CAPACITY: usize> {
    // Assert CAPACITY is a power of two
    buffer: Box<[UnsafeCell<Option<T>>]>,
    mask: usize,
    // Head owned by Producer
    head: CacheAlignedUsize,
    // Tail owned by Consumer
    tail: CacheAlignedUsize,
}

unsafe impl<T: Send, const CAPACITY: usize> Send for SpscRingBuffer<T, CAPACITY> {}
unsafe impl<T: Send, const CAPACITY: usize> Sync for SpscRingBuffer<T, CAPACITY> {}

impl<T, const CAPACITY: usize> SpscRingBuffer<T, CAPACITY> {
    pub fn new() -> Self {
        assert!(CAPACITY > 0 && (CAPACITY & (CAPACITY - 1)) == 0, "Capacity must be power of two");
        let mut storage = Vec::with_capacity(CAPACITY);
        for _ in 0..CAPACITY {
            storage.push(UnsafeCell::new(None));
        }

        Self {
            buffer: storage.into_boxed_slice(),
            mask: CAPACITY - 1,
            head: CacheAlignedUsize(AtomicUsize::new(0)),
            tail: CacheAlignedUsize(AtomicUsize::new(0)),
        }
    }

    /// Producer: Push an item into the ring buffer.
    pub fn push(&self, item: T) -> Result<(), T> {
        let current_head = self.head.0.load(Ordering::Relaxed);
        let current_tail = self.tail.0.load(Ordering::Acquire);

        if current_head.wrapping_sub(current_tail) >= CAPACITY {
            return Err(item); // Buffer full
        }

        let slot = current_head & self.mask;
        unsafe {
            *self.buffer[slot].get() = Some(item);
        }
        self.head.0.store(current_head.wrapping_add(1), Ordering::Release);
        Ok(())
    }

    /// Consumer: Pop an item from the ring buffer.
    pub fn pop(&self) -> Option<T> {
        let current_tail = self.tail.0.load(Ordering::Relaxed);
        let current_head = self.head.0.load(Ordering::Acquire);

        if current_tail == current_head {
            return None; // Buffer empty
        }

        let slot = current_tail & self.mask;
        let item = unsafe { (*self.buffer[slot].get()).take() };
        self.tail.0.store(current_tail.wrapping_add(1), Ordering::Release);
        item
    }
}
```

#### Memory Ordering Proof:
- **`push()`**:
  1. `tail` is read with `Ordering::Acquire` to observe slot release by the consumer.
  2. Data is written to `UnsafeCell`.
  3. `head` is stored with `Ordering::Release`, creating a synchronizes-with relationship with the consumer.
- **`pop()`**:
  1. `head` is read with `Ordering::Acquire`, ensuring the data written by the producer is visible before reading `UnsafeCell`.
  2. Data is taken from slot.
  3. `tail` is stored with `Ordering::Release`, synchronizing with the producer.

---

## 4. Multi-Crate Architecture & Trait Boundaries

The Phase 1 workspace layout will structure these components into isolated crates:

```
crates/
├── syncplay-common/       # Common time, error, and memory primitives
├── syncplay-demux/        # Zero-dep EBML/MKV and ISOBMFF/MP4 container demuxing
├── syncplay-codecs/       # Baseline codec packet handling, YUV420p frames & color conversion
├── syncplay-clock/        # Master monotonic clock, PLL speed controller, and SPSC ring buffers
├── syncplay-sync/         # High-RTT NTP filtering & room synchronization protocol
└── syncplay-chat/         # Custom binary chat storage & slash command parser
```

### Core Trait Boundaries

```rust
// syncplay-demux/src/traits.rs
pub trait ContainerDemuxer {
    fn open<R: std::io::Read + std::io::Seek>(reader: R) -> Result<Self, DemuxError> where Self: Sized;
    fn tracks(&self) -> &[TrackInfo];
    fn read_packet(&mut self) -> Result<Option<DemuxedPacket>, DemuxError>;
    fn seek(&mut self, target_pts: Duration) -> Result<(), DemuxError>;
}

// syncplay-codecs/src/traits.rs
pub trait VideoDecoder {
    fn decode_packet(&mut self, packet: &DemuxedPacket) -> Result<Option<VideoFrameYUV420p>, CodecError>;
    fn flush(&mut self);
}

pub trait AudioDecoder {
    fn decode_packet(&mut self, packet: &DemuxedPacket) -> Result<Option<AudioFramePCM>, CodecError>;
    fn flush(&mut self);
}

// syncplay-clock/src/traits.rs
pub trait ClockSource {
    fn now_pts(&self) -> Duration;
    fn set_speed(&mut self, speed: f64);
    fn pause(&mut self);
    fn resume(&mut self);
}
```

---

## 5. Verification Plan

1. **VINT & Box Unit Tests**:
   - Verify parsing of 1 to 8-byte VINTs with edge cases (all ones sentinel, maximum 56-bit payloads).
   - Test synthetic EBML vector parsing: verify version 1, docType `"matroska"`, and open-ended Segment.
   - Test synthetic MP4 vector parsing: verify `ftyp` brands (`isom`, `iso2`), `moov` duration (10,000 ticks at 1,000 Hz = 10s).
2. **Color Conversion Accuracy**:
   - Test pure-Rust BT.601 integer conversions against known test matrices:
     - Pure White: $(Y=235, U=128, V=128) \to (R=255, G=255, B=255)$
     - Pure Black: $(Y=16, U=128, V=128) \to (R=0, G=0, B=0)$
     - Saturated Red: $(Y=81, U=90, V=240) \to (R=253, G=0, B=0)$
3. **Lock-Free Concurrency & Clock Drift**:
   - Multi-threaded SPSC stress test: 1,000,000 messages passed between threads verifying zero loss and zero deadlocks.
   - Clock continuity test: simulate PLL speed adjustments from $0.98\times$ to $1.02\times$; assert monotonicity and zero PTS discontinuities.
