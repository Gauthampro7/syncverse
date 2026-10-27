---
name: bitstream-vector-testing
description: Operational skill for generating in-tree synthetic EBML and MP4 test vectors and verifying bitstream parsers without media fixtures
version: 1.0.0
---

# Skill: Bitstream Vector Testing

## Overview
This skill provides the methodologies, bit-level vector definitions, and verification procedures for testing zero-dependency container demuxers (`syncverse-demux`) using synthetic, in-tree test vectors. Large multimedia files are prohibited; all container headers and stream packets must be generated deterministically in memory.

---

## 1. Synthetic Vector Generators

### 1.1 In-Tree 48-Byte EBML Matroska Header
Generate a valid, minimal Matroska container header byte array:
```rust
pub fn create_synthetic_ebml_header() -> Vec<u8> {
    vec![
        0x1A, 0x45, 0xDF, 0xA3, // EBML Header Element ID (0x1A45DFA3)
        0x9F,                   // Header Size = 31 bytes (VINT 0x1F | 0x80)
        0x42, 0x86, 0x81, 0x01, // EBMLVersion = 1
        0x42, 0xF7, 0x81, 0x01, // EBMLReadVersion = 1
        0x42, 0xF2, 0x81, 0x04, // EBMLMaxIDLength = 4
        0x42, 0xF3, 0x81, 0x08, // EBMLMaxSizeLength = 8
        0x42, 0x82, 0x88,       // DocType Element ID & Length (8 bytes)
        b'm', b'a', b't', b'r', b'o', b's', b'k', b'a',
        0x42, 0x87, 0x81, 0x04, // DocTypeVersion = 4
        0x18, 0x53, 0x80, 0x67, // Segment Element ID (0x18538067)
        0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF // Unknown Size (8-byte VINT)
    ]
}
```

### 1.2 Synthetic 132-Byte ISOBMFF MP4 Header
Generate an ISOBMFF box hierarchy (`ftyp`, `moov`, `mvhd`, `trak`):
```rust
pub fn create_synthetic_mp4_header() -> Vec<u8> {
    let mut buf = Vec::new();
    // ftyp box (24 bytes)
    buf.extend_from_slice(&24u32.to_be_bytes());
    buf.extend_from_slice(b"ftyp");
    buf.extend_from_slice(b"isom"); // major_brand
    buf.extend_from_slice(&0x200u32.to_be_bytes()); // minor_version
    buf.extend_from_slice(b"isommp42"); // compatible_brands

    // moov box (108 bytes container)
    buf.extend_from_slice(&108u32.to_be_bytes());
    buf.extend_from_slice(b"moov");

    // mvhd box (100 bytes)
    buf.extend_from_slice(&100u32.to_be_bytes());
    buf.extend_from_slice(b"mvhd");
    buf.extend_from_slice(&[0u8; 92]); // mvhd payload stub

    buf
}
```

---

## 2. Boundary Test Procedures

### 2.1 RFC 8794 EBML VINT Testing
When testing the VINT parser, verify all 6 boundary conditions:
1. **Octet Widths**: Test single-byte (`0x81` $\to 1$), two-byte (`0x40 0x02` $\to 2$), up to eight-byte (`0x01 ...`).
2. **Element ID vs Data Size**: Ensure element IDs retain marker bits, while sizes strip marker bits.
3. **Sentinels**: Assert that sentinels with all data bits set to 1 return `VintSize::Unknown`.
4. **Invalid 0x00**: Assert `buf[0] == 0x00` returns `Err(VintError::InvalidZeroPrefix)`.
5. **Reserved IDs**: Assert element IDs with all data bits set to 1 return `Err(VintError::ReservedElementId)`.
6. **Truncation**: Assert truncated slices return `Err(VintError::UnexpectedEof)`.

### 2.2 ISOBMFF Box Parser Boundary Testing
Verify 32-bit and 64-bit box header parsing:
1. **Valid 64-Bit Box**: Ensure `size == 1` correctly parses the following 8-byte `largesize` field.
2. **Minimum Largesize**: Ensure `largesize == 16` (empty box) succeeds.
3. **Invalid Largesize**: Assert `largesize < 16` (`[0, 1, 7, 8, 15]`) returns `Err(DemuxError::MalformedBox)`.
4. **Oversized Largesize**: Assert `largesize > i64::MAX as u64` returns `Err(DemuxError::MalformedBox)`.
5. **Truncated Header**: Assert slice `< 8` bytes (or `< 16` bytes when `size == 1`) returns `Err(DemuxError::UnexpectedEof)`.

### 2.3 Timescale Conversion & Overflow Testing
Verify Matroska PTS integer arithmetic:
$$\text{PTS}_{\text{ms}} = \left\lfloor \frac{(\text{ClusterTimecode} + \text{BlockTimecode}) \times \text{TimestampScale}}{1\,000\,000} \right\rfloor$$
- Test 24 hours of continuous playback: $\text{ClusterTimecode} = 86,400,000$, $\text{TimestampScale} = 1,000,000$.
- Assert that calculations use 64-bit integer arithmetic without overflowing `i64`.
- Assert that negative `BlockTimecode` does not cause underflow.

---

## 3. Verification Commands

Run the test suite:
```bash
cargo test -p syncverse-demux -- --nocapture
python3 tests/empirical_challenge/test_demux_codecs_clock.py
```
Assert all tests pass cleanly.
