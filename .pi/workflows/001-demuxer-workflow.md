# Workflow: 001-demuxer-workflow

## 1. Metadata & OpenSpec Mapping
- **Workflow ID**: `001-demuxer-workflow`
- **Target Change Proposal**: `openspec/changes/001-demuxer-bitstream.md`
- **Capability Specification**: `openspec/specs/demuxer/spec.md`
- **Target Crates**: `syncplay-common`, `syncplay-demux`
- **Primary Agents**: `spec_implementer`, `bitstream_tester`
- **Required Skills**: `openspec-change-mapping`, `bitstream-vector-testing`, `zero-dependency-audit`

---

## 2. Prerequisites & Dependencies
- **Upstream Dependencies**: None (Foundational primitive).
- **Blocks**: `002-codecs-workflow`, `003-clock-workflow`.
- **Pre-flight Checks**:
  1. Confirm zero forbidden codec libraries in `Cargo.toml`.
  2. Confirm workspace compiles cleanly.

---

## 3. Step-by-Step Execution Plan

### Step 1: Foundational Common Types (`syncplay-common`)
1. Implement `MediaTime`: Monotonic media presentation timestamp in milliseconds and nanoseconds with rational arithmetic conversion.
2. Implement `ByteReader` and `BitReader`: Safe, bounds-checked slice reading primitives.
3. Implement `Crc32`: Table-driven IEEE 802.3 CRC32 checksum engine.
4. Run `cargo test -p syncplay-common`.

### Step 2: EBML RFC 8794 VINT Parser (`syncplay-demux/src/ebml/vint.rs`)
1. Implement octet width determination via `leading_zeros() + 1` (supporting 1 to 8 bytes).
2. Distinguish between Element ID (marker bit retained) and Data Size (marker bit cleared).
3. Handle unknown data size sentinel (all data bits = 1).
4. Implement strict bounds checking:
   - Reject empty slices with `UnexpectedEof`.
   - Reject leading byte `0x00` with `InvalidZeroPrefix`.
   - Reject reserved element IDs with `ReservedElementId`.
5. Write unit tests covering all 8 octet widths and invalid prefixes.

### Step 3: Matroska Container Demuxer (`syncplay-demux/src/ebml/`)
1. Implement Element ID lookup and Master Element body parsing (`elements.rs`).
2. Implement `SimpleBlock` parser (`simple_block.rs`) extracting track number, relative timecode, keyframe flag, and lacing.
3. Implement absolute PTS calculation via 64-bit integer arithmetic:
   $$\text{PTS}_{\text{ms}} = \left\lfloor \frac{(\text{ClusterTimecode} + \text{BlockTimecode}) \times \text{TimestampScale}}{1\,000\,000} \right\rfloor$$
4. Verify calculation supports 24-hour playback without 32-bit integer overflow.

### Step 4: ISOBMFF Box Header & Sample Table Parser (`syncplay-demux/src/isobmff/`)
1. Implement 32-bit and 64-bit box header parsing (`box_header.rs`):
   - Standard boxes: $size \ge 8$ or $size == 0$.
   - 64-bit `largesize`: $size == 1 \implies 16 \le largesize \le 2^{63}-1$.
   - Reject $largesize < 16$ or $largesize > \text{i64::MAX}$ with `MalformedBox`.
2. Implement sample table index resolver (`sample_table.rs`) mapping sample index $S$ to file offsets via `stts`, `stsc`, `stsz`, and `stco`/`co64`.

### Step 5: In-Tree Synthetic Test Vectors (`syncplay-demux/src/synthetic.rs`)
1. Embed the byte-exact 48-byte EBML header vector.
2. Embed the 132-byte ISOBMFF MP4 box vector.
3. Author test cases verifying demuxing from raw memory slices.

---

## 4. Verification & Acceptance Criteria
Execute the automated test suite:
```bash
# Crate unit test verification
cargo test -p syncplay-common -- --nocapture
cargo test -p syncplay-demux -- --nocapture

# Adversarial empirical bitstream test
python3 tests/empirical_challenge/test_demux_codecs_clock.py

# Lint inspection
cargo clippy -p syncplay-demux --all-targets -- -D warnings
```
Criteria for success:
- 100% test pass rate with 0 errors.
- 0 compiler warnings.
- Zero external codec dependencies in `Cargo.lock`.
