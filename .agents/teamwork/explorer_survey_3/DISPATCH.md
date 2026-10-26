# Dispatch: Explorer Survey 3 (Auto-Updater & Room Activity Announcements)

## Objective
Investigate:
1. Auto-update requirements:
   - Querying GitHub Releases API (`https://api.github.com/repos/Gauthampro7/syncplay-desktop-preview/releases/latest`) asynchronously in background without blocking UI or playback.
   - Checking existing workspace dependencies (e.g. `ureq`, `reqwest`, `minreq`, or raw HTTP via `std::net`, or if a lightweight HTTP client is needed - check banned crate list in AGENTS.md!).
   - Semver parsing / comparison logic between `CARGO_PKG_VERSION` and release tag (e.g. `v0.1.2` > `v0.1.1`).
   - Action handler to open release page or download asset (e.g., using `open` crate or system launcher).
   - Error handling: timeouts, rate limits, offline gracefully handled.
2. Room activity announcements:
   - How `syncplay-chat` works (`syncplay_chat.bin`, chat messages, system announcements).
   - How chat messages are broadcast and displayed in UI.
   - How room events (media loaded, readiness changed, paused, resumed, seeked) can format and inject system announcement chat messages.

## Input Information
- `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (read section `## 2026-10-26T11:12:06Z`)
- `/home/gautham/syncplay-alt/AGENTS.md` (check dependency whitelist & prohibitions!)
- `/home/gautham/syncplay-alt/Cargo.toml`
- `/home/gautham/syncplay-alt/crates/syncplay-chat`
- `/home/gautham/syncplay-alt/crates/syncplay-core`

## Output Requirements
Produce a comprehensive report in `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_3/handoff.md`.
Include:
- Recommended HTTP strategy compliant with whitelist and zero-warning rules
- Semver comparison details and tag normalization
- Update check lifecycle (startup background thread + manual menu trigger)
- Chat announcement format and event trigger integration points
- Security, offline, rate-limiting considerations


## 2026-10-26T11:15:20Z
You are Explorer 3 for Phase 5 Survey.
Your identity: explorer_survey_3
Your working directory: /home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_3/
Read your dispatch assignment in /home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_3/DISPATCH.md and the authoritative user request in /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (under section `## 2026-10-26T11:12:06Z`).

Investigate auto-update architecture and room activity announcements.
Analyze:
1. Querying GitHub Releases API asynchronously in background, parsing semver, handling rate limits/offline.
2. Compliance with AGENTS.md dependency whitelist and zero-warning rules.
3. syncplay-chat room activity announcements (media loaded, readiness changed, paused, resumed, seeked) and how system messages are formatted and broadcast.

Produce your comprehensive handoff report at /home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_3/handoff.md and message back when complete.
