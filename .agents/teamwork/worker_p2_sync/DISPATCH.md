## 2026-10-25T10:57:59Z
You are worker_p2_sync, an expert systems and network engineer in pure Rust.
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_sync/`.

MANDATORY FIRST STEP:
Read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` completely before starting any work.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Scope & Task for Milestone 3 (Real-Time Watch-Party Transport in `crates/syncplay-sync`):
1. **Zero External Dependency Invariant**:
   - Strictly NO `tokio`, NO `async-std`, NO `serde` (unless already in workspace whitelist; use pure-Rust byte-level serialization or standard library).
   - Use standard library `std::net::UdpSocket` with `set_nonblocking(true)`.
   - Use `syncplay_common::Crc32` for packet integrity verification.
2. **Implement Binary Wire Protocol (`crates/syncplay-sync/src/wire.rs`)**:
   - Magic: `b"SPNT"` (SyncPlay Network Transport, 4 bytes).
   - Packet Header: `magic: [u8; 4]`, `packet_type: u8`, `flags: u8`, `payload_len: u16`, `crc32: u32` (IEEE 802.3 CRC32 of payload).
   - Packet Types:
     - `0x01` NTP Probe Request: `probe_id: u32`, `t1_client_send_ns: u64`
     - `0x02` NTP Probe Response: `probe_id: u32`, `t1_client_send_ns: u64`, `t2_server_recv_ns: u64`, `t3_server_send_ns: u64`
     - `0x03` Room State Broadcast: `sequence: u64`, `ref_media_pts_ms: u64`, `target_speed_q16: u32`, `ref_server_wall_ms: u64`, `paused: u8`
     - `0x04` Barrier Prepare / Schedule: `session_id: u32`, `target_unpause_server_ms: u64`, `preroll_budget_ms: u32`
     - `0x05` Barrier Ready / Commit Ack: `session_id: u32`, `client_id: u32`, `preroll_completed_ms: u32`
     - `0x06` Seek / Command Event: `event_id: u32`, `target_pts_ms: u64`
   - Provide safe, bounds-checked binary serialization (`to_bytes(&self) -> Vec<u8>`) and deserialization (`from_bytes(buf: &[u8]) -> Result<WirePacket, SyncError>`).
3. **Implement Non-Blocking UDP Transport (`crates/syncplay-sync/src/transport.rs`)**:
   - `UdpSyncTransport`: wraps `std::net::UdpSocket` in non-blocking mode.
   - Methods to send and poll datagrams (`poll_packet(&self) -> Result<Option<(WirePacket, SocketAddr)>, SyncError>`).
   - `HostServer`:
     - Binds listening UDP socket (e.g. `127.0.0.1:0` or specific port).
     - Responds to NTP probes immediately by stamping $T_2$ (arrival) and $T_3$ (send).
     - Tracks connected clients and calculates maximum RTT: $\max_i(\text{RTT}_i)$.
     - Broadcasts `RoomState` updates.
     - Initiates barrier unpause: schedules $T_{\text{target\_server}} = t_{\text{server\_now}} + 1.5 \times \max_i(\text{RTT}_i) + t_{\text{preroll\_budget}} + 100\text{ms}$.
   - `ClientNode`:
     - Connects/sends to Host.
     - Sends periodic NTP probes, calculates $\hat{\theta}$ and RTT, feeds samples into `Rfc5905Filter`.
     - Feeds estimated clock offset $\hat{\theta}$ and server timeline into `PllSpeedController`.
     - Controls local playback rate $r \in [0.96, 1.04]$, maintaining phase error $|e| < 15\text{ms}$ in Zone 1 deadband.
     - Handles barrier unpause: acknowledges readiness, schedules unpause at local wall time $T_{\text{local}} = T_{\text{target\_server}} - \hat{\theta}$.
4. **Re-export in `crates/syncplay-sync/src/lib.rs`**:
   - `pub mod wire; pub mod transport;`
   - Re-export `WirePacket`, `PacketType`, `UdpSyncTransport`, `HostServer`, `ClientNode`.
5. **Write Comprehensive Tests**:
   - Unit tests in `crates/syncplay-sync/src/wire.rs` and `src/transport.rs`.
   - Integration test: Spawn live in-process Host and Client over `127.0.0.1`, perform real NTP exchanges, evaluate PLL convergence, and execute barrier unpause synchronization.
6. **Empirical Verification Standards**:
   - `cargo check --workspace --all-targets` (0 warnings)
   - `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings)
   - `cargo test -p syncplay-sync -- --nocapture` (all tests pass)
   - `python3 tests/empirical_challenge/test_sync_engine.py` (must pass cleanly)
   - `./scripts/audit_dependencies.sh` (zero banned engines)
   - `python3 scripts/intent_harness.py` (100.0% score)
   - `./scripts/verify_phase1.sh` (all 8 stages pass)
7. **Deliver**:
   - Write comprehensive handoff report with exact commands, test output, and file paths to `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_sync/handoff.md`.
   - Notify parent orchestrator via `send_message`.
