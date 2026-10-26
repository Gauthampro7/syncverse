# Dispatch: Worker M3 (Room Activity Announcements Module)

## Objective
Implement Milestone 3 of Syncplay-Alt Phase 5:
1. Create `crates/syncplay-chat/src/announcements.rs`:
   - Standardized announcement formatting conforming to `ORIGINAL_REQUEST.md` (R4):
     - Media loaded: `🎬 {user} loaded '{filename}' ({duration})`
     - Readiness changed: `✓ {user} is ready to watch` or `⏳ {user} is not ready`
     - Playback paused: `⏸ {user} paused playback`
     - Playback resumed: `▶ {user} resumed playback`
     - Seeking: `⏩ {user} seeked to {timecode}`
     - Unpause gate waiting (R3): `⏸ Paused: Waiting for {users} to load media and get ready...`
   - Include helper functions to format timecode from milliseconds (`format_timecode(pts_ms: u64) -> String` producing `HH:MM:SS` or `MM:SS`).
2. In `crates/syncplay-chat/src/lib.rs`:
   - Expose `pub mod announcements;` and re-export `RoomAnnouncement`.
3. Unit tests in `crates/syncplay-chat`:
   - Test formatting of all announcement types.
   - Test binary persistence: construct `ChatRecord::new_text` with announcement text, write to binary log with `0xAA55` framing and CRC32, read back and verify.
   - Test full-text search: index announcement records into `InvertedIndex` and verify search tokens (`"loaded"`, `"ready"`, `"paused"`, `"resumed"`, `"seeked"`, `"waiting"`).

## Files Owned Exclusively
- `crates/syncplay-chat/src/announcements.rs`
- `crates/syncplay-chat/src/lib.rs`
- `crates/syncplay-chat/tests/` (or tests inside `announcements.rs`)

## References
- User request: `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (read section `## 2026-10-26T11:12:06Z`)
- Project spec: `/home/gautham/syncplay-alt/PROJECT.md`
- Survey report: `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_3/handoff.md`

## Invariants & Warnings
- Zero compiler warnings (`#![deny(warnings)]`).
- Clippy clean: `cargo clippy -p syncplay-chat --all-targets -- -D warnings`.
- Zero banned dependencies (no `mpv`, `vlc`, `sqlite`, `rocksdb`).
- All existing tests in `syncplay-chat` must continue to pass.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## Completion Criteria
Run `cargo check -p syncplay-chat --all-targets`, `cargo test -p syncplay-chat`, and `cargo clippy -p syncplay-chat --all-targets -- -D warnings`. All must pass with 0 warnings.
Write your completion report in `/home/gautham/syncplay-alt/.agents/teamwork/worker_m3/handoff.md` and message back.

## 2026-10-26T11:43:21Z
You are Worker M3 for Phase 5 Milestone 3 (Room Activity Announcements).
Your identity: worker_m3
Your working directory: /home/gautham/syncplay-alt/.agents/teamwork/worker_m3/
Read your dispatch assignment in /home/gautham/syncplay-alt/.agents/teamwork/worker_m3/DISPATCH.md and the authoritative user request in /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (under section `## 2026-10-26T11:12:06Z`). Also read the survey report in /home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_3/handoff.md.

Implement Milestone 3:
- Create `crates/syncplay-chat/src/announcements.rs`: standardized formatting for all 5 room events (media loaded, ready/unready, paused, resumed, seeked) + unpause gate waiting.
- In `crates/syncplay-chat/src/lib.rs`: expose `pub mod announcements;` and re-export `RoomAnnouncement`.
- Unit tests: formatting, binary log persistence with `0xAA55` framing and CRC32, and inverted index full-text search.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Run builds and tests: `cargo check -p syncplay-chat --all-targets`, `cargo test -p syncplay-chat`, and `cargo clippy -p syncplay-chat --all-targets -- -D warnings`.
Produce your handoff report at /home/gautham/syncplay-alt/.agents/teamwork/worker_m3/handoff.md and message back when complete.
