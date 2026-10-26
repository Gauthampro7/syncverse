# BRIEFING — 2026-10-25T09:50:00Z

## Mission
Empirical adversarial challenge testing of Milestone 1 Matroska/EBML parser (`crates/syncplay-demux/src/ebml.rs`).

## 🔒 My Identity
- Archetype: empirical_challenger
- Roles: critic, specialist
- Working directory: /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_demux_1
- Original parent: 0644b5fa-9f3b-44ab-92cd-57079ebc4b70
- Milestone: Phase 2 Milestone 1 (Matroska/EBML Demuxer)
- Instance: 1 of 1

## 🔒 Key Constraints
- Review-only — do NOT modify implementation code directly in the target codebase.
- EMPIRICAL CHALLENGER: Must run verification code directly; do NOT trust claims or logs without empirical reproduction.
- Never write tests or source code in `.agents/teamwork/` (metadata only). Put any test harnesses in `tests/` or run scratch/verification tests in standard project test paths.
- Write challenge report to `challenge_report.md` and handoff report to `handoff.md` with explicit verdict `APPROVE` or `REQUEST_CHANGES`.

## Current Parent
- Conversation ID: 0644b5fa-9f3b-44ab-92cd-57079ebc4b70
- Updated: 2026-10-25T09:50:00Z

## Review Scope
- **Files to review**: `crates/syncplay-demux/src/ebml.rs`, `crates/syncplay-demux/src/lib.rs`, `tests/`
- **Interface contracts**: `PROJECT.md`, `AGENTS.md`, `ORIGINAL_REQUEST.md`
- **Review criteria**: Frame lacing robustness (Xiph, Fixed, EBML delta, no lacing), malformed VINT/buffer boundary handling, Cues seeking boundaries, multi-track queue ordering & memory safety, zero warnings, zero unsafe violations.

## Attack Surface
- **Hypotheses tested**:
  - Malformed & truncated lacing headers in all 4 modes (tested, cleanly rejected with CorruptedBlock)
  - VINT parser underflow/overflow bounds (tested, clean rejection)
  - Signed delta VINT with all data bits = 1 (tested: CONFIRMED DEFECT at ebml.rs:172)
  - EBML lacing with frame_count = 1 (tested: CONFIRMED DEFECT at ebml.rs:141-209)
  - Cues seeking boundary conditions (tested: 0 cues, before first cue, exact cue, between cues, after last cue, u64::MAX — robust)
  - Multi-track interleaved packet FIFO order & keyframe flag inheritance (tested — robust)
  - Corrupted SimpleBlock handling in Cluster loop (tested — robust)
  - Unknown child elements with undefined size in cluster (analyzed at ebml.rs:1014)
- **Vulnerabilities found**:
  - ebml.rs:172: Signed delta VINT in EBML lacing mistakenly evaluated as Element ID (`is_element_id = true`), causing valid delta values with all data bits = 1 (e.g. 0xFF for +64) to be rejected as `VintError::ReservedElementId`.
  - ebml.rs:141-209: EBML lacing unconditionally computes and pushes `last_sz`, generating 2 frames when declared `frame_count` is 1 (`payload[0] == 0`), creating a phantom 0-byte non-keyframe packet.
- **Untested angles**: Hardware storage error recovery during mid-stream block read.

## Loaded Skills
- None specified by orchestrator.

## Key Decisions Made
- Constructed dedicated empirical test suite at `crates/syncplay-demux/tests/ebml_adversarial_challenge.rs` and `tests/empirical_challenge/test_ebml_adversarial.py`.
- Verified all 15 tests execute and reproduce lacing defects.
- Issued verdict: `REQUEST_CHANGES` to fix the two EBML lacing defects.

## Artifact Index
- DISPATCH.md — task assignment
- progress.md — liveness heartbeat
- challenge_report.md — detailed adversarial challenge findings
- handoff.md — 5-component handoff report
- crates/syncplay-demux/tests/ebml_adversarial_challenge.rs — empirical challenge test harness
- tests/empirical_challenge/test_ebml_adversarial.py — Python test harness
