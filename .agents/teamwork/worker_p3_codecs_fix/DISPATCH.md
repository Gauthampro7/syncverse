# Dispatch: worker_p3_codecs_fix

## Identity
- Role: Implementation Worker (Milestone 1 Iteration 2 Remediation)
- Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/worker_p3_codecs_fix
- Authoritative User Request: /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md
- Challenger Evidence Report: /home/gautham/syncplay-alt/.agents/teamwork/challenger_p3_codecs_1/challenge_report.md
- Challenger Handoff: /home/gautham/syncplay-alt/.agents/teamwork/challenger_p3_codecs_1/handoff.md

## Scope & Write Ownership
You exclusively own:
- `crates/syncplay-codecs/src/h264/decoder.rs`
- `crates/syncplay-codecs/src/h264/sps.rs`
- `crates/syncplay-codecs/src/h264/pps.rs`
- `crates/syncplay-codecs/tests/`

## Mandatory Integrity Warning
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## Remediation Objectives
1. **Dynamic Resolution Dimension Match in `decoder.rs`**:
   In `H264Decoder::decode_nal`, when `first_mb_in_slice > 0`, only copy previous frame data if `prev.width == width && prev.height == height`. If dimensions changed across packets, do not copy from mismatched dimensions.
2. **Arithmetic Overflow Defenses in `sps.rs` & `pps.rs`**:
   - In `Sps::parse` scaling lists: calculate `next_scale = ((last_scale as i64 + delta_scale as i64).rem_euclid(256)) as i32;` to eliminate integer overflow when `delta_scale == i32::MAX`.
   - In `Sps::visible_dimensions()`: use checked/saturating arithmetic to compute `raw_width`, `raw_height`, `crop_x`, and `crop_y`. Check that `pic_width_in_mbs_minus1 < 1024` and `pic_height_in_map_units_minus1 < 1024`.
   - In `Pps::parse` scaling lists: ensure identical overflow-safe arithmetic.
3. Verify that `cargo test -p syncplay-codecs` and `cargo test --test adversarial_bitstream_fuzz_test` pass cleanly with 0 warnings.

Write `handoff.md` and report back to orchestrator_3.

## 2026-10-25T16:12:31Z
You are worker_p3_codecs_fix, the remediation worker for Milestone 1 Iteration 2 of Syncplay-Alt Phase 3.
Your working directory is /home/gautham/syncplay-alt/.agents/teamwork/worker_p3_codecs_fix.
The workspace root is /home/gautham/syncplay-alt.

Read your dispatch instructions in:
/home/gautham/syncplay-alt/.agents/teamwork/worker_p3_codecs_fix/DISPATCH.md
the authoritative user request in:
/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically the latest request under ## 2026-10-25T15:21:44Z),
and the challenger evidence report in:
/home/gautham/syncplay-alt/.agents/teamwork/challenger_p3_codecs_1/challenge_report.md
/home/gautham/syncplay-alt/.agents/teamwork/challenger_p3_codecs_1/handoff.md.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Your task is to remediate the edge-case defects identified in Milestone 1:
1. `crates/syncplay-codecs/src/h264/decoder.rs`:
   In `H264Decoder::decode_nal`, when `first_mb_in_slice > 0`, check `if prev.width == width && prev.height == height`. Only copy previous frame pixels if dimensions match! If dimensions changed, do not copy from mismatched dimensions.
2. `crates/syncplay-codecs/src/h264/sps.rs`:
   - In scaling lists: `next_scale = ((last_scale as i64 + delta_scale as i64).rem_euclid(256)) as i32;` preventing 32-bit addition overflow under `i32::MAX`.
   - In `visible_dimensions()`: use checked/saturating arithmetic to compute `raw_width`, `raw_height`, `crop_x`, `crop_y`. Cap `pic_width_in_mbs_minus1 < 1024` and `pic_height_in_map_units_minus1 < 1024`.
3. `crates/syncplay-codecs/src/h264/pps.rs`:
   Apply same overflow-safe arithmetic to scaling lists if applicable.
4. Verify by running:
   - `cargo test --workspace`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `python3 scripts/intent_harness.py`

Write `handoff.md` and report back to orchestrator_3 (my conversation ID is ecd27bad-3488-46aa-ba26-8b0b7cecf0eb).
