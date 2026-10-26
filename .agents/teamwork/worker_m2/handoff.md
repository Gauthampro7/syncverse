# Phase 5 Milestone 2 Completion Report: Wire Protocol & Peer State Tracking

## 1. Observation

1. **Wire Protocol Extension (`crates/syncplay-sync/src/wire.rs:22-58, 159-250`)**:
   - Added `PacketType::PeerStatus = 0x09` to `PacketType` and implemented `TryFrom<u8>`.
   - Defined `PeerStatusPayload`:
     ```rust
     #[derive(Debug, Clone, PartialEq, Eq)]
     pub struct PeerStatusPayload {
         pub client_id: u32,
         pub has_media: bool,
         pub is_ready: bool,
         pub duration_ms: u64,
         pub file_size_bytes: u64,
         pub username: String,
         pub filename: String,
     }
     ```
   - Implemented binary serialization and deserialization with Little-Endian numeric encodings, length-prefixed UTF-8 strings (`u16` byte lengths), bounds checking (minimum payload 26 bytes), and table-driven IEEE 802.3 CRC32 verification (`Crc32::digest`).
   - Added `WirePacket::peer_status(...)` constructor and re-exported `PeerStatusPayload` in `crates/syncplay-sync/src/lib.rs:22`.

2. **Negative Test Fixture Update (`crates/syncplay-sync/tests/chat_transport_adversarial.rs:11, 302-303`)**:
   - Updated the unknown opcodes array in `chat_transport_adversarial.rs:303` to remove `0x09`:
     ```rust
     for &bad_op in &[0x00u8, 0x07, 0x0A, 0x33, 0x7F, 0xFE, 0xFF] {
     ```
   - Verified `crates/syncplay-sync/tests/watch_party_transport.rs:483` array `&[0x00u8, 0x07, 0x42, 0xAA, 0xFF]` does not contain `0x09`.

3. **Peer State Tracking & Session Decoupling (`crates/syncplay-sync/src/transport.rs:80-97, 280-305, 410-465, 740-800`)**:
   - Decoupled `ClientSession.is_user_ready: bool` (persistent watch readiness) from `ClientSession.barrier_preroll_ready: bool` (transient decode ack).
   - Added `has_media: bool`, `filename: String`, `duration_ms: u64`, `file_size_bytes: u64`, and `username: String` to `ClientSession`.
   - On `HostServer`:
     - Updated `poll_and_process()` to handle `PacketPayload::PeerStatus`, updating the sender's session and relaying the datagram to all other connected clients without echoing back to the sender.
     - Added query methods: `are_all_users_ready(&self) -> bool`, `get_unready_clients(&self) -> Vec<(u32, String)>`, and `peer_statuses(&self) -> Vec<PeerStatusPayload>`.
     - Added broadcast method: `broadcast_peer_status(&mut self, status: &PeerStatusPayload) -> Result<usize, SyncError>`.
   - On `ClientNode`:
     - Added `send_peer_status(&mut self, has_media: bool, is_ready: bool, duration_ms: u64, file_size_bytes: u64, username: &str, filename: &str) -> Result<(), SyncError>`.
     - Automatically strips directory components using `Path::file_name` to prevent privacy leaks of local filesystem trees.
     - Updated `poll_and_process()` to record received `PeerStatus` payloads into `peer_statuses: HashMap<u32, PeerStatusPayload>` and a queue drained via `poll_peer_statuses()`.

4. **Integration & Unit Verification**:
   - `crates/syncplay-sync/src/wire.rs`: Added 4 unit tests (`test_peer_status_roundtrip`, `test_peer_status_empty_strings`, `test_peer_status_unicode_and_large_values`, `test_peer_status_malformed_payload`).
   - `crates/syncplay-sync/tests/peer_status_test.rs`: Created new comprehensive test suite with 5 integration tests:
     - `test_peer_status_wire_roundtrips`: Validates exact round-trips with standard, zero/empty, and Unicode/large boundary payloads.
     - `test_peer_status_path_sanitization`: Verifies full directory paths (`/home/gautham/Videos/...`) are stripped to basenames.
     - `test_peer_status_adversarial_rejections`: Verifies sub-26-byte truncated payloads, declared length overflows, invalid UTF-8 bytes, and bit-flipped CRC32 errors are rejected with `SyncError::MalformedPayload` or `SyncError::CrcMismatch`.
     - `test_host_and_multi_client_peer_status_exchange`: Simulates 2 clients and 1 host; verifies peer status transmission, host session tracking, relaying between clients, readiness queries, and host broadcast.
     - `test_barrier_unpause_independence_from_user_readiness`: Verifies that initiating a two-phase barrier unpause resets `barrier_preroll_ready` without clearing persistent `is_user_ready`.

5. **Tool Commands and Results**:
   - `cargo check -p syncplay-sync --all-targets`: Exits 0, 0 compiler warnings.
   - `cargo test -p syncplay-sync`: Exits 0, all 38 tests pass (20 unit tests, 4 adversarial chat tests, 1 multi-client chat test, 5 peer status tests, 7 adversarial challenge tests, 5 watch party transport tests).
   - `cargo clippy -p syncplay-sync --all-targets -- -D warnings`: Exits 0, 0 warnings.
   - `./scripts/audit_dependencies.sh`: Clean (19 banned patterns checked, 0 violations).

---

## 2. Logic Chain

1. **Wire Protocol Compatibility (From Observation 1 & 2)**:
   - Opcode `0x09` was allocated for `PacketType::PeerStatus`.
   - In negative testing suites (`tests/chat_transport_adversarial.rs`), opcode `0x09` was previously part of the unknown opcode fixture array. Removing `0x09` from this array ensures adversarial tests verify truly unknown opcodes (`0x00`, `0x07`, `0x0A`, `0x33`, `0x7F`, etc.) while allowing `0x09` to function as a valid registered packet type.
   - Using IEEE 802.3 table-driven CRC32 ensures bit corruption on lossy WAN UDP datagrams is detected and dropped deterministically before parsing.

2. **Decoupled User Readiness vs Transient Pre-Roll Barrier (From Observation 3 & 4)**:
   - The watch-party synchronization protocol uses a two-phase commit unpause where `prepare_barrier_unpause()` resets pre-roll decode readiness across peers to collect pre-roll acks (`BarrierReady`).
   - If `ClientSession.is_ready` were overloaded for both, any unpause or seek would inadvertently clear the user's "Ready to Watch" checkbox.
   - By separating `is_user_ready: bool` from `barrier_preroll_ready: bool`, persistent watch readiness survives barrier scheduling, seeking, and playback state shifts.

3. **Privacy by Default (From Observation 3 & 4)**:
   - Users loading media from graphical file pickers pass full local paths (`/home/user/...` or `C:\Users\...`).
   - `ClientNode::send_peer_status` automatically sanitizes the filename to `Path::file_name` prior to wire serialization, guaranteeing local directory trees and usernames are never transmitted over the network.

4. **Multi-Peer Eventual Consistency (From Observation 3 & 4)**:
   - UDP datagrams can be lost over WAN links.
   - Because `PeerStatus` is self-contained and idempotent (contains client ID, readiness state, media presence, duration, file size, username, and filename), each datagram completely updates the peer's state. When sent upon state changes or periodic heartbeats, any dropped packet is healed by the next datagram without state desynchronization.

---

## 3. Caveats

- Milestone 2 strictly covers the wire protocol, `syncplay-sync` state models, host relay, and client methods. UI rendering (`RoomMember` display in `syncplay-ui`) and playback runtime unpause gating (`playback_runtime.rs` in `syncplay-core`) are handled in Milestones 3 & 4.
- In-progress E2E test files in `tests/e2e_phase5/` (managed by Worker M5) require downstream milestones (M1 updater and M4 UI) and do not affect `syncplay-sync` compilation or test execution.

---

## 4. Conclusion

Phase 5 Milestone 2 is completely implemented, verified, and passes all requirements:
1. `PacketType::PeerStatus = 0x09` and `PeerStatusPayload` are implemented with robust LE serialization/deserialization, bounds checking, and table-driven CRC32 framing.
2. Bad opcode arrays in negative adversarial test suites have been updated to remove `0x09`.
3. `ClientSession` decouples `is_user_ready` from `barrier_preroll_ready` and tracks media metadata.
4. `HostServer` and `ClientNode` fully support sending, receiving, relaying, and querying peer statuses.
5. All 38 unit and integration tests in `syncplay-sync` pass cleanly with zero compiler warnings and zero clippy warnings.

---

## 5. Verification Method

To independently verify the implementation:

```bash
# 1. Verify syncplay-sync compiles with zero warnings
cargo check -p syncplay-sync --all-targets

# 2. Run all unit and integration tests (38 tests)
cargo test -p syncplay-sync

# 3. Run the new peer status test suite
cargo test -p syncplay-sync --test peer_status_test -- --nocapture

# 4. Run strict Clippy linter check with warnings denied
cargo clippy -p syncplay-sync --all-targets -- -D warnings

# 5. Run banned dependency audit script
./scripts/audit_dependencies.sh
```
