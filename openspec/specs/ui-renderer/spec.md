# Capability Specification: Desktop Graphical UI & Renderer (`syncverse-ui`)

**Component**: `syncverse-ui`  
**Standards Compliance**: Desktop Windowing Compositor Protocols (Wayland `xdg_shell`, X11 EWMH/NetWM, Win32), GPU Texture Presentation (`wgpu`, VSync), Sub-16ms Input Latency Bounds, RFC 2119  
**Status**: Authoritative OpenSpec Specification  

---

## 1. Overview & Architectural Role

The `syncverse-ui` crate specifies the native desktop graphical player user interface, GPU video viewport, On-Screen Display (OSD), and watch-party sidebar architecture. Desktop video players frequently suffer from input stutter, sluggish timeline scrubbing, audio crackle, and UI lockouts when interface rendering shares lock-guarded state with media decoding or network synchronization routines.

`syncverse-ui` delivers a modern, VLC-grade desktop player application running natively on Linux (Wayland and X11) and Windows while enforcing:
1. **Thread Partitioning**: Complete isolation of the graphical UI event loop from media demuxing, video/audio decoding, audio output streaming, and UDP network handling.
2. **Lock-Free Triple-Buffered Atomic State Snapshots**: Telemetry exchange via `PlayerUiSnapshot` pointers swapped with atomic memory operations, guaranteeing UI readers never lock or wait ($< 50\text{ ns}$).
3. **GPU Video Viewport & Aspect Ratio Preservation**: Hardware-accelerated rendering surface maintaining exact media aspect ratio with automatic pillarboxing/letterboxing and vsync frame pacing.
4. **Docked Playback Controls Bar**: Intuitive bottom panel featuring transport controls, timecode display, volume/mute controls, and playback speed selector.
5. **Debounced Timeline Scrubbing with Hover Preview**: Decoupling 60–144 Hz visual scrubber and tooltip feedback from 10 Hz rate-limited seek command emissions.
6. **In-Player Watch-Party Sidebar**: Collapsible right panel hosting room metadata, peer status, live sync diagnostics, and real-time dual-temporal smart chat with slash-command dispatching.
7. **Native File Management**: Desktop integration with native file open dialogs via `rfd` (XDG Desktop Portal) and window drag-and-drop file loading.

---

## 2. Requirements & Scenarios

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be interpreted as described in RFC 2119.

### Requirement: Native Desktop Windowing & Compositor Lifecycle (Wayland & X11)
The application SHALL instantiate a native desktop graphical window utilizing `eframe` / `egui` and `winit`:
1. **Compositor Protocols**: The window SHALL interface natively with Wayland compositors (via `xdg_shell` protocol on Fedora Linux / GNOME) and fallback to X11 (via EWMH/NetWM) when Wayland is unavailable, as well as native Windows desktop windowing.
2. **Window Operations**: The window MUST support standard desktop window management operations: minimize, maximize, smooth interactive window resizing, and clean close without leaking GPU buffers or OS handles.
3. **Fullscreen Toggle**: The application SHALL support toggling fullscreen mode via the `F11` keypress and via double-clicking the video viewport surface. In fullscreen mode, window title bar decorations SHALL be hidden, and playback controls and sidebars SHALL automatically fade/hide after $2.5\text{ seconds}$ of mouse inactivity, reappearing immediately upon mouse cursor movement.
4. **CLI Headless & Terminal Fallback**: The application MUST maintain support for command-line arguments `--headless` and `--terminal`, enabling automated testing harnesses, headless servers, and ANSI terminal rendering without requiring an active graphical display server.

#### Scenario: Launch Desktop Application Window
- **Given** an invocation of `syncverse` without arguments on a Wayland desktop session
- **When** the application starts up
- **Then** a graphical desktop window titled "Syncverse" appears with default geometry ($1280 \times 720\text{ px}$)
- **And** the window responds smoothly to window manager resize, minimize, and maximize actions.

#### Scenario: Toggle Fullscreen Mode via F11
- **Given** an active video playback session in windowed mode
- **When** the user presses `F11` or double-clicks the video viewport
- **Then** the application enters borderless fullscreen mode spanning the active display
- **And** when the mouse cursor remains stationary for $> 2.5\text{ seconds}$, the bottom control bar and top header auto-hide
- **And** upon subsequent mouse movement, controls reappear within $< 16\text{ ms}$.

---

### Requirement: Thread Decoupling & Mutex-Free State Sharing
The UI event and presentation loop SHALL NOT acquire mutex locks shared with media demuxing, video decoding, audio output, or network synchronization threads. Player state MUST be communicated to the UI via an immutable, self-contained `PlayerUiSnapshot` published through a triple-buffered atomic exchange mechanism adhering to a strict **Single-Producer Single-Consumer (SPSC)** thread topology:

1. **Sole Publisher Mandate**: A single, dedicated **Telemetry Aggregator** thread (executing within the `syncverse-core` coordinator loop) SHALL be the sole publisher authorized to write to the triple buffer. Subsystem threads SHALL NOT publish directly to the triple buffer; they SHALL report metrics to the Telemetry Aggregator via atomic variables or bounded lock-free queues.
2. **Atomic Publication**: The Telemetry Aggregator compiles telemetry into the dirty buffer and executes a release-ordered atomic swap with the clean index at a periodic tick rate (e.g. 60–120 Hz).
3. **Wait-Free Polling**: Polling the latest snapshot (`poll_latest_snapshot`) by the UI consumer thread MUST complete in $O(1)$ time ($< 50\text{ ns}$) with zero blocking, locking, system calls, or read tearing.

#### Scenario: Verify Zero Lock Contention During Frame Decoding
- **Given** an active video decoding thread processing a large video keyframe
- **When** the UI event loop polls for updated player state
- **Then** the UI thread retrieves the latest atomic snapshot immediately ($< 50\text{ ns}$)
- **And** zero thread blocking or priority inversion occurs.

---

### Requirement: Player UI Snapshot Schema & Telemetry
The `PlayerUiSnapshot` struct MUST encapsulate all telemetry required for the UI, OSD HUD, and watch-party sidebar:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct PlayerUiSnapshot {
    pub monotonic_timestamp: u64, // Nanoseconds (CLOCK_MONOTONIC)
    pub media_position_pts: u64,  // Presentation timestamp (nanoseconds)
    pub media_duration_pts: u64,  // Total file duration (nanoseconds)
    pub playback_state: u64,      // 0 = Stopped, 1 = Playing, 2 = Paused, 3 = Seeking
    pub network_rtt_nanos: u64,   // Filtered round-trip time
    pub clock_offset_nanos: i64,  // Filtered clock offset θ
    pub phase_error_nanos: i64,   // PLL phase error e(t)
    pub pll_speed_ratio_q16: u32, // Playback speed in Q16 fixed-point (65536 = 1.0000x)
    pub peer_count: u16,          // Total peers in room
    pub peers_ready: u16,         // Peers ready in current barrier session
    pub room_sequence: u64,       // Monotonic room update sequence
}
```

The size of the snapshot struct MUST NOT exceed 128 bytes (verified at 72 bytes) to ensure fast cache-line copying and prevent false sharing.

#### Scenario: Read Consistent Atomic Snapshot Under SPSC Telemetry Aggregation
- **Given** audio, video, and network subsystems concurrently updating internal counters
- **When** the Telemetry Aggregator compiles and publishes the unified `PlayerUiSnapshot` to the SPSC triple buffer
- **Then** the UI reader polls and receives an internally consistent snapshot where all telemetry fields reflect the exact same atomic tick
- **And** zero torn reads or multi-producer race conditions occur.

---

### Requirement: GPU Video Viewport & Aspect Ratio Preservation
The UI SHALL render decoded video frames onto a GPU texture surface using `wgpu` or `egui::ColorImage` texture allocation:
1. **Aspect Ratio Preservation**: The video viewport MUST preserve the native aspect ratio ($\text{AR}_{\text{video}} = W_{\text{video}} / H_{\text{video}}$) of the media stream regardless of the player window dimensions.
2. **Pillarboxing & Letterboxing**:
   - If the available viewport aspect ratio is wider than the video aspect ratio ($\text{AR}_{\text{avail}} > \text{AR}_{\text{video}}$), the viewport MUST apply vertical **pillarboxing** (black bars on left and right) with horizontally centered video rendering.
   - If the available viewport aspect ratio is taller than the video aspect ratio ($\text{AR}_{\text{avail}} < \text{AR}_{\text{video}}$), the viewport MUST apply horizontal **letterboxing** (black bars on top and bottom) with vertically centered video rendering.
3. **Geometry Calculation**:
   $$\text{scale} = \min\left(\frac{W_{\text{avail}}}{W_{\text{video}}}, \frac{H_{\text{avail}}}{H_{\text{video}}}\right)$$
   $$W_{\text{rendered}} = W_{\text{video}} \cdot \text{scale}, \quad H_{\text{rendered}} = H_{\text{video}} \cdot \text{scale}$$
   $$X_{\text{offset}} = X_{\text{avail}} + \frac{W_{\text{avail}} - W_{\text{rendered}}}{2}, \quad Y_{\text{offset}} = Y_{\text{avail}} + \frac{H_{\text{avail}} - H_{\text{rendered}}}{2}$$
4. **VSync Pacing**: Texture uploads and surface presentations SHALL be synchronized with the compositor's VSync refresh rate (60–144 Hz) to prevent screen tearing and frame pacing stutter.

#### Scenario: Pillarboxing Under Ultra-Wide Window Resizing
- **Given** a 16:9 video stream ($1920 \times 1080$, $\text{AR} \approx 1.778$)
- **When** the user resizes the window to an ultra-wide dimension ($2560 \times 1080$, $\text{AR} \approx 2.370$)
- **Then** the rendered video height fills the available height ($1080\text{ px}$)
- **And** the rendered video width is scaled to $1920\text{ px}$
- **And** black pillarbox bars of width $(2560 - 1920) / 2 = 320\text{ px}$ are displayed symmetrically on both left and right flanks.

#### Scenario: Letterboxing Under Tall Window Resizing
- **Given** a 16:9 video stream ($1920 \times 1080$, $\text{AR} \approx 1.778$)
- **When** the user resizes the window to a square aspect ratio ($1280 \times 1280$, $\text{AR} = 1.0$)
- **Then** the rendered video width fills the available width ($1280\text{ px}$)
- **And** the rendered video height is scaled to $1280 / (16/9) = 720\text{ px}$
- **And** black letterbox bars of height $(1280 - 720) / 2 = 280\text{ px}$ are displayed symmetrically on both top and bottom.

---

### Requirement: Interactive Playback Controls Bar
The UI SHALL provide an intuitive docked controls bar at the bottom of the window (height $48\text{--}56\text{ px}$):
1. **Transport Buttons**:
   - **Play/Pause Toggle**: Displays `▶` when paused and `❚❚` when playing. Triggered by button click or Spacebar shortcut.
   - **Stop Button**: Stops playback and resets media position PTS to 0.
2. **Timecode HUD**: Fixed-width monospace timecode `HH:MM:SS / HH:MM:SS` displaying current media PTS and total media duration.
3. **Volume & Mute Controls**:
   - Horizontal volume slider with range $[0.0, 1.0]$ ($0\%\text{--}100\%$).
   - Mute toggle button (shortcut `M`) that mutes audio output while preserving the previous volume level.
4. **Playback Speed Selector**: Dropdown or step selector supporting speeds $0.5\times, 0.75\times, 1.0\times, 1.25\times, 1.5\times, 2.0\times$.
5. **Panel Toggle Buttons**: Quick-access toggles for Watch-Party Sidebar visibility and Fullscreen mode.

#### Scenario: Spacebar Toggles Play/Pause
- **Given** an active video playback session
- **When** the user presses the Spacebar
- **Then** the UI speculative state immediately transitions between Playing and Paused
- **And** a `PlayerCommand::TogglePause` is queued to the core coordinator within $< 16\text{ ms}$.

#### Scenario: Volume Slider and Mute Toggle Adjust Audio Gain
- **Given** active audio playback with volume set to $80\%$ ($0.80$)
- **When** the user clicks the Mute toggle or presses `M`
- **Then** audio output is muted ($0.00$ gain) and the mute icon is displayed
- **And** when the user un-mutes, volume restores immediately to $80\%$.

---

### Requirement: Interactive Seekbar with Hover Preview & 10Hz Debounced Scrubbing
The timeline slider SHALL decouple smooth high-refresh visual feedback from rate-limited command emissions:
1. **Visual Feedback & Hover Tooltip**:
   - The visual scrubber thumb position and hover timestamp tooltip MUST update at the full display refresh rate ($60\text{--}144\text{ Hz}$).
   - Hovering over the seekbar MUST display a floating tooltip indicating the exact target timestamp (`HH:MM:SS`) corresponding to the cursor's horizontal position.
2. **Debounced Command Emissions**:
   - Seek commands emitted to the demuxer and network sync engine during active mouse dragging MUST be rate-limited (debounced) to a maximum frequency of $10\text{ Hz}$ ($100\text{ ms}$ minimum interval between intermediate seek packets).
3. **Authoritative Release Command**:
   - Upon mouse release (drag completion), a final authoritative seek command MUST be dispatched immediately to guarantee accurate landing on the user's release position.

#### Scenario: Rapid Mouse Drag on Progress Scrub-Bar with Tooltip Preview
- **Given** a user continuously dragging the scrub thumb across the timeline for $500\text{ ms}$
- **When** mouse move events fire at $120\text{ Hz}$ (60 events)
- **Then** the visual thumb updates 60 times without lag
- **And** the hover timestamp tooltip reflects current mouse position continuously
- **And** exactly 5 throttled intermediate seek commands are dispatched during the drag ($10\text{ Hz}$)
- **And** when the mouse button is released, a 6th final authoritative seek command is dispatched immediately, for a total of 6 emitted seek commands.

---

### Requirement: In-Player Watch-Party Sidebar & Real-Time Smart Chat
The UI SHALL provide a collapsible right sidebar panel (nominal width $280\text{--}320\text{ px}$) dedicated to watch-party interaction:
1. **Connection & Sync Diagnostics**:
   - Displays room role (Host / Client) and peer count.
   - Real-time network sync diagnostic badge displaying filtered RTT ($\text{ms}$), clock offset $\theta$ ($\text{ms}$), phase error $e(t)$ ($\text{ms}$), and PLL micro-speed multiplier.
   - Status dot color-coded according to drift zone:
     - **Green (`#22C55E`)**: Phase error $|e(t)| < 15\text{ ms}$ (Deadband in-sync).
     - **Amber (`#F59E0B`)**: Phase error $15\text{ ms} \le |e(t)| \le 1500\text{ ms}$ (Active PLL micro-speed slew).
     - **Red (`#EF4444`)**: Phase error $|e(t)| > 1500\text{ ms}$ or packet loss $> 10\%$ (Desynchronized / Hard seek).
2. **Scrollable Chat Stream**:
   - Displays incoming and historical chat messages retrieved from `syncplay_chat.bin`.
   - Each message displays author name, wall-clock time, media timestamp ($t_{\text{media}}$) with click-to-seek jump, and text payload.
3. **Interactive Chat Input**:
   - Single-line text input field supporting Enter-to-send.
   - Automatic slash-command parsing and dispatching: `/seek <timecode>`, `/bookmark`, `/clip`, `/poll <question>`, `/sync-status`.

#### Scenario: Render Watch-Party Sidebar with Live Room Telemetry
- **Given** a connected watch-party session with 3 peers, RTT $280\text{ ms}$, phase error $e(t) = +22\text{ ms}$, and PLL speed $1.018\times$
- **When** the watch-party sidebar is rendered
- **Then** the status badge dot is Amber (`#F59E0B`) indicating active micro-speed slew
- **And** the peer list displays all 3 connected room members
- **And** new chat messages scroll smoothly into view without hitching video playback.

---

### Requirement: Native File Selection & Drag-and-Drop Loading
The application SHALL provide user-friendly file loading methods:
1. **Native OS File Dialogs**:
   - The application SHALL support opening media files via `rfd` (Rust File Dialog).
   - On Fedora Linux / Wayland, `rfd` SHALL interface with the `xdg-desktop-portal` (GNOME portal) via D-Bus, presenting the user with the system-native GTK/GNOME file picker without requiring direct GTK3/GTK4 compilation dependencies.
   - Supported file extensions in the file filter SHALL include: `*.mkv`, `*.webm`, `*.mp4`, `*.m4v`, `*.mov`, `*.avi`, `*.ts`, `*.flv`, `*.ogg`, `*.ogv`.
2. **Drag-and-Drop File Loading**:
   - The application window SHALL accept dropped files directly from desktop file managers (e.g. GNOME Files / Nautilus, Windows Explorer).
   - Upon dropping a file onto the player window, the player SHALL immediately load the file, parse container headers, and initiate playback.
3. **Recent Media History**:
   - The application SHOULD persist a recent media history (up to 10 entries) stored in `$XDG_CONFIG_HOME/syncverse/recent_history.json`, accessible via a "Recent Files" menu.

#### Scenario: Open File via Native File Picker Dialog
- **Given** the player running with no media loaded
- **When** the user clicks "Open File" (shortcut `Ctrl+O`)
- **Then** the native OS file picker dialog appears via XDG Desktop Portal
- **When** the user selects a video file `movie.mkv` and confirms
- **Then** the player immediately demuxes container headers and begins synchronized playback.

#### Scenario: Drag-and-Drop Video File into Window
- **Given** the player running in windowed mode
- **When** the user drags a video file `video.mp4` from GNOME Files and drops it into the player window
- **Then** the player detects the drop event, loads `video.mp4`, and begins video presentation.

---

### Requirement: Memory Footprint & Hot-Loop Performance Budget
The graphical UI subsystem MUST operate within bounded resource limits:
1. **Memory Budget**:
   - **GPU Texture Memory**: Bounded by video frame size ($1920 \times 1080 \times 4\text{ bytes} \approx 8.3\text{ MB}$ per double-buffered texture $\approx 16.6\text{ MB}$).
   - **Egui UI State & Meshes**: $\le 25\text{ MB}$.
   - **Overall Application Resident Set Size (RSS) Idle**: $\le 85\text{ MB}$ in steady-state playback.
2. **CPU Utilization**:
   - At 60 FPS video playback with active sidebar, UI thread CPU utilization MUST remain $\le 5\%$ on modern multi-core x86_64 processors.
3. **Allocation Invariant**:
   - Zero unnecessary heap allocations SHALL occur in the per-frame render loop; UI vertex meshes and font textures MUST be cached and updated in-place.

#### Scenario: Measure Steady-State Memory Allocation Under Active Desktop Playback
- **Given** continuous 60 FPS video playback for 10 minutes with active video viewport, bottom controls, and watch-party sidebar open
- **When** monitoring process heap allocation and GPU texture usage
- **Then** zero continuous memory growth occurs
- **And** total application RSS remains strictly under $85\text{ MB}$.
