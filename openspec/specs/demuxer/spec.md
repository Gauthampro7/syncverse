# Capability Specification: Container Demuxing (`syncverse-demux`)

**Component**: `syncverse-demux`  
**Standard Compliance**: RFC 8794 (EBML), Matroska Specification, ISO/IEC 14496-12 (ISOBMFF)  
**Status**: Authoritative OpenSpec Specification  

---

## 1. Overview & Architectural Role

The `syncverse-demux` crate provides zero-dependency stream demuxing for Matroska (MKV/WebM) and ISO Base Media File Format (ISOBMFF / MP4) containers. In compliance with the project's zero-external-codec invariant, demuxing is implemented directly from raw byte streams without linking against `libavformat`, `gstreamer`, `nom-ebml`, or any external C/C++ or Rust parsing libraries.

The demuxer extracts compressed audio and video packets (`DemuxPacket`), parses container timing metadata (timescales, frame presentation timestamps, keyframe flags), and maps random-access seek tables (`Cues` in MKV, `stbl` in MP4) to enable deterministic seeking and pre-roll decoding.

---

## 2. Requirements & Scenarios

### Requirement: EBML Variable-Length Integer (VINT) Parsing
The demuxer SHALL decode variable-length integers (VINT) conforming to RFC 8794. The octet length of a VINT MUST be determined by the number of leading zero bits in the first octet plus one (from 1 to 8 octets). The parser MUST distinguish between an Element ID (where the length marker bit is retained) and an Element Data Size (where the length marker bit is masked out). When all data bits of a size VINT are set to `1`, the parser SHALL interpret the element data size as undefined/unknown.
If the available input slice length is less than the octet length indicated by the leading zeros, the parser MUST return `Err(VintError::UnexpectedEof { needed: usize, available: usize })`. Per RFC 8794 Section 4.1, if all data bits of an Element ID are set to 1 (e.g. `0xFF`, `0xFFFF`, `0xFFFFFF`, `0xFFFFFFFF`), the parser MUST return `Err(VintError::ReservedElementId)`.

#### Scenario: Parse Single-Octet and Multi-Octet VINTs
- **Given** a buffer containing single-octet and multi-octet VINT byte sequences:
  - `0x81` (1-octet VINT with marker bit at position 7)
  - `0x40 0x02` (2-octet VINT with marker bit at position 6)
  - `0x1F 0xFF 0xFF 0xFF` (4-octet VINT with all data bits set to 1)
- **When** the VINT parser processes each byte slice
- **Then**:
  - `0x81` yields `raw_id = 0x81`, `data_size = Some(1)`, and `octet_width = 1`
  - `0x40 0x02` yields `raw_id = 0x4002`, `data_size = Some(2)`, and `octet_width = 2`
  - `0x1F 0xFF 0xFF 0xFF` yields `raw_id = 0x1FFFFFFF`, `data_size = None` (Unknown Size), and `octet_width = 4`

#### Scenario: Reject Invalid Zero-Prefix Octet
- **Given** an invalid byte buffer starting with `0x00`
- **When** the VINT parser attempts to parse an integer
- **Then** the parser MUST return a `VintError::InvalidZeroPrefix` error and MUST NOT enter an infinite loop.

#### Scenario: Handle Truncated VINT and Reserved Element ID
- **Given** a 1-byte buffer `[0x01]` (indicating an 8-octet VINT)
- **When** the VINT parser parses the buffer
- **Then** it returns `Err(VintError::UnexpectedEof { needed: 8, available: 1 })`
- **Given** an Element ID buffer `[0xFF]` or `[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]`
- **When** parsing an Element ID
- **Then** the parser returns `Err(VintError::ReservedElementId)`.

---

### Requirement: Matroska Master Element & Header Traversal
The demuxer SHALL parse the top-level EBML header (`0x1A45DFA3`) and validate that `EBMLVersion` is $\le 1$, `EBMLMaxIDLength` is $\le 4$, and `DocType` is either `"matroska"` or `"webm"`. The parser SHALL navigate the root `Segment` (`0x18538067`) and extract container-level global metadata from `Info` (`0x1549A966`), including `TimestampScale` (`0x2AD7B1`, defaulting to 1,000,000 ns if absent) and container `Duration` (`0x4489`).

#### Scenario: Parse Synthetic EBML Header Vector
- **Given** the 48-byte in-tree synthetic EBML header test vector (36-byte EBML Header + 12-byte Segment Header):
  ```
  [0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x86, 0x81, 0x01, 0x42, 0xF7, 0x81, 0x01,
   0x42, 0xF2, 0x81, 0x04, 0x42, 0xF3, 0x81, 0x08, 0x42, 0x82, 0x88, b'm', b'a',
   b't', b'r', b'o', b's', b'k', b'a', 0x42, 0x87, 0x81, 0x04, 0x18, 0x53, 0x80,
   0x67, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
  ```
- **When** `EbmlDemuxer::probe` and `EbmlDemuxer::parse_headers` are executed
- **Then** probe returns `Ok(true)`
- **And** the parsed `DocType` is `"matroska"` with version `4`
- **And** the root `Segment` is recognized with unknown size (`None`)
- **And** the total length of the synthetic vector is verified to be exactly 48 bytes.

---

### Requirement: Matroska Track Declaration & Codec Identification
The demuxer SHALL parse child elements of `Tracks` (`0x1654AE6B`) and individual `TrackEntry` (`0xAE`) declarations. For each track, the parser MUST extract `TrackNumber` (`0xD7`), `TrackUID` (`0x73C5`), `TrackType` (`0x83`: `1` for Video, `2` for Audio), and `CodecID` (`0x86`). For video tracks, the parser MUST extract `VideoSettings` (`0xE0`) with `PixelWidth` (`0xB0`) and `PixelHeight` (`0xBA`). For audio tracks, the parser MUST extract `AudioSettings` (`0xE1`) with `SamplingFrequency` (`0xB5`) and `Channels` (`0x9F`).

#### Scenario: Identify Video and Audio Tracks in Matroska
- **Given** a Matroska track table containing:
  - Track 1: Type `1`, CodecID `"V_VP9"`, PixelWidth `1920`, PixelHeight `1080`
  - Track 2: Type `2`, CodecID `"A_OPUS"`, SamplingFrequency `48000.0`, Channels `2`
- **When** `parse_headers` parses the `Tracks` element
- **Then** `ContainerInfo::tracks` contains exactly 2 tracks
- **And** Track 1 is typed `TrackType::Video` with dimensions $1920 \times 1080$
- **And** Track 2 is typed `TrackType::Audio` with sample rate $48,000\text{ Hz}$ and 2 channels.

---

### Requirement: Matroska Cluster & SimpleBlock Bitstream Extraction
The demuxer SHALL parse `Cluster` (`0x1F43B675`) elements and extract packets from `SimpleBlock` (`0xA3`) payloads. The parser MUST calculate the presentation timestamp in nanoseconds ($\text{PTS}_{\text{ns}}$) and convert it to milliseconds ($\text{PTS}_{\text{ms}}$) using integer arithmetic:
$$\text{PTS}_{\text{ns}} = (\text{ClusterTimecode} + \text{BlockTimecode}) \times \text{TimestampScale}$$
$$\text{PTS}_{\text{ms}} = \left\lfloor \frac{\text{PTS}_{\text{ns}}}{1\,000\,000} \right\rfloor = \left\lfloor \frac{(\text{ClusterTimecode} + \text{BlockTimecode}) \times \text{TimestampScale}}{1\,000\,000} \right\rfloor$$
where $\text{ClusterTimecode}$ is `u64` cast to `i64`, $\text{BlockTimecode}$ is the 16-bit signed relative timestamp (`i16` sign-extended to `i64`), and $\text{TimestampScale}$ is nanoseconds per tick (`u64`, default $1,000,000$).
The parser MUST extract the track number VINT from the `SimpleBlock` header, read the 16-bit signed big-endian relative timestamp (`i16`), and extract the flags byte. Keyframe status MUST be determined by checking Bit 7 (`flags & 0x80 != 0`).

#### Scenario: Extract Keyframe SimpleBlock
- **Given** a Cluster with base timecode $10,000$ ticks and `TimestampScale = 1,000,000` ($1,000,000\text{ ns} = 1\text{ ms}$ per tick)
- **And** a SimpleBlock with Track VINT `0x81` (Track 1), relative timecode `0x00FA` (+250 ticks), and flags `0x80` (Keyframe flag set, no lacing)
- **When** `EbmlDemuxer::next_packet` reads the block
- **Then** $\text{PTS}_{\text{ns}} = (10,000 + 250) \times 1,000,000 = 10,250,000,000\text{ ns}$
- **And** the returned `DemuxPacket` has:
  - `track_id = 1`
  - `pts_ms = 10250`
  - `is_keyframe = true`
  - Payload buffer containing raw bitstream bytes matching the block data.

---

### Requirement: ISOBMFF Box Header Parsing & Hierarchy
The demuxer SHALL parse ISOBMFF box headers supporting both standard 32-bit box lengths and 64-bit `largesize` boxes (`size == 1`). If `size == 0`, the parser SHALL treat the box as extending to the end of the input stream.
The parser MUST enforce strict bounds validation:
1. For 64-bit `largesize` boxes (`size == 1`): `if size == 1 && largesize < 16 => Err(DemuxError::MalformedBox)`. Furthermore, `largesize` MUST NOT exceed `i64::MAX as u64` ($2^{63}-1$).
2. For standard 32-bit boxes: `if size != 0 && size < 8 => Err(DemuxError::MalformedBox)`.
3. If input buffer length is less than `header_size` (8 bytes for standard, 16 bytes for `largesize`), the parser MUST return `Err(DemuxError::UnexpectedEof)`.
For `FullBox` structures, the parser MUST read the 1-byte version and 3-byte flags following the box header.

#### Scenario: Parse Standard 32-bit and 64-bit Box Headers
- **Given** an 8-byte buffer `[0x00, 0x00, 0x00, 0x18, b'f', b't', b'y', b'p']`
- **When** `BoxHeader::parse` is invoked
- **Then** the header is parsed as FourCC `ftyp` with `header_size = 8` and `total_size = 24`
- **Given** a 16-byte buffer `[0x00, 0x00, 0x00, 0x01, b'm', b'd', b'a', b't', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]`
- **When** `BoxHeader::parse` is invoked
- **Then** the header is parsed as FourCC `mdat` with `header_size = 16` and `total_size = 4294967296` (4 GB).

#### Scenario: Reject Corrupted and Malformed Box Header Sizes
- **Given** an 8-byte buffer `[0x00, 0x00, 0x00, 0x05, b'm', b'd', b'a', b't']` (`size = 5`)
- **When** `BoxHeader::parse` is invoked
- **Then** the parser returns `Err(DemuxError::MalformedBox)`
- **Given** a 16-byte buffer `[0x00, 0x00, 0x00, 0x01, b'm', b'd', b'a', b't', 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08]` (`largesize = 8`)
- **When** `BoxHeader::parse` is invoked
- **Then** the parser returns `Err(DemuxError::MalformedBox)`.

#### Scenario: Parse Synthetic MP4 Header Vector
- **Given** the 132-byte in-tree synthetic MP4 header test vector containing `ftyp` (`isom`, `iso2`) and `moov/mvhd` (timescale 1,000 ticks/s, duration 10,000 ticks = 10.0s)
- **When** `IsoBmffDemuxer::probe` and `IsoBmffDemuxer::parse_headers` are executed
- **Then** probe returns `Ok(true)`
- **And** the container timescale is identified as $1000\text{ Hz}$
- **And** container duration is computed as $10,000\text{ ms}$ ($10.0\text{ seconds}$).

---

### Requirement: ISOBMFF Sample Table (`stbl`) Index Resolution
The demuxer SHALL construct a random-access sample index from the `moov/trak/mdia/minf/stbl` sub-boxes:
1. `stsd`: Sample description (codec FourCC `avc1`, `mp4a`, etc.).
2. `stts`: Time-to-Sample run-length table for Decode TimeStamps (DTS).
3. `ctts`: Composition Time-to-Sample offsets for Presentation TimeStamps (PTS).
4. `stss`: Sync Sample Table identifying 1-based keyframe sample numbers ($sync\_sample \in [1, N]$). The demuxer MUST convert these to 0-based sample indices: $sample\_index = sync\_sample - 1$.
5. `stsc`: Sample-to-Chunk mapping run-length entries $(first\_chunk, samples\_per\_chunk, sample_description_index)$. The demuxer MUST validate that $samples\_per\_chunk \ge 1$; if $samples\_per\_chunk == 0$, the parser MUST return `Err(DemuxError::CorruptSampleTable)`.
6. `stsz`: Uniform or per-sample byte lengths.
7. `stco` / `co64`: 32-bit or 64-bit absolute chunk offsets in the file.

All internal sample table indexing MUST be 0-based ($S \in [0, N-1]$). The parser MUST compute sample $S$ byte offset as:
$$\text{Offset}(S) = \text{ChunkOffset}[C] + \sum_{j=S-k}^{S-1} \text{size}(j)$$
where chunk $C$ and index within chunk $k$ ($k \in [0, samples\_per\_chunk - 1]$) are derived from `stsc`.

#### Scenario: Resolve Sample Location and PTS
- **Given** an MP4 sample table with:
  - `stts`: 1 entry with 10 samples of duration 100 ticks (timescale 1000 Hz)
  - `stsz`: uniform sample size of 500 bytes
  - `stsc`: 1 entry indicating Chunk 1 contains 10 samples
  - `stco`: Chunk 1 offset is at byte $10,000$
- **When** resolving sample index $S = 4$
- **Then** sample 4 size is $500\text{ bytes}$
- **And** sample 4 absolute file offset is $10,000 + (4 \times 500) = 12,000\text{ bytes}$
- **And** sample 4 presentation timestamp is $400\text{ ms}$.

---

### Requirement: Deterministic Container Seeking
The demuxer SHALL implement seeking (`seek_to(target_pts)`) using container seek indices (`Cues` in MKV, `stss` / `stts` in MP4). The demuxer MUST seek to the keyframe (sync sample) immediately preceding or equal to `target_pts`. The demuxer MUST return the exact presentation timestamp of the keyframe landed upon to allow the video decode pipeline to initiate pre-roll.

#### Scenario: Seek to Non-Keyframe Timestamp
- **Given** a Matroska container with keyframes at PTS $0\text{ ms}$, $2000\text{ ms}$, and $4000\text{ ms}$
- **When** `seek_to(2500)` is invoked
- **Then** the demuxer seeks to the keyframe at $2000\text{ ms}$
- **And** returns `Ok(MediaTime::from_millis(2000))`
- **And** subsequent calls to `next_packet()` return packets starting from the keyframe at $2000\text{ ms}$.
