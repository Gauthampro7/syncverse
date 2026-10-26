# BRIEFING — 2026-10-26T12:16:00Z

## Mission
Implement Phase 5 Milestone 4: Peer Media Status, Diagnostics & Unpause Gate UI in `syncplay-ui` and `syncplay-core`.

## 🔒 My Identity
- Archetype: worker
- Roles: implementer, qa, specialist
- Working directory: /home/gautham/syncplay-alt/.agents/teamwork/worker_m4
- Original parent: 1a21f3c9-f57a-4ad4-b306-97b4ba0e698e
- Milestone: Milestone 4: Phase 1 Verification Harness
- Phase 5 Role: Worker M4 (Peer Media Status, Diagnostics & Unpause Gate UI)

## 🔒 Key Constraints
- Exclusively own `/home/gautham/syncplay-alt/scripts/verify_phase1.sh`
- Pure, genuine implementation; no shortcuts, no hardcoded test results
- Zero external codec/database frameworks (strict enforcement per AGENTS.md)
- Must implement all 7 automated validation stages
- Must test and ensure `./scripts/verify_phase1.sh` executes from any directory, passes all stages cleanly, and exits 0
- Phase 5: Maintain strict 72-byte size invariant for `PlayerUiSnapshot`.
- Phase 5: Zero compiler warnings (`#![deny(warnings)]`), zero clippy warnings (`-D warnings`).
- Phase 5: Zero banned dependencies (no `mpv`, `vlc`, `sqlite`, `rocksdb`).
- Phase 5: Files owned: `crates/syncplay-ui/src/gui/sidebar.rs`, `crates/syncplay-ui/src/gui/app.rs`, `crates/syncplay-ui/src/command.rs`, `crates/syncplay-ui/src/snapshot.rs`, `crates/syncplay-core/src/playback_runtime.rs`, `crates/syncplay-core/tests/`.

## Current Parent
- Conversation ID: ad8bd8cc-722a-48fc-97f9-4ff7f3c97d5d
- Updated: 2026-10-26T11:51:06Z

## Task Summary
- **What to build**:
  - Extend `RoomMember` in `crates/syncplay-ui/src/gui/sidebar.rs` with media info & readiness.
  - Render sidebar with "Ready to Watch" checkbox, readiness badges (`[✓ Ready]` / `[⏳ Not Ready]`), loaded media filename & duration, file match diagnostic badges (`[Match ✔]` / `[⚠️ Mismatch]`), and Host "⚡ Force Play" bypass.
  - Add commands to `PlayerCommand`: `SetReady(bool)`, `SetForcePlay(bool)`, `ForcePlay`.
  - Add `UiEvent::UnpauseBlocked { unready_peers, message }` and handle banner in `SyncplayApp`.
  - Add status bitflags to `PlayerUiSnapshot` (`SNAPSHOT_FLAG_GATE_BLOCKED`, `SNAPSHOT_FLAG_FORCE_PLAY`, `SNAPSHOT_FLAG_LOCAL_READY`, `SNAPSHOT_FLAG_HAS_MISMATCH`) while maintaining exact 72-byte size.
  - Hook unpause gate into `playback_runtime.rs` on `TogglePause`, dispatch alert banner & chat announcements (`RoomAnnouncement::gate_waiting`).
  - Broadcast `PeerStatus` and standardized `RoomAnnouncement` on media load, ready toggle, pause, resume, seek.
  - Render on-screen alert banner over viewport when unpause is blocked with 1-click Host "Force Play" bypass.
  - Unit and integration tests in `crates/syncplay-ui` and `crates/syncplay-core/tests/`.
- **Success criteria**: All checks pass: cargo check, cargo clippy, cargo test, zero warnings, genuine logic, zero banned dependencies.
- **Interface contracts**: `PROJECT.md`, `DISPATCH.md`, `ORIGINAL_REQUEST.md`.
- **Code layout**: `crates/syncplay-ui/`, `crates/syncplay-core/`.

## Key Decisions Made
- [Phase 5] Use bitflags in `PlayerUiSnapshot::playback_state` to ensure 72-byte struct invariant is strictly preserved.
- [Phase 5] Rich peer metadata (`RoomMember`) conveyed via `UiEvent::RoomMembersUpdated`.
- [Phase 5] Path basename extraction handles both `/` and `\` for cross-platform Linux and Windows peer compatibility.
- [Phase 5] Duration tolerance for media match diagnostics set to ±2000ms.

## Artifact Index
- crates/syncplay-ui/src/command.rs — SetReady, SetForcePlay, ForcePlay
- crates/syncplay-ui/src/event.rs — UiEvent::UnpauseBlocked
- crates/syncplay-ui/src/snapshot.rs — SNAPSHOT_FLAG_* bitflags & query helpers
- crates/syncplay-ui/src/gui/sidebar.rs — RoomMember media metadata, match evaluation, sidebar UI
- crates/syncplay-ui/src/gui/app.rs — Unpause blocked banner handling & viewport rendering
- crates/syncplay-core/src/coordinator.rs — Match arms for new PlayerCommand variants
- crates/syncplay-core/src/playback_runtime.rs — Unpause gate logic, peer status & announcement broadcasts
- crates/syncplay-core/tests/unpause_gate_and_peer_diagnostics_integration.rs — Integration tests

## Change Tracker
- **Files modified**: `crates/syncplay-ui/src/command.rs`, `crates/syncplay-ui/src/event.rs`, `crates/syncplay-ui/src/snapshot.rs`, `crates/syncplay-ui/src/gui/sidebar.rs`, `crates/syncplay-ui/src/gui/app.rs`, `crates/syncplay-ui/src/gui/dialogs.rs`, `crates/syncplay-ui/src/gui/mod.rs`, `crates/syncplay-ui/src/lib.rs`, `crates/syncplay-core/src/coordinator.rs`, `crates/syncplay-core/src/playback_runtime.rs`, `crates/syncplay-core/tests/unpause_gate_and_peer_diagnostics_integration.rs`
- **Build status**: All cargo check, cargo clippy, cargo test, intent harness, and verify_phase1.sh pass with 0 errors and 0 warnings.
- **Pending issues**: None.

## Quality Status
- **Build/test result**: All workspace tests passed (100% pass rate).
- **Lint status**: Clippy clean (`-D warnings`).
- **Tests added/modified**: 6 unit tests in `crates/syncplay-ui/src/gui/sidebar.rs`, 6 integration tests in `crates/syncplay-core/tests/unpause_gate_and_peer_diagnostics_integration.rs`.

## Loaded Skills
- None
