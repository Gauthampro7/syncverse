# Adversarial Challenge & Stress-Test Report: Pi Framework Workflows (Milestone M2)

## Challenge Summary

**Overall risk assessment**: LOW

All task workflows in `.pi/workflows/` (`001-demuxer-workflow.md` through `006-ui-workflow.md` and `e2e-verification-workflow.md`) have been subjected to empirical stress testing and rigorous mathematical verification against the remediated OpenSpec specifications (`openspec/specs/` and `openspec/changes/001` through `006`).

All four empirical test suites executed cleanly with zero failures. The task workflows accurately reflect all architectural invariants, mathematical formulations, structural prerequisites, affected crates, deliverables, and verification commands. Minor operational nuances (such as symlink aliases and workspace build sequencing for Milestone M3) have been analyzed and found to be well-mitigated.

---

## 1:1 Mapping Verification

| Workflow File | Target Change Proposal | Capability Specification | Target Crates | Upstream Dependencies | Blocks | Status |
|---|---|---|---|---|---|---|
| `001-demuxer-workflow.md` | `openspec/changes/001-demuxer-bitstream.md` | `openspec/specs/demuxer/spec.md` | `syncplay-common`, `syncplay-demux` | None (Root) | `002`, `003` | **VERIFIED 1:1** |
| `002-codecs-workflow.md` | `openspec/changes/002-codecs-presentation.md` | `openspec/specs/codecs/spec.md` | `syncplay-codecs` | `001` | `003` | **VERIFIED 1:1** |
| `003-clock-workflow.md` | `openspec/changes/003-clock-pipeline.md` | `openspec/specs/av-pipeline/spec.md` | `syncplay-clock` | `001`, `002` | `004`, `006` | **VERIFIED 1:1** |
| `004-sync-workflow.md` | `openspec/changes/004-network-sync-protocol.md` | `openspec/specs/sync-engine/spec.md` | `syncplay-sync` | `003` | `005`, `006` | **VERIFIED 1:1** |
| `005-chat-workflow.md` | `openspec/changes/005-smart-chat-storage.md` | `openspec/specs/smart-chat/spec.md` | `syncplay-chat` | `001`, `004` | `006` | **VERIFIED 1:1** |
| `006-ui-workflow.md` | `openspec/changes/006-ui-osd-renderer.md` | `openspec/specs/ui-renderer/spec.md` | `syncplay-ui`, `syncplay-core` | `003`, `004`, `005` | None (Terminal) | **VERIFIED 1:1** |
| `e2e-verification-workflow.md` | User Request Requirement R4 / Verification Harness | Project-wide (`openspec/project.md`) | All Workspace Crates | `001`–`006` | Final Release | **VERIFIED** |

*Note on 003*: The symlink `003-clock-pipeline-workflow.md -> 003-clock-workflow.md` provides bi-directional compatibility whether tools look up the workflow by its change proposal name (`003-clock-pipeline`) or by subsystem (`003-clock-workflow`).

---

## Adversarial Challenges

### [Low] Challenge 1: Workflow Filename Suffix Standardization & Symlink Resolution
- **Assumption challenged**: Automated workflow parsers and CI tooling expect workflow filenames to match OpenSpec change proposal slugs exactly.
- **Attack scenario**: A script scanning `.pi/workflows/` with `find .pi/workflows/ -type f` without dereferencing symlinks (`-L`) might miss `003-clock-pipeline-workflow.md` or notice discrepancies between short names (`003-clock-workflow.md`, `004-sync-workflow.md`) and change proposal names (`003-clock-pipeline.md`, `004-network-sync-protocol.md`).
- **Blast radius**: Cosmetic naming inconsistency or failed lookups if a script expects literal `004-network-sync-protocol-workflow.md`.
- **Mitigation**: The workflow files embed authoritative metadata headers with explicit `Target Change Proposal: openspec/changes/00X-*.md` and `Capability Specification: openspec/specs/*/spec.md`. The symlink `003-clock-pipeline-workflow.md -> 003-clock-workflow.md` is present. No action required; metadata provides unambiguous binding.

### [Low] Challenge 2: Workspace Scaffolding Sequencing for Milestone M3
- **Assumption challenged**: Workflow 001 verification commands (`cargo test -p syncplay-common`, `cargo test -p syncplay-demux`) assume the top-level `Cargo.toml` workspace manifest already exists.
- **Attack scenario**: If a downstream worker starts Milestone M3 by attempting to run verification commands before creating the root `Cargo.toml` and crate manifests, cargo commands will fail with `could not find Cargo.toml`.
- **Blast radius**: Downstream worker confusion during early M3 initialization.
- **Mitigation**: Workflow 001 Pre-flight Checks specify verifying `Cargo.toml` and clean workspace compilation. Step 1 explicitly commands creating `syncplay-common` foundational primitives. Worker agents in Milestone M3 will initialize the workspace `Cargo.toml` as the foundational scaffold step.

### [Low] Challenge 3: ISOBMFF 64-Bit Largesize Underflow and Integer Overflow Bounds
- **Assumption challenged**: 64-bit box header parsing handles non-standard box sizes and prevents buffer over-reads without panicking.
- **Attack scenario**: Malicious or corrupted MP4 files with `size == 1` and `largesize < 16` (e.g., `largesize = 0, 1, 8, 15`) or `largesize > i64::MAX as u64` ($2^{63}-1$).
- **Blast radius**: Out-of-bounds memory indexing or integer overflow during slice slicing.
- **Mitigation**: Workflow 001 Step 4 and `openspec/specs/demuxer/spec.md` explicitly enforce:
  `if size == 1 && largesize < 16 => Err(DemuxError::MalformedBox)`
  `if largesize > i64::MAX as u64 => Err(DemuxError::MalformedBox)`
  Empirically proven by `test_demux_codecs_clock.py` (all tests passed).

### [Low] Challenge 4: YUV420p Row Stride Vectorization and Tail Padding
- **Assumption challenged**: Color conversion loops can process video frames using SIMD instructions without checking row boundary bounds on non-multiple-of-16 widths.
- **Attack scenario**: A 480p frame with width 854 is processed using 16-byte SIMD vector loads. If chroma planes have stride 427 without padding, vector loads at the end of each row read past allocated memory.
- **Blast radius**: Memory corruption, page faults, or segmentation faults during hot-loop presentation rendering.
- **Mitigation**: Workflow 002 Step 1 enforces 16-byte row stride alignment `stride = (width + 15) & !15` and $+16$B SIMD tail padding at the end of every plane allocation. Empirically tested across 9 diverse resolutions (including 854x480, 127x63) in `test_demux_codecs_clock.py`; counter-example confirms unpadded stride triggers memory over-reads.

### [Low] Challenge 5: Atomic SeqLock Clock Re-Anchoring Concurrency Under Rate Shifts
- **Assumption challenged**: Clock presentation time stays strictly continuous across PLL micro-speed adjustments ($1.00\times \to 1.04\times$).
- **Attack scenario**: Concurrent reader threads querying `CurrentPTS` while the writer updates `anchor_pts`, `anchor_wall`, and `speed` sequentially in independent non-atomic fields, producing torn reads and phantom timeline jumps.
- **Blast radius**: Jittery video frames, audio underruns, or negative timestamp steps.
- **Mitigation**: Workflow 003 Step 1 mandates publishing state snapshots atomically via `SeqLock` or `AtomicU128`. Empirically verified in `test_demux_codecs_clock.py`: naive disjoint clock produced torn reads, while SeqLock clock executed with 0 torn reads and zero phantom jumps across concurrent reader threads.

### [Low] Challenge 6: NTP Outlier Filtering, Asymmetric Jitter & Route Shift Watchdog
- **Assumption challenged**: NTP 4-timestamp exchange can filter asymmetric bufferbloat delays and recover if the network route shifts.
- **Attack scenario**: An asymmetric bufferbloat spike (+330ms delay) arrives, or the network route shifts permanently, increasing RTT by 100ms.
- **Blast radius**: Clock offset latch-up, false dispersion freeze ($D=0$), or permanent desynchronization.
- **Mitigation**: Workflow 004 Step 2 specifies RFC 5905 8-sample sliding window min-RTT selection, an effective dispersion floor $D_{\text{effective}} = \max(D, 5.0\text{ ms})$ preventing zero-dispersion freeze, outlier rejection $\text{RTT} > 2.0 \times \text{median}$ or $|\theta - \hat{\theta}| > 3 D_{\text{effective}}$, and an 8-rejection watchdog buffer flush. Empirically proven in `test_sync_engine.py`: asymmetric probes are rejected, legitimate step offsets are accepted, and reroute recovery occurs cleanly.

### [Low] Challenge 7: Binary Log False-Sync Resilience & CJK Ideograph Indexing
- **Assumption challenged**: Append-only binary chat logs can recover from corruptions without losing records, and inverted index search supports non-Western scripts.
- **Attack scenario**:
  1. A corrupted payload contains `[0xAA, 0x55]` followed by an oversized length field (`record_total_len > file_len`). A naive scanner would treat this as a premature EOF and truncate the file, dropping all subsequent valid records.
  2. Single-character search queries in Chinese/Japanese (e.g. "猫", "狗") are discarded if the index enforces a 2-character minimum word length.
- **Blast radius**: Irreversible loss of chat history; broken search in CJK languages.
- **Mitigation**:
  1. Workflow 005 Step 2 specifies that candidates with oversized lengths are classified as false sync words, advancing byte-by-byte without aborting recovery. Empirically verified in `test_smart_chat.py` (all 9 valid records salvaged).
  2. Workflow 005 Step 3 specifies CJK unigram + overlapping bigram segmentation with single-ideograph length exemption ($1 \le \text{len} \le 48$). Empirically verified in `test_smart_chat.py`.

### [Low] Challenge 8: SPSC Triple-Buffer Telemetry Aggregation & 10 Hz Scrubber Debouncing
- **Assumption challenged**: High-frequency mouse drag events (120 Hz) on the scrubber bar do not overwhelm the network sync engine, and UI snapshot reads remain wait-free and untorn.
- **Attack scenario**: A 500ms drag emits 60 seek packets, causing network congestion, PLL oscillation, and buffer starvation.
- **Blast radius**: Player freezing, sync divergence, and audio hitching.
- **Mitigation**: Workflow 006 Step 3 implements a 10 Hz rate limiter on drag events. For a 500ms drag at 120 Hz, exactly 5 intermediate seek packets are dispatched, plus 1 authoritative final seek packet upon mouse release (total 6 commands). Empirically verified in `test_ui_renderer.py`. In addition, `PlayerUiSnapshot` fits in 72 bytes (under the 128-byte cache line pair bound), and a centralized Telemetry Aggregator ensures strict SPSC topology.

---

## Stress Test Results

| Test Harness | Scenario | Expected Behavior | Actual Behavior | Result |
|---|---|---|---|---|
| `test_demux_codecs_clock.py` | Synthetic EBML Vector Length | Exactly 48 bytes | 48 bytes parsed, DocType='matroska' | **PASS** |
| `test_demux_codecs_clock.py` | RFC 8794 VINT Bounds | Truncated VINT raises `UnexpectedEof`, `0x00` raises `InvalidZeroPrefix`, all-1s ID raises `ReservedElementId` | Correct errors raised | **PASS** |
| `test_demux_codecs_clock.py` | 24-Hour Playback Timescale Math | PTS in nanoseconds computed via 64-bit integer arithmetic without overflow | PTS_ns = 86.4T ns (0.000937% of i64::MAX) | **PASS** |
| `test_demux_codecs_clock.py` | ISOBMFF Largesize Validation | Reject `largesize < 16` and `largesize > i64::MAX` with `MalformedBox` | Boundary 16 accepted; [0, 1, 7, 8, 15], i64::MAX+1, u64::MAX rejected | **PASS** |
| `test_demux_codecs_clock.py` | YUV420p Stride & SIMD Padding | 16B row alignment + 16B tail padding prevents OOB over-reads | 9 resolutions tested with zero over-reads; unpadded triggers failure | **PASS** |
| `test_demux_codecs_clock.py` | SeqLock State Re-Anchoring | Atomic state publication eliminates torn reads across rate transitions | 0 torn reads across concurrent readers (naive clock produced torn reads) | **PASS** |
| `test_demux_codecs_clock.py` | SPSC Ring Buffer Wrapping | Power-of-two modulo wrapping, full/empty detection, concurrent stress | 50,000 items transferred across threads without loss or corruption | **PASS** |
| `test_sync_engine.py` | Discrete PI PLL Dynamics | $e(0)=450\text{ ms}$ converges to $< 15\text{ ms}$ deadband within 20s with 0 overshoot past $-15\text{ ms}$ | Enters deadband at $t=12.0\text{ s}$ ($e=11.21\text{ ms}$), zero overshoot | **PASS** |
| `test_sync_engine.py` | NTP Asymmetry & Dispersion Freeze | Reject asymmetric jitter, maintain floor $D_{\text{eff}} \ge 5\text{ ms}$, watchdog reset after 8 rejections | Asymmetry rejected; reroute recovered after 8 consecutive rejections | **PASS** |
| `test_sync_engine.py` | Two-Phase Barrier Unpause Margin | Target schedule guarantees $\ge 100\text{ ms}$ positive commit arrival margin across $0..200\text{ ms}$ preroll | Margins $300\text{ ms}$, $250\text{ ms}$, $200\text{ ms}$, $150\text{ ms}$, $100\text{ ms}$ (all $\ge 100\text{ ms}$) | **PASS** |
| `test_smart_chat.py` | False-Sync Resilience in Binary Log | Mid-stream `0xAA55` with oversized length does not abort recovery or drop valid subsequent records | All 9 valid records salvaged; false candidate skipped safely | **PASS** |
| `test_smart_chat.py` | Torn Write & Bit Corruption | Incomplete trailing record marked torn write; flipped bits skipped | 9/9 records salvaged; torn write and corrupted chunks reported | **PASS** |
| `test_smart_chat.py` | CJK Inverted Index FTS | CJK unigrams/bigrams match multi-character words; single CJK characters indexed | "吉他" matches; "猫" matches; Turkish "İSTANBUL" -> "istanbul" | **PASS** |
| `test_ui_renderer.py` | Triple-Buffer SPSC Concurrency | Telemetry Aggregator publishes to triple buffer with 0 torn snapshots; struct size $\le 128\text{B}$ | 72 bytes struct; 0 inconsistencies across 2554 snapshots | **PASS** |
| `test_ui_renderer.py` | 10 Hz Scrubber Debouncing | 500ms drag at 120 Hz emits 5 intermediate seeks + 1 release seek | Exactly 5 intermediate seeks + 1 release seek = 6 seeks emitted | **PASS** |
| `test_ui_renderer.py` | UI Memory & Font Atlas Bounding | Chat overlay capped to 5 bubbles; Alpha8 atlas 4MB; total UI RAM $\le 15.0\text{ MB}$ | 1000 burst capped to 5; Alpha8 atlas 4MB; total RAM 13.5 MB $\le 15.0\text{ MB}$ | **PASS** |

---

## Unchallenged Areas

- **Crate Source Code Compilation (`cargo check`, `cargo test`)**:
  - Reason not challenged: Source code implementation (`crates/*`) and `Cargo.toml` are deliverables of Milestone M3 (Pure-Rust Multi-Crate Workspace Scaffold). Milestone M2 specifically delivers the Pi framework specifications, agent prompts, skills, and workflows. Full compilation verification will be performed in Milestone M3.
- **Physical Multi-Node Network Simulation**:
  - Reason not challenged: Real UDP network sockets will be instantiated during Milestone M3/M4 integration tests. The underlying mathematical and algorithmic invariants of the NTP filter and PI PLL controller have already been verified in the empirical test suite.

---

## Final Recommendation

The task workflows in `.pi/workflows/` (`001-demuxer-workflow.md` through `006-ui-workflow.md` and `e2e-verification-workflow.md`) are mathematically rigorous, structurally sound, strictly aligned with OpenSpec change proposals and capability specifications, and empirically verified against all challenge test suites.

**Recommendation**: **APPROVE**.
