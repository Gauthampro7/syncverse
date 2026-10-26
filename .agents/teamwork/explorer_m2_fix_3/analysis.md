# Technical Investigation Report: Agent Prompt Banned Crate Synchronization

**Author**: `explorer_m2_fix_3` (teamwork_preview_explorer)  
**Date**: 2026-10-25  
**Target Milestone**: M2 Remediation (`worker_m2_fix`)  
**Investigated Files**:
- `/home/gautham/syncplay-alt/AGENTS.md` (Authoritative Root Architecture Baseline)
- `/home/gautham/syncplay-alt/.pi/agents/spec_implementer.md`
- `/home/gautham/syncplay-alt/.pi/agents/storage_auditor.md`
- `/home/gautham/syncplay-alt/.pi/agents/bitstream_tester.md`
- `/home/gautham/syncplay-alt/.pi/agents/sync_simulator.md`
- `/home/gautham/syncplay-alt/.pi/skills/zero-dependency-audit/SKILL.md`
- `/home/gautham/syncplay-alt/.pi/skills/openspec-change-mapping/SKILL.md`
- `/home/gautham/syncplay-alt/.pi/workflows/e2e-verification-workflow.md`

---

## 1. Executive Summary

During Milestone M2 adversarial challenge, Challenger 1 (`challenger_m2_1/challenge_report.md` Section 3) revealed that agent prompts under `.pi/agents/` contain incomplete and truncated lists of prohibited crates compared to the authoritative root `AGENTS.md` (lines 21–22).

Specifically:
- Root `AGENTS.md` explicitly enumerates **37 prohibited crates** (26 media/codec engines + 11 database/search engines).
- `.pi/agents/spec_implementer.md` (line 15) lists only 18 specific crates plus a wildcard `libav*`, omitting **19 crates** including `diesel`, `hound`, `image`, `imageproc`, `libswscale`, `libswresample`, `sqlite3`, and all `-sys` bindings.
- `.pi/agents/storage_auditor.md` (line 12) only mentions 4 engines informally (`SQLite, RocksDB, Sled, Heed`) in prose, completely omitting `diesel`, `sqlx`, `sqlite3`, `rocksdb-sys`, `tantivy`, `meilisearch`, and any reference to media crates.
- `.pi/agents/bitstream_tester.md` (line 14) mentions only 4 frameworks in prose (`FFmpeg, GStreamer, Symphonia, or libav`), omitting the remaining 33 crates.
- `.pi/agents/sync_simulator.md` lacks an explicit prohibited dependency invariant clause.

This desynchronization creates a severe **agent prompt evasion vulnerability**: an autonomous agent operating under a specialized prompt (e.g. `spec_implementer`) may inadvertently adopt unlisted crates such as `image` (for RGBA frame layout) or `hound` (for audio buffer tests) or `diesel` (for query models), believing they are permissible under its local prompt instructions.

This report provides the full set-theoretic comparison, blast radius assessment, and **exact line-by-line replacement blocks** ready for application by `worker_m2_fix`.

---

## 2. Authoritative Baseline (`AGENTS.md` lines 20–24)

From `/home/gautham/syncplay-alt/AGENTS.md`:
```markdown
#### Strictly Banned Crate & Library List:
- **Media & Codec Engines**: `ffmpeg`, `ffmpeg-sys`, `ffmpeg-next`, `libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`, `gstreamer`, `gstreamer-sys`, `mpv`, `mpv-sys`, `libmpv`, `vlc`, `libvlc`, `symphonia`, `symphonia-core`, `dav1d`, `dav1d-sys`, `rav1e`, `openh264`, `minimp3`, `nom-ebml`, `hound`, `image`, `imageproc`.
- **Database & Search Engines**: `sqlite`, `sqlite3`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `rocksdb-sys`, `heed`, `tantivy`, `meilisearch`, `diesel`.
```

### Complete Inventory of All 37 Prohibited Crates

| # | Category | Crate Name | Description / Rationale for Prohibition |
|---|---|---|---|
| 1 | Media & Codec | `ffmpeg` | Monolithic C/C++ multimedia framework |
| 2 | Media & Codec | `ffmpeg-sys` | Native C FFI bindings to FFmpeg |
| 3 | Media & Codec | `ffmpeg-next` | Idiomatic Rust wrapper around FFmpeg |
| 4 | Media & Codec | `libavcodec` | Bitstream encoding/decoding library |
| 5 | Media & Codec | `libavformat` | Container muxing/demuxing library |
| 6 | Media & Codec | `libavutil` | FFmpeg utility library |
| 7 | Media & Codec | `libswscale` | Color space scaling & conversion library (must use in-tree Q8 fixed-point converter) |
| 8 | Media & Codec | `libswresample` | Audio resampling & format conversion library |
| 9 | Media & Codec | `gstreamer` | Pipeline-based multimedia framework |
| 10 | Media & Codec | `gstreamer-sys` | C bindings to GStreamer |
| 11 | Media & Codec | `mpv` | Standalone media player IPC / client library |
| 12 | Media & Codec | `mpv-sys` | C bindings to libmpv |
| 13 | Media & Codec | `libmpv` | MPv C API bindings |
| 14 | Media & Codec | `vlc` | VLC media player wrapper |
| 15 | Media & Codec | `libvlc` | Native VLC C API bindings |
| 16 | Media & Codec | `symphonia` | Pure-Rust multimedia demuxing/decoding engine |
| 17 | Media & Codec | `symphonia-core` | Core traits and primitives for Symphonia |
| 18 | Media & Codec | `dav1d` | Pure AV1 decoder |
| 19 | Media & Codec | `dav1d-sys` | C bindings to dav1d |
| 20 | Media & Codec | `rav1e` | Pure-Rust AV1 video encoder |
| 21 | Media & Codec | `openh264` | Cisco H.264 codec bindings |
| 22 | Media & Codec | `minimp3` | MP3 audio decoder |
| 23 | Media & Codec | `nom-ebml` | Nom-based EBML parser (must use in-tree RFC 8794 VINT reader) |
| 24 | Media & Codec | `hound` | WAV audio container reader/writer |
| 25 | Media & Codec | `image` | Generic image raster and format decoder (violates pure Q8 frame pipeline) |
| 26 | Media & Codec | `imageproc` | Image processing operations (violates pure Q8 frame pipeline) |
| 27 | Database & Search | `sqlite` | SQLite database engine wrapper |
| 28 | Database & Search | `sqlite3` | Direct SQLite3 bindings |
| 29 | Database & Search | `rusqlite` | Ergonomic SQLite wrapper |
| 30 | Database & Search | `sqlx` | Async SQL toolkit |
| 31 | Database & Search | `sled` | Embedded key-value database |
| 32 | Database & Search | `rocksdb` | Key-value store based on LevelDB |
| 33 | Database & Search | `rocksdb-sys` | C bindings to RocksDB |
| 34 | Database & Search | `heed` | LMDB key-value store wrapper |
| 35 | Database & Search | `tantivy` | Full-text search engine (must use in-tree BTreeMap inverted index) |
| 36 | Database & Search | `meilisearch` | External search engine |
| 37 | Database & Search | `diesel` | SQL ORM and query builder |

---

## 3. Discrepancy Analysis Across Agent Prompts

### 3.1 `.pi/agents/spec_implementer.md`

#### Current Text (Lines 13–16):
```markdown
1. **Zero Forbidden Dependencies**:
   - You MUST NOT add dependencies on any external multimedia, codec, player, or database frameworks.
   - Banned list: `ffmpeg`, `libav*`, `gstreamer`, `mpv`, `vlc`, `symphonia`, `dav1d`, `rav1e`, `openh264`, `minimp3`, `nom-ebml`, `sqlite`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `heed`, `tantivy`, `meilisearch`.
   - Permitted: `thiserror`, standard library (`std::net`, `std::sync::atomic`, `std::time::Instant`, etc.), and in-tree workspace crates.
```

#### Discrepancies:
1. **Missing Media & Codec Crates (15 crates)**:
   - `ffmpeg-sys`, `ffmpeg-next`: C-bindings and modern wrapper for FFmpeg.
   - `libavcodec`, `libavformat`, `libavutil`: Masked by wildcard `libav*`, but `libav*` does not explicitly forbid exact crate names and fails in regex matching.
   - `libswscale`, `libswresample`: Crucial FFmpeg libraries that **do not start with `libav`** and thus were completely unmentioned!
   - `gstreamer-sys`, `mpv-sys`, `libmpv`, `libvlc`, `symphonia-core`, `dav1d-sys`: Direct C/sys bindings.
   - `hound`: Audio parsing crate.
   - `image`, `imageproc`: Image decoding/processing crates.
2. **Missing Database & Search Crates (4 crates)**:
   - `sqlite3`: Explicit alternative crate name for SQLite.
   - `rocksdb-sys`: C-binding wrapper for RocksDB.
   - `diesel`: Prohibited ORM framework.
3. **Total Missing**: 19 specific crates absent from line 15.

---

### 3.2 `.pi/agents/storage_auditor.md`

#### Current Text (Lines 10–16):
```markdown
## 2. Core Invariants & Prohibitions

1. **Zero Database Libraries**:
   - The storage layer MUST NOT use SQLite, RocksDB, Sled, Heed, or any embedded KV/relational engine.
   - All persistence is implemented via a custom append-only binary log (`syncplay_chat.bin`) using `std::fs::File`.
2. **Zero External Search Engines**:
   - Inverted indexing, tokenization, and query execution MUST be implemented from scratch in pure Rust using `std::collections::BTreeMap`.
3. **Data Integrity & Crash Resilience**:
```

#### Discrepancies:
1. **Informal Prose instead of Exact Crate Names**: Uses capitalized English terms (`SQLite, RocksDB, Sled, Heed`) rather than exact Cargo crate identifiers.
2. **Missing Database Crates**: Completely omits `sqlite`, `sqlite3`, `rusqlite`, `sqlx`, `rocksdb-sys`, and `diesel`.
3. **Missing Search Engine Crates in Section 2.2**: Mentions "Zero External Search Engines" but omits explicit references to `tantivy` and `meilisearch`.
4. **Omission of Workspace-Wide Invariant**: As an auditor agent validating the storage subsystem, it must be cognizant of the full 37-crate blacklist so that it can flag any forbidden dependencies introduced in `Cargo.toml`.

---

### 3.3 `.pi/agents/bitstream_tester.md`

#### Current Text (Lines 11–15):
```markdown
You operate under strict from-scratch principles:
1. **Zero External Test Fixtures**: All container headers, track entries, sample tables, and packet bitstreams MUST be synthesized programmatically in memory or defined as byte-exact in-tree byte arrays.
2. **Exhaustive Boundary & Adversarial Coverage**: Every binary parser must be stressed against malformed inputs, truncated slices, integer overflow triggers, and boundary conditions.
3. **No External Codec/Demux Libraries**: Never use FFmpeg, GStreamer, Symphonia, or libav to generate or validate test vectors.
```

#### Discrepancies:
1. **Truncated List**: Line 14 mentions only 4 frameworks in prose (`FFmpeg, GStreamer, Symphonia, or libav`).
2. **Missing Specific Crates**: Does not list `dav1d`, `rav1e`, `openh264`, `minimp3`, `nom-ebml`, `hound`, `image`, or database crates.

---

### 3.4 `.pi/agents/sync_simulator.md`

#### Current Text (Lines 9–16):
```markdown
## 2. Core Invariants & Simulated Environment

You evaluate synchronization protocols across simulated transatlantic/transpacific WAN links configured as follows:
- **Round-Trip Time (RTT)**: 250ms to 400ms base delay.
- **Delay Jitter**: $\pm 30\text{ms}$ asymmetric delay variation.
- **Packet Loss / Reordering**: Up to 5% loss and out-of-order delivery.
- **Local Clock Skew**: Up to $\pm 100\text{ PPM}$ hardware clock drift.
```

#### Discrepancies:
1. Mentions "without depending on live network infrastructure" in Section 1, but lacks an explicit invariant bullet regarding zero external network/sync/media libraries in Section 2.

---

### 3.5 Secondary Reference Locations in Workflows and Skills

While not agent prompts, the following files contain truncated lists that should be documented:
1. `.pi/workflows/e2e-verification-workflow.md` (Line 85):
   - Current: `- Assert 0 occurrences of banned multimedia/player/database crates (`ffmpeg`, `libav*`, `gstreamer`, `mpv`, `vlc`, `symphonia`, `dav1d`, `sqlite`, `rusqlite`, `sqlx`, `nom-ebml`, `rocksdb`).`
   - Omissions: `diesel`, `hound`, `image`, `imageproc`, `libswscale`, `libswresample`, `sled`, `heed`, `tantivy`, `meilisearch`, etc.
2. `.pi/skills/openspec-change-mapping/SKILL.md` (Line 79):
   - Current: `- [ ] No forbidden crates (`ffmpeg`, `libav*`, `sqlite`, etc.) are introduced in `Cargo.toml`.`
   - Omissions: Abbreviated with `etc.`.
3. `.pi/workflows/005-chat-workflow.md` (Line 18):
   - Current: `2. Confirm zero external database libraries (`sqlite`, `rocksdb`, etc.).`

---

## 4. Exact Line-by-Line Replacement Blocks

These replacement blocks are formatted for exact substitution by `worker_m2_fix`.

### 4.1 Target: `/home/gautham/syncplay-alt/.pi/agents/spec_implementer.md`

#### Option A (Recommended — Categorical Matching with `AGENTS.md`)
Replaces lines 13–16:

```markdown
<<<<
1. **Zero Forbidden Dependencies**:
   - You MUST NOT add dependencies on any external multimedia, codec, player, or database frameworks.
   - Banned list: `ffmpeg`, `libav*`, `gstreamer`, `mpv`, `vlc`, `symphonia`, `dav1d`, `rav1e`, `openh264`, `minimp3`, `nom-ebml`, `sqlite`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `heed`, `tantivy`, `meilisearch`.
   - Permitted: `thiserror`, standard library (`std::net`, `std::sync::atomic`, `std::time::Instant`, etc.), and in-tree workspace crates.
====
1. **Zero Forbidden Dependencies**:
   - You MUST NOT add dependencies on any external multimedia, codec, player, or database frameworks.
   - Strictly Banned Media & Codec Engines: `ffmpeg`, `ffmpeg-sys`, `ffmpeg-next`, `libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`, `gstreamer`, `gstreamer-sys`, `mpv`, `mpv-sys`, `libmpv`, `vlc`, `libvlc`, `symphonia`, `symphonia-core`, `dav1d`, `dav1d-sys`, `rav1e`, `openh264`, `minimp3`, `nom-ebml`, `hound`, `image`, `imageproc`.
   - Strictly Banned Database & Search Engines: `sqlite`, `sqlite3`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `rocksdb-sys`, `heed`, `tantivy`, `meilisearch`, `diesel`.
   - Permitted: `thiserror`, standard library (`std::net`, `std::sync::atomic`, `std::time::Instant`, etc.), and in-tree workspace crates.
>>>>
```

#### Option B (Single-Line Replacement for Line 15)
Replaces line 15:

```markdown
<<<<
   - Banned list: `ffmpeg`, `libav*`, `gstreamer`, `mpv`, `vlc`, `symphonia`, `dav1d`, `rav1e`, `openh264`, `minimp3`, `nom-ebml`, `sqlite`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `heed`, `tantivy`, `meilisearch`.
====
   - Banned list: `ffmpeg`, `ffmpeg-sys`, `ffmpeg-next`, `libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`, `gstreamer`, `gstreamer-sys`, `mpv`, `mpv-sys`, `libmpv`, `vlc`, `libvlc`, `symphonia`, `symphonia-core`, `dav1d`, `dav1d-sys`, `rav1e`, `openh264`, `minimp3`, `nom-ebml`, `hound`, `image`, `imageproc`, `sqlite`, `sqlite3`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `rocksdb-sys`, `heed`, `tantivy`, `meilisearch`, `diesel`.
>>>>
```

---

### 4.2 Target: `/home/gautham/syncplay-alt/.pi/agents/storage_auditor.md`

#### Option A (Recommended — Full Categorical Invariants)
Replaces lines 11–15:

```markdown
<<<<
1. **Zero Database Libraries**:
   - The storage layer MUST NOT use SQLite, RocksDB, Sled, Heed, or any embedded KV/relational engine.
   - All persistence is implemented via a custom append-only binary log (`syncplay_chat.bin`) using `std::fs::File`.
2. **Zero External Search Engines**:
   - Inverted indexing, tokenization, and query execution MUST be implemented from scratch in pure Rust using `std::collections::BTreeMap`.
====
1. **Zero Database Libraries**:
   - The storage layer MUST NOT use any external database or persistence engine: `sqlite`, `sqlite3`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `rocksdb-sys`, `heed`, `diesel`, or any embedded KV/relational/document engine.
   - All persistence is implemented via a custom append-only binary log (`syncplay_chat.bin`) using `std::fs::File`.
2. **Zero External Search Engines**:
   - Inverted indexing, tokenization, and query execution MUST NOT use external search engines (`tantivy`, `meilisearch`) and MUST be implemented from scratch in pure Rust using `std::collections::BTreeMap`.
   - General Workspace Invariant: In accordance with root `AGENTS.md`, all 37 prohibited crates (`ffmpeg`, `ffmpeg-sys`, `ffmpeg-next`, `libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`, `gstreamer`, `gstreamer-sys`, `mpv`, `mpv-sys`, `libmpv`, `vlc`, `libvlc`, `symphonia`, `symphonia-core`, `dav1d`, `dav1d-sys`, `rav1e`, `openh264`, `minimp3`, `nom-ebml`, `hound`, `image`, `imageproc`, `sqlite`, `sqlite3`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `rocksdb-sys`, `heed`, `tantivy`, `meilisearch`, `diesel`) are strictly forbidden.
>>>>
```

#### Option B (Direct Line-12 Substitution)
Replaces line 12:

```markdown
<<<<
   - The storage layer MUST NOT use SQLite, RocksDB, Sled, Heed, or any embedded KV/relational engine.
====
   - The storage layer MUST NOT use any banned database or search engines: `sqlite`, `sqlite3`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `rocksdb-sys`, `heed`, `tantivy`, `meilisearch`, `diesel`.
>>>>
```

---

### 4.3 Target: `/home/gautham/syncplay-alt/.pi/agents/bitstream_tester.md`

#### Recommended Replacement
Replaces line 14:

```markdown
<<<<
3. **No External Codec/Demux Libraries**: Never use FFmpeg, GStreamer, Symphonia, or libav to generate or validate test vectors.
====
3. **No External Codec/Demux Libraries**: Never use external media, codec, or database crates (`ffmpeg`, `ffmpeg-sys`, `ffmpeg-next`, `libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`, `gstreamer`, `gstreamer-sys`, `mpv`, `mpv-sys`, `libmpv`, `vlc`, `libvlc`, `symphonia`, `symphonia-core`, `dav1d`, `dav1d-sys`, `rav1e`, `openh264`, `minimp3`, `nom-ebml`, `hound`, `image`, `imageproc`, `sqlite`, `sqlite3`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `rocksdb-sys`, `heed`, `tantivy`, `meilisearch`, `diesel`) to generate or validate test vectors.
>>>>
```

---

### 4.4 Target: `/home/gautham/syncplay-alt/.pi/agents/sync_simulator.md`

#### Recommended Replacement
Replaces lines 9–16:

```markdown
<<<<
## 2. Core Invariants & Simulated Environment

You evaluate synchronization protocols across simulated transatlantic/transpacific WAN links configured as follows:
- **Round-Trip Time (RTT)**: 250ms to 400ms base delay.
- **Delay Jitter**: $\pm 30\text{ms}$ asymmetric delay variation.
- **Packet Loss / Reordering**: Up to 5% loss and out-of-order delivery.
- **Local Clock Skew**: Up to $\pm 100\text{ PPM}$ hardware clock drift.
====
## 2. Core Invariants & Simulated Environment

You operate under strict zero-dependency invariants:
- **Zero Forbidden External Dependencies**: In accordance with root `AGENTS.md`, never use external media, sync, or database frameworks (`ffmpeg`, `libav*`, `gstreamer`, `symphonia`, `sqlite`, `diesel`, etc.).
- **Round-Trip Time (RTT)**: 250ms to 400ms base delay.
- **Delay Jitter**: $\pm 30\text{ms}$ asymmetric delay variation.
- **Packet Loss / Reordering**: Up to 5% loss and out-of-order delivery.
- **Local Clock Skew**: Up to $\pm 100\text{ PPM}$ hardware clock drift.
>>>>
```

---

### 4.5 Target: `/home/gautham/syncplay-alt/.pi/workflows/e2e-verification-workflow.md`

#### Recommended Replacement
Replaces line 85:

```markdown
<<<<
- Assert 0 occurrences of banned multimedia/player/database crates (`ffmpeg`, `libav*`, `gstreamer`, `mpv`, `vlc`, `symphonia`, `dav1d`, `sqlite`, `rusqlite`, `sqlx`, `nom-ebml`, `rocksdb`).
====
- Assert 0 occurrences of banned multimedia/player/database crates enumerated in root `AGENTS.md` (`ffmpeg`, `ffmpeg-sys`, `ffmpeg-next`, `libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`, `gstreamer`, `gstreamer-sys`, `mpv`, `mpv-sys`, `libmpv`, `vlc`, `libvlc`, `symphonia`, `symphonia-core`, `dav1d`, `dav1d-sys`, `rav1e`, `openh264`, `minimp3`, `nom-ebml`, `hound`, `image`, `imageproc`, `sqlite`, `sqlite3`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `rocksdb-sys`, `heed`, `tantivy`, `meilisearch`, `diesel`).
>>>>
```

---

## 5. Summary Table for Implementation Worker

| Target File | Target Lines | Recommended Modification | Missing Crates Added |
|---|---|---|---|
| `.pi/agents/spec_implementer.md` | 14–15 | Split into Media & Codec and Database & Search lists matching `AGENTS.md` | 19 crates (`diesel`, `hound`, `image`, `imageproc`, `libswscale`, `libswresample`, `sqlite3`, `rocksdb-sys`, all `-sys` crates) |
| `.pi/agents/storage_auditor.md` | 11–15 | Explicitly ban all 11 database/search crates + cite full 37-crate workspace invariant | 7 database crates (`diesel`, `sqlx`, `sqlite3`, `rusqlite`, `rocksdb-sys`, `tantivy`, `meilisearch`) + 26 media crates |
| `.pi/agents/bitstream_tester.md` | 14 | Update prohibition line 14 to include full banned list | All 37 prohibited crates |
| `.pi/agents/sync_simulator.md` | 10–12 | Add explicit zero-dependency invariant bullet | Invariant reference to `AGENTS.md` |
| `.pi/workflows/e2e-verification-workflow.md` | 85 | Expand banned crate list to all 37 crates | All omitted crates from `AGENTS.md` |

---

## 6. Verification Method for `worker_m2_fix`

After applying the replacement blocks, `worker_m2_fix` must verify:
1. Grep verification confirming `diesel`, `hound`, `image`, and `imageproc` appear in `.pi/agents/spec_implementer.md` and `.pi/agents/storage_auditor.md`:
   ```bash
   grep -E "(diesel|hound|image|imageproc)" .pi/agents/spec_implementer.md
   grep -E "(diesel|sqlx|sqlite3)" .pi/agents/storage_auditor.md
   ```
2. Crate count verification: assert that all 37 crates from `AGENTS.md` are present in `.pi/agents/spec_implementer.md`:
   ```python
   import re

   with open("AGENTS.md") as f:
       agents_text = f.read()
   with open(".pi/agents/spec_implementer.md") as f:
       spec_text = f.read()

   banned_crates = [
       "ffmpeg", "ffmpeg-sys", "ffmpeg-next", "libavcodec", "libavformat", "libavutil",
       "libswscale", "libswresample", "gstreamer", "gstreamer-sys", "mpv", "mpv-sys",
       "libmpv", "vlc", "libvlc", "symphonia", "symphonia-core", "dav1d", "dav1d-sys",
       "rav1e", "openh264", "minimp3", "nom-ebml", "hound", "image", "imageproc",
       "sqlite", "sqlite3", "rusqlite", "sqlx", "sled", "rocksdb", "rocksdb-sys",
       "heed", "tantivy", "meilisearch", "diesel"
   ]
   missing = [c for c in banned_crates if f"`{c}`" not in spec_text]
   assert len(missing) == 0, f"Missing in spec_implementer: {missing}"
   print("All 37 crates verified in spec_implementer.md!")
   ```
3. Run the adversarial stress test suite:
   ```bash
   python3 tests/empirical_challenge/test_pi_adversarial_stress.py
   ```
