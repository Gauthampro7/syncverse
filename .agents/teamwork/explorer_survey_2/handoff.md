# Phase 5 Survey Handoff Report: UI Sidebar, Playback Controls, & Unpause Gate

**Author**: `explorer_survey_2`  
**Date**: 2026-10-26T11:23:00Z  
**Target Subsystems**: `crates/syncplay-ui`, `crates/syncplay-core`, `crates/syncplay-clock`  
**Milestone**: Phase 5 Survey (Social Watch-Party Polish, Auto-Update, & Unpause Gate)

---

## 1. Observation

### 1.1 `PlayerUiSnapshot` Definition and Lock-Free Triple Buffer Exchange
- **File**: `crates/syncplay-ui/src/snapshot.rs` (lines 12–37):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
  #[repr(C)]
  pub struct PlayerUiSnapshot {
      pub monotonic_timestamp: u64,
      pub media_position_pts: u64,
      pub media_duration_pts: u64,
      pub playback_state: u64,
      pub network_rtt_nanos: u64,
      pub clock_offset_nanos: i64,
      pub phase_error_nanos: i64,
      pub pll_speed_ratio_q16: u32,
      pub peer_count: u16,
      pub peers_ready: u16,
      pub room_sequence: u64,
  }
  ```
- **Size Assertion**: `crates/syncplay-ui/src/snapshot.rs` (lines 175–177):
  ```rust
  let size = std::mem::size_of::<PlayerUiSnapshot>();
  assert_eq!(size, 72, "PlayerUiSnapshot size must be exactly 72 bytes");
  assert!(size <= 128, "PlayerUiSnapshot must fit in 128-byte cache line pair");
  ```
- **Python Intent Harness Verification**: `tests/empirical_challenge/test_ui_renderer.py` (lines 115–118):
  ```python
  struct_size = 8 + 8 + 8 + 8 + 8 + 8 + 8 + 4 + 2 + 2 + 8 # 72 bytes
  assert struct_size <= 128, f"PlayerUiSnapshot size {struct_size}B exceeds 128B limit"
  ```
- **Current Snapshot Publication**: `crates/syncplay-core/src/playback_runtime.rs` (lines 812–826):
  `run_playback_worker` publishes `PlayerUiSnapshot` at ~40 Hz. Notice that `peers_ready` is currently hardcoded to `peer_count`:
  ```rust
  let snap = PlayerUiSnapshot {
      monotonic_timestamp: now.elapsed().as_nanos() as u64,
      media_position_pts: (current_pts_ms.max(0.0) * 1_000_000.0) as u64,
      media_duration_pts: dur_ms * 1_000_000,
      playback_state: if is_playing { 1 } else { 0 },
      network_rtt_nanos: rtt_nanos,
      clock_offset_nanos: offset_nanos,
      phase_error_nanos: phase_err_nanos,
      pll_speed_ratio_q16: pll_ratio,
      peer_count,
      peers_ready: peer_count, // Currently identical to peer_count
      room_sequence: 1,
  };
  exchange.publish(snap);
  ```

### 1.2 Room Members Representation & Delivery to egui
- **File**: `crates/syncplay-ui/src/gui/sidebar.rs` (lines 17–27):
  ```rust
  #[derive(Debug, Clone, PartialEq)]
  pub struct RoomMember {
      pub username: String,
      pub is_local: bool,
      pub is_host: bool,
      pub rtt_ms: f64,
  }
  ```
  `RoomMember` does NOT currently track:
  - Loaded file name (`file_name: Option<String>`)
  - Formatted duration or duration in ms (`duration_ms: Option<u64>`)
  - File size in bytes (`file_size_bytes: Option<u64>`)
  - User readiness state (`is_ready: bool`)
- **Event Delivery Channel**: `crates/syncplay-ui/src/event.rs` (lines 7–13):
  ```rust
  #[derive(Debug, Clone)]
  pub enum UiEvent {
      NewChatRecord(ChatRecord),
      RoomMembersUpdated(Vec<RoomMember>),
  }
  ```
- **Event Consumption in egui**: `crates/syncplay-ui/src/gui/app.rs` (lines 101–111):
  ```rust
  while let Ok(event) = self.event_rx.try_recv() {
      match event {
          UiEvent::NewChatRecord(record) => {
              self.chat_records.push(record);
              self.sidebar_state.autoscroll = true;
          }
          UiEvent::RoomMembersUpdated(members) => {
              self.sidebar_state.members = members;
          }
      }
  }
  ```

### 1.3 Sidebar Rendering of Members List
- **File**: `crates/syncplay-ui/src/gui/sidebar.rs` (lines 290–314):
  ```rust
  fn render_members_list(&self, ui: &mut Ui) {
      ui.collapsing(format!("Members ({})", self.members.len()), |ui| {
          for member in &self.members {
              ui.horizontal(|ui| {
                  if member.is_host {
                      ui.label(egui::RichText::new("★").color(Color32::from_rgb(250, 204, 21)));
                  } else {
                      ui.label("👤");
                  }
                  if member.is_local {
                      ui.label(egui::RichText::new(format!("{} (You)", member.username)).strong().color(Color32::from_rgb(96, 165, 250)));
                  } else {
                      ui.label(&member.username);
                  }
                  if member.rtt_ms > 0.0 {
                      ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                          ui.label(egui::RichText::new(format!("{:.0}ms", member.rtt_ms)).small().color(Color32::GRAY));
                      });
                  }
              });
          }
      });
  }
  ```
  Missing:
  - Local "Ready to Watch" toggle checkbox.
  - Per-member readiness badges (`[✓ Ready]` vs `[⏳ Not Ready]`).
  - Loaded media filename and formatted `HH:MM:SS` duration (or `[⏳ No file loaded]`).
  - Diagnostic comparison badges (`[Match ✔]` vs `[⚠️ Mismatch]`).
  - Host "Force Play" bypass toggle.

### 1.4 Playback Unpause Handling in `playback_runtime.rs`
- **File**: `crates/syncplay-core/src/playback_runtime.rs` (lines 239–254):
  ```rust
  PlayerCommand::TogglePause => {
      let now = Instant::now();
      let will_play = !clock.is_playing();
      if will_play {
          clock.set_state(now, PlaybackState::Playing);
          let _ = audio_tx.send(AudioWorkerCommand::Resume);
      } else {
          clock.set_state(now, PlaybackState::Paused);
          let _ = audio_tx.send(AudioWorkerCommand::Pause);
      }
      if let WatchPartyRole::Host { server, .. } = &mut party_role {
          let current_pts = clock.current_pts(now).max(0.0) as u64;
          server.set_media_state(current_pts, speed, !will_play);
          let _ = server.broadcast_room_state();
      }
  }
  ```
  Observation: Playback directly transitions to `PlaybackState::Playing` with zero unpause gate validation. It does not check whether connected peers have loaded media or are in the `Ready` state.

### 1.5 Help Menu and Update Checking
- **File**: `crates/syncplay-ui/src/gui/app.rs` (lines 394–405):
  ```rust
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
  Observation: No "Check for Updates..." entry exists under the Help menu. No update checker or update notification banner is currently implemented in `syncplay-ui`.

---

## 2. Logic Chain

### 2.1 Preserving the 72-Byte `PlayerUiSnapshot` Invariant vs Dynamic Peer Data
1. `PlayerUiSnapshot` is stored directly inside `TripleBufferSnapshotExchange` as an unboxed array `[UnsafeCell<PlayerUiSnapshot>; 3]` and copied atomically with single-pointer-width or unrolled SIMD loads/stores.
2. If `PlayerUiSnapshot` contained heap pointers (`String`, `Vec<RoomMember>`), the atomic copy `*self.buffers[write_slot].get() = snapshot;` would duplicate raw heap pointers, causing double-frees and undefined behavior.
3. Furthermore, `assert_eq!(size, 72)` is an explicit compile-time/unit test check in `snapshot.rs:176`, and `test_ui_renderer.py:116` asserts `struct_size = 72`.
4. Therefore, dynamic peer media metadata (filenames, durations, readiness) MUST NOT be placed directly inside `PlayerUiSnapshot`.
5. Instead, the architectural division of responsibilities is:
   - **Atomic Telemetry (`PlayerUiSnapshot`, 72 bytes)**: Polled at 60 Hz by egui. Uses the upper bits of `playback_state: u64` (bits 8..15) as atomic flags (`FLAG_GATE_BLOCKED`, `FLAG_FORCE_PLAY`, `FLAG_LOCAL_READY`, `FLAG_HAS_MISMATCH`, `FLAG_UPDATE_AVAILABLE`) while `peers_ready` and `peer_count` reflect exact ready/total peer counts.
   - **Rich UI Event Stream (`UiEvent`)**: Delivered via `mpsc::channel`. Conveys `UiEvent::RoomMembersUpdated(Vec<RoomMember>)` whenever room peers, filenames, durations, or readiness states change, and `UiEvent::UnpauseBlocked { unready_peers, message }` when an unpause attempt is rejected by the gate.

### 2.2 Unpause Gate Enforcement Logic
1. When unpause is triggered (via Spacebar, bottom control bar Play button, video viewport click, or Playback menu), `PlayerCommand::TogglePause` is received by `run_playback_worker`.
2. When transitioning from `Paused` to `Playing`:
   - If user is not in a watch party (`party_role == None`), proceed immediately.
   - If user is in a watch party (`Host` or `Client`):
     - Check if `force_play` override is active. If active, bypass the gate and resume.
     - Otherwise, inspect all connected peers AND local user:
       - Is local media loaded and `local_is_ready == true`?
       - Are all remote peers loaded with media (`file_name.is_some()`) and `is_ready == true`?
     - If ANY peer is unready or has no media loaded:
       - Keep playback paused (`clock.set_state(now, PlaybackState::Paused)`).
       - Maintain audio sink paused (`audio_tx.send(AudioWorkerCommand::Pause)`).
       - Format alert string: e.g. `"⏸ Paused: Waiting for [Username(s)] to load media and get ready..."`.
       - Send `UiEvent::UnpauseBlocked` to UI event channel so an on-screen toast/banner appears.
       - Broadcast a system announcement to room chat via UDP so all peers see why playback was halted.
3. If all peers are ready or Host activates "Force Play":
   - Resume playback (`clock.set_state(now, PlaybackState::Playing)`).
   - Resume audio sink (`audio_tx.send(AudioWorkerCommand::Resume)`).
   - Broadcast room announcement: `"▶ {user} resumed playback"`.

### 2.3 Sidebar Member Matching & Readiness UI Diagnostics
1. **Local Readiness**:
   - `SidebarState` maintains `local_ready: bool`.
   - When a media file is loaded (`load_file` / `OpenFile`), `local_ready` defaults to `true`.
   - Render an interactive checkbox: `ui.checkbox(&mut self.local_ready, "Ready to Watch")`.
   - When toggled, sends `PlayerCommand::SetReady(self.local_ready)` to coordinator.
2. **Peer Media & Duration**:
   - For each `member` in `self.members`:
     - If `member.file_name.is_none()`: display `[⏳ No file loaded]`.
     - If `member.file_name.is_some()`: display `📄 {file_name} ({formatted_duration})` where `formatted_duration` is computed via `format_duration_hms(ms)` -> `HH:MM:SS`.
3. **Diagnostic Matching Badge**:
   - Compare peer's file against local file:
     - Match condition: `(local_duration_ms as i64 - peer_duration_ms as i64).abs() <= 2000` (within ±2.0 seconds) OR `local_filename.eq_ignore_ascii_case(peer_filename)`.
     - If matched: render `[Match ✔]` in green (`Color32::from_rgb(34, 197, 94)`).
     - If mismatched: render `[⚠️ Mismatch]` in red/amber (`Color32::from_rgb(239, 68, 68)`).
4. **Readiness Badge**:
   - If `member.is_ready`: render `[✓ Ready]` in green (`Color32::from_rgb(34, 197, 94)`).
   - If `!member.is_ready`: render `[⏳ Not Ready]` in amber (`Color32::from_rgb(245, 158, 11)`).
5. **Host Bypass**:
   - If local user is host (`is_host == true`), display `⚡ Force Play (Bypass Gate)` toggle in sidebar and inside blocked unpause on-screen alerts.

### 2.4 Auto-Update Checker & Help Menu Integration
1. **Endpoint**: GitHub Releases API: `https://api.github.com/repos/Gauthampro7/syncplay-desktop-preview/releases/latest`.
2. **Zero-Banned-Dependency Compliant Strategy**:
   - System `curl` (/usr/bin/curl) invoked asynchronously in a detached background thread via `std::process::Command::new("curl").args(["-s", "-m", "5", "-H", "User-Agent: syncplay-alt", ...])`.
   - Zero external monolithic player or database crates involved. Passes `./scripts/audit_dependencies.sh`.
   - Gracefully handles non-zero exit codes, HTTP 403 rate limits, network timeouts, or offline states by silently logging and discarding without blocking UI rendering or audio pacing.
3. **Semver Comparison**:
   - Normalizes tags by stripping `'v'`/`'V'`: e.g. `"v0.1.2"` -> `(0, 1, 2)`.
   - Compares against `CARGO_PKG_VERSION` (currently `0.1.1` -> `(0, 1, 1)`).
   - If remote > current, dispatches `UiEvent::UpdateAvailable { latest_version, release_url }`.
4. **Persistent Banner**:
   - Rendered docked at the top of the window (`egui::Panel::top("update_banner")`).
   - Displays: `"🚀 New version available: v{latest_version}"`.
   - Action button: `"⬇ Download & Install"`, opening `release_url` in default browser via `ui.ctx().open_url(OpenUrl { url, new_tab: true })`.
   - Dismiss button: `"✖ Dismiss"`, hiding banner for the remainder of the session.
5. **Help Menu**:
   - Adds `"🔄 Check for Updates..."` under the Help menu dropdown.
   - When clicked, triggers immediate asynchronous check and provides user feedback (either banners the update or shows toast: "You're up to date!").

---

## 3. Caveats

1. **No External Database Engine Mandate**: All chat announcements and room history must continue to use `syncplay_chat.bin` and in-memory structures. No SQLite or document store may be introduced.
2. **72-Byte `PlayerUiSnapshot` Invariant**: The memory size must remain exactly 72 bytes. Any changes expanding this struct would break `test_struct_memory_size` in `snapshot.rs` and the Python intent harness `test_ui_renderer.py`. Upper bits in `playback_state` must be used for flags.
3. **Headless & Terminal Modes**: While this survey targets the graphical `syncplay-ui` application, `InteractivePlayer` in `syncplay-core/src/interactive.rs` should also reject unpausing if peers are unready, displaying the waiting notification on the terminal HUD.

---

## 4. Conclusion

The existing GUI and runtime architecture in `syncplay-ui` and `syncplay-core` is exceptionally clean, well-decoupled, and directly ready for Phase 5 enhancements. 

The complete design specification is detailed below:

### 4.1 Interface Definitions & Struct Extensions

#### 4.1.1 `crates/syncplay-ui/src/snapshot.rs` (Bitflags & Helpers)
```rust
// Keep PlayerUiSnapshot struct layout EXACTLY 72 bytes.
pub const PLAYBACK_STATE_PAUSED: u64 = 0;
pub const PLAYBACK_STATE_PLAYING: u64 = 1;
pub const PLAYBACK_STATE_SEEKING: u64 = 2;
pub const PLAYBACK_STATE_BUFFERING: u64 = 3;

pub const SNAPSHOT_FLAG_GATE_BLOCKED: u64 = 0x0100;
pub const SNAPSHOT_FLAG_FORCE_PLAY: u64    = 0x0200;
pub const SNAPSHOT_FLAG_LOCAL_READY: u64   = 0x0400;
pub const SNAPSHOT_FLAG_HAS_MISMATCH: u64  = 0x0800;
pub const SNAPSHOT_FLAG_UPDATE_AVAIL: u64  = 0x1000;

impl PlayerUiSnapshot {
    #[inline]
    pub fn is_playing(&self) -> bool {
        (self.playback_state & 0xFF) == PLAYBACK_STATE_PLAYING
    }
    #[inline]
    pub fn is_gate_blocked(&self) -> bool {
        (self.playback_state & SNAPSHOT_FLAG_GATE_BLOCKED) != 0
    }
    #[inline]
    pub fn is_force_play(&self) -> bool {
        (self.playback_state & SNAPSHOT_FLAG_FORCE_PLAY) != 0
    }
    #[inline]
    pub fn is_local_ready(&self) -> bool {
        (self.playback_state & SNAPSHOT_FLAG_LOCAL_READY) != 0
    }
    #[inline]
    pub fn has_media_mismatch(&self) -> bool {
        (self.playback_state & SNAPSHOT_FLAG_HAS_MISMATCH) != 0
    }
}
```

#### 4.1.2 `crates/syncplay-ui/src/gui/sidebar.rs` (`RoomMember` Extension)
```rust
/// Room member in active watch-party session with media & readiness metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomMember {
    pub username: String,
    pub is_local: bool,
    pub is_host: bool,
    pub rtt_ms: f64,
    /// Loaded media file name (base name without full path). None if no file loaded.
    pub file_name: Option<String>,
    /// Media duration in milliseconds. None if no file loaded.
    pub duration_ms: Option<u64>,
    /// File size in bytes. None if no file loaded.
    pub file_size_bytes: Option<u64>,
    /// Readiness state ("Ready to Watch" vs "Not Ready").
    pub is_ready: bool,
}

impl RoomMember {
    pub fn new(username: String, is_local: bool, is_host: bool, rtt_ms: f64) -> Self {
        Self {
            username,
            is_local,
            is_host,
            rtt_ms,
            file_name: None,
            duration_ms: None,
            file_size_bytes: None,
            is_ready: false,
        }
    }
}
```

#### 4.1.3 `crates/syncplay-ui/src/command.rs` (`PlayerCommand` Extension)
```rust
pub enum PlayerCommand {
    // Existing commands...
    TogglePause,
    Seek(u64),
    SetSpeed(f64),
    Stop,
    SetVolume(f32),
    ToggleMute,
    OpenFile(PathBuf),
    SendChatMessage(String),
    ExecuteSlashCommand(String),
    HostParty { port: u16, room: String, username: String },
    ConnectParty { addr: String, room: String, username: String },
    DisconnectParty,

    // Phase 5 additions:
    /// Set local user readiness state.
    SetReady(bool),
    /// Set host Force Play bypass state.
    SetForcePlay(bool),
    /// Host trigger to bypass unpause gate immediately.
    ForcePlay,
    /// Trigger asynchronous manual update check.
    CheckForUpdates,
}
```

#### 4.1.4 `crates/syncplay-ui/src/event.rs` (`UiEvent` Extension)
```rust
pub enum UiEvent {
    /// New chat message to append to the sidebar chat history.
    NewChatRecord(ChatRecord),
    /// Updated list of watch-party room members.
    RoomMembersUpdated(Vec<RoomMember>),
    /// Playback unpause gate blocked notification with list of blocking usernames.
    UnpauseBlocked {
        unready_peers: Vec<String>,
        message: String,
    },
    /// Newer application release detected from GitHub Releases API.
    UpdateAvailable {
        latest_version: String,
        release_url: String,
    },
    /// Finished manual update check (e.g. up to date).
    UpdateChecked {
        is_latest: bool,
        current_version: String,
    },
}
```

#### 4.1.5 Duration Formatting Helper: `format_duration_hms`
```rust
/// Formats millisecond duration into standard `HH:MM:SS` timecode.
pub fn format_duration_hms(duration_ms: u64) -> String {
    let total_sec = duration_ms / 1000;
    let h = total_sec / 3600;
    let m = (total_sec % 3600) / 60;
    let s = total_sec % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}
```

---

## 5. Verification Method

To verify these implementations independently:

### 5.1 Compilation & Lints
```bash
# 1. Dependency Invariant Audit (Zero banned players / database engines)
./scripts/audit_dependencies.sh

# 2. Workspace Compilation & Deny Warnings Check
cargo check --workspace --all-targets

# 3. Unit & Integration Test Suite
cargo test --workspace -- --nocapture

# 4. Strict Clippy Linter Check
cargo clippy --workspace --all-targets -- -D warnings
```

### 5.2 Pi Framework Intent Verification Harness
```bash
python3 scripts/intent_harness.py
```
Asserts that all 8 architectural invariants (including UI memory ceiling, 72-byte snapshot size, and lock-free triple buffer) pass with 100.0% score.

### 5.3 Automated Unit Tests to Add
1. **Semver Comparison**:
   Verify version comparison logic handles `v0.1.2` > `v0.1.1`, `0.2.0` > `0.1.9`, `v1.0.0` > `0.9.9`, and equal versions.
2. **File Match Diagnostic**:
   Verify duration match within $\pm 2000\text{ms}$ yields `[Match ✔]`, filename match case-insensitively yields `[Match ✔]`, duration delta $> 2000\text{ms}$ with different name yields `[⚠️ Mismatch]`, and missing media yields `[⏳ No file loaded]`.
3. **Unpause Gate State Machine**:
   Verify unpausing in a multi-peer session halts playback when a peer is unready or has no file loaded, emitting `UiEvent::UnpauseBlocked`, and unpausing succeeds immediately when "Force Play" is active or all peers are ready.

---
*End of Phase 5 Survey Handoff Report*
