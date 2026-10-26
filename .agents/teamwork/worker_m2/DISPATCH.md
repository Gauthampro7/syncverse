# Dispatch: Worker M2 (Wire Protocol & Peer State Tracking)

## Objective
Implement Milestone 2 of Syncplay-Alt Phase 5:
1. In `crates/syncplay-sync/src/wire.rs`:
   - Add `PacketType::PeerStatus = 0x09`.
   - Implement `PeerStatusPayload`:
     ```rust
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
   - Implement binary serialization and deserialization with Little-Endian numeric types, length-prefixed strings, bounds checks, and CRC32 table digest verification.
2. In `crates/syncplay-sync/tests/chat_transport_adversarial.rs:303` and `crates/syncplay-sync/tests/watch_party_transport.rs:483`:
   - Update bad opcode arrays (remove `0x09` so it is recognized as a valid opcode).
3. In `crates/syncplay-sync/src/transport.rs`:
   - Extend `ClientSession`:
     - Separate persistent `is_user_ready: bool` from transient `barrier_preroll_ready: bool`.
     - Add `has_media: bool`, `filename: String`, `duration_ms: u64`, `file_size_bytes: u64`.
   - On `HostServer`:
     - Handle `PacketPayload::PeerStatus`: update the sender's `ClientSession` and broadcast the `PeerStatus` packet to all other connected clients.
     - Add method to query all clients' readiness and media status.
   - On `ClientNode`:
     - Add method `send_peer_status(&mut self, has_media: bool, is_ready: bool, duration_ms: u64, file_size_bytes: u64, username: &str, filename: &str) -> Result<()>` to broadcast local status to the host.
4. Unit & Integration tests in `crates/syncplay-sync`:
   - Serialization / deserialization round-trip for `PeerStatusPayload` (empty filename, unicode filename, large duration).
   - Multi-peer status relay through `HostServer`.
   - Regression tests verifying barrier unpause and existing NTP probes still function cleanly.

## Files Owned Exclusively
- `crates/syncplay-sync/src/wire.rs`
- `crates/syncplay-sync/src/transport.rs`
- `crates/syncplay-sync/tests/chat_transport_adversarial.rs`
- `crates/syncplay-sync/tests/watch_party_transport.rs`
- `crates/syncplay-sync/tests/peer_status_test.rs` (new test file)

## References
- User request: `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (read section `## 2026-10-26T11:12:06Z`)
- Project spec: `/home/gautham/syncplay-alt/PROJECT.md`
- Survey report: `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_1/handoff.md`

## Invariants & Warnings
- Zero compiler warnings (`#![deny(warnings)]`).
- Clippy clean: `cargo clippy -p syncplay-sync --all-targets -- -D warnings`.
- Zero banned dependencies (no `mpv`, `vlc`, `sqlite`, `rocksdb`).
- All existing 33+ unit and adversarial tests in `syncplay-sync` must continue to pass.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## Completion Criteria
Run `cargo check -p syncplay-sync --all-targets`, `cargo test -p syncplay-sync`, and `cargo clippy -p syncplay-sync --all-targets -- -D warnings`. All must pass with 0 warnings.
Write your completion report in `/home/gautham/syncplay-alt/.agents/teamwork/worker_m2/handoff.md` and message back.
