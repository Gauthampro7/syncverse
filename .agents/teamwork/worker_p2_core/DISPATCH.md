# Dispatch Instructions — worker_p2_core

## Mission
Implement and wire all CLI subcommands (`inspect`, `play`, `party host|connect`, `chat demo|log|query`, `verify`) and the Runtime Coordinator / Telemetry Aggregator in `crates/syncplay-core`. Ensure zero compiler/clippy warnings, zero banned crates, 100% test pass rate, and 100% Intent Compliance (`scripts/intent_harness.py`).

## Authoritative Reference
- Read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`
- Read `/home/gautham/syncplay-alt/AGENTS.md`
- Read `/home/gautham/syncplay-alt/.agents/teamwork/orchestrator_2/PROJECT.md`

## MANDATORY INTEGRITY WARNING
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## Detailed Scope & Requirements
1. **Target Files**:
   - `crates/syncplay-core/src/main.rs`
   - `crates/syncplay-core/src/lib.rs`
   - `crates/syncplay-core/src/coordinator.rs` (if applicable)
   - Integration tests in `crates/syncplay-core/tests/`

2. **CLI Subcommands to Wire & Perfect**:
   - `syncplay-alt inspect <file|synthetic-mkv|synthetic-mp4>`:
     - Demuxes container headers using `EbmlDemuxer` or `IsoBmffDemuxer`.
     - Displays timescale, duration, track inventory (video, audio, subtitle), and keyframe sample tables.
   - `syncplay-alt play [file|synthetic-mkv|synthetic-mp4]`:
     - Sets up `PlayerCoordinator` and `TelemetryAggregator`.
     - Connects demuxer packet reading, Q8 planar YUV420p -> RGBA conversion, SPSC ring buffer, monotonic SeqLock presentation clock, and atomic triple-buffered UI snapshot (`PlayerUiSnapshot`).
     - Demonstrates live playback ticks with OSD telemetry and interactive/simulated seeks without phase jumps.
   - `syncplay-alt party host [port]` and `syncplay-alt party connect <addr>`:
     - Uses `syncplay-sync`'s `HostServer` and `ClientNode` with non-blocking `UdpSocket`.
     - Exchanges NTP 4-timestamp probes (`T1, T2, T3, T4`).
     - Applies RFC 5905 sliding-window filter with dispersion floor $D \ge 5.0\text{ms}$.
     - Runs discrete PI PLL speed controller ($K_p = 0.17, K_i = 0.0072$) with tri-zone policy.
     - Runs barrier two-phase commit unpause and network chat packet relaying.
   - `syncplay-alt chat demo`:
     - Appends `ChatRecord` instances to `syncplay_chat.bin` with `0xAA55` framing and CRC32.
     - Performs FTS search (English, hashtag, CJK unigrams/bigrams).
     - Parses and dispatches slash commands (`/seek`, `/poll`, `/bookmark`, `/clip`, `/sync-status`).
     - Exports markdown transcript.
   - `syncplay-alt chat query <search_term>`:
     - Recovers records from `syncplay_chat.bin` using `ChatRecoveryScanner`, indexes into `InvertedIndexFts`, and outputs search results.
   - `syncplay-alt chat log <message>`:
     - Appends a new timestamp-anchored text message to `syncplay_chat.bin`.
   - `syncplay-alt verify`:
     - Runs `python3 scripts/intent_harness.py` and reports compliance score.

3. **Invariants & Lints**:
   - ZERO banned crates (`ffmpeg`, `vlc`, `mpv`, `symphonia`, `sqlite`, `tokio`, etc.).
   - `#![deny(warnings)]`, `#![deny(missing_docs)]`, `#![deny(unsafe_code)]`.
   - All tests must pass:
     - `cargo check --workspace --all-targets` (0 warnings)
     - `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings)
     - `cargo test --workspace -- --nocapture` (100% pass)
     - `python3 scripts/intent_harness.py` (100.0% compliance)
     - `./scripts/verify_phase1.sh` (all 8 stages pass)
     - Test CLI commands:
       `cargo run -p syncplay-core -- inspect synthetic-mkv`
       `cargo run -p syncplay-core -- inspect synthetic-mp4`
       `cargo run -p syncplay-core -- play synthetic-mkv`
       `cargo run -p syncplay-core -- party demo`
       `cargo run -p syncplay-core -- chat demo`
       `cargo run -p syncplay-core -- verify`

4. **Output Requirements**:
   - Write handoff report to `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_core/handoff.md` with:
     - 1. Observation (files changed, command outputs)
     - 2. Logic Chain (technical rationale)
     - 3. Caveats (if any)
     - 4. Conclusion (summary of changes)
     - 5. Verification Method (exact commands to run)
   - Send completion message to orchestrator with summary and verdict.


## 2026-10-25T13:54:56Z
You are worker_p2_core, working in /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_core/.
Read /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_core/DISPATCH.md and /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md.
Implement and wire all CLI subcommands (`inspect`, `play`, `party host|connect`, `chat demo|log|query`, `verify`) and the Runtime Coordinator / Telemetry Aggregator in `crates/syncplay-core`.
Ensure zero compiler warnings under `#![deny(warnings)]`, zero clippy warnings under `-D warnings`, zero banned dependencies, 100% test pass rate, and 100.0% Intent Compliance score (`scripts/intent_harness.py`).
Deliver your handoff report to /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_core/handoff.md and send a completion message to the orchestrator.


## 2026-10-25T14:05:15Z
**Context**: Milestone 5 Implementation Status Check
**Content**: Safety timer has fired. Please report on your current progress in `crates/syncplay-core` (CLI commands, telemetry aggregator, intent compliance) or deliver your handoff report if complete.
**Action**: Update your `progress.md`, reply with your status, or write `handoff.md` and send your completion report.
