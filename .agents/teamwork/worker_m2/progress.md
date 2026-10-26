# Progress: Phase 5 Milestone 2 (Wire Protocol & Peer State Tracking)

Last visited: 2026-10-26T11:45:00Z
Status: Completed

## Tasks
- [x] Baseline verification: cargo check, cargo test, cargo clippy pass
- [x] Initialized DISPATCH.md and updated BRIEFING.md
- [x] Implement `PacketType::PeerStatus = 0x09` and `PeerStatusPayload` in `crates/syncplay-sync/src/wire.rs`
- [x] Add serialization and deserialization with bounds checking and CRC32 table digest
- [x] Update negative test opcode arrays in `crates/syncplay-sync/tests/chat_transport_adversarial.rs`
- [x] Extend `ClientSession` in `crates/syncplay-sync/src/transport.rs` (decouple `is_user_ready` from `barrier_preroll_ready`, add media metadata fields)
- [x] Update `HostServer` to handle `PeerStatus`, update `ClientSession`, relay to peers, add status query methods
- [x] Update `ClientNode` with `send_peer_status` and handle incoming `PeerStatus`
- [x] Write unit tests for `PeerStatusPayload` (empty filename, unicode, large values, malformed payload)
- [x] Write integration test `crates/syncplay-sync/tests/peer_status_test.rs` for multi-peer status relay, sanitization, adversarial cases, and barrier independence
- [x] Final verification: cargo check, cargo test, cargo clippy (38/38 tests passing, 0 warnings)
- [ ] Handoff report and parent notification
