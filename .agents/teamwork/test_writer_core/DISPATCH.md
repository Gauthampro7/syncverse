## 2026-10-30T22:10:34Z
You are test_writer_core, a teamwork_test_writer.

Your assigned working directory is:
/home/gautham/syncverse/.agents/teamwork/test_writer_core/

MANDATORY FIRST STEP:
Read /home/gautham/syncverse/.agents/teamwork/ORIGINAL_REQUEST.md, /home/gautham/syncverse/AGENTS.md, and /home/gautham/syncverse/.agents/teamwork/orchestrator/PROJECT.md before writing tests.

TASK OBJECTIVE:
Write comprehensive automated tests in `crates/syncverse-sync/tests/` and `crates/syncverse-demux/tests/` covering the new Core & Invariant functionality:
1. PLL Target Rate Tracking Test (`crates/syncverse-sync/tests/pll_target_rate_test.rs`):
   - Test `PllSpeedController::update_with_target_rate` with target rates 0.5x, 1.0x, 1.25x, 1.5x, 2.0x.
   - Verify micro-speed steering stays strictly within `[r_target * 0.96, r_target * 1.04]`.
   - Verify Zone 1 deadband returns `r_target`.
   - Verify Zone 3 hard seeks trigger when drift exceeds 1500ms.
2. Subdirectory Subtitle Scanner Test (`crates/syncverse-demux/tests/subtitle_subdirectories_test.rs`):
   - Create temporary test directories with media files and companion subtitles in `subtitles/`, `Subtitles/`, `subs/`, and `Subs/`.
   - Verify `scan_external_subtitles` correctly identifies and lists subtitles from these subdirectories.
3. Case-Insensitive Slash Command Parser Test (`crates/syncverse-chat/tests/command_case_test.rs`):
   - Verify `/POLL`, `/Poll`, `/SEEK`, `/Seek`, `/BOOKMARK` parse without panic.
4. Soft-Knee Audio Limiter Test (`crates/syncverse-codecs/tests/soft_clip_test.rs`):
   - Test `soft_clip_sample` with inputs $s \in [-2.0, 2.0]$.
   - Verify identity for $|s| \le 0.85$.
   - Verify smooth saturation bounded strictly in $(-1.0, 1.0)$ for large amplitudes without discontinuity.

STANDARDS:
- Tests must compile cleanly under `#![deny(warnings)]`.
- Document all tests with docstrings.
- Run `cargo test --package syncverse-sync --test pll_target_rate_test` and relevant test commands to verify.
- Write your completion report to:
  /home/gautham/syncverse/.agents/teamwork/test_writer_core/handoff.md
- Send a completion message to your parent when done.
