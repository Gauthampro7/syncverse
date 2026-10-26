# Dispatch: Worker M4 (Peer Media Status, Diagnostics & Unpause Gate UI)

## Objective
Implement Milestone 4 of Syncplay-Alt Phase 5:
1. In `crates/syncplay-ui/src/gui/sidebar.rs`:
   - Extend `RoomMember`:
     ```rust
     pub struct RoomMember {
         pub username: String,
         pub is_local: bool,
         pub is_host: bool,
         pub rtt_ms: f64,
         pub file_name: Option<String>,
         pub duration_ms: Option<u64>,
         pub file_size_bytes: Option<u64>,
         pub is_ready: bool,
     }
     ```
   - Render in the watch-party sidebar:
     - "Ready to Watch" toggle checkbox for local user (defaulting to ready on media load). Sends `PlayerCommand::SetReady(bool)` on toggle.
     - Per-member readiness badges: `[✓ Ready]` in green (`Color32::from_rgb(34, 197, 94)`) vs `[⏳ Not Ready]` in amber (`Color32::from_rgb(245, 158, 11)`).
     - Loaded media display: filename, formatted `HH:MM:SS` duration, or `[⏳ No file loaded]`.
     - File match diagnostic badge comparing peer media against local media:
       - `[Match ✔]` in green if duration matches within ±2000ms OR filename matches (case-insensitive basename).
       - `[⚠️ Mismatch]` in amber/red if duration or filename differs significantly.
       - `[⏳ No file loaded]` if peer has no media.
     - Host "Force Play" bypass toggle/button: `⚡ Force Play (Bypass Gate)`.
2. In `crates/syncplay-ui/src/command.rs`:
   - Add `SetReady(bool)`, `SetForcePlay(bool)`, `ForcePlay`.
3. In `crates/syncplay-ui/src/event.rs`:
   - Ensure `UiEvent::UnpauseBlocked { unready_peers: Vec<String>, message: String }` and `UiEvent::RoomMembersUpdated(Vec<RoomMember>)` are defined.
4. In `crates/syncplay-ui/src/snapshot.rs`:
   - Maintain strict 72-byte size invariant (`assert_eq!(std::mem::size_of::<PlayerUiSnapshot>(), 72)`).
   - Add status bitflags in upper bits of `playback_state` (`SNAPSHOT_FLAG_GATE_BLOCKED`, `SNAPSHOT_FLAG_FORCE_PLAY`, `SNAPSHOT_FLAG_LOCAL_READY`, `SNAPSHOT_FLAG_HAS_MISMATCH`).
5. In `crates/syncplay-core/src/playback_runtime.rs`:
   - Track local readiness (`local_ready: bool`, default true when media is loaded) and `force_play: bool`.
   - Update `RoomMembersUpdated` event dispatched to UI with complete peer media and readiness info.
   - When opening/changing media:
     - Update local media info and set `local_ready = true`.
     - Broadcast `PeerStatus` over UDP (`client_node.send_peer_status` or `host_server.broadcast_peer_status`).
     - Broadcast room activity chat announcement: `RoomAnnouncement::media_loaded`.
   - On `PlayerCommand::SetReady(ready)`:
     - Update `local_ready = ready`.
     - Broadcast `PeerStatus`.
     - Broadcast chat announcement: `RoomAnnouncement::ready` or `RoomAnnouncement::not_ready`.
   - On `PlayerCommand::TogglePause` while paused (unpause trigger):
     - If in a watch party: evaluate if any connected peer has no media loaded or is `Not Ready`, or local user is unready/has no media.
     - If unready peers exist and `force_play == false`:
       - Hold playback paused (`clock.set_state(now, PlaybackState::Paused)`).
       - Maintain audio sink paused.
       - Dispatch `UiEvent::UnpauseBlocked` to UI.
       - Broadcast chat announcement: `RoomAnnouncement::gate_waiting`.
     - If all peers ready or `force_play == true`:
       - Resume playback and audio.
       - If Host: initiate barrier unpause (`prepare_barrier_unpause`).
       - Broadcast chat announcement: `RoomAnnouncement::resumed`.
   - On `PlayerCommand::TogglePause` while playing (pause trigger):
     - Pause playback and audio.
     - Broadcast chat announcement: `RoomAnnouncement::paused`.
   - On `PlayerCommand::Seek` completion (debounced):
     - Broadcast chat announcement: `RoomAnnouncement::seeked`.
   - In UI: render on-screen alert banner over viewport when unpause is blocked (`"⏸ Paused: Waiting for [Username(s)] to load media and get ready..."`) with a 1-click Host "Force Play" bypass button.
6. Tests:
   - Unit tests for file match diagnostics (±2s, filename match, mismatch, no media).
   - Integration tests in `crates/syncplay-core` for unpause gate blocking and force play override.

## Files Owned Exclusively
- `crates/syncplay-ui/src/gui/sidebar.rs`
- `crates/syncplay-ui/src/gui/app.rs` (on-screen unpause banner & event handling)
- `crates/syncplay-ui/src/command.rs`
- `crates/syncplay-ui/src/snapshot.rs`
- `crates/syncplay-core/src/playback_runtime.rs`
- `crates/syncplay-core/tests/`

## References
- User request: `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (read section `## 2026-10-26T11:12:06Z`)
- Project spec: `/home/gautham/syncplay-alt/PROJECT.md`
- Survey report: `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_1/handoff.md`
- Survey report: `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_2/handoff.md`
- Wire protocol handoff: `/home/gautham/syncplay-alt/.agents/teamwork/worker_m2/handoff.md`
- Announcements handoff: `/home/gautham/syncplay-alt/.agents/teamwork/worker_m3/handoff.md`

## Invariants & Warnings
- Zero compiler warnings (`#![deny(warnings)]`).
- Clippy clean: `cargo clippy --workspace --all-targets -- -D warnings`.
- Zero banned dependencies (no `mpv`, `vlc`, `sqlite`, `rocksdb`).
- Snapshot size MUST be exactly 72 bytes.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## Completion Criteria
Run `cargo check --workspace --all-targets`, `cargo test -p syncplay-ui`, `cargo test -p syncplay-core`, and `cargo clippy --workspace --all-targets -- -D warnings`. All must pass with 0 warnings.
Write completion report in `/home/gautham/syncplay-alt/.agents/teamwork/worker_m4/handoff.md` and message back.

## 2026-10-26T11:51:06Z

You are Worker M4 for Phase 5 Milestone 4 (Peer Media Status, Diagnostics & Unpause Gate UI).
Your identity: worker_m4
Your working directory: /home/gautham/syncplay-alt/.agents/teamwork/worker_m4/
Read your dispatch assignment in /home/gautham/syncplay-alt/.agents/teamwork/worker_m4/DISPATCH.md and the authoritative user request in /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (under section `## 2026-10-26T11:12:06Z`). Also read the survey reports and handoff reports from worker_m1, worker_m2, and worker_m3.

Implement Milestone 4:
- Extend `RoomMember` in `crates/syncplay-ui/src/gui/sidebar.rs` with media info and readiness.
- Render sidebar: "Ready to Watch" checkbox, `[✓ Ready]` / `[⏳ Not Ready]`, peer media filename & `HH:MM:SS` duration or `[⏳ No file loaded]`, `[Match ✔]` / `[⚠️ Mismatch]` diagnostic badges, and Host `⚡ Force Play` toggle.
- Add commands in `crates/syncplay-ui/src/command.rs`: `SetReady(bool)`, `SetForcePlay(bool)`, `ForcePlay`.
- Ensure `PlayerUiSnapshot` memory layout is strictly 72 bytes.
- In `crates/syncplay-core/src/playback_runtime.rs`:
  - Hook unpause gate into `PlayerCommand::TogglePause`: hold playback paused if unready/unloaded peers exist, dispatch `UiEvent::UnpauseBlocked`, broadcast `RoomAnnouncement::gate_waiting` chat announcement.
  - Bypass unpause gate if all peers are ready or Host has `force_play == true`.
  - Hook media open, readiness toggle, pause, resume, and seek to broadcast `PeerStatus` and standardized `RoomAnnouncement` chat announcements.
  - Render on-screen alert banner over viewport when unpause is blocked with 1-click Host "Force Play" bypass.
- Write unit and integration tests verifying diagnostics, readiness toggle, unpause gate, and force play.
