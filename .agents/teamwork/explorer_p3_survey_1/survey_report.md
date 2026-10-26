# Syncplay-Alt Phase 3 Technical Survey Report: R1 & R2
**Author**: explorer_p3_survey_1  
**Date**: 2026-10-25  
**Workspace**: `/home/gautham/syncplay-alt`  
**Authority**: AGENTS.md, openspec/, ORIGINAL_REQUEST.md (Phase 3)  

---

## 1. Executive Summary & Architectural Invariants

Phase 3 transitions Syncplay-Alt from a headless mathematical engine into a desktop-grade, interactive media player and synchronized watch-party platform on Fedora Linux. This survey investigates the technical blueprints, interface contracts, algorithms, and integration strategies for:
1. **R1: Fedora Desktop Application & Interactive Terminal/OSD Presentation Mode (`play <FILE>`)**
2. **R2: Pure-Rust Linux Audio Output & Resampling Engine**

### Architectural Invariants Strictly Preserved:
- **Zero External Media/Codec/Database Frameworks**: No `ffmpeg`, `libav*`, `gstreamer`, `mpv`, `vlc`, `symphonia`, `dav1d`, `hound`, `image`, `sqlite`, or `alsa-sys`. All bitstreams, pixels, audio waveforms, clocks, and logs are processed from first principles in pure Rust.
- **Pure-Rust Dependency Whitelist**: Workspace remains restricted to `std`, `thiserror`, in-tree crates, and safe POSIX/Linux syscall interfaces.
- **Decoupled Lock-Free Threading**: 128-byte cache-aligned SPSC ring buffers (`SpscRingBuffer`) for inter-thread media transfers, triple-buffered atomic snapshots (`PlayerUiSnapshot`, 72 bytes) via the central Telemetry Aggregator.
- **VLC-Grade Responsiveness**: Sub-16ms keyboard input latency, 60fps presentation refresh, 10Hz scrubber debouncing, $\le 15.0\text{ MB}$ UI RAM footprint.

---

## 2. Requirement 1 (R1): Fedora Desktop Integration & Terminal Presentation

### 2.1 Fedora Desktop Application Specification (`syncplay-alt.desktop`)

Under the freedesktop.org Desktop Entry Specification v1.5, desktop environments on Fedora (GNOME 46+ on Wayland) discover installed applications through `.desktop` files placed in standard system or user directories.

#### File Locations & Permissions
- **Desktop Entry**: `~/.local/share/applications/syncplay-alt.desktop` (mode `0644`)
- **Repository Asset**: `assets/syncplay-alt.desktop`
- **Application Icon**: `~/.local/share/icons/hicolor/scalable/apps/syncplay-alt.svg` (mode `0644`)
- **Repository Icon Asset**: `assets/syncplay-alt.svg` (already present, high-resolution vector icon featuring neon orbital sync rings, squircle background, glowing nodes, and central play glyph)
- **Binary Target**: `/home/gautham/.local/bin/syncplay-alt` (or user `$PATH`)

#### Desktop Entry File Content
```ini
[Desktop Entry]
Version=1.5
Type=Application
Name=Syncplay-Alt
GenericName=Synchronized Media Player
Comment=Ultra-efficient pure-Rust standalone media player and long-distance synchronized watch-party platform
Exec=syncplay-alt play %F
Terminal=true
Icon=syncplay-alt
Categories=AudioVideo;Player;Video;Audio;
MimeType=video/x-matroska;video/mp4;video/webm;video/quicktime;audio/x-matroska;audio/mp4;audio/webm;
Keywords=syncplay;player;video;vlc;party;sync;media;matroska;mp4;mkv;
StartupNotify=true
Actions=HostParty;ConnectParty;Inspect;

[Desktop Action HostParty]
Name=Host Watch Party Room
Exec=syncplay-alt party host

[Desktop Action ConnectParty]
Name=Connect to Watch Party
Exec=syncplay-alt party connect 127.0.0.1:18242

[Desktop Action Inspect]
Name=Inspect Media Container
Exec=syncplay-alt inspect %F
```

#### Key Technical Decisions:
1. `Terminal=true`: Because the presentation mode renders a sleek terminal ANSI TrueColor half-block visual interface with interactive keyboard navigation, GNOME launches it inside the user's configured terminal (e.g., `ptyxis`, `gnome-terminal`, `kitty`, `alacritty`).
2. `Exec=syncplay-alt play %F`: `%F` expands to a list of file paths when files are opened from GNOME Files (Nautilus) or the application grid.
3. MIME Associations: Covers all common containers (`video/x-matroska`, `video/mp4`, `video/webm`, `video/quicktime`).
4. Validation & Database Update:
   ```bash
   desktop-file-validate ~/.local/share/applications/syncplay-alt.desktop
   update-desktop-database ~/.local/share/applications/
   ```
   *Direct Observation*: `desktop-file-validate` was executed on the existing `~/.local/share/applications/syncplay-alt.desktop` and passed cleanly with exit code 0.

---

### 2.2 Interactive Terminal / OSD Presentation Architecture

The presentation mode (`syncplay-alt play <FILE>`) replaces the static 5-tick simulation with a continuous, interactive 60fps rendering engine running directly in the terminal without external UI/terminal frameworks (no `crossterm`, `ncurses`, `ratatui`, or `termion`).

#### 2.2.1 Zero-Dependency Terminal Raw Mode & Lifecycle Guard

In canonical terminal mode, keystrokes are buffered by the line discipline until the user presses Enter, and typed characters echo automatically. To achieve instant $<16\text{ms}$ keyboard response, the terminal must be switched to **raw mode** without blocking.

##### Implementation Approaches:
1. **Approach A (Pure Safe Subprocess via `stty`)**:
   - On initialization:
     `stty -g` records the current terminal configuration string.
     `stty raw -echo min 0 time 0` configures non-blocking, un-buffered input where `std::io::stdin().read()` returns available bytes immediately (0 bytes if no key was pressed).
   - On exit (or panic):
     `stty <saved_config>` restores the terminal.
   - *Advantage*: 100% safe Rust, zero architecture-specific C struct definitions, zero compiler lints.
2. **Approach B (Direct POSIX `termios` ioctl / libc)**:
   - Uses `ioctl(0, TCGETS, &mut termios)` (`0x5401`) and `ioctl(0, TCSETS, &termios)` (`0x5402`).
   - Clears `ECHO | ICANON | IEXTEN | ISIG` on `c_lflag`, clears `IXON | ICRNL` on `c_iflag`.
   - Sets `c_cc[VMIN] = 0` and `c_cc[VTIME] = 0`.
   - *Advantage*: Microsecond-level mode switch without process fork overhead.

##### Recommended Architecture: `TerminalGuard` RAII Struct
To guarantee the user's terminal is NEVER left in a broken/raw state upon panic, signal (`SIGINT`), or error:
```rust
pub struct TerminalGuard {
    is_raw: bool,
    orig_termios: Option<termios>, // or saved stty string
}

impl TerminalGuard {
    pub fn enter_raw_mode() -> Self {
        if !std::io::stdin().is_terminal() {
            return Self { is_raw: false, orig_termios: None };
        }
        // Switch to Alternate Screen buffer: \x1b[?1049h
        // Hide cursor: \x1b[?25l
        print!("\x1b[?1049h\x1b[?25l");
        let _ = std::io::stdout().flush();
        // Engage raw mode...
        Self { is_raw: true, ... }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.is_raw {
            // Restore main screen buffer: \x1b[?1049l
            // Show cursor: \x1b[?25h
            // Reset colors: \x1b[0m
            print!("\x1b[?25h\x1b[0m\x1b[?1049l");
            let _ = std::io::stdout().flush();
            // Restore terminal settings...
        }
    }
}
```

##### Headless & CI Testability Guarantee:
When running under `cargo test`, non-interactive scripts, or standard pipes, `std::io::stdin().is_terminal()` evaluates to `false`. The player automatically skips raw terminal mode, disables cursor codes, and runs in headless presentation mode (or terminates cleanly after `--ticks N`), ensuring zero test failures or terminal hanging.

---

#### 2.2.2 Non-Blocking Keyboard Navigation & ANSI Escape Code Parser

In raw mode, keystrokes arrive as raw byte sequences from `std::io::stdin()`. Arrow keys and modified keys arrive as ANSI escape sequences beginning with `0x1B` (`ESC`).

##### Non-Blocking Input Reader Thread:
A lightweight dedicated thread polls `stdin` and pushes normalized `InputEvent` tokens into an `SpscRingBuffer<InputEvent, 32>` (or mpsc channel) drained by the 60fps presentation loop on every frame tick:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCommand {
    TogglePause,         // Space
    SeekRelative(i64),   // Left (-5s), Right (+5s), Shift+Left (-1s), Shift+Right (+1s)
    ShiftSpeed(i32),     // [ (-0.1x), ] (+0.1x)
    ToggleMute,          // M / m
    AdjustVolume(f32),   // Up (+5%), Down (-5%)
    ToggleChatOverlay,   // T / C
    ToggleSyncHud,       // S / s
    OpenCommandPrompt,   // /
    Quit,                // Q / q / Esc
}
```

##### ANSI Escape Byte Parsing Table:
| Key | Byte Stream | Parsed Command |
|---|---|---|
| `Space` | `0x20` | `KeyCommand::TogglePause` |
| `Left Arrow` | `0x1B, 0x5B, 0x44` (`\x1b[D`) | `KeyCommand::SeekRelative(-5000)` |
| `Right Arrow` | `0x1B, 0x5B, 0x43` (`\x1b[C`) | `KeyCommand::SeekRelative(+5000)` |
| `Shift + Left` | `0x1B, 0x5B, 0x31, 0x3B, 0x32, 0x44` (`\x1b[1;2D`) | `KeyCommand::SeekRelative(-1000)` |
| `Shift + Right` | `0x1B, 0x5B, 0x31, 0x3B, 0x32, 0x43` (`\x1b[1;2C`) | `KeyCommand::SeekRelative(+1000)` |
| `[` | `0x5B` | `KeyCommand::ShiftSpeed(-1)` (-0.1x) |
| `]` | `0x5D` | `KeyCommand::ShiftSpeed(+1)` (+0.1x) |
| `M` / `m` | `0x4D` / `0x6D` | `KeyCommand::ToggleMute` |
| `Up Arrow` | `0x1B, 0x5B, 0x41` (`\x1b[A`) | `KeyCommand::AdjustVolume(+0.05)` |
| `Down Arrow` | `0x1B, 0x5B, 0x42` (`\x1b[B`) | `KeyCommand::AdjustVolume(-0.05)` |
| `T` / `t` / `C` / `c` | `0x54` / `0x74` / `0x43` / `0x63` | `KeyCommand::ToggleChatOverlay` |
| `S` / `s` | `0x53` / `0x73` | `KeyCommand::ToggleSyncHud` |
| `/` | `0x2F` | `KeyCommand::OpenCommandPrompt` |
| `Q` / `q` | `0x51` / `0x71` | `KeyCommand::Quit` |
| `Esc` (alone) | `0x1B` (followed by read timeout) | `KeyCommand::Quit` / Close Prompt |

---

#### 2.2.3 ANSI 24-Bit TrueColor Half-Block Video Rendering

Terminal character cells typically exhibit a 1:2 width-to-height aspect ratio. Using Unicode **Upper Half Block** `▀` (`U+2580`):
- Foreground color (`\x1b[38;2;R_top;G_top;B_topm`) colors the top half of the cell.
- Background color (`\x1b[48;2;R_bot;G_bot;B_botm`) colors the bottom half of the cell.
- Character `▀` displays both pixels simultaneously in a single cell with an effective 1:1 square pixel aspect ratio!

##### Resolution Scaling & Viewport Calculation:
Let terminal dimensions obtained via `ioctl(1, TIOCGWINSZ, &mut winsize)` be $C$ columns and $R$ rows.
1. The terminal layout reserves:
   - Header Bar: 1 row
   - Scrubber Progress Bar: 1 row
   - OSD / Chat Overlay / Status Line: 2 rows
   - Usable Video Rows: $H_{\text{cell}} = R - 4$ rows.
2. Effective Video Pixel Canvas:
   $W_{\text{pixel}} = C$ pixels wide, $H_{\text{pixel}} = 2 \times H_{\text{cell}}$ pixels high.
3. Aspect Ratio Preservation:
   Standard 16:9 media is scaled into $W_{\text{pixel}} \times H_{\text{pixel}}$ with centered black letterboxing.
4. Hot-Loop Color Optimization:
   Writing unoptimized escape codes produces $\sim 40\text{ bytes}$ per cell. For $120 \times 40$ cells, this is $192\text{ KB}$ per frame, which can introduce terminal rendering lag.
   Syncplay-Alt implements differential escape code emission:
   - Track `last_fg: (u8, u8, u8)` and `last_bg: (u8, u8, u8)`.
   - Emit color change escape sequences only when color changes.
   - If `top_pixel == bot_pixel`: emit space character with background color (`\x1b[48;2;...m `).
   - Reduces frame payload to $< 18\text{ KB}$ per frame, comfortably streamed at 60fps ($< 1.1\text{ MB/s}$ internal throughput).

---

#### 2.2.4 Real-Time Scrubber Progress Bar & HUD Components

##### 1. Monospace Timecode HUD
Calculated using the existing `syncplay_ui::format_timecode_hud(current_ms, duration_ms)`:
`00:02:14 / 01:45:00`

##### 2. Scrubber Progress Bar Layout
Given usable terminal width $C$:
- Timecode length: 23 characters (`00:02:14 / 01:45:00`)
- Speed indicator: 9 characters (` [1.000x]`)
- Available bar width: $B = C - 36$ characters.
- Thumb position: $K = \lfloor (P \times B) \rfloor$, where progress ratio $P = \frac{\text{current\_pts}}{\text{duration\_pts}}$.
- Filled track (elapsed): `━` rendered in Cyan (`#06B6D4`).
- Scrubber thumb: `●` rendered in glowing Emerald (`#10B981`) or White (`#FFFFFF`).
- Empty track (remaining): `─` rendered in Dark Slate (`#334155`).
- Visual bar example:
  `00:02:14 [━━━━━━━━━━━━━●──────────────────────────] 01:45:00 [1.000x]`

##### 3. Tri-Color Sync Health Diagnostic Badge
Evaluated using `syncplay_ui::evaluate_sync_health(phase_error_ms, packet_loss)`:
- `Green` (`#22C55E`): `\x1b[38;2;34;197;94m● IN-SYNC\x1b[0m` ($|e(t)| < 15\text{ms}$)
- `Amber` (`#F59E0B`): `\x1b[38;2;245;158;11m● SLEW (1.025x)\x1b[0m` ($15\text{ms} \le |e(t)| \le 1500\text{ms}$)
- `Red` (`#EF4444`): `\x1b[38;2;239;68;68m● DRIFT (+1800ms)\x1b[0m` ($|e(t)| > 1500\text{ms}$ or loss $> 10\%$)

##### 4. Floating In-Player Chat Overlay
Integrated with `syncplay_ui::BoundedChatOverlay` (strictly bounded to $\le 5$ visible bubbles, 4.0s dwell + 1.0s exponential fade out).
Rendered directly above the scrubber as semi-transparent pills:
`╭─ @Gautham [00:15]: Awesome guitar solo! ──╮`
`╭─ @Alice [00:20]: Look at this intro scene! #epic ──╮`

##### 5. Interactive Slash-Command Bar (`/`)
Pressing `/` enters command input mode:
- Terminal prompt: `Command: /seek 01:23.450_`
- User types slash-command with live cursor.
- Pressing `Enter` executes `syncplay_chat::SlashCommandParser::parse`:
  * `/seek <time>` $\to$ dispatches `PlayerCommand::Seek(target_pts_ms)` to coordinator.
  * `/bookmark <label>` $\to$ creates timeline-anchored bookmark in `syncplay_chat.bin`.
  * `/clip <duration>` $\to$ triggers synchronized clip marker.
  * `/sync-status` $\to$ toggles extended network diagnostic overlay (RTT, $\hat{\theta}$, PLL integral accumulator, peer states).
  * Text without slash $\to$ logs smart chat comment, indexes in inverted index, broadcasts to room.
- Pressing `Esc` cancels prompt and resumes standard navigation.

---

## 3. Requirement 2 (R2): Pure-Rust Linux Audio Output & Resampling Engine

### 3.1 Zero-External-Dependency Linux Audio Sink Survey

#### 3.1.1 Banned Framework Audit & Policy Compliance
Under `AGENTS.md` Section 2.1, crates such as `symphonia`, `hound`, `minimp3`, `libavutil`, and `alsa-sys` are strictly banned. No external C audio libraries may be linked.

#### 3.1.2 Linux Audio Interface Comparison:

| Mechanism | Description | Zero Banned Crates? | Desktop Audio Graph | Headless/CI Friendly | Latency |
|---|---|---|---|---|---|
| **Direct ALSA (`/dev/snd/pcm*`)** | Pure-Rust ioctl on `/dev/snd/pcmC*D*p` | Yes | ❌ Fails on modern Fedora (PipeWire holds exclusive ALSA lock $\to$ `EBUSY`) | Requires ALSA hardware | $< 10\text{ms}$ |
| **Piped System Audio Server (`pw-cat` / `aplay`)** | Spawn `pw-cat --playback --raw ... -` with `Stdio::piped()` | Yes (uses standard system CLI, zero Rust crates) | ✅ Flawlessly routes to PipeWire/WirePlumber, Bluetooth, HDMI, headphones | Graceful fallback to Null sink if CLI absent | $20\text{--}40\text{ms}$ |
| **Stdout Raw PCM Stream (`--audio-stdout`)** | Writes interleaved PCM bytes to stdout | Yes | ✅ Enables standard Unix pipelines (`syncplay-alt play ... \| pw-play -`) | Excellent | Direct |
| **Null / Mock Audio Sink** | Advances time monotonically without writing samples | Yes | N/A (headless) | ✅ Guaranteed zero-failure CI testing | Precise |

#### 3.1.3 Selected Architecture: Multi-Tier Pure-Rust `AudioSink` Engine
```rust
pub trait AudioSink: Send {
    fn write_samples(&mut self, samples: &[f32]) -> Result<(), AudioSinkError>;
    fn flush(&mut self) -> Result<(), AudioSinkError>;
    fn sample_rate(&self) -> u32;
    fn channels(&self) -> u16;
}
```

1. **`PipeAudioSink` (Primary Desktop Sink)**:
   - On Fedora 44, auto-detects `pw-cat` (PipeWire native playback tool).
   - Spawns:
     ```bash
     pw-cat --playback --raw --rate 48000 --channels 2 --format f32 --latency 40ms -
     ```
     Fallback to `pw-play` or `aplay`:
     ```bash
     aplay -r 48000 -c 2 -f FLOAT_LE -t raw -
     ```
   - Audio worker thread writes little-endian IEEE 754 float sample bytes directly to `child.stdin`.
   - Zero crate dependencies, zero C bindings, native desktop volume and device switching support.
2. **`NullAudioSink` (CI / Headless Fallback)**:
   - Used when neither `pw-cat` nor `aplay` is installed, or when `--no-audio` is passed.
   - Accurately tracks sample counts and paces presentation without hanging or panicking.

---

### 3.2 Pure-Rust Linear Resampling Engine (`LinearResampler`)

Different media streams provide audio at varying sample rates (e.g. 44.1 kHz for CD/YouTube audio, 48.0 kHz for standard broadcast/Opus video, 96.0 kHz for high-res audio). To connect to a fixed-rate 48.0 kHz Linux audio sink, Syncplay-Alt requires a high-precision, zero-allocation sample rate converter.

#### 3.2.1 Mathematical Formulation
Let:
- Input sample rate: $f_{\text{in}}$ (e.g. 44,100 Hz)
- Output sample rate: $f_{\text{out}}$ (e.g. 48,000 Hz)
- PLL commanded playback speed multiplier: $r \in [0.96, 1.04]$
- Channels: $C = 2$ (stereo)

The effective fractional resampling step per output audio frame is:
$$\text{step} = \frac{f_{\text{in}}}{f_{\text{out}} \times r}$$

For each output sample index $n \ge 0$:
$$p(n) = n \cdot \text{step} + \text{phase}_{\text{initial}}$$
Integer base index: $i = \lfloor p(n) \rfloor$  
Fractional interpolation parameter: $\alpha = p(n) - i, \quad \alpha \in [0.0, 1.0)$

Linear interpolation for each channel $c \in \{0, 1\}$:
$$\hat{s}_c(n) = (1.0 - \alpha) \cdot s_c[i] + \alpha \cdot s_c[i + 1]$$

#### 3.2.2 Zero-Allocation Hot-Loop & Phase Continuity
1. **Inter-Chunk Phase Continuity**:
   Audio arrives in discrete packets (e.g., 1024 or 2048 samples). If the fractional phase is discarded at chunk boundaries, a high-frequency clicking artifact occurs.
   `LinearResampler` persists:
   - `phase: f64`: fractional remainder after the last generated sample.
   - `last_samples: [f32; 2]`: final stereo pair of the previous packet to interpolate across $i=0$.
2. **Pre-Allocated Scratch Buffers**:
   Output slice is written directly into a pre-allocated `Vec<f32>` buffer sized for maximum chunk expansion:
   $$\text{capacity} = C \times \left\lceil N_{\text{in\_max}} \times \frac{f_{\text{out}}}{f_{\text{in}}} \times \frac{1}{0.96} \right\rceil + 16$$
   Zero heap allocations occur inside `resample_chunk()`.

#### 3.2.3 Micro-Speed Audio Steering (PLL Integration)
When the network sync engine's discrete PI PLL micro-speed controller commands a speed change (e.g. $r = 1.025$ to eliminate a +35ms transatlantic phase error):
- Updating $r$ dynamically adjusts the resampling step $\text{step} = \frac{f_{\text{in}}}{f_{\text{out}} \times 1.025}$.
- Audio consumption accelerates by $2.5\%$, continuously phase-aligning with `MasterClock` and the remote party host without pitch discontinuity, hard-seek pops, or timeline stutter.

---

### 3.3 Zero-Allocation Audio Buffer Underrun Protection

Network jitter (±30ms WAN jitter) or thread scheduling delays can cause the audio decoding thread to temporarily lag behind the DAC consumption rate, draining the audio SPSC ring buffer (`spsc.try_pop() == None`).

#### Underrun Protection Strategy:
1. **Pre-Allocated Silence Scratch Buffer**:
   `SILENCE_BUFFER: [f32; 1024] = [0.0; 1024]` allocated once at startup.
2. **Detection & Injection**:
   When `spsc.try_pop()` yields `None`:
   - Increment atomic `underrun_counter`.
   - Feed `SILENCE_BUFFER` into `AudioSink::write_samples`.
   - Prevents ALSA/PipeWire pipe underrun errors (`-EPIPE`), broken pipe signals (`SIGPIPE`), and speaker pop noise.
3. **Smooth Re-entry**:
   When valid frames resume, apply a 2ms linear ramp-in on the first frame if recovering from extended underrun.

---

### 3.4 Decoupled A/V Pipeline Data Flow

```
+-------------------------------------------------------------------------+
|                              Demuxer / Decoder                          |
|         (syncplay-demux Matroska/ISOBMFF & syncplay-codecs)             |
+------------------------------------+------------------------------------+
                                     |
              +----------------------+----------------------+
              | Video (VideoFrameRGBA)                      | Audio (AudioFramePCM)
              v                                             v
  +-------------------------+                   +-------------------------+
  | SpscRingBuffer<RGBA, 16>|                   |SpscRingBuffer<Audio, 64>|
  | 128B Cache-Line Aligned |                   | 128B Cache-Line Aligned |
  +-----------+-------------+                   +-----------+-------------+
              |                                             |
              v                                             v
+-----------------------------+               +-----------------------------+
| Presentation Loop (60 Hz)   |               | Audio Worker Thread         |
| - MasterClock.current_pts() |               | - LinearResampler (44.1k->48k|
| - ANSI Half-Block Renderer  |               | - Volume & Mute Scaling     |
| - Scrubber Debouncer (10Hz) |               | - Underrun Silence Injector |
| - Timecode HUD & Sync Badge |               | - AudioSink (pw-cat/aplay)  |
+--------------+--------------+               +--------------+--------------+
               |                                             |
               +----------------------+----------------------+
                                      |
                                      v
                      +-------------------------------+
                      | Telemetry Aggregator          |
                      | - Compiles SubsystemMetrics   |
                      | - Publishes PlayerUiSnapshot  |
                      | - TripleBufferSnapshotExchange|
                      +-------------------------------+
```

---

## 4. Implementation Recommendations & Workspace Layout

### 4.1 Crate Responsibilities

1. **`crates/syncplay-ui`**:
   - `src/terminal.rs`: `TerminalGuard` (raw mode, screen clearing, alternate screen), ANSI 24-bit TrueColor half-block renderer, HUD formatter, key code mapper.
   - `src/render.rs`: `TerminalRenderer` buffering frame strings and flushing to stdout.
   - Existing modules (`snapshot.rs`, `overlay.rs`, `scrubber.rs`, `command.rs`) remain unmodified and fully integrated.
2. **`crates/syncplay-codecs`**:
   - `src/audio.rs`: `LinearResampler` (pure-Rust linear rate converter with phase continuity and zero allocations).
   - Integration with `AudioFramePCM`.
3. **`crates/syncplay-core`**:
   - `src/audio_sink.rs`: `AudioSink` trait, `PipeAudioSink` (spawning `pw-cat` / `aplay`), `NullAudioSink`, and `StdoutAudioSink`.
   - `src/coordinator.rs`: Audio worker thread management, SPSC audio ring buffer instantiation, interactive keyboard event dispatching.
   - `src/main.rs`: Full `play` command running interactive 60fps presentation loop with keyboard input, fallback to non-interactive mode when `--ticks N` or non-terminal detected.

---

## 5. Verification & Anti-Drift Matrix

Before declaring Phase 3 R1 & R2 complete, the following checks must pass:

```bash
# 1. Dependency Blacklist Audit (Strict zero external media/database engines)
./scripts/audit_dependencies.sh

# 2. Desktop Entry Validation
desktop-file-validate ~/.local/share/applications/syncplay-alt.desktop

# 3. Workspace Compilation & Zero Warning Check
export PATH="$HOME/.cargo/bin:$PATH"
cargo check --workspace --all-targets

# 4. Strict Clippy Check
cargo clippy --workspace --all-targets -- -D warnings

# 5. Full Workspace Test Suite
cargo test --workspace -- --nocapture

# 6. Empirical Challenge Test Suites
python3 tests/empirical_challenge/test_demux_codecs_clock.py
python3 tests/empirical_challenge/test_sync_engine.py
python3 tests/empirical_challenge/test_smart_chat.py
python3 tests/empirical_challenge/test_ui_renderer.py

# 7. Pi Intent Verification Harness (100% Compliance Guard)
python3 scripts/intent_harness.py
```

---
**Report finalized by explorer_p3_survey_1.**
