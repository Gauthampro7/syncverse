## 2026-10-30T22:05:04Z
You are worker_m1, a teamwork_worker.

Your assigned working directory is:
/home/gautham/syncverse/.agents/teamwork/worker_m1/

MANDATORY FIRST STEP:
Read /home/gautham/syncverse/.agents/teamwork/ORIGINAL_REQUEST.md and /home/gautham/syncverse/AGENTS.md before starting any work.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

TASK SCOPE (Milestone 1: Architectural Invariants, Standards Compliance & Mathematical Precision):
Review the detailed survey blueprints at:
- /home/gautham/syncverse/.agents/teamwork/explorer_p5_survey_1/handoff.md
- /home/gautham/syncverse/.agents/teamwork/explorer_p5_survey_2/handoff.md
- /home/gautham/syncverse/.agents/teamwork/orchestrator/PROJECT.md

Implement the following genuine, high-precision improvements across the workspace:
1. `crates/syncverse-sync/src/pll.rs` & `crates/syncverse-core/src/playback_runtime.rs`:
   - In `pll.rs`: Extend `PllSpeedController` to track target rate `r_target`. Add `pub fn update_with_target_rate(&mut self, target_pts_ms: u64, actual_pts_ms: u64, dt_secs: f64, r_target: f64) -> PllCorrection`.
     Scale micro-speed steering range relative to `r_target`: `(r_target * (1.0 + delta_r)).clamp(r_target * 0.96, r_target * 1.04)`.
     In Zone 1 (Deadband), return `speed_multiplier: r_target`. Keep `pub fn update(&mut self, ...)` delegating to `update_with_target_rate(..., 1.0)` for backward compatibility. Add unit tests for non-1.0x target rates (e.g. 1.25x, 1.5x, 0.75x).
   - In `crates/syncverse-core/src/playback_runtime.rs`: In Zone 1 (Deadband, `drift_ms <= 15`), reanchor client clock to `room_state.r_target`:
     `let target_speed = room_state.r_target; if (speed - target_speed).abs() > 1e-4 { speed = target_speed; clock.reanchor(now, target_speed); }`
2. `crates/syncverse-demux/src/universal.rs`:
   - In `UniversalDemuxer::init_from_context` (around lines 207-210), replace `f64` chapter timescale math with 128-bit integer rational conversion per AGENTS.md Section 4 Rule 6:
     `let num = tb.numerator() as i128; let den = if tb.denominator() != 0 { tb.denominator() as i128 } else { 1 };`
     `let start_ms = ((ch.start() as i128 * num * 1000) / den).max(0) as u64;`
     `let end_ms = ((ch.end() as i128 * num * 1000) / den).max(0) as u64;`
3. `crates/syncverse-demux/src/subtitles/scanner.rs`:
   - In `scan_external_subtitles`, check standard subtitle subdirectories in media directory: `subtitles/`, `Subtitles/`, `subs/`, `Subs/` for matching base names with supported extensions. Add unit tests.
4. `crates/syncverse-codecs/src/audio/`:
   - Implement `soft_clip_sample(s: f32) -> f32` with smooth `tanh` saturation for $|s| > 0.85$ (zero allocations).
   - Use `soft_clip_sample` in `universal.rs` downmixed audio and in `playback_runtime.rs` volume scaling to prevent digital clipping buzz. Add unit tests verifying smooth limiting and zero distortion below 0.85.
5. `crates/syncverse-clock/src/master_clock.rs`:
   - Add formal `// SAFETY:` justification comments for unsafe blocks at lines 186 and 202 (proving exclusive mutable access guaranteed by writer holding `writer_lock`).
6. `crates/syncverse-chat/src/commands.rs`:
   - Fix panic vulnerability at line 184 in `/poll` command: safely slice remaining arguments without `.find("poll").unwrap()`. Add unit test verifying `/POLL` and `/Poll` parse without panic.
7. `crates/syncverse-common/src/time.rs` & `path.rs`:
   - In `time.rs`: Implement `MediaTime::from_timecode_str(s: &str) -> Result<Self, CommonError>` and `impl std::str::FromStr for MediaTime`. Add unit tests.
   - In `path.rs`: Strip leading `/` before Windows drive letters (e.g. `/C:/...` -> `C:/...`).
   - Clean legacy "Syncplay-Alt" docstrings in `common/src/lib.rs`, `common/src/path.rs`, `common/src/error.rs`, `sync/src/lib.rs`, `core/src/aggregator.rs`.
8. `crates/syncverse-core/src/error.rs` & `playback_runtime.rs`:
   - Add `Common(#[from] syncverse_common::CommonError)` to `CoreError`.
   - Replace untyped `String` and `Box<dyn Error>` in `playback_runtime.rs` with strongly typed `CoreError`.

STANDARDS & VERIFICATION:
- Must maintain `#![deny(warnings)]`, `#![deny(missing_docs)]` with thorough docstrings on all public items.
- Run:
  1. `./scripts/audit_dependencies.sh`
  2. `cargo check --workspace --all-targets`
  3. `cargo clippy --workspace --all-targets -- -D warnings`
  4. `cargo test --workspace -- --nocapture`
- Write your comprehensive completion report to:
  /home/gautham/syncverse/.agents/teamwork/worker_m1/handoff.md
- When finished, send a message to your parent.

## 2026-10-30T22:21:09Z
**Context**: Milestone 1 Implementation Verification
**Content**: `test_writer_core` noted a clippy warning: `collapsible_if` in `crates/syncverse-common/src/path.rs:67`.
**Action**: Please ensure this `if` is collapsed so `cargo clippy --workspace --all-targets -- -D warnings` passes with zero warnings.

