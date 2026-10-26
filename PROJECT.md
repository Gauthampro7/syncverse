# Project: Syncplay-Alt Phase 5 (Watch-Party Social & Auto-Update Polish)

## Architecture
Syncplay-Alt is a standalone native desktop media player and synchronized watch-party platform in pure Rust. Phase 5 adds social polish, peer media matching diagnostics, peer readiness tracking with a strict playback unpause gate, room activity announcements, and automated GitHub release update checking.

### Subsystem Interaction & Data Flow
1. **Auto-Updater (`syncplay-ui/src/gui/updater.rs`)**:
   - Asynchronous worker queries GitHub Releases API on startup and via Help menu.
   - Normalizes semver tags and compares with `CARGO_PKG_VERSION`.
   - Dispatches `UiEvent::UpdateAvailable` to egui.
   - Renders top banner with download/release link and dismiss button.

2. **Wire Protocol & Peer State (`syncplay-sync/src/wire.rs`, `syncplay-sync/src/transport.rs`)**:
   - New wire packet `PeerStatus` (opcode `0x09`) encapsulates peer media metadata (filename, duration_ms, file_size_bytes) and persistent user readiness (`is_ready`).
   - `ClientSession` tracks persistent `is_user_ready`, distinct from transient `barrier_preroll_ready`.
   - Host broadcasts peer status updates to all room members over UDP.

3. **Room Announcements & Chat Persistence (`syncplay-chat`, `syncplay-core`)**:
   - Key room events (media loaded, ready/unready, pause, resume, seek, gate waiting) format standardized chat announcements with emojis.
   - Broadcast via `ChatMessage` (`0x08`) over UDP.
   - Stored in `syncplay_chat.bin` with `0xAA55` framing and CRC32; indexed in in-memory inverted index FTS.

4. **UI Sidebar, Diagnostics & Unpause Gate (`syncplay-ui`, `syncplay-core`)**:
   - `RoomMember` holds peer media info, duration, and readiness.
   - Sidebar displays `[✓ Ready]` / `[⏳ Not Ready]`, peer duration `HH:MM:SS` (or `[⏳ No file loaded]`), and `[Match ✔]` / `[⚠️ Mismatch]` badges.
   - Unpause gate in `playback_runtime.rs` intercepts unpause commands: halts if any peer is unready/unloaded unless Host "Force Play" bypass is active.
   - `PlayerUiSnapshot` strictly maintains 72 bytes.

---

## Feature Inventory
Every feature from the Survey phase and user request:

| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | GitHub Releases API Querying | Query `https://api.github.com/repos/Gauthampro7/syncplay-desktop-preview/releases/latest` asynchronously with timeouts & rate limit handling | M1 | ORIGINAL_REQUEST §R1 |
| 2 | Semver Parsing & Comparison | Normalize `vX.Y.Z` and compare against `CARGO_PKG_VERSION` | M1 | ORIGINAL_REQUEST §R1 |
| 3 | Auto-Update UI Banner & Actions | Top notification banner with 1-click download/release page link and Dismiss button | M1 | ORIGINAL_REQUEST §R1 |
| 4 | Help Menu "Check for Updates" | Manual menu trigger under Help menu | M1 | ORIGINAL_REQUEST §R1 |
| 5 | Wire Protocol Media Metadata | Wire packet transmitting filename, duration_ms, file_size_bytes | M2 | ORIGINAL_REQUEST §R2 |
| 6 | Wire Protocol Readiness State | Wire packet transmitting `Ready` vs `Not Ready` state | M2 | ORIGINAL_REQUEST §R3 |
| 7 | Negative Test Fixtures Update | Update `chat_transport_adversarial.rs` & `watch_party_transport.rs` opcode lists | M2 | Survey Explorer 1 |
| 8 | Host Peer Session Decoupling | Decouple `ClientSession` `is_user_ready` from `barrier_preroll_ready` | M2 | Survey Explorer 1 |
| 9 | Media Status Broadcast on Change | Instant UDP broadcast upon media open or change | M2 | ORIGINAL_REQUEST §R2 |
| 10 | Room Activity Announcements | Standardized chat announcements for media loaded, ready/unready, pause, resume, seek | M3 | ORIGINAL_REQUEST §R4 |
| 11 | Seek Announcement Debouncing | Avoid chat flood during continuous scrubber drag | M3 | Survey Explorer 3 |
| 12 | System Chat Broadcast & Persistence | Send announcements over UDP `ChatMessage`, persist in `syncplay_chat.bin`, index in FTS | M3 | ORIGINAL_REQUEST §R4 |
| 13 | Sidebar Peer Media Display | Display filename and formatted duration `HH:MM:SS` or `[⏳ No file loaded]` | M4 | ORIGINAL_REQUEST §R2 |
| 14 | File Match Diagnostics Badge | Compare duration (±2s) / filename: `[Match ✔]` vs `[⚠️ Mismatch]` | M4 | ORIGINAL_REQUEST §R2 |
| 15 | "Ready to Watch" Toggle & Badges | Interactive checkbox defaulting to ready on media load; `[✓ Ready]` vs `[⏳ Not Ready]` | M4 | ORIGINAL_REQUEST §R3 |
| 16 | Strict Playback Unpause Gate | Hold paused if unready/unloaded peers exist; on-screen alert + chat announcement | M4 | ORIGINAL_REQUEST §R3 |
| 17 | Host "Force Play" Bypass | Host override button/toggle to allow unpausing regardless of peer readiness | M4 | ORIGINAL_REQUEST §R3 |
| 18 | 72-Byte Snapshot Invariant | Preserve `PlayerUiSnapshot` memory layout at exactly 72 bytes | M4 | Survey Explorer 2 |
| 19 | E2E Testing Suite (Tiers 1-4) | Comprehensive opaque-box test suite published via `TEST_READY.md` | M5 | Project Pattern |
| 20 | Dependency & Clippy Cleanliness | Zero banned crates (`audit_dependencies.sh`), `#![deny(warnings)]`, clippy -D warnings | M5 | ORIGINAL_REQUEST §R5 |
| 21 | Clean Git Commit & Push | Commit all changes cleanly and push to `origin/main` | M5 | ORIGINAL_REQUEST §R5 |

---

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| M1 | Auto-Update Detection & Notification | Async background updater querying GitHub releases API, semver comparison, UI notification banner, and Help menu entry | none | DONE |
| M2 | Wire Protocol & Peer State Tracking | UDP wire packet `PeerStatus` (0x09) for media metadata & readiness, `ClientSession` decoupling, and negative test updates | none | DONE |
| M3 | Room Activity Announcements | Standardized room activity chat announcements (5 event types), seek debouncing, UDP relay, binary log storage, and FTS indexing | M2 | DONE |
| M4 | Peer Media, Diagnostics & Unpause Gate UI | Sidebar media display, match badge (`[Match ✔]`/`[⚠️ Mismatch]`), readiness toggle, strict unpause gate in `playback_runtime`, Host Force Play, and 72-byte snapshot preservation | M2 | DONE |
| M5 | E2E Testing, Final Verification & Push | Opaque-box E2E test suite (Tiers 1-4), adversarial test hardening, dependency audit, cargo check/test, clippy -D warnings, and git push | M1, M3, M4 | DONE |

---

## Interface Contracts

### 1. `syncplay-sync` Wire Protocol (`crates/syncplay-sync/src/wire.rs`)
```rust
pub const PACKET_TYPE_PEER_STATUS: u8 = 0x09;

#[derive(Debug, Clone, PartialEq, Eq)]
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

### 2. `syncplay-ui` Events & Commands (`crates/syncplay-ui/src/`)
```rust
// In crates/syncplay-ui/src/event.rs
pub enum UiEvent {
    NewChatRecord(ChatRecord),
    RoomMembersUpdated(Vec<RoomMember>),
    UnpauseBlocked { unready_peers: Vec<String>, message: String },
    UpdateAvailable { latest_version: String, release_url: String, download_url: Option<String> },
    UpdateChecked { is_latest: bool, current_version: String },
}

// In crates/syncplay-ui/src/command.rs
pub enum PlayerCommand {
    // Existing...
    SetReady(bool),
    SetForcePlay(bool),
    ForcePlay,
    CheckForUpdates,
}

// In crates/syncplay-ui/src/gui/sidebar.rs
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

### 3. `syncplay-chat` Room Announcements (`crates/syncplay-chat/src/announcements.rs`)
```rust
pub struct RoomAnnouncement;
impl RoomAnnouncement {
    pub fn media_loaded(user: &str, filename: &str, duration_hms: &str) -> String;
    pub fn ready(user: &str) -> String;
    pub fn not_ready(user: &str) -> String;
    pub fn paused(user: &str) -> String;
    pub fn resumed(user: &str) -> String;
    pub fn seeked(user: &str, timecode: &str) -> String;
    pub fn gate_waiting(users: &[String]) -> String;
}
```

---

## Code Layout
- `crates/syncplay-sync/src/wire.rs`: Wire packet definitions, `PeerStatusPayload`, (de)serialization.
- `crates/syncplay-sync/src/transport.rs`: `ClientSession` state, `HostServer` and `ClientNode` peer status handling.
- `crates/syncplay-sync/tests/`: Integration tests, updated negative opcode assertions.
- `crates/syncplay-ui/src/gui/updater.rs`: Asynchronous GitHub releases checker, semver comparison, browser launcher.
- `crates/syncplay-ui/src/gui/sidebar.rs`: Room members rendering, media info, match badges, readiness checkbox.
- `crates/syncplay-ui/src/gui/app.rs`: Top update banner, Help menu item, event draining.
- `crates/syncplay-ui/src/snapshot.rs`: 72-byte `PlayerUiSnapshot` layout and bitflag helpers.
- `crates/syncplay-chat/src/announcements.rs`: Standardized announcement formatting.
- `crates/syncplay-core/src/playback_runtime.rs`: Unpause gate logic, force play bypass, room event hooks.
- `tests/e2e/`: Opaque-box E2E test suite.
