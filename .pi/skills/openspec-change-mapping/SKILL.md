---
name: openspec-change-mapping
description: Operational skill for decomposing OpenSpec change proposals and RFC-2119 capability specs into atomic, test-driven implementation units
version: 1.0.0
---

# Skill: OpenSpec Change Mapping

## Overview
This skill guides autonomous agents in ingesting OpenSpec change proposals (`openspec/changes/*.md`) and capability specifications (`openspec/specs/*/spec.md`), parsing their normative RFC-2119 requirements, and decomposing them into atomic, verifiable, test-driven pull requests or development slices.

---

## 1. Specification Structure Analysis

Each capability specification in `openspec/specs/` contains:
1. **Context & Motivation**: Why this component exists and architectural constraints.
2. **Normative Requirements (`### Requirement:`)**: Concrete functional obligations containing RFC-2119 keywords (`MUST`, `MUST NOT`, `SHALL`, `SHALL NOT`, `SHOULD`, `MAY`).
3. **Scenarios (`#### Scenario:`)**: Testable behavior specifications structured as:
   - `Given`: Initial system state, input buffers, or network condition.
   - `When`: Action performed or method invoked.
   - `Then`: Assertions on output values, errors, or side-effects.

Each change proposal in `openspec/changes/` contains:
1. **Dependency Graph**: Explicit upstream dependencies and blocked downstream changes.
2. **Affected Crates**: Specific file paths and module hierarchies to modify or create.
3. **Deliverables & Implementation Plan**: Step-by-step engineering milestones.
4. **Verification & Acceptance Criteria**: Concrete automated tests that validate completion.

---

## 2. Decomposition Methodology

Follow this 4-step decomposition workflow:

### Step 1: Upstream Dependency Validation
Before beginning implementation of change proposal `X`:
1. Check Section 2 (Dependency Graph) of `openspec/changes/X.md`.
2. Inspect the codebase to confirm that all upstream dependencies have already landed and pass `cargo test`.
3. If an upstream dependency is unfulfilled, STOP and report the block to the orchestrator.

### Step 2: Clause & Scenario Extraction
1. Extract all RFC-2119 clauses from the spec into a structured table:
   | Requirement ID | RFC-2119 Clause | Scope / Crate | Test Scenario |
   |---|---|---|---|
   | DEMUX-01 | MUST resolve VINT width 1..8 | syncverse-demux | `test_vint_widths` |
2. Match every `#### Scenario:` to a concrete Rust unit test function in the target crate.

### Step 3: Atomic Slice Slicing
Decompose the change into 4 sequential, verifiable slices:

- **Slice A: Interfaces & Error Hierarchy**:
  - Define custom error enum with `#[derive(thiserror::Error, Debug)]`.
  - Define traits and data structures (zero logic stubs with `todo!()` or empty types).
  - Add `#![deny(missing_docs)]` and doc comments.
  - Verify with `cargo check -p <crate>`.

- **Slice B: Scenario-Derived Test Harness**:
  - Implement unit tests mapping directly to the Given/When/Then scenarios.
  - Implement adversarial boundary tests (empty inputs, MAX values, corrupted buffers).
  - Verify that tests compile and fail as expected.

- **Slice C: Implementation of Primitives**:
  - Implement algorithmic routines, parsers, or math logic.
  - Guarantee zero external forbidden dependencies.
  - Ensure all `#[test]` cases in Slice B pass cleanly.

- **Slice D: Linting & Integration Verification**:
  - Run `cargo clippy -p <crate> --all-targets -- -D warnings`.
  - Run matching empirical challenge tests (`python3 tests/empirical_challenge/...`).
  - Update OpenSpec change status.

---

## 3. Verification & Compliance Checklist

Before declaring a change proposal mapped and executed:
- [ ] Every `MUST` / `SHALL` requirement is exercised by at least one unit test.
- [ ] No forbidden crates (`ffmpeg`, `libav*`, `sqlite`, etc.) are introduced in `Cargo.toml`.
- [ ] `cargo test -p <crate>` runs and passes with 0 failures.
- [ ] `cargo clippy -p <crate> -- -D warnings` reports 0 warnings.
- [ ] Memory layouts adhere to SIMD (16-byte) and cache-line (128-byte) alignment constraints.
