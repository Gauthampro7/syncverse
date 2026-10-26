# Technical Handoff Report: Terminal Input & Presentation Loop Architecture (Milestone 4 / R1)

**Author**: `explorer_p3_m4_1`  
**Target Milestone**: Milestone 4 (Requirement R1: Fedora Desktop Application & Interactive Terminal/OSD Player Launcher)  
**Recipient**: `worker_m4` / `orchestrator_3_gen2`  
**Status**: Exploration Complete & Verified  

---

## 1. Observation

### 1.1 Existing Invocation of `play` in `syncplay-core`
- **File**: `crates/syncplay-core/src/main.rs`, lines 56–82.
- **CLI Handling**: Parses `play` subcommand arguments:
  ```rust
  "play" => {
      let mut target = "synthetic-mkv".to_string();
      let mut ticks: Option<usize> = None;
      // ... argument parsing for --ticks, --headless, --demo, <FILE> ...
      if let Some(n) = ticks {
          run_playback_simulation(&target, n);
      } else if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
          run_interactive_player(&target);
      } else {
          run_playback_simulation(&target, 5);
      }
  }
  ```
- **Current `run_playback_simulation`**: Defined at `crates/syncplay-core/src/main.rs:229-271`. Instantiates `PlayerCoordinator::new()` and executes `coordinator.run_presentation_pipeline(target, num_ticks)`.
- **Current `run_interactive_player`**: Defined at `crates/syncplay-core/src/main.rs:274-503`.
  - Initializes `TerminalGuard::enter_raw_mode()`.
  - Inspects container via `PlayerCoordinator::inspect(target)`.
  - Creates standalone `MasterClock::new(0.0, 1.0, PlaybackState::Playing)`.
  - Runs an ad-hoc loop throttled to **33ms (~30fps)** (line 356: `if now.duration_since(last_render) >= Duration::from_millis(33)`).
  - Keyboard handling match arm:
    ```rust
    KeyCommand::ToggleSyncHud | KeyCommand::OpenCommandPrompt => {}
    ```
    Neither Sync HUD (`S`) nor Slash Command bar (`/`) is currently implemented.
  - OSD status is hardcoded:
    ```rust
    rtt_ms: 28,
    phase_err_ms: 2.4,
    ```
    It does not consume live telemetry from `TripleBufferSnapshotExchange` or `TelemetryAggregator`.
  - Chat overlay displays 3 static hardcoded strings (`Alice`, `Bob`, `Charlie`), without connecting to `syncplay_chat::ChatStore` or `BoundedChatOverlay`.
  - `InteractivePlayer: fn run_presentation_loop(&mut self) -> Result<(), CoreError>;` specified in `PROJECT.md:40` does not yet exist as a struct.

### 1.2 Terminal Raw Mode & Input Handling in `syncplay-ui`
- **File**: `crates/syncplay-ui/src/terminal.rs`, lines 35–172.
- **Raw Mode Implementation**:
  - Uses `std::process::Command::new("stty")` with `-echo -icanon opost isig min 0 time 0`.
  - `min 0 time 0` configures POSIX non-blocking reads on standard input (`read(0, ...)` returns immediately with available bytes, or 0 if none).
  - Preserves `OPOST` (automatic `\n` to `\r\n` translation) and `ISIG` (signal delivery).
  - Switches to Alternate Screen buffer (`\x1b[?1049h`), hides cursor (`\x1b[?25l`), and clears screen (`\x1b[2J\x1b[1;1H`).
  - `Drop` implementation safely restores normal screen buffer, cursor, and original `stty` settings.
- **Key Sequence Parser (`parse_key_sequence`)**:
  - `b' '`: `KeyCommand::TogglePause`
  - `b'q' | b'Q' | 0x1B`: `KeyCommand::Quit`
  - `b'm' | b'M'`: `KeyCommand::ToggleMute`
  - `b't' | b'T' | b'c' | b'C'`: `KeyCommand::ToggleChatOverlay`
  - `b's' | b'S'`: `KeyCommand::ToggleSyncHud`
  - `b'/'`: `KeyCommand::OpenCommandPrompt`
  - `b'[' | b'<'`: `KeyCommand::ShiftSpeed(-1)`
  - `b']' | b'>'`: `KeyCommand::ShiftSpeed(1)`
  - `\x1b[D`: `KeyCommand::SeekRelative(-5000)` (Left Arrow)
  - `\x1b[C`: `KeyCommand::SeekRelative(5000)` (Right Arrow)
  - `\x1b[A`: `KeyCommand::AdjustVolume(0.05)` (Up Arrow)
  - `\x1b[B`: `KeyCommand::AdjustVolume(-0.05)` (Down Arrow)
  - `\x1b[1;2D`: `KeyCommand::SeekRelative(-1000)` (Shift+Left Arrow)
  - `\x1b[1;2C`: `KeyCommand::SeekRelative(1000)` (Shift+Right Arrow)
- **Terminal Size Polling Overhead**:
  - `get_terminal_size()` executes `Command::new("stty").arg("size").output()` synchronously.
  - Calling this on every 16ms frame at 60fps results in **60 process forks per second**, causing CPU overhead and jitter.

### 1.3 Subsystem Integration Interfaces
- **MasterClock** (`crates/syncplay-clock/src/master_clock.rs`):
  - Provides lock-free SeqLock reading via `current_pts(now)` and `is_playing()`.
  - Rate transitions via `reanchor(now, new_speed)` guarantee $C^0$ continuity: $\lim_{\epsilon \to 0} |\text{CurrentPTS}(t + \epsilon) - \text{CurrentPTS}(t - \epsilon)| = 0$.
  - State transitions via `set_state(now, next_state)`.
- **Audio Output** (`crates/syncplay-core/src/audio_sink.rs`):
  - `AutoAudioSink` probes `pw-cat` (PipeWire native on Fedora), falls back to `aplay` (ALSA), and defaults to `NullAudioSink` in headless CI.
  - Implements `write_samples(&mut [f32])`, `flush()`, `pause()`, and `resume()`.
- **Telemetry Aggregator & Snapshot Exchange** (`crates/syncplay-core/src/aggregator.rs`, `crates/syncplay-ui/src/snapshot.rs`):
  - `TelemetryAggregator` publishes 72-byte `PlayerUiSnapshot` (cache-line aligned) to `TripleBufferSnapshotExchange`.
  - `PlayerUiSnapshot` contains: `media_position_pts`, `media_duration_pts`, `playback_state`, `network_rtt_nanos`, `clock_offset_nanos`, `phase_error_nanos`, `pll_speed_ratio_q16`, `peer_count`, `peers_ready`.
- **TrueColor Video & OSD Formatting** (`crates/syncplay-codecs/src/color.rs`, `crates/syncplay-ui/src/overlay.rs`):
  - `convert_yuv420p_to_terminal(src, dst, standard)` maps planar YUV420p frames into `TerminalTrueColorBuffer` using half-block `▀` cells (top pixel = FG, bottom pixel = BG).
  - `format_timecode_hud(current_ms, duration_ms)` formats monospace `HH:MM:SS / HH:MM:SS`.
  - `evaluate_sync_health(phase_error_ms, packet_loss)` returns tri-color badge (`Green` < 15ms, `Amber` 15–1500ms, `Red` > 1500ms).
  - `BoundedChatOverlay` bounds visible chat bubbles to 5 items with a 5.0s window.
- **Slash Commands** (`crates/syncplay-chat/src/commands.rs`):
  - `SlashCommandParser::parse(input, current_pts_ms)` parses `/seek <time>`, `/bookmark [label]`, `/clip <start> <end>`, `/poll <q> <opts>`, and `/sync-status`.

---

## 2. Logic Chain

1. **Compliance with Zero-External-Codec Invariant**:
   - `scripts/audit_dependencies.sh` strictly forbids media and database crates.
   - `Cargo.lock` contains zero external media crates or `libc`.
   - The terminal raw mode implementation in `TerminalGuard` uses only standard library `std::process::Command` to invoke `/usr/bin/stty`, which is standard on Fedora Linux.
   - `syncplay-core` enforces `#![deny(unsafe_code)]`. Using `Command::new("stty")` is completely safe Rust and introduces zero unsafe blocks.

2. **Resolving the 60fps Loop Timing & Process Fork Overhead**:
   - A 60fps loop requires a frame budget of **16.66ms** (`Duration::from_micros(16666)`).
   - Calling `Command::new("stty").arg("size")` every 16ms is an anti-pattern (60 forks/sec).
   - *Solution*: Cache the terminal size and re-probe it throttled (e.g., every 300ms, or every 18 frames).
   - Non-blocking stdin reading (`read(0, ...)` with `VMIN=0, VTIME=0`) takes < 1µs.
   - Using a short loop sleep of `Duration::from_millis(2)` prevents CPU busy-spinning while ensuring < 2ms latency from keypress to state change.

3. **Flicker-Free Terminal Rendering Architecture**:
   - Issuing `\x1b[2J` (full screen clear) on every frame at 60fps causes visible blinking and flicker.
   - *Solution*: Use ANSI cursor home `\x1b[1;1H` and overwrite the display.
   - Append `\x1b[K` (clear to end of line) after each line to eliminate trailing artifacts on line length change.
   - Append `\x1b[J` (clear to end of screen) at the very bottom.
   - Buffer the entire screen in a single `String` and flush stdout once per frame (`print!("{}", screen); stdout().flush()`).

4. **Modular Architecture via `InteractivePlayer`**:
   - `PROJECT.md` line 40 defines the interface contract:
     `InteractivePlayer: fn run_presentation_loop(&mut self) -> Result<(), CoreError>;`
   - Placing `InteractivePlayer` in `crates/syncplay-core/src/interactive.rs` allows both `src/main.rs` and automated integration tests in `crates/syncplay-core/tests/` to exercise the exact same code path.
   - Adding a `max_ticks: Option<usize>` configuration field allows tests to run a bounded number of presentation frames without hanging or requiring human keystrokes in CI.

5. **Keyboard Navigation & Command Prompt Mechanics**:
   - Define a two-state input machine:
     ```rust
     pub enum PlayerInputMode {
         Normal,
         CommandBar,
     }
     ```
   - In `Normal` mode:
     - All single-key commands (`Space`, `Left`/`Right`, `Shift+Left`/`Right`, `[`, `]`, `M`, `Up`/`Down`, `T`, `S`, `Q`, `Esc`) are parsed by `parse_key_sequence` and applied immediately.
     - Pressing `/` switches to `CommandBar` mode and initializes `command_buffer = "/"`
   - In `CommandBar` mode:
     - Raw keystrokes append to `command_buffer`.
     - `Backspace` pops characters; if empty, returns to `Normal`.
     - `Esc` cancels without executing.
     - `Enter` submits `command_buffer` to `SlashCommandParser::parse`.
       * `/seek <time>` -> executes seek on `MasterClock`.
       * `/sync-status` -> toggles `show_sync_hud = true` and updates status message.
       * `/bookmark` -> stores bookmark and displays confirmation banner.
       * Chat message -> appends to `ChatStore` and `BoundedChatOverlay`.
     - Returns to `Normal` mode.

6. **Connecting Live Telemetry Aggregator**:
   - In each frame, `coordinator.tick(now)` gathers metrics from `MasterClock`, `Rfc5905Filter`, and `PllSpeedController`.
   - `TelemetryAggregator` publishes the metrics to `TripleBufferSnapshotExchange`.
   - The UI reads `ui_exchange.poll_latest_snapshot()` and uses real `network_rtt_nanos`, `phase_error_nanos`, and `pll_speed_ratio_q16` in `render_osd_hud` and the sync diagnostics panel.

---

## 3. Caveats

1. **Subprocess `stty` Availability**:
   - Relies on `/usr/bin/stty` being installed in the environment. On Fedora Linux (`coreutils`), this is guaranteed.
   - In headless environments (CI without pseudo-terminal), `stdin().is_terminal()` returns `false`, so `TerminalGuard::enter_raw_mode()` safely returns an inactive guard without invoking `stty`.
2. **Audio Daemon in CI**:
   - In headless or containerized CI environments where PipeWire or ALSA are absent, `AutoAudioSink` transparently falls back to `NullAudioSink`, ensuring all tests pass without sound hardware.
3. **Keyframe Snapping Availability**:
   - Container keyframe seek (`±5s`) relies on Cues (MKV) or `stss` (MP4). When playing raw bitstreams or synthetic vectors without seek tables, the seek seamlessly defaults to linear timestamp addition `(cur_pts ± 5000ms).clamp(0, duration)`.

---

## 4. Conclusion & Recommended Implementation Plan

The terminal presentation architecture is fully viable in pure Rust with zero forbidden frameworks. The worker should implement the following concrete changes:

### Step 1: Create `crates/syncplay-core/src/interactive.rs`
Define and implement `InteractivePlayer`:
```rust
//! Interactive terminal presentation player and OSD renderer for Fedora Linux.

use std::io::{stdout, IsTerminal, Read, Write};
use std::time::{Duration, Instant};

use syncplay_chat::{ChatRecord, ChatStore, ParsedSlashCommand, SlashCommandParser};
use syncplay_clock::PlaybackState;
use syncplay_codecs::{TerminalColorCell, TerminalTrueColorBuffer};
use syncplay_ui::{
    format_timecode_hud, render_hotkeys_footer, render_osd_hud, render_scrubber_bar,
    BoundedChatOverlay, KeyCommand, OsdHudState, TerminalGuard,
};

use crate::audio_sink::{AudioSink, AutoAudioSink};
use crate::coordinator::{MediaInfo, PlayerCoordinator};
use crate::error::CoreError;

/// Mode of input handling in the interactive player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerInputMode {
    /// Normal playback hotkey navigation.
    Normal,
    /// Slash-command and chat input prompt.
    CommandBar,
}

/// Standalone interactive terminal presentation player.
pub struct InteractivePlayer {
    /// Runtime coordinator binding demuxing, clock, sync, and telemetry.
    pub coordinator: PlayerCoordinator,
    /// Audio sink backend (PipeWire/ALSA or Null fallback).
    pub audio_sink: AutoAudioSink,
    /// Target media identifier or path.
    pub target: String,
    /// Inspected media container metadata.
    pub media_info: Option<MediaInfo>,
    /// Playback volume level in [0.0, 1.0].
    pub volume: f32,
    /// Whether audio output is currently muted.
    pub is_muted: bool,
    /// Whether the floating smart chat overlay is visible.
    pub show_chat: bool,
    /// Whether the detailed sync diagnostics HUD is visible.
    pub show_sync_hud: bool,
    /// Active keyboard input mode.
    pub input_mode: PlayerInputMode,
    /// Buffer holding current command bar text.
    pub command_buffer: String,
    /// Active transient status message and expiration time.
    pub status_message: Option<(String, Instant)>,
    /// Target frame rate in FPS (default 60).
    pub target_fps: u32,
    /// Optional limit on rendered frames for test/headless execution.
    pub max_ticks: Option<usize>,
}
```

Implement `run_presentation_loop`:
1. Check `stdin().is_terminal() && stdout().is_terminal()`.
2. Construct `TerminalGuard::enter_raw_mode()`.
3. Frame loop targeted at `Duration::from_micros(1_000_000 / self.target_fps as u64)` (~16.6ms for 60fps).
4. In each frame:
   - Poll keys from `guard.poll_key()`.
   - Process `KeyCommand` in `Normal` mode, or raw text input in `CommandBar` mode.
   - Advance `coordinator.tick(now)` and poll snapshot from `ui_exchange`.
   - Update terminal size throttled (every 300ms).
   - Render video canvas (using `convert_yuv420p_to_terminal` or procedural wave).
   - Render scrubber, OSD HUD line with live telemetry, floating chat overlay, sync diagnostics panel (if `show_sync_hud`), command bar prompt (if `CommandBar`), and hotkeys footer.
   - Single-pass atomic stdout write with `\x1b[1;1H` cursor repositioning.
   - Short sleep (`thread::sleep(Duration::from_millis(2))`) to prevent busy-spinning.
   - Break when `Quit` requested or `max_ticks` reached.

### Step 2: Implement Keyframe Snapping in Keyframe Seek
When `KeyCommand::SeekRelative(delta)` arrives:
- For `delta = ±5000` (Left/Right):
  - If `media_info.keyframe_entries` or demuxer keyframe list is populated, find the keyframe closest to `cur_pts + delta`.
  - Jump to that keyframe PTS.
- For `delta = ±1000` (Shift+Left/Right):
  - Jump directly to `(cur_pts + delta).clamp(0, duration)`.

### Step 3: Implement Sync Diagnostics HUD Panel
When `show_sync_hud` is true, render a formatted box below the OSD HUD line displaying:
- Connection status and role (Standalone / Host / Client).
- Filtered RTT (ms) and effective dispersion ($D_{\text{effective}}$).
- Clock phase error ($e(t)$) and filtered clock offset ($\hat{\theta}$).
- Discrete PI PLL steering rate ($r \in [0.96, 1.04]$) and integral accumulator.
- Barrier unpause state machine phase.

### Step 4: Implement Slash-Command Execution & Chat Integration
- Parse submitted string using `SlashCommandParser::parse(&self.command_buffer, Some(current_ms))`.
- Handle `/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`.
- Non-command text: save to `ChatStore` and push to `BoundedChatOverlay`.
- Set transient status banner (e.g. `"[SEEK] Target: 00:15.000"` for 3 seconds).

### Step 5: Wire `InteractivePlayer` into `crates/syncplay-core/src/lib.rs` and `main.rs`
- Export `InteractivePlayer`, `PlayerInputMode` in `syncplay-core::lib`.
- Refactor `syncplay-core/src/main.rs:run_interactive_player` to instantiate `InteractivePlayer` and call `player.run_presentation_loop()`.

### Step 6: Add Automated Verification Tests
Add test cases in `crates/syncplay-core/tests/cli_and_coordinator_integration.rs`:
- `test_interactive_player_bounded_ticks()`: instantiates `InteractivePlayer` with `max_ticks = Some(10)`, verifies clean execution at 60fps and returns `Ok(())`.
- `test_interactive_player_command_dispatch()`: verifies `/seek`, `/bookmark`, speed shift, and volume adjustments.

---

## 5. Verification Method

To independently verify the implementation, execute the following commands:

1. **Dependency Blacklist Audit**:
   ```bash
   ./scripts/audit_dependencies.sh
   ```
   *Expected*: Clean audit, zero banned dependencies.

2. **Workspace Compilation & Warning Deny**:
   ```bash
   export PATH="$HOME/.cargo/bin:$PATH"
   cargo check --workspace --all-targets
   cargo clippy --workspace --all-targets -- -D warnings
   ```
   *Expected*: Clean build, 0 warnings.

3. **Workspace Unit & Integration Tests**:
   ```bash
   cargo test --workspace -- --nocapture
   ```
   *Expected*: 100% test pass rate across all 8 crates.

4. **Empirical Challenge Suites**:
   ```bash
   python3 tests/empirical_challenge/test_ui_renderer.py
   python3 tests/empirical_challenge/test_demux_codecs_clock.py
   python3 tests/empirical_challenge/test_sync_engine.py
   python3 tests/empirical_challenge/test_smart_chat.py
   ```
   *Expected*: All empirical challenge suites exit code 0.

5. **Pi Framework Intent Verification Harness**:
   ```bash
   python3 scripts/intent_harness.py
   ```
   *Expected*: 8/8 intents passing with 100.0% compliance score.

6. **Interactive CLI Verification**:
   ```bash
   cargo run -p syncplay-core --bin syncplay-alt -- play synthetic-full-mkv --ticks 10
   ```
   *Expected*: Runs 10 frames of playback simulation with live OSD telemetry, keyframe inspection, and clean exit.

---
*End of Handoff Report.*
