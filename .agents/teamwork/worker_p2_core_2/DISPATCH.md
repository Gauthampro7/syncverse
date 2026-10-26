# Dispatch Instructions — worker_p2_core_2 (Replacement Worker)

## Mission
You are the replacement worker for Milestone 5: implement and wire all CLI subcommands (`inspect`, `play`, `party host|connect`, `chat demo|log|query`, `verify`), the Runtime Coordinator, and the Telemetry Aggregator in `crates/syncplay-core`.
Ensure zero compiler warnings under `#![deny(warnings)]`, zero clippy warnings under `-D warnings`, zero banned dependencies, 100% test pass rate, and 100.0% Intent Compliance score (`scripts/intent_harness.py`).

## Authoritative Reference
- Read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`
- Read `/home/gautham/syncplay-alt/AGENTS.md`
- Read `/home/gautham/syncplay-alt/.agents/teamwork/orchestrator_2/PROJECT.md`

## MANDATORY INTEGRITY WARNING
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## Detailed Scope & Responsibilities
1. **Target Files**:
   - `crates/syncplay-core/src/main.rs`
   - `crates/syncplay-core/src/lib.rs`
   - `crates/syncplay-core/src/coordinator.rs` (if applicable)
   - Integration tests in `crates/syncplay-core/tests/`

2. **Specific CLI Subcommands**:
   - `inspect <file|synthetic-mkv|synthetic-mp4>`:
     - Demux container headers with `EbmlDemuxer` or `IsoBmffDemuxer`.
     - Output timescale, duration, track inventory (video/audio/subtitle), and keyframe sample tables.
   - `play [file|synthetic-mkv|synthetic-mp4]`:
     - Wire `PlayerCoordinator` and `TelemetryAggregator`.
     - Feed demuxer packets, Q8 YUV420p -> RGBA conversion, SPSC ring buffer, monotonic clock, and triple-buffered snapshot (`PlayerUiSnapshot`).
     - Display live playback ticks and handle seeks cleanly.
   - `party host [port]` and `party connect <addr>`:
     - Connect `HostServer` and `ClientNode` from `syncplay-sync` over real `std::net::UdpSocket` in non-blocking mode.
     - Execute RFC 5905 NTP 4-timestamp exchange, PI PLL speed steering, and barrier unpause.
     - Include fallback/demo mode if no peer is connected.
   - `chat demo`:
     - Run demo with `ChatStore`, binary log append, FTS search (English, hashtag, CJK unigram/bigram), and markdown export.
   - `chat query <query_string>`:
     - Search local binary log index and display results.
   - `chat log <message>`:
     - Append new timestamped message to `syncplay_chat.bin`.
   - `verify`:
     - Execute `python3 scripts/intent_harness.py`.

3. **Verification Steps**:
   - `cargo check --workspace --all-targets` (0 warnings)
   - `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings)
   - `cargo test --workspace -- --nocapture` (100% pass)
   - `python3 scripts/intent_harness.py` (100.0% score)
   - `./scripts/verify_phase1.sh` (all 8 stages pass)
   - Verify CLI runs:
     `cargo run -p syncplay-core -- inspect synthetic-mkv`
     `cargo run -p syncplay-core -- inspect synthetic-mp4`
     `cargo run -p syncplay-core -- play synthetic-mkv`
     `cargo run -p syncplay-core -- party demo`
     `cargo run -p syncplay-core -- chat demo`
     `cargo run -p syncplay-core -- verify`

4. **Output Requirements**:
   - Write handoff report to `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_core_2/handoff.md` with:
     - 1. Observation
     - 2. Logic Chain
     - 3. Caveats
     - 4. Conclusion
     - 5. Verification Method
   - Send completion message to orchestrator with summary and verdict.


## 2026-10-25T14:08:49Z
You are worker_p2_core_2, working in /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_core_2/.
Read /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_core_2/DISPATCH.md and /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md.
Implement and wire all CLI subcommands (`inspect`, `play`, `party host|connect`, `chat demo|log|query`, `verify`) and the Runtime Coordinator / Telemetry Aggregator in `crates/syncplay-core`.
Ensure zero compiler warnings under `#![deny(warnings)]`, zero clippy warnings under `-D warnings`, zero banned dependencies, 100% test pass rate, and 100.0% Intent Compliance score (`scripts/intent_harness.py`).
Deliver your handoff report to /home/gautham/syncplay-alt/.agents/teamwork/worker_p2_core_2/handoff.md and send a completion message to the orchestrator.
