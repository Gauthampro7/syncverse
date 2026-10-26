# Execution Plan — Syncplay-Alt Phase 1

## Overview
Phase 1 delivers the complete architectural foundation, OpenSpec specifications, Pi agent development framework, compiling pure-Rust workspace with working foundational primitives, and automated verification harness with high-RTT network simulation.

## Milestones & Iteration Plan

### Milestone 1: OpenSpec Specification Suite (`openspec/`)
- **Objective**: Author `openspec/project.md`, `openspec/specs/<subsystem>/spec.md` (6 subsystems with RFC-2119 MUST/SHALL requirements and Given/When/Then scenarios), and `openspec/changes/` roadmap.
- **Cycle**:
  1. Worker (`worker_m1`): Authors all OpenSpec documents based on `PROJECT.md` and survey analyses (`explorer_survey_1`, `explorer_survey_2`, `explorer_survey_3`).
  2. Reviewers (`reviewer_m1_1`, `reviewer_m1_2`): Independent verification of RFC-2119 conformance, Given/When/Then scenario coverage, and consistency across specifications.
  3. Challengers (`challenger_m1_1`, `challenger_m1_2`): Adversarial scenario stress testing (edge cases, clock jumps, packet corruptions, malformed boxes).
  4. Forensic Auditor (`auditor_m1`): Integrity audit (authentic specifications, zero facade/dummy content).
  5. Gate check.

### Milestone 2: Pi Framework Setup (`AGENTS.md` & `.pi/`)
- **Objective**: Establish `AGENTS.md` (architecture invariants, zero external codec rules, verification commands) and `.pi/` directory (prompts, skills, workflows mapping to OpenSpec changes).
- **Cycle**:
  1. Worker (`worker_m2`): Creates `AGENTS.md`, `.pi/agents/`, `.pi/skills/`, `.pi/workflows/`.
  2. Reviewers (`reviewer_m2_1`, `reviewer_m2_2`): Review tool/agent compatibility, workflow completeness.
  3. Challengers (`challenger_m2_1`, `challenger_m2_2`): Verify skill workflows against OpenSpec change proposals.
  4. Forensic Auditor (`auditor_m2`): Verify invariant integrity and anti-cheating rules.
  5. Gate check.

### Milestone 3: Pure-Rust Multi-Crate Workspace Scaffold & Foundational Primitives
- **Objective**: Create 8-crate Cargo workspace (`syncplay-common`, `syncplay-demux`, `syncplay-codecs`, `syncplay-clock`, `syncplay-sync`, `syncplay-chat`, `syncplay-ui`, `syncplay-core`) with working foundational primitives (EBML/ISOBMFF header parsers, NTP clock filter + PLL speed controller math, custom binary chat log + inverted index search + slash-command parser) and automated unit tests.
- **Cycle**:
  1. Worker (`worker_m3`): Sets up workspace, verifies rust toolchain, implements primitives and unit tests.
  2. Reviewers (`reviewer_m3_1`, `reviewer_m3_2`): Check code quality, zero-dependency compliance, unit test passes.
  3. Challengers (`challenger_m3_1`, `challenger_m3_2`): Run property/stress tests against parsers, controllers, and log engine.
  4. Forensic Auditor (`auditor_m3`): Static analysis and lockfile inspection for zero forbidden dependencies (`ffmpeg`, `sqlite`, etc.).
  5. Gate check.

### Milestone 4: Verification Harness & Deterministic Simulation Suite (`scripts/verify_phase1.sh`)
- **Objective**: Author `scripts/verify_phase1.sh` and `scripts/audit_dependencies.sh` executing 7 validation stages, including the high-RTT network simulation test (250–400ms RTT, ±30ms jitter, PLL phase error converges < 15ms without hard seek under 1.5s drift, barrier-sync unpause aligns start < 10ms).
- **Cycle**:
  1. Worker (`worker_m4`): Implements `scripts/verify_phase1.sh` and simulation harness.
  2. Reviewers (`reviewer_m4_1`, `reviewer_m4_2`): Verify all checks pass end-to-end with exit code 0.
  3. Challengers (`challenger_m4_1`, `challenger_m4_2`): Inject adversarial jitter, latency spikes, and packet drops to verify simulation bounds.
  4. Forensic Auditor (`auditor_m4`): Verify harness authenticity and zero cheating/mocking.
  5. Gate check.
