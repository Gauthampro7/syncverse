# Workflow: e2e-verification-workflow

## 1. Metadata & OpenSpec Mapping
- **Workflow ID**: `e2e-verification-workflow`
- **Target Deliverable**: Self-Contained Verification Harness (`scripts/verify_phase1.sh`)
- **Authority**: Authoritative User Request (Requirement R4) & Project Acceptance Criteria
- **Target Crates**: Entire Workspace (`crates/*`)
- **Primary Agents**: `spec_implementer`, `bitstream_tester`, `sync_simulator`, `storage_auditor`
- **Required Skills**: `zero-dependency-audit`, `network-sync-simulation`, `bitstream-vector-testing`

---

## 2. Verification Harness Architecture

The end-to-end verification harness runs 7 automated validation stages:

```
┌─────────────────────────────────────────────────────────────┐
│                  Stage 1: OpenSpec Audit                    │
│   Verify specs for 6 subsystems & 6 change proposals exist  │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                 Stage 2: Pi Framework Audit                 │
│      Verify AGENTS.md, .pi/agents, .pi/skills, .pi/workflows │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│           Stage 3: Zero-Dependency Blacklist Audit          │
│       Run audit_dependencies.sh on Cargo.toml / Cargo.lock  │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│             Stage 4: Workspace Compilation & Lint           │
│           cargo check & cargo clippy (-D warnings)          │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│            Stage 5: Full Workspace Unit Test Suite          │
│                    cargo test --workspace                   │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│       Stage 6: Synthetic Bitstream & Storage Tests          │
│    Adversarial python empirical challenge harnesses 1 & 2   │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│       Stage 7: High-RTT Network Sync & UI Tests             │
│    Adversarial python empirical challenge harnesses 3 & 4   │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. Step-by-Step Execution Plan

### Step 1: OpenSpec Specification Suite Audit
Verify presence and structural integrity of OpenSpec documents:
1. `openspec/project.md` exists and contains master architecture invariants.
2. `openspec/specs/` contains specs for:
   - `demuxer/spec.md`
   - `codecs/spec.md`
   - `av-pipeline/spec.md`
   - `sync-engine/spec.md`
   - `smart-chat/spec.md`
   - `ui-renderer/spec.md`
3. Each spec contains formal `### Requirement:` and `#### Scenario:` Given/When/Then blocks.
4. `openspec/changes/` contains proposals `001` through `006`.

### Step 2: Pi Framework Integrity Check
1. Root `AGENTS.md` exists with banned crate list, memory layout rules, and verification commands.
2. `.pi/agents/` contains all 4 specialized agent prompts (`spec_implementer.md`, `bitstream_tester.md`, `sync_simulator.md`, `storage_auditor.md`).
3. `.pi/skills/` contains all 4 operational skills with valid frontmatter metadata.
4. `.pi/workflows/` contains phase workflows `001` through `006` and `e2e-verification-workflow.md`.

### Step 3: Zero-Dependency Blacklist Audit
Execute `scripts/audit_dependencies.sh`:
- Assert 0 occurrences of banned multimedia/player/database crates enumerated in root `AGENTS.md` (`ffmpeg`, `ffmpeg-sys`, `ffmpeg-next`, `libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`, `gstreamer`, `gstreamer-sys`, `mpv`, `mpv-sys`, `libmpv`, `vlc`, `libvlc`, `symphonia`, `symphonia-core`, `dav1d`, `dav1d-sys`, `rav1e`, `openh264`, `minimp3`, `nom-ebml`, `hound`, `image`, `imageproc`, `sqlite`, `sqlite3`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `rocksdb-sys`, `heed`, `tantivy`, `meilisearch`, `diesel`).
- Exit code must be 0.

### Step 4: Workspace Compilation & Static Linting
Execute compiler and clippy checks:
```bash
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```
Assert 0 compilation errors and 0 warnings.

### Step 5: Rust Unit & Integration Test Suite
Execute workspace tests:
```bash
cargo test --workspace -- --nocapture
```
Assert 100% test pass rate across all 8 crates.

### Step 6: Empirical Challenge Adversarial Test Suite
Execute the empirical challenge Python test suite:
```bash
python3 tests/empirical_challenge/test_demux_codecs_clock.py
python3 tests/empirical_challenge/test_sync_engine.py
python3 tests/empirical_challenge/test_smart_chat.py
python3 tests/empirical_challenge/test_ui_renderer.py
```
Assert that all 8 challenge areas pass with 0 errors.

---

## 4. Verification & Acceptance Criteria
Execute the complete verification script:
```bash
./scripts/verify_phase1.sh
```
Acceptance criteria:
- All 7 stages report `[PASS]`.
- Exit code is 0.
