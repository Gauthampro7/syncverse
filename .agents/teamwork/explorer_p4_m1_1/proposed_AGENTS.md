# AGENTS.md — Autonomous Agent Architecture & Engineering Standards

## 1. System Vision & Engineering Philosophy

**Syncplay-Alt** is an open-source, ultra-efficient standalone desktop media player and long-distance synchronized watch-party platform written in pure, idiomatic Rust. Traditional synchronized media solutions function as fragile IPC scripting wrappers around external monolithic players (e.g., `mpv`, `vlc`) or depend on external database daemons (`sqlite`, `rocksdb`).

Syncplay-Alt fundamentally diverges from these architectures:
1. **Standalone Native Architecture with Universal Media Compatibility**: Syncplay-Alt is a standalone player running its own presentation and decoding pipeline, strictly eliminating external monolithic player scripting/IPC wrappers (`mpv`, `vlc`) and external database engines (`sqlite`, `diesel`, `rocksdb`). Universal container demuxing (MKV, WebM, MP4, MOV, AVI, MPEG-TS, FLV, OGG) and video/audio bitstream decoding (H.264, H.265, VP8, VP9, AV1, MPEG-2/4, VC-1; AAC, MP3, Opus, Vorbis, FLAC, AC3, DTS, PCM) are handled via direct multimedia library bindings (`ffmpeg-next`, `symphonia`), feeding into in-tree planar YUV420p, packed RGBA, and PCM audio models. Smart chat persistence is built from first principles with an in-tree binary log (`syncplay_chat.bin`) and inverted index.
2. **Deterministic Mathematical Synchronization**: Clock phase alignment over high-RTT (250–400ms RTT, $\pm 30\text{ms}$ jitter) WAN links is achieved using RFC 5905 NTP 4-timestamp exchange, sliding-window minimum RTT filtering with dispersion freezing defense, critically damped discrete Proportional-Integral (PI) Phase-Locked Loop (PLL) micro-speed steering ($r \in [0.96, 1.04]$), and barrier-synchronized two-phase commit unpausing ($< 10\text{ms}$ cross-peer alignment).
3. **Decoupled Lock-Free Threading & Native Desktop Graphical UI**: A modern, hardware-accelerated desktop player window (`eframe`/`egui`/`wgpu`) running natively on Linux (Wayland/X11) and Windows. Cross-thread communication uses 128-byte cache-line aligned Single-Producer Single-Consumer (SPSC) ring buffers and triple-buffered atomic UI snapshots (`PlayerUiSnapshot`, 72 bytes) published via a centralized Telemetry Aggregator, guaranteeing sub-microsecond handoffs, zero mutex contention, zero audio buffer underruns, and desktop-grade UI responsiveness (< 16ms input-to-visual latency).
4. **Locally Persisted Dual-Temporal Smart Chat**: Crash-resilient append-only binary log storage (`syncplay_chat.bin`) with `0xAA55` framing, dual timestamps ($t_{wall}, t_{media}$), UUID sender identification, IEEE 802.3 CRC32 checksums, forward-scanning false-sync recovery, an in-memory inverted index supporting Unicode CJK unigram/bigram segmentation with single-ideograph length exemption, and slash-command dispatching (`/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`).

---

## 2. Invariant Prohibitions & Dependency Whitelist

### 2.1 Preserved Architectural Invariants & Forbidden Frameworks
Under no circumstances may any crate within this workspace depend on, link against, or vendor code from external monolithic players or external database engines.

#### Strictly Banned Crate & Library List:
- **Monolithic Player Frameworks & IPC Wrappers**: `mpv`, `mpv-sys`, `libmpv`, `vlc`, `libvlc`, and any subprocess/IPC wrapper around external player binaries. Syncplay-Alt MUST execute rendering and playback controls entirely within its own process space.
- **Database & Search Engines**: `sqlite`, `sqlite3`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `rocksdb-sys`, `heed`, `tantivy`, `meilisearch`, `diesel`. All chat persistence and full-text search MUST use the in-tree `syncplay_chat.bin` binary log and inverted index.

Any presence of these banned crates or their native libraries in `Cargo.toml`, `Cargo.lock`, or build scripts will result in immediate rejection by `scripts/audit_dependencies.sh` and CI verification.

### 2.2 Dependency Whitelist
The workspace permits foundational pure-Rust crates, direct multimedia bindings, and native graphical windowing libraries:
- **Core Primitives**: `thiserror` (strongly typed error enum derivation), standard library modules (`std::net`, `std::sync::atomic`, `std::time::Instant`, `std::collections::BTreeMap`), and in-tree mathematical/bitstream readers (`ByteReader`, `BitReader`, `Crc32`).
- **Universal Multimedia & Audio Backends**: `ffmpeg-next`, `ffmpeg-sys-next` (utilizing system `libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`), `symphonia`, `symphonia-core`, `cpal`, `rodio`.
- **Desktop Graphical UI & Windowing**: `eframe`, `egui`, `wgpu`, `winit`, `rfd` (native file dialogs via XDG Desktop Portal / Win32).

---

## 3. Workspace Architecture & Crate Responsibilities

The workspace is organized into 8 focused crates under `crates/`:

```
crates/
├── syncplay-common/     # Shared types, timestamps, CRC32, bitstream readers, error abstractions
├── syncplay-demux/      # Universal container demuxer (MKV, MP4, AVI, TS, FLV, OGG) + pure-Rust parsers
├── syncplay-codecs/     # Universal video/audio decoders, planar YUV420p, RGBA, PCM, Q8 color converter
├── syncplay-clock/      # Master monotonic clock, rate re-anchoring, audio pacer, 128B SPSC ring buffers
├── syncplay-sync/       # RFC 5905 NTP filter, discrete PI PLL micro-speed controller, barrier unpause
├── syncplay-chat/       # syncplay_chat.bin log, false-sync recovery, inverted index FTS, slash commands
├── syncplay-ui/         # eframe/egui desktop player window, GPU video viewport, debouncer, sidebar
└── syncplay-core/       # Coordinator orchestrating demuxing, decoding, audio sink (cpal), sync, and UI
```

### Detailed Crate Contracts:
1. **`syncplay-common`**:
   - `MediaTime`: Monotonic media presentation time in milliseconds and nanoseconds with rational arithmetic conversion.
   - `WallTime`: Monotonic wall-clock timestamp backed by `std::time::Instant` (`CLOCK_MONOTONIC`).
   - `ByteReader` / `BitReader`: Zero-copy, bounds-checked binary slice readers.
   - `Crc32`: Table-driven IEEE 802.3 CRC32 calculation (`0xEDB88320` polynomial, initial `0xFFFFFFFF`, final inversion).
2. **`syncplay-demux`**:
   - `UniversalDemuxer`: High-efficiency demuxing engine powered by `ffmpeg-next` supporting Matroska (`.mkv`, `.webm`), ISOBMFF/MP4 (`.mp4`, `.m4v`, `.mov`), AVI (`.avi`), MPEG-TS (`.ts`), FLV (`.flv`), and OGG (`.ogg`, `.ogv`).
   - `EbmlDemuxer` & `IsoBmffDemuxer`: In-tree zero-dependency demuxers preserving byte-exact parsing of synthetic test vectors and timescale arithmetic.
   - Seek Table Resolver: Maps random-access seek points and computes target presentation timestamps.
3. **`syncplay-codecs`**:
   - `UniversalDecoder`: Video bitstream decoder (H.264, H.265, VP8, VP9, AV1, MPEG-2, MPEG-4, VC-1) emitting `VideoFrameYUV420p` and `VideoFrameRGBA`; audio bitstream decoder (AAC, MP3, Opus, Vorbis, FLAC, AC3/E-AC3, DTS, PCM) emitting normalized `AudioFramePCM` (48kHz, stereo, f32).
   - `VideoFrameYUV420p`: 64-byte cache-aligned plane buffers ($Y, U, V$), 16-byte row stride alignment (`(width + 15) & !15`), and $+16$B SIMD tail padding.
   - `VideoFrameRGBA`: Packed 32-bit RGBA frames with 64-byte row alignment.
   - Pure-Rust Q8 Fixed-Point Color Space Converter ($Y'CbCr \to R'G'B'$ limited range ITU-R BT.601 / BT.709) with branchless clamping.
4. **`syncplay-clock`**:
   - Master Monotonic Clock: Continuous presentation tracking with SeqLock atomic rate re-anchoring guaranteeing $C^0$ mathematical continuity across speed shifts.
   - `AudioPacer`: Paces audio against `MasterClock`, evaluates audio timing delta, and injects silence frames on underruns to prevent thread starvation and audio crackle.
   - SPSC Lock-Free Ring Buffer: Power-of-two capacity, unbounded monotonic `AtomicUsize` counters (`head`, `tail`), bitwise masking. Head and tail isolated on separate 128-byte cache lines (`#[repr(align(128))]`) to eliminate false sharing.
5. **`syncplay-sync`**:
   - NTP 4-timestamp probe exchange ($T_1, T_2, T_3, T_4$) and RFC 5905 8-sample sliding-window minimum RTT filter with effective dispersion floor $D_{\text{effective}} = \max(D, 5.0\text{ ms})$.
   - Reference Timeline Equation: $\tau_{\text{target}}(t_{\text{local}}) = \tau_{\text{ref\_media}} + r_{\text{target}} \cdot \left[ (t_{\text{local}} + \hat{\theta}) - t_{\text{ref\_server}} \right]$.
   - Discrete PI PLL Speed Controller ($K_p = 0.17\text{ s}^{-1}, K_i = 0.0072\text{ s}^{-2}$) with Tri-Zone drift policy (Zone 1 deadband $<15\text{ms}$, Zone 2 micro-speed $[0.96, 1.04]$, Zone 3 hard seek $>1500\text{ms}$).
   - Two-Phase Barrier Unpause: Scheduled playback resumption guaranteeing $\ge 100\text{ms}$ positive commit arrival margin across all peers and cross-peer start alignment $< 10\text{ms}$.
6. **`syncplay-chat`**:
   - Binary Log (`syncplay_chat.bin`): 16-byte fixed header, `0xAA55` framing, dual timestamps ($t_{wall}, t_{media}$), UUID sender identification, IEEE 802.3 CRC32 checksums, and forward byte-by-byte false-sync recovery.
   - Inverted Index FTS: Unicode case folding (Latin, Turkish dotted I), punctuation stripping, CJK unigram + overlapping bigram segmentation, and single-ideograph indexing.
   - Slash Commands: `/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status` normalized to absolute milliseconds and dispatched to `syncplay-sync`.
7. **`syncplay-ui`**:
   - Native Desktop Window: Cross-platform `eframe`/`egui`/`wgpu` application window supporting Wayland, X11, and Windows, title bar, minimize/maximize, window resizing, clean close, and `F11`/double-click fullscreen toggle.
   - Video Viewport: GPU-accelerated video rendering surface with aspect-ratio preservation (automatic letterboxing / pillarboxing) and vsync frame pacing.
   - Playback Controls Bar: Bottom docked controls with Play/Pause, Stop, interactive seekbar with mouse hover timestamp tooltip and 10Hz debounced drag-scrubbing, volume slider with mute toggle, timecode HUD (`HH:MM:SS / HH:MM:SS`), and playback speed selector.
   - Watch-Party Sidebar: Collapsible right panel with room members, RTT, sync health indicator badge (Green, Amber, Red), real-time smart chat, and slash-command input.
   - File Management: Native file selection via `rfd` (XDG Desktop Portal on Fedora Linux, Win32 on Windows), drag-and-drop file loading, and recent media history.
   - Triple-Buffered Atomic Snapshot: 72-byte `PlayerUiSnapshot` published exclusively by Telemetry Aggregator; read wait-free by UI thread.
   - Headless / Terminal Fallback: Command-line flags (`--headless`, `--terminal`) preserved for automated test suites and TTY environments.
8. **`syncplay-core`**:
   - Coordinator binding universal demuxer, decoder, master clock, audio sink (`cpal`), sync engine, smart chat, and UI presentation.
   - Telemetry Aggregator polling subsystems and publishing unified `PlayerUiSnapshot` to `syncplay-ui`.

---

## 4. Coding Standards & Lints

All code written by agents MUST conform to the following standards:
1. **Zero Warnings Policy**: All crates MUST compile with zero compiler warnings. Enforce `#![deny(warnings)]` in CI.
2. **Mandatory Documentation**: Public interfaces MUST include `#![deny(missing_docs)]` and thorough docstrings explaining algorithms, invariants, and edge cases.
3. **Strongly Typed Errors**: Never use `String`, `&str`, or `Box<dyn Error>` for operational error handling. Define strongly typed enums with `#[derive(thiserror::Error, Debug)]`.
4. **Memory Safety & Unsafe Code Rules**:
   - `#![deny(unsafe_code)]` is standard across crates.
   - Unsafe code is permitted ONLY where strictly necessary for C-FFI multimedia bindings (`ffmpeg-sys-next`), SIMD intrinsics (`syncplay-codecs`), or lock-free slot indexing (`syncplay-clock`).
   - Every `unsafe` block MUST be preceded by a formal `// SAFETY:` comment proving memory safety, bounds validity, alignment, and absence of data races.
5. **SIMD & Memory Alignment**:
   - YUV420p frame row strides MUST be aligned to 16-byte boundaries (`(width + 15) & !15`).
   - Frame plane allocations MUST be aligned to 64 bytes with $+16$B SIMD tail padding.
   - SPSC ring-buffer atomic head/tail structures MUST be aligned to 128 bytes (`#[repr(align(128))]`) to eliminate false sharing.
6. **Integer Arithmetic & Pacing Continuity**:
   - Use fixed-point Q8 arithmetic for color space transformations.
   - Use 64-bit integer arithmetic for container timescale conversions to avoid float precision loss or 32-bit integer overflows.
   - Ensure $C^0$ mathematical continuity across all playback speed transitions.

---

## 5. Verification Commands & Tooling

Agents MUST verify their work using the following standard commands before declaring a task complete:

```bash
# 1. Dependency Blacklist Audit (Ensures zero banned monolithic players or database engines)
./scripts/audit_dependencies.sh

# 2. Workspace Compilation & Warning Check
cargo check --workspace --all-targets

# 3. Unit & Integration Test Suite
cargo test --workspace -- --nocapture

# 4. Strict Clippy Linter Check
cargo clippy --workspace --all-targets -- -D warnings

# 5. Empirical Challenge & Mathematical Invariant Test Suite
python3 tests/empirical_challenge/test_demux_codecs_clock.py
python3 tests/empirical_challenge/test_sync_engine.py
python3 tests/empirical_challenge/test_smart_chat.py
python3 tests/empirical_challenge/test_ui_renderer.py

# 6. Pi Framework Intent Verification Harness (Anti-Drift Guard)
python3 scripts/intent_harness.py

# 7. End-to-End Automated Verification Script
./scripts/verify_phase1.sh
```
