## 2026-10-24T23:03:24Z

You are explorer_survey_3 (teamwork_preview_explorer).
Your Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_3/
Project Root: /home/gautham/syncplay-alt
Authoritative User Request: /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md

### Objective
Conduct an exhaustive technical exploration and survey for the smart chat system, multi-crate Rust workspace architecture, Pi agent framework, and verification harness of Syncplay-Alt:
1. Locally persisted smart in-player chat system (timestamp-anchored messages, custom binary log + indexed storage engine with full-text search, slash-commands `/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`).
2. Pure-Rust multi-crate workspace scaffold with strict zero-dependency compliance on external media/codec/database engines (`ffmpeg`, `libav*`, `gstreamer`, `mpv`, `vlc`, `symphonia`, `dav1d`, `sqlite`, `rusqlite`, `sqlx`).
3. Pi framework setup (`AGENTS.md` and `.pi/`) and automated verification harness (`scripts/verify_phase1.sh`).

### Scope & Constraints
- Read-only technical investigation. Do NOT write or modify application source code.
- Write your working files exclusively in your working directory `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_3/`.
- Prioritize custom zero-dependency implementations (no SQLite/RocksDB/Tantivy; use custom append-only binary log + in-memory inverted index).

### Detailed Requirements to Survey & Specify
1. **Smart In-Player Chat & Custom Storage Engine**:
   - Custom Binary Log Format (`syncplay_chat.bin`):
     * Header: Magic bytes `b"SPCH"`, version (u16), flags (u16), reserved (u32).
     * Record layout: length (u32), record type (u8: message, reaction, marker, poll), wall_timestamp_utc_ms (u64), media_timestamp_ms (u64), sender_id (16-byte UUID / 32-byte string), payload_len (u32), payload bytes, crc32 checksum (u32).
     * Recovery & corrupt record handling: forward scan with CRC validation, seeking to next valid magic/boundary on corruption.
   - Inverted Index & Full-Text Search (FTS):
     * Tokenization (Unicode lowercase, punctuation stripping, stop-word handling).
     * Posting list: `Term -> Vec<(RecordOffset, MediaTimestampMs)>`.
     * Query parser: prefix matching, multi-term intersection (AND), phrase search.
     * Timeline range queries: querying chat messages within `[T_media - window, T_media + window]` for on-screen overlay display.
     * Room history export: formatted JSON and markdown/text export formats.
   - Smart Slash-Commands & Media Triggers:
     * Parser grammar for `/seek <timestamp_or_hms>`, `/bookmark [optional_label]`, `/clip <start_time> <end_time>`, `/poll <"Question"> <"Opt1"> <"Opt2">...`, `/sync-status`.
     * Media command dispatching & sync message event mapping.
2. **Multi-Crate Workspace Architecture & Dependency Rules**:
   - Clean workspace structure:
     * `crates/syncplay-common`: Core types, timestamps, error handling, CRC32, serialization primitives.
     * `crates/syncplay-demux`: EBML/MKV and MP4/ISOBMFF container demuxing traits & parsers.
     * `crates/syncplay-codecs`: Codec interfaces, YUV420p/RGB frame definitions, pixel converters.
     * `crates/syncplay-clock`: Monotonic clock, PTS tracking, audio/video synchronization ring buffer.
     * `crates/syncplay-sync`: NTP clock filter, PLL speed controller, timeline equation, barrier unpause state machine.
     * `crates/syncplay-chat`: Custom binary log engine, inverted index, slash command parser, history exporter.
     * `crates/syncplay-ui`: UI state model, OSD overlay layout primitives.
     * `crates/syncplay-core`: Top-level coordinator engine binding the crates together.
   - Zero-dependency audit script: grep/cargo metadata inspection to ensure zero prohibited crates.
3. **Pi Framework (`.pi/` & `AGENTS.md`) & `scripts/verify_phase1.sh` Architecture**:
   - Required skills, prompts, and workflows in `.pi/`.
   - Structure of `scripts/verify_phase1.sh`: OpenSpec validation, Pi framework verification, dependency check, `cargo test --workspace`, test vector execution, and deterministic network sync simulation (250-400ms RTT, ±30ms jitter, PLL error < 15ms, barrier unpause < 10ms).

### Deliverables
Write a comprehensive report to `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_3/analysis.md` and complete a structured handoff report in `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_3/handoff.md`.
Notify the orchestrator via `send_message` when complete.
