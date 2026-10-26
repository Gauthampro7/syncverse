# Project: Syncplay-Alt Phase 2 Implementation

## Architecture
Syncplay-Alt is an open-source, ultra-efficient standalone media player and long-distance synchronized watch-party platform in pure, idiomatic Rust with zero external media, codec, or database frameworks.

### Subsystem Boundaries & Data Flow
1. **Container Demuxing (`syncplay-demux`)**:
   - Parses MKV/WebM (EBML) and MP4/MOV (ISOBMFF).
   - Produces sequential `DemuxPacket` payloads (PTS/DTS, track index, byte slice).
   - Keyframe index lookup and random-access seeking (`seek_to(pts)`).
   - Multi-track stream demuxing (video, audio, subtitles).

2. **Presentation Pipeline (`syncplay-codecs`, `syncplay-clock`, `syncplay-ui`)**:
   - `syncplay-codecs`: Fixed-point Q8 color space converter ($Y'CbCr \to R'G'B'$) for BT.601 and BT.709 with 16-byte SIMD row alignment. Planar YUV420p and packed RGBA frame representations.
   - `syncplay-clock`: Master monotonic presentation clock with atomic SeqLock timeline re-anchoring ensuring $C^0$ continuity. Cache-aligned 128B SPSC lock-free ring buffer for inter-thread audio/video transfer.
   - `syncplay-ui`: Lock-free atomic triple buffer publishing `PlayerUiSnapshot` (72B). 10Hz scrubber debouncer and memory-bounded OSD rendering ($\le 15\text{MB}$ RAM budget).

3. **High-RTT Network Watch-Party Transport (`syncplay-sync`)**:
   - Real UDP peer-to-peer and client-server datagram transport.
   - RFC 5905 NTP 4-timestamp exchange ($\hat{\theta}$, RTT, $D_{\text{effective}} \ge 5.0\text{ms}$).
   - Discrete PI PLL micro-speed controller ($K_p = 0.17, K_i = 0.0072$) maintaining $|e| < 15\text{ms}$ phase alignment across $250\text{--}400\text{ms}$ jittery WAN links.
   - Two-phase barrier unpause protocol guaranteeing $< 10\text{ms}$ cross-peer start alignment.

4. **Smart Chat & Media Slash-Commands (`syncplay-chat`)**:
   - Append-only binary log `syncplay_chat.bin` with `0xAA55` framing, dual timestamps ($t_{wall}, t_{media}$), UUID sender tracking, and IEEE 802.3 CRC32 checksums.
   - Crash-recovery forward scanner recovering from corrupted bytes and false-sync candidate patterns without data loss.
   - In-memory inverted index FTS with Unicode case folding, CJK unigram/bigram segmentation, and single ideograph indexing.
   - Interactive media slash-command parser and dispatcher (`/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`).

5. **Runtime Coordinator & CLI (`syncplay-core`)**:
   - Binds demuxer, decoder, clock, sync, chat, and presentation loops.
   - Telemetry aggregator publishing unified `PlayerUiSnapshot` to UI.
   - CLI subcommands: `inspect <file>`, `play <file>`, `party [host|connect]`, `chat demo`, `verify`.

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | MKV/WebM Sequential Packet Demuxing | Demux sequential audio/video `DemuxPacket`s with PTS/DTS | M1 | R1 (`001-demuxer-bitstream`) [VERIFIED] |
| 2 | MP4/ISOBMFF Sequential Packet Demuxing | Demux sequential audio/video `DemuxPacket`s with PTS/DTS | M1 | R1 (`001-demuxer-bitstream`) [VERIFIED] |
| 3 | Keyframe Index Lookup & Seeking | `seek_to(pts)` seeking to preceding sync sample | M1 | R1 (`001-demuxer-bitstream`) [VERIFIED] |
| 4 | Multi-track Stream Parsing | Video, audio, and timed text stream track separation | M1 | R1 (`001-demuxer-bitstream`) [VERIFIED] |
| 5 | Q8 Fixed-Point Color Conversion | Pure-Rust BT.601/BT.709 conversion with 16B SIMD alignment | M2 | R2 (`002-codecs-presentation`) |
| 6 | 128B Cache-Aligned SPSC Ring Buffer | Lock-free audio/video sample ring buffer | M2 | R2 (`003-clock-pipeline`) |
| 7 | Master Monotonic Presentation Clock | Continuous presentation loop with atomic re-anchoring | M2 | R2 (`003-clock-pipeline`) |
| 8 | 10Hz Scrubber Debouncer | 10Hz rate limiter on drag events with release seek | M2 | R2 (`006-ui-osd-renderer`) |
| 9 | Memory-Bounded OSD State Rendering | Overlay capped at 5 bubbles, $\le 15\text{MB}$ UI RAM | M2 | R2 (`006-ui-osd-renderer`) |
| 10 | Real UDP Transport | UDP socket transport for peer-to-peer / client-server | M3 | R3 (`004-network-sync-protocol`) |
| 11 | RFC 5905 NTP Probe Exchange | 4-timestamp probe exchange, $\hat{\theta}$, RTT, $D_{\text{eff}} \ge 5\text{ms}$ | M3 | R3 (`004-network-sync-protocol`) |
| 12 | Discrete PI PLL Micro-Speed Steering | $K_p = 0.17, K_i = 0.0072$, $|e| < 15\text{ms}$ phase alignment | M3 | R3 (`004-network-sync-protocol`) |
| 13 | Two-Phase Barrier Unpause Protocol | Cross-peer playback resume alignment $< 10\text{ms}$ | M3 | R3 (`004-network-sync-protocol`) |
| 14 | Binary Chat Log Persistence | `syncplay_chat.bin` with `0xAA55` framing, dual timestamps, CRC32 | M4 | R4 (`005-smart-chat-storage`) |
| 15 | False-Sync Crash Recovery Scanner | Forward scanner recovering from corrupted bytes | M4 | R4 (`005-smart-chat-storage`) |
| 16 | Inverted Index FTS | Case folding, CJK unigrams/bigrams, single ideographs | M4 | R4 (`005-smart-chat-storage`) |
| 17 | Media Slash-Command Dispatcher | `/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status` | M4 | R4 (`005-smart-chat-storage`) |
| 18 | Central Telemetry Aggregator | Polls subsystems, publishes lock-free `PlayerUiSnapshot` | M5 | R2 / R5 (`syncplay-core`) |
| 19 | CLI Subcommands (`inspect`, `play`, `party`, `chat`, `verify`) | Complete executable CLI functionality | M5 | CLI Requirements (`syncplay-core`) |
| 20 | Pi Intent Verification & Zero-Drift Defense | 100% Intent Compliance, zero-dependency audit, verify_phase1.sh | M6 | R5 (`scripts/verify_intent.sh`) |

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| Survey | Deep Technical Survey | Map exact requirements, existing code, and test vectors | none | DONE |
| M1 | Container Demuxing & Packet Processing | `crates/syncplay-demux` | Survey | DONE (GATE PASSED) |
| M2 | Video/Audio Presentation Engine | `crates/syncplay-codecs`, `syncplay-clock`, `syncplay-ui` | Survey | DONE (GATE PASSED) |
| M3 | Network Watch-Party Transport | `crates/syncplay-sync` | Survey | DONE (GATE PASSED) |
| M4 | Smart Chat & Slash Commands | `crates/syncplay-chat` | Survey | DONE (GATE PASSED) |
| M5 | Top-Level Coordinator & CLI | `crates/syncplay-core` | M1, M2, M3, M4 | DONE (GATE PASSED) |
| M6 | Final Verification & Victory Audit | System-wide integration, Intent Verification, scripts | M5 | DONE (GATE PASSED) |

## Interface Contracts
- `Demuxer` trait: `fn read_packet(&mut self) -> Result<Option<DemuxPacket>, DemuxError>; fn seek_to(&mut self, pts: MediaTime) -> Result<MediaTime, DemuxError>; fn tracks(&self) -> &[StreamTrack];`
- `ColorConverter`: `fn yuv420p_to_rgba(src: &VideoFrameYUV420p, dst: &mut VideoFrameRGBA, matrix: ColorMatrix) -> Result<(), CodecError>;`
- `MasterClock`: `fn current_pts(&self, now: WallTime) -> MediaTime; fn set_speed(&self, now: WallTime, speed: f64);`
- `SpscRingBuffer<T>`: `fn try_push(&self, item: T) -> Result<(), T>; fn try_pop(&self) -> Option<T>;`
- `SyncEngine`: `fn handle_probe_response(&mut self, probe: &NtpProbe) -> Result<(), SyncError>; fn update_pll(&mut self, local_wall: WallTime, target_pts: MediaTime, current_pts: MediaTime) -> PllAction;`
- `ChatStore`: `fn append(&mut self, record: &ChatRecord) -> Result<u64, ChatError>; fn search(&self, query: &str) -> Vec<SearchResult>;`
- `PlayerCoordinator`: `fn inspect(path: &Path) -> Result<MediaInfo, CoreError>; fn play(path: &Path) -> Result<(), CoreError>; fn party(role: PartyRole) -> Result<(), CoreError>;`

## Code Layout
```
crates/
├── syncplay-common/     # Shared types, timestamps, CRC32, bitstream readers, errors
├── syncplay-demux/      # Pure-Rust EBML/MKV & ISOBMFF/MP4 container demuxers + vectors [VERIFIED]
├── syncplay-codecs/     # Planar YUV420p / packed RGBA, Q8 fixed-point color converter
├── syncplay-clock/      # Master monotonic clock, 128B-aligned SPSC ring buffer
├── syncplay-sync/       # RFC 5905 NTP filter, discrete PI PLL micro-speed, UDP transport
├── syncplay-chat/       # syncplay_chat.bin log, false-sync recovery, inverted index FTS
├── syncplay-ui/         # Lock-free atomic triple-buffered snapshot, debouncer, OSD
└── syncplay-core/       # Top-level runtime coordinator, CLI entrypoint, telemetry aggregator
```
