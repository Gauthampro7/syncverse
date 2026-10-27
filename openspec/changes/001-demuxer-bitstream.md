# Change Proposal: 001-demuxer-bitstream

**Change ID**: `001-demuxer-bitstream`  
**Title**: Zero-Dependency Container Demuxing Primitives & Bitstream Extraction  
**Status**: Ready for Implementation  
**Target Milestone**: Phase 1 Foundational Primitives  

---

## 1. Context & Motivation

To fulfill the zero-external-codec architectural invariant, Syncverse cannot rely on FFmpeg or GStreamer to demux media containers. This change establishes the foundational container parsers for Matroska (EBML/MKV/WebM) and ISOBMFF (MP4/MOV) in pure, safe Rust.

---

## 2. Dependency Graph

```
[001-demuxer-bitstream] (Roots: syncverse-common, syncverse-demux)
         │
         ▼
[002-codecs-presentation]
         │
         ▼
[003-clock-pipeline]
```

- **Dependencies**: None (Foundational primitive).
- **Blocks**: `002-codecs-presentation`, `003-clock-pipeline`.

---

## 3. Affected Crates & Components

- **`crates/syncverse-common`**: Byte reading utilities (`ByteReader`), `MediaTime` type, common error abstractions.
- **`crates/syncverse-demux`**:
  - `src/ebml/vint.rs`: RFC 8794 Variable-Length Integer (VINT) parser.
  - `src/ebml/elements.rs`: Matroska Element ID definitions and Master element parser.
  - `src/ebml/simple_block.rs`: SimpleBlock binary wire parser (keyframe flag, relative timestamp, lacing).
  - `src/isobmff/box_header.rs`: 32-bit and 64-bit box header parser with bounds checks.
  - `src/isobmff/sample_table.rs`: Sample table index resolver (`stts`, `stsc`, `stsz`, `stco`).
  - `src/synthetic.rs`: Byte-exact in-tree synthetic EBML (48B) and MP4 (132B) test vectors.

---

## 4. Deliverables & Implementation Plan

1. **EBML VINT Decoder**:
   - Octet width resolution via `leading_zeros() + 1` (1 to 8 bytes).
   - Distinction between Element ID (marker bit retained) and Data Size (marker bit masked out).
   - Unknown size sentinel detection (all data bits = 1).
   - Input slice length check (`UnexpectedEof`) and RFC 8794 reserved Element ID rejection (`ReservedElementId`).
2. **Matroska Stream Demuxer**:
   - Header validation: `DocType` `"matroska"` or `"webm"`.
   - Cluster and SimpleBlock extraction, calculating absolute PTS in milliseconds via integer arithmetic:
     $$\text{PTS}_{\text{ms}} = \left\lfloor \frac{(\text{ClusterTimecode} + \text{BlockTimecode}) \times \text{TimestampScale}}{1\,000\,000} \right\rfloor$$
3. **ISOBMFF Box Parser**:
   - 32-bit standard size ($size \ge 8$ or $0$), 64-bit `largesize` ($size == 1 \implies largesize \ge 16$ and $largesize \le 2^{63}-1$), and end-of-file box (`size == 0`).
   - Sample Table indexing algorithm mapping 0-based sample index $S$ to absolute file offset, validating $samples\_per\_chunk \ge 1$.
4. **Synthetic Test Vectors**:
   - 48-byte synthetic EBML header vector.
   - 132-byte synthetic MP4 header vector.

---

## 5. Verification Commands

Execute the following commands from the repository root:

```bash
# 1. Type and borrow check affected crates
cargo check -p syncverse-common -p syncverse-demux

# 2. Run unit tests including synthetic test vector parsing
cargo test -p syncverse-demux -- --nocapture

# 3. Verify zero external media dependencies in manifest
cargo tree -p syncverse-demux --edges no-dev
```

Expected output: All unit tests pass with zero warnings, and the dependency tree contains only workspace crates and permitted utilities.
