## 2026-10-24T23:58:48Z
You are worker_m2 (teamwork_preview_worker).
Your Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/worker_m2/
Project Root: /home/gautham/syncplay-alt
Authoritative User Request: /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md
Project Scope Document: /home/gautham/syncplay-alt/.agents/teamwork/orchestrator_1/PROJECT.md
OpenSpec Reference: /home/gautham/syncplay-alt/openspec/

### Objective
Create the complete Pi Framework Agent-Driven Development Setup (Requirement R2):
1. Root `/home/gautham/syncplay-alt/AGENTS.md`
2. Agent Prompts under `/home/gautham/syncplay-alt/.pi/agents/`
3. Skills under `/home/gautham/syncplay-alt/.pi/skills/`
4. Task Workflows under `/home/gautham/syncplay-alt/.pi/workflows/` mapping to `openspec/changes/`

### Detailed Requirements
1. **`AGENTS.md`**:
   - Master architecture invariants and core design philosophy.
   - Strict "no external codec/player/database library" rules with explicit banned list (`ffmpeg`, `libav*`, `gstreamer`, `mpv`, `vlc`, `symphonia`, `dav1d`, `sqlite`, `rusqlite`, `sqlx`, `nom-ebml`, `rocksdb`).
   - Pure-Rust from-scratch development rules across the 8-crate workspace layout.
   - Strict coding standards: error handling via custom enums (`thiserror`-style or manual `std::error::Error`), `#![deny(missing_docs)]`, zero warnings, 16-byte SIMD stride alignment, lock-free SPSC concurrency semantics, IEEE 802.3 CRC32 checksums.
   - Standard build, test, and verification commands.
2. **`.pi/agents/`**:
   - `spec_implementer.md`: Specialized coding agent prompt for translating OpenSpec requirements and RFC-2119 clauses into pure Rust crate implementations.
   - `bitstream_tester.md`: Specialized agent prompt for synthetic container and bitstream testing without large media fixtures.
   - `sync_simulator.md`: Specialized agent prompt for simulating high-RTT (250–400ms RTT, ±30ms jitter) network links, verifying discrete PI PLL speed control and barrier unpause alignment.
   - `storage_auditor.md`: Specialized agent prompt for verifying binary log crash resilience, false-sync recovery, and inverted index FTS.
3. **`.pi/skills/`**:
   - `.pi/skills/openspec-change-mapping/SKILL.md`: Skill for decomposing OpenSpec change proposals into atomic, test-driven PRs.
   - `.pi/skills/bitstream-vector-testing/SKILL.md`: Skill for generating in-tree synthetic EBML and MP4 test vectors and verifying bitstream parsers.
   - `.pi/skills/network-sync-simulation/SKILL.md`: Skill for running deterministic clock-sync simulations and asserting phase error convergence.
   - `.pi/skills/zero-dependency-audit/SKILL.md`: Skill for running dependency blacklisting and `Cargo.lock` audits.
4. **`.pi/workflows/`**:
   - Phase-by-phase execution plans mapping directly to OpenSpec change proposals:
     * `001-demuxer-workflow.md` (maps to `openspec/changes/001-demuxer-bitstream.md`)
     * `002-codecs-workflow.md` (maps to `openspec/changes/002-codecs-presentation.md`)
     * `003-clock-workflow.md` (maps to `openspec/changes/003-clock-pipeline.md`)
     * `004-sync-workflow.md` (maps to `openspec/changes/004-network-sync-protocol.md`)
     * `005-chat-workflow.md` (maps to `openspec/changes/005-smart-chat-storage.md`)
     * `006-ui-workflow.md` (maps to `openspec/changes/006-ui-osd-renderer.md`)
     * `e2e-verification-workflow.md` (maps to verification harness execution)

### Mandatory Integrity Warning
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

### Deliverables
Write your progress in `/home/gautham/syncplay-alt/.agents/teamwork/worker_m2/progress.md` and complete a full handoff report in `/home/gautham/syncplay-alt/.agents/teamwork/worker_m2/handoff.md`.
Notify orchestrator via `send_message` when all files have been created and verified.
