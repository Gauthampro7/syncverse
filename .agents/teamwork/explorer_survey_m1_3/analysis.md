# Comprehensive Workspace Survey: Licensing, Documentation & GitHub Configuration

**Survey Agent**: `explorer_survey_m1_3`  
**Date**: 2026-10-27  
**Workspace**: `/home/gautham/syncplay-alt`  
**Mission**: Survey workspace licensing, documentation references to Syncplay-Alt, and Git / GitHub CLI state for migration to `Syncverse` under the MIT License (R3, R4).

---

## Executive Summary

1. **Licensing**:
   - Currently, there is **no `LICENSE` file** in the repository root.
   - Root `Cargo.toml` specifies dual licensing: `license = "MIT OR Apache-2.0"`.
   - All 8 member crates specify `license.workspace = true` and will seamlessly inherit pure `MIT` once root `Cargo.toml` is updated.
   - Zero source files (`.rs`, `.py`) contain incompatible header notices.
   - Windows resource script (`packaging/windows/syncplay-alt.rc`) contains an old copyright string requiring update.
   - A root `LICENSE` file containing standard MIT terms with copyright year 2026 must be created, and `Cargo.toml` updated to `license = "MIT"`.

2. **Documentation & Codebase References**:
   - Over **1,200 occurrences** of `syncplay` exist across Markdown documentation, Cargo manifests, Windows/Linux packaging files, helper scripts, tests, and user-facing UI strings.
   - All 8 crate directories must be renamed from `crates/syncplay-*` to `crates/syncverse-*`.
   - Key user-facing UI locations (`crates/syncplay-ui/src/gui/app.rs`, `dialogs.rs`, `file_dialog.rs`, `viewport.rs`) contain window titles, about dialogs, and empty viewport texts currently hardcoded to `Syncplay-Alt`.
   - The auto-updater in `crates/syncplay-ui/src/gui/updater.rs` queries `https://api.github.com/repos/Gauthampro7/syncplay-desktop-preview/releases/latest` and expects asset names like `syncplay-alt-linux-*.tar.gz`; both the URL and asset pattern must be updated to `syncverse`.
   - Desktop and Windows installer assets (`assets/syncplay-alt.*`, `packaging/windows/*`) must be renamed and reconfigured.

3. **Git & GitHub CLI Configuration**:
   - Active branch is `main`, up to date with remote `origin` (`https://github.com/Gauthampro7/syncplay-desktop-preview.git`).
   - GitHub CLI (`gh`) is fully authenticated as `Gauthampro7` with OAuth scopes `'gist', 'read:org', 'repo', 'workflow'`, capable of creating repositories and managing remotes.
   - The destination repository `Gauthampro7/syncverse` does **not yet exist** on GitHub.
   - Exact CLI execution plan provided below to create `syncverse`, repoint `origin`, and cleanly push branches and tags.

---

## 1. Licensing Survey (Requirement R3)

### 1.1 Existing License Files
- Inspection of `/home/gautham/syncplay-alt/` confirmed that **no `LICENSE` or `LICENSE-MIT` file exists** in the repository root or any crate subdirectories.
- Only generated rustdoc third-party font license notices exist in `target/doc/static.files/`.

### 1.2 Cargo Manifest License Declarations
Across the entire repository, exactly 9 `Cargo.toml` manifests were inspected:

| Manifest Path | Field Name | Existing Value | Required Action for R3 |
|---|---|---|---|
| `Cargo.toml` (root) | `[workspace.package].license` | `"MIT OR Apache-2.0"` (line 18) | Change to `"MIT"` |
| `Cargo.toml` (root) | `[workspace.package].authors` | `["Gautham <gautham@syncplay-alt.internal>"]` (line 17) | Change to `["Gautham"]` or `["Gautham <gauthampro7@gmail.com>"]` |
| `crates/syncplay-chat/Cargo.toml` | `[package].license.workspace` | `true` (line 6) | Inherits `"MIT"` automatically |
| `crates/syncplay-clock/Cargo.toml` | `[package].license.workspace` | `true` (line 6) | Inherits `"MIT"` automatically |
| `crates/syncplay-codecs/Cargo.toml` | `[package].license.workspace` | `true` (line 6) | Inherits `"MIT"` automatically |
| `crates/syncplay-common/Cargo.toml` | `[package].license.workspace` | `true` (line 6) | Inherits `"MIT"` automatically |
| `crates/syncplay-core/Cargo.toml` | `[package].license.workspace` | `true` (line 6) | Inherits `"MIT"` automatically |
| `crates/syncplay-demux/Cargo.toml` | `[package].license.workspace` | `true` (line 6) | Inherits `"MIT"` automatically |
| `crates/syncplay-sync/Cargo.toml` | `[package].license.workspace` | `true` (line 6) | Inherits `"MIT"` automatically |
| `crates/syncplay-ui/Cargo.toml` | `[package].license.workspace` | `true` (line 6) | Inherits `"MIT"` automatically |

Because all 8 crates use Cargo workspace inheritance (`license.workspace = true`), updating `license = "MIT"` in the root `Cargo.toml` will instantaneously apply to every crate in the workspace.

### 1.3 Header Comments & Metadata
- **Rust (`.rs`) and Python (`.py`) Source Files**: A comprehensive regex search across all source files confirmed **zero** license header notices or conflicting license requirements.
- **Windows Packaging**:
  - `packaging/windows/syncplay-alt.rc:33`:
    ```rc
    VALUE "LegalCopyright", "Copyright (C) 2026 Syncplay Alt Contributors"
    ```
    Must be updated to:
    ```rc
    VALUE "LegalCopyright", "Copyright (c) 2026 Gautham"
    ```
  - `packaging/windows/installer.iss:69-72`:
    ```iss
    ; Documentation & License
    Source: "..\..\README.md"; DestDir: "{app}"; DestName: "README.txt"; Flags: ignoreversion isreadme
    Source: "..\..\AGENTS.md"; DestDir: "{app}"; DestName: "ARCHITECTURE.txt"; Flags: ignoreversion
    ```
    Must add the new `LICENSE` file:
    ```iss
    Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
    ```
    and under `[Setup]`:
    ```iss
    LicenseFile=..\..\LICENSE
    ```

### 1.4 Required MIT License Content
A new file `/home/gautham/syncplay-alt/LICENSE` must be created with the standard MIT text:

```text
MIT License

Copyright (c) 2026 Gautham

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## 2. Documentation & References Survey (Requirement R1, R3)

### 2.1 Primary Markdown Documentation
The following core documentation files contain extensive references to Syncplay-Alt:

1. **`README.md`**:
   - Line 1: `# Syncplay-Alt` -> `# Syncverse`
   - Line 10: `Syncplay-Alt is a standalone desktop media player...` -> `Syncverse...`
   - Lines 23-37: Quick Start CLI commands: `syncplay-alt play ...` -> `syncverse play ...`
   - Lines 60, 70, 73: Watch-party CLI examples: `syncplay-alt party host` -> `syncverse party host`
   - Lines 114, 117, 120, 123: CLI inspect / play / chat / verify examples.
   - Lines 130-140: Directory structure diagram showing `crates/syncplay-*` -> `crates/syncverse-*`.
   - Add new `## 📄 License` section pointing to `LICENSE` under MIT terms.

2. **`AGENTS.md`**:
   - Title: `# AGENTS.md — Autonomous Agent Architecture & Engineering Standards`
   - Line 5: `Syncplay-Alt is an open-source...` -> `Syncverse is an open-source...`
   - Line 26: `Syncplay-Alt MUST execute rendering and playback controls...`
   - Section 3 (Lines 37-88): Crate inventory table and descriptions (`syncplay-common` ... `syncplay-core`).
   - Chat log file references: `syncplay_chat.bin`.

3. **`PROJECT.md`**:
   - Title & § Architecture: `# Project: Syncverse Phase 5...`
   - Line 36: Auto-updater endpoint reference.
   - Subsystem interaction lists and interface contract snippets (`crates/syncplay-*` -> `crates/syncverse-*`).

4. **`TEST_INFRA.md` & `TEST_READY.md`**:
   - Document titles and executive summaries referencing Syncplay-Alt.
   - Integration test directory mappings (`crates/syncplay-core/tests/e2e_phase5.rs` -> `crates/syncverse-core/tests/e2e_phase5.rs`).

5. **`openspec/` Specifications & Changes**:
   - `openspec/project.md`: Master specification (244 lines) defining architectural invariants, crate topology, and algorithms.
   - `openspec/specs/av-pipeline/spec.md`, `codecs/spec.md`, `demuxer/spec.md`, `smart-chat/spec.md`, `sync-engine/spec.md`, `ui-renderer/spec.md`.
   - `openspec/changes/001-demuxer-bitstream.md` through `006-ui-osd-renderer.md`.

6. **`.pi/` Framework Intent Matrix**:
   - `.pi/harness/intent_matrix.json`: INTENT-01 through INTENT-08 descriptions and capabilities.
   - `.pi/workflows/*.md` and `.pi/agents/*.md`.

### 2.2 Desktop Launchers, Packaging & Installers

| File Path | References to Rename | Required Update |
|---|---|---|
| `assets/syncplay-alt.desktop` | Filename & content (`Name=Syncplay-Alt`, `Exec=syncplay-alt`, `Icon=syncplay-alt`) | Rename file to `assets/syncverse.desktop`; update keys to `Syncverse` / `syncverse` |
| `assets/syncplay-alt.svg` | Filename | Rename file to `assets/syncverse.svg` |
| `assets/syncplay-alt.ico` | Filename | Rename file to `assets/syncverse.ico` |
| `packaging/windows/installer.iss` | Lines 6-12: `MyAppName "Syncplay Alt"`, `MyAppURL ".../syncplay-desktop-preview"`, `MyAppExeName "syncplay-alt.exe"`, `MyAppAssocPrefix "SyncplayAlt"` | Update to `Syncverse`, `syncverse.exe`, `https://github.com/Gauthampro7/syncverse` |
| `packaging/windows/syncplay-alt.rc` | Filename & strings: `FileDescription`, `InternalName`, `OriginalFilename`, `ProductName` | Rename file to `syncverse.rc`; update to `Syncverse` |
| `packaging/windows/syncplay-alt.manifest` | Filename & assembly identity `SyncplayAlt.Desktop.Player` | Rename file to `syncverse.manifest`; update to `Syncverse.Desktop.Player` |
| `packaging/windows/build_installer.bat` | Exe name `syncplay-alt.exe` | Update to `syncverse.exe` |
| `packaging/windows/build_installer.ps1` | Exe name `syncplay-alt.exe` | Update to `syncverse.exe` |
| `scripts/install_linux.sh` | Desktop file path, binary install `~/.local/bin/syncplay-alt`, icon paths | Update to `syncverse`, `syncverse.desktop`, `syncverse.svg` |
| `scripts/install_windows.ps1` | Install path `Programs\Syncplay-Alt`, binary `syncverse.exe`, shortcut `Syncverse.lnk` | Update to `Syncverse` |
| `scripts/audit_dependencies.sh` | Header comments and strings | Update to `Syncverse` |
| `scripts/intent_harness.py` | Docstrings and log prefixes | Update to `Syncverse` |

### 2.3 User-Facing UI Strings & Binary Target in Rust Source Code

| File Path | Line Number | Existing Code / String | Required Update |
|---|---|---|---|
| `crates/syncplay-core/Cargo.toml` | Line 21 | `[[bin]] name = "syncplay-alt"` | Change binary name to `"syncverse"` |
| `crates/syncplay-core/src/main.rs` | Line 1, 23 | Doc comments: `Syncplay-Alt` | Change to `Syncverse` |
| `crates/syncplay-core/src/main.rs` | Line 39 | `println!("syncplay-alt v0.1.1...");` | Change to `syncverse v0.1.1...` |
| `crates/syncplay-core/src/main.rs` | Lines 47, 110, 160-172 | CLI help usages (`syncplay-alt <COMMAND>`) | Change to `syncverse <COMMAND>` |
| `crates/syncplay-core/src/main.rs` | Line 129, 152 | ANSI banner: `SYNCPLAY-ALT` | Change to `SYNCVERSE` |
| `crates/syncplay-ui/src/gui/app.rs` | Line 47 | `pub struct SyncplayApp` | Rename to `SyncverseApp` (or keep alias) |
| `crates/syncplay-ui/src/gui/app.rs` | Line 194 | `format!("Syncplay-Alt v{} is up to date!", ...)` | Change to `"Syncverse v{}..."` |
| `crates/syncplay-ui/src/gui/app.rs` | Line 582 | `ui.button("ℹ About Syncplay-Alt")` | Change to `"ℹ About Syncverse"` |
| `crates/syncplay-ui/src/gui/app.rs` | Line 724 | `.with_title("Syncplay-Alt — Desktop Media Player")` | Change to `"Syncverse — Desktop Media Player"` |
| `crates/syncplay-ui/src/gui/app.rs` | Line 732 | `.with_app_id("syncplay-alt")` | Change to `"syncverse"` |
| `crates/syncplay-ui/src/gui/app.rs` | Line 753 | `eframe::run_native("Syncplay-Alt — Desktop Media Player", ...)` | Change to `"Syncverse — Desktop Media Player"` |
| `crates/syncplay-ui/src/gui/dialogs.rs` | Lines 437, 444, 453 | About dialog: `"ℹ About Syncplay-Alt"`, `ui.heading("Syncplay-Alt")` | Change to `"Syncverse"` |
| `crates/syncplay-ui/src/gui/file_dialog.rs` | Line 44 | `.set_title("Open Media File — Syncplay-Alt")` | Change to `"Open Media File — Syncverse"` |
| `crates/syncplay-ui/src/gui/recent_files.rs` | Lines 69, 74 | `base.join("syncplay-alt").join("recent_files.txt")` | Change to `syncverse` |
| `crates/syncplay-ui/src/gui/viewport.rs` | Line 310 | `"Syncplay-Alt\nDrop video file here or click Open File"` | Change to `"Syncverse\nDrop video file here..."` |
| `crates/syncplay-ui/src/gui/updater.rs` | Line 13 | `GITHUB_RELEASES_URL = "https://api.github.com/repos/Gauthampro7/syncplay-desktop-preview/releases/latest"` | Change to `"https://api.github.com/repos/Gauthampro7/syncverse/releases/latest"` |
| `crates/syncplay-ui/src/gui/updater.rs` | Line 228 | `user_agent = format!("syncplay-alt/{}", current_version)` | Change to `"syncverse/{}"` |
| `crates/syncplay-ui/src/gui/updater.rs` | Line 285 | `.name("syncplay-updater".into())` | Change to `"syncverse-updater"` |
| `crates/syncplay-ui/src/gui/updater.rs` | Lines 412, 416, 476, 480 | Mock test release asset names (`syncplay-alt-*.tar.gz`) | Keep aligned with tests |
| `crates/syncplay-chat/src/commands.rs` | Line 318 | `"=== Syncplay-Alt Sync Status ===\n"` | Update to `"=== Syncverse Sync Status ===\n"` |
| `crates/syncplay-chat/src/export.rs` | Line 17 | `"# Syncplay-Alt Room Chat Transcript\n\n"` | Update to `"# Syncverse Room Chat Transcript\n\n"` |
| `crates/syncplay-core/tests/adversarial_phase5_challenge.rs` | Line 452 | Release URL fixture | Update to `syncverse` |
| `tests/e2e_phase5/tier1_feature_coverage.rs` | Lines 30, 45 | Release URL fixture assertions | Update to `syncverse` |
| `tests/e2e_phase5/tier4_scenarios.rs` | Line 192 | Release URL fixture | Update to `syncverse` |

---

## 3. Git State & GitHub CLI Configuration (Requirement R4)

### 3.1 Local Git Repository Status
- **Current Branch**: `main`
- **Working Tree**: Clean on tracked files. Uncommitted untracked items reside solely in `.agents/teamwork/` scratch directories.
- **Current Remotes**:
  ```text
  origin  https://github.com/Gauthampro7/syncplay-desktop-preview.git (fetch)
  origin  https://github.com/Gauthampro7/syncplay-desktop-preview.git (push)
  ```
- **Latest Commit**:
  `5a8d910 (HEAD -> main, origin/main) chore(agents): persist teamwork Phase 5 audit logs and verification reports`
- **Tags**: `v0.1.1` present on commit `0c12205`.

### 3.2 GitHub CLI (`gh`) Authentication & Authorization
- **Status check (`gh auth status`)**:
  ```text
  github.com
    ✓ Logged in to github.com account Gauthampro7 (keyring)
    - Active account: true
    - Git operations protocol: https
    - Token: gho_************************************
    - Token scopes: 'gist', 'read:org', 'repo', 'workflow'
  ```
- **Evaluation**: The authentication token possesses the essential `'repo'` scope. This provides full administrative rights to create new public or private repositories, update remotes, manage webhooks, and push commits.

### 3.3 GitHub Destination Repository Verification
- Checked existence via `gh repo view Gauthampro7/syncverse`:
  ```text
  GraphQL: Could not resolve to a Repository with the name 'Gauthampro7/syncverse'. (repository)
  ```
- Checked existence via `gh repo view Gauthampro7/Syncverse`:
  ```text
  GraphQL: Could not resolve to a Repository with the name 'Gauthampro7/Syncverse'. (repository)
  ```
- **Conclusion**: The target repository does not exist on GitHub yet and must be created.

---

## 4. Execution Blueprint: Git & GitHub Remote Setup

The implementer agent should follow this verified command sequence to create the repository and push cleanly:

### Step 1: Create the GitHub Repository
Run GitHub CLI to create the remote repository under `Gauthampro7`:
```bash
gh repo create Gauthampro7/syncverse \
  --public \
  --description "Ultra-Efficient Standalone Desktop Media Player & Long-Distance Synchronized Watch-Party Platform in Pure Rust"
```
*(Note: If Gautham prefers the repository to remain private during initial staging, supply `--private` instead of `--public`).*

### Step 2: Repoint Local Git Remote
Update the existing `origin` remote URL (or retain `syncplay-desktop-preview` as `old-origin`):
```bash
# Option A: Direct repoint of origin
git remote set-url origin https://github.com/Gauthampro7/syncverse.git

# Option B (Recommended for safety): Rename old origin and add new origin
git remote rename origin legacy-origin
git remote add origin https://github.com/Gauthampro7/syncverse.git
```

Verify the configured remotes:
```bash
git remote -v
```

### Step 3: Stage and Commit Renamed Codebase
After the refactoring and renaming agents complete their file edits and pass verification:
```bash
git add -A
git commit -m "feat: migrate and rename project from Syncplay-Alt to Syncverse under MIT License"
```

### Step 4: Push Branches and Tags
Push the `main` branch with upstream tracking, followed by tags:
```bash
git push -u origin main
git push origin --tags
```

### Step 5: Verification of Remote
Confirm remote availability via CLI:
```bash
gh repo view Gauthampro7/syncverse
```

---

## 5. Potential Pitfalls & Recommendations for Implementation

1. **Circular Import & Crate Path Discipline**:
   When renaming `crates/syncplay-*` to `crates/syncverse-*`, perform directory renames first, update root `Cargo.toml` members, update each crate's `Cargo.toml` dependencies, and then perform `grep_search` across `crates/` to ensure no `syncplay_` import remains.
2. **Empirical Challenge Tests Alignment**:
   Python tests in `tests/empirical_challenge/` (e.g. `test_desktop_packaging.py` and `test_core_adversarial_p2.py`) check specific filenames like `syncplay-alt.desktop` and `syncplay_chat.bin`. Ensure any corresponding test assertions are updated in lockstep when the files and binaries are renamed.
3. **Auto-Updater Tests Compatibility**:
   `crates/syncplay-ui/src/gui/updater.rs` and its corresponding tests in `tests/e2e_phase5/` test GitHub release JSON fixtures. The URL `https://api.github.com/repos/Gauthampro7/syncverse/releases/latest` must match across production constants and test fixtures.
4. **Desktop Entry Validation**:
   On Fedora Linux, running `desktop-file-validate assets/syncverse.desktop` is required by `test_desktop_packaging.py` and `install_linux.sh`. The renamed `.desktop` file must adhere strictly to XDG Desktop Entry Spec v1.5.
