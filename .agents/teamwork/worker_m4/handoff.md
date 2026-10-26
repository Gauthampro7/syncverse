# Phase 5 Milestone 4 Handoff Report: Peer Media Status, Diagnostics & Unpause Gate UI

## 1. Observation
1. **Command and Event Expansion**:
   - `crates/syncplay-ui/src/command.rs`: Added variants to `PlayerCommand`:
     - `SetReady(bool)`: Updates the local peer's readiness state.
     - `SetForcePlay(bool)`: Updates the host's Force Play toggle.
     - `ForcePlay`: Immediate host trigger to bypass the unpause gate.
   - `crates/syncplay-ui/src/event.rs`: Added `UiEvent::UnpauseBlocked { unready_peers: Vec<String>, message: String }`.
2. **72-Byte Snapshot Invariant Preserved**:
   - `crates/syncplay-ui/src/snapshot.rs`:
     - Bitflag masks defined in upper bits of `playback_state`: `SNAPSHOT_FLAG_GATE_BLOCKED = 0x10`, `SNAPSHOT_FLAG_FORCE_PLAY = 0x20`, `SNAPSHOT_FLAG_LOCAL_READY = 0x40`, `SNAPSHOT_FLAG_HAS_MISMATCH = 0x80`.
     - Query helpers added: `is_playing()`, `is_gate_blocked()`, `is_force_play()`, `is_local_ready()`, `has_media_mismatch()`.
     - Memory layout verified via unit tests: `size_of::<PlayerUiSnapshot>() == 72` strictly maintained.
3. **Sidebar UI & File Diagnostics**:
   - `crates/syncplay-ui/src/gui/sidebar.rs`:
     - Extended `RoomMember` with `file_name: Option<String>`, `duration_ms: Option<u64>`, `file_size_bytes: Option<u64>`, `is_ready: bool`. Added constructors `new(...)` and `with_media(...)`.
     - Added `FileMatchStatus` enum (`Match`, `Mismatch`, `NoFileLoaded`) with `.label()` helper.
     - Implemented `extract_basename(path: &str)` supporting both Unix `/` and Windows `\` path separators.
     - Implemented `evaluate_file_match(...)` applying ±2000ms duration tolerance and case-insensitive basename matching.
     - Rendered "Ready to Watch" checkbox and Host "⚡ Force Play" toggle.
     - Rendered readiness badges (`[✓ Ready]` in green `#22c55e` vs `[⏳ Not Ready]` in amber `#f59e0b`), loaded media filename + duration (`HH:MM:SS`), and diagnostic match badges (`[Match ✔]`, `[⚠️ Mismatch]`, `[⏳ No file loaded]`).
4. **Viewport Alert Banner**:
   - `crates/syncplay-ui/src/gui/app.rs`:
     - Added `UnpauseBlockedBanner` and handled `UiEvent::UnpauseBlocked` in `SyncplayApp::ui`.
     - Rendered viewport on-screen alert banner with warning symbol, message, dismiss button, and 1-click Host "⚡ Force Play" bypass button.
     - Auto-cleared banner when playback resumes; auto-set local user readiness to `true` when a file is loaded.
5. **Coordinator and Playback Runtime Integration**:
   - `crates/syncplay-core/src/coordinator.rs`: Added handling for `SetReady`, `SetForcePlay`, `ForcePlay`.
   - `crates/syncplay-core/src/playback_runtime.rs`:
     - Tracked `local_ready`, `force_play`, `gate_blocked`, `current_file_name`, `current_file_size`.
     - Intercepted `TogglePause`: if paused, unpause gate checks all active peers (`last_seen < 15s`). If any peer lacks media or `!is_user_ready`, and `!force_play`, playback remains paused, dispatches `UiEvent::UnpauseBlocked`, and broadcasts `RoomAnnouncement::gate_waiting`.
     - If all peers are ready or `force_play == true`, unpause proceeds immediately.
     - Intercepted `OpenFile`: sets `local_ready = true`, broadcasts `PeerStatusPayload`, and emits `RoomAnnouncement::media_loaded`.
     - Intercepted `SetReady`: broadcasts `PeerStatusPayload` and `RoomAnnouncement::readiness`.
     - Intercepted `Seek`, `TogglePause` (pausing/resuming): broadcasts `RoomAnnouncement::seeked_ms`, `paused`, and `resumed`.
     - Telemetry loop populates snapshot flags: `SNAPSHOT_FLAG_GATE_BLOCKED`, `SNAPSHOT_FLAG_FORCE_PLAY`, `SNAPSHOT_FLAG_LOCAL_READY`, `SNAPSHOT_FLAG_HAS_MISMATCH`.
6. **Integration & Unit Testing**:
   - Unit tests in `crates/syncplay-ui/src/gui/sidebar.rs`: tested duration formatting, tolerance matching, Windows/Linux basename matching, mismatch detection, missing media handling.
   - Integration test suite in `crates/syncplay-core/tests/unpause_gate_and_peer_diagnostics_integration.rs`:
     - `test_snapshot_memory_layout_and_flags`
     - `test_file_match_diagnostics_integration`
     - `test_unpause_gate_network_peer_status_and_gate_blocking`
     - `test_unpause_gate_force_play_bypass`
     - `test_room_announcements_for_media_and_readiness`
     - `test_room_member_struct_and_constructors`
   - Verification suite:
     - `cargo check --workspace --all-targets`: Clean exit 0.
     - `cargo clippy --workspace --all-targets -- -D warnings`: Clean exit 0.
     - `cargo test --workspace`: All tests passed cleanly (0 failed).
     - `./scripts/audit_dependencies.sh`: 0 violations of banned frameworks.
     - `python3 scripts/intent_harness.py`: 8/8 intents passed, 100% compliance.
     - `./scripts/verify_phase1.sh`: All 8 stages passed cleanly.

## 2. Logic Chain
1. *Requirement*: Prevent accidental watch-party playback resumption while peers are still downloading, loading, or unready.
   - *Implementation*: `playback_runtime.rs` inspects `server.clients()` (for Host) and `client.peer_statuses()` (for Client). If unready peers exist and `!force_play`, unpausing is halted, `UiEvent::UnpauseBlocked` displays an on-screen warning banner, and `RoomAnnouncement::gate_waiting` notifies peers in the chat log.
2. *Requirement*: Allow Host to override unpause gate if a peer is AFK or uncooperative.
   - *Implementation*: Host has "⚡ Force Play" toggle in the sidebar and a 1-click "⚡ Force Play" button on the unpause blocked alert banner. Clicking it dispatches `PlayerCommand::ForcePlay`, which sets `force_play = true`, clears `gate_blocked`, and initiates synchronized unpause immediately.
3. *Requirement*: Retain 72-byte `PlayerUiSnapshot` size invariant for wait-free triple buffering.
   - *Implementation*: Peer metadata contains unbounded strings (`file_name`, `username`) and cannot fit in fixed-size triple-buffer snapshots. Rich peer metadata is transmitted via `UiEvent::RoomMembersUpdated`, while compact status flags (`GATE_BLOCKED`, `FORCE_PLAY`, `LOCAL_READY`, `HAS_MISMATCH`) are packed into unused upper bits of `playback_state: u8`. This guarantees exact 72-byte sizing without cache-line false sharing or heap allocations.
4. *Requirement*: Multi-platform file matching diagnostics.
   - *Implementation*: Peers running Windows report paths with `\`, while Linux peers report `/`. `extract_basename` strips leading path segments regardless of delimiter, allowing `C:\Media\movie.mkv` and `/media/movie.mkv` to match cleanly. Furthermore, a ±2000ms tolerance handles differences in container muxing timestamps across re-encodes.

## 3. Caveats
- No caveats. The implementation strictly conforms to all requirements in `ORIGINAL_REQUEST.md` (section `## 2026-10-26T11:12:06Z`) and `AGENTS.md`. No banned crates (`mpv`, `vlc`, `sqlite`, `rocksdb`) were introduced.

## 4. Conclusion
Phase 5 Milestone 4 is fully implemented, verified, and complete. All unit tests, integration tests, clippy linter checks, dependency audits, intent harnesses, and Phase 1 verification scripts pass with zero warnings and zero errors.

## 5. Verification Method
To independently verify the implementation:
```bash
# 1. Dependency audit
./scripts/audit_dependencies.sh

# 2. Workspace compilation and warning check
cargo check --workspace --all-targets

# 3. Strict Clippy check
cargo clippy --workspace --all-targets -- -D warnings

# 4. Milestone 4 integration test suite
cargo test -p syncplay-core --test unpause_gate_and_peer_diagnostics_integration -- --nocapture

# 5. Full workspace test suite
cargo test --workspace

# 6. Intent verification harness
python3 scripts/intent_harness.py

# 7. End-to-end automated verification script
./scripts/verify_phase1.sh
```
