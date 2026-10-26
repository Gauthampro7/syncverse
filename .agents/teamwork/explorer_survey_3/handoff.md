# Handoff Report: Auto-Update Architecture & Room Activity Announcements

**Author**: `explorer_survey_3`  
**Date**: `2026-10-26T11:22:00Z`  
**Working Directory**: `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_3/`  
**Recipient**: `ad8bd8cc-722a-48fc-97f9-4ff7f3c97d5d` (Parent Orchestrator)  
**Milestone**: Phase 5 Survey — Watch-Party Social & Auto-Update Polish  

---

## 1. Observation

### 1.1 Dependency Invariants & Whitelist Audit
- **Dependency Audit Script**: Executed `./scripts/audit_dependencies.sh`. Output:
  ```
  [INFO] Auditing Cargo.toml manifests for banned monolithic player and database engines...
  [INFO] Auditing Cargo.lock for banned dependencies...
  [PASS] Dependency audit clean (19 banned patterns checked, 0 violations).
  [PASS] Monolithic player wrappers and external database engines are strictly excluded.
  ```
  Quoted from `scripts/audit_dependencies.sh` lines 21–44, the 19 banned patterns are:
  - External players: `"mpv"`, `"mpv-sys"`, `"libmpv"`, `"vlc"`, `"libvlc"`, `"gstreamer"`, `"gstreamer-sys"`.
  - Database engines: `"sqlite"`, `"sqlite3"`, `"rusqlite"`, `"libsqlite3-sys"`, `"sqlx"`, `"sled"`, `"rocksdb"`, `"rocksdb-sys"`, `"heed"`, `"tantivy"`, `"meilisearch"`, `"diesel"`.
  Neither HTTP clients (`ureq`, `minreq`, `reqwest`), TLS crates (`rustls`, `native-tls`), nor serialization crates (`serde_json`, `semver`) are present in `BANNED_PATTERNS`.
- **Existing `Cargo.lock` Inspection**:
  - `semver` is already resolved in `Cargo.lock` at lines 2954–2958:
    ```toml
    [[package]]
    name = "semver"
    version = "1.0.28"
    source = "registry+https://github.com/rust-lang/crates.io-index"
    checksum = "8a7852d02fc848982e0c167ef163aaff9cd91dc640ba85e263cb1ce46fae51cd"
    ```
  - `serde` is already resolved in `Cargo.lock` at line 2967 (`version = "1.0.228"`).
  - Current workspace package version in `Cargo.toml` line 15: `version = "0.1.1"`.
- **Pi Intent Verification Harness**: Executed `python3 scripts/intent_harness.py`. All 8 intents (INTENT-01 through INTENT-08) passed with a 100.0% Intent Compliance Score in 18.8s with 0 drift.

### 1.2 GitHub Releases API Architecture & Specifications
- **Target Endpoint**: `https://api.github.com/repos/Gauthampro7/syncplay-desktop-preview/releases/latest`
- **GitHub API Invariants**:
  - Requires mandatory `User-Agent` header (e.g. `syncplay-alt/0.1.1`). Requests omitting `User-Agent` receive HTTP 403 Forbidden.
  - Rate limiting for unauthenticated IP queries is 60 requests/hour. GitHub returns HTTP 403 or 429 with headers `x-ratelimit-remaining: 0` and `x-ratelimit-reset: <epoch_seconds>`.
  - JSON schema includes `tag_name` (e.g. `"v0.1.2"`), `html_url`, `name`, `body` (markdown release notes), and `assets` array with `name`, `size`, and `browser_download_url`.

### 1.3 UI Integration Points in `syncplay-ui`
- In `crates/syncplay-ui/src/gui/app.rs`:
  - Lines 101–111: `SyncplayApp::ui()` drains background events from `self.event_rx: Receiver<UiEvent>`.
  - Lines 218–415: Top menu bar renders File, Playback, Watch Party, View, and Help menus.
  - Lines 395–404:
    ```rust
    // --- HELP MENU ---
    ui.menu_button("Help", |ui| {
        if ui.button("⌨ Keyboard Shortcuts & Controls (F1)").clicked() {
            ui.close();
            self.shortcuts_dialog.open();
        }
        if ui.button("ℹ About Syncplay-Alt").clicked() {
            ui.close();
            self.about_dialog.open();
        }
    });
    ```
    Currently lacks a `"🔄 Check for Updates..."` item.
  - Top notification banner: Egui provides `egui::TopBottomPanel::top(...)` which can be inserted directly below or above the top menu bar to show a persistent, non-intrusive notification.
- In `crates/syncplay-ui/src/event.rs` lines 6–13:
  ```rust
  #[derive(Debug, Clone)]
  pub enum UiEvent {
      NewChatRecord(ChatRecord),
      RoomMembersUpdated(Vec<RoomMember>),
  }
  ```
  `UiEvent` can be augmented with `UpdateCheckResult(Result<Option<UpdateInfo>, UpdateError>)`.

### 1.4 Chat & System Announcement Pipeline in `syncplay-chat` & `syncplay-sync`
- In `crates/syncplay-chat/src/record.rs`:
  - Lines 20–36: `ChatRecord` contains `record_type: u8`, `flags: u8`, `wall_timestamp_utc_ms: u64`, `media_timestamp_ms: u64`, `sender_id: [u8; 16]`, `sender_name: String`, and `payload: Vec<u8>`.
  - Lines 39–56: `ChatRecord::new_text(wall_ms, media_ms, sender_id, sender_name, text)` creates a text record (`record_type: 1`).
  - Lines 58–87: Framed with `SYNC_WORD = [0xAA, 0x55]`, 4-byte length, body, and CRC32.
- In `crates/syncplay-sync/src/wire.rs`:
  - Lines 38–40: `PacketType::ChatMessage = 0x08`.
  - Lines 127–155: `ChatMessagePayload` carries `sequence: u32`, `wall_timestamp_ms: u64`, `media_timestamp_ms: u64`, and `payload: Vec<u8>`.
- In `crates/syncplay-sync/src/transport.rs`:
  - Lines 249–261: `HostServer` receives `ChatMessage` and relays it to all connected peers except sender.
  - Lines 276–291: `HostServer::broadcast_chat_message` sends chat packets to all registered clients.
  - Lines 637–653: `ClientNode::send_chat_message_with_pts` transmits chat packets to the host.
- In `crates/syncplay-core/src/playback_runtime.rs`:
  - Lines 305–333: User chat messages (`PlayerCommand::SendChatMessage`) construct a `ChatRecord::new_text`, serialize it via `.to_network_bytes()`, broadcast it via `WatchPartyRole`, and deliver it to UI via `event_tx.send(UiEvent::NewChatRecord(rec))`.
  - Lines 358–361 & 430–437: System announcements currently use `sender_id: [0u8; 16]` and `sender_name: "System".to_string()`.
- In `crates/syncplay-ui/src/gui/sidebar.rs`:
  - Lines 341–410: `render_chat_message` displays wall timestamp (`HH:MM:SS`), interactive media timestamp button (`[MM:SS]` seeking on click), sender name, and message payload text.

---

## 2. Logic Chain

### 2.1 Auto-Update Strategy & Whitelist Compliance
1. **Dependency Analysis**:
   - `ORIGINAL_REQUEST.md` (R1) mandates an asynchronous, non-blocking background update checker querying GitHub Releases API.
   - `scripts/audit_dependencies.sh` only bans monolithic player wrappers (`mpv`, `vlc`, `gstreamer`) and database engines (`sqlite`, `diesel`, etc.).
   - Standard pure-Rust HTTP clients are completely permissible. However, minimizing dependency bloat and compile times is an explicit workspace invariant.
   - `reqwest` pulls in `tokio`, `hyper`, and ~80 transitive crates, which is unnecessarily heavy for an occasional background query.
   - `ureq` (version `2.12` with `features = ["tls", "json"]`) or `minreq` (version `2.13` with `features = ["https-rustls", "json-using-serde"]`) provides synchronous blocking HTTP with zero async runtime overhead, negligible compile times (< 5s), and built-in connection timeouts.
   - `semver` is **already in `Cargo.lock`** at version `1.0.28`. Adding `semver = "1.0"` to `crates/syncplay-ui/Cargo.toml` incurs 0 new dependencies.
   - **Verdict**: Use `ureq` (or `minreq`) + `semver = "1.0"` in `crates/syncplay-ui`.

2. **Non-Blocking Background Lifecycle**:
   - To guarantee zero UI stutter and prevent stalling the 60fps presentation loop or audio pacing, network queries must never run on the UI thread.
   - Strategy: Spawn a detached worker thread via `std::thread::Builder::new().name("updater-worker".into()).spawn(move || { ... })`:
     - Triggers on startup after `SyncplayApp::new()`.
     - Triggers on manual user click of `Help -> "🔄 Check for Updates..."`.
     - Enforces a strict 5-second socket read/connect timeout (`ureq::builder().timeout(Duration::from_secs(5))`).
     - Transmits `UiEvent::UpdateCheckResult(result)` to `event_tx` and calls `ctx.request_repaint()`.

3. **Semver Parsing & Tag Normalization**:
   - Remote tags follow `"vX.Y.Z"`, `"vX.Y.Z-beta"`, or `"X.Y.Z"`.
   - Local version is obtained via compile-time macro `env!("CARGO_PKG_VERSION")` (`"0.1.1"`).
   - Normalization function:
     ```rust
     pub fn parse_semver(tag: &str) -> Result<semver::Version, semver::Error> {
         let clean = tag.trim().trim_start_matches(|c| c == 'v' || c == 'V');
         semver::Version::parse(clean)
     }
     ```
   - Comparison: `remote_version > local_version` returns `true` if and only if remote release is strictly newer under SemVer 2.0.0 precedence rules.

4. **Action Handler & Browser Launching**:
   - In `GitHubRelease.assets`, locate OS-specific download artifact:
     - Windows: asset ending in `.exe` or `.msi`.
     - Linux: asset ending in `.tar.gz`, `.rpm`, or `.AppImage`.
     - Fallback: `release.html_url` (the GitHub web release page).
   - To open URLs in the default web browser cross-platform without external crates:
     ```rust
     pub fn open_browser_url(url: &str) -> std::io::Result<()> {
         #[cfg(target_os = "linux")]
         { std::process::Command::new("xdg-open").arg(url).spawn()?; }
         #[cfg(target_os = "windows")]
         { std::process::Command::new("cmd").args(["/c", "start", "", url]).spawn()?; }
         #[cfg(target_os = "macos")]
         { std::process::Command::new("open").arg(url).spawn()?; }
         Ok(())
     }
     ```
     Additionally, signal `ctx.open_url(egui::OpenUrl::new_tab(url))` for native window manager integration.

5. **Rate Limiting & Offline Fault Tolerance**:
   - Offline / Network Down: Return `UpdateError::NetworkOffline`. On startup, fail silently without disrupting the user. On manual check, display `"Unable to connect to GitHub (network unreachable)"`.
   - Rate Limited: Detect HTTP status 403/429. If header `x-ratelimit-remaining` is `"0"`, extract `x-ratelimit-reset` unix timestamp and return `UpdateError::RateLimited { reset_epoch_secs }`. Display `"GitHub API rate limit reached. Resets at HH:MM:SS"`.
   - Timeout: Terminate after 5.0s, returning `UpdateError::Timeout`.

---

### 2.2 Room Activity Announcements Strategy & Format
1. **Announcement Requirements**:
   `ORIGINAL_REQUEST.md` (R4) mandates formatting and broadcasting synchronized chat system announcements for 5 room events:
   - **Media loaded**: `🎬 {user} loaded '{filename}' ({duration})`
   - **Readiness changed**: `✓ {user} is ready to watch` / `⏳ {user} is not ready`
   - **Playback paused**: `⏸ {user} paused playback`
   - **Playback resumed**: `▶ {user} resumed playback`
   - **Seeking**: `⏩ {user} seeked to {timecode}`
   - In addition, R3 specifies unpause gate auto-pause: `⏸ Paused: Waiting for [Username(s)] to load media and get ready...`

2. **Integration Architecture**:
   - **Originating Node**: The peer where the event originates constructs the announcement text, sets `sender_name: "System"`, `sender_id: [0u8; 16]`, and `wall_timestamp_utc_ms` to current time.
   - **Wire Protocol**: Broadcasts the message over UDP using existing `PacketType::ChatMessage` (`0x08`).
   - **Host Relay**: The `HostServer` receives the packet and automatically relays it to all other peers in the room (`transport.rs` lines 251–260).
   - **Storage & Indexing**: Every receiving node (and the originating node) records the announcement in `syncplay_chat.bin` with `0xAA55` framing and indexes it in the in-memory inverted index FTS.
   - **UI Presentation**: `SyncplayApp` receives `UiEvent::NewChatRecord`, pushes to `chat_records`, and displays it in `SidebarState::render_chat_message`.

3. **Debounce Defense Against Chat Flooding**:
   - Continuous seekbar scrubbing emits 10Hz debounced seeks (Intent-05).
   - **Rule**: Seeking announcements MUST be emitted ONLY upon seek completion (scrubber thumb release or discrete keyboard step `←`/`→`), NEVER during intermediate dragging ticks.
   - Rapid play/pause toggles within 200ms should be suppressed or rate-limited to avoid notification spam.

---

## 3. Caveats

1. **Private GitHub Repositories**: If `Gauthampro7/syncplay-desktop-preview` is a private repository, unauthenticated calls to GitHub Releases API will return HTTP 404. The updater handles 404 cleanly by reporting "No releases found" without crashing or failing tests.
2. **Local Playback Mode (Solo)**: When playing media offline with no watch party connected, room activity announcements (pause/resume/seek) should not clutter chat; on-screen HUD glyphs (`TransportGlyphKind`) provide local feedback. File loaded announcements may be logged to the chat log for timeline reference.
3. **HTTP Proxy / Strict Firewalls**: In air-gapped or proxy-restricted networks, `ureq` will timeout after 5 seconds and report offline status gracefully.

---

## 4. Conclusion & Concrete Design Proposals

### 4.1 Auto-Updater Module Architecture (`crates/syncplay-ui/src/gui/updater.rs`)

```rust
//! Asynchronous GitHub Releases auto-update checker and release launcher.

use std::sync::mpsc::Sender;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Target repository release API URL.
pub const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/Gauthampro7/syncplay-desktop-preview/releases/latest";

/// Strongly typed update error enum conforming to AGENTS.md Section 4.
#[derive(Debug, Error, Clone)]
pub enum UpdateError {
    #[error("Network connection offline or unreachable: {0}")]
    NetworkOffline(String),
    #[error("Update check timed out after 5.0 seconds")]
    Timeout,
    #[error("GitHub API rate limit exceeded (resets at {reset_time})")]
    RateLimited { reset_time: String },
    #[error("Failed to parse release metadata: {0}")]
    ParseError(String),
    #[error("No newer release found")]
    NoUpdateAvailable,
}

/// Information describing an available remote update.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
    pub download_url: Option<String>,
    pub release_notes: Option<String>,
}

/// Spawns a background thread to check for updates asynchronously.
pub fn spawn_update_check(
    current_version: &'static str,
    event_tx: Sender<crate::event::UiEvent>,
    ctx: egui::Context,
) {
    std::thread::Builder::new()
        .name("syncplay-updater".into())
        .spawn(move || {
            let res = check_for_updates(current_version);
            let _ = event_tx.send(crate::event::UiEvent::UpdateCheckResult(res));
            ctx.request_repaint();
        })
        .expect("failed to spawn updater thread");
}
```

### 4.2 Room Activity Announcement Templates (`crates/syncplay-chat/src/announcements.rs`)

```rust
//! Standardized room activity announcements conforming to ORIGINAL_REQUEST.md R4.

/// Formats key room event announcements with standardized emojis and syntax.
pub struct RoomAnnouncement;

impl RoomAnnouncement {
    /// 🎬 {user} loaded '{filename}' ({duration})
    pub fn media_loaded(user: &str, filename: &str, duration_hms: &str) -> String {
        format!("🎬 {} loaded '{}' ({})", user, filename, duration_hms)
    }

    /// ✓ {user} is ready to watch
    pub fn ready(user: &str) -> String {
        format!("✓ {} is ready to watch", user)
    }

    /// ⏳ {user} is not ready
    pub fn not_ready(user: &str) -> String {
        format!("⏳ {} is not ready", user)
    }

    /// ⏸ {user} paused playback
    pub fn paused(user: &str) -> String {
        format!("⏸ {} paused playback", user)
    }

    /// ▶ {user} resumed playback
    pub fn resumed(user: &str) -> String {
        format!("▶ {} resumed playback", user)
    }

    /// ⏩ {user} seeked to {timecode}
    pub fn seeked(user: &str, timecode: &str) -> String {
        format!("⏩ {} seeked to {}", user, timecode)
    }

    /// ⏸ Paused: Waiting for [Username(s)] to load media and get ready...
    pub fn gate_waiting(users: &[String]) -> String {
        format!("⏸ Paused: Waiting for {} to load media and get ready...", users.join(", "))
    }
}
```

### 4.3 Notification Banner Rendering in `SyncplayApp` (`crates/syncplay-ui/src/gui/app.rs`)

```rust
// Render persistent top notification banner if update is available and not dismissed
if let Some(ref update) = self.update_info {
    if !self.is_update_banner_dismissed {
        egui::TopBottomPanel::top("update_notification_banner")
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(15, 23, 42)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("🚀 Update Available:").strong().color(egui::Color32::from_rgb(56, 189, 248)));
                    ui.label(format!("v{} is available (current: v{}).", update.latest_version, update.current_version));
                    
                    if ui.button(egui::RichText::new("📥 Download").strong()).clicked() {
                        let target = update.download_url.as_deref().unwrap_or(&update.release_url);
                        let _ = crate::gui::updater::open_browser_url(target);
                        ui.ctx().open_url(egui::OpenUrl::new_tab(target));
                    }
                    if ui.button("Release Page").clicked() {
                        let _ = crate::gui::updater::open_browser_url(&update.release_url);
                        ui.ctx().open_url(egui::OpenUrl::new_tab(&update.release_url));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✖ Dismiss").clicked() {
                            self.is_update_banner_dismissed = true;
                        }
                    });
                });
            });
    }
}
```

---

## 5. Verification Method

### 5.1 Verification Commands
The implementation must be verified using the standard test and lint commands:

1. **Dependency Blacklist Audit**:
   ```bash
   ./scripts/audit_dependencies.sh
   ```
   *Expected Outcome*: Exit code 0, 0 violations. Ensures no banned player or DB engines.

2. **Workspace Compilation & Zero Compiler Warnings**:
   ```bash
   cargo check --workspace --all-targets
   ```
   *Expected Outcome*: Exit code 0, 0 compiler warnings.

3. **Strict Clippy Linter Check**:
   ```bash
   cargo clippy --workspace --all-targets -- -D warnings
   ```
   *Expected Outcome*: Exit code 0, 0 warnings.

4. **Unit & Integration Test Suite**:
   ```bash
   cargo test --workspace -- --nocapture
   ```
   *Expected Outcome*: 100% pass across all existing and new unit tests (testing semver comparisons, announcement formatting, and binary serialization).

5. **Pi Framework Intent Verification Harness**:
   ```bash
   python3 scripts/intent_harness.py
   ```
   *Expected Outcome*: 100.0% Intent Compliance Score across all 8 intents.

### 5.2 Unit Test Matrix to Implement
- `test_semver_tag_normalization`: Validates `"v0.1.2" > "v0.1.1"`, `"0.2.0" > "0.1.9"`, `"v0.1.1" == "0.1.1"`, rejection of invalid tags.
- `test_room_announcement_formatting`: Validates exact string formatting matching the 5 required templates.
- `test_room_announcement_binary_persistence`: Appends formatted announcement to `syncplay_chat.bin`, verifies `0xAA55` framing, CRC32, and recovers record with `sender_name == "System"`.
- `test_room_announcement_fts_search`: Inverted index indexes words like `"loaded"`, `"ready"`, `"paused"`, `"resumed"`, `"seeked"` and successfully retrieves them.
- `test_rate_limit_detection`: Simulates HTTP 403 response with `x-ratelimit-remaining: 0` and verifies `UpdateError::RateLimited` parsing.
- `test_offline_graceful_handling`: Simulates DNS/socket connection failure and verifies non-fatal `UpdateError::NetworkOffline`.

### 5.3 Invalidation Conditions
- Any introduction of banned crates (`mpv`, `vlc`, `sqlite`, etc.) in `Cargo.toml` or `Cargo.lock`.
- Any blocking HTTP calls on the UI thread or audio thread exceeding 1ms.
- Any chat announcement spam during continuous seekbar drag scrubbing.
- Any unhandled `unwrap()` panic in updater or announcement parsing.
