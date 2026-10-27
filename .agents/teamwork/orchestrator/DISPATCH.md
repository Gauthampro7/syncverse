## 2026-10-27T16:00:04Z
Deploy multiple subagents generously to break down the work. Refactor and clean up the workspace, rename the project from Syncplay-Alt to Syncverse across the entire codebase, crate directories, packages, and documentation under the MIT License, and configure and push to a new GitHub repository.

Working directory: /home/gautham/syncplay-alt
Integrity mode: development

## Requirements

### R1. Full Project and Crate Renaming to Syncverse
Rename the project from Syncplay-Alt to Syncverse everywhere. This includes renaming all crate directories (`crates/syncplay-*` to `crates/syncverse-*`), Cargo package names (e.g., `syncverse-common`, `syncverse-core`), internal crate imports and dependencies, the binary executable name to `syncverse`, UI window title and text, desktop launcher entries (`syncverse.desktop`), Linux and Windows installer configurations, and all references across documentation.

### R2. Codebase Refactoring and Hygiene Cleanup
Perform codebase cleanup across all crates to eliminate unused code, dead imports, and formatting inconsistencies while strictly preserving all existing media playback, demuxing, decoding, synchronization, and UI functionality. Maintain `#![deny(warnings)]` and clippy cleanliness across the entire workspace.

### R3. Open-Source MIT Licensing and Official Documents
Update the project license to the open-source MIT License in `LICENSE` and across all crate manifests. Update all official documentation (`README.md`, architecture guides, specifications) to reflect Syncverse and its MIT licensing terms.

### R4. GitHub Repository Setup and Push
Create or update the remote repository on GitHub to a new repository named `syncverse` (or `Syncverse`) under the `Gauthampro7` account (using GitHub CLI or git remote configuration), and push all commits and branches to it.

## Acceptance Criteria

### Compilation and Static Analysis
- [ ] `cargo check --workspace --all-targets` compiles cleanly with zero warnings under `#![deny(warnings)]`.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` completes with zero errors or warnings.
- [ ] `./scripts/audit_dependencies.sh` passes without encountering banned dependencies.

### Verification Test Suite
- [ ] `cargo test --workspace` executes and passes all unit and integration tests.
- [ ] `python3 tests/empirical_challenge/test_demux_codecs_clock.py` passes.
- [ ] `python3 tests/empirical_challenge/test_sync_engine.py` passes.
- [ ] `python3 tests/empirical_challenge/test_smart_chat.py` passes.
- [ ] `python3 tests/empirical_challenge/test_ui_renderer.py` passes.

### Renaming and Metadata
- [ ] Binary target produces `syncverse` executable and launches without missing crate dependencies.
- [ ] All workspace crates reside in directories named `crates/syncverse-*` with matching Cargo package names and inter-crate dependency references.
- [ ] `syncverse.desktop` and installer scripts reference the new `syncverse` binary and naming.
- [ ] `LICENSE` and `Cargo.toml` manifests specify the MIT License.
- [ ] `README.md` and project documentation reference Syncverse under the MIT License.

### Remote Version Control
- [ ] Git remote origin points to the new repository (`syncverse` under `Gauthampro7`) and all local commits are successfully pushed.


## 2026-10-27T18:51:28Z
Deploy one subagent just to check and find out all the features needed for it, and deploy 10 subagents in parallel generously to finish the work. Implement missing features standard for a desktop video player in Syncverse, including video and audio configuration, automatic subtitle detection and selection, and keyboard shortcuts to cycle between available audio and subtitle tracks.

Working directory: /home/gautham/syncverse
Integrity mode: development

## Requirements

### R1. Feature Audit and Discovery
Perform an initial audit of the video player codebase to identify missing features standard for a desktop video player. Formulate a checklist of standard player features to implement alongside the explicitly requested features.

### R2. Subtitle Detection and Track Selection
Automatically detect available subtitles from both embedded container tracks (e.g. Matroska/MP4) and external subtitle files (such as .srt, .vtt, .ass) located in the media file directory with matching base filenames. Provide UI controls to select and switch between active subtitle tracks, as well as an option to disable subtitles. Render selected subtitles synchronized with media playback.

### R3. Audio and Video Configuration Controls
Provide user-facing controls and configuration settings standard for a video player. For audio: volume/mute control, audio delay/sync offset, and audio track selection. For video: aspect ratio selection/override, playback speed control, and video adjustments standard for desktop playback.

### R4. Audio and Subtitle Cycling Hotkeys
Provide dedicated keyboard shortcuts to cycle sequentially through available audio tracks and available subtitle tracks during active playback, with on-screen HUD/telemetry feedback indicating the newly selected track.

### R5. Architecture and Dependency Invariants
All additions must strictly adhere to the workspace architectural invariants in AGENTS.md: implemented in pure Rust or direct multimedia bindings (`ffmpeg-next`), with zero monolithic player wrappers/IPC (`mpv`, `vlc`) and zero external database daemons (`sqlite`, `rocksdb`).

## Verification Resources
- Use existing workspace verification suites: `cargo check --workspace --all-targets`, `cargo test --workspace`, and empirical challenge test suites in `tests/empirical_challenge/`.
- In-tree synthetic media test vectors and mock tracks generated within tests.

## Acceptance Criteria

### Subtitle Support
- [ ] Subtitle tracks embedded within media containers (MKV, MP4) are detected and listed in the player UI.
- [ ] External subtitle files (.srt, .vtt, .ass) residing in the same directory as the media file with matching base name are automatically detected and listed.
- [ ] Selecting a subtitle track renders the corresponding text synchronized to the current media presentation timestamp.
- [ ] An option to disable/turn off subtitles is available and clears subtitle display.

### Audio and Subtitle Track Cycling
- [ ] Pressing the audio cycle hotkey advances playback to the next available audio track (or cycles back to the first) and updates active audio decoding.
- [ ] Pressing the subtitle cycle hotkey advances through available subtitle tracks (including an 'Off' state) and updates active subtitle presentation.
- [ ] Visual HUD or status notification displays the active track name/index upon cycling.

### Video and Audio Configuration
- [ ] Audio configuration UI allows switching audio tracks, adjusting volume, and configuring audio sync/offset.
- [ ] Video configuration UI allows selecting/adjusting standard playback settings including aspect ratio and playback speed.

### Workspace Standards and Regression Prevention
- [ ] `cargo check --workspace --all-targets` compiles with zero warnings under `#![deny(warnings)]`.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` completes with zero errors or warnings.
- [ ] `./scripts/audit_dependencies.sh` passes without banned monolithic player or database dependencies.
- [ ] `cargo test --workspace` passes all unit and integration tests.
- [ ] All empirical challenge test suites (`test_demux_codecs_clock.py`, `test_sync_engine.py`, `test_smart_chat.py`, `test_ui_renderer.py`) continue to pass.
