# Plan: Syncverse Project Transformation & Clean Deployment

## Architecture & Scope
Refactor and clean up the workspace, rename the project from Syncplay-Alt to Syncverse across the entire codebase, crate directories, packages, and documentation under the MIT License, and configure and push to a new GitHub repository.

## Milestones

### Milestone 1: Survey & Mapping (3 Explorers)
- Map all occurrences of `syncplay` (case-insensitive) across:
  - Crate directory paths: `crates/syncplay-*` -> `crates/syncverse-*`
  - Cargo manifests: root `Cargo.toml`, crate `Cargo.toml` files, inter-crate dependency paths, package names, binary name `syncverse`.
  - Rust source files: `use syncplay_*::` -> `use syncverse_*::`, module references, constants, UI titles, binary names.
  - Test suites & test scripts: `tests/empirical_challenge/*.py`, `tests/e2e/*`, shell scripts in `scripts/`.
  - Desktop entries: `syncplay.desktop` -> `syncverse.desktop`, icon references, installer scripts.
  - Licensing & Docs: `LICENSE`, `README.md`, `AGENTS.md`, `PROJECT.md`, documentation references.
  - Git state: existing remotes, commit history, branch structure, GitHub CLI (`gh`) auth status.

### Milestone 2: Crate Renaming, Cargo Manifests & Core Imports (Worker + Reviewers + Challengers + Auditor)
- Rename directories `crates/syncplay-*` to `crates/syncverse-*`.
- Update root `Cargo.toml` workspace members and default-run target `syncverse`.
- Update each crate's `Cargo.toml` (`[package] name = "syncverse-..."`), inter-crate dependencies, paths, repository URL, license = "MIT".
- Update all Rust `use syncplay_...` import statements across all crates and tests to `use syncverse_...`.
- Update binary definition `[[bin]] name = "syncverse"`.

### Milestone 3: UI, Desktop, Scripts & Test Harness Alignment (Worker + Reviewers + Challengers + Auditor)
- UI window title update: "Syncverse" (and any UI strings referencing Syncplay).
- Desktop file: rename/create `syncverse.desktop`, verify Exec=syncverse, Icon=syncverse, Name=Syncverse.
- Scripts update: `scripts/audit_dependencies.sh`, `scripts/verify_phase1.sh`, `scripts/intent_harness.py`, etc.
- Test suites update: `tests/empirical_challenge/*.py` to import or call `syncverse` instead of `syncplay`.

### Milestone 4: Codebase Refactoring, Hygiene Cleanup & MIT Licensing / Documentation (Worker + Reviewers + Challengers + Auditor)
- Eliminate unused imports, dead code, clippy warnings under `#![deny(warnings)]`.
- Update `LICENSE` to open-source MIT License.
- Update `README.md`, architecture docs, and all markdown documentation to Syncverse + MIT.
- Ensure format consistency (`cargo fmt --check`).

### Milestone 5: GitHub Setup & Push (Worker + Reviewer)
- Verify GitHub CLI authentication for `Gauthampro7`.
- Create or configure GitHub repository `syncverse` (or `Syncverse`) under `Gauthampro7`.
- Set git remote origin to point to `Gauthampro7/syncverse`.
- Commit all changes cleanly with descriptive git message.
- Push all commits and branches to `origin`.

### Milestone 6: Full Verification Gate (Reviewer + Challenger + Auditor)
- Verify `cargo check --workspace --all-targets` passes with zero warnings.
- Verify `cargo clippy --workspace --all-targets -- -D warnings` passes.
- Verify `./scripts/audit_dependencies.sh` passes.
- Verify `cargo test --workspace` passes.
- Verify empirical tests (`test_demux_codecs_clock.py`, `test_sync_engine.py`, `test_smart_chat.py`, `test_ui_renderer.py`).
- Verify binary `syncverse` exists and builds.
- Verify remote repository URL and commit push.
- Forensic Auditor integrity review.
