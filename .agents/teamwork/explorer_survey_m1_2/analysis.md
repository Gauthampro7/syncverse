# Workspace Survey & Mapping Report: Tests, Empirical Harnesses, Scripts, Desktop Integration & UI Branding

**Surveyor**: `explorer_survey_m1_2`  
**Milestone**: M1 (Survey & Mapping)  
**Target Goal**: Support the comprehensive rename from `Syncplay-Alt` to `Syncverse` across the workspace.

---

## 1. Test Files, Fixtures, and Runners Inventory

### 1.1 Integration Tests in `crates/*/tests/` (37 Test Files)
Each crate containing integration tests currently imports other crates via `syncplay_<subsystem>`. When crate directories are renamed from `crates/syncplay-*` to `crates/syncverse-*` and package names are changed to `syncverse-*`, every `use syncplay_*` statement must be transformed to `use syncverse_*`.

| Crate | Test File Path | Imports & Key References |
|---|---|---|
| `syncplay-chat` | `crates/syncplay-chat/tests/adversarial_chat_tests.rs` | `use syncplay_chat::{...}`; asserts `"Syncplay-Alt Sync Status"` (L146); uses `syncplay_chat::make_file_header` (L160); temp filename `syncplay_test_store_{}.bin` (L407) |
| `syncplay-chat` | `crates/syncplay-chat/tests/chat_adversarial_challenge.rs` | `use syncplay_chat::{...}`; asserts `"=== Syncplay-Alt Sync Status ==="` (L513); temp filename `syncplay_adversarial_store_{}.bin` (L519) |
| `syncplay-chat` | `crates/syncplay-chat/tests/persistence_fts_slash_stress_p3.rs` | `use syncplay_chat::{...}`; asserts `"Syncplay-Alt Sync Status"` (L444); references `syncplay_chat.bin` in doc comments |
| `syncplay-clock` | `crates/syncplay-clock/tests/adversarial_clock_stress.rs` | `use syncplay_clock::{...}` |
| `syncplay-codecs` | `crates/syncplay-codecs/tests/adversarial_bitstream_fuzz_test.rs` | `use syncplay_codecs::{...}`, `use syncplay_common::{...}` |
| `syncplay-codecs` | `crates/syncplay-codecs/tests/adversarial_codecs_stress.rs` | `use syncplay_codecs::{...}` |
| `syncplay-codecs` | `crates/syncplay-codecs/tests/audio_resampler_adversarial_stress_tests.rs` | `use syncplay_codecs::{AudioFramePCM, LinearResampler}` |
| `syncplay-codecs` | `crates/syncplay-codecs/tests/audio_resampler_remediation_challenge_test.rs` | `use syncplay_codecs::audio::resampler::LinearResampler` |
| `syncplay-codecs` | `crates/syncplay-codecs/tests/audio_resampler_tests.rs` | `use syncplay_codecs::{AudioFramePCM, LinearResampler}` |
| `syncplay-codecs` | `crates/syncplay-codecs/tests/audio_truecolor_simd_challenge.rs` | `use syncplay_codecs::{...}` |
| `syncplay-codecs` | `crates/syncplay-codecs/tests/bitstream_decoders_test.rs` | `use syncplay_codecs::{...}`, `use syncplay_common::reader::BitReader` |
| `syncplay-codecs` | `crates/syncplay-codecs/tests/challenger_remediation_verification_test.rs` | `use syncplay_codecs::{...}` |
| `syncplay-codecs` | `crates/syncplay-codecs/tests/universal_codecs_adversarial_stress.rs` | `use syncplay_codecs::{...}` |
| `syncplay-core` | `crates/syncplay-core/tests/adversarial_audio_pacer_sink_stress.rs` | `use syncplay_clock::{...}`, `use syncplay_codecs::AudioFramePCM`, `use syncplay_core::{...}` |
| `syncplay-core` | `crates/syncplay-core/tests/adversarial_crates_1_to_4.rs` | `use syncplay_clock::{...}`, `use syncplay_codecs::{...}`, `use syncplay_common::{...}`, `use syncplay_demux::{...}` |
| `syncplay-core` | `crates/syncplay-core/tests/adversarial_crates_5_to_8.rs` | `use syncplay_chat::{...}`, `use syncplay_core::{...}`, `use syncplay_sync::{...}`, `use syncplay_ui::{...}` |
| `syncplay-core` | `crates/syncplay-core/tests/adversarial_milestone4_interactive_stress.rs` | `use syncplay_clock::PlaybackState`, `use syncplay_core::{...}`, `use syncplay_ui::{...}` |
| `syncplay-core` | `crates/syncplay-core/tests/adversarial_phase5_challenge.rs` | `use syncplay_chat::RoomAnnouncement`, `use syncplay_common::Crc32`, `use syncplay_sync::{...}`, `use syncplay_ui::{...}` |
| `syncplay-core` | `crates/syncplay-core/tests/audio_pipeline_and_underrun_tests.rs` | `use syncplay_clock::{...}`, `use syncplay_codecs::{...}`, `use syncplay_core::{...}` |
| `syncplay-core` | `crates/syncplay-core/tests/cli_and_coordinator_integration.rs` | `use syncplay_chat::{...}`, `use syncplay_clock::{...}`, `use syncplay_core::{...}`, `use syncplay_demux::build_synthetic_full_mkv`, `use syncplay_sync::{...}`, `use syncplay_ui::{...}` |
| `syncplay-core` | `crates/syncplay-core/tests/commands_chat_cli_empirical.rs` | `use syncplay_chat::{...}`, `use syncplay_clock::PlaybackState`, `use syncplay_core::PlayerCoordinator`, `use syncplay_ui::PlayerCommand` |
| `syncplay-core` | `crates/syncplay-core/tests/core_adversarial_challenge.rs` | `use syncplay_chat::{...}`, `use syncplay_clock::PlaybackState`, `use syncplay_core::{...}`, `use syncplay_sync::{...}`, `use syncplay_ui::{...}` |
| `syncplay-core` | `crates/syncplay-core/tests/e2e_phase5.rs` | `#[path = "../../../tests/e2e_phase5/mod.rs"] mod e2e_phase5;` (Links to `tests/e2e_phase5/`) |
| `syncplay-core` | `crates/syncplay-core/tests/party_chat_integration.rs` | `use syncplay_chat::{...}`, `use syncplay_sync::{...}`, `use syncplay_clock::{...}`, `use syncplay_codecs::VideoFrameRGBA`, `use syncplay_core::{...}` |
| `syncplay-core` | `crates/syncplay-core/tests/unpause_gate_and_peer_diagnostics_integration.rs` | `use syncplay_chat::{...}`, `use syncplay_sync::{...}`, `use syncplay_ui::{...}` |
| `syncplay-demux` | `crates/syncplay-demux/tests/demux_tests.rs` | `use syncplay_demux::{...}`, `use syncplay_common::{...}` |
| `syncplay-demux` | `crates/syncplay-demux/tests/ebml_adversarial_challenge.rs` | `use syncplay_demux::{...}` |
| `syncplay-demux` | `crates/syncplay-demux/tests/isobmff_adversarial_challenge.rs` | `use syncplay_demux::{...}` |
| `syncplay-demux` | `crates/syncplay-demux/tests/universal_adversarial_challenge.rs` | `use syncplay_demux::{...}` |
| `syncplay-demux` | `crates/syncplay-demux/tests/universal_demux_tests.rs` | `use syncplay_demux::{...}` |
| `syncplay-sync` | `crates/syncplay-sync/tests/chat_transport_adversarial.rs` | `use syncplay_sync::{...}`, `use syncplay_common::{...}` |
| `syncplay-sync` | `crates/syncplay-sync/tests/multi_client_watch_party_chat.rs` | `use syncplay_sync::{...}`, `use syncplay_common::{...}` |
| `syncplay-sync` | `crates/syncplay-sync/tests/peer_status_test.rs` | `use syncplay_sync::{...}` |
| `syncplay-sync` | `crates/syncplay-sync/tests/sync_adversarial_challenge.rs` | `use syncplay_sync::{...}`, `use syncplay_clock::{...}` |
| `syncplay-sync` | `crates/syncplay-sync/tests/watch_party_transport.rs` | `use syncplay_sync::{...}` |
| `syncplay-ui` | `crates/syncplay-ui/tests/adversarial_challenge.rs` | `use syncplay_ui::{...}` |
| `syncplay-ui` | `crates/syncplay-ui/tests/viewport_debouncer_empirical.rs` | `use syncplay_ui::{...}` |

*(Note: `syncplay-common` has no external `tests/` directory; its tests reside inside `src/` modules).*

### 1.2 End-to-End Test Suite in `tests/e2e_phase5/` (6 Files, 126 Tests)
Exported and run via `crates/syncplay-core/tests/e2e_phase5.rs`:
- `tests/e2e_phase5/mod.rs`: Root module re-exporting tiers and models.
- `tests/e2e_phase5/contract_models.rs`: Specification contracts (`RoomAnnouncement`, `format_duration_hms`).
- `tests/e2e_phase5/tier1_feature_coverage.rs`: 90 unit/integration tests across 18 features:
  - Lines 8-18: `use syncplay_chat::{...}`, `use syncplay_sync::wire::{...}`, `use syncplay_ui::gui::updater::{...}`.
  - Lines 30, 45, 52-54, 65, 171: Test fixtures containing `"syncplay-alt-linux.tar.gz"`, `"syncplay-alt-v0.1.2-windows.exe"`, `"https://github.com/Gauthampro7/syncplay-desktop-preview/releases/tag/v0.1.2"`.
- `tests/e2e_phase5/tier2_boundary_corner.rs`: 25 boundary tests. Imports `syncplay_sync::wire::{...}`, `syncplay_ui::gui::updater::{...}`.
- `tests/e2e_phase5/tier3_cross_feature.rs`: 5 cross-feature tests. Imports `syncplay_chat::{...}`, `syncplay_sync::wire::{...}`, `syncplay_ui::{...}`. Lines 137-138 test asset names `"syncplay-alt-linux.tar.gz"`.
- `tests/e2e_phase5/tier4_scenarios.rs`: 5 application scenarios. Lines 192-201 test GitHub release asset URLs and release notes for `"syncplay-alt-linux-x86_64.tar.gz"` and `"syncplay-alt-windows-x64.exe"`.

### 1.3 In-Tree Unit Test Modules (`#[cfg(test)]`) Across All Crates
- `crates/syncplay-chat/src/announcements.rs:229`: imports `syncplay_common::Crc32`, creates `syncplay_chat.bin` in temp directory.
- `crates/syncplay-chat/src/commands.rs:481`: tests slash commands including `/sync-status` which prints `"=== Syncplay-Alt Sync Status ==="`.
- `crates/syncplay-chat/src/export.rs:64`: tests transcript markdown exporter asserting `"# Syncplay-Alt Room Chat Transcript"`.
- `crates/syncplay-chat/src/record.rs:317`: tests binary record serialization (`HEADER_MAGIC = *b"SPCH"`, `0xAA55` sync word).
- `crates/syncplay-chat/src/storage.rs:254`: tests recovery scanner on `syncplay_chat.bin`.
- `crates/syncplay-clock/src/audio_pacer.rs:297`, `master_clock.rs:239`, `reorder.rs:93`, `spsc.rs:143`.
- `crates/syncplay-codecs/src/audio/resampler.rs:246`, `audio/universal.rs:335`, `color.rs:645`, `frame.rs:254`, `universal_video.rs:421`.
- `crates/syncplay-common/src/crc.rs:46`, `path.rs:67` (URI parsing and normalization), `reader.rs:306`, `time.rs:129`.
- `crates/syncplay-core/src/aggregator.rs:65`, `audio_sink.rs:423`, `coordinator.rs:708`.
- `crates/syncplay-demux/src/ebml.rs:1068`, `isobmff.rs:1094`, `vint.rs:72`.
- `crates/syncplay-sync/src/barrier.rs:98`, `ntp.rs:162`, `pll.rs:120`, `timeline.rs:41`, `transport.rs:871`, `wire.rs:737`.
- `crates/syncplay-ui/src/debouncer.rs:125`, `gui/controls.rs:552`, `gui/recent_files.rs:78`, `gui/sidebar.rs:737`, `gui/updater.rs:346`, `gui/viewport.rs:327`, `osd.rs:131`, `overlay.rs:201`, `render.rs:183`, `scrubber.rs:60`, `snapshot.rs:214`, `terminal.rs:327`.

### 1.4 Test Fixtures and Synthetic Generators
- In-memory synthetic containers in `crates/syncplay-demux/src/synthetic.rs`:
  - `build_synthetic_full_mkv()`: 48-byte EBML Header + SeekHead + Info + Tracks (H.264 + AAC) + Cluster SimpleBlocks.
  - `build_synthetic_full_mp4()`: 132-byte `ftyp` + `moov` (mvhd, trak, mdia, minf, stbl) + `mdat`.
- `SYNTHETIC_EBML_VECTOR`: 48-byte byte array shared across Python test harnesses (`test_demux_codecs_clock.py`, `test_desktop_packaging.py`, `test_remediation_m4_adversarial.py`).

---

## 2. Empirical Challenge Suite Mapping (`tests/empirical_challenge/`)

There are 25 Python test suites in `tests/empirical_challenge/`.

### 2.1 The 4 Mandatory Empirical Challenge Scripts (Acceptance Criteria)

1. **`test_demux_codecs_clock.py`**:
   - Focus: Mathematical invariant validation for container parsing (EBML vint octet width 1-8, ISOBMFF 64-bit largesize bounds), 16-byte row stride alignment, SIMD tail padding, atomic SeqLock rate-shift continuity, SPSC ring-buffer wrapping counters, and Q8 fixed-point color conversion.
   - Invocation: Self-contained pure Python simulation. Zero hardcoded binary paths or external process calls.

2. **`test_sync_engine.py`**:
   - Focus: RFC 5905 sliding-window minimum RTT filtering with $D_{\text{effective}} \ge 5.0\text{ms}$ dispersion floor, discrete PI PLL speed steering ($K_p = 0.17, K_i = 0.0072$) under Tri-Zone drift policy, and two-phase barrier unpause scheduled arrival margin $\ge 100\text{ms}$.
   - Invocation: Self-contained pure Python simulation. Zero hardcoded binary paths or external process calls.

3. **`test_smart_chat.py`**:
   - Focus: Binary log layout validation (magic `b"SPCH"`, version 1, CRC32, `0xAA55` framing), forward false-sync candidate recovery, and Unicode inverted index FTS (Turkish dotted I case folding, punctuation preservation `#`/`@`, CJK unigram + overlapping bigrams).
   - Invariant Alert: Line 24 defines `HEADER_MAGIC = b"SPCH"`. The in-tree Rust crate `crates/syncplay-chat/src/record.rs:8` also defines `pub const HEADER_MAGIC: [u8; 4] = *b"SPCH";`. If magic is preserved as `SPCH` (or synchronized to `SVCH`), both files must agree.

4. **`test_ui_renderer.py`**:
   - Focus: Viewport geometry aspect ratio preservation (letterboxing / pillarboxing calculations across 10 boundary resolutions), 10Hz scrubber debouncer rate-limiting, and memory bounds (Alpha8 4MB font atlas, 5-bubble chat overlay, $\le 15\text{MB}$ RAM).
   - Invocation: Pure Python simulation.

### 2.2 Empirical Challenge Scripts Invoking Cargo or the Binary Target

| Script | How it Invokes Binary / Workspace | References to Check/Update on Rename |
|---|---|---|
| `test_core_adversarial_p2.py` | Line 41: `BINARY_PATH = os.path.abspath("target/debug/syncplay-alt")`<br>Line 68: `assert "SYNCPLAY-ALT" in out`<br>Line 79: `assert "syncplay-alt v0.1.1" in out`<br>Line 204: `chat_log = "syncplay_chat.bin"`<br>Line 218: `assert "No chat log found at syncplay_chat.bin. 0 hits." in out`<br>Line 289: `cargo build --bin syncplay-alt` | Binary path `syncplay-alt` $\to$ `syncverse`<br>Output assertions `"SYNCPLAY-ALT"`, `"syncplay-alt v0.1.1"` |
| `test_desktop_packaging.py` | Line 20: `DESKTOP_PATH = ROOT / "assets" / "syncplay-alt.desktop"`<br>Line 21: `SVG_PATH = ROOT / "assets" / "syncplay-alt.svg"`<br>Line 24: `USER_BIN_PATH = Path.home() / ".local" / "bin" / "syncplay-alt"`<br>Line 46: `("Icon", "syncplay-alt")`<br>Line 52: `assert "Exec=syncplay-alt play %U" in content`<br>Line 80: `assert "syncplay-alt" in res.stdout.lower()`<br>Line 90: `theme.has_icon("syncplay-alt")`<br>Line 100: `target/release/syncplay-alt`<br>Line 103: `target/debug/syncplay-alt`<br>Line 115: `cargo run -q -p syncplay-core --` | All file paths and assertions must match `syncverse.desktop`, `syncverse.svg`, `syncverse` executable, `syncverse-core` package |
| `test_milestone5_integration_stress.py` | Line 30: `BINARY_PATH = ROOT / "target" / "debug" / "syncplay-alt"`<br>Spawns subprocesses: `syncplay-alt party host` and `party connect` | Binary path `syncplay-alt` $\to$ `syncverse` |
| `test_remediation_m4_adversarial.py` | Line 34: `USER_BIN = Path.home() / ".local" / "bin" / "syncplay-alt"`<br>Line 47: `target/release/syncplay-alt`<br>Line 50: `target/debug/syncplay-alt`<br>Line 70: `os.execl(bin_path, "syncplay-alt", "play", target)`<br>Line 232: `~/.local/share/applications/syncplay-alt.desktop`<br>Line 254: `theme.has_icon("syncplay-alt")` | Binary paths, desktop filename, icon name |
| `test_phase5_adversarial_verification.py` | Lines 326-345: Executes `cargo test --test adversarial_phase5_challenge`, `cargo test --test e2e_phase5`, `cargo test -p syncplay-sync --test peer_status_test` | Package name `syncplay-sync` $\to$ `syncverse-sync` |
| `test_phase5_concurrency_debouncing_pacer.py` | Lines 295-345: Executes cargo tests | Update if `-p syncplay-*` arguments are passed |
| `test_audio_pacer_sink_resilience.py` | Line 99: `cargo test -p syncplay-core` | `syncplay-core` $\to$ `syncverse-core` |
| `test_ebml_adversarial.py` | Line 16: `cargo test -p syncplay-demux --test ebml_adversarial_challenge` | `syncplay-demux` $\to$ `syncverse-demux` |
| `test_sync_adversarial_p2.py` | Line 286: `cargo test -p syncplay-sync -- --nocapture` | `syncplay-sync` $\to$ `syncverse-sync` |
| `test_pi_adversarial_stress.py` | Lines 74, 97: References `/home/gautham/syncplay-alt/scripts/audit_dependencies.sh`<br>Simulates `Cargo.toml` and `Cargo.lock` audits | Path references |

---

## 3. Scripts Inventory (`scripts/`)

| Script Name | Purpose | Syncplay References & Renaming Impacts |
|---|---|---|
| `scripts/audit_dependencies.sh` | Enforces zero monolithic player (`mpv`, `vlc`) and zero database (`sqlite`, `rocksdb`, `diesel`) invariant in `Cargo.toml` and `Cargo.lock`. | Header comments cite `Syncplay-Alt`. Uses `crates/*/Cargo.toml` wildcard, which will automatically pick up `crates/syncverse-*/Cargo.toml`. |
| `scripts/verify_phase1.sh` | 8-stage verification pipeline executing OpenSpec audit, Pi framework check, dependency audit, `cargo check`, `cargo clippy`, `cargo test`, empirical challenges, and `intent_harness.py`. | Title banner `"Syncplay-Alt Phase 1 End-to-End Automated Verification Harness"`. References test script paths. |
| `scripts/verify_phase4.sh` | 8-stage verification pipeline for Phase 4 desktop GUI realignment. | Title banner `"Syncplay-Alt Phase 4 End-to-End Automated Verification Harness"`. |
| `scripts/intent_harness.py` | Automated harness executing verification commands from `.pi/harness/intent_matrix.json`. | Banner and help messages cite `Syncplay-Alt`. Reads `.pi/harness/intent_matrix.json` (which contains `syncplay-core`, `syncplay_chat.bin`, `./scripts/audit_dependencies.sh`). |
| `scripts/verify_intent.sh` | Bash wrapper around `scripts/intent_harness.py`. | Header comments cite `Syncplay-Alt`. |
| `scripts/install_linux.sh` | Builds release binary, installs executable to `~/.local/bin/syncplay-alt`, installs `syncplay-alt.svg` and `syncplay-alt.desktop`, validates with `desktop-file-validate`. | Needs update to install `~/.local/bin/syncverse`, copy `assets/syncverse.svg` and `assets/syncverse.desktop`, and update echo banners. |
| `scripts/install_windows.ps1` | Builds release binary, installs to `$env:LOCALAPPDATA\Programs\Syncplay-Alt\syncplay-alt.exe`, creates `Syncplay-Alt.lnk` desktop shortcut. | Needs update to install to `Programs\Syncverse\syncverse.exe`, create `Syncverse.lnk`, and update descriptive text. |
| `scripts/build_windows_cross.sh` | Cross-compiles for `x86_64-pc-windows-gnu`. | Banner: `"Syncplay-Alt Windows Cross-Compilation"`; binary output: `target/x86_64-pc-windows-gnu/release/syncplay-alt.exe` $\to$ `syncverse.exe`. |
| `scripts/setup_deps.sh` | Provisions `libavcodec`/`clang` sysroot into `.deps/` and creates `.cargo/config.toml`. | Header comments and banners cite `Syncplay-Alt`. Environment variable `SYNCPLAY_DEPS_DIR` $\to$ `SYNCVERSE_DEPS_DIR` (or fallback). |

---

## 4. Desktop Integration & UI Branding Inventory

### 4.1 Desktop Assets & Windows Packaging Files

1. **`assets/syncplay-alt.desktop`** (Should be renamed to `assets/syncverse.desktop`):
   - Line 4: `Name=Syncplay-Alt` $\to$ `Name=Syncverse`
   - Line 7: `Exec=syncplay-alt %U` $\to$ `Exec=syncverse %U`
   - Line 8: `Icon=syncplay-alt` $\to$ `Icon=syncverse`
   - Line 17: `Exec=syncplay-alt party host` $\to$ `Exec=syncverse party host`
   - Line 21: `Exec=syncplay-alt party connect 127.0.0.1:18242` $\to$ `Exec=syncverse party connect 127.0.0.1:18242`
   - Line 25: `Exec=syncplay-alt inspect %U` $\to$ `Exec=syncverse inspect %U`

2. **`assets/syncplay-alt.svg`** (Should be renamed to `assets/syncverse.svg`):
   - Clean XML vector graphic with squircle background, orbital rings, and glowing nodes.

3. **`assets/syncplay-alt.ico`** (Should be renamed to `assets/syncverse.ico`):
   - Windows multi-resolution icon file.

4. **`packaging/windows/installer.iss`**:
   - Line 6: `#define MyAppName "Syncplay Alt"` $\to$ `"Syncverse"`
   - Line 8: `#define MyAppPublisher "Syncplay Alt Project"` $\to$ `"Syncverse Project"`
   - Line 9: `#define MyAppURL "https://github.com/Gauthampro7/syncplay-desktop-preview"` $\to$ `"https://github.com/Gauthampro7/syncverse"`
   - Line 10: `#define MyAppExeName "syncplay-alt.exe"` $\to$ `"syncverse.exe"`
   - Line 11: `#define MyAppAssocPrefix "SyncplayAlt"` $\to$ `"Syncverse"`
   - Line 23: `DefaultDirName={autopf}\SyncplayAlt` $\to` `{autopf}\Syncverse`
   - Line 34, 66: `SetupIconFile=..\..\assets\syncplay-alt.ico` $\to$ `assets\syncverse.ico`
   - Line 44: `OutputBaseFilename=SyncplayAlt-Setup-v{#MyAppVersion}-x64` $\to$ `Syncverse-Setup-v{#MyAppVersion}-x64`
   - Line 60: `Description: "Associate Syncplay Alt with supported media files..."`
   - Line 114: `ValueData: "Play with Syncplay Alt"` $\to$ `"Play with Syncverse"`

5. **`packaging/windows/build_installer.bat` & `packaging/windows/build_installer.ps1`**:
   - Calls `cargo build --release --bin syncplay-alt` $\to$ `cargo build --release --bin syncverse`
   - Executable path `target\release\syncplay-alt.exe` $\to$ `syncverse.exe`

6. **`packaging/windows/syncplay-alt.manifest`** (Rename to `syncverse.manifest`):
   - Line 6: `name="SyncplayAlt.Desktop.Player"` $\to$ `name="Syncverse.Desktop.Player"`
   - Line 9: `<description>Syncplay Alt Standalone Desktop Player</description>` $\to$ `<description>Syncverse Standalone Desktop Player</description>`

7. **`packaging/windows/syncplay-alt.rc`** (Rename to `syncverse.rc`):
   - Line 9: `IDI_APPLICATION ICON "../../assets/syncplay-alt.ico"` $\to$ `assets/syncverse.ico`
   - Line 29: `VALUE "CompanyName", "Syncplay Alt Project"` $\to$ `"Syncverse Project"`
   - Line 30: `VALUE "FileDescription", "Syncplay Alt - Standalone Synchronized Desktop Media Player"`
   - Line 32: `VALUE "InternalName", "syncplay-alt"` $\to$ `"syncverse"`
   - Line 33: `VALUE "LegalCopyright", "Copyright (C) 2026 Syncplay Alt Contributors"` $\to$ `Syncverse Contributors`
   - Line 34: `VALUE "OriginalFilename", "syncplay-alt.exe"` $\to$ `"syncverse.exe"`
   - Line 35: `VALUE "ProductName", "Syncplay Alt"` $\to$ `"Syncverse"`
   - Line 46: `1 24 "syncplay-alt.manifest"` $\to$ `"syncverse.manifest"`

### 4.2 UI Window Titles & Branding in `crates/syncplay-ui/` and `crates/syncplay-core/`

1. **Window Titles & Viewport ID**:
   - `crates/syncplay-ui/src/gui/app.rs:724`: `.with_title("Syncplay-Alt — Desktop Media Player")` $\to$ `"Syncverse — Desktop Media Player"`
   - `crates/syncplay-ui/src/gui/app.rs:732`: `viewport.with_app_id("syncplay-alt")` $\to$ `"syncverse"`
   - `crates/syncplay-ui/src/gui/app.rs:753`: `"Syncplay-Alt — Desktop Media Player"`
   - `crates/syncplay-ui/src/gui/file_dialog.rs:44`: `.set_title("Open Media File — Syncplay-Alt")` $\to$ `"Open Media File — Syncverse"`

2. **Modals, Dialogs & Buttons**:
   - `crates/syncplay-ui/src/gui/app.rs:582`: `ui.button("ℹ About Syncplay-Alt")` $\to$ `"ℹ About Syncverse"`
   - `crates/syncplay-ui/src/gui/dialogs.rs:444`: `egui::Window::new("ℹ About Syncplay-Alt")` $\to$ `"ℹ About Syncverse"`
   - `crates/syncplay-ui/src/gui/dialogs.rs:453`: `ui.heading("Syncplay-Alt")` $\to$ `"Syncverse"`

3. **Placeholder & Notification Messages**:
   - `crates/syncplay-ui/src/gui/viewport.rs:310`: `"Syncplay-Alt\nDrop video file here or click Open File"` $\to$ `"Syncverse\nDrop video file here or click Open File"`
   - `crates/syncplay-ui/src/gui/app.rs:194`: `format!("Syncplay-Alt v{} is up to date!", current_version)` $\to$ `format!("Syncverse v{} is up to date!", current_version)`

4. **Recent Files & Cache Directories**:
   - `crates/syncplay-ui/src/gui/recent_files.rs:74`: `base.join("syncplay-alt").join("recent_files.txt")` $\to$ `base.join("syncverse").join("recent_files.txt")`

5. **Auto-Updater Configuration**:
   - `crates/syncplay-ui/src/gui/updater.rs:13`: `"https://api.github.com/repos/Gauthampro7/syncplay-desktop-preview/releases/latest"` $\to$ `"https://api.github.com/repos/Gauthampro7/syncverse/releases/latest"`
   - `crates/syncplay-ui/src/gui/updater.rs:228`: `let user_agent = format!("syncplay-alt/{}", current_version);` $\to$ `format!("syncverse/{}", current_version)`
   - `crates/syncplay-ui/src/gui/updater.rs:285`: `.name("syncplay-updater".into())` $\to$ `.name("syncverse-updater".into())`

6. **CLI & Core Binary Strings (`crates/syncplay-core/src/`)**:
   - `crates/syncplay-core/src/main.rs:21`: `[[bin]] name = "syncplay-alt"` in `Cargo.toml` $\to$ `name = "syncverse"`
   - `crates/syncplay-core/src/main.rs:39`: `println!("syncplay-alt v0.1.1 (pure-Rust standalone player, VLC-grade desktop UI)");` $\to$ `"syncverse v0.1.1..."`
   - `crates/syncplay-core/src/main.rs:129`: `println!("\x1b[1;36m[GUI]\x1b[0m Launching Syncplay-Alt Desktop Graphical Player...");`
   - `crates/syncplay-core/src/main.rs:152`: `SYNCPLAY-ALT — Standalone High-Precision Media Player & Watch Party Engine`
   - `crates/syncplay-core/src/main.rs:160-172`: CLI usage lines `syncplay-alt <COMMAND>`, `syncplay-alt inspect`, etc. $\to$ `syncverse <COMMAND>`
   - `crates/syncplay-core/src/interactive.rs:489`: `" SYNCPLAY-ALT — Target: {} ({})"` $\to$ `" SYNCVERSE — Target: {} ({})"`
   - `crates/syncplay-core/src/playback_runtime.rs:1327, 1340`: Worker thread names `"syncplay-audio-worker"` and `"syncplay-playback-worker"` $\to$ `"syncverse-audio-worker"`, `"syncverse-playback-worker"`

7. **Struct & Symbol Names**:
   - `SyncplayApp` in `crates/syncplay-ui/src/gui/app.rs:47` can optionally be renamed to `SyncverseApp` (or kept as type alias for seamless compatibility).

---

## 5. Summary of Crate Directory and Package Renaming Map

| Current Crate Directory | Current Cargo Package Name | Target Crate Directory | Target Cargo Package Name |
|---|---|---|---|
| `crates/syncplay-common` | `syncplay-common` | `crates/syncverse-common` | `syncverse-common` |
| `crates/syncplay-demux` | `syncplay-demux` | `crates/syncverse-demux` | `syncverse-demux` |
| `crates/syncplay-codecs` | `syncplay-codecs` | `crates/syncverse-codecs` | `syncverse-codecs` |
| `crates/syncplay-clock` | `syncplay-clock` | `crates/syncverse-clock` | `syncverse-clock` |
| `crates/syncplay-sync` | `syncplay-sync` | `crates/syncverse-sync` | `syncverse-sync` |
| `crates/syncplay-chat` | `syncplay-chat` | `crates/syncverse-chat` | `syncverse-chat` |
| `crates/syncplay-ui` | `syncplay-ui` | `crates/syncverse-ui` | `syncverse-ui` |
| `crates/syncplay-core` | `syncplay-core` (bin: `syncplay-alt`) | `crates/syncverse-core` | `syncverse-core` (bin: `syncverse`) |
