# Comprehensive Workspace Survey & Renaming Analysis
**Explorer**: `explorer_survey_m1_1`  
**Date**: 2026-10-27  
**Mission**: Survey and map workspace crate directories, Cargo manifests, Rust source code imports/symbols, non-Rust assets, and compilation/lint status for full project renaming from **Syncplay-Alt** to **Syncverse** under the MIT License.

---

## 1. Executive Summary

The workspace currently consists of **8 interconnected Rust crates** in `crates/syncplay-*`, 1 root `Cargo.toml`, 129 Rust source files (units and integration tests), multiple empirical Python challenge suites, cross-platform packaging scripts, and OpenSpec documentation.

### Core Findings Matrix
| Component | Current State | Target State | Notes |
| :--- | :--- | :--- | :--- |
| **Crate Directories** | 8 directories named `crates/syncplay-*` | 8 directories named `crates/syncverse-*` | Directory rename + Cargo path updates |
| **Cargo Packages** | `syncplay-common`, `syncplay-demux`, `syncplay-codecs`, `syncplay-clock`, `syncplay-sync`, `syncplay-chat`, `syncplay-ui`, `syncplay-core` | `syncverse-common`, `syncverse-demux`, `syncverse-codecs`, `syncverse-clock`, `syncverse-sync`, `syncverse-chat`, `syncverse-ui`, `syncverse-core` | Full package name update across all manifests |
| **Binary Target** | `[[bin]] name = "syncplay-alt"` in `syncplay-core` | `[[bin]] name = "syncverse"` in `syncverse-core` | Binary executable produces `target/debug/syncverse` |
| **License** | `MIT OR Apache-2.0` in manifests, no `LICENSE` file | MIT License in new `LICENSE` file and all manifests | Strict MIT licensing per R3 |
| **Compilation** | Zero warnings under `#![deny(warnings)]` | Zero warnings under `#![deny(warnings)]` | Verified via `cargo check` & `cargo clippy` |
| **Test Suites** | 100% pass across all cargo tests and python harnesses | 100% pass across all cargo tests and python harnesses | Verified via test runner |
| **Rust Source Files** | 76 files with 340 `syncplay` references | All references updated to `syncverse` | 152 imports, 49 code paths, 61 literals |
| **Git Remote** | `https://github.com/Gauthampro7/syncplay-desktop-preview.git` | `https://github.com/Gauthampro7/syncverse.git` | GitHub CLI (`gh`) already authenticated |

---

## 2. Crate Directory Mapping & Dependency Topology

### 2.1 Crate Directory Mapping Table
The workspace contains exactly 8 crates located under `crates/`:

| Current Directory | Target Directory | Current Package Name | Target Package Name | Library Symbol (`use ...`) |
| :--- | :--- | :--- | :--- | :--- |
| `crates/syncplay-common` | `crates/syncverse-common` | `syncplay-common` | `syncverse-common` | `syncverse_common` |
| `crates/syncplay-demux` | `crates/syncverse-demux` | `syncplay-demux` | `syncverse-demux` | `syncverse_demux` |
| `crates/syncplay-codecs` | `crates/syncverse-codecs` | `syncplay-codecs` | `syncverse-codecs` | `syncverse_codecs` |
| `crates/syncplay-clock` | `crates/syncverse-clock` | `syncplay-clock` | `syncverse-clock` | `syncverse_clock` |
| `crates/syncplay-sync` | `crates/syncverse-sync` | `syncplay-sync` | `syncverse-sync` | `syncverse_sync` |
| `crates/syncplay-chat` | `crates/syncverse-chat` | `syncplay-chat` | `syncverse-chat` | `syncverse_chat` |
| `crates/syncplay-ui` | `crates/syncverse-ui` | `syncplay-ui` | `syncverse-ui` | `syncverse_ui` |
| `crates/syncplay-core` | `crates/syncverse-core` | `syncplay-core` | `syncverse-core` | `syncverse_core` |

### 2.2 Dependency Acyclic Directed Graph (DAG)
The crates form a clean, acyclic dependency topology:

```
syncverse-common (Root foundation: MediaTime, WallTime, CRC32, BitReader)
├── syncverse-demux (Universal ffmpeg-next demuxer, EBML, ISOBMFF)
├── syncverse-codecs (Universal video/audio decoders, YUV420p, RGBA, PCM, Q8)
│   └── syncverse-clock (depends on common, codecs: MasterClock, SPSC, AudioPacer)
│       └── syncverse-sync (depends on common, clock: NTP, PI PLL, barrier unpause)
│           └── syncverse-chat (depends on common, sync: binary log, FTS index, slash commands)
└── syncverse-ui (depends on common, codecs, clock, chat: eframe/egui GUI, viewport, sidebar)
    └── syncverse-core (Coordinator & Main Binary: integrates all 7 sub-crates)
```

---

## 3. Cargo Manifest Mapping (`Cargo.toml`)

### 3.1 Root `Cargo.toml`
- **Location**: `/home/gautham/syncplay-alt/Cargo.toml`
- **Current Content**:
```toml
[workspace]
members = [
    "crates/syncplay-common",
    "crates/syncplay-demux",
    "crates/syncplay-codecs",
    "crates/syncplay-clock",
    "crates/syncplay-sync",
    "crates/syncplay-chat",
    "crates/syncplay-ui",
    "crates/syncplay-core",
]
resolver = "2"

[workspace.package]
version = "0.1.1"
edition = "2021"
authors = ["Gautham <gautham@syncplay-alt.internal>"]
license = "MIT OR Apache-2.0"

[workspace.dependencies]
thiserror = "2.0"
ffmpeg-next = "9.0.0"
```
- **Required Changes**:
  1. Rename all 8 members from `"crates/syncplay-*"` to `"crates/syncverse-*"`.
  2. Update `authors`: change email domain if appropriate (e.g., `gautham@syncverse.internal` or personal).
  3. Update `license`: change `"MIT OR Apache-2.0"` to `"MIT"`.
  4. Add `repository = "https://github.com/Gauthampro7/syncverse"`.
  5. Add `description = "Ultra-efficient standalone desktop media player and long-distance synchronized watch-party platform in pure Rust"`.

### 3.2 Individual Crate `Cargo.toml` Manifests

#### 1. `crates/syncplay-common/Cargo.toml`
- **Current**: `name = "syncplay-common"`
- **Target**: `name = "syncverse-common"`, `license.workspace = true`

#### 2. `crates/syncplay-demux/Cargo.toml`
- **Current**:
  - `name = "syncplay-demux"`
  - `syncplay-common = { path = "../syncplay-common" }`
- **Target**:
  - `name = "syncverse-demux"`
  - `syncverse-common = { path = "../syncverse-common" }`
- **Hygiene Note**: Note that `syncplay-common` is declared as a dependency but its types are not directly invoked in `syncplay-demux/src/`. Consider keeping it if needed for trait parity or removing it during R2 cleanup.

#### 3. `crates/syncplay-codecs/Cargo.toml`
- **Current**:
  - `name = "syncplay-codecs"`
  - `syncplay-common = { path = "../syncplay-common" }`
- **Target**:
  - `name = "syncverse-codecs"`
  - `syncverse-common = { path = "../syncverse-common" }`

#### 4. `crates/syncplay-clock/Cargo.toml`
- **Current**:
  - `name = "syncplay-clock"`
  - `syncplay-common = { path = "../syncplay-common" }`
  - `syncplay-codecs = { path = "../syncplay-codecs" }`
- **Target**:
  - `name = "syncverse-clock"`
  - `syncverse-common = { path = "../syncverse-common" }`
  - `syncverse-codecs = { path = "../syncverse-codecs" }`
- **Hygiene Note**: `syncplay-common` is listed but not directly imported in `syncplay-clock/src/`.

#### 5. `crates/syncplay-sync/Cargo.toml`
- **Current**:
  - `name = "syncplay-sync"`
  - `syncplay-common = { path = "../syncplay-common" }`
  - `syncplay-clock = { path = "../syncplay-clock" }`
- **Target**:
  - `name = "syncverse-sync"`
  - `syncverse-common = { path = "../syncverse-common" }`
  - `syncverse-clock = { path = "../syncverse-clock" }`
- **Hygiene Note**: `syncplay-clock` is declared but not imported in `syncplay-sync/src/`.

#### 6. `crates/syncplay-chat/Cargo.toml`
- **Current**:
  - `name = "syncplay-chat"`
  - `syncplay-common = { path = "../syncplay-common" }`
  - `syncplay-sync = { path = "../syncplay-sync" }`
- **Target**:
  - `name = "syncverse-chat"`
  - `syncverse-common = { path = "../syncverse-common" }`
  - `syncverse-sync = { path = "../syncverse-sync" }`

#### 7. `crates/syncplay-ui/Cargo.toml`
- **Current**:
  - `name = "syncplay-ui"`
  - `syncplay-common = { path = "../syncplay-common" }`
  - `syncplay-codecs = { path = "../syncplay-codecs" }`
  - `syncplay-clock = { path = "../syncplay-clock" }`
  - `syncplay-chat = { path = "../syncplay-chat" }`
- **Target**:
  - `name = "syncverse-ui"`
  - `syncverse-common = { path = "../syncverse-common" }`
  - `syncverse-codecs = { path = "../syncverse-codecs" }`
  - `syncverse-clock = { path = "../syncverse-clock" }`
  - `syncverse-chat = { path = "../syncverse-chat" }`
- **Hygiene Note**: `syncplay-common` is listed in `[dependencies]` but not imported in `syncplay-ui/src/`.

#### 8. `crates/syncplay-core/Cargo.toml`
- **Current**:
  - `name = "syncplay-core"`
  - Dependencies: `syncplay-common`, `syncplay-demux`, `syncplay-codecs`, `syncplay-clock`, `syncplay-sync`, `syncplay-chat`, `syncplay-ui`
  - `[[bin]] name = "syncplay-alt"`
  - `path = "src/main.rs"`
- **Target**:
  - `name = "syncverse-core"`
  - Dependencies: updated to `syncverse-*`
  - `[[bin]] name = "syncverse"`
  - `path = "src/main.rs"`

---

## 4. Rust Source Code Survey (`syncplay` Occurrences)

A comprehensive automated scan of the 129 Rust source files in `crates/` and `tests/` identified **76 distinct files** containing **340 occurrences** of `syncplay` / `syncplay_`.

### 4.1 Categorization of Occurrences
1. **`use syncplay_*` statements (152 occurrences)**:
   - Direct crate imports across `src/` modules and `tests/`.
   - Examples: `use syncplay_common::Crc32;`, `use syncplay_chat::{ChatRecord, ChatStore};`, `use syncplay_ui::PlayerUiSnapshot;`.
2. **Fully-qualified code symbols & paths (49 occurrences)**:
   - Path references like `syncplay_sync::SyncError::InvalidMagic(_)`, `syncplay_chat::make_file_header(0)`, `syncplay_common::Crc32::digest(...)`.
3. **String Literals, File Paths, URLs, Identifiers (61 occurrences)**:
   - Window title: `.with_title("Syncplay-Alt — Desktop Media Player")` (`crates/syncplay-ui/src/gui/app.rs:724`).
   - App ID: `.with_app_id("syncplay-alt")` (`crates/syncplay-ui/src/gui/app.rs:732`).
   - Config directory: `base.join("syncplay-alt").join("recent_files.txt")` (`crates/syncplay-ui/src/gui/recent_files.rs:74`).
   - GitHub Releases API & User-Agent: `"https://api.github.com/repos/Gauthampro7/syncplay-desktop-preview/releases/latest"` and `format!("syncplay-alt/{}", current_version)` (`crates/syncplay-ui/src/gui/updater.rs:13,228`).
   - Thread name: `.name("syncplay-updater".into())` (`crates/syncplay-ui/src/gui/updater.rs:285`).
   - CLI banner, usage, and version strings in `crates/syncplay-core/src/main.rs:27-173`.
   - Binary test log filename: `syncplay_chat.bin` (or tests using `syncplay_test_store_*.bin`).
4. **Documentation Comments (50 occurrences)**:
   - Module doc comments (`//! ... syncplay-...`) and item doc comments (`/// ...`).
5. **Other Expressions & Variable Names (28 occurrences)**:
   - Variable names, formatting macros, and test assertions.

### 4.2 Breakdown by Crate
| Crate / Area | Affected Files | Total Matches | Key Files & Descriptions |
| :--- | :--- | :--- | :--- |
| **`syncplay-chat`** | 8 | 28 | `src/announcements.rs`, `src/record.rs`, `src/storage.rs`, `src/export.rs`, and 3 test suites |
| **`syncplay-clock`** | 3 | 4 | `src/audio_pacer.rs`, `src/error.rs`, `tests/adversarial_clock_stress.rs` |
| **`syncplay-codecs`** | 15 | 36 | `src/audio/universal.rs`, `src/error.rs`, `src/h264/*`, and 9 test suites |
| **`syncplay-common`** | 3 | 3 | `src/error.rs`, `src/lib.rs`, `src/path.rs` (module doc comments only) |
| **`syncplay-core`** | 19 | 156 | `src/main.rs` (28 matches), `src/coordinator.rs` (11), `src/playback_runtime.rs` (9), `src/error.rs` (9), and 12 integration test suites |
| **`syncplay-demux`** | 5 | 6 | `tests/demux_tests.rs`, `tests/ebml_adversarial_challenge.rs`, `tests/universal_demux_tests.rs`, etc. |
| **`syncplay-sync`** | 7 | 28 | `src/wire.rs`, `src/lib.rs`, and 5 test suites |
| **`syncplay-ui`** | 12 | 47 | `src/gui/updater.rs` (17), `src/gui/app.rs` (13), `src/gui/recent_files.rs` (2), `src/gui/sidebar.rs` (2), and 2 test suites |
| **`tests/`** | 4 | 32 | `tests/e2e_phase5/tier1_feature_coverage.rs` (15), `tier3_cross_feature.rs` (7), `tier4_scenarios.rs` (8), `tier2_boundary_corner.rs` (2) |
| **Total** | **76** | **340** | |

### 4.3 High-Risk / Critical Rust Code References
The survey identified several places where renaming requires special care to prevent compilation or runtime breakage:

1. **`env!("CARGO_BIN_EXE_syncplay-alt")`**  
   - **File**: `crates/syncplay-core/tests/party_chat_integration.rs:120`
   - **Impact**: Cargo automatically generates environment variables for binary targets named `CARGO_BIN_EXE_<bin_name>`. When the binary is renamed in `Cargo.toml` to `syncverse`, this macro MUST be updated to `env!("CARGO_BIN_EXE_syncverse")` or compilation will immediately fail.
2. **`resolve_binary_path()` & CLI Assertions**  
   - **File**: `crates/syncplay-core/tests/commands_chat_cli_empirical.rs:436,464,473,482`
   - **Impact**: Resolves binary using `path.push("syncplay-alt")` and asserts `stdout.contains("syncplay-alt v0.1.1")` and `stdout.contains("SYNCPLAY-ALT")`. These must be coordinated with `main.rs` changes to `syncverse`.
3. **Application ID & Window Title in `syncplay-ui`**  
   - **File**: `crates/syncplay-ui/src/gui/app.rs:724,732`
   - **Impact**: Sets Wayland/X11 `app_id` to `"syncplay-alt"` and window title to `"Syncplay-Alt — Desktop Media Player"`. Needs update to `"syncverse"` and `"Syncverse — Desktop Media Player"`.
4. **Recent Media Config Directory**  
   - **File**: `crates/syncplay-ui/src/gui/recent_files.rs:74`
   - **Impact**: Resolves `$XDG_CONFIG_HOME/syncplay-alt/recent_files.txt`. Should resolve `$XDG_CONFIG_HOME/syncverse/recent_files.txt` with backward-compatible migration fallback.
5. **Updater API Endpoint & User-Agent**  
   - **File**: `crates/syncplay-ui/src/gui/updater.rs:13,228,285`
   - **Impact**: `GITHUB_RELEASES_URL` points to `Gauthampro7/syncplay-desktop-preview`. User-Agent is `syncplay-alt/{}`. Thread name is `syncplay-updater`. Updating to `Gauthampro7/syncverse` and `syncverse/{}`.
6. **Cross-crate Test Path Inclusion**  
   - **File**: `crates/syncplay-core/tests/e2e_phase5.rs:11`
   - **Impact**: Uses `#[path = "../../../tests/e2e_phase5/mod.rs"] mod e2e_phase5;`. Since directory renaming changes `crates/syncplay-core` to `crates/syncverse-core`, the relative depth is preserved (3 levels up), so the relative path remains intact.

---

## 5. Non-Rust Files, Packaging, Documentation, and Git

### 5.1 Assets & Desktop Integration
- **`assets/syncplay-alt.desktop`**:
  - Name: `Syncplay-Alt` -> `Syncverse`
  - Exec: `syncplay-alt %U` -> `syncverse %U`
  - Icon: `syncplay-alt` -> `syncverse`
  - Target file name: `assets/syncverse.desktop`
- **`assets/syncplay-alt.svg`** & **`assets/syncplay-alt.ico`**:
  - Rename to `assets/syncverse.svg` and `assets/syncverse.ico`.

### 5.2 Windows Packaging & Resources
- **`packaging/windows/syncplay-alt.manifest`** -> `syncverse.manifest`
  - Name: `SyncplayAlt.Desktop.Player` -> `Syncverse.Desktop.Player`
  - Description: `Syncplay Alt Standalone Desktop Player` -> `Syncverse Standalone Desktop Player`
- **`packaging/windows/syncplay-alt.rc`** -> `syncverse.rc`
  - Resource script defining `IDI_APPLICATION ICON "../../assets/syncverse.ico"`
  - LegalCopyright: `Copyright (C) 2026 Syncverse Contributors`
  - OriginalFilename: `syncverse.exe`
  - ProductName: `Syncverse`
- **`packaging/windows/installer.iss`**:
  - Inno Setup script definitions: `MyAppName "Syncverse"`, `MyAppExeName "syncverse.exe"`, `MyAppAssocPrefix "Syncverse"`, `DefaultDirName "{autopf}\Syncverse"`.
- **`packaging/windows/build_installer.ps1` & `build_installer.bat`**:
  - Update paths and binary references to `syncverse.exe`.

### 5.3 Scripts
- **`scripts/install_linux.sh`**:
  - Installs to `~/.local/bin/syncverse`, installs `assets/syncverse.desktop` and `assets/syncverse.svg`.
- **`scripts/install_windows.ps1`**:
  - Installs to `$env:LOCALAPPDATA\Programs\Syncverse\syncverse.exe` and creates `Syncverse.lnk`.
- **`scripts/audit_dependencies.sh`**:
  - Uses wildcard `crates/*/Cargo.toml`, so it adapts automatically to renamed crates. Comments should be updated to Syncverse.
- **`scripts/intent_harness.py`**:
  - Validates intent compliance across the codebase.

### 5.4 Licensing and Documentation
- **`LICENSE`**:
  - Currently missing from repository root.
  - Must create `LICENSE` with standard MIT License text (Copyright 2026 Gautham & Syncverse Contributors).
- **`README.md`**:
  - 9 occurrences of `syncplay-alt` and multiple references to `Syncplay-Alt`.
  - Architecture diagram lists `syncplay-*` crates.
  - Needs update to `Syncverse`, `syncverse-*` crates, MIT license badge/section.
- **`openspec/` & `.pi/`**:
  - Specifications (`openspec/specs/*`, `openspec/project.md`) and agent prompts (`.pi/agents/*`, `.pi/harness/intent_matrix.json`) document system architecture and refer to crate names and binary names.

### 5.5 Remote Git Repository (`Gauthampro7`)
- Current remote: `origin https://github.com/Gauthampro7/syncplay-desktop-preview.git`
- GitHub CLI authentication status:
  - Account: `Gauthampro7`
  - Active: `true`
  - Token scopes: `'gist', 'read:org', 'repo', 'workflow'`
- Requirement R4 target:
  - Create or point remote to `syncverse` under `Gauthampro7` and push all commits.

---

## 6. Current Build, Lint, and Test Health Status

A comprehensive build and test execution verified pristine health:

1. **`cargo check --workspace --all-targets`**:
   - Status: **PASSED (0.28s)**
   - Result: 0 warnings, 0 errors under `#![deny(warnings)]`.
2. **`cargo clippy --workspace --all-targets -- -D warnings`**:
   - Status: **PASSED (0.15s)**
   - Result: 0 warnings, 0 errors.
3. **`cargo clippy --workspace --all-targets --all-features`**:
   - Status: **PASSED (1.92s)**
   - Result: 0 warnings across all targets and features.
4. **Code Hygiene & Suppression Attributes**:
   - Searched for `#[allow(dead_code)]`: **0 occurrences**.
   - Searched for `#[allow(unused)]`: **0 occurrences**.
   - Searched for general `#[allow(...)]`: Only **1 occurrence** in entire workspace (`#[allow(clippy::too_many_arguments)]` in `crates/syncplay-ui/src/gui/sidebar.rs:53`).
5. **`cargo test --workspace`**:
   - Status: **PASSED**
   - Result: Over 250 unit, integration, and doc tests executed across all 8 crates with 0 failures, 0 ignored.
6. **`./scripts/audit_dependencies.sh`**:
   - Status: **PASSED**
   - Result: 19 banned monolithic player and external database patterns checked, 0 violations.
7. **Empirical Mathematical Challenge Suites**:
   - `python3 tests/empirical_challenge/test_demux_codecs_clock.py`: **PASSED (0 errors)**.
   - `python3 tests/empirical_challenge/test_sync_engine.py`: **PASSED (0 errors)**.
   - `python3 tests/empirical_challenge/test_smart_chat.py`: **PASSED (0 errors)**.
   - `python3 tests/empirical_challenge/test_ui_renderer.py`: **PASSED (0 errors)**.
8. **Pi Framework Intent Verification Harness (`scripts/intent_harness.py`)**:
   - Status: **PASSED (8/8 intents verified, 100.0% compliance score)**.

---

## 7. Recommended Implementation Sequence

To guarantee atomic transitions with zero broken intermediate builds:

1. **Stage 1 (Licensing & Metadata Preparation)**:
   - Create root `LICENSE` file (MIT License).
   - Update `Cargo.toml` workspace package metadata (`license = "MIT"`, `repository`).
2. **Stage 2 (Crate Manifests & Directory Rename)**:
   - Rename crate directories in `crates/`: `syncplay-*` -> `syncverse-*`.
   - Update package names and path dependencies across all 8 crate `Cargo.toml` manifests.
   - Update `[[bin]] name = "syncverse"` in `crates/syncverse-core/Cargo.toml`.
   - Update root `Cargo.toml` `members = ["crates/syncverse-*", ...]`.
3. **Stage 3 (Rust Source Code Imports & Identifiers)**:
   - Batch rename `use syncplay_*` to `use syncverse_*` across 76 files.
   - Update fully-qualified paths (`syncplay_common::`, `syncplay_chat::`, etc.).
   - Update `env!("CARGO_BIN_EXE_syncplay-alt")` -> `env!("CARGO_BIN_EXE_syncverse")` in `party_chat_integration.rs`.
   - Update test helper `resolve_binary_path()` in `commands_chat_cli_empirical.rs`.
   - Update UI window title and app id in `crates/syncverse-ui/src/gui/app.rs`.
   - Update updater endpoint and User-Agent in `crates/syncverse-ui/src/gui/updater.rs`.
4. **Stage 4 (Assets, Packaging, and Scripts)**:
   - Rename and update `assets/syncverse.desktop`, `assets/syncverse.svg`, `assets/syncverse.ico`.
   - Update `packaging/windows/` files (`syncverse.manifest`, `syncverse.rc`, `installer.iss`).
   - Update `scripts/install_linux.sh` and `scripts/install_windows.ps1`.
   - Update `README.md`, `openspec/`, and test scripts.
5. **Stage 5 (Verification & GitHub Remote Setup)**:
   - Run `cargo check`, `cargo clippy`, `cargo test`, `./scripts/audit_dependencies.sh`, and Python empirical test suites.
   - Create new repository `syncverse` on GitHub via `gh repo create Gauthampro7/syncverse --public --source=. --remote=origin --push` or update remote and push.
