# Workflow: 005-chat-workflow

## 1. Metadata & OpenSpec Mapping
- **Workflow ID**: `005-chat-workflow`
- **Target Change Proposal**: `openspec/changes/005-smart-chat-storage.md`
- **Capability Specification**: `openspec/specs/smart-chat/spec.md`
- **Target Crates**: `syncverse-chat`
- **Primary Agents**: `spec_implementer`, `storage_auditor`
- **Required Skills**: `openspec-change-mapping`, `zero-dependency-audit`

---

## 2. Prerequisites & Dependencies
- **Upstream Dependencies**: `001-demuxer-workflow`, `004-sync-workflow`.
- **Blocks**: `006-ui-workflow`.
- **Pre-flight Checks**:
  1. Confirm `syncverse-common` CRC32 and timestamps are functional.
  2. Confirm zero external database libraries (`sqlite`, `rocksdb`, etc.).

---

## 3. Step-by-Step Execution Plan

### Step 1: Binary Log Storage Engine (`syncverse-chat/src/storage/`)
1. Implement 16-byte fixed file header (`log.rs`):
   - Magic `b"SPCH"`, version `1u16`, flags `0u16`, reserved `0u32`, header CRC32.
2. Implement record framing and serialization (`record.rs`):
   - `[0xAA, 0x55]` sync word (2 bytes), record total length (4 bytes LE, total record size = len(body) + 10 >= 48 bytes), body, body CRC32 (4 bytes).
   - Dual-temporal coordinates ($t_{wall}, t_{media}$) preserving absolute UTC history and playback PTS.
   - 16-byte UUID sender identifier and UTF-8 payload.

### Step 2: Forward-Scanning Recovery State Machine (`syncverse-chat/src/storage/recovery.rs`)
1. Implement byte-by-byte forward scanner:
   - Identify candidate sync words `0xAA55`.
   - Validate record length and body CRC32.
2. Implement crash recovery edge cases:
   - **Torn Write at EOF**: Detect incomplete trailing record, truncate/ignore it safely, preserving all prior records.
   - **Bit Corruptions**: Advance byte-by-byte past corrupted regions and resume valid record extraction.
   - **False-Sync Resilience**: When payload contains `0xAA55` followed by an invalid length (`record_total_len < 48` or exceeding remaining file bytes) or CRC mismatch, treat as a false candidate; advance by 1 byte without aborting recovery or dropping subsequent records.

### Step 3: Unicode Inverted Index & Full-Text Search (`syncverse-chat/src/index/`)
1. Implement tokenizer (`tokenizer.rs`):
   - Case folding supporting Latin and multi-byte Unicode (e.g., Turkish dotted capital `İ` $\to$ `i`).
   - Punctuation stripping with `#` (hashtags) and `@` (mentions) explicitly preserved.
   - CJK unigram and overlapping bigram segmentation (`U+4E00`–`U+9FFF`).
   - Single-character ideograph indexing (length bounds 1..48 for CJK, 2..48 for Western).
2. Implement inverted index (`inverted.rs`):
   - `BTreeMap<String, Vec<u64>>` posting lists.
   - Two-pointer linear merge for Boolean AND queries in $O(\sum |L_i|)$ time.
3. Implement spatial timeline query (`timeline.rs`):
   - `query_timeline_window(media_ts, window_ms)` over `BTreeMap` in $O(\log N + K)$ time for floating comments.

### Step 4: Slash-Command Parser & Media Triggers (`syncverse-chat/src/commands/`)
1. Implement grammar parser (`parser.rs`):
   - `/seek <timecode>`: Support `HH:MM:SS`, `MM:SS`, `+15s`, `-10s` converted to absolute milliseconds.
   - `/bookmark [label]`, `/clip <duration>`, `/poll "<question>" "opt1" "opt2"`, `/sync-status`.
2. Implement dispatcher (`dispatch.rs`) translating commands into sync wire events.

### Step 5: History Exporters (`syncverse-chat/src/export/`)
Implement JSON and Markdown chat transcript generators.

---

## 4. Verification & Acceptance Criteria
Execute the automated test suite:
```bash
# Crate unit test verification
cargo test -p syncverse-chat -- --nocapture

# Adversarial storage recovery & CJK FTS test
python3 tests/empirical_challenge/test_smart_chat.py

# Lint inspection
cargo clippy -p syncverse-chat --all-targets -- -D warnings
```
Criteria for success:
- 100% test pass rate with 0 errors.
- Verified recovery from torn writes, bit flips, and false-sync payload markers.
- Verified CJK unigram/bigram search matching and single-ideograph retrieval.
- 0 compiler warnings.
