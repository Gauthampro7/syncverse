## 2026-10-25T11:20:48Z
You are worker_p2_chat, an expert systems engineer in pure Rust.
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_chat/`.

MANDATORY FIRST STEP:
Read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` completely before starting any work.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Scope & Task for Milestone 4 (Smart Chat, Binary Log, Inverted Index & Slash Commands in `crates/syncplay-chat`):
1. **Zero External Database / Search Frameworks**:
   - Strictly NO `sqlite`, NO `sled`, NO `rocksdb`, NO `tantivy`, NO `meilisearch`.
   - Pure-Rust append-only binary log `syncplay_chat.bin` with `0xAA55` framing and IEEE 802.3 CRC32.
2. **Review and Complete `syncplay-chat` Subsystem**:
   - Inspect existing modules in `crates/syncplay-chat/src/`: `record.rs`, `storage.rs`, `index.rs`, `commands.rs`.
   - Implement `SlashCommandDispatcher` (e.g. in `src/commands/dispatch.rs` or `src/commands.rs`):
     - Maps `/seek <time>` -> seek event to target PTS in milliseconds.
     - Maps `/bookmark [label]` -> creates media timestamp bookmark.
     - Maps `/clip <start> <end> [title]` -> creates clip definition with start/end media timestamps.
     - Maps `/poll <question> | <opt1> | <opt2> ...` -> creates active poll struct.
     - Maps `/sync-status` -> queries sync state and formats report string.
   - Ensure `ChatStore` / `InvertedIndexFts` supports:
     - Unicode case folding (Latin, Turkish dotted I).
     - CJK unigrams and overlapping bigrams (`U+4E00`..`U+9FFF`) with single ideograph length exemption (1..48 chars).
     - Timeline window queries (`query_timeline_window(media_ts, window_ms)`) over `BTreeMap` in $O(\log N + K)$ time.
     - Crash recovery scanner forward-scanning byte-by-byte on false-sync candidates without corrupting valid trailing records.
   - Chat network packet encoding:
     - Provide serialization of `ChatRecord` to/from network bytes for watch-party transmission over UDP `WirePacket` (or a dedicated chat packet type).
3. **Re-export and Integration in `crates/syncplay-chat/src/lib.rs`**:
   - Expose `SlashCommandDispatcher`, `DispatchAction`, `ChatStore`, `ChatRecord`, `ChatRecoveryScanner`, `InvertedIndexFts`, `ParsedCommand`.
4. **Comprehensive Unit & Adversarial Tests**:
   - Unit tests for slash command dispatching and timestamp conversions.
   - False-sync fuzzing test: Inject corrupt bytes, false `0xAA55` patterns, truncated records, and verify exact byte-by-byte recovery.
   - CJK multi-term query tests with mixed English and Chinese/Japanese/Korean text.
5. **Empirical Verification Standards**:
   - `cargo check --workspace --all-targets` (0 warnings)
   - `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings)
   - `cargo test -p syncplay-chat -- --nocapture` (all tests pass)
   - `python3 tests/empirical_challenge/test_smart_chat.py` (must pass cleanly)
   - `./scripts/audit_dependencies.sh` (clean exit 0, zero forbidden crates)
   - `python3 scripts/intent_harness.py` (100.0% score)
   - `./scripts/verify_phase1.sh` (all 8 stages pass cleanly)
6. **Deliver**:
   - Write comprehensive handoff report to `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_chat/handoff.md`.
   - Notify parent orchestrator via `send_message`.
