# Plan: Syncplay-Alt Phase 5 (Watch-Party Social & Auto-Update Polish)

## Milestone Breakdown

### Milestone 1: Auto-Update Detection & Notification (R1)
- Scope: `crates/syncplay-ui`, `crates/syncplay-ui/Cargo.toml`
- Deliverables:
  - Add `ureq` (or lightweight HTTP client) and `semver` to `syncplay-ui`.
  - Implement `crates/syncplay-ui/src/gui/updater.rs`: async background querying of GitHub Releases API, semver normalization & comparison, rate-limit/offline/timeout error handling, URL launcher.
  - Implement top notification banner in `crates/syncplay-ui/src/gui/app.rs` with 1-click download/release page button and Dismiss button.
  - Add "Check for Updates..." to Help menu.
  - Unit tests for semver comparison, update checking, error handling.

### Milestone 2: Wire Protocol & Peer State Tracking (R2, R3 wire)
- Scope: `crates/syncplay-sync`
- Deliverables:
  - Add `PacketType::PeerStatus = 0x09` in `crates/syncplay-sync/src/wire.rs`.
  - Implement `PeerStatusPayload`: `client_id`, `has_media`, `is_ready`, `duration_ms`, `file_size_bytes`, `username`, `filename`.
  - Binary encoding, decoding, length checks, and CRC32 table digest validation.
  - Update negative test fixtures in `crates/syncplay-sync/tests/chat_transport_adversarial.rs:303` and `crates/syncplay-sync/tests/watch_party_transport.rs:483`.
  - Extend `ClientSession` in `transport.rs`: separate `is_user_ready` from `barrier_preroll_ready`, store media metadata.
  - Implement UDP broadcasting of `PeerStatus` upon media open/change and readiness toggle.
  - Unit & integration tests for serialization and multi-peer status exchange.

### Milestone 3: Room Activity Announcements (R4)
- Scope: `crates/syncplay-chat`, `crates/syncplay-core`
- Deliverables:
  - Implement `crates/syncplay-chat/src/announcements.rs`: standardized formatting for media loaded, ready/unready, paused, resumed, seeked, and unpause gate blocked.
  - Integrate event hooks in `crates/syncplay-core/src/playback_runtime.rs` to format and broadcast system announcements via UDP `ChatMessage` (`0x08`).
  - Seek announcement debouncing: emit only on seek completion, not during scrubber dragging.
  - Store in `syncplay_chat.bin` with `0xAA55` framing and CRC32; index in in-memory inverted index FTS.
  - Unit tests for announcement formatting, binary log persistence, and FTS search.

### Milestone 4: Peer Media Status, File Match Diagnostics, Readiness & Unpause Gate UI (R2, R3 UI & Core)
- Scope: `crates/syncplay-ui`, `crates/syncplay-core`
- Deliverables:
  - Extend `RoomMember` in `syncplay-ui/src/gui/sidebar.rs` with `file_name`, `duration_ms`, `file_size_bytes`, `is_ready`.
  - Implement sidebar display: filename, formatted `HH:MM:SS` duration or `[⏳ No file loaded]`.
  - Implement diagnostic badges: `[Match ✔]` (duration ±2s or filename match) vs `[⚠️ Mismatch]`.
  - Implement "Ready to Watch" toggle checkbox in sidebar (defaulting to true on media load) and peer badges: `[✓ Ready]` vs `[⏳ Not Ready]`.
  - Implement strict unpause gate in `crates/syncplay-core/src/playback_runtime.rs`: hold playback paused if unready/unloaded peers exist, dispatch `UiEvent::UnpauseBlocked` for on-screen notification, broadcast chat announcement.
  - Host "Force Play" bypass toggle in sidebar and on-screen notification.
  - Maintain 72-byte `PlayerUiSnapshot` memory invariant.
  - Unit and integration tests for UI rendering, diagnostic badges, and unpause gate behavior.

### Milestone 5: E2E Testing Track, Final Verification & Git Push (R5)
- Scope: Project-wide
- Deliverables:
  - Parallel E2E test suite published via `TEST_READY.md`.
  - Run `./scripts/audit_dependencies.sh` (0 violations).
  - Run `cargo check --workspace --all-targets` (0 warnings).
  - Run `cargo test --workspace` (100% passing).
  - Run `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings).
  - Run `python3 scripts/intent_harness.py` (100% compliance).
  - Git commit cleanly and push to `origin/main`.
