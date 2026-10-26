# Project: Syncplay-Alt (Phase 1)

## Architecture
Syncplay-Alt is an open-source, high-efficiency standalone media player and long-distance synchronized watch-party platform built in pure Rust with **zero external media, codec, or database frameworks**.

### Subsystem Topology
1. **Container Demuxing Subsystem (`syncplay-demux`)**:
   - Zero-dependency EBML / Matroska (MKV) parser: RFC 8794 variable-length integer (VINT) decoder, Element ID parser, Segment/Tracks/Cluster traversal.
   - Zero-dependency ISOBMFF / MP4 parser: 32-bit & 64-bit Box headers (`ftyp`, `moov`, `mvhd`, `trak`, `mdia`, `stbl`, `stsd`, `stts`, `stsc`, `stsz`, `stco`, `co64`).
   - In-tree synthetic test vectors for instant, deterministic verification.
2. **Codec & Pixel Subsystem (`syncplay-codecs`)**:
   - Baseline audio/video packet representations, planar YUV420p and packed RGB24/RGBA32 frames.
   - Pure-Rust Q8 fixed-point integer color space conversion ($Y'CbCr \to R'G'B'$ limited range) with SIMD-friendly layout.
3. **Master Monotonic Clock & Ring Buffer (`syncplay-clock`)**:
   - Monotonic time base (`std::time::Instant`), PTS tracking with atomic anchor re-anchoring on rate changes.
   - Lock-free Single-Producer Single-Consumer (SPSC) ring buffer with 128-byte cache-line alignment to eliminate false sharing.
4. **Network Synchronization Protocol (`syncplay-sync`)**:
   - NTP-style 4-timestamp probe exchange ($T_1, T_2, T_3, T_4$) with RFC 5905 8-sample sliding-window minimum RTT filtering.
   - Deterministic Reference Timeline mapping room clock to local presentation.
   - Tri-Zone PLL micro-speed drift controller: Zone 1 deadband ($|e| < 15\text{ ms} \to 1.0\times$), Zone 2 continuous micro-speed ($15\text{ ms} \le |e| \le 1500\text{ ms} \to r \in [0.96, 1.04]$ via discrete PI controller), Zone 3 hard seek ($|e| > 1500\text{ ms}$).
   - Barrier-synchronized two-phase unpause state machine with pre-roll decode and presentation scheduling ($< 10\text{ ms}$ alignment).
5. **Smart In-Player Chat & Storage Engine (`syncplay-chat`)**:
   - Custom crash-resilient binary append-only log (`syncplay_chat.bin`) with `0xAA55` framing, dual timestamps ($t_{wall}, t_{media}$), UUID sender, and IEEE 802.3 CRC32 checksum.
   - In-memory inverted index FTS (tokenization, stop-words, BTreeMap postings, two-pointer Boolean AND) and temporal range query ($[T_{media} - \Delta t, T_{media} + \Delta t]$).
   - Slash-command grammar & lexer (`/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`) mapped to sync protocol wire messages.
6. **Native UI & OSD Overlay (`syncplay-ui`)**:
   - Zero-contention lock-free triple-buffered atomic snapshot state model (`PlayerUiSnapshot`).
   - OSD overlay elements: timecodes, playback status, sync health badge, floating chat subtitles.
7. **Core Runtime Engine (`syncplay-core`)**:
   - Top-level orchestrator binding demuxing, decoding, clock, sync, chat, and UI.

---

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | OpenSpec Project & Specs | Exhaustive RFC-2119 specs for 6 core subsystems with Given/When/Then scenarios | M1 | Survey |
| 2 | OpenSpec Roadmap | Dependency-graphed multi-phase change proposals under `openspec/changes/` | M1 | Survey |
| 3 | Pi Framework & Invariants | Root `AGENTS.md` and `.pi/` agent prompts, skills, workflows | M2 | Survey |
| 4 | Pure-Rust Workspace Topology | 8-crate workspace scaffold with clean trait boundaries and common primitives | M3 | Survey |
| 5 | Zero-Dependency Blacklist Enforcement | Ban external media/db crates (`ffmpeg`, `libav*`, `sqlite`, etc.) | M3 | Survey |
| 6 | EBML/MKV Demuxer Primitives | VINT parser, Element ID parser, synthetic EBML header test vector | M3 | Survey 1 |
| 7 | ISOBMFF/MP4 Demuxer Primitives | 32/64-bit box parser, sample table mapping, synthetic MP4 header test vector | M3 | Survey 1 |
| 8 | Fixed-Point Color Space Conversion | Pure-Rust Q8 YUV420p to RGB converter with SIMD layout | M3 | Survey 1 |
| 9 | Monotonic Clock & Ring Buffer | Continuous PTS tracking & 128-byte aligned SPSC lock-free ring buffer | M3 | Survey 1 |
| 10 | NTP Clock Filter | 4-timestamp probe exchange + sliding-window minimum RTT outlier filter | M3 | Survey 2 |
| 11 | Tri-Zone PLL Controller | Discrete PI controller converging phase error < 15ms under 1.5s drift | M3 | Survey 2 |
| 12 | Barrier Unpause State Machine | Two-phase commit unpause achieving cross-peer alignment < 10ms | M3 | Survey 2 |
| 13 | Binary Chat Log Storage | `syncplay_chat.bin` append-only engine with 0xAA55 framing & CRC32 | M3 | Survey 3 |
| 14 | Inverted Index & FTS Engine | In-memory tokenized posting list, Boolean AND, and temporal overlay range queries | M3 | Survey 3 |
| 15 | Slash-Command Parser & Media Triggers | Grammars for `/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status` | M3 | Survey 3 |
| 16 | Verification Script Harness | `scripts/verify_phase1.sh` running 7 automated validation stages | M4 | Survey 3 |
| 17 | High-RTT Network Simulation Test | Deterministic simulation (250-400ms RTT, ±30ms jitter, PLL < 15ms, barrier < 10ms) | M4 | Survey 2, 3 |

---

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| M1 | OpenSpec Specification Suite | `openspec/project.md`, `openspec/specs/` (6 subsystems), `openspec/changes/` | none | DONE |
| M2 | Pi Agent-Driven Development Setup | `AGENTS.md` and `.pi/` directory (agents, skills, workflows) | M1 | DONE |
| M3 | Pure-Rust Multi-Crate Workspace Scaffold & Foundational Primitives | 8-crate workspace, zero-dep enforcement, EBML/MP4 parsers, NTP/PLL sync, binary chat log & FTS | M1, M2 | DONE |
| M4 | Verification Harness & Deterministic Simulation Suite | `scripts/verify_phase1.sh`, dependency audit, deterministic high-RTT simulation tests | M3 | DONE |

---

## Interface Contracts

### Demuxer ↔ Core (`syncplay-demux` ↔ `syncplay-core`)
- `trait Demuxer`:
  ```rust
  pub trait Demuxer: Send {
      fn probe(reader: &mut (dyn ReadSeek + Send)) -> Result<bool, DemuxError>;
      fn init(&mut self, reader: Box<dyn ReadSeek + Send>) -> Result<ContainerInfo, DemuxError>;
      fn read_packet(&mut self) -> Result<Option<RawPacket>, DemuxError>;
      fn seek_to(&mut self, target_pts_ms: u64) -> Result<u64, DemuxError>;
  }
  ```

### Codec ↔ Presentation (`syncplay-codecs` ↔ `syncplay-clock`)
- `struct VideoFrame`:
  ```rust
  pub struct VideoFrame {
      pub pts_ms: u64,
      pub width: u32,
      pub height: u32,
      pub format: PixelFormat, // YUV420p, RGB24, RGBA32
      pub planes: Vec<Vec<u8>>,
  }
  ```

### Sync Engine ↔ Clock (`syncplay-sync` ↔ `syncplay-clock`)
- `struct PllCorrection`:
  ```rust
  pub struct PllCorrection {
      pub speed_multiplier: f64, // 0.96 .. 1.04
      pub hard_seek_target: Option<u64>,
      pub phase_error_ms: f64,
  }
  ```

### Chat Engine ↔ UI / Sync (`syncplay-chat` ↔ `syncplay-ui`)
- `trait ChatStore`:
  ```rust
  pub trait ChatStore: Send + Sync {
      fn append(&mut self, record: ChatRecord) -> Result<u64, ChatError>;
      fn search(&self, query: &str) -> Vec<SearchResult>;
      fn query_timeline_window(&self, media_ts: u64, window_ms: u64) -> Vec<ChatRecord>;
      fn export_history_json(&self) -> String;
  }
  ```

---

## Code Layout
```
/home/gautham/syncplay-alt/
├── AGENTS.md                                # Root invariants & agent coding rules
├── Cargo.toml                               # Workspace manifest (8 crates)
├── Cargo.lock                               # Locked dependencies (0 forbidden crates)
├── openspec/
│   ├── project.md                           # Master OpenSpec project specification
│   ├── specs/                               # Subsystem capability specifications
│   │   ├── demuxer/spec.md
│   │   ├── codecs/spec.md
│   │   ├── av-pipeline/spec.md
│   │   ├── sync-engine/spec.md
│   │   ├── smart-chat/spec.md
│   │   └── ui-renderer/spec.md
│   └── changes/                             # Ordered, dependency-graphed change proposals
│       ├── 001-demuxer-bitstream.md
│       ├── 002-codecs-presentation.md
│       ├── 003-clock-pipeline.md
│       ├── 004-network-sync-protocol.md
│       ├── 005-smart-chat-storage.md
│       └── 006-ui-osd-renderer.md
├── .pi/
│   ├── agents/                              # Specialized Pi coding agent prompts
│   ├── skills/                              # Operational Pi skills
│   └── workflows/                           # Execution workflows
├── crates/
│   ├── syncplay-common/                     # Shared types, timestamps, CRC32, serialization
│   ├── syncplay-demux/                      # EBML/MKV and MP4 parsers + synthetic vectors
│   ├── syncplay-codecs/                     # Frame models, Q8 YUV-to-RGB converter
│   ├── syncplay-clock/                      # Monotonic clock, PTS tracking, SPSC ring buffer
│   ├── syncplay-sync/                       # NTP filter, discrete PI PLL, barrier unpause
│   ├── syncplay-chat/                       # syncplay_chat.bin log, inverted index, slash commands
│   ├── syncplay-ui/                         # Lock-free atomic snapshot UI state
│   └── syncplay-core/                       # Top-level runtime coordinator
└── scripts/
    ├── audit_dependencies.sh                # Blacklist inspection script
    └── verify_phase1.sh                     # 7-stage automated verification harness
```
