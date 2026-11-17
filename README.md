# Syncverse

> **Ultra-Efficient Standalone Desktop Media Player & Long-Distance Synchronized Watch-Party Platform**  
> *Written in pure, idiomatic Rust • Hardware-accelerated desktop GUI (`eframe`/`egui`/`wgpu`) • Universal container & codec compatibility • Mathematical WAN clock synchronization*

---

## 🌟 Highlights

Syncverse is a standalone desktop media player and synchronized watch-party platform built from the ground up to eliminate the clunkiness, IPC wrappers, and jitter of traditional solutions. Born out of the frustration of watching legacy sync tools choke on transcontinental latency while trying to share an unforgettable film across opposite ends of the earth, Syncverse ensures two screens half a world away stay locked to the exact same frame in time.

- **Desktop Graphical Player UI**: A modern, hardware-accelerated desktop window natively supporting **Linux (Wayland & X11)** and **Windows**. Features a GPU video viewport with aspect ratio preservation (letterboxing / pillarboxing), dark theme, and fluid vsync frame pacing.
- **VLC-Grade Format Compatibility**: Universal container demuxing (**MKV, MP4, WebM, AVI, MOV, MPEG-TS, FLV, OGG**) and video/audio bitstream decoding (**H.264, H.265 / HEVC 10-bit, VP8, VP9, AV1, MPEG-2/4, VC-1**; **AAC, MP3, Opus, Vorbis, FLAC, AC3, DTS / DTS-HD MA, PCM**).
- **Sub-Microsecond Lock-Free Threading**: UI, video decoding, audio pacing, and network synchronization run on completely isolated threads communicating via 128-byte cache-line aligned SPSC ring buffers and triple-buffered atomic snapshots (`PlayerUiSnapshot`).
- **Deterministic Long-Distance Sync**: Perfect synchronization over high-RTT WAN links (250–400ms RTT, $\pm 30\text{ms}$ jitter) using RFC 5905 NTP 4-timestamp exchange, sliding-window minimum RTT filtering, discrete Proportional-Integral (PI) Phase-Locked Loop (PLL) micro-speed steering ($r \in [0.96, 1.04]$), and barrier-synchronized two-phase commit unpausing ($< 10\text{ms}$ peer alignment).
- **Locally Persisted Dual-Temporal Smart Chat**: Crash-resilient append-only binary log storage (`syncplay_chat.bin`) with `0xAA55` framing, dual timestamps ($t_{\text{wall}}, t_{\text{media}}$), click-to-seek timestamp links, full-text search, and slash commands.
- **Terminal / Headless Fallback**: Full TUI presentation mode (`--terminal`) and headless simulation mode (`--headless`) for SSH sessions, CI pipelines, and automated test harnesses.

---

## 🚀 Quick Start

### 1. Launching from the Desktop (GNOME / Fedora)
Press <kbd>Super</kbd> (Windows key), type **Syncverse**, and press <kbd>Enter</kbd>.

### 2. Launching from the Terminal
```bash
# Open the graphical desktop player (with file picker & drag-and-drop):
syncverse

# Open the graphical player directly with a video file:
syncverse play ~/Videos/movie.mkv
# Supports .mkv, .mp4, .webm, .avi, .mov, .ts, etc.

# Open in terminal presentation mode (TUI):
syncverse play ~/Videos/movie.mkv --terminal
```

---

## 🎮 Keyboard & Mouse Controls

| Action | Mouse | Keyboard Shortcut |
| :--- | :--- | :--- |
| **Play / Pause** | Click viewport or bottom play button | <kbd>Space</kbd> |
| **Stop** | Stop button | <kbd>S</kbd> |
| **Seek** | Drag or click seekbar | <kbd>←</kbd> / <kbd>→</kbd> (5 s); <kbd>Shift</kbd> (30 s); <kbd>Ctrl</kbd> (60 s); <kbd>Alt</kbd> (5 min) |
| **Jump to start / end** | — | <kbd>Home</kbd> / <kbd>End</kbd> (End goes to 1 s before the end) |
| **Jump to a given time** | — | <kbd>Ctrl</kbd> + <kbd>T</kbd> |
| **Step one frame forward / back** | — | <kbd>.</kbd> / <kbd>,</kbd> (pauses playback if it is running) |
| **Previous / next chapter** | — | <kbd>PgUp</kbd> / <kbd>PgDn</kbd> |
| **Previous / next file in queue or folder** | — | <kbd>P</kbd> / <kbd>N</kbd> |
| **Volume** | Drag volume slider or scroll wheel over the video (5% per step) | <kbd>↑</kbd> / <kbd>↓</kbd> (5%); <kbd>Ctrl</kbd> + <kbd>↑</kbd> / <kbd>↓</kbd> (1%). Maximum is 200% |
| **Mute / Unmute** | Click speaker icon or middle-click the video | <kbd>M</kbd> |
| **Playback speed** | Speed dropdown | <kbd>[</kbd> or <kbd>-</kbd> slower, <kbd>]</kbd> or <kbd>+</kbd> faster (0.1× steps, range 0.25× to 4.0×); <kbd>=</kbd> or <kbd>Backspace</kbd> resets to 1.0× |
| **Cycle audio track** | Audio menu | <kbd>B</kbd> |
| **Cycle subtitle track** | Subtitle menu | <kbd>V</kbd> or <kbd>C</kbd> |
| **Audio delay** | Audio dialog | <kbd>J</kbd> (−50 ms) / <kbd>K</kbd> (+50 ms); <kbd>Shift</kbd> + <kbd>J</kbd> resets to 0 |
| **Subtitle delay** | Subtitle dialog | <kbd>G</kbd> (−50 ms) / <kbd>H</kbd> (+50 ms); <kbd>Shift</kbd> + <kbd>G</kbd> resets to 0 |
| **Cycle aspect ratio** | Video dialog | <kbd>A</kbd> |
| **Cycle zoom mode / reset to Fit** | Video dialog | <kbd>Z</kbd> / <kbd>Shift</kbd> + <kbd>Z</kbd> |
| **A-B repeat** | A-B button | <kbd>R</kbd> or <kbd>Shift</kbd> + <kbd>L</kbd> cycles (set A, set B, off); <kbd>Shift</kbd> + <kbd>R</kbd> clears; <kbd>Alt</kbd> + <kbd>[</kbd> / <kbd>]</kbd> nudge point A by 100 ms; <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>[</kbd> / <kbd>]</kbd> nudge point B |
| **Loop current file** | — | <kbd>L</kbd> |
| **Fullscreen Toggle** | Double-click viewport | <kbd>F</kbd>, <kbd>F11</kbd> or <kbd>Alt</kbd> + <kbd>Enter</kbd>; <kbd>Esc</kbd> leaves fullscreen |
| **Open File Dialog** | `File -> Open File` | <kbd>Ctrl</kbd> + <kbd>O</kbd> |
| **Add files to queue** | `File -> Add Files to Playlist` | <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>O</kbd> |
| **Open path from clipboard** | `File -> Open Path from Clipboard` | <kbd>Ctrl</kbd> + <kbd>V</kbd> |
| **Queue dialog** | — | <kbd>Ctrl</kbd> + <kbd>L</kbd> |
| **Media info and codec statistics** | — | <kbd>Ctrl</kbd> + <kbd>I</kbd> or <kbd>Ctrl</kbd> + <kbd>J</kbd> |
| **Effects (equalizer, video adjustments)** | — | <kbd>Ctrl</kbd> + <kbd>E</kbd> |
| **Screenshot** | — | <kbd>F8</kbd> or <kbd>Ctrl</kbd> + <kbd>S</kbd> |
| **Toggle Watch-Party Sidebar** | Chat icon or `View -> Sidebar` | <kbd>Tab</kbd> |
| **Send emoji reaction** | — | <kbd>1</kbd> to <kbd>6</kbd> (❤️ 😂 🔥 👏 🍿 🎉) |
| **Shortcuts help** | `Help -> Keyboard Shortcuts & Controls` | <kbd>F1</kbd> |
| **Quit Application** | Window close button | <kbd>Ctrl</kbd> + <kbd>Q</kbd> |

Single-letter keys are ignored while a text box has focus or a dialog is open. This table was written from the key handling in `crates/syncverse-ui/src/gui/app.rs`.

---

## 👥 Long-Distance Watch Parties

Syncverse enables low-latency, stutter-free watch parties over international distances without external database daemons or player scripting bridges.

### 1. From the Desktop Graphical UI (Recommended)
- **Host Room**: Click `Watch Party -> Host Watch Party Room...` (or click `🌐 Host Room` at the top of the sidebar). It automatically displays your detected local LAN and loopback IP addresses (`192.168.x.x:18242`) with a single-click `📋 Copy` button to share with peers.
- **Join Room**: Click `Watch Party -> Connect to Watch Party...` (or click `🔗 Join Room` in the sidebar). Enter your friend's IP:port and connect.
- **Controls & Help**: Press <kbd>F1</kbd> or click `Help -> Keyboard Shortcuts & Controls` anytime inside the player.

### 2. From the Terminal
```bash
# Host a Watch-Party Room
syncverse party host --port 18242 --file /path/to/movie.mkv

# Connect to a Watch-Party Room
syncverse party connect 192.168.1.14:18242 --file /path/to/movie.mkv
```
- The client exchanges NTP probes, filters transit jitter, and locks its playback clock to the reference timeline using the discrete PI PLL micro-speed controller.

### Sync Health Indicator Badges
In the sidebar and OSD status bar, a real-time badge reports sync health:
- 🟢 **`● SYNCED`** (Green): Phase error $< 15\text{ms}$. Playback is in the deadband; speed multiplier is exactly $1.000\times$.
- 🟡 **`● DRIFT`** (Amber): Phase error between $15\text{ms}$ and $1500\text{ms}$. PI PLL micro-speed steering is actively nudging playback speed ($0.96\times \dots 1.04\times$) smoothly without pitch changes or stutter.
- 🔴 **`● SEEK`** (Red): Phase error $> 1500\text{ms}$. Automatic hard seek triggered to re-align immediately with the room reference timeline.

---

## 💬 Smart In-Player Chat & Slash Commands

Messages are timestamped both with the wall-clock time and the media playback PTS ($t_{\text{media}}$), and stored locally in the crash-resilient `syncplay_chat.bin` log.

### Interactive Slash Commands
Type these commands into the chat bar during playback:
- `/seek <time>`: Seeks room playback to a specific timestamp (e.g. `/seek 14:30`, `/seek 1h20m`, `/seek 450s`).
- `/bookmark <label>`: Anchors a permanent bookmark at the current playback position.
- `/clip <start> <end> <title>`: Marks an in-player highlight clip (e.g. `/clip 10:00 10:45 Epic Intro`).
- `/poll <question> | <opt1> | <opt2>`: Broadcasts an in-player voting poll card to all room participants.
- `/sync-status`: Displays detailed network telemetry (RTT, phase drift, PLL multiplier, dispersion).

---

## 📦 Container & Codec Support Matrix

| Container / Codec | Formats Supported |
| :--- | :--- |
| **Containers** | Matroska (`.mkv`, `.webm`), ISOBMFF (`.mp4`, `.mov`, `.m4v`), AVI (`.avi`), MPEG-TS (`.ts`), Flash Video (`.flv`), OGG (`.ogg`, `.ogv`) |
| **Video Codecs** | H.264 (AVC Baseline/Main/High), H.265 (HEVC 8-bit & 10-bit), VP8, VP9 (Profile 0/2), AV1, MPEG-2, MPEG-4 Part 2, VC-1 |
| **Audio Codecs** | AAC (LC/HE), MP3, Opus, Vorbis, FLAC (16/24-bit), AC3, E-AC3, DTS / DTS-HD Master Audio, uncompressed PCM (8/16/24/32-bit integer & float) |
| **Audio Output** | In-process output through `cpal` (48 kHz stereo float, resampled to the device rate if needed). If `cpal` cannot open a device, it falls back to a PipeWire or ALSA command-line player run as a subprocess, and then to a silent null sink. Headless mode always uses the null sink |

---

## 🛠️ CLI Utilities & Inspection

```bash
# Inspect container headers, timescale, and audio/video tracks:
syncverse inspect /path/to/media.mkv

# Run 5-tick non-interactive playback simulation:
syncverse play /path/to/media.mkv --ticks 5

# Search local binary chat log via inverted index:
syncverse chat query "awesome solo"

# Verify all architectural invariants and Intent Contracts:
syncverse verify
```

---

## 🏗️ Architecture

```
crates/
├── syncverse-common/     # MediaTime, WallTime, CRC32, zero-copy ByteReader / BitReader
├── syncverse-demux/      # Universal container demuxer (MKV, MP4, AVI, TS, FLV, OGG)
├── syncverse-codecs/     # Universal video/audio decoders, planar YUV420p, RGBA, PCM
├── syncverse-clock/      # Master monotonic clock, rate re-anchoring, 128B SPSC ring buffers
├── syncverse-sync/       # RFC 5905 NTP filter, discrete PI PLL micro-speed controller
├── syncverse-chat/       # syncplay_chat.bin log, false-sync recovery, inverted index FTS
├── syncverse-ui/         # eframe/egui desktop player window, GPU video viewport, sidebar
└── syncverse-core/       # Application coordinator, background playback runtime, audio sink
```

---

## 📜 Invariant Guarantees & Verification

Syncverse enforces strict engineering standards:
1. **Zero Warnings Policy**: All crates compile with `#![deny(warnings)]` and `#![deny(missing_docs)]`.
2. **Zero Monolithic Player Wrappers**: Strictly independent of external player processes (`mpv`, `vlc`).
3. **Zero External Database Daemons**: Chat persistence and search are implemented entirely in-tree via `syncplay_chat.bin` and inverted indexing.

Verify the workspace locally:
```bash
./scripts/audit_dependencies.sh       # Audit manifests for forbidden crates
cargo check --workspace --all-targets # Zero compiler warnings
cargo test --workspace                # Full unit & integration test suite
python3 scripts/intent_harness.py     # 100% Intent Compliance Harness
./scripts/verify_phase1.sh            # Complete 8-stage automated verification
```

---

## 📄 License

This project is licensed under the [MIT License](LICENSE) — see the LICENSE file for details.  
Copyright (c) 2026 Gautham.
