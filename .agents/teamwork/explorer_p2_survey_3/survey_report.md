# Technical Survey Report: Network Watch-Party Transport, Smart Chat, Pi Intent Verification, and CLI Functionality

**Target Milestone**: Phase 2 Architecture & Worker Implementation  
**Author**: `explorer_p2_survey_3` (teamwork_preview_explorer)  
**Authority**: `ORIGINAL_REQUEST.md`, `AGENTS.md`, OpenSpec Specifications (`sync-engine`, `smart-chat`), Pi Framework  
**Date**: 2026-10-25  

---

## 1. Executive Summary

This report delivers a deep technical survey of **Requirement 3 (Network Watch-Party Transport in `syncplay-sync`)**, **Requirement 4 (Smart Chat & Media Slash-Commands in `syncplay-chat`)**, **Requirement 5 (Pi Intent Verification Harness & Zero-Drift Defense)**, and **CLI Executable Functionality in `syncplay-core`**.

### Current State Assessment:
- **Phase 1 Foundational Primitives**: All 8 crates compile with zero warnings under `#![deny(warnings)]`, `#![deny(missing_docs)]`, and `#![deny(unsafe_code)]` (except justified SIMD/SPSC primitives). The mathematical core for RFC 5905 NTP filtering, discrete PI PLL micro-speed control, two-phase barrier unpause, binary chat log persistence (`syncplay_chat.bin`), inverted index CJK full-text search, and slash-command parsing is fully implemented and passes all unit tests, empirical Python challenge suites, and the Pi Intent Verification Harness (100.0% Intent Compliance Score).
- **Phase 2 Implementation Gaps**:
  1. `syncplay-sync` currently contains the offline mathematical models and state transitions, but **lacks the real UDP datagram networking layer**, packet serialization/deserialization wire framing, client/server peer session loops, and network-driven clock synchronization.
  2. `syncplay-chat` possesses the binary log format, recovery scanner, FTS inverted index, and command parser, but **lacks an active slash-command dispatcher** that translates commands into player and network events, network broadcast integration for chat records, and dedicated CLI queries/exports.
  3. `syncplay-core` provides `PlayerCoordinator` and `TelemetryAggregator`, but `syncplay-alt party` and `syncplay-alt play` currently execute simulated/mocked tick sequences rather than live network transport and demuxer/codec pipeline presentation.
  4. The **Pi Intent Verification Harness** currently tests empirical simulations and static audits; as real UDP networking and full playback are implemented, the harness must maintain 100% compliance without introducing external dependencies or regressions.

---

## 2. Deep Dive: Subsystem Architecture & Requirements

### 2.1 R3: Network Watch-Party Transport (`syncplay-sync`)

#### Existing Primitives:
- **`crates/syncplay-sync/src/ntp.rs`**:
  - `ProbeSample`: 4-timestamp measurement ($T_1, T_2, T_3, T_4$), calculating clock offset $\theta = \frac{(T_2 - T_1) + (T_3 - T_4)}{2}$ and $\text{RTT} = (T_4 - T_1) - (T_3 - T_2)$.
  - `Rfc5905Filter`: 8-sample sliding window, minimum RTT selection for $\hat{\theta}$, dispersion computation with $D_{\text{effective}} = \max(D, 5.0\text{ms})$ floor, outlier rejection ($s.\text{rtt} > 2.0 \cdot \text{median}(\text{rtt})$ or $|s.\text{offset} - \hat{\theta}| > 3 D_{\text{effective}}$), and 8-rejection watchdog flush.
- **`crates/syncplay-sync/src/pll.rs`**:
  - `PllSpeedController`: Discrete PI controller evaluated at interval $\Delta t = 0.2\text{s}$.
  - Critically damped gains: $K_p = 0.17\text{ s}^{-1}$, $K_i = 0.0072\text{ s}^{-2}$, anti-windup clamp $I_{\text{max}} = 32.0\text{s}$.
  - Tri-zone drift policy: Zone 1 Deadband ($|e| < 15\text{ms}$, $r = 1.0\times$); Zone 2 Micro-Speed Slew ($15\text{ms} \le |e| \le 1500\text{ms}$, $r \in [0.96, 1.04]$); Zone 3 Discontinuous Hard Seek ($|e| > 1500\text{ms}$).
- **`crates/syncplay-sync/src/timeline.rs`**:
  - `RoomState`: Reference timeline mapping room master clock to local presentation:
    $$\tau_{\text{target}}(t_{\text{local}}) = \tau_{\text{ref\_media}} + r_{\text{target}} \cdot [(t_{\text{local}} + \hat{\theta}) - t_{\text{ref\_server}}]$$
- **`crates/syncplay-sync/src/barrier.rs`**:
  - `compute_target_unpause_server(t_server_now_ms, max_rtt_ms, preroll_budget_ms)`:
    $$T_{\text{target\_server}} = t_{\text{server\_now}} + 1.5 \cdot \max_i(\text{RTT}_i) + t_{\text{preroll\_budget}} + 100\text{ms}$$
  - `BarrierStateMachine`: States `Idle`, `Preparing`, `Ready`, `Committed`, `Active`.

#### Required New Implementations for Phase 2:
1. **Binary Wire Framing & Packet Codec (`syncplay-sync::wire`)**:
   - Pure-Rust packet serialization and deserialization without external serde/bincode dependencies (using `syncplay-common::ByteReader`, `BitReader`, and table-driven `Crc32`).
   - Magic: `b"SPNT"` (4 bytes), Version: `1` (2 bytes), Packet Type: `u8`, Flags: `u8`, Sequence: `u64`, Payload Length: `u32`, Payload Bytes, CRC32: `u32`.
2. **Real UDP Transport Engine (`syncplay-sync::transport`)**:
   - `UdpSyncTransport`: Encapsulates non-blocking `std::net::UdpSocket`.
   - Host/Server session manager: binds port (default `18242`), maintains client registry (`HashMap<SocketAddr, PeerSession>`), tracks peer RTT and readiness, periodically broadcasts `RoomStateBroadcast` (10 Hz).
   - Client session manager: connects to host IP:port, initiates periodic NTP probes (interval 200–500ms), calculates $\hat{\theta}$, tracks phase error against room reference timeline, and adjusts master clock rate.
3. **Barrier Unpause Network Coordination**:
   - Host broadcasts `BarrierPrepare`.
   - Clients pre-roll audio/video pipelines and transmit `BarrierReady`.
   - Host confirms all peers ready (with timeout abort recovery) and broadcasts `BarrierCommit`.
   - Clients unpause presentation at exact monotonic instant $T_{\text{target\_local}} = T_{\text{target\_server}} - \hat{\theta}$.

---

### 2.2 R4: Smart Chat & Media Slash-Commands (`syncplay-chat`)

#### Existing Primitives:
- **`crates/syncplay-chat/src/record.rs`**:
  - `ChatRecord`: Dual timestamps (`wall_timestamp_utc_ms`, `media_timestamp_ms`), 16-byte sender UUID, sender name (up to 64 bytes), payload.
  - Wire layout: `SYNC_WORD = [0xAA, 0x55]`, `total_len` (`body.len() + 10 >= 48`), body, IEEE 802.3 CRC32.
  - Header: 16-byte fixed file header with magic `b"SPCH"`, version `1`, CRC32.
- **`crates/syncplay-chat/src/storage.rs`**:
  - `ChatLogStorage`: Append-only disk logging to `syncplay_chat.bin`.
  - `ChatRecoveryScanner`: Forward scanner recovering from mid-stream byte corruption and false-sync candidate patterns (`offset += 1`), detecting trailing torn writes at EOF.
- **`crates/syncplay-chat/src/index.rs`**:
  - `InvertedIndexFts`: In-memory inverted index, posting lists `BTreeMap<String, Vec<usize>>`, spatial timeline `BTreeMap<u64, Vec<usize>>`.
  - Tokenization: Unicode case folding (including Turkish dotted I), punctuation stripping (preserving `#` and `@`), CJK unigram and overlapping bigram segmentation (`U+4E00..U+9FFF`), single ideograph exemption (1..48 chars), Western stop-word filtering.
  - Multi-term Boolean AND search via linear two-pointer merge.
  - Temporal spatial window queries `query_timeline_window(current_pts, past_ms, future_ms)`.
- **`crates/syncplay-chat/src/commands.rs`**:
  - `SlashCommandParser`: Parses `/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`.
  - Timecode normalization: `HH:MM:SS.mmm`, `MM:SS`, relative offsets (`+15s`, `-10s`), decimal seconds to milliseconds.
- **`crates/syncplay-chat/src/export.rs`**:
  - `export_transcript_markdown` and `export_history_json`.

#### Required New Implementations for Phase 2:
1. **Slash-Command Dispatcher (`syncplay-chat::commands::dispatch`)**:
   - `SlashCommandDispatcher`: Trait and execution engine routing parsed commands:
     - `/seek <time>` $\to$ emits `PlayerCommand::Seek(target_ms)` to clock and broadcasts seek request to network sync engine.
     - `/bookmark [label]` $\to$ creates persistent `ChatRecord` with `record_type = 3` (Bookmark) anchored at current media PTS, appends to log, indices in FTS.
     - `/clip <start> <end> [label]` $\to$ validates interval ($start < end$) and persists clip bookmark.
     - `/poll "Q" "O1" "O2"` $\to$ serializes poll structure into command payload and broadcasts to room peers.
     - `/sync-status` $\to$ queries `PlayerUiSnapshot` metrics from Telemetry Aggregator and formats diagnostic report (RTT, offset, PLL speed, peer count).
2. **Network Chat Record Relaying**:
   - Integration with `syncplay-sync` wire transport: when a user sends a chat message or reaction, serialize as `WirePacket::Chat` and broadcast to room.
   - Upon receipt from network, validate CRC32, append to `syncplay_chat.bin`, and insert into in-memory `InvertedIndexFts`.
3. **CLI Chat Interaction**:
   - Subcommand `syncplay-alt chat query "<search terms>"`: loads persistent `syncplay_chat.bin`, builds index, prints matching results with timestamps.
   - Subcommand `syncplay-alt chat export [json|markdown] <path>`: exports historical transcripts to file.

---

### 2.3 R5: Pi Framework Intent Verification Harness & Zero-Drift Defense

#### Current Verification Suite:
- **`scripts/audit_dependencies.sh`**:
  - Verifies zero forbidden multimedia engines (`ffmpeg`, `vlc`, `mpv`, `symphonia`, `gstreamer`, etc.) and database engines (`sqlite`, `rocksdb`, `sqlx`, etc.) in `Cargo.toml` and `Cargo.lock`.
- **`scripts/intent_harness.py`**:
  - Evaluates all 8 foundational intents in `.pi/harness/intent_matrix.json`:
    - `INTENT-01`: Zero-External-Codec Invariant (passes in 0.40s)
    - `INTENT-02`: Deterministic High-RTT Network Clock Synchronization (passes in 0.02s)
    - `INTENT-03`: Barrier-Synchronized Two-Phase Commit Unpause (passes in 0.02s)
    - `INTENT-04`: Crash-Resilient Dual-Temporal Local Smart Chat (passes in 0.02s)
    - `INTENT-05`: Decoupled Lock-Free Threading & VLC-Grade Responsiveness (passes in 0.72s)
    - `INTENT-06`: Zero-Allocation Fixed-Point Color Transformation (passes in 0.22s)
    - `INTENT-07`: Robust Container Demuxing & Timescale Integer Arithmetic (passes in 0.21s)
    - `INTENT-08`: OpenSpec Traceability & Anti-Drift Guarantee (passes in 0.28s)
  - Current Intent Compliance Score: **100.0%**.
- **`scripts/verify_phase1.sh`**:
  - Runs all 8 verification stages end-to-end. Exits 0 cleanly.

#### Architectural Invariants for Phase 2 Workers:
1. **Zero Warnings Policy**: `#![deny(warnings)]` and `cargo clippy --workspace --all-targets -- -D warnings` must exit 0 at all times.
2. **Documentation**: `#![deny(missing_docs)]` is mandatory on all public modules, structs, enums, and functions.
3. **Safety**: `#![deny(unsafe_code)]` remains strictly enforced in `syncplay-sync`, `syncplay-chat`, and `syncplay-core`.
4. **Dependency Whitelist**: Pure standard library (`std::net`, `std::sync::atomic`, `std::time::Instant`, `std::collections::BTreeMap`) and `thiserror`. Absolutely NO external async runtimes (`tokio`, `async-std`), external serialization (`serde`, `bincode`), or networking libraries.

---

### 2.4 CLI Executable Functionality in `syncplay-core`

The binary `syncplay-alt` (entry point `crates/syncplay-core/src/main.rs`) requires concrete implementations for five primary subcommands:

1. **`syncplay-alt inspect <file>`**:
   - Probes and demuxes container headers from real MKV/WebM or MP4/MOV files or synthetic test vectors (`synthetic-mkv`, `synthetic-mp4`).
   - Displays container timescale, duration, track inventory (ID, type, codec string), cluster/box counts, and keyframe seek table.
2. **`syncplay-alt play <file>`**:
   - Instantiates `PlayerCoordinator` and master monotonic clock.
   - Demuxes bitstream packets sequentially from `<file>`.
   - Feeds video/audio frames into cache-aligned SPSC ring buffers.
   - Drives presentation loop with live 10 Hz OSD telemetry snapshot updates (`PlayerUiSnapshot`).
3. **`syncplay-alt party [host|connect <host:port>]`**:
   - `host [port]`: Starts watch-party host server on UDP port (default 18242). Manages room state, serves NTP probes, broadcasts reference timeline, and coordinates barrier unpause.
   - `connect <host:port>`: Connects client node to host. Runs continuous NTP probe loop, filters offset via `Rfc5905Filter`, computes phase error against reference timeline, applies discrete PI PLL micro-speed adjustments to local clock, and logs live convergence telemetry.
4. **`syncplay-alt chat [demo|query <query>|export <fmt> <file>]`**:
   - `demo`: Appends test records with `0xAA55` framing, runs FTS, tests slash-command parser, and generates Markdown transcript.
   - `query "<search term>"`: Loads persistent `syncplay_chat.bin`, searches index via multi-term Boolean AND, and prints matching records.
   - `export [json|markdown] <path>`: Exports full transcript to specified file.
5. **`syncplay-alt verify`**:
   - Programmatically executes `python3 scripts/intent_harness.py` and reports Intent Compliance Score.

---

## 3. Data Structures, Protocols & Wire Formats

### 3.1 Network Datagram Wire Format (`syncplay-sync`)

All network packets are transmitted as UDP datagrams with the following little-endian binary header:

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|       Magic: 'S' 'P' 'N' 'T' (0x53, 0x50, 0x4E, 0x54)         |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|          Version (1)          |  Packet Type  |     Flags     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                        Sequence Number                        |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                         Payload Length                        |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                        Payload Bytes...                       |
|                             ...                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                      CRC32 Checksum (LE)                      |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

#### Packet Type ID Allocation:
| Type ID | Enum Variant | Direction | Description |
|---|---|---|---|
| `0x01` | `ProbeRequest` | Client $\to$ Host | Contains Probe ID (`u64`) and Client transmit time $T_1$ (`u64` nanos). |
| `0x02` | `ProbeResponse` | Host $\to$ Client | Contains Probe ID (`u64`), $T_1$, Host receive $T_2$, Host reply transmit $T_3$. |
| `0x03` | `RoomStateBroadcast` | Host $\to$ All | Monotonic Room State: seq (`u64`), $t_{\text{ref\_server}}$ (`u64`), $\tau_{\text{ref\_media}}$ (`u64`), $r_{\text{target}}$ (`f64`), `is_paused` (`u8`), `peer_count` (`u16`). |
| `0x04` | `BarrierPrepare` | Host $\to$ All | Unpause request: seq (`u64`), target server time $T_{\text{target\_server}}$ (`u64`), $\tau_{\text{unpause}}$ (`u64`). |
| `0x05` | `BarrierReady` | Client $\to$ Host | Pre-roll complete: seq (`u64`), `client_id` (`u64`). |
| `0x06` | `BarrierCommit` | Host $\to$ All | Execution rendezvous: seq (`u64`), $T_{\text{target\_server}}$ (`u64`). |
| `0x07` | `BarrierAbort` | Host $\to$ All | Abort unpause: seq (`u64`), `reason_code` (`u8`). |
| `0x08` | `UserCommand` | Client $\to$ Host | Remote control: command type (`u8`: 0=Play, 1=Pause, 2=Seek), target PTS (`u64`). |
| `0x09` | `ChatMessage` | Any $\to$ Any | Shared chat: dual timestamps, sender UUID, name, UTF-8 payload. |

#### Rust Wire Model Definition:
```rust
/// Syncplay Network Transport packet types.
#[derive(Debug, Clone, PartialEq)]
pub enum WirePacket {
    /// NTP probe request.
    ProbeRequest { probe_id: u64, t1_nanos: u64 },
    /// NTP probe response.
    ProbeResponse { probe_id: u64, t1_nanos: u64, t2_nanos: u64, t3_nanos: u64 },
    /// Room state broadcast.
    RoomStateBroadcast {
        seq: u64,
        t_ref_server_nanos: u64,
        tau_ref_media_ms: u64,
        r_target: f64,
        is_paused: bool,
        peer_count: u16,
    },
    /// Barrier prepare broadcast.
    BarrierPrepare { seq: u64, target_server_ms: u64, tau_unpause_ms: u64 },
    /// Barrier ready report.
    BarrierReady { seq: u64, client_id: u64 },
    /// Barrier commit broadcast.
    BarrierCommit { seq: u64, start_time_server_ms: u64 },
    /// Barrier abort broadcast.
    BarrierAbort { seq: u64, reason_code: u8 },
    /// User command dispatch.
    UserCommand { cmd_type: u8, target_pts_ms: u64 },
    /// Network chat record.
    ChatMessage(ChatRecord),
}
```

### 3.2 Error Types Hierarchy

Workers must maintain strongly typed errors using `thiserror`:

```rust
// In syncplay-sync/src/error.rs
#[derive(Error, Debug, Clone, PartialEq)]
pub enum SyncError {
    #[error("insufficient NTP probe samples in window")]
    InsufficientSamples,
    #[error("NTP probe sample rejected as outlier: {0}")]
    OutlierRejected(String),
    #[error("phase error {0} ms exceeded hard seek threshold")]
    HardSeekRequired(f64),
    #[error("barrier unpause error: {0}")]
    BarrierFailure(String),
    #[error("network I/O error: {0}")]
    Io(String),
    #[error("malformed packet wire data: {0}")]
    MalformedPacket(String),
    #[error("packet CRC32 checksum mismatch")]
    ChecksumMismatch,
    #[error("network timeout waiting for peer response")]
    Timeout,
}

// In syncplay-chat/src/error.rs
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ChatError {
    #[error("invalid header magic: expected SPCH")]
    InvalidHeaderMagic,
    #[error("header CRC32 mismatch")]
    HeaderCrcMismatch,
    #[error("malformed record: {0}")]
    MalformedRecord(String),
    #[error("CRC32 mismatch in record body")]
    CrcMismatch,
    #[error("invalid slash command: {0}")]
    InvalidCommand(String),
    #[error("I/O error: {0}")]
    Io(String),
}
```

---

## 4. Worker Task Breakdown & Implementation Roadmap

To execute Phase 2 cleanly across parallel worker subagents, the implementation should be partitioned into 6 focused work units:

```
┌────────────────────────────────────────────────────────────────────────┐
│                   Phase 2 Implementation DAG                           │
└────────────────────────────────────────────────────────────────────────┘

  [Worker 1: syncplay-sync Wire & Packets]
               │
               ▼
  [Worker 2: syncplay-sync UDP Transport (Host/Client)]
               │
               ├─────────────────────────────────────────┐
               ▼                                         ▼
  [Worker 3: syncplay-sync Barrier Network]  [Worker 4: syncplay-chat Dispatcher & Relay]
               │                                         │
               └─────────────────┬───────────────────────┘
                                 ▼
                     [Worker 5: syncplay-core CLI]
                                 │
                                 ▼
                     [Worker 6: Intent & E2E Validation]
```

### Work Unit 1: Wire Packet Codec in `syncplay-sync`
- **Target Files**: `crates/syncplay-sync/src/wire.rs`, `crates/syncplay-sync/src/lib.rs`
- **Tasks**:
  1. Define `WirePacket` enum and binary wire layout matching specification.
  2. Implement `WirePacket::serialize(&self) -> Vec<u8>` using pure Rust integer encoding and `syncplay_common::Crc32`.
  3. Implement `WirePacket::deserialize(bytes: &[u8]) -> Result<WirePacket, SyncError>` with bounds checking and CRC validation.
  4. Unit test round-trip serialization for all 9 packet types.

### Work Unit 2: UDP Transport & Client/Host Nodes in `syncplay-sync`
- **Target Files**: `crates/syncplay-sync/src/transport.rs`, `crates/syncplay-sync/src/session.rs`, `crates/syncplay-sync/src/lib.rs`
- **Tasks**:
  1. Implement `UdpSyncTransport` wrapping `std::net::UdpSocket` with non-blocking send/receive.
  2. Implement `HostServer`: tracks connected peers in `HashMap<SocketAddr, PeerSession>`, responds to `ProbeRequest` with timestamps $T_2, T_3$, broadcasts `RoomStateBroadcast` at 10 Hz.
  3. Implement `ClientNode`: connects to host, transmits periodic NTP probes, updates `Rfc5905Filter`, calculates $\hat{\theta}$, updates local `RoomState`, evaluates drift via `PllSpeedController`.
  4. Unit test client-host loopback datagram exchange over `127.0.0.1`.

### Work Unit 3: Barrier Unpause Protocol Network Synchronization
- **Target Files**: `crates/syncplay-sync/src/barrier.rs`
- **Tasks**:
  1. Connect `BarrierStateMachine` to UDP packet handlers for `BarrierPrepare`, `BarrierReady`, `BarrierCommit`, and `BarrierAbort`.
  2. Implement ready timeout tracking on host ($t_{\text{server\_now}} + \max(\text{RTT}) + 200\text{ms} + 50\text{ms}$).
  3. Implement client scheduled execution: arm timer to unpause local clock at $T_{\text{target\_local}} = T_{\text{target\_server}} - \hat{\theta}$.
  4. Integration test multi-client barrier synchronization verifying start alignment $< 10\text{ms}$.

### Work Unit 4: Smart Chat Command Dispatcher & Network Relay
- **Target Files**: `crates/syncplay-chat/src/commands/dispatch.rs`, `crates/syncplay-chat/src/lib.rs`
- **Tasks**:
  1. Implement `SlashCommandDispatcher` mapping parsed commands to actionable operations (`Seek`, `Bookmark`, `Clip`, `Poll`, `SyncStatus`).
  2. Implement network relay hook for chat messages: encode `ChatMessage` packet for `syncplay-sync` transport.
  3. Implement CLI search and export helpers in `syncplay-chat`.

### Work Unit 5: CLI Subcommand Implementation in `syncplay-core`
- **Target Files**: `crates/syncplay-core/src/main.rs`, `crates/syncplay-core/src/coordinator.rs`
- **Tasks**:
  1. Enhance `inspect <file>`: output track inventory, duration, timescale, and keyframe seek index.
  2. Implement `play <file>`: drive presentation loop with demuxer packet stream, master clock, and OSD telemetry snapshot publication.
  3. Implement `party host [port]` & `party connect <host:port>`: launch live network transport loops with real UDP datagrams, NTP filtering, and PLL speed control.
  4. Implement `chat query "<terms>"` and `chat export [json|markdown] <path>`.

### Work Unit 6: Pi Intent Verification Harness & E2E Validation
- **Target Files**: `scripts/verify_phase1.sh`, `scripts/intent_harness.py`, `tests/empirical_challenge/`
- **Tasks**:
  1. Verify zero banned dependencies via `./scripts/audit_dependencies.sh`.
  2. Verify workspace compilation and clippy zero-warning policy: `cargo check --workspace --all-targets` and `cargo clippy --workspace --all-targets -- -D warnings`.
  3. Execute full unit test suite: `cargo test --workspace -- --nocapture`.
  4. Run Pi Intent Verification Harness (`./scripts/verify_intent.sh`) verifying 100.0% Intent Compliance Score.
  5. Run `./scripts/verify_phase1.sh` verifying all 8 stages pass cleanly.

---

## 5. Verification Commands for Worker Validation

Workers MUST execute and pass the following sequence of commands before marking their tasks complete:

```bash
# 1. Dependency Invariant Audit (Zero banned multimedia/database frameworks)
./scripts/audit_dependencies.sh

# 2. Workspace Compilation Check (Zero warnings policy)
PATH="$HOME/.cargo/bin:$PATH" cargo check --workspace --all-targets

# 3. Workspace Clippy Lint Check (Zero warnings policy)
PATH="$HOME/.cargo/bin:$PATH" cargo clippy --workspace --all-targets -- -D warnings

# 4. Workspace Unit & Integration Test Suite
PATH="$HOME/.cargo/bin:$PATH" cargo test --workspace -- --nocapture

# 5. Empirical Challenge & Mathematical Invariant Test Suite
python3 tests/empirical_challenge/test_demux_codecs_clock.py
python3 tests/empirical_challenge/test_sync_engine.py
python3 tests/empirical_challenge/test_smart_chat.py
python3 tests/empirical_challenge/test_ui_renderer.py
python3 tests/empirical_challenge/test_pi_adversarial_stress.py

# 6. Pi Framework Intent Verification Harness (100% Intent Compliance)
./scripts/verify_intent.sh

# 7. End-to-End Automated Verification Script
./scripts/verify_phase1.sh

# 8. Executable Binary CLI Validation
PATH="$HOME/.cargo/bin:$PATH" cargo run -p syncplay-core -- inspect synthetic-mkv
PATH="$HOME/.cargo/bin:$PATH" cargo run -p syncplay-core -- inspect synthetic-mp4
PATH="$HOME/.cargo/bin:$PATH" cargo run -p syncplay-core -- play synthetic-mkv
PATH="$HOME/.cargo/bin:$PATH" cargo run -p syncplay-core -- party host
PATH="$HOME/.cargo/bin:$PATH" cargo run -p syncplay-core -- chat demo
PATH="$HOME/.cargo/bin:$PATH" cargo run -p syncplay-core -- verify
```

---

## 6. Conclusion

The foundational primitives across `syncplay-sync`, `syncplay-chat`, `syncplay-core`, and the Pi Framework provide a rock-solid, mathematically proven base. Transitioning to Phase 2 requires concrete networking wire framing and socket transport in `syncplay-sync`, slash-command dispatching and network relay in `syncplay-chat`, and end-to-end CLI command binding in `syncplay-core`. By adhering strictly to the zero-external-codec invariant, pure safe Rust standards, and the 6-work-unit roadmap detailed in this report, the Worker subagents can deliver Phase 2 efficiently with zero architectural drift.
