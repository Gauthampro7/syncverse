# Dispatch: Worker M1 (Auto-Update Detection & Notification)

## Objective
Implement Milestone 1 of Syncplay-Alt Phase 5:
1. Add `ureq` (with `tls`, `json`) and `semver = "1.0"` to `crates/syncplay-ui/Cargo.toml`.
2. Implement `crates/syncplay-ui/src/gui/updater.rs`:
   - Query GitHub Releases API asynchronously in background thread: `https://api.github.com/repos/Gauthampro7/syncplay-desktop-preview/releases/latest`.
   - Set 5.0 second timeout and User-Agent: `syncplay-alt/<version>`.
   - Semver parsing and normalization (`v0.1.2` -> `0.1.2`), compare against `CARGO_PKG_VERSION` (`0.1.1`).
   - Rate limit (HTTP 403/429 with reset time), timeout, offline error handling without panicking.
   - Action handler to open browser to download URL or release page (`open_browser_url` / `ctx.open_url`).
3. In `crates/syncplay-ui/src/event.rs`:
   - Add `UpdateAvailable { latest_version: String, release_url: String, download_url: Option<String> }`.
   - Add `UpdateChecked { is_latest: bool, current_version: String }`.
4. In `crates/syncplay-ui/src/gui/app.rs`:
   - Top notification banner: render when `update_info` is Some and not dismissed:
     - Title `"🚀 Update Available: v{latest}"`
     - Action buttons: `"📥 Download"` (opens direct installer or release page), `"Release Page"`
     - `"✖ Dismiss"` button to hide banner
   - Help menu: add `"🔄 Check for Updates..."` item triggering manual check.
5. Unit tests:
   - Semver tag normalization & comparison (`v0.1.2` > `0.1.1`, `0.2.0` > `0.1.9`, invalid tags).
   - Rate limiting and offline error handling.

## Files Owned Exclusively
- `crates/syncplay-ui/Cargo.toml`
- `crates/syncplay-ui/src/gui/updater.rs`
- `crates/syncplay-ui/src/gui/app.rs`
- `crates/syncplay-ui/src/gui/mod.rs`
- `crates/syncplay-ui/src/event.rs`

## References
- User request: `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (read section `## 2026-10-26T11:12:06Z`)
- Project spec: `/home/gautham/syncplay-alt/PROJECT.md`
- Survey report: `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_3/handoff.md`
- Survey report: `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_2/handoff.md`

## Invariants & Warnings
- Zero compiler warnings (`#![deny(warnings)]`).
- Clippy clean: `cargo clippy -p syncplay-ui -- -D warnings`.
- Zero banned dependencies (no `mpv`, `vlc`, `sqlite`, `rocksdb`).
- Non-blocking: NEVER execute blocking network I/O on UI rendering thread.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## Completion Criteria
Run `cargo check -p syncplay-ui`, `cargo test -p syncplay-ui`, and `cargo clippy -p syncplay-ui -- -D warnings`. All must pass with 0 warnings.
Write your completion report in `/home/gautham/syncplay-alt/.agents/teamwork/worker_m1/handoff.md` and message back.


## 2026-10-26T11:27:33Z
You are Worker M1 for Phase 5 Milestone 1 (Auto-Update Detection & Notification).
Your identity: worker_m1
Your working directory: /home/gautham/syncplay-alt/.agents/teamwork/worker_m1/
Read your dispatch assignment in /home/gautham/syncplay-alt/.agents/teamwork/worker_m1/DISPATCH.md and the authoritative user request in /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (under section `## 2026-10-26T11:12:06Z`). Also read the survey report in /home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_3/handoff.md.

Implement Milestone 1:
- Add `ureq` (with `tls`, `json`) and `semver = "1.0"` to `crates/syncplay-ui/Cargo.toml`.
- Implement `crates/syncplay-ui/src/gui/updater.rs`: async background querying of GitHub Releases API, semver normalization & comparison, rate-limit/offline/timeout error handling, URL launcher.
- Implement top notification banner in `crates/syncplay-ui/src/gui/app.rs` with 1-click download/release page button and Dismiss button.
- Add "Check for Updates..." to Help menu.
- Unit tests for semver comparison, update checking, error handling.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Run builds and tests: `cargo check -p syncplay-ui`, `cargo test -p syncplay-ui`, and `cargo clippy -p syncplay-ui -- -D warnings`.
Produce your handoff report at /home/gautham/syncplay-alt/.agents/teamwork/worker_m1/handoff.md and message back when complete.
