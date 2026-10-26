## 2026-10-25T17:17:16Z
You are `worker_p3_network` (role: Implementation Worker M3 Watch-Party Networking & In-Band Smart Chat).
Your working directory is: `/home/gautham/syncplay-alt/.agents/teamwork/worker_p3_network`.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

FIRST: Read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`.
Read the survey report: `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_survey_3/handoff.md`.

Your mission is to implement Phase 3 Requirement R4: Full Interactive Watch-Party Networking & In-Band Smart Chat:
1. `syncplay-sync` Wire Protocol & Chat Transport:
   - In `crates/syncplay-sync/src/wire.rs`, add `PacketType::ChatMessage = 0x08` (NOTE: `0x07` is tested as an unknown bad type in `watch_party_transport.rs`, so `0x08` must be used).
   - Define `ChatMessagePayload` or integrate wire packet serialization for chat messages with sequence number, timestamps, and payload bytes.
   - In `HostServer` (`crates/syncplay-sync/src/host.rs`):
     * Handle incoming `ChatMessage` packets from any connected peer.
     * Re-broadcast / relay the chat packet to all other connected peers (avoid echoing back to sender).
     * Expose a method to query or drain received chat messages (`poll_chat_messages()`).
     * Allow the host itself to broadcast a chat message (`broadcast_chat_message()`).
   - In `ClientNode` (`crates/syncplay-sync/src/client.rs`):
     * Handle incoming `ChatMessage` packets from the host.
     * Expose a method to drain received chat messages (`poll_chat_messages()`).
     * Allow sending a chat message to the host (`send_chat_message(&mut self, payload: &[u8])`).
2. `syncplay-chat` In-Band Integration:
   - Ensure chat messages sent/received over UDP can be constructed from or converted to `ChatRecord` / `ChatNetworkPacket`.
   - On receipt of chat messages, append to local `ChatStore` (`syncplay_chat.bin` with `0xAA55` framing, IEEE 802.3 CRC32, dual timestamps $t_{wall}, t_{media}$, UUID sender ID).
   - In-memory inverted index FTS with Unicode CJK unigram/bigram segmentation and slash commands (`/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`).
3. `syncplay-core` CLI & Party Runtime:
   - In `crates/syncplay-core/src/main.rs`:
     * Parse `syncplay-alt party host --port <PORT> --file <MEDIA>` and `syncplay-alt party connect <HOST:PORT> --file <MEDIA>`.
     * Maintain backward-compatible fallback for positional arguments (`party host [PORT]` and `party connect [ADDR]`).
   - In `crates/syncplay-core/src/coordinator.rs` (and/or dedicated party runtime):
     * Link UDP network events with `MasterClock`, `ChatStore`, and UI snapshots.
     * For non-interactive / automated tests (e.g. `test_core_adversarial_p2.py` or stdin is not a TTY), do NOT hang on stdin; execute verification polls cleanly and exit or handle gracefully.
4. Comprehensive Testing:
   - Write integration tests in `crates/syncplay-sync/tests/` and/or `crates/syncplay-core/tests/` testing:
     * Multi-client watch party with 1 host and 2+ clients.
     * NTP clock synchronization running concurrently with chat exchange.
     * Bidirectional in-band chat messaging and host relaying.
     * Local chat store persistence (`syncplay_chat.bin`) and FTS search verification.
     * CLI argument parsing for `--port` and `--file`.
5. Strict Quality Invariants:
   - Zero compiler warnings (`cargo check --workspace --all-targets`).
   - Zero clippy warnings (`cargo clippy --workspace --all-targets -- -D warnings`).
   - Zero banned external dependencies (`./scripts/audit_dependencies.sh`).
   - 100.0% compliance on the Pi Intent Harness (`python3 scripts/intent_harness.py`).
   - All tests pass (`cargo test --workspace`).

## 2026-10-25T17:28:07Z
**Context**: Milestone 3 Implementation (`worker_p3_network`)
**Content**: Checking in on your implementation progress for M3 (Watch-Party UDP Networking & In-Band Smart Chat). How is the wire protocol (`PacketType::ChatMessage = 0x08`), host relay, client chat transport, and CLI `--port`/`--file` parsing progressing?
**Action**: Please reply with a brief status update or proceed with implementation and deliver your handoff report when complete.
