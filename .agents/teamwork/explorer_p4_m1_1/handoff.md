# Handoff Report — Specification Realignment Strategy for Milestone 1

- **Agent ID**: `explorer_p4_m1_1`
- **Working Directory**: `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_1`
- **Target Recipient**: Orchestrator (`ee88cac4-07d4-4dad-bc7e-4c3f606ef03f`) & Phase 4 Milestone 1 Workers
- **Date & Timestamp**: `2026-10-25T20:34:00Z`
- **Handoff Type**: Hard (Task Complete)

---

## 1. Observation

Direct examination of repository specifications, rules, and code yielded the following verbatim observations:

### 1.1 Invariant Rules & Prohibitions in `AGENTS.md`
- **File**: `/home/gautham/syncplay-alt/AGENTS.md`
  - **Lines 8–11**:
    > "1. **Zero External Media, Codec, or Database Frameworks**: Container demuxing, video/audio bitstream decoding, color space transformation, clock synchronization, and smart chat persistence are built entirely from first principles in pure Rust."
  - **Lines 17–24 (§2.1 The Zero-External-Codec Invariant)**:
    > "Under no circumstances may any crate within this workspace depend on, link against, or vendor code from external media, codec, player, or database frameworks.
    >
    > #### Strictly Banned Crate & Library List:
    > - **Media & Codec Engines**: `ffmpeg`, `ffmpeg-sys`, `ffmpeg-next`, `libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`, `gstreamer`, `gstreamer-sys`, `mpv`, `mpv-sys`, `libmpv`, `vlc`, `libvlc`, `symphonia`, `symphonia-core`, `dav1d`, `dav1d-sys`, `rav1e`, `openh264`, `minimp3`, `nom-ebml`, `hound`, `image`, `imageproc`.
    > - **Database & Search Engines**: `sqlite`, `sqlite3`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `rocksdb-sys`, `heed`, `tantivy`, `meilisearch`, `diesel`."
  - **Lines 26–30 (§2.2 Dependency Whitelist)**:
    > "Only pure-Rust foundational primitives are allowed:
    > - `thiserror` (strongly typed error enum derivation).
    > - Standard library modules (`std::net`, `std::sync::atomic`, `std::time::Instant`, `std::collections::BTreeMap`).
    > - In-tree mathematical primitives, byte-level readers (`ByteReader`, `BitReader`), and custom checksums."
  - **Lines 103–107 (§3 Item 7 `syncplay-ui`)**:
    > "7. **`syncplay-ui`**:
    > - Lock-Free Atomic Triple-Buffer Snapshot: `PlayerUiSnapshot` (72 bytes, fitting in a 128-byte cache line pair)...
    > - Scrubber Debouncer: 10 Hz rate limiter on drag events...
    > - Memory Bounding: Floating chat overlay capped at 5 visible bubbles (4.0s dwell, 1.0s fade)... font glyph atlas (4.0 MB)... total UI memory consumption $\le 15.0\text{ MB}$."

### 1.2 Master Specification in `openspec/project.md`
- **File**: `/home/gautham/syncplay-alt/openspec/project.md`
  - **Lines 25–30 (§2.1 The Zero-External-Codec Invariant)**:
    > "The workspace MUST NOT link against, wrap, or depend on external multimedia or database engines. Specifically:
    > - **Forbidden Media/Player Frameworks**: `ffmpeg`, `libavcodec`, `libavformat`, `libavutil`, `gstreamer`, `mpv`, `vlc`, `symphonia`, `dav1d`, `rav1e`, `openh264`, `minimp3`.
    > - **Forbidden Database/Search Engines**: `sqlite`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `heed`, `tantivy`, `meilisearch`."
  - **Lines 86–95 (§3.1 Crate Responsibilities)**: Currently limits `syncplay-demux` and `syncplay-codecs` strictly to zero-dependency parsers, and limits `syncplay-ui` to terminal/OSD models without desktop windowing or GPU texture presentation.

### 1.3 Capability Specification in `openspec/specs/ui-renderer/spec.md`
- **File**: `/home/gautham/syncplay-alt/openspec/specs/ui-renderer/spec.md`
  - **Lines 1–131**: Specifies thread decoupling via `PlayerUiSnapshot` (72 bytes), SPSC triple-buffer exchange (`TripleBufferSnapshotExchange`), 10Hz scrubber debouncing, OSD glyph overlay, and a strict $\le 15\text{MB}$ memory budget based on terminal rendering and software dirty-rect compositor.
  - **Absences**: Does not define requirements for native desktop graphical windowing (Wayland/X11), GPU texture surfaces (`wgpu`), aspect ratio preservation (letterboxing/pillarboxing geometry), bottom playback controls bar, in-player watch-party sidebar, or native OS file pickers (`rfd`).

### 1.4 Phase 4 Requirements Mandate
- **File**: `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (lines 182–231):
  - **R1. Flawless Desktop Graphical Player UI**: Modern desktop application window (Wayland/X11, Windows), GPU-accelerated video viewport preserving aspect ratio (letterboxing/pillarboxing), bottom control bar (Play/Pause, Stop, interactive seekbar with hover tooltip and 10Hz debouncing, volume slider, mute toggle, timecode HUD, speed selector), watch-party sidebar (members, RTT, sync health badge, real-time smart chat, slash-commands), and native file dialogs (`rfd`) / drag-and-drop.
  - **R2. Full VLC-Level Video & Audio Compatibility**: MKV/WebM, MP4/MOV, AVI, MPEG-TS, FLV, OGG; H.264, H.265, VP8, VP9, AV1, MPEG-2, MPEG-4, VC-1; AAC, MP3, Opus, Vorbis, FLAC, AC3/E-AC3, DTS, PCM.
  - **R3. Comprehensive Documentation & Specification Realignment**: Update `AGENTS.md`, `openspec/project.md`, `openspec/specs/`, `scripts/audit_dependencies.sh`, and `scripts/intent_harness.py`.

### 1.5 Patch Applicability
- **Tool execution**: `git apply --check /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_1/specification_realignment.patch`
- **Result**: Exit code 0 (clean dry-run application with zero merge conflicts).

---

## 2. Logic Chain

From the observations above, the realignment strategy is logically deduced step-by-step:

### 2.1 Identification of Obsolete Prohibitions vs. Preserved Invariants (AGENTS.md §2.1 & §2.2)

1. **Obsolete Media & Codec Prohibitions**:
   - *Observation*: `AGENTS.md` lines 21–22 explicitly ban `ffmpeg`, `ffmpeg-sys`, `ffmpeg-next`, `libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`, `symphonia`, `symphonia-core`, `image`, `imageproc`.
   - *Requirement Reference*: ORIGINAL_REQUEST R2 mandates playing MKV, MP4, WebM, AVI, TS, FLV, OGG containing H.264, H.265, VP8, VP9, AV1, MPEG-2, MPEG-4, VC-1 video and AAC, MP3, Opus, Vorbis, FLAC, AC3, DTS audio.
   - *Deduction*: Writing pure-Rust software decoders from scratch for 8 complex video formats (including AV1 and HEVC) and 8 audio formats within the scope of a desktop player project is computationally prohibitive. Direct multimedia bindings via `ffmpeg-next` / `ffmpeg-sys-next` (utilizing host `libav*` libraries) and `symphonia` are the industry standard for VLC-level format coverage in Rust. Therefore, the bans on `ffmpeg-next`, `ffmpeg-sys-next`, `libav*`, `symphonia`, and `symphonia-core` are obsolete and MUST be removed from §2.1 and added to the §2.2 whitelist.

2. **Obsolete GUI, Windowing & Audio I/O Prohibitions**:
   - *Observation*: `AGENTS.md` §2.2 whitelists only `thiserror` and standard library modules, prohibiting any GUI toolkit, windowing system, or audio output device framework.
   - *Requirement Reference*: ORIGINAL_REQUEST R1 mandates a native graphical desktop window (Wayland/X11/Windows), GPU video surface, and synchronized audio playback.
   - *Deduction*: The whitelist in §2.2 MUST be expanded to include:
     - `eframe`, `egui`: Native immediate-mode GUI framework.
     - `wgpu`: GPU rendering and hardware-accelerated video texture presentation.
     - `winit`: Cross-platform window creation and event loop (Wayland `xdg_shell`, X11, Win32).
     - `rfd`: Native file open dialogs interfacing via `xdg-desktop-portal` (Fedora GNOME) and Win32.
     - `cpal`, `rodio`: Native audio playback sink interfacing with system audio servers (PipeWire, ALSA, WASAPI).

3. **Invariants Requiring Strict Preservation**:
   - **Monolithic Player Script Wrappers**:
     - *Banned*: `mpv`, `mpv-sys`, `libmpv`, `vlc`, `libvlc`, and any IPC subprocess wrapping of external player binaries (`mpv --input-ipc-server`, `vlc -I rc`).
     - *Justification*: Syncplay-Alt is an autonomous standalone media player. Wrapping external players causes IPC brittleness, process lifetime desynchronization, display pipeline stutter, and prevents direct access to decoded frame buffers required by the `MasterClock` and discrete PI PLL speed controller.
   - **External Embedded Database & Search Engines**:
     - *Banned*: `sqlite`, `sqlite3`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `rocksdb-sys`, `heed`, `tantivy`, `meilisearch`, `diesel`.
     - *Justification*: The local append-only binary log `syncplay_chat.bin` with `0xAA55` framing, dual timestamps ($t_{\text{wall}}, t_{\text{media}}$), IEEE 802.3 CRC32 verification, forward false-sync recovery, and the in-memory inverted index FTS are architectural crown jewels of Syncplay-Alt. Replacing them with external database daemons would introduce heavy native dependencies, locking overhead, and schema migration fragility.
   - **Zero Warnings & Documentation Integrity**:
     - `#![deny(warnings)]` and `#![deny(missing_docs)]` remain strictly enforced.
   - **Lock-Free Concurrency & WAN Clock Synchronization**:
     - 128-byte cache-line aligned SPSC ring buffers, atomic 72-byte `PlayerUiSnapshot` triple buffering, RFC 5905 NTP 4-timestamp filtering with $D_{\text{effective}} \ge 5.0\text{ms}$ dispersion floor, and discrete PI PLL micro-speed controller ($r \in [0.96, 1.04]$) remain intact.

---

### 2.2 Realigned Specification for `openspec/project.md`

`openspec/project.md` must be updated across all 6 sections:
1. **Target Milestone & Scope**: Updated from Phase 1 scaffolding to Phase 4 Desktop Graphical UI & Universal Media Compatibility.
2. **System Vision (§1)**: Clarifies that Syncplay-Alt is a standalone player running its own presentation and decoding pipeline, strictly eliminating external monolithic player scripting/IPC wrappers (`mpv`, `vlc`) and external database engines (`sqlite`, `diesel`, `rocksdb`).
3. **Invariants (§2.1 & §2.2)**: Formally transitions from "The Zero-External-Codec Invariant" to "The Standalone Player & Storage Invariant", clearly separating permitted direct libraries (`ffmpeg-next`, `symphonia`, `eframe`, `wgpu`, `cpal`, `rfd`) from strictly prohibited external player wrappers and databases.
4. **Subsystem Topology (§3)**: Updates crate definitions:
   - `syncplay-demux`: Hosts `UniversalDemuxer` alongside pure-Rust synthetic vector parsers.
   - `syncplay-codecs`: Hosts `UniversalDecoder`, planar YUV420p, packed RGBA, and normalized `AudioFramePCM` models.
   - `syncplay-clock`: Integrates `MasterClock`, `AudioPacer`, and 128B SPSC ring buffers.
   - `syncplay-ui`: Houses the `eframe`/`egui`/`wgpu` desktop graphical player application.
   - `syncplay-core`: Coordinates demuxer, decoder, audio sink (`cpal`), sync engine, and launches the desktop GUI.
5. **Threading & Data Flow (§4)**: Incorporates the GUI loop, GPU video texture upload surface, audio sink thread, and centralized `TelemetryAggregator`.
6. **Subsystem Interface Contracts (§5)**: Adds `UniversalDemuxer`, `UniversalDecoder`, `AudioFramePCM`, and expanded `PlayerCommand` enums (`OpenFile`, `SetVolume`, `ToggleMute`).

---

### 2.3 RFC-2119 Desktop Graphical UI Requirements (`openspec/specs/ui-renderer/spec.md`)

`openspec/specs/ui-renderer/spec.md` is updated to define exhaustive RFC-2119 requirements:

1. **Native Desktop Windowing & Compositor Lifecycle (Wayland & X11)**:
   - The application **SHALL** instantiate a native desktop window utilizing `eframe` / `egui` and `winit`.
   - The window **SHALL** interface natively with Wayland compositors (via `xdg_shell` on Fedora Linux) and fallback to X11, as well as native Windows desktop windowing.
   - The window **MUST** support minimize, maximize, smooth resizing, and clean close without leaking GPU buffers.
   - The application **SHALL** support fullscreen mode via `F11` and viewport double-click, with playback controls and sidebars auto-hiding after $2.5\text{ seconds}$ of mouse inactivity.
   - The application **MUST** maintain `--headless` and `--terminal` CLI flags for automated CI testing and headless servers.

2. **GPU Video Viewport & Aspect Ratio Preservation**:
   - The UI **SHALL** render decoded video frames onto a GPU texture surface using `wgpu` or `egui::ColorImage` texture allocation.
   - The video viewport **MUST** preserve the native aspect ratio ($\text{AR} = W_{\text{video}} / H_{\text{video}}$) of the media stream regardless of window resizing.
   - When $\text{AR}_{\text{avail}} > \text{AR}_{\text{video}}$, the viewport **MUST** apply vertical **pillarboxing** (symmetrical black bars on left and right) with horizontally centered video.
   - When $\text{AR}_{\text{avail}} < \text{AR}_{\text{video}}$, the viewport **MUST** apply horizontal **letterboxing** (symmetrical black bars on top and bottom) with vertically centered video.
   - The viewport **MUST** compute rendered bounds via:
     $$\text{scale} = \min\left(\frac{W_{\text{avail}}}{W_{\text{video}}}, \frac{H_{\text{avail}}}{H_{\text{video}}}\right), \quad W_{\text{rend}} = W_{\text{video}} \cdot \text{scale}, \quad H_{\text{rend}} = H_{\text{video}} \cdot \text{scale}$$
     $$X_{\text{off}} = X_{\text{avail}} + \frac{W_{\text{avail}} - W_{\text{rend}}}{2}, \quad Y_{\text{off}} = Y_{\text{avail}} + \frac{H_{\text{avail}} - H_{\text{rend}}}{2}$$
   - Texture presentation **SHALL** synchronize with display VSync refresh rate (60–144 Hz) to eliminate tearing.

3. **Interactive Playback Controls Bar**:
   - The UI **SHALL** provide a bottom docked controls bar ($48\text{--}56\text{ px}$ height).
   - **MUST** provide Play/Pause toggle (button + Spacebar shortcut), Stop button (resets PTS to 0), and monospace timecode HUD `HH:MM:SS / HH:MM:SS`.
   - **MUST** provide volume slider ($[0.0, 1.0]$) with mute toggle button (`M` key shortcut) preserving prior volume.
   - **SHALL** provide speed selector ($0.5\times, 0.75\times, 1.0\times, 1.25\times, 1.5\times, 2.0\times$).

4. **Debounced Interactive Seekbar with Hover Preview**:
   - Visual scrub thumb and hover preview tooltip **MUST** update at the native refresh rate ($60\text{--}144\text{ Hz}$).
   - Hovering over the seekbar **MUST** display a floating tooltip indicating the exact target timestamp (`HH:MM:SS`) at the cursor position.
   - Emitted seek commands during mouse dragging **MUST** be debounced to a maximum rate of $10\text{ Hz}$ ($100\text{ ms}$ interval).
   - Upon mouse button release, a final authoritative seek command **MUST** be dispatched immediately (e.g. 500ms drag at 120Hz produces 5 intermediate + 1 final = 6 seeks).

5. **In-Player Watch-Party Sidebar & Real-Time Smart Chat**:
   - The UI **SHALL** provide a collapsible right sidebar panel ($280\text{--}320\text{ px}$ width).
   - **MUST** display room connection status (Host/Client), peer list, live RTT ($\text{ms}$), clock offset $\theta$ ($\text{ms}$), phase error $e(t)$ ($\text{ms}$), and PLL micro-speed multiplier.
   - **MUST** display color-coded status badge: Green ($|e| < 15\text{ ms}$), Amber ($15\text{ ms} \le |e| \le 1500\text{ ms}$), Red ($|e| > 1500\text{ ms}$).
   - **MUST** display scrollable smart chat message history with author, $t_{\text{wall}}$, and $t_{\text{media}}$ (with click-to-seek jump).
   - **SHALL** provide chat input field supporting regular chat and slash-commands (`/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`).

6. **Native File Selection & Drag-and-Drop Loading**:
   - The application **SHALL** support opening media files via `rfd` (Rust File Dialog) interfacing with `xdg-desktop-portal` (Fedora GNOME) via D-Bus and Win32 file picker on Windows.
   - Supported extensions in file filter **SHALL** include: `*.mkv`, `*.webm`, `*.mp4`, `*.m4v`, `*.mov`, `*.avi`, `*.ts`, `*.flv`, `*.ogg`, `*.ogv`.
   - The window **SHALL** accept dropped files directly from desktop file managers; upon file drop, the player **SHALL** load container headers and start playback immediately.
   - The application **SHOULD** persist recent media history (up to 10 entries) in `$XDG_CONFIG_HOME/syncplay-alt/recent_history.json`.

7. **Memory Footprint & Performance Bounds**:
   - GPU texture memory **MUST** remain bounded by double-buffered 1080p frame size ($\le 16.6\text{ MB}$).
   - Egui UI state heap allocations **MUST** remain $\le 25\text{ MB}$.
   - Overall application Resident Set Size (RSS) idle **MUST** remain $\le 85\text{ MB}$.
   - UI thread CPU utilization at 60 FPS video playback with active sidebar **MUST** remain $\le 5\%$ on modern multi-core x86_64 processors.

---

## 3. Caveats

1. **Host Dynamic Library Availability**:
   - `ffmpeg-next` and `ffmpeg-sys-next` require system `ffmpeg-devel` libraries (`libavcodec.so`, `libavformat.so`, `libavutil.so`, `libswscale.so`, `libswresample.so`) and `clang-libs` for `bindgen`.
   - Teammate agent `explorer_p4_m1_2` has designed `scripts/setup_deps.sh` to extract these RPM packages into `$HOME/.local` in user-space via `dnf download` + `rpm2cpio` without requiring root/sudo privileges on Fedora.
2. **Scope Boundary**:
   - As an explorer agent operating under strict read-only constraints, this agent has NOT directly overwritten `AGENTS.md`, `openspec/project.md`, or `openspec/specs/ui-renderer/spec.md`.
   - Instead, full proposed replacements and a verified unified patch (`specification_realignment.patch`) have been generated in this agent's folder for clean application by the implementation worker.

---

## 4. Conclusion

1. **Obsolete Bans Successfully Isolated**:
   - Identified obsolete prohibitions against `ffmpeg-next`, `ffmpeg-sys-next`, `symphonia`, `eframe`, `egui`, `wgpu`, `winit`, `rfd`, `cpal`, `rodio` in `AGENTS.md` §2.1 and `openspec/project.md` §2.1.
2. **Preserved Invariants Formally Enforced**:
   - Strict prohibitions against monolithic player scripting/IPC wrappers (`mpv`, `vlc`) and external database engines (`sqlite`, `diesel`, `rocksdb`, `tantivy`) remain 100% active.
   - Zero-warnings policy (`#![deny(warnings)]`), public documentation (`#![deny(missing_docs)]`), lock-free SPSC ring buffers, 72-byte `PlayerUiSnapshot`, and deterministic RFC 5905 NTP / discrete PI PLL sync remain fully preserved.
3. **Desktop Graphical UI Formally Specified**:
   - RFC-2119 requirements for Wayland/X11 windowing, GPU viewport with aspect-ratio preservation (letterboxing/pillarboxing), 10Hz debounced seekbar, watch-party sidebar, and native file dialogs are authored in full.
4. **All Artifacts Prepared and Verified**:
   - Authored `proposed_AGENTS.md`, `proposed_openspec_project.md`, `proposed_ui_renderer_spec.md`.
   - Generated `specification_realignment.patch` (68,684 bytes) and verified it passes `git apply --check` cleanly with 0 conflicts.

---

## 5. Verification Method

To independently verify this specification realignment strategy:

1. **Verify Patch Cleanliness**:
   ```bash
   git apply --check /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_1/specification_realignment.patch
   ```
   *Expected Result*: Returns exit code `0` with zero error messages.

2. **Inspect Generated Artifacts**:
   ```bash
   ls -lh /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_1/
   ```
   *Expected Result*:
   - `proposed_AGENTS.md` (7,361 bytes)
   - `proposed_openspec_project.md` (10,950 bytes)
   - `proposed_ui_renderer_spec.md` (14,488 bytes)
   - `specification_realignment.patch` (68,684 bytes)

3. **Verify Preserved Invariants in Proposed Files**:
   ```bash
   grep -E "(mpv|vlc|sqlite|diesel)" /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_1/proposed_AGENTS.md
   ```
   *Expected Result*: Confirms `mpv`, `vlc`, `sqlite`, `diesel` remain in the "Strictly Banned Crate & Library List".

4. **Verify Whitelisted Multimedia & GUI Backends**:
   ```bash
   grep -E "(ffmpeg-next|symphonia|eframe|egui|wgpu|rfd|cpal)" /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_1/proposed_AGENTS.md
   ```
   *Expected Result*: Confirms these libraries are present in §2.2 Dependency Whitelist.

5. **Verify RFC-2119 Keywords in Desktop GUI Capability Spec**:
   ```bash
   grep -E "(SHALL|MUST|SHOULD)" /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m1_1/proposed_ui_renderer_spec.md
   ```
   *Expected Result*: All graphical windowing, aspect ratio preservation, debounced scrubbing, sidebar, and native file dialog requirements use standard RFC-2119 keywords.
