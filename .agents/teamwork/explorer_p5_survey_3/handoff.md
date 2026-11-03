# Technical Audit & Architectural Investigation Report: UI, Chat, and Coordinator Subsystems

- **Author**: `explorer_p5_survey_3` (Teamwork Explorer)
- **Target Subsystems**: `crates/syncverse-ui/`, `crates/syncverse-chat/`, `crates/syncverse-core/`
- **Date**: 2026-10-30
- **Status**: Complete Investigation & Improvement Proposal

---

## Executive Summary

A comprehensive, zero-modification technical investigation was conducted across the User Interface (`syncverse-ui`), Chat & Persistence (`syncverse-chat`), and Coordinator (`syncverse-core`) crates of the Syncverse workspace. The investigation evaluated desktop windowing lifecycle, GPU video viewport pacing, controls bar ergonomics, watch-party sidebar interactivity, binary log resilience, inverted index full-text search, coordinator event loops, audio/video synchronization, and the strict 72-byte `PlayerUiSnapshot` architectural invariant.

The existing codebase exhibits exemplary engineering rigor:
1. Strict adherence to zero banned monolithic player wrappers (`mpv`, `vlc`) and zero external database daemons (`sqlite`, `rocksdb`).
2. Lock-free SPSC single-producer single-consumer triple-buffering (`TripleBufferSnapshotExchange`) preserving the strict 72-byte `PlayerUiSnapshot` memory layout within a 128-byte cache-line pair, eliminating torn reads.
3. Crash-resilient binary logging (`syncplay_chat.bin`) with `0xAA55` framing, forward scanning, and byte-by-byte false-sync recovery.
4. Pure-Rust in-memory inverted index FTS with Unicode CJK unigram/bigram tokenization and timeline spatial querying.
5. Smooth audio playback with micro-fade-in/fade-out transitions eliminating DC pops during seeks and track switches.

However, concrete feature gaps and usability enhancements were identified that will elevate Syncverse to a premier desktop media player without compromising its high-efficiency performance profile or violating architectural invariants.

---

## 1. Observation: Empirical Codebase Analysis

### 1.1 User Interface Subsystem (`crates/syncverse-ui/`)

#### 1. Desktop Window Management & Viewport
- **Source**: `crates/syncverse-ui/src/gui/app.rs:1970-2016` and `crates/syncverse-ui/src/gui/viewport.rs:1-350`
- **Window Initialization**: Implemented via `eframe::run_native` configured with `ViewportBuilder`:
  - Default size: `[1280.0, 720.0]`, minimum size: `[640.0, 360.0]`, resizable: `true`, drag-and-drop: `true`.
  - Linux Wayland/X11 application ID set to `"syncverse"`.
  - Dynamic window title synchronized to current media filename via `ctx.send_viewport_cmd(ViewportCommand::Title(...))`.
- **GPU Video Viewport Rendering**:
  - Direct rendering via `egui::ColorImage` texture allocation (`VideoViewport::update_playback`).
  - Frames pulled wait-free from `SpscRingBuffer<VideoFrameRGBA, 32>` synchronized against `MasterClock::current_pts(now)`.
  - Frame pacing ensures vsync smoothness: frames arriving ahead of presentation clock are held; frames within presentation window ($\Delta \in [-50\text{ms}, +25\text{ms}]$) are presented; late frames are dropped to prevent accumulation.
- **Aspect Ratio Preservation**:
  - `AspectRatioOverride` presets: `Original (Fit)`, `16:9`, `4:3`, `21:9`, `1:1`, `Fill / Stretch` (`gui/aspect_ratio.rs:1-120`).
  - Geometry calculation in `compute_video_rect` guarantees letterboxing or pillarboxing with zero distortion across standard and ultrawide viewports (16:9, 4:3, 2.39:1, 21:9, 32:9) and guards against division-by-zero on non-positive or 0x0 geometries.

#### 2. Controls Bar & Scrubber Dynamics
- **Source**: `crates/syncverse-ui/src/gui/controls.rs:1-878`
- **Styling**: Sleek dark aesthetic palette (`BAR_BACKGROUND = #141417`, `BAR_BORDER = #27272A`, `PLAY_ACCENT_BG = #0EA5E9`).
- **Transport Controls**:
  - Play/Pause toggle with sky-blue accent fill when paused (`controls.rs:487-509`).
  - Stop button resetting presentation to `00:00:00` (`controls.rs:511-527`).
  - Chapter navigation: Previous (`⏮`) and Next (`⏭`) buttons (`controls.rs:529-563`).
  - Monospace timecode HUD: `HH:MM:SS / HH:MM:SS` formatted via `format_timecode_hud` (`controls.rs:565-580`).
- **Interactive Seekbar**:
  - Background track height: 4.0px normal, expands to 8.0px on hover/drag (`controls.rs:331`).
  - Progress fill in `#38BDF8 Sky Blue`; seekhead thumb with `#38BDF8` glow halo.
  - Golden chapter notch tick markers rendered along the seekbar track (`controls.rs:364-378`).
  - Hover preview tooltip: renders `HH:MM:SS — 📌 Chapter Title` floating above cursor (`controls.rs:400-438`).
  - 10 Hz rate-limited seek debouncer (`SeekDebouncer`) during active drag, emitting intermediate seeks at 100ms intervals, followed by an authoritative seek upon mouse release or click-to-seek (`controls.rs:440-472`).
- **Right-Aligned Settings Group**:
  - Volume slider (0.0 to 1.0, 80px width) with mouse wheel scroll adjustment (±5% per notch) and formatted percentage readout (`controls.rs:725-756`).
  - Mute button (`🔇`, `🔉`, `🔊`) with unmuted volume restoration (`controls.rs:757-768`).
  - Playback speed combo box supporting `[0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0]` (`controls.rs:702-721`).
  - Aspect ratio selector combo box (`controls.rs:681-699`).
  - Quick-access dialog triggers: Audio (`🎚`), Subtitles (`CC`), Video (`🎬`).
  - Watch-party sidebar toggle (`💬`), lossless screenshot (`📸`), and fullscreen toggle (`⛶` / `🗗`).

#### 3. Watch-Party Sidebar & Interactivity
- **Source**: `crates/syncverse-ui/src/gui/sidebar.rs:1-1010`
- **Panel Layout**: Collapsible/resizable right docked panel (240px to 450px, default 300px); renders a sleek floating edge handle (`◀ 👥`) when collapsed (`sidebar.rs:443-473`).
- **Sync Diagnostics HUD**:
  - Green (`● SYNCED`), Amber (`● DRIFT`), or Red (`● DESYNC`) health badge based on phase error $e(t)$ and RTT (`sidebar.rs:476-497`).
  - Reports micro-speed steering notation if active.
- **Room Members & Peer Media Matching**:
  - Displays connected members, host crown (`★`), local user tag (`(You)`), and network RTT.
  - Readiness toggle checkbox: `[✓ Ready]` (Green) vs `[⏳ Not Ready]` (Amber) (`sidebar.rs:549-562`).
  - Media match diagnostic badges evaluated via `evaluate_file_match_with_hash` (`sidebar.rs:588-649`):
    - `[Exact Match ✔]`: Identical SHA-256 fingerprint hash.
    - `[Match ✔]`: Duration within ±2000ms or identical basename.
    - `[⚠️ Variation]`: Matching duration but differing encode hash.
    - `[❌ Desync Alert]`: Duration difference > 2000ms.
    - `[⚠️ Mismatch]`: Differing media.
    - `[⏳ No file loaded]`: Peer idle without media.
- **Unpause Gate & Host Force Play**:
  - On-screen foreground alert banner (`UnpauseBlockedBanner`) when playback resumption is blocked by unready/unloaded peers (`app.rs:1814-1867`).
  - Host "⚡ Force Play" bypass trigger in the banner and sidebar checkbox (`sidebar.rs:515-522`).
- **Interactive Dual-Temporal Smart Chat**:
  - Wall timestamp $t_{\text{wall}}$ (`HH:MM:SS`) and media presentation timestamp $t_{\text{media}}$ (`[MM:SS]`).
  - Clickable $t_{\text{media}}$ button: clicking seeks presentation directly to that timestamp (`sidebar.rs:701-721`).
  - Emoji reaction hotbar (`❤️`, `😂`, `🔥`, `👏`, `🍿`, `🎉`) spawning floating particle animations across the video viewport (`sidebar.rs:770-777`, `reactions.rs`).

#### 4. Global Keyboard Shortcuts
- **Source**: `crates/syncverse-ui/src/gui/app.rs:970-1193`
- Current bindings:
  - `Space`: Toggle Play/Pause
  - `S`: Stop playback (resets to 0)
  - `Left` / `Right`: Seek -5s / +5s (`Shift` + `Left` / `Right`: -30s / +30s)
  - `Up` / `Down`: Volume +5% / -5%
  - `M`: Toggle audio mute
  - `[` / `]`: Speed -0.25x / +0.25x; `Backspace`: Reset speed to 1.0x
  - `B`: Cycle audio tracks
  - `V` or `C`: Cycle subtitle tracks (including Off)
  - `A`: Cycle aspect ratio presets
  - `J` / `K`: Audio delay -50ms / +50ms
  - `Z` / `X`: Subtitle delay -50ms / +50ms
  - `1`..`6`: Send floating emoji reaction
  - `PageUp` / `PageDown`: Previous / Next chapter
  - `F8` or `Ctrl+S`: Lossless PNG screenshot to `~/Pictures/Syncverse/`
  - `F11` or `F`: Toggle fullscreen
  - `Tab`: Toggle watch-party sidebar
  - `F1`: Open shortcuts help dialog
  - `Ctrl+O`: Open native file dialog
  - `Ctrl+Q`: Stop and exit application
  - `Escape`: Closes open modals in priority order, then exits fullscreen

#### 5. Triple-Buffered `PlayerUiSnapshot` Architectural Invariant
- **Source**: `crates/syncverse-ui/src/snapshot.rs:9-93` and `snapshot.rs:221-226`
- **Memory Layout**:
  ```rust
  #[repr(C)]
  pub struct PlayerUiSnapshot {
      pub monotonic_timestamp: u64, // 8 bytes
      pub media_position_pts: u64,  // 8 bytes
      pub media_duration_pts: u64,  // 8 bytes
      pub playback_state: u64,      // 8 bytes (State in bits 0..7, Bitflags in bits 8..63)
      pub network_rtt_nanos: u64,   // 8 bytes
      pub clock_offset_nanos: i64,  // 8 bytes
      pub phase_error_nanos: i64,   // 8 bytes
      pub pll_speed_ratio_q16: u32, // 4 bytes
      pub peer_count: u16,          // 2 bytes
      pub peers_ready: u16,         // 2 bytes
      pub room_sequence: u64,       // 8 bytes
  }
  ```
- **Byte Size Calculation**: $7 \times 8 + 4 + 2 + 2 + 8 = 56 + 8 + 8 = 72\text{ bytes}$.
- **Invariant**: `std::mem::size_of::<PlayerUiSnapshot>() == 72` verified by unit test `test_struct_memory_size`. Fits entirely within a 128-byte cache-line pair, eliminating torn reads across cores.
- **Bitflags**: Bits 8..11 of `playback_state` are currently allocated:
  - `SNAPSHOT_FLAG_GATE_BLOCKED = 0x0100`
  - `SNAPSHOT_FLAG_FORCE_PLAY   = 0x0200`
  - `SNAPSHOT_FLAG_LOCAL_READY  = 0x0400`
  - `SNAPSHOT_FLAG_HAS_MISMATCH = 0x0800`
  - **Remaining Free Bits**: 52 bits (bits 12..63) remain unallocated.

---

### 1.2 Chat & Persistence Subsystem (`crates/syncverse-chat/`)

#### 1. Binary Log Storage & Crash Recovery
- **Source**: `crates/syncverse-chat/src/record.rs:1-315` and `crates/syncverse-chat/src/storage.rs:1-250`
- **Header**: 16-byte fixed header: Magic `SPCH` (`0x53504348`), Version 1, Flags, Reserved 0, CRC32 (`storage.rs:55-70`).
- **Record Framing**:
  - Sync word: `0xAA55` (`[0xAA, 0x55]`).
  - Total length: 4 bytes `u32` LE.
  - Body: Record type (1B), Flags (1B), Wall UTC ms (8B), Media PTS ms (8B), Sender UUID (16B), Name length (1B), Sender Name (name_len B), Payload length (4B), Payload (payload_len B).
  - CRC32: 4 bytes table-driven IEEE 802.3 `0xEDB88320` over body slice.
  - Minimum valid record size: 48 bytes.
- **False-Sync Recovery Scanner**:
  - `ChatRecoveryScanner::recover`: Reads bytes linearly; upon finding `0xAA55`, validates length $\ge 48$ and remaining file bounds.
  - Verifies body CRC32 against calculated CRC32.
  - On corruption or false-sync word (e.g. `0xAA55` appearing inside compressed video payload or text): increments offset by 1 byte (`offset += 1`), records `corrupted_chunks += 1`, and continues forward scanning until the next valid record boundary.
  - Verified by `python3 tests/empirical_challenge/test_smart_chat.py` (Test 4: 9/9 valid records salvaged from false-sync corruption).

#### 2. Inverted Index Full-Text Search
- **Source**: `crates/syncverse-chat/src/index.rs:1-250`
- **Specification Tokenization (`tokenize_spec`)**:
  - Unicode case folding (including Turkish dotted I `İ` $\to$ `istanbul`).
  - CJK Segmentation: Unified Ideographs (U+4E00..U+9FFF) tokenized into unigrams (single-character ideograph exemption) and overlapping bigrams (`吉`, `他`, `吉他`).
  - Western Words: Alphanumeric strings, preserving `#hashtags` and `@mentions`, length 2..48 chars.
  - Stop words: English stop words (`the`, `is`, `at`, `which`, etc.) filtered from Western tokens.
- **Search Engine**:
  - Posting lists: `BTreeMap<String, Vec<usize>>`.
  - Multi-term Boolean AND search using linear two-pointer list intersection in $O(|L_1| + |L_2|)$.
  - Spatial timeline queries: `query_timeline_window(current_pts, past_ms, future_ms)` querying `BTreeMap<u64, Vec<usize>>` in $O(\log N + K)$.

#### 3. Slash Commands & Activity Announcements
- **Source**: `crates/syncverse-chat/src/commands.rs:1-460` and `crates/syncverse-chat/src/announcements.rs:1-555`
- Supported commands:
  - `/seek <time>`: Parses `HH:MM:SS`, `MM:SS`, seconds (`123.4`), and relative offsets (`+15s`, `-10s`).
  - `/bookmark [label]`: Creates timeline bookmark at current media PTS.
  - `/clip <start> <end> [label]`: Marks a video segment.
  - `/poll "Question" "Opt1" "Opt2"` (or `/poll Question | Opt1 | Opt2`).
  - `/sync-status`: Generates formatted diagnostics report (`format_sync_status_report`).
- Standardized room activity announcements:
  - Authorship: System UUID `[0u8; 16]`, display name `"System"`.
  - Events: `MediaLoaded`, `Ready`, `NotReady`, `Paused`, `Resumed`, `Seeked`, `GateWaiting`.

---

### 1.3 Coordinator Subsystem (`crates/syncverse-core/`)

#### 1. Playback Coordinator Runtime
- **Source**: `crates/syncverse-core/src/playback_runtime.rs:1-2624`
- Dedicated worker thread (`playback_coordinator_thread`):
  - Manages `UniversalDemuxer`, `UniversalAudioDecoder`, `UniversalVideoDecoder`.
  - Streams normalized f32 audio (48kHz stereo) to dedicated `run_audio_sink_worker` thread running `AutoAudioSink` (PipeWire/ALSA).
  - Implements micro-fade-in/fade-out (256 samples linear ramp) on seek reset, eliminating DC offset pop/crackle (`playback_runtime.rs:83-96`).
  - Decodes video frames into `SpscRingBuffer<VideoFrameRGBA, 32>`.
  - Scans and loads container subtitle tracks and external subtitle files (.srt, .vtt, .ass).
  - Handles Watch-Party networking: UDP client/server packet exchange, ChaCha20-Poly1305 AEAD decryption, UPnP port mapping, NTP clock filtering, and barrier unpausing.
  - Unpause gate evaluation: blocks unpause and alerts room if connected peers have not loaded media or are not ready (`playback_runtime.rs:1046-1122`).
  - Telemetry publication: aggregates subsystem metrics and calls `exchange.publish(snap)` at ~40 Hz (`playback_runtime.rs:2172-2334`).
  - EOF auto-pause: detects `current_pts_ms >= dur_ms`, clamps clock to duration, pauses audio sink, and updates server room state (`playback_runtime.rs:2180-2193`).

---

## 2. Logic Chain: Analysis & Gap Identification

1. **Observation 1 (Keybindings in `app.rs:970-1193`)**:
   - The user interface currently handles Space, S, Arrows, M, Bracket keys, B, V/C, A, J/K, Z/X, 1-6, PageUp/PageDown, F8, F11, F, Tab, F1, Ctrl+O, Ctrl+Q, Escape.
   - **Gap**: Keys `,` (comma) and `.` (period) are completely unhandled. In standard desktop players (VLC, mpv, IINA), `,` and `.` step exactly one frame backward and forward while paused.
   - **Inference**: Frame-stepping is essential for scrutinizing video details, checking subtitle synchronization, and video editing. A `PlayerCommand::StepFrame { forward: bool }` (or direct step seek of $\approx 41.7\text{ms}$ or $1/\text{fps}$) can be bound to `,` and `.` without threading changes.

2. **Observation 2 (Seek increments in `app.rs:990-1012` and menus `app.rs:1377-1406`)**:
   - Arrow keys support -5s / +5s, and Shift+Arrows support -30s / +30s.
   - **Gap**: No coarse jump (e.g. ±60s) or single-frame micro-jump is available via keyboard, and menu items do not provide frame-step controls.
   - **Inference**: Adding Ctrl+Left/Right for ±60s (or customizable seek leaps) provides standard hierarchical seeking (1 frame $\to$ 5s $\to$ 30s $\to$ 60s $\to$ chapter).

3. **Observation 3 (Volume wheel controls in `app.rs:1783-1792` vs `controls.rs:744-755`)**:
   - Viewport scrolling adjusts volume by ±5%. On the controls bar, scrolling adjusts volume only when directly hovering the 80px volume slider.
   - **Gap**: Scrolling mouse wheel over the seekbar or empty space in the controls bar does nothing.
   - **Inference**: Expanding scroll wheel detection to the entire playback controls bar provides a smoother desktop feel.

4. **Observation 4 (Audio Mute State in `PlayerUiSnapshot:14-37`)**:
   - `VolumeControlState` in `controls.rs:16-94` maintains `is_muted`.
   - `PlayerUiSnapshot` has 72 bytes and 52 unused bits in `playback_state: u64`.
   - **Gap**: The snapshot does not encode whether audio is muted.
   - **Inference**: Defining bitflag `SNAPSHOT_FLAG_MUTED = 0x1000` in `snapshot.rs` allows UI and telemetry consumers to know mute state while preserving the 72-byte invariant.

5. **Observation 5 (Subtitle Overlay Configuration in `subtitle_overlay.rs:205-255` vs `dialogs.rs:1220-1500`)**:
   - `SubtitleOverlayConfig` supports rich styling: `min_font_size`, `max_font_size`, `font_scale_ratio`, `text_color`, `pill_background_color`, `enable_shadow`.
   - `SubtitleConfigDialog` currently only exposes track selection, external file loading, and delay offset.
   - **Gap**: Users cannot adjust subtitle font size or styling from the UI dialog.
   - **Inference**: Exposing font scale ratio (e.g. 50% to 200%), text color presets, and background pill opacity in `SubtitleConfigDialog` directly connects existing overlay capabilities to the user.

6. **Observation 6 (Loop Playback Mode at EOF in `playback_runtime.rs:2180-2193`)**:
   - When presentation PTS reaches media duration, `playback_runtime.rs` automatically pauses playback.
   - **Gap**: There is no "Loop Playback" (repeat) toggle.
   - **Inference**: Introducing a loop mode (toggleable via menu, shortcut `L`, or slash command `/loop`) that automatically seeks to PTS 0 upon reaching EOF satisfies standard media player expectations. State can be stored in `SNAPSHOT_FLAG_LOOP_ENABLED = 0x2000` (72-byte invariant preserved).

7. **Observation 7 (Recent Files in `recent_files.rs:1-95` and `app.rs:1308-1329`)**:
   - `RecentFilesManager` persists up to 10 MRU paths in `$XDG_CONFIG_HOME/syncverse/recent_files.txt`.
   - **Gap**: If a file is deleted from disk after being added, clicking it in the menu fails silently. Path names are not truncated if excessively long.
   - **Inference**: Adding path validation with clean OSD error notifications ("File not found: ..."), dead-path pruning, and middle-ellipsis display formatting enhances robustness.

8. **Observation 8 (Clipboard File Loading in `app.rs:1301-1348`)**:
   - File loading currently occurs via native dialog (`Ctrl+O`), drag-and-drop, or CLI argument.
   - **Gap**: No "Open from Clipboard" feature (`Ctrl+V` when not focused on text input).
   - **Inference**: Reading text from `ctx.input(|i| i.raw.clipboard_text)` or system clipboard when pressing `Ctrl+V` (or selecting "Open Path from Clipboard" in File menu) allows instant playback of copied file paths or URLs.

9. **Observation 9 (Chat Full-Text Search in `index.rs` vs `sidebar.rs:413-440`)**:
   - `InvertedIndexFts` has full Unicode CJK tokenization and Boolean AND search implemented and tested.
   - **Gap**: The watch-party sidebar chat scroll area renders messages linearly without any search box or filter bar.
   - **Inference**: Adding a collapsible Search Bar at the top of the chat area (filtering displayed messages via `chat_index.search(...)`) exposes the FTS engine directly to the user.

10. **Observation 10 (Slash Commands in `commands.rs:122-243`)**:
    - Current commands: `/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`.
    - **Gap**: No `/help` command to list syntax; no `/clear` to reset the local chat history view; no `/loop` to toggle repeat; `/bookmark` does not have a `/bookmark list` sub-command.
    - **Inference**: Adding `/help`, `/clear`, `/loop`, and `/who` slash commands provides complete chat-based playback and room management.

11. **Observation 11 (Responsive Layouts for Constrained Window Dimensions)**:
    - On smaller window widths (e.g. 640px to 800px) with the sidebar open (300px), available width for the bottom controls bar is $\approx 500\text{px}$.
    - **Gap**: The bottom controls row contains 14 separate elements totaling over 650px, causing right-side elements to clip or overflow.
    - **Inference**: Applying responsive folding (hiding speed and aspect ratio combo boxes on width $< 700\text{px}$, as they are accessible via context menu) prevents visual overflow.

---

## 3. Caveats & Assumptions

1. **Hardware Acceleration & Video Rendering**:
   - Viewport rendering relies on CPU decoding (`ffmpeg-next` / software bitstream decoders) emitting RGBA buffers uploaded to GPU textures via `egui::ColorImage`. This report assumes hardware decode acceleration (VA-API / NVDEC / D3D11VA) is out of scope for the current pure-Rust software pipeline audit.
2. **Read-Only Investigation Mode**:
   - As an explorer agent in read-only survey mode, zero source files within `crates/` were modified. All proposals are presented with concrete design specifications and code snippets for implementer agents.
3. **Network Mode & Offline Audit**:
   - External network connections (WAN peers, remote GitHub update checking) were not actively pinged; analysis is based on static code structure, local loopback tests, and in-tree mock harnesses.

---

## 4. Conclusion: Prioritized Improvement Opportunities

The investigation has identified 12 high-impact, non-breaking improvement opportunities organized into three domains. All proposed enhancements strictly preserve the 72-byte `PlayerUiSnapshot` invariant and maintain zero mutex contention.

| Domain | Feature / Improvement | Description & User Benefit | Architectural Impact |
|---|---|---|---|
| **UI** | **Frame Stepping (`,` and `.`)** | Step 1 frame backward (`,`) or forward (`.`) while paused ($\approx 41.7\text{ms}$ or $1/\text{fps}$). | Adds `PlayerCommand::StepFrame(bool)` or relative step seek. Zero lock contention. |
| **UI** | **Hierarchical Quick Seeks** | Left/Right: ±5s; Shift+Arrows: ±30s; Ctrl+Arrows: ±60s; Menu entries for all leaps. | Pure UI/keymap logic. Zero data structure changes. |
| **UI** | **Loop Playback Mode** | Toggleable repeat mode (Ctrl+L / menu / `/loop`). Automatically seeks to 0 at EOF. | Uses bitflag `SNAPSHOT_FLAG_LOOP_ENABLED = 0x2000`. Preserves 72-byte snapshot. |
| **UI** | **Mute State in Snapshot** | Expose audio mute state via snapshot bitflag `SNAPSHOT_FLAG_MUTED = 0x1000`. | Uses unused bit 12 in `playback_state: u64`. Preserves 72-byte snapshot. |
| **UI** | **Subtitle Styling UI** | Add font size scaling slider (50%–200%), color presets, and opacity in `SubtitleConfigDialog`. | Wires into existing `SubtitleOverlayConfig`. Zero performance impact. |
| **UI** | **Clipboard Path Loading** | Ctrl+V or File $\to$ "Open Path from Clipboard" to load copied file paths or URLs. | Uses `ctx.input().raw.clipboard_text`. |
| **UI** | **Recent Files Polish** | Prune dead files, display OSD error toast if missing, middle-ellipsis truncation. | Minor enhancement in `recent_files.rs` and `app.rs`. |
| **UI** | **Responsive Controls Folding** | Dynamically collapse speed and aspect ratio combo boxes when bar width $< 700\text{px}$. | Responsive UI layout guard in `controls.rs`. |
| **Chat**| **Integrated Chat Search UI** | Add a search toggle/input in sidebar leveraging `InvertedIndexFts` with highlight filter. | Connects existing FTS engine to UI view. O(1) memory. |
| **Chat**| **Compact Timestamp Toggle** | Toggle to switch between full dual timestamps and compact media-only timestamps. | UI display toggle in `SidebarState`. |
| **Chat**| **New Slash Commands** | Implement `/help`, `/clear`, `/loop`, `/who`, and `/bookmark list`. | Handled in `SlashCommandParser` and `SlashCommandDispatcher`. |
| **Core**| **Folder Subtitle Auto-Detection**| Auto-load companion `.srt`/`.vtt` when opening a video file in the same directory. | Background file scan in `playback_runtime.rs`. |

### Detailed Implementation Specifications

#### 1. Frame Stepping Implementation Design
In `crates/syncverse-ui/src/gui/app.rs`:
```rust
// In global hotkeys (app.rs):
if ctx.input(|i| i.key_pressed(egui::Key::Period)) {
    // Step forward 1 frame (~41.7ms @ 24fps)
    let current_ms = snapshot.media_position_pts / 1_000_000;
    let dur_ms = snapshot.media_duration_pts / 1_000_000;
    let step_ms = 42; // default 1-frame duration
    let target = (current_ms + step_ms).min(dur_ms);
    if snapshot.is_playing() {
        let _ = self.command_tx.send(PlayerCommand::TogglePause);
    }
    self.viewport.clear_pending();
    let _ = self.command_tx.send(PlayerCommand::Seek(target));
    self.osd_overlay.show_notification(
        format!("Frame Step: +1 ({})", ControlsBarState::format_timestamp(target)),
        now,
    );
}
if ctx.input(|i| i.key_pressed(egui::Key::Comma)) {
    // Step backward 1 frame
    let current_ms = snapshot.media_position_pts / 1_000_000;
    let step_ms = 42;
    let target = current_ms.saturating_sub(step_ms);
    if snapshot.is_playing() {
        let _ = self.command_tx.send(PlayerCommand::TogglePause);
    }
    self.viewport.clear_pending();
    let _ = self.command_tx.send(PlayerCommand::Seek(target));
    self.osd_overlay.show_notification(
        format!("Frame Step: -1 ({})", ControlsBarState::format_timestamp(target)),
        now,
    );
}
```

#### 2. Loop Playback Mode Invariant Preservation
In `crates/syncverse-ui/src/snapshot.rs`:
```rust
/// Status bitflag indicating audio output is currently muted.
pub const SNAPSHOT_FLAG_MUTED: u64 = 0x1000;
/// Status bitflag indicating loop / repeat playback mode is enabled.
pub const SNAPSHOT_FLAG_LOOP_ENABLED: u64 = 0x2000;

impl PlayerUiSnapshot {
    /// Returns true if audio output is muted.
    #[inline]
    pub fn is_muted(&self) -> bool {
        (self.playback_state & SNAPSHOT_FLAG_MUTED) != 0
    }

    /// Returns true if loop playback mode is active.
    #[inline]
    pub fn is_loop_enabled(&self) -> bool {
        (self.playback_state & SNAPSHOT_FLAG_LOOP_ENABLED) != 0
    }
}
```
In `crates/syncverse-core/src/playback_runtime.rs`:
```rust
// EOF handling with Loop Mode:
if dur_ms > 0 && current_pts_ms >= dur_ms as f64 {
    if is_loop_enabled {
        // Automatically rewind to 0 and continue presenting
        clock.seek_to(0.0, now);
        current_audio_generation = current_audio_generation.wrapping_add(1);
        let _ = audio_tx.send(AudioWorkerCommand::Reset);
        if let Some(ref mut sess) = session {
            sess.seek(0, &video_ring);
        }
    } else {
        // Standard EOF auto-pause
        current_pts_ms = dur_ms as f64;
        if is_playing {
            clock.seek_to(dur_ms as f64, now);
            clock.set_state(now, PlaybackState::Paused);
            let _ = audio_tx.send(AudioWorkerCommand::Pause);
            is_playing = false;
        }
    }
}
```

#### 3. Subtitle Styling Dialog Extension
In `crates/syncverse-ui/src/gui/dialogs.rs:1470`:
```rust
// 4. Subtitle Typography & Styling
ui.heading("Subtitle Appearance");
ui.horizontal(|ui| {
    ui.label("Font Size:");
    let mut scale = self.overlay_config.font_scale_ratio * 1000.0; // e.g. 35.0
    if ui.add(egui::Slider::new(&mut scale, 20.0..=80.0).suffix(" pt")).changed() {
        self.overlay_config.font_scale_ratio = scale / 1000.0;
        self.overlay_config.min_font_size = scale * 0.4;
        self.overlay_config.max_font_size = scale * 1.5;
    }
});
ui.horizontal(|ui| {
    ui.label("Text Color:");
    ui.color_edit_button_srgba(&mut self.overlay_config.text_color);
    ui.label("Background Fill:");
    ui.color_edit_button_srgba(&mut self.overlay_config.pill_background_color);
});
ui.checkbox(&mut self.overlay_config.enable_shadow, "Enable Text Shadow / Outline");
```

#### 4. Chat Inverted Index Search UI Integration
In `crates/syncverse-ui/src/gui/sidebar.rs`:
```rust
// In show_content (sidebar.rs):
ui.horizontal(|ui| {
    ui.label(egui::RichText::new("Smart Chat").strong());
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button(if self.show_search { "✖ Search" } else { "🔍 Search" }).clicked() {
            self.show_search = !self.show_search;
            if !self.show_search {
                self.search_query.clear();
            }
        }
    });
});

if self.show_search {
    ui.horizontal(|ui| {
        ui.label("Filter:");
        ui.text_edit_singleline(&mut self.search_query);
        if !self.search_query.is_empty() && ui.button("Clear").clicked() {
            self.search_query.clear();
        }
    });
}

// In message filtering:
let filtered_records: Vec<&ChatRecord> = if self.search_query.trim().is_empty() {
    chat_records.iter().collect()
} else {
    // Leverage InvertedIndexFts in O(|L1| + |L2|)
    self.chat_index.search(self.search_query.trim())
};
```

---

## 5. Verification Method

To verify the observations, architectural invariants, and proposed improvements independently, execute the following commands:

1. **Dependency Blacklist Audit**:
   ```bash
   ./scripts/audit_dependencies.sh
   ```
   *Expected*: Passes with zero violations. Confirms zero forbidden monolithic player (`mpv`, `vlc`) or database (`sqlite`, `rocksdb`) dependencies.

2. **PlayerUiSnapshot Invariant Verification**:
   ```bash
   cargo test --package syncverse-ui --test hud_hotkeys_test test_player_ui_snapshot_invariant_72_bytes -- --nocapture
   ```
   *Expected*: Passes showing `size_of::<PlayerUiSnapshot>() == 72`.

3. **Workspace Compilation & Lint Check**:
   ```bash
   cargo check --workspace --all-targets
   cargo clippy --workspace --all-targets -- -D warnings
   ```
   *Expected*: Zero errors, zero warnings under `#![deny(warnings)]`.

4. **Unit & Integration Test Suite**:
   ```bash
   cargo test --workspace -- --nocapture
   ```
   *Expected*: All tests across 8 crates pass cleanly.

5. **Empirical Challenge Suites**:
   ```bash
   python3 tests/empirical_challenge/test_ui_renderer.py
   python3 tests/empirical_challenge/test_smart_chat.py
   ```
   *Expected*:
   - `test_ui_renderer.py`: All 4 test sections pass (0 errors). Verifies SPSC snapshot exchange with zero torn reads, 10Hz scrubber debouncing, Alpha8 font atlas bounding, and aspect ratio geometry.
   - `test_smart_chat.py`: All tests pass (0 errors). Verifies crash-resilient binary recovery, false-sync immunity, and inverted index CJK full-text search.

6. **Anti-Drift Intent Harness & End-to-End Verification**:
   ```bash
   python3 scripts/intent_harness.py
   ./scripts/verify_phase1.sh
   ```
   *Expected*: Full green verification across all architectural specifications.

---
*Report completed and certified by `explorer_p5_survey_3`.*
