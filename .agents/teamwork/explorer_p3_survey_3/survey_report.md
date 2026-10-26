# Technical Survey Report: R4 (Watch-Party Networking & In-Band Smart Chat) & R5 (Intent Harness & Zero-Drift Defense)

**Author**: `explorer_p3_survey_3`  
**Working Directory**: `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_survey_3`  
**Workspace Root**: `/home/gautham/syncplay-alt`  
**Target Milestone**: Phase 3 (VLC-Grade Interactive Standalone Player & Synchronized Watch-Party Engine)  
**Date**: 2026-10-25  

---

## 1. Executive Summary

This survey provides an exhaustive architectural, wire-protocol, and mathematical inspection of **Requirement 4 (Full Interactive Multi-Peer Watch-Party Networking)** and **Requirement 5 (Pi Framework Intent Verification Harness & Zero-Drift Defense)** for Phase 3 of Syncplay-Alt.

All foundational mathematical primitives and wire formats built in Phase 1 & 2 are compiling cleanly with zero compiler warnings and zero clippy warnings. All 11 empirical challenge test suites currently pass 100%, and the Intent Verification Harness reports an Intent Compliance Score of 100.0% (8/8 intents verified, 0 drift).

However, a significant implementation gap exists between the current Phase 2 milestone code and the authoritative Phase 3 requirements:
1. **CLI and Coordinator Party Mode**: In `crates/syncplay-core/src/main.rs` (lines 56–63, 250–286) and `crates/syncplay-core/src/coordinator.rs` (lines 418–435), the `party host` and `party connect` subcommands only execute a single initial probe or socket poll before falling back to a hardcoded demo simulation loop and terminating. They do not parse `--port <PORT>` and `--file <MEDIA>`, nor do they maintain a persistent, interactive presentation loop.
2. **In-Band Smart Chat Messaging**: While `syncplay_chat::ChatRecord` and `syncplay_chat::ChatNetworkPacket` exist with `0xAA55` framing and IEEE 802.3 CRC32 checksums, they are not yet multiplexed in real-time over the active watch-party UDP transport during interactive playback. When a user enters a typed message or slash command, it must be framed, sent over UDP to host/peers, persisted to `syncplay_chat.bin`, indexed in `InvertedIndexFts`, and rendered on `BoundedChatOverlay` in real time.
3. **Packet Type Collision Defense**: In `crates/syncplay-sync/tests/watch_party_transport.rs` (line 483), an explicit negative test asserts that `0x07` is an `UnknownPacketType`. Any new wire packet type for chat must be carefully designated (e.g., `PacketType::ChatMessage = 0x08`) to prevent breaking existing regression test fixtures.

---

## 2. Detailed Technical Survey of R4: Multi-Peer Watch-Party Networking

### 2.1 Wire Protocol & Transport Layer (`syncplay-sync`)

The SyncPlay Network Transport (SPNT) binary wire protocol is defined in `crates/syncplay-sync/src/wire.rs`.
Every wire packet has a fixed 12-byte header:
```
+-----------------------------------+-----------------------------------+
| 0..3: Magic b"SPNT"               | 4: PacketType (u8) | 5: Flags (u8)|
+-----------------------------------+--------------------+--------------+
| 6..7: PayloadLen (u16 LE)         | 8..11: Payload CRC32 (u32 LE)     |
+-----------------------------------+-----------------------------------+
| 12..N: Payload Bytes (Variable)                                       |
+-----------------------------------------------------------------------+
```

Currently defined `PacketType` values (`crates/syncplay-sync/src/wire.rs:25-38`):
| Value | Identifier | Payload Size | Fields |
|---|---|---|---|
| `0x01` | `NtpProbeRequest` | 12 bytes | `probe_id: u32`, `t1_client_send_ns: u64` |
| `0x02` | `NtpProbeResponse` | 28 bytes | `probe_id: u32`, `t1_ns: u64`, `t2_ns: u64`, `t3_ns: u64` |
| `0x03` | `RoomStateBroadcast` | 29 bytes | `sequence: u64`, `ref_media_pts_ms: u64`, `target_speed_q16: u32`, `ref_server_wall_ms: u64`, `paused: u8` |
| `0x04` | `BarrierPrepare` | 16 bytes | `session_id: u32`, `target_unpause_server_ms: u64`, `preroll_budget_ms: u32` |
| `0x05` | `BarrierReady` | 12 bytes | `session_id: u32`, `client_id: u32`, `preroll_completed_ms: u32` |
| `0x06` | `SeekCommand` | 12 bytes | `event_id: u32`, `target_pts_ms: u64` |

#### Low-Level Transport Wrapper:
`crates/syncplay-sync/src/transport.rs:18-68`:
`UdpSyncTransport` wraps a `std::net::UdpSocket` set in non-blocking mode (`set_nonblocking(true)`).
- `send_packet(&WirePacket, SocketAddr) -> Result<usize, SyncError>`
- `poll_packet() -> Result<Option<(WirePacket, SocketAddr)>, SyncError>`

### 2.2 Host Server Orchestration (`HostServer`)

Located in `crates/syncplay-sync/src/transport.rs:86-334`:
- **State Tracked**:
  * `transport: UdpSyncTransport`
  * `clients: HashMap<SocketAddr, ClientSession>` tracking `client_id`, `rtt_ms`, `last_seen: Instant`, `is_ready: bool`.
  * `room_sequence: u64`, `ref_media_pts_ms: u64`, `target_speed: f64`, `is_paused: bool`.
  * `barrier: BarrierStateMachine`, `barrier_session_id: u32`, `scheduled_unpause_server_ms: Option<u64>`.
- **Key Operations**:
  * `poll_and_process()`: Non-blocking poll. Handles `NtpProbeRequest` (replies with timestamps $T_2, T_3$), `BarrierReady` (marks client ready), and `SeekCommand` (updates reference PTS and broadcasts room state).
  * `broadcast_room_state()`: Sends `0x03` packet to all registered clients.
  * `prepare_barrier_unpause(preroll_budget_ms)`: Calculates $T_{\text{target\_server}}$, resets client readiness, broadcasts `BarrierPrepare(flags = 0)`.
  * `commit_barrier_unpause(target_ms)`: Broadcasts `BarrierPrepare(flags = FLAG_BARRIER_COMMIT)`.
  * `check_scheduled_unpause()`: Evaluates server monotonic clock against scheduled target.

### 2.3 Client Peer Orchestration (`ClientNode`)

Located in `crates/syncplay-sync/src/transport.rs:337-587`:
- **State Tracked**:
  * `transport: UdpSyncTransport`, `host_addr: SocketAddr`, `client_id: u32`.
  * `filter: Rfc5905Filter`, `pll: PllSpeedController`, `barrier: BarrierStateMachine`.
  * `pending_probes: HashMap<u32, u64>`, `latest_room_state: Option<RoomState>`.
  * `local_pts_ms: u64`, `scheduled_unpause_local_ms: Option<u64>`, `is_playing: bool`.
- **Key Operations**:
  * `send_ntp_probe()`: Sends `NtpProbeRequest` with monotonic $T_1$.
  * `poll_and_process()`:
    - `NtpProbeResponse`: calculates sample $(T_1, T_2, T_3, T_4)$ and calls `filter.process_sample()`.
    - `RoomStateBroadcast`: updates target speed and reference media timestamp.
    - `BarrierPrepare`:
      * If `FLAG_BARRIER_COMMIT`: commits barrier.
      * If Phase 1: computes $T_{\text{target\_local}} = T_{\text{target\_server}} - \hat{\theta}_{\text{ms}}$, arms local unpause timer, sends `BarrierReady` back to host.
    - `SeekCommand`: jumps local PTS and resets PLL.
  * `update_pll(dt_secs)`: Evaluates target presentation timestamp against local PTS and steers speed ratio $r \in [0.96, 1.04]$.
  * `check_scheduled_unpause()`: Automatically flips `is_playing = true` when local clock reaches target unpause instant.

### 2.4 Mathematical Invariants of Network Sync

1. **RFC 5905 Minimum RTT Filter (`crates/syncplay-sync/src/ntp.rs`)**:
   - Window size $W = 8$.
   - Outlier rejection: rejects candidate sample if $\text{RTT} > 2.0 \times \text{median}(\text{RTT})$ or $|\theta - \hat{\theta}| > 3 D_{\text{effective}}$.
   - Effective dispersion floor: $D_{\text{effective}} = \max(D, 5.0\text{ms})$ prevents filter freeze under stable connections.
   - Watchdog timer: flushes buffer after 8 consecutive rejections to recover from network route changes.
2. **Discrete PI PLL Speed Controller (`crates/syncplay-sync/src/pll.rs`)**:
   - Interval $\Delta t = 0.2\text{s}$, gains $K_p = 0.17\text{s}^{-1}$, $K_i = 0.0072\text{s}^{-2}$, anti-windup $I_{\text{max}} = 32.0\text{s}$.
   - Tri-Zone drift policy:
     * **Zone 1 (Deadband)**: $|e| < 15\text{ms} \implies r = 1.0\times$.
     * **Zone 2 (Micro-Speed)**: $15\text{ms} \le |e| \le 1500\text{ms} \implies r = \text{clamp}(1.0 + \Delta r, 0.96, 1.04)$.
     * **Zone 3 (Hard Seek)**: $|e| > 1500\text{ms} \implies$ instantaneous seek, resets PLL.
   - Critically damped dynamics: initial error $+450\text{ms}$ converges to $< 15\text{ms}$ at $t = 12.0\text{s}$ ($\le 20\text{s}$) with zero overshoot past $-15\text{ms}$.
3. **Two-Phase Barrier Unpause (`crates/syncplay-sync/src/barrier.rs`)**:
   - Schedule equation: $T_{\text{target\_server}} = t_{\text{server\_now}} + 1.5 \times \max_i(\text{RTT}_i) + t_{\text{preroll\_budget}} + 100\text{ms}$.
   - Guarantees $\ge 100\text{ms}$ commit arrival safety margin across all pre-roll intervals up to 200ms.
   - Cross-peer start alignment discrepancy is strictly $< 10\text{ms}$.

### 2.5 Smart Chat Subsystem & Local Persistence (`syncplay-chat`)

1. **Binary Log Wire Framing (`crates/syncplay-chat/src/record.rs`)**:
   - Fixed 16-byte header: `magic: b"SPCH"`, `version: 1`, `flags: 0`, `reserved: 0`, `crc32: u32 LE`.
   - Record framing:
     * Sync word: `0xAA, 0x55` (2 bytes).
     * Total record length: `u32 LE` ($\ge 48$ bytes, equals body length + 10).
     * Body: `record_type: u8`, `flags: u8`, `wall_timestamp_utc_ms: u64 LE`, `media_timestamp_ms: u64 LE`, `sender_id: [u8; 16]`, `sender_name_len: u8`, `sender_name: UTF-8`, `payload_len: u32 LE`, `payload: bytes`.
     * Body CRC32: `u32 LE` (IEEE 802.3 CRC32).
2. **Network Packet Format**:
   - `ChatNetworkPacket`: `sequence: u32 LE` (4 bytes) + framed `ChatRecord` bytes.
3. **Crash Recovery Scanner (`crates/syncplay-chat/src/storage.rs`)**:
   - Forward scan looking for `0xAA55`.
   - Rejects candidate if length exceeds remaining file bytes or if body CRC fails. Advances byte-by-byte ($+1$), preserving valid trailing records and isolating false-sync markers.
   - Truncates only incomplete trailing writes at EOF.
4. **Inverted Index Full-Text Search (`crates/syncplay-chat/src/index.rs`)**:
   - Unicode lowercase folding (including Turkish dotted I `İ` $\to$ `i\u0307`).
   - Punctuation stripping with `#` and `@` preserved.
   - CJK unigram + overlapping bigram segmentation for `U+4E00`..`U+9FFF`.
   - Single ideograph indexing exemption ($1 \le len \le 48$).
   - $O(\sum |L_i|)$ linear merge for Boolean AND queries.
   - Spatial queries over `BTreeMap` in $O(\log N + K)$ time for floating comments.
5. **Slash Commands (`crates/syncplay-chat/src/commands.rs`)**:
   - `/seek <time>` (`01:23:45`, `14:30`, `+15s`, `-10s`).
   - `/bookmark [label]`, `/clip <start> <end> [label]`, `/poll <"Q"> <"O1"> <"O2">`, `/sync-status`.

### 2.6 OSD Overlay & Presentation (`syncplay-ui`)

- `BoundedChatOverlay` (`crates/syncplay-ui/src/overlay.rs:14-68`):
  * Dwell time: 4.0s visible + 1.0s fade out = 5.0s window.
  * Maximum active visible bubbles: strictly capped at 5.
  * `on_message(message: String, t_secs: f64)`.
- `PlayerUiSnapshot` (`crates/syncplay-ui/src/snapshot.rs:12-45`):
  * Exactly 72 bytes (`repr(C)`). Fits in a single 128-byte cache-line pair.
  * Published exclusively via `TelemetryAggregator` in `syncplay-core`.
  * Read wait-free by UI presentation loop.

---

## 3. Gap Analysis for Requirement 4

| Feature | Current State | Required State (Phase 3) |
|---|---|---|
| `party host` CLI args | Accepts optional single argument `extra_param` as port number (`main.rs:257`). Falls back to demo. | MUST support `syncplay-alt party host --port <PORT> --file <MEDIA>` (with defaults `port=18242`, `file=synthetic-mkv`). |
| `party connect` CLI args | Accepts optional single argument `extra_param` as host address (`main.rs:270`). Falls back to demo. | MUST support `syncplay-alt party connect <HOST:PORT> --file <MEDIA>` (with default `file=synthetic-mkv`). |
| Host Server Runtime | Single non-blocking poll call, then prints fallback message and exits (`coordinator.rs:418-424`). | Persistent interactive host server broadcasting room state, answering NTP probes, handling barrier unpauses, and serving media. |
| Client Peer Runtime | Single probe sent, single non-blocking poll call, then exits (`coordinator.rs:425-434`). | Persistent interactive client loop executing continuous NTP exchange, PLL speed steering, barrier participation, and presentation. |
| In-Band Chat Over UDP | `ChatNetworkPacket` exists in `syncplay-chat`, tested in unit tests and loopback demo, but NOT integrated into live party runtime. | Typed chat messages framed and sent over UDP, persisted to `syncplay_chat.bin`, indexed in `InvertedIndexFts`, and rendered on `BoundedChatOverlay`. |
| Chat Relay by Host | Host does not relay chat packets between peers. | Host server receives client chat messages and relays them to all other connected party peers. |
| Slash Command Dispatch | Dispatcher parses commands in test fixtures, but `/seek` does not trigger barrier unpause or network seek packet in party mode. | Slash commands executed in chat bar dispatch to `syncplay-sync` and synchronize across all room participants. |

---

## 4. Detailed Technical Survey of R5: Intent Harness & Zero-Drift Defense

### 4.1 Dependency Blacklist Audit (`scripts/audit_dependencies.sh`)

- Script audits both `Cargo.toml` (and all crate manifests `crates/*/Cargo.toml`) and `Cargo.lock`.
- Banned libraries checked (42 patterns):
  * Codecs/Media: `ffmpeg`, `libav*`, `gstreamer`, `mpv`, `vlc`, `symphonia`, `dav1d`, `rav1e`, `openh264`, `minimp3`, `nom-ebml`, `hound`, `image`, `imageproc`.
  * Databases/Search: `sqlite`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `heed`, `tantivy`, `meilisearch`, `diesel`.
- **Current Status**: PASSED. Zero banned dependencies detected.

### 4.2 Compiler & Clippy Zero-Warning Enforcement

- Commands executed:
  * `cargo check --workspace --all-targets`
  * `cargo clippy --workspace --all-targets -- -D warnings`
- Crate lint attributes enforced across all crates:
  * `#![deny(warnings)]`
  * `#![deny(missing_docs)]`
  * `#![deny(unsafe_code)]` (except justified SIMD in codecs and lock-free slot indexing in clock/ui).
- **Current Status**: PASSED. Zero warnings across entire workspace.

### 4.3 Pi Framework Intent Verification Harness (`scripts/intent_harness.py`)

- Matrix file: `.pi/harness/intent_matrix.json` (version 1.0.0, authority `AGENTS.md`).
- Report file: `.pi/harness/intent_report.json`.
- Evaluates 8 core architectural intents:
  1. `INTENT-01`: Zero-External-Codec Invariant (`./scripts/audit_dependencies.sh`)
  2. `INTENT-02`: Deterministic High-RTT Network Clock Synchronization (`python3 tests/empirical_challenge/test_sync_engine.py`)
  3. `INTENT-03`: Barrier-Synchronized Two-Phase Commit Unpause (`python3 tests/empirical_challenge/test_sync_engine.py`)
  4. `INTENT-04`: Crash-Resilient Dual-Temporal Local Smart Chat (`python3 tests/empirical_challenge/test_smart_chat.py`)
  5. `INTENT-05`: Decoupled Lock-Free Threading & VLC-Grade Responsiveness (`python3 tests/empirical_challenge/test_ui_renderer.py`)
  6. `INTENT-06`: Zero-Allocation Fixed-Point Color Transformation (`python3 tests/empirical_challenge/test_demux_codecs_clock.py`)
  7. `INTENT-07`: Robust Container Demuxing & Timescale Integer Arithmetic (`python3 tests/empirical_challenge/test_demux_codecs_clock.py`)
  8. `INTENT-08`: OpenSpec Traceability & Anti-Drift Guarantee (`cargo test --workspace -- --nocapture`)
- **Current Status**: 8/8 PASSED. Compliance score: 100.0%.

### 4.4 Empirical Challenge Test Suites (`tests/empirical_challenge/*.py`)

All 11 test suites were executed directly and verified:
1. `test_demux_codecs_clock.py`: 48B EBML vector, 24h timescale arithmetic without overflow, 64-bit largesize checks, 16B SIMD alignment, SeqLock snapshot reanchoring. (PASSED)
2. `test_ebml_adversarial.py`: Frame lacing unpacking (Xiph, Fixed, EBML signed deltas), Cues seeking edge cases. (PASSED)
3. `test_isobmff_adversarial.py`: 64-bit largesize bounds, stss keyframe parsing. (PASSED)
4. `test_sync_engine.py`: NTP RFC 5905 filter, $D_{\text{effective}} \ge 5.0\text{ms}$ floor, watchdog flush, critically damped PI PLL convergence in 12.0s, barrier unpause commit margin $\ge 100\text{ms}$. (PASSED)
5. `test_sync_adversarial_p2.py`: WAN link simulation (250–400ms RTT, $\pm 30\text{ms}$ jitter), boundary drift 1500ms, hard seek $> 1500\text{ms}$, anti-windup clamping at $I_{\text{max}} = 32.0\text{s}$. (PASSED)
6. `test_smart_chat.py`: Binary log framing, false-sync candidate recovery, CJK unigram/bigram tokenization, single ideograph retention. (PASSED)
7. `test_smart_chat_adversarial_p2.py`: Extreme corruption, torn writes, multilingual FTS. (PASSED)
8. `test_ui_renderer.py`: SPSC triple buffer concurrency, `PlayerUiSnapshot` size = 72 bytes, 10Hz scrubber debouncer (6 commands for 500ms drag), Alpha8 font atlas 4MB footprint. (PASSED)
9. `test_crates_5_to_8_adversarial.py`: Full cross-crate stress test. (PASSED)
10. `test_pi_adversarial_stress.py`: Manifest injection audit, prompt wire layout, DAG validation. (PASSED)
11. `test_core_adversarial_p2.py`: CLI banner, `inspect`, `play`, `party`, `chat`, `verify` subcommands. (PASSED)

### 4.5 Critical Architectural Invariants & Regression Pitfalls

1. **`PacketType` Enum Safety**:
   - In `crates/syncplay-sync/tests/watch_party_transport.rs` line 483:
     ```rust
     for &bad_type in &[0x00u8, 0x07, 0x42, 0xAA, 0xFF] {
     ```
     This test explicitly asserts that `0x07` produces `SyncError::UnknownPacketType(0x07)`.
   - **Rule**: If adding a new packet type to `PacketType` for chat, use `PacketType::ChatMessage = 0x08` (or assign `0x08`, leaving `0x07` unknown). Do NOT use `0x07` without updating this test, or else `watch_party_transport.rs` will fail!
2. **`PlayerUiSnapshot` Size Invariant**:
   - `test_ui_renderer.py` and `intent_matrix.json` strictly require that `PlayerUiSnapshot` size is exactly 72 bytes (fitting in a 128-byte cache line pair).
   - **Rule**: Do NOT add fields to `PlayerUiSnapshot` or alter its `#[repr(C)]` layout without verifying struct memory alignment.
3. **Clippy & Documentation Standards**:
   - All public methods must include thorough docstrings.
   - Code must compile with `#![deny(warnings)]` and `#![deny(missing_docs)]`.
4. **Zero Banned Dependencies**:
   - Do NOT add any external media, codec, or database crates (`sqlite`, `image`, `hound`, `vlc`, `ffmpeg`).

---

## 5. Architectural Blueprint for Phase 3 Implementation

### 5.1 CLI Argument Parser for `party` (`syncplay-core/src/main.rs`)

Implement flexible argument parsing in `run_party_mode`:
- Syntax supported:
  * `syncplay-alt party host [--port <PORT>] [--file <MEDIA>]` (default port: 18242, default file: "synthetic-mkv")
  * `syncplay-alt party connect <HOST:PORT> [--file <MEDIA>]` (default file: "synthetic-mkv")
  * Legacy/test syntax: `syncplay-alt party host [PORT]` and `syncplay-alt party connect [HOST:PORT]`
  * `syncplay-alt party demo` (runs in-process loopback simulation)
- For non-interactive or pipe environments (or when `--demo` is active): execute connection handshake, verify loopback, print status, and exit 0 (maintaining 100% compatibility with `test_core_adversarial_p2.py`).
- For interactive terminal mode: launch the live watch-party presentation loop!

### 5.2 In-Band Chat Wire Integration (`syncplay-sync` and `syncplay-chat`)

To support live in-band smart chat messaging:
1. In `crates/syncplay-sync/src/wire.rs`:
   Add `PacketType::ChatMessage = 0x08` (avoiding `0x07`).
   Add `PacketPayload::ChatMessage(Vec<u8>)` holding the framed `ChatRecord` bytes.
   Add `WirePacket::chat_message(bytes: Vec<u8>) -> Self`.
2. In `HostServer`:
   - When a `PacketPayload::ChatMessage(bytes)` arrives from client `addr`:
     * Validate framing using `ChatRecord::from_network_bytes(&bytes)`.
     * Append to host's local `ChatStore` (`syncplay_chat.bin` + inverted index).
     * Add to host's `BoundedChatOverlay` for on-screen display.
     * Relay to all other connected clients (`broadcast_chat_message(&bytes, exclude_addr)`).
3. In `ClientNode`:
   - When `PacketPayload::ChatMessage(bytes)` arrives from host:
     * Validate framing using `ChatRecord::from_network_bytes(&bytes)`.
     * Append to client's local `ChatStore`.
     * Add to client's `BoundedChatOverlay` for on-screen display.
4. Sending Chat Messages:
   - When the user presses `/` or types in the chat input bar:
     * If message starts with `/`: dispatch through `SlashCommandParser`.
       - If `/seek <time>`: dispatch `WirePacket::seek_command` or trigger barrier unpause.
       - If `/sync-status`: render diagnostic report in OSD.
     * If normal text: construct `ChatRecord::new_text(now_utc_ms, current_pts_ms, uuid, user_name, &text)`.
       - Serialize to framed bytes.
       - Append locally to `ChatStore` and `BoundedChatOverlay`.
       - Send `WirePacket::chat_message(bytes)` over UDP!

### 5.3 Interactive Watch-Party Presentation Loop

In `syncplay-core/src/coordinator.rs`:
Create an interactive party runner `run_interactive_party`:
- Runs at 60fps (16.6ms loop tick).
- Handles non-blocking UDP I/O:
  * Host: polls client datagrams, answers NTP, monitors barrier unpause, relays chat, broadcasts room state at 5Hz.
  * Client: polls host datagrams, sends NTP probes at 5Hz until synchronized (then 1Hz), evaluates discrete PI PLL speed steering, participates in barrier unpause.
- Manages `MasterClock` and presentation timeline:
  * If unpause scheduled, monitors wall clock and unpauses presentation precisely on time.
  * If playing, advances media PTS and renders terminal TrueColor half-block frames.
- Renders Sleek OSD HUD:
  * Timecode HUD (`format_timecode_hud`)
  * Tri-color sync health badge (`evaluate_sync_health`: Green $< 15\text{ms}$, Amber $\le 1500\text{ms}$, Red $> 1500\text{ms}$)
  * Floating chat bubbles from `BoundedChatOverlay` (up to 5 bubbles, 4s dwell, 1s fade)
- Handles Keyboard Inputs:
  * `Space`: initiate barrier-synchronized unpause or pause.
  * `Left`/`Right`: seek $\pm 5\text{s}$.
  * `[` / `]`: speed change $\pm 0.1\times$.
  * `T`/`C`: toggle chat overlay.
  * `S`: toggle sync diagnostics HUD.
  * `/`: interactive slash-command / chat prompt.
  * `Q` / `Esc`: clean exit.

---

## 6. Verification Matrix for Phase 3 Deliverables

| Target | Command | Expected Result |
|---|---|---|
| Zero Banned Dependencies | `./scripts/audit_dependencies.sh` | Exit 0, 0 violations |
| Compiler Lints & Warnings | `cargo check --workspace --all-targets` | Exit 0, 0 warnings |
| Clippy Strict Linter | `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0, 0 warnings |
| Unit & Integration Tests | `cargo test --workspace -- --nocapture` | 100% unit tests pass |
| Intent Compliance Harness | `python3 scripts/intent_harness.py` | 100.0% Intent Compliance Score |
| All Empirical Test Suites | `for t in tests/empirical_challenge/*.py; do python3 "$t"; done` | All 11 suites pass cleanly |
| CLI Party Host Verification | `cargo run -p syncplay-core -- party host --port 18242 --file synthetic-mkv` | Starts host, binds port, prepares room |
| CLI Party Connect Verification | `cargo run -p syncplay-core -- party connect 127.0.0.1:18242 --file synthetic-mkv` | Connects, exchanges NTP probes, syncs |
| In-Band Chat Verification | Unit and integration tests verify UDP chat roundtrip, `syncplay_chat.bin` write, FTS indexing, and overlay bubble addition | Verified in tests and interactive mode |
