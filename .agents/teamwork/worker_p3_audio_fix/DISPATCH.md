# Dispatch: worker_p3_audio_fix

## Identity
- Role: Implementation Worker (Milestone 2 Iteration 2 Remediation)
- Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/worker_p3_audio_fix
- Authoritative User Request: /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md
- Reviewer Evidence Report: /home/gautham/syncplay-alt/.agents/teamwork/reviewer_p3_audio_1/review_report.md
- Reviewer Handoff: /home/gautham/syncplay-alt/.agents/teamwork/reviewer_p3_audio_1/handoff.md

## Scope & Write Ownership
You exclusively own:
- `crates/syncplay-codecs/src/audio/resampler.rs`
- `crates/syncplay-codecs/tests/` (resampler tests)

## Mandatory Integrity Warning
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## Remediation Objectives
1. **Fix Inverted Speed Ratio Step Calculation**:
   In `crates/syncplay-codecs/src/audio/resampler.rs`, change the `step` calculation:
   `step = ((self.in_rate as f64) * speed_ratio) / (self.out_rate as f64);`
   (when `speed_ratio` $r > 1.0$, input media is consumed faster per output sample, advancing the timeline faster to catch up).
2. **Update Test Assertions**:
   In `resampler.rs` and all test suites, update expected frame assertions to reflect $\text{expected\_out} \approx \frac{N_{\text{in}} \times f_{\text{out}}}{f_{\text{in}} \times r}$.
   Verify $r = 1.04$ produces fewer output frames from a fixed input buffer than $r = 0.96$.
3. **Add 6-Channel Test Coverage**:
   Add test for 6-channel (5.1 surround) audio resampling in `tests/audio_resampler_tests.rs`.
4. Verify:
   - `cargo test --workspace`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `python3 scripts/intent_harness.py`

Write `handoff.md` and report back to orchestrator_3.


## 2026-10-25T17:00:17Z
You are worker_p3_audio_fix, the remediation worker for Milestone 2 Iteration 2 of Syncplay-Alt Phase 3.
Your working directory is /home/gautham/syncplay-alt/.agents/teamwork/worker_p3_audio_fix.
The workspace root is /home/gautham/syncplay-alt.

Read your dispatch instructions in:
/home/gautham/syncplay-alt/.agents/teamwork/worker_p3_audio_fix/DISPATCH.md
the authoritative user request in:
/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically the latest request under ## 2026-10-25T15:21:44Z),
and the reviewer evidence report in:
/home/gautham/syncplay-alt/.agents/teamwork/reviewer_p3_audio_1/review_report.md
/home/gautham/syncplay-alt/.agents/teamwork/reviewer_p3_audio_1/handoff.md.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Your task is to remediate the micro-speed ratio step formula and test assertions:
1. `crates/syncplay-codecs/src/audio/resampler.rs`:
   Change `step` calculation:
   `step = ((self.in_rate as f64) * speed_ratio) / (self.out_rate as f64);`
   (when playing faster at $r = 1.04$, we must advance through more input media samples per output sample).
2. Correct the test assertions:
   - In `resampler.rs` unit tests
   - In `tests/audio_resampler_tests.rs`
   - In `tests/audio_resampler_adversarial_stress_tests.rs`
   Verify that $r = 1.04$ produces fewer output samples from a fixed input chunk than $r = 0.96$ ($N_{\text{out}} \approx N_{\text{in}} / \text{step}$).
3. Add 6-channel (5.1 surround) test coverage in `tests/audio_resampler_tests.rs`.
4. Run:
   - `cargo test --workspace`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `python3 scripts/intent_harness.py`

Write `handoff.md` and report back to orchestrator_3 (my conversation ID is ecd27bad-3488-46aa-ba26-8b0b7cecf0eb).
