# Syncplay-Alt

> **Ultra-Efficient Standalone Desktop Media Player & Long-Distance Synchronized Watch-Party Platform**  
> *Written in pure, idiomatic Rust • Hardware-accelerated desktop GUI (`eframe`/`egui`/`wgpu`) • Universal container & codec compatibility • Mathematical WAN clock synchronization*

---

## 🌟 Highlights

Syncplay-Alt is a standalone desktop media player and synchronized watch-party platform built from the ground up to eliminate the clunkiness, IPC wrappers, and jitter of traditional solutions.

- **Desktop Graphical Player UI**: A modern, hardware-accelerated desktop window natively supporting **Linux (Wayland & X11)** and **Windows**. Features a GPU video viewport with aspect ratio preservation (letterboxing / pillarboxing), dark theme, and fluid vsync frame pacing.
- **VLC-Grade Format Compatibility**: Universal container demuxing (**MKV, MP4, WebM, AVI, MOV, MPEG-TS, FLV, OGG**) and video/audio bitstream decoding (**H.264, H.265 / HEVC 10-bit, VP8, VP9, AV1, MPEG-2/4, VC-1**; **AAC, MP3, Opus, Vorbis, FLAC, AC3, DTS / DTS-HD MA, PCM**).
- **Sub-Microsecond Lock-Free Threading**: UI, video decoding, audio pacing, and network synchronization run on completely isolated threads communicating via 128-byte cache-line aligned SPSC ring buffers and triple-buffered atomic snapshots (`PlayerUiSnapshot`).
- **Deterministic Long-Distance Sync**: Perfect synchronization over high-RTT WAN links (250–400ms RTT, $\pm 30\text{ms}$ jitter) using RFC 5905 NTP 4-timestamp exchange, sliding-window minimum RTT filtering, discrete Proportional-Integral (PI) Phase-Locked Loop (PLL) micro-speed steering ($r \in [0.96, 1.04]$), and barrier-synchronized two-phase commit unpausing ($< 10\text{ms}$ peer alignment).
- **Locally Persisted Dual-Temporal Smart Chat**: Crash-resilient append-only binary log storage (`syncplay_chat.bin`) with `0xAA55` framing, dual timestamps ($t_{\text{wall}}, t_{\text{media}}$), click-to-seek timestamp links, full-text search, and slash commands.
- **Terminal / Headless Fallback**: Full TUI presentation mode (`--terminal`) and headless simulation mode (`--headless`) for SSH sessions, CI pipelines, and automated test harnesses.

---

## 🚀 Quick Start

### 1. Launching from the Desktop (GNOME / Fedora)
Press <kbd>Super</kbd> (Windows key), type **Syncplay-Alt**, and press <kbd>Enter</kbd>.

### 2. Launching from the Terminal
```bash
# Open the graphical desktop player (with file picker & drag-and-drop):
syncplay-alt

# Open the graphical player directly with a video file:
syncplay-alt play ~/Videos/movie.mkv
# Supports .mkv, .mp4, .webm, .avi, .mov, .ts, etc.

# Open in terminal presentation mode (TUI):
syncplay-alt play ~/Videos/movie.mkv --terminal
```

---

## 🎮 Keyboard & Mouse Controls

| Action | Mouse | Keyboard Shortcut |
| :--- | :--- | :--- |
| **Play / Pause** | Click viewport or bottom play button | <kbd>Space</kbd> |
| **Seek Relative** | Drag or click seekbar | <kbd>←</kbd> / <kbd>→</kbd> ($\pm 5\text{s}$) |
| **Fine Seek** | — | <kbd>Shift</kbd> + <kbd>←</kbd> / <kbd>→</kbd> ($\pm 1\text{s}$) |
| **Volume Up / Down** | Drag volume slider | <kbd>↑</kbd> / <kbd>↓</kbd> ($\pm 5\%$) |
| **Mute / Unmute** | Click speaker icon | <kbd>M</kbd> |
| **Fullscreen Toggle** | Double-click viewport | <kbd>F11</kbd> |
| **Open File Dialog** | Click `File -> Open File` | <kbd>Ctrl</kbd> + <kbd>O</kbd> |
| **Toggle Watch-Party Sidebar** | Click chat icon or `View -> Sidebar` | <kbd>Tab</kbd> or <kbd>T</kbd> |
| **Adjust Speed** | Speed dropdown ($0.5\times \dots 2.0\times$) | <kbd>[</kbd> / <kbd>]</kbd> ($\pm 0.1\times$) |
| **Quit Application** | Window close button | <kbd>Ctrl</kbd> + <kbd>Q</kbd> |

---

## 👥 Long-Distance Watch Parties

Syncplay-Alt enables low-latency, stutter-free watch parties over international distances without external database daemons or player scripting bridges.

### 1. From the Desktop Graphical UI (Recommended)
- **Host Room**: Click `Watch Party -> Host Watch Party Room...` (or click `🌐 Host Room` at the top of the sidebar). It automatically displays your detected local LAN and loopback IP addresses (`192.168.x.x:18242`) with a single-click `📋 Copy` button to share with peers.
- **Join Room**: Click `Watch Party -> Connect to Watch Party...` (or click `🔗 Join Room` in the sidebar). Enter your friend's IP:port and connect.
- **Controls & Help**: Press <kbd>F1</kbd> or click `Help -> Keyboard Shortcuts & Controls` anytime inside the player.

### 2. From the Terminal
```bash
# Host a Watch-Party Room
syncplay-alt party host --port 18242 --file /path/to/movie.mkv

# Connect to a Watch-Party Room
syncplay-alt party connect 192.168.1.14:18242 --file /path/to/movie.mkv
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
| **Audio Output** | Native PipeWire streaming via `/usr/bin/pw-cat` (48kHz stereo float), ALSA (`/usr/bin/aplay`), and Null sink fallback |

---

## 🛠️ CLI Utilities & Inspection

```bash
# Inspect container headers, timescale, and audio/video tracks:
syncplay-alt inspect /path/to/media.mkv

# Run 5-tick non-interactive playback simulation:
syncplay-alt play /path/to/media.mkv --ticks 5

# Search local binary chat log via inverted index:
syncplay-alt chat query "awesome solo"

# Verify all architectural invariants and Intent Contracts:
syncplay-alt verify
```

---

## 🏗️ Architecture

```
crates/
├── syncplay-common/     # MediaTime, WallTime, CRC32, zero-copy ByteReader / BitReader
├── syncplay-demux/      # Universal container demuxer (MKV, MP4, AVI, TS, FLV, OGG)
├── syncplay-codecs/     # Universal video/audio decoders, planar YUV420p, RGBA, PCM
├── syncplay-clock/      # Master monotonic clock, rate re-anchoring, 128B SPSC ring buffers
├── syncplay-sync/       # RFC 5905 NTP filter, discrete PI PLL micro-speed controller
├── syncplay-chat/       # syncplay_chat.bin log, false-sync recovery, inverted index FTS
├── syncplay-ui/         # eframe/egui desktop player window, GPU video viewport, sidebar
└── syncplay-core/       # Application coordinator, background playback runtime, audio sink
```

---

## 📜 Invariant Guarantees & Verification

Syncplay-Alt enforces strict engineering standards:
1. **Zero Warnings Policy**: All crates compile with `#![deny(warnings)]` and `#![deny(missing_docs)]`.
2. **Zero Monolithic Player Wrappers**: Strictly independent of external player processes (`mpv`, `vlc`).
3. **Zero External Database Daemons**: Chat persistence and search are implemented entirely in-tree via `syncplay_chat.bin` and inverted indexing.

Verify the workspace locally:
```bash
./scripts/audit_dependencies.sh       # Audit manifests for forbidden crates
cargo check --workspace --all-targets # Zero compiler warnings
cargo test --workspace                # Full unit & integration test suite
python3 scripts/intent_harness.py     # 100% Intent Compliance Harness
./scripts/verify_phase4.sh            # Complete 8-stage automated verification
```
