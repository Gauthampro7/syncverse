# Syncverse: Master Project Specification

**Status**: Living Architectural Baseline  
**Target Milestone**: Phase 4 Desktop Graphical UI & Universal Media Compatibility  
**Authoritative Scope**: Standalone Native Media Player with VLC-Grade GUI and Universal Format Compatibility  

---

## 1. System Vision & Architecture Philosophy

Syncverse is an open-source, ultra-efficient standalone desktop media player and long-distance synchronized watch-party platform built in pure Rust. Traditional synchronized media solutions function as fragile scripting wrappers around external monolithic players (e.g., `mpv`, `vlc`) or depend on external database daemons (`sqlite`, `rocksdb`). 

Syncverse fundamentally diverges from these approaches by implementing a standalone native architecture with universal format compatibility:
1. **Standalone Native Architecture with Universal Media Compatibility**: Syncverse is a standalone player running its own presentation and decoding pipeline, strictly eliminating external monolithic player scripting/IPC wrappers (`mpv`, `vlc`) and external database engines (`sqlite`, `diesel`, `rocksdb`). Universal container demuxing (MKV/WebM, MP4/MOV, AVI, MPEG-TS, FLV, OGG) and video/audio bitstream decoding (H.264, H.265, VP8, VP9, AV1, MPEG-2/4, VC-1; AAC, MP3, Opus, Vorbis, FLAC, AC3, DTS, PCM) are handled via direct multimedia library bindings (`ffmpeg-next`, `symphonia`), feeding into in-tree planar YUV420p, packed RGBA, and PCM audio models. Smart chat persistence is built from first principles with an in-tree binary log (`syncplay_chat.bin`) and inverted index.
2. **Deterministic Mathematical Synchronization**: A network engine combining RFC 5905 NTP 4-timestamp exchange, sliding-window minimum RTT filtering with dispersion freezing defense, discrete Proportional-Integral (PI) Phase-Locked Loop (PLL) drift steering ($r \in [0.96, 1.04]$), and two-phase barrier unpausing capable of maintaining cross-peer audio/video alignment within $< 15\text{ ms}$ over 250–400ms high-jitter WAN links.
3. **Decoupled Lock-Free Threading & Native Desktop Graphical UI**: A modern, hardware-accelerated desktop player window (`eframe`/`egui`/`wgpu`) running natively on Linux (Wayland/X11) and Windows. Cross-thread communication uses 128-byte cache-line aligned Single-Producer Single-Consumer (SPSC) ring buffers and triple-buffered atomic UI snapshots (`PlayerUiSnapshot`, 72 bytes) published via a centralized Telemetry Aggregator, guaranteeing sub-microsecond handoffs, zero mutex contention, zero audio buffer underruns, and desktop-grade UI responsiveness (< 16ms input-to-visual latency).
4. **Locally Persisted Dual-Temporal Smart Chat**: A crash-resilient append-only binary log (`syncplay_chat.bin`) with `0xAA55` framing and CRC32 verification, an in-memory inverted index supporting sub-millisecond full-text search, moving-window timeline queries for on-screen bullet comments, and interactive slash-commands (`/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`).

---

## 2. Architectural Invariants & Prohibitions

All crates and components within the Syncverse workspace MUST strictly adhere to the following architectural invariants:

### 2.1 Preserved Architectural Invariants & Forbidden Frameworks
The workspace MUST NOT link against, wrap, or depend on external monolithic player engines or external database systems. Specifically:
- **Forbidden Monolithic Player Frameworks & IPC Wrappers**: `mpv`, `mpv-sys`, `libmpv`, `vlc`, `libvlc`, and any subprocess/IPC wrapper around external player binaries. Syncverse MUST execute presentation, decoding, and playback control within its own native process space.
- **Forbidden Database & Search Engines**: `sqlite`, `sqlite3`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `rocksdb-sys`, `heed`, `tantivy`, `meilisearch`, `diesel`. All chat persistence and full-text search MUST use the in-tree `syncplay_chat.bin` binary log and inverted index.
- **Permitted Multimedia & GUI Dependencies**: Pure-Rust utility primitives such as `thiserror`, standard library networking/threading (`std::net`, `std::sync::atomic`, `std::time::Instant`), direct multimedia libraries (`ffmpeg-next`, `ffmpeg-sys-next`, `symphonia`, `symphonia-core`), native audio output sinks (`cpal`, `rodio`), and cross-platform desktop GUI libraries (`eframe`, `egui`, `wgpu`, `winit`, `rfd`).

### 2.2 Memory Safety & Concurrency Invariant
- `#![deny(unsafe_code)]` SHALL be enforced across all crates by default.
- Any necessary unsafe code (restricted strictly to FFI multimedia bindings, SIMD color space intrinsics, and lock-free atomic slots) MUST be isolated, heavily audited, and documented with explicit `// SAFETY:` proofs.
- Lock-free memory orderings MUST use explicit `Acquire`/`Release` semantics. No spinlocks or busy-waits are permitted on the UI event thread or audio rendering callback.

### 2.3 Monotonic Clocking Invariant
- Media presentation timing and network drift calculations MUST use monotonic time sources (`std::time::Instant` backed by `CLOCK_MONOTONIC`).
- Wall-clock time (`std::time::SystemTime` / `CLOCK_REALTIME`) MUST NOT be used for timeline pacing, frame scheduling, or PLL steering, as it is vulnerable to external NTP slewing, leap seconds, and administrative step adjustments.

---

## 3. Subsystem Topology & Crates

The codebase is organized into an 8-crate workspace under `crates/`:

```
┌────────────────────────────────────────────────────────────────────────┐
│                             syncverse-core                              │
│         Top-level runtime coordinator & application orchestrator        │
└──────────────┬─────────────────────────┬──────────────────────┬────────┘
               │                         │                      │
               ▼                         ▼                      ▼
┌────────────────────────┐  ┌────────────────────────┐  ┌────────────────────────┐
│      syncverse-ui       │  │     syncverse-sync      │  │     syncverse-chat      │
│  eframe/egui desktop   │  │  NTP filter, discrete  │  │  Custom binary log,    │
│  window, GPU viewport  │  │  PI PLL, barrier sync  │  │  inverted index & cmds │
└──────────────┬─────────┘  └────────────┬───────────┘  └───────────┬────────────┘
               │                         │                          │
               └─────────────────────────┼──────────────────────────┘
                                         │
                                         ▼
                            ┌────────────────────────┐
                            │     syncverse-clock     │
                            │  Master clock, audio   │
                            │  pacer & SPSC buffers  │
                            └────────────┬───────────┘
                                         │
                         ┌───────────────┴───────────────┐
                         ▼                               ▼
            ┌────────────────────────┐      ┌────────────────────────┐
            │    syncverse-demux      │      │    syncverse-codecs     │
            │  UniversalDemuxer &    │      │  UniversalDecoder,     │
            │  pure-Rust demuxers    │      │  YUV420p, RGBA, PCM    │
            └────────────┬───────────┘      └────────────┬───────────┘
                         │                               │
                         └───────────────┬───────────────┘
                                         │
                                         ▼
                            ┌────────────────────────┐
                            │    syncverse-common     │
                            │  MediaTime, CRC32,     │
                            │  ByteReader, errors    │
                            └────────────────────────┘
```

### 3.1 Crate Responsibilities
1. **`syncverse-common`**: Foundational types (`MediaTime`, `WallTime`, `Rational`), byte and bitstream readers (`ByteReader`, `BitReader`), IEEE 802.3 CRC32 checksum engine, and unified error models.
2. **`syncverse-demux`**: Universal container demuxer (`UniversalDemuxer` via `ffmpeg-next`) supporting MKV/WebM, MP4/MOV, AVI, MPEG-TS, FLV, OGG, alongside in-tree zero-dependency demuxers (`EbmlDemuxer`, `IsoBmffDemuxer`) for synthetic test vectors.
3. **`syncverse-codecs`**: Universal video and audio decoders (`UniversalDecoder` via `ffmpeg-next`), planar YUV420p (`VideoFrameYUV420p`), packed RGBA32 (`VideoFrameRGBA`), and normalized audio PCM (`AudioFramePCM`, 48kHz, stereo, f32) memory models, and pure-Rust Q8 fixed-point color space conversion.
4. **`syncverse-clock`**: Monotonic presentation clock with continuous SeqLock rate-shift re-anchoring, `AudioPacer` with silence frame underrun protection, and 128-byte cache-aligned lock-free Single-Producer Single-Consumer (SPSC) ring buffers.
5. **`syncverse-sync`**: Long-distance high-RTT synchronization protocol featuring RFC 5905 4-timestamp probe exchange, sliding-window minimum RTT filtering with effective dispersion floor $D_{\text{effective}} \ge 5.0\text{ms}$, deterministic reference timeline equation, discrete PI PLL micro-speed controller ($r \in [0.96, 1.04]$), and two-phase barrier unpause state machine.
6. **`syncverse-chat`**: Crash-resilient append-only binary chat engine (`syncplay_chat.bin`) with `0xAA55` framing and CRC32 checks, in-memory inverted index FTS, timeline overlay window query, and slash-command grammar (`/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`).
7. **`syncverse-ui`**: High-responsiveness (< 16ms input latency) desktop graphical user interface built on `eframe`/`egui`/`wgpu` hosting GPU video viewport with aspect ratio preservation (letterboxing/pillarboxing), bottom playback controls bar, 10Hz debounced interactive seekbar, collapsible watch-party sidebar, native file dialogs (`rfd`), and headless/terminal fallback.
8. **`syncverse-core`**: Top-level coordinator binding demuxing, decoding, clock, audio output sink (`cpal`), sync, chat, and GUI threads into a unified application lifecycle with centralized `TelemetryAggregator`.

---

## 4. End-to-End Threading & Data-Flow Architecture

Work is strictly partitioned across isolated threads communicating via lock-free primitives:

```
                          ┌──────────────────────────┐
                          │   Media File / Network   │
                          └────────────┬─────────────┘
                                       │
                                       ▼ [ReadSeek / File Path]
                          ┌──────────────────────────┐
                          │ Universal Demuxer Thread │
                          │ (MKV, MP4, AVI, TS, OGG) │
                          └────────────┬─────────────┘
                                       │
                                       ▼ [Lock-Free SPSC DemuxPacket Queue]
                          ┌──────────────────────────┐
                          │ Universal Decoder Thread │
                          │ (Video & Audio Decoding) │
                          └────────────┬─────────────┘
                                       │
            ┌──────────────────────────┴──────────────────────────┐
            ▼ [SPSC VideoFrame Queue]                             ▼ [SPSC AudioFrame Queue]
┌──────────────────────────┐                              ┌──────────────────────────┐
│   Presentation Thread    │◄──────────────┐              │     Audio Sink Thread    │
│  (GPU Texture Upload /   │               │              │   (cpal / PipeWire /     │
│   VSync Compositing)     │               │              │    ALSA Output Stream)   │
└───────────┬──────────────┘               │              └────────────┬─────────────┘
            │                              │                           │
            ▼                              │                           ▼
┌──────────────────────────┐      [Atomic Snapshot]       ┌────────────┴─────────────┐
│   eframe / egui GUI Loop │◄─────────────────────────────┤       Master Clock       │
│ - Wayland/X11 window     │                              │  PTS(t) = anchor_pts +   │
│ - GPU Video Viewport     ├─────────────────────────────►│    (t - anchor_wall) * r │
│ - Bottom Controls Bar    │      [SPSC Command Queue]    └──────────┬───────────────┘
│ - Watch-Party Sidebar    │                                         │
└──────────┬───────────────┘                                         │
           │                                                         │
           ▼                                                         ▼
┌──────────────────────────┐                              ┌──────────────────────────┐
│     Smart Chat Engine    │                              │       Sync Engine        │
│ - syncplay_chat.bin log  │                              │ - UDP NTP 4-timestamp    │
│ - Inverted index search  │                              │ - RFC 5905 min-RTT filter│
│ - Slash-command dispatch ├─────────────────────────────►│ - Discrete PI PLL speed │
└──────────────────────────┘      [Sync Wire Messages]    │ - Two-phase barrier start│
                                                          └──────────────────────────┘
```

---

## 5. Subsystem Interface Contracts

### 5.1 Demuxer ↔ Core Contract
```rust
pub trait Demuxer: Send {
    fn probe(reader: &mut (dyn SeekableStream + Send)) -> Result<bool, DemuxError> where Self: Sized;
    fn init(&mut self, reader: Box<dyn SeekableStream + Send>) -> Result<ContainerInfo, DemuxError>;
    fn next_packet(&mut self) -> Result<Option<DemuxPacket>, DemuxError>;
    fn seek_to(&mut self, target_pts: MediaTime) -> Result<MediaTime, DemuxError>;
}
```

### 5.2 Codec & Frame Contract
```rust
pub trait VideoDecoder: Send {
    fn decode_packet(&mut self, packet: &DemuxPacket) -> Result<Vec<VideoFrameYUV420p>, CodecError>;
    fn flush(&mut self) -> Result<(), CodecError>;
}

pub trait AudioDecoder: Send {
    fn decode_packet(&mut self, packet: &DemuxPacket) -> Result<Vec<AudioFramePCM>, CodecError>;
    fn flush(&mut self) -> Result<(), CodecError>;
}

pub trait ColorSpaceConverter: Send + Sync {
    fn convert_yuv420p_to_rgba(
        &self,
        y: &[u8],
        u: &[u8],
        v: &[u8],
        y_stride: usize,
        uv_stride: usize,
        rgba_out: &mut [u8],
        width: u32,
        height: u32,
    ) -> Result<(), CodecError>;
}
```

### 5.3 Clock & Presentation Contract
```rust
pub trait MonotonicClock: Send + Sync {
    fn current_pts(&self) -> MediaTime;
    fn set_playback_rate(&self, rate: f64);
    fn pause_at(&self, pts: MediaTime);
    fn resume_at(&self, pts: MediaTime, wall_instant: std::time::Instant);
}
```

### 5.4 UI Command Contract
```rust
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerCommand {
    TogglePause,
    Stop,
    Seek(u64),
    SetSpeed(f64),
    SetVolume(f32),
    ToggleMute,
    OpenFile(std::path::PathBuf),
    SendChatMessage(String),
    ExecuteSlashCommand(String),
}
```

### 5.5 Network Sync Engine Contract
```rust
pub trait SyncEngine: Send {
    fn process_incoming_packet(&mut self, packet_bytes: &[u8]) -> Result<(), SyncError>;
    fn evaluate_drift(&mut self, current_playback_pts: MediaTime) -> DriftAction;
    fn initiate_barrier_unpause(&mut self, target_pts: MediaTime) -> Result<(), SyncError>;
}
```

### 5.6 Smart Chat & Storage Engine Contract
```rust
pub trait ChatEngine: Send + Sync {
    fn append_message(&mut self, sender: &str, text: &str, media_pts: Option<MediaTime>) -> Result<u64, ChatError>;
    fn search_fts(&self, query: &str) -> Vec<SearchResult>;
    fn query_timeline_window(&self, center_pts: MediaTime, window_ms: u64) -> Vec<ChatRecordMeta>;
    fn execute_slash_command(&mut self, command_line: &str) -> Result<SlashCommandResult, ChatError>;
    fn export_transcript(&self, format: TranscriptFormat) -> String;
}
```

---

## 6. Verification and Acceptance Criteria

The system architecture is validated against concrete criteria:
1. **Preserved Invariants & Audit Cleanliness**: Static verification confirming 0 banned monolithic player wrappers (`mpv`, `vlc`) and 0 banned database engines (`sqlite`) via `./scripts/audit_dependencies.sh`.
2. **Universal Format Compatibility**: Smooth demuxing and decoding across MKV, MP4, WebM, AVI containers with H.264, H.265, VP9, AV1 video and AAC, MP3, Opus, Vorbis audio.
3. **Desktop Graphical UI Responsiveness**: Launching the application opens an `eframe`/`egui` desktop window with GPU video viewport preserving aspect ratio (letterboxing/pillarboxing), bottom playback controls bar, 10Hz debounced interactive seekbar, watch-party sidebar, and sub-16ms latency.
4. **Deterministic High-RTT Sync**: Automated simulation across 250–400ms RTT and $\pm 30\text{ms}$ jitter verifying the NTP filter and discrete PI PLL converge phase error to $< 15\text{ ms}$ without a hard seek for drifts $< 1.5\text{s}$, and barrier unpause starts across peers within $< 10\text{ ms}$.
5. **Chat Persistence & FTS**: Binary log crash-recovery, IEEE 802.3 CRC32 verification, Boolean AND search, and temporal overlay queries.
6. **Compilation Cleanliness**: Zero warnings under `cargo check --workspace` and `cargo test --workspace`.
