# Change Proposal: 005-smart-chat-storage

**Change ID**: `005-smart-chat-storage`  
**Title**: Locally Persisted Smart Chat, Binary Log Engine & Inverted Index FTS  
**Status**: Ready for Implementation  
**Target Milestone**: Phase 1 Foundational Primitives  

---

## 1. Context & Motivation

Watch-party participants require rich in-player communication including timeline-anchored annotations, instant full-text search over chat history, and interactive media slash-commands. Relying on SQLite or external databases compromises portability and violates the zero-dependency invariant. This change implements a custom crash-resilient append-only binary log and an in-memory inverted index from scratch.

---

## 2. Dependency Graph

```
[001-demuxer-bitstream] ──┐
                          ├─► [005-smart-chat-storage] (syncplay-chat)
[004-network-sync-protocol] ┘          │
                                       ▼
                             [006-ui-osd-renderer]
```

- **Dependencies**: `001-demuxer-bitstream`, `004-network-sync-protocol`.
- **Blocks**: `006-ui-osd-renderer`.

---

## 3. Affected Crates & Components

- **`crates/syncplay-chat`**:
  - `src/storage/log.rs`: `syncplay_chat.bin` append-only binary log writer and header validator (`SPCH`).
  - `src/storage/record.rs`: Binary wire layout, `0xAA55` framing, dual timestamps, and CRC32 checks.
  - `src/storage/recovery.rs`: Torn-write detection and byte-by-byte forward-scanning resynchronization state machine treating oversized lengths as false sync words.
  - `src/index/tokenizer.rs`: Unicode lowercase folding, punctuation stripping, CJK unigram/bigram segmentation (`U+4E00`–`U+9FFF`), and single-ideograph length exemption.
  - `src/index/inverted.rs`: Posting lists, two-pointer Boolean AND intersection, and BM25 ranking.
  - `src/index/timeline.rs`: Spatial timeline queries (`query_timeline_window`) for bullet comment overlays.
  - `src/commands/parser.rs`: Slash-command grammar lexer (`/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`).
  - `src/commands/dispatch.rs`: Media trigger router emitting sync wire events.
  - `src/export/mod.rs`: JSON and Markdown room transcript generators.

---

## 4. Deliverables & Implementation Plan

1. **Custom Binary Log (`syncplay_chat.bin`)**:
   - 16-byte file header with magic `b"SPCH"` and header CRC32.
   - Record serialization with `[0xAA, 0x55]` framing and IEEE 802.3 CRC32 verification.
   - Dual-temporal coordinates ($t_{wall}, t_{media}$) preserving absolute UTC history and playback PTS.
   - Recovery scanner salvaging valid records upon detecting torn writes at EOF, with false-sync word rejection preventing premature truncation.
2. **Inverted Index & Full-Text Search**:
   - Inverted Index & Full-Text Search with Unicode CJK n-gram segmentation and single-ideograph indexing.
   - $O(\sum |L_i|)$ two-pointer linear merge for Boolean AND queries.
   - Spatial timeline queries over `BTreeMap` in $O(\log N + K)$ time for floating comments.
3. **Slash-Command Parser & Media Triggers**:
   - Normalizing timecodes (`01:23:45`, `14:30`, `+15s`, `-10s`) into absolute milliseconds.
   - Dispatching `/seek` and `/bookmark` events directly to `syncplay-sync`.
4. **History Exporters**:
   - Machine-readable JSON array and human-readable Markdown transcript formats.

---

## 5. Verification Commands

Execute the following commands from the repository root:

```bash
# 1. Type and borrow check affected crate
cargo check -p syncplay-chat

# 2. Run unit tests including binary log persistence, recovery, FTS, and command parsing
cargo test -p syncplay-chat -- --nocapture

# 3. Verify zero database or external search dependencies (no sqlite, tantivy, etc.)
cargo tree -p syncplay-chat --edges no-dev
```

Expected output: All unit tests pass; recovery tests successfully salvage records after simulated torn writes at EOF; inverted index returns correct Boolean search results.
