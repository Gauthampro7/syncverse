# Comprehensive Technical Survey Report: Pure-Rust Video & Audio Bitstream Decoding Engine (Phase 3 Requirement R3)

**Author**: `explorer_p3_survey_2`  
**Date**: 2026-10-25T15:35:00Z  
**Workspace**: `/home/gautham/syncplay-alt`  
**Target Milestone**: M1 (Features 1, 2, 3 in `orchestrator_3/PROJECT.md`)  
**Scope**: Pure safe Rust H.264/AVC bitstream parsing, audio packet demuxing and unpacking (PCM, Vorbis, Opus), and high-performance Q8 color conversion with terminal ANSI 24-bit TrueColor half-block rendering buffers.

---

## 1. Executive Summary & Problem Scope

Phase 3 requires transforming Syncplay-Alt into a standalone desktop application and interactive player for Fedora Linux with zero external media frameworks (no `ffmpeg`, `libav*`, `gstreamer`, `vlc`, `mpv`, or `symphonia`).

While Phase 2 implemented container demuxing (`syncplay-demux`), Q8 limited-range color space conversion (`syncplay-codecs`), monotonic clock re-anchoring (`syncplay-clock`), network synchronization (`syncplay-sync`), and binary chat logging (`syncplay-chat`), video frames in the presentation pipeline were synthetic solid-color buffers.

Requirement R3 establishes pure-Rust bitstream decoding and payload unpacking:
1. **H.264 / AVC Bitstream Parser**:
   - Parse Annex B NAL unit streams (`0x000001` and `0x00000001` start codes) and AVCC length-prefixed NAL units.
   - Strip emulation prevention bytes (`0x000003` $\to$ RBSP).
   - Implement Exponential-Golomb bitstream decoders: unsigned `ue(v)`, signed `se(v)`, truncated `te(v)`.
   - Decode Sequence Parameter Sets (SPS) and Picture Parameter Sets (PPS).
   - Parse slice headers (`first_mb_in_slice`, `slice_type`, `frame_num`, `pic_order_cnt_lsb`).
   - Reconstruct parsed video frames into planar `VideoFrameYUV420p` respecting 16-byte row stride alignment and 64-byte allocation bounds.
2. **Audio Packet Demuxing & Payload Unpacking**:
   - Extract audio packets from MKV (`A_PCM/*`, `A_OPUS`, `A_VORBIS`) and MP4 (`sowt`, `twos`, `in24`, `fl32`, `lpcm`, `Opus`, `vorb`).
   - Unpack PCM integer/float formats to normalized `f32` in linear range `[-1.0, 1.0]`.
   - Parse Opus TOC bytes (RFC 6716) and unpack packet payloads into `AudioFramePCM`.
   - Parse Vorbis identification headers and extract unpacked audio samples.
3. **Q8 Fixed-Point Color Space Conversion & Terminal TrueColor Buffers**:
   - Audit existing BT.601 and BT.709 Q8 fixed-point integer color space converters.
   - Implement ANSI 24-bit TrueColor terminal half-block (`▀`, `\u{2580}`) rendering buffers, enabling 2 vertical video pixels per character cell for 60fps terminal playback without flickering.

---

## 2. Existing Codebase Audit & Gap Analysis

### 2.1 `crates/syncplay-common`
- **Current State**:
  - `reader::ByteReader`: Zero-copy, bounds-checked slice reader (`read_u8`, `read_u16_be/le`, `read_u32_be/le`, `read_u64_be/le`, `read_i16_be`, `read_bytes`, `skip`, `seek`).
  - `reader::BitReader`: Bit-level stream reader (`read_bit`, `read_bits(count)`).
  - `crc::Crc32`: Table-driven IEEE 802.3 CRC32.
  - `time::MediaTime` / `WallTime`: Microsecond and nanosecond timestamps.
- **Identified Gaps**:
  - `BitReader` lacks Exponential-Golomb decoding routines (`read_ue`, `read_se`, `read_te`).
  - Lacks byte-alignment helper `byte_align()` and bitstream trailing bit verification (`rbsp_trailing_bits`).

### 2.2 `crates/syncplay-demux`
- **Current State**:
  - Matroska / EBML demuxer (`ebml.rs`) parses Header, Info, Tracks, Cues seek index, and Clusters with `SimpleBlock` and `Block` payloads.
  - Lacing unpacker (`unpack_lacing`) supports no lacing, Xiph lacing, fixed-size lacing, and EBML lacing.
  - ISOBMFF / MP4 demuxer (`isobmff.rs`) parses `ftyp`, `moov`, `trak`, `mdia`, `minf`, `stbl` (`stsd`, `stts`, `ctts`, `stss`, `stsc`, `stsz`, `stco`, `co64`), and `mdat`.
  - In `isobmff.rs` (lines 582–612), the parser already inspects `stsd` video entries for child boxes `avcC`, `hvcC`, `vpcC` and stores the raw bytes in `TrackInfo::codec_private`.
  - Synthetic generators (`synthetic.rs`) generate 48-byte EBML, 132-byte MP4, and full multi-track test vectors with video and audio packets.
- **Identified Gaps**:
  - Demuxed packets contain raw bytes (`DemuxPacket.data`). In MP4, video packets are in AVCC format (4-byte length prefixes); in MKV, video packets may be AVCC or Annex B.
  - No demux-level packet classifier or unpacker to convert raw compressed packets into codec frames.

### 2.3 `crates/syncplay-codecs`
- **Current State**:
  - `frame.rs`:
    - `AlignedBuffer`: 64-byte aligned memory allocation with `#[repr(align(64))]` and drop cleanup.
    - `VideoFrameYUV420p`: 3 planar buffers (`y_plane`, `u_plane`, `v_plane`), 16-byte row stride alignment `(width + 15) & !15`, and $+16$B SIMD tail padding.
    - `VideoFrameRGBA`: 32-bit packed RGBA buffer with 64-byte row alignment and Alpha pre-initialized to 255.
    - `AudioFramePCM`: Interleaved `f32` audio samples bounded to $[-1.0, 1.0]$, sample rate, channel count, and PTS.
  - `color.rs`:
    - `yuv_to_rgb_bt601` and `yuv_to_rgb_bt709`: Branchless clamping Q8 fixed-point integer formulas with rounding bias $+128$.
    - `convert_yuv420p_to_rgba`: Zero-allocation hot loop with $2\times 2$ chroma reuse kernel.
- **Identified Gaps**:
  - Zero H.264 bitstream parsing code.
  - Zero audio bitstream or PCM payload unpacking code.
  - Lacks terminal TrueColor rendering buffer and ANSI escape sequence formatter.

### 2.4 `crates/syncplay-core`
- **Current State**:
  - `PlayerCoordinator::run_presentation_pipeline`: Currently demuxes a packet, but creates a dummy solid-color YUV frame filled with `128` instead of decoding the demux packet bitstream.
- **Identified Gaps**:
  - Needs to pass demuxed video packets through `H264Decoder` and audio packets through `AudioDecoder` before feeding the clock ring buffers and OSD renderer.

---

## 3. Subsystem 1: Pure-Rust H.264 / AVC Bitstream Parser & Reconstruction

### 3.1 NAL Unit Demarcation & Start Code Scanning
H.264 bitstreams are formatted in one of two ways:
1. **Annex B Byte Stream**:
   - NAL units are delimited by 3-byte prefix `[0x00, 0x00, 0x01]` or 4-byte prefix `[0x00, 0x00, 0x00, 0x01]`.
   - 4-byte prefixes are required before SPS, PPS, and the first slice of an access unit; 3-byte prefixes may precede secondary slices.
2. **AVCC Format (MP4 / Matroska `codec_private`)**:
   - Each NAL unit is preceded by a big-endian length prefix of $N$ bytes ($N \in \{1, 2, 4\}$, given by `length_size_minus_one + 1` from `avcC`). Standard MP4 uses $N = 4$.

#### Annex B Scanner Algorithm:
```rust
pub struct NalUnit<'a> {
    pub header: NalHeader,
    pub payload: &'a [u8], // Raw payload slice including header byte
}

pub struct AnnexBScanner<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> AnnexBScanner<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn next_nal(&mut self) -> Option<&'a [u8]> {
        // Fast scan for 0x00, 0x00, 0x01 or 0x00, 0x00, 0x00, 0x01
        // Skip leading zeros
        while self.pos < self.data.len() && self.data[self.pos] == 0 {
            self.pos += 1;
        }
        if self.pos >= self.data.len() {
            return None;
        }
        // Find start code prefix
        ...
    }
}
```

### 3.2 Emulation Prevention Byte Stripping (RBSP Un-Escaping)
Per ITU-T H.264 Section 7.3.1, when `[0x00, 0x00, 0x03]` occurs in the NAL payload, the `0x03` byte (emulation prevention byte) MUST be discarded if followed by `0x00`, `0x01`, `0x02`, or `0x03`.

```rust
pub fn unescape_rbsp(nal_payload: &[u8]) -> Vec<u8> {
    let mut rbsp = Vec::with_capacity(nal_payload.len());
    let mut i = 0;
    while i < nal_payload.len() {
        if i + 2 < nal_payload.len() 
            && nal_payload[i] == 0x00 
            && nal_payload[i + 1] == 0x00 
            && nal_payload[i + 2] == 0x03 
        {
            rbsp.push(0x00);
            rbsp.push(0x00);
            i += 3; // Drop 0x03
            // If next byte exists and <= 0x03, it will be pushed in next iteration
        } else {
            rbsp.push(nal_payload[i]);
            i += 1;
        }
    }
    rbsp
}
```

### 3.3 NAL Unit Header
The first byte of every NAL unit encodes:
- `forbidden_zero_bit`: 1 bit (bit 7). MUST be 0. If 1, bitstream is corrupted.
- `nal_ref_idc`: 2 bits (bits 5–6). Indicates whether the NAL unit is a reference picture (`> 0`) or disposable (`0`).
- `nal_unit_type`: 5 bits (bits 0–4).

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NalUnitType {
    Unspecified = 0,
    SliceNonIdr = 1,
    SliceDataPartitionA = 2,
    SliceDataPartitionB = 3,
    SliceDataPartitionC = 4,
    SliceIdr = 5,
    Sei = 6,
    Sps = 7,
    Pps = 8,
    Aud = 9,
    EndOfSequence = 10,
    EndOfStream = 11,
    FillerData = 12,
    SpsExtension = 13,
    PrefixNalUnit = 14,
    SubsetSps = 15,
    Other(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NalHeader {
    pub forbidden_zero_bit: bool,
    pub nal_ref_idc: u8,
    pub unit_type: NalUnitType,
}

impl NalHeader {
    pub fn parse(byte: u8) -> Result<Self, CodecError> {
        let fzb = (byte & 0x80) != 0;
        if fzb {
            return Err(CodecError::InvalidNal("forbidden_zero_bit is 1".into()));
        }
        let ref_idc = (byte >> 5) & 0x03;
        let u_type = match byte & 0x1F {
            0 => NalUnitType::Unspecified,
            1 => NalUnitType::SliceNonIdr,
            2 => NalUnitType::SliceDataPartitionA,
            3 => NalUnitType::SliceDataPartitionB,
            4 => NalUnitType::SliceDataPartitionC,
            5 => NalUnitType::SliceIdr,
            6 => NalUnitType::Sei,
            7 => NalUnitType::Sps,
            8 => NalUnitType::Pps,
            9 => NalUnitType::Aud,
            10 => NalUnitType::EndOfSequence,
            11 => NalUnitType::EndOfStream,
            12 => NalUnitType::FillerData,
            13 => NalUnitType::SpsExtension,
            14 => NalUnitType::PrefixNalUnit,
            15 => NalUnitType::SubsetSps,
            other => NalUnitType::Other(other),
        };
        Ok(Self {
            forbidden_zero_bit: fzb,
            nal_ref_idc: ref_idc,
            unit_type: u_type,
        })
    }
}
```

### 3.4 Exponential-Golomb Bitstream Decoding Math
Exponential-Golomb coding (ITU-T H.264 Section 9.1):
1. **Unsigned Exponential-Golomb (`ue(v)`)**:
   - Number of leading zero bits $M$.
   - Read 1 '1' bit.
   - Read $M$ bits as binary value $\text{INFO}$.
   $$\text{code\_num} = 2^M - 1 + \text{INFO}$$
2. **Signed Exponential-Golomb (`se(v)`)**:
   - Derived from `ue(v)`:
     $$k = \text{code\_num}$$
     $$\text{value} = \begin{cases} \lceil k/2 \rceil & \text{if } k \text{ is odd} \\ -k/2 & \text{if } k \text{ is even (} k > 0) \\ 0 & \text{if } k = 0 \end{cases}$$
     Bitwise formula: `if (k & 1) != 0 { ((k + 1) / 2) as i32 } else { -((k / 2) as i32) }`.
3. **Truncated Exponential-Golomb (`te(v)`)**:
   - If `max == 1`: read 1 bit, return `!bit as u32`.
   - If `max > 1`: return `ue(v)`.

Implementation added to `syncplay-common::BitReader`:
```rust
impl<'a> BitReader<'a> {
    pub fn read_ue(&mut self) -> Result<u32, CommonError> {
        let mut leading_zeros = 0usize;
        while !self.read_bit()? {
            leading_zeros += 1;
            if leading_zeros > 31 {
                return Err(CommonError::InvalidData("Exp-Golomb leading zeros > 31".into()));
            }
        }
        if leading_zeros == 0 {
            return Ok(0);
        }
        let info = self.read_bits(leading_zeros)?;
        let code_num = ((1u64 << leading_zeros) - 1) + info;
        if code_num > u32::MAX as u64 {
            return Err(CommonError::InvalidData("Exp-Golomb code_num exceeds u32".into()));
        }
        Ok(code_num as u32)
    }

    pub fn read_se(&mut self) -> Result<i32, CommonError> {
        let k = self.read_ue()?;
        if (k & 1) != 0 {
            Ok(((k + 1) / 2) as i32)
        } else {
            Ok(-((k / 2) as i32))
        }
    }
}
```

### 3.5 SPS Parameter Decoding & Dimension Calculation
From ITU-T H.264 Section 7.3.2.1.1:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sps {
    pub profile_idc: u8,
    pub constraint_set_flags: u8,
    pub level_idc: u8,
    pub seq_parameter_set_id: u32,
    pub chroma_format_idc: u32, // default 1 (4:2:0)
    pub separate_colour_plane_flag: bool,
    pub bit_depth_luma_minus8: u32,
    pub bit_depth_chroma_minus8: u32,
    pub log2_max_frame_num_minus4: u32,
    pub pic_order_cnt_type: u32,
    pub log2_max_pic_order_cnt_lsb_minus4: u32,
    pub delta_pic_order_always_zero_flag: bool,
    pub offset_for_non_ref_pic: i32,
    pub offset_for_top_to_bottom_field: i32,
    pub num_ref_frames_in_pic_order_cnt_cycle: u32,
    pub max_num_ref_frames: u32,
    pub gaps_in_frame_num_value_allowed_flag: bool,
    pub pic_width_in_mbs_minus1: u32,
    pub pic_height_in_map_units_minus1: u32,
    pub frame_mbs_only_flag: bool,
    pub mb_adaptive_frame_field_flag: bool,
    pub direct_8x8_inference_flag: bool,
    pub frame_cropping_flag: bool,
    pub frame_crop_left_offset: u32,
    pub frame_crop_right_offset: u32,
    pub frame_crop_top_offset: u32,
    pub frame_crop_bottom_offset: u32,
    pub vui_parameters_present_flag: bool,
}
```

#### Exact Visible Dimension Equations:
For 4:2:0 chroma subsampling (`chroma_format_idc == 1`):
$$\text{CropUnitX} = 2$$
$$\text{CropUnitY} = 2 \times (2 - \text{frame\_mbs\_only\_flag} \text{ as } u32)$$
$$\text{width} = (\text{pic\_width\_in\_mbs\_minus1} + 1) \times 16 - (\text{frame\_crop\_left\_offset} + \text{frame\_crop\_right\_offset}) \times \text{CropUnitX}$$
$$\text{height} = (\text{pic\_height\_in\_map\_units\_minus1} + 1) \times 16 \times (2 - \text{frame\_mbs\_only\_flag} \text{ as } u32) - (\text{frame\_crop\_top\_offset} + \text{frame\_crop\_bottom\_offset}) \times \text{CropUnitY}$$

### 3.6 PPS Parameter Decoding
From ITU-T H.264 Section 7.3.2.2:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pps {
    pub pic_parameter_set_id: u32,
    pub seq_parameter_set_id: u32,
    pub entropy_coding_mode_flag: bool, // false: CAVLC, true: CABAC
    pub bottom_field_pic_order_in_frame_present_flag: bool,
    pub num_slice_groups_minus1: u32,
    pub num_ref_idx_l0_default_active_minus1: u32,
    pub num_ref_idx_l1_default_active_minus1: u32,
    pub weighted_pred_flag: bool,
    pub weighted_bipred_idc: u8,
    pub pic_init_qp_minus26: i32,
    pub pic_init_qs_minus26: i32,
    pub chroma_qp_index_offset: i32,
    pub deblocking_filter_control_present_flag: bool,
    pub constrained_intra_pred_flag: bool,
    pub redundant_pic_cnt_present_flag: bool,
}
```

### 3.7 Slice Header Parsing
From ITU-T H.264 Section 7.3.3:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceType {
    P = 0,
    B = 1,
    I = 2,
    Sp = 3,
    Si = 4,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceHeader {
    pub first_mb_in_slice: u32,
    pub slice_type: SliceType,
    pub pic_parameter_set_id: u32,
    pub frame_num: u64,
    pub idr_pic_id: Option<u32>,
    pub pic_order_cnt_lsb: Option<u32>,
    pub delta_pic_order_cnt_bottom: Option<i32>,
}
```
Slice type normalization: `slice_type % 5` where `0=P, 1=B, 2=I, 3=SP, 4=SI`.

### 3.8 Frame Reconstruction into `VideoFrameYUV420p`
The video decoder stores active SPS and PPS tables:
```rust
pub struct H264Decoder {
    sps_table: BTreeMap<u32, Sps>,
    pps_table: BTreeMap<u32, Pps>,
    active_sps_id: Option<u32>,
    active_pps_id: Option<u32>,
}
```
When an IDR or non-IDR slice is parsed:
1. Active SPS provides visible `width` and `height`.
2. A `VideoFrameYUV420p` is initialized using `VideoFrameYUV420p::new(width, height, pts_ms)`.
3. Row strides ($S_y, S_u, S_v$) are aligned to 16 bytes: `(width + 15) & !15`.
4. Macroblock data ($16 \times 16$ luma and $8 \times 8$ chroma) is written directly into plane slices with branchless bounds checks.

---

## 4. Subsystem 2: Audio Packet Demuxing & Payload Unpacking

### 4.1 PCM Audio Unpacking
Uncompressed PCM packets contain interleaved raw integer or float audio samples:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcmFormat {
    U8,
    I16Le,
    I16Be,
    I24Le,
    I24Be,
    I32Le,
    I32Be,
    F32Le,
    F32Be,
}
```
Normalization formulas to linear range `[-1.0, 1.0]`:
- `I16Le/Be`: $s = \text{clamp}(v_{\text{i16}} \text{ as f32} / 32768.0, -1.0, 1.0)$
- `I24Le/Be`: 3 bytes sign-extended to `i32`: $s = \text{clamp}(v_{\text{i24}} \text{ as f32} / 8388608.0, -1.0, 1.0)$
- `I32Le/Be`: $s = \text{clamp}(v_{\text{i32}} \text{ as f64} / 2147483648.0 \text{ as f32}, -1.0, 1.0)$
- `U8`: $s = ((v_{\text{u8}} \text{ as f32}) - 128.0) / 128.0$
- `F32Le/Be`: $s = \text{clamp}(f, -1.0, 1.0)$ (with NaN checked and rejected).

```rust
pub fn unpack_pcm(
    data: &[u8],
    format: PcmFormat,
    channels: u16,
    sample_rate: u32,
    pts_ms: u64,
) -> Result<AudioFramePCM, CodecError> {
    if channels == 0 || sample_rate == 0 {
        return Err(CodecError::InvalidAudio("channels and sample_rate must be > 0".into()));
    }
    // Parse samples according to format
    ...
    AudioFramePCM::new(sample_rate, channels, pts_ms, samples)
}
```

### 4.2 Opus Packet Demuxing & TOC Parsing (RFC 6716)
The first byte of an Opus packet is the TOC (Table of Contents):
- `config`: `(toc >> 3) & 0x1F` (5 bits) $\implies$ determines audio bandwidth (NB, MB, WB, SWB, FB) and frame duration (2.5ms, 5ms, 10ms, 20ms, 40ms, 60ms).
- `s_flag` (stereo): `(toc >> 2) & 0x01` (1 bit) $\implies 0 = \text{mono}, 1 = \text{stereo}$.
- `c_flag` (frame count code): `toc & 0x03` (2 bits):
  - 0: 1 frame in packet.
  - 1: 2 frames of equal size.
  - 2: 2 frames of different sizes.
  - 3: Arbitrary number of frames ($M$ frames).

#### Exact Sample Count Derivation:
Standard 20ms Opus frame at 48,000 Hz contains exactly:
$$\text{samples\_per\_channel} = \frac{48000 \times 20}{1000} = 960\text{ samples}$$
For stereo, total float samples = $960 \times 2 = 1920\text{ samples}$.
The unpacker extracts duration, parses packet headers, and produces valid `AudioFramePCM` structures matching PTS timestamps.

### 4.3 Vorbis Identification Header & Packet Unpacking (RFC 5215)
- Vorbis header packet 1: `[0x01, b'v', b'o', b'r', b'b', b'i', b's']`
  - `vorbis_version`: 32-bit LE (must be 0)
  - `audio_channels`: 8-bit unsigned ($> 0$)
  - `audio_sample_rate`: 32-bit LE ($> 0$)
  - `bitrate_maximum`, `bitrate_nominal`, `bitrate_minimum`: 32-bit LE
  - `blocksize_0`, `blocksize_1`: 4 bits each ($2^N$, $64 \le N \le 8192$)
  - `framing_flag`: 1 bit (must be non-zero).

---

## 5. Subsystem 3: Q8 Color Space Converter & Terminal TrueColor Rendering

### 5.1 Existing Q8 Color Space Converter Audit
In `crates/syncplay-codecs/src/color.rs`:
- ITU-R BT.601 limited-range:
  $$C = Y - 16, \quad D = U - 128, \quad E = V - 128$$
  $$R = \text{clamp}((298 \cdot C + 409 \cdot E + 128) \gg 8)$$
  $$G = \text{clamp}((298 \cdot C - 100 \cdot D - 208 \cdot E + 128) \gg 8)$$
  $$B = \text{clamp}((298 \cdot C + 516 \cdot D + 128) \gg 8)$$
- ITU-R BT.709 limited-range:
  $$R = \text{clamp}((298 \cdot C + 459 \cdot E + 128) \gg 8)$$
  $$G = \text{clamp}((298 \cdot C - 55 \cdot D - 136 \cdot E + 128) \gg 8)$$
  $$B = \text{clamp}((298 \cdot C + 541 \cdot D + 128) \gg 8)$$
- Performance: $2 \times 2$ unrolled chroma reuse processes 4 luma pixels per chroma load with zero heap allocations in the hot loop.
- Verification: Validated against floating-point reference within $\pm 1$ LSB.

### 5.2 Terminal ANSI 24-bit TrueColor Half-Block Rendering Buffers
Terminal video presentation uses Unicode upper half-block characters (`▀`, `\u{2580}`):
- **Foreground Color**: Sets color of upper half pixel `\x1b[38;2;R_top;G_top;B_topm`.
- **Background Color**: Sets color of lower half pixel `\x1b[48;2;R_bot;G_bot;B_botm`.
- **Character**: `▀`.
- **Result**: Each character cell displays **2 vertical pixels**. A terminal of $W \times H$ cells displays $W \times (2H)$ pixels!

#### Data Structures:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TerminalColorCell {
    pub top_r: u8,
    pub top_g: u8,
    pub top_b: u8,
    pub bot_r: u8,
    pub bot_g: u8,
    pub bot_b: u8,
}

#[derive(Debug, Clone)]
pub struct TerminalTrueColorBuffer {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<TerminalColorCell>,
}

impl TerminalTrueColorBuffer {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            cells: vec![TerminalColorCell::default(); cols * rows],
        }
    }

    /// Renders buffer to ANSI 24-bit TrueColor string with run-length color deduplication.
    pub fn render_ansi(&self, out: &mut String) {
        out.clear();
        let mut last_top = (0u8, 0u8, 0u8);
        let mut last_bot = (0u8, 0u8, 0u8);
        let mut initialized = false;

        for r in 0..self.rows {
            out.push_str("\x1b[H\x1b["); // cursor positioning or line feed
            for c in 0..self.cols {
                let cell = self.cells[r * self.cols + c];
                let top = (cell.top_r, cell.top_g, cell.top_b);
                let bot = (cell.bot_r, cell.bot_g, cell.bot_b);

                if !initialized || top != last_top {
                    use std::fmt::Write;
                    let _ = write!(out, "\x1b[38;2;{};{};{}m", top.0, top.1, top.2);
                    last_top = top;
                }
                if !initialized || bot != last_bot {
                    use std::fmt::Write;
                    let _ = write!(out, "\x1b[48;2;{};{};{}m", bot.0, bot.1, bot.2);
                    last_bot = bot;
                }
                initialized = true;
                out.push('▀');
            }
            out.push_str("\x1b[0m\n");
            initialized = false;
        }
    }
}
```

#### Converter from YUV420p to Terminal Buffer:
```rust
pub fn convert_yuv420p_to_terminal(
    src: &VideoFrameYUV420p,
    dst: &mut TerminalTrueColorBuffer,
    standard: ColorStandard,
) -> Result<(), CodecError> {
    // Scales src (W x H) into dst (cols x (rows * 2)) pixels using Q8 fixed-point integer math.
    // For each cell (col, row):
    // top pixel maps to y_src = row * 2, x_src = col
    // bot pixel maps to y_src = row * 2 + 1, x_src = col
    // Populates dst.cells[row * dst.cols + col]
    ...
}
```

---

## 6. Failure Modes, Edge Cases & Adversarial Defenses

| Failure Mode | Attack / Scenario | Defensive Mechanism |
|---|---|---|
| **Truncated NAL Stream** | Stream ends mid-NAL or mid-Exp-Golomb sequence | `BitReader` returns `UnexpectedEof { needed, available }`, no panics |
| **Start Code False Positive** | Payload contains `0x000001` without emulation prevention | Enforce `unescape_rbsp` unescaping before slice/header parsing |
| **Exp-Golomb Overflow** | More than 31 consecutive zero bits in `ue(v)` | Strict check `if leading_zeros > 31 => Err(CommonError::InvalidData)` |
| **Malformed SPS Cropping** | Crop offsets exceed picture width or height | Validate `crop_left + crop_right < width_raw`, clamp if invalid |
| **Audio Sample Out of Bounds** | Audio packet unpacks values outside $[-1.0, 1.0]$ or `NaN` | Strict clamp or error return in `AudioFramePCM::new` |
| **Terminal Odd Height** | Video height has odd number of rows | Bottom pixel on trailing row rendered as black padding `(0, 0, 0)` |
| **Terminal Resizing** | Terminal window resized during playback | Dynamic buffer reallocation without tearing or out-of-bounds access |

---

## 7. Concrete Test Vectors & Assertion Specs

### 7.1 Synthetic H.264 Annex B Stream Test Vector
A byte-exact Annex B stream containing:
1. **SPS NAL** (`0x67`): 320x240, Baseline profile (66), Level 3.1 (31), `pic_width_in_mbs = 20`, `pic_height_in_map_units = 15`.
2. **PPS NAL** (`0x68`): `pps_id = 0`, `sps_id = 0`, CAVLC.
3. **IDR Slice NAL** (`0x65`): `slice_type = 7` (all I-slice), `first_mb = 0`, with emulation prevention bytes `[0x00, 0x00, 0x03, 0x01]` to verify unescaping.

### 7.2 Synthetic Audio Packet Test Vectors
1. **16-bit Signed PCM Vector**:
   - `[0x00, 0x00, 0x00, 0x40, 0x00, 0x80, 0x00, 0xC0]` (4 stereo samples) $\implies$ verifies $(0.0, 0.5, -1.0, -0.5)$.
2. **Opus TOC Test Vector**:
   - `[0x78, ...]` (TOC byte `0b01111000`: CELT Fullband, 20ms duration = 960 samples, mono) $\implies$ verifies sample count and duration $\frac{960}{48000} = 0.020\text{s}$.

### 7.3 Terminal Half-Block TrueColor Verification
- A $2 \times 2$ frame with top row red/green, bottom row blue/white:
  - Cell (0, 0): top = `(255, 0, 0)`, bot = `(0, 0, 255)`.
  - Cell (1, 0): top = `(0, 255, 0)`, bot = `(255, 255, 255)`.
  - Verifies exact ANSI output: `\x1b[38;2;255;0;0m\x1b[48;2;0;0;255m▀\x1b[38;2;0;255;0m\x1b[48;2;255;255;255m▀\x1b[0m\n`.

---

## 8. Implementation Blueprint & Roadmap for Milestone M1

The survey recommends the following implementation sequence for Worker M1:
1. **Step 1**: In `crates/syncplay-common/src/reader.rs`, implement `read_ue()`, `read_se()`, `read_te()` on `BitReader`. Add unit tests.
2. **Step 2**: In `crates/syncplay-codecs`:
   - Add `src/h264/nal.rs`: Annex B scanner, AVCC parser, RBSP unescaping.
   - Add `src/h264/sps.rs`: SPS decoder and visible dimension arithmetic.
   - Add `src/h264/pps.rs`: PPS decoder.
   - Add `src/h264/slice.rs`: Slice header parser.
   - Add `src/h264/decoder.rs`: `H264Decoder` producing `VideoFrameYUV420p`.
3. **Step 3**: In `crates/syncplay-codecs`:
   - Add `src/audio/pcm.rs`: PCM format decoder.
   - Add `src/audio/opus.rs`: Opus TOC parser and packet unpacker.
   - Add `src/audio/vorbis.rs`: Vorbis header and packet unpacker.
   - Add `src/audio/decoder.rs`: `AudioDecoder` producing `AudioFramePCM`.
4. **Step 4**: In `crates/syncplay-codecs/src/color.rs`:
   - Add `TerminalColorCell`, `TerminalTrueColorBuffer`.
   - Add `convert_yuv420p_to_terminal` and `convert_rgba_to_terminal`.
5. **Step 5**: In `crates/syncplay-codecs`:
   - Export types in `src/lib.rs`.
   - Add unit and adversarial stress test suite in `tests/bitstream_decoders_test.rs`.
6. **Step 6**: Run verification commands:
   - `./scripts/audit_dependencies.sh`
   - `cargo test --workspace`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `python3 scripts/intent_harness.py`

This design strictly respects all architectural invariants: zero C/C++, pure safe Rust, zero warnings, and 100% intent compliance.
