# Orchestrator 5 Dispatch — Syncplay-Alt Watch-Party Social & Auto-Update Polish

## Mission
Implement auto-update detection from GitHub releases, peer media status display with file matching diagnostics, peer readiness state with a strict auto-pause playback gate, and synchronized room activity announcements.

## Authoritative User Request
See `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (section `## 2026-10-26T11:12:06Z`).

## Key Requirements & Invariants
- R1: Auto-Update Detection & Notification (asynchronous, non-blocking GitHub API release check, Help menu trigger, notification banner with download/dismiss).
- R2: Peer Media Status & File Match Diagnostics (UDP wire packet broadcast of filename, duration, size; sidebar display; match/mismatch badge).
- R3: Peer Readiness & Strict Playback Unpause Gate (readiness toggle, UDP sync, pause gate if peers unready, room alert, host "Force Play" bypass).
- R4: Automated Room Activity Announcements (formatted chat announcements for media loaded, readiness, play/pause, seek).
- R5: Zero warnings (`#![deny(warnings)]`, `-D warnings`), invariant prohibitions (no monolithic player frameworks `mpv`/`vlc`, no database engines `sqlite`/`rocksdb`), all tests pass, git commit and push to `origin/main`.


## 2026-10-26T11:13:32Z
You are the Project Orchestrator for Phase 5 of Syncplay-Alt: Watch-Party Social & Auto-Update Polish.

Your working directory is: `/home/gautham/syncplay-alt/.agents/teamwork/orchestrator_5/`
Your identity is: `orchestrator_5`
The workspace root is: `/home/gautham/syncplay-alt`
Your dispatch instructions and user request are in `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` under `## 2026-10-26T11:12:06Z` and in `/home/gautham/syncplay-alt/.agents/teamwork/orchestrator_5/DISPATCH.md`.

## Requirements Summary
1. R1: Auto-Update Detection & Notification
   - Async background update checker querying GitHub Releases API (`https://api.github.com/repos/Gauthampro7/syncplay-desktop-preview/releases/latest`).
   - Run on startup and add manual "Check for Updates..." under Help menu.
   - UI banner for newer version tag with 1-click action (open release page / direct download) and Dismiss button.
   - Non-blocking, handle timeouts/rate limits/offline gracefully.
2. R2: Peer Media Status & File Match Diagnostics
   - Transmit media metadata over UDP wire packets: filename (basename), duration_ms, file_size_bytes.
   - Display peer media info in sidebar: filename, formatted duration `HH:MM:SS`, or `[⏳ No file loaded]`.
   - Diagnostic badge comparing local vs peer media: `[Match ✔]` (duration ±2s / filename match) vs `[⚠️ Mismatch]`.
   - Instant broadcast on media open/change.
3. R3: Peer Readiness & Strict Playback Unpause Gate
   - "Ready to Watch" toggle checkbox in sidebar for local user (defaulting to ready on media load).
   - Sync readiness state over UDP wire packets.
   - Sidebar badge: `[✓ Ready]` (green) or `[⏳ Not Ready]` (amber).
   - Strict unpause gate: if any peer has no media loaded or is unready when play/unpause is triggered, hold playback paused, display on-screen alert/notification, broadcast chat announcement.
   - "Force Play" bypass toggle/button for host.
4. R4: Automated Room Activity Announcements
   - Broadcast chat system announcements: media loaded, readiness changed, paused, resumed, seeked.
5. R5: Verification, Zero Warnings & Push
   - Strict `#![deny(warnings)]` and `cargo clippy --workspace --all-targets -- -D warnings`.
   - Zero banned dependencies (`mpv`, `vlc`, `sqlite`, `rocksdb`).
   - Unit & integration tests for updater, wire packet exchange, readiness unpause gate.
   - Commit cleanly and push to `origin/main`.
