# Handoff Report: Phase 2 Technical Survey of R3, R4, R5, and CLI Functionality

**Agent**: `explorer_p2_survey_3`  
**Role**: `teamwork_preview_explorer`  
**Milestone**: Phase 2 Technical Survey & Worker Implementation Roadmap  
**Date**: 2026-10-25  

---

## 1. Observation

Direct inspections of the workspace codebase, specifications, and test harnesses revealed the following exact facts:

1. **`crates/syncplay-sync/src/lib.rs` (lines 1–17)**:
   ```rust
   pub mod barrier;
   pub mod error;
   pub mod ntp;
   pub mod pll;
   pub mod timeline;

   pub use barrier::{compute_target_unpause_server, BarrierPhase, BarrierStateMachine};
   pub use error::SyncError;
   pub use ntp::{ProbeSample, Rfc5905Filter};
   pub use pll::{PllCorrection, PllSpeedController, PllZone};
   pub use timeline::RoomState;
   ```
   The crate exports offline mathematical and state models for NTP filtering, PI PLL control, timeline evaluation, and barrier unpause, but **contains no network datagram transport or wire packet codec modules** (e.g., `transport.rs` or `wire.rs`).

2. **`crates/syncplay-sync/src/ntp.rs` (lines 35–160)**:
   - `Rfc5905Filter::new(8, 5_000_000.0)` implements an 8-sample sliding window with a minimum dispersion floor of $5.0\text{ms}$ ($5,000,000\text{ ns}$).
   - Lines 128–144 implement outlier rejection: `rtt > 2.0 * med_rtt || (sample.offset - filtered_theta).abs() > 3.0 * dispersion_eff`.
   - Lines 133–140 implement the watchdog timer: `consecutive_rejections >= 8` clears the window and restarts with the candidate sample.

3. **`crates/syncplay-sync/src/pll.rs` (lines 48–118)**:
   - `PllSpeedController` defaults to $K_p = 0.17\text{ s}^{-1}$, $K_i = 0.0072\text{ s}^{-2}$, $I_{\text{max}} = 32.0\text{ s}$, $r \in [0.96, 1.04]$, deadband $0.015\text{ s}$ ($15\text{ ms}$), and hard seek threshold $1.500\text{ s}$ ($1500\text{ ms}$).
   - Lines 84–116 categorize phase error into Zone 1 (`Deadband`), Zone 2 (`MicroSpeed`), and Zone 3 (`HardSeek`).

4. **`crates/syncplay-sync/src/barrier.rs` (lines 33–40)**:
   - `compute_target_unpause_server`:
     ```rust
     let rtt_factor = (1.5 * (max_rtt_ms as f64)) as u64;
     t_server_now_ms + rtt_factor + preroll_budget_ms + 100
     ```
   - Lines 113–124 verify that across client pre-roll intervals ($0$ to $200\text{ ms}$), the commit arrival margin is strictly $\ge 100\text{ ms}$.

5. **`crates/syncplay-chat/src/record.rs` (lines 6–15, 58–85)**:
   - File header magic: `HEADER_MAGIC = *b"SPCH"`; Record sync word: `SYNC_WORD = [0xAA, 0x55]`.
   - Total record size formula: `total_len = (2 + 4 + body.len() + 4) as u32 >= 48 bytes`.
   - Record body contains `record_type`, `flags`, `wall_timestamp_utc_ms`, `media_timestamp_ms`, `sender_id`, `sender_name`, `payload`, and trailing IEEE 802.3 CRC32.

6. **`crates/syncplay-chat/src/storage.rs` (lines 93–125)**:
   - `ChatRecoveryScanner::recover`: if `total_len < 48 || offset + total_len > file_len` or if CRC32 fails, scanner advances byte-by-byte (`offset += 1`) and increments `corrupted_chunks`, preventing premature truncation when encountering false sync patterns.

7. **`crates/syncplay-chat/src/index.rs` (lines 23–76, 124–173)**:
   - `tokenize_spec`: converts text via Unicode case folding, preserves hashtags `#` and mentions `@`, segments CJK characters (`0x4E00..=0x9FFF`) into unigrams and overlapping bigrams, exempts CJK ideographs from 2-character minimum (length 1..48), and filters Western stop-words.
   - `InvertedIndexFts::search`: executes multi-term Boolean AND using linear two-pointer posting list intersection in $O(\sum |L_i|)$.
   - `InvertedIndexFts::query_timeline_window`: executes temporal queries over `BTreeMap` in $O(\log N + K)$ time.

8. **`crates/syncplay-chat/src/commands.rs` (lines 41–185)**:
   - `SlashCommandParser::parse`: parses `/seek`, `/bookmark`, `/clip`, `/poll`, and `/sync-status`.
   - `parse_time_to_ms`: parses `HH:MM:SS.mmm`, `MM:SS`, relative offsets (`+15s`, `-10s`), and raw seconds into absolute milliseconds.

9. **`crates/syncplay-core/src/main.rs` (lines 202–300)**:
   - `run_playback_simulation`: simulates 5 ticks with `std::thread::sleep(Duration::from_millis(50))` and executes a single seek command on `PlayerCoordinator`.
   - `run_party_mode`: attempts to bind UDP port `18242` on `host`, but executes a deterministic 8-step simulation loop with synthetic jitter rather than real network packet exchanges.

10. **Command Execution Results**:
    - `./scripts/audit_dependencies.sh`:
      ```
      [INFO] Auditing Cargo.toml manifests for banned dependencies...
      [INFO] Auditing Cargo.lock for banned dependencies...
      [PASS] Dependency audit clean. Zero forbidden external engines detected.
      ```
    - `./scripts/verify_intent.sh`:
      ```
      Total Intents Evaluated: 8
      Passed Intents:          8/8
      Compliance Score:        100.0%
      [PASS] All Intent Contracts satisfied with 0 drift. Safe to proceed.
      ```
    - `PATH="$HOME/.cargo/bin:$PATH" cargo check --workspace --all-targets` and `PATH="$HOME/.cargo/bin:$PATH" cargo clippy --workspace --all-targets -- -D warnings`: exited 0 with 0 warnings.
    - `./scripts/verify_phase1.sh`: exited 0 with all 8 stages passing cleanly.

---

## 2. Logic Chain

1. **Premise 1 (Zero-Drift Baseline)**: Observation 10 demonstrates that the Phase 1 implementation satisfies all OpenSpec specifications, zero banned dependencies, zero compiler/clippy warnings, and achieves a 100.0% Intent Compliance Score.
2. **Premise 2 (Mathematical Readiness of R3)**: Observations 2, 3, and 4 establish that the core synchronization algorithms (NTP minimum-RTT filter, discrete PI PLL micro-speed controller, and barrier unpause timing) are mathematically validated, unit-tested, and fully functional in pure Rust.
3. **Premise 3 (Transport Gap in R3)**: Observations 1 and 9 reveal that `syncplay-sync` does not yet possess a binary wire packet codec (`WirePacket`) or UDP datagram socket transport engine (`UdpSyncTransport`), and `syncplay-core` runs a simulated loop instead of live peer communication.
4. **Premise 4 (Readiness and Dispatch Gap in R4)**: Observations 5, 6, 7, and 8 prove that `syncplay-chat` has an append-only binary log with false-sync resilience, an inverted index with Unicode CJK tokenization, and a slash-command parser. However, it lacks an active `SlashCommandDispatcher` routing parsed commands to player/network events, and network relay integration for chat messages.
5. **Premise 5 (CLI Implementation Gap in `syncplay-core`)**: Observation 9 demonstrates that `syncplay-alt inspect`, `play`, `party`, and `chat` need complete integration with the underlying demuxer, presentation pipeline, UDP network transport, and persistent storage.
6. **Deduction (Actionable Roadmap)**: Therefore, Phase 2 implementation can proceed immediately by dividing the work into 6 modular, decoupled work units that implement the wire protocol, socket transport, barrier coordination, chat dispatcher, CLI subcommands, and regression verification, without altering existing verified mathematical algorithms.

---

## 3. Caveats

1. **Network Environment Assumptions**: Real WAN testing involves NAT traversal, firewall configurations, and UDP packet loss. Phase 2 targets direct UDP datagram communication between peers or via a designated host server (e.g., LAN, port forwarding, or VPS host). STUN/TURN/ICE traversal is outside the Phase 2 scope.
2. **Audio/Video Codec State**: This survey focused on R3 (transport), R4 (chat), R5 (harness), and CLI in `syncplay-core`. Container demuxing (`syncplay-demux`) and frame color conversion (`syncplay-codecs`) are being surveyed by peer survey agents (`explorer_p2_survey_1` and `explorer_p2_survey_2`).
3. **Async Runtime Constraint**: In strict adherence to `AGENTS.md` and the Zero-External-Dependency invariant, asynchronous runtimes like `tokio` are forbidden. All networking must use non-blocking `std::net::UdpSocket` polling integrated into the coordinator loop.

---

## 4. Conclusion

1. **Phase 1 Baseline is 100% Intact**: All foundational primitives and mathematical invariants pass cleanly with zero compiler warnings, zero clippy warnings, zero banned dependencies, and a 100.0% Intent Compliance Score.
2. **Phase 2 Blueprint is Concrete**:
   - **`syncplay-sync`**: Add `wire.rs` (defining `WirePacket` for probe, room state, barrier, and commands with `SPNT` framing and CRC32) and `transport.rs` (providing non-blocking `UdpSyncTransport`, `HostServer`, and `ClientNode`).
   - **`syncplay-chat`**: Add `commands/dispatch.rs` (implementing `SlashCommandDispatcher` mapping slash commands to player and network events) and network chat relay.
   - **`syncplay-core`**: Connect CLI subcommands (`inspect`, `play`, `party host|connect`, `chat query|export`) to the real demuxer, presentation loop, UDP transport, and binary chat log.
   - **Pi Framework**: Enforce continuous 100% compliance with `scripts/verify_intent.sh` and `scripts/verify_phase1.sh`.

---

## 5. Verification Method

To independently verify all observations and conclusions:

```bash
# 1. Verify Zero External Dependencies
./scripts/audit_dependencies.sh

# 2. Verify Compilation and Zero Warnings
PATH="$HOME/.cargo/bin:$PATH" cargo check --workspace --all-targets
PATH="$HOME/.cargo/bin:$PATH" cargo clippy --workspace --all-targets -- -D warnings

# 3. Verify All Workspace Unit & Integration Tests
PATH="$HOME/.cargo/bin:$PATH" cargo test --workspace -- --nocapture

# 4. Verify Pi Intent Verification Harness (100.0% Compliance)
./scripts/verify_intent.sh

# 5. Verify Full Phase 1 Verification Script
./scripts/verify_phase1.sh

# 6. Verify CLI Executable Commands
PATH="$HOME/.cargo/bin:$PATH" cargo run -p syncplay-core -- inspect synthetic-mkv
PATH="$HOME/.cargo/bin:$PATH" cargo run -p syncplay-core -- play synthetic-mkv
PATH="$HOME/.cargo/bin:$PATH" cargo run -p syncplay-core -- party host
PATH="$HOME/.cargo/bin:$PATH" cargo run -p syncplay-core -- chat demo
PATH="$HOME/.cargo/bin:$PATH" cargo run -p syncplay-core -- verify
```

### Invalidation Conditions:
- Any appearance of forbidden crates (`ffmpeg`, `vlc`, `sqlite`, `tokio`, etc.) in `Cargo.lock`.
- Any compiler or clippy warning emitted under `#![deny(warnings)]`.
- Any Intent Compliance Score below 100.0% reported by `python3 scripts/intent_harness.py`.
- Any regression in the mathematical convergence proof of the discrete PI PLL or RFC 5905 NTP filter.
