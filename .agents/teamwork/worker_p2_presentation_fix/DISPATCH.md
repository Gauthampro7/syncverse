# Dispatch for worker_p2_presentation_fix

## Task
Remediate all 5 defects identified by `auditor_p2_presentation` and `challenger_p2_presentation_2` across `crates/syncplay-codecs` and `crates/syncplay-ui`.

## Exclusive Write Ownership
- `crates/syncplay-codecs/`
- `crates/syncplay-ui/`
- `crates/syncplay-clock/`

## Inputs
- `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`
- `/home/gautham/syncplay-alt/AGENTS.md`
- `/home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_presentation/audit_report.md`
- `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_presentation_2/challenge_report.md`

## Required Fixes
1. **`crates/syncplay-codecs/src/frame.rs` (`AudioFramePCM::new`)**:
   - Fix sample range check:
     ```rust
     for &s in &samples {
         if s.is_nan() || !(-1.0..=1.0).contains(&s) {
             return Err(CodecError::InvalidAudio(format!(
                 "sample value {} exceeds [-1.0, 1.0] bounds",
                 s
             )));
         }
     }
     ```
2. **`crates/syncplay-codecs/tests/adversarial_codecs_stress.rs`**:
   - Remove unused imports (`Bt601Converter`, `Bt709Converter`, `ColorSpaceConverter`, `clamp_u8`) from lines 10–12 to satisfy `#![deny(warnings)]`.
3. **`crates/syncplay-ui/tests/adversarial_challenge.rs`**:
   - Fix lines 461–462: replace `1 * 3600` with `3600` and `+ 0` with proper expression to eliminate `clippy::identity_op` warning.
4. **`crates/syncplay-ui/src/scrubber.rs`**:
   - Fix floating-point accumulation drift in `on_drag_move`:
     Change condition to `(t_ms - self.last_seek_time_ms) >= (self.debounce_interval_ms - 1e-6)` so that micro-precision float rounding does not drop expected debounce ticks.
5. **`crates/syncplay-ui/src/snapshot.rs` (`TripleBufferSnapshotExchange`)**:
   - Unify `clean_idx: AtomicU8` and `has_new: AtomicBool` into a single atomic byte: `clean_flags: AtomicU8` (or `AtomicU16`).
   - Bit layout for `clean_flags`:
     - Bits 0..=1: clean buffer index (0, 1, or 2).
     - Bit 2 (`0x04`): `HAS_NEW` flag.
   - In `publish`:
     - Write to `self.write_idx`.
     - In CAS loop: read current `clean_flags`, compute new flags with `(self.write_idx & 0x03) | 0x04`, swap with `compare_exchange_weak(curr, new, Release, Relaxed)`, update `self.write_idx = curr & 0x03`.
   - In `read_latest`:
     - In CAS loop: read current `clean_flags`. If `(curr & 0x04) == 0` (no new snapshot), keep current read buffer. If `(curr & 0x04) != 0` (has new snapshot), compute new flags with `(self.read_idx & 0x03)` (clearing bit 2), swap with `compare_exchange_weak(curr, new, AcqRel, Acquire)`, update `self.read_idx = curr & 0x03`.
   - This eliminates all sequence regressions under extreme spin contention and guarantees reader monotonicity.

## Verification
- Run `export PATH="$HOME/.cargo/bin:$PATH"`
- Run `cargo check --workspace --all-targets` (must pass with 0 warnings)
- Run `cargo clippy --workspace --all-targets -- -D warnings` (must pass with 0 warnings)
- Run `cargo test --workspace -- --nocapture` (all unit and integration tests must pass)
- Run `python3 tests/empirical_challenge/test_ui_renderer.py`
- Run `./scripts/audit_dependencies.sh`
- Run `python3 scripts/intent_harness.py` (must score 100.0%)
- Run `./scripts/verify_phase1.sh` (all 8 stages must pass cleanly)

## Deliverables
- Fixes applied in the codebase
- Progress log in `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_presentation_fix/progress.md`
- Formal handoff in `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_presentation_fix/handoff.md`


## 2026-10-25T10:40:32Z
You are worker_p2_presentation_fix, a teamwork_preview_worker for Milestone 2 Remediation of Syncplay-Alt Phase 2.
Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_presentation_fix/
Workspace root: /home/gautham/syncplay-alt

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

MANDATORY INSTRUCTIONS:
1. You MUST read /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically ## 2026-10-25T09:09:06Z) and /home/gautham/syncplay-alt/AGENTS.md before starting.
2. Read your task dispatch at /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_presentation_fix/DISPATCH.md.
3. Read the audit failure report at /home/gautham/syncplay-alt/.agents/teamwork/auditor_p2_presentation/audit_report.md and Challenger 2 report at /home/gautham/syncplay-alt/.agents/teamwork/challenger_p2_presentation_2/challenge_report.md.
4. Implement all 5 remediations:
   a) `AudioFramePCM::new` NaN check in `crates/syncplay-codecs/src/frame.rs`: `if s.is_nan() || !(-1.0..=1.0).contains(&s)`.
   b) Unused imports in `crates/syncplay-codecs/tests/adversarial_codecs_stress.rs`.
   c) Clippy `identity_op` in `crates/syncplay-ui/tests/adversarial_challenge.rs`.
   d) Scrubber debouncer float accumulation tolerance (`- 1e-6`) in `crates/syncplay-ui/src/scrubber.rs`.
   e) TripleBufferSnapshotExchange in `crates/syncplay-ui/src/snapshot.rs`: Unify `clean_idx` and `has_new` into `clean_flags: AtomicU8` with CAS loop to eliminate phantom swaps and sequence regressions under extreme spin contention.
5. Verification:
   - Run `export PATH="$HOME/.cargo/bin:$PATH"`
   - Run `cargo check --workspace --all-targets` (must be 0 warnings)
   - Run `cargo clippy --workspace --all-targets -- -D warnings` (must be 0 warnings)
   - Run `cargo test --workspace -- --nocapture` (all tests must pass)
   - Run `python3 tests/empirical_challenge/test_ui_renderer.py`
   - Run `./scripts/audit_dependencies.sh`
   - Run `python3 scripts/intent_harness.py` (must score 100.0%)
   - Run `./scripts/verify_phase1.sh` (all 8 stages must pass cleanly)
6. Update progress in /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_presentation_fix/progress.md.
7. Write formal handoff report in /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_presentation_fix/handoff.md.
8. Send a message to your parent orchestrator with your verification results and handoff path.
