# Capability Specification: Smart In-Player Chat & Storage Engine (`syncplay-chat`)

**Component**: `syncplay-chat`  
**Standards Compliance**: Custom Binary Framing (`SPCH`), IEEE 802.3 CRC32, Unicode Standard Case Folding  
**Status**: Authoritative OpenSpec Specification  

---

## 1. Overview & Architectural Role

The `syncplay-chat` crate provides a zero-dependency, locally persisted in-player chat system, full-text search engine, and slash-command media control interface. Unlike external chat implementations that embed heavyweight SQL engines (`sqlite`, `rocksdb`) or separate daemon services, `syncplay-chat` is implemented natively in pure Rust.

The engine features:
1. **Dual-Temporal Coordinate Model**: Every record anchors to both physical UTC wall time ($t_{wall}$) and media playback presentation timestamp ($t_{media}$).
2. **Crash-Resilient Append-Only Binary Log (`syncplay_chat.bin`)**: High-throughput binary record persistence with `0xAA55` synchronization framing, CRC32 integrity validation, and torn-write auto-recovery.
3. **In-Memory Inverted Index & FTS**: Tokenization pipeline with Unicode folding, stop-word filtering, multi-term Boolean AND intersection, and temporal range queries for on-screen bullet comment overlays.
4. **Smart Slash-Command Console**: Grammars and event triggers for `/seek`, `/bookmark`, `/clip`, `/poll`, and `/sync-status` coordinating directly with the network synchronization protocol.

---

## 2. Requirements & Scenarios

### Requirement: Dual-Temporal Coordinate Anchoring
Every chat record MUST record two distinct 64-bit integer timestamps:
1. `wall_timestamp_utc_ms` ($t_{wall}$): Physical UTC epoch timestamp in milliseconds recorded upon message generation or arrival.
2. `media_timestamp_ms` ($t_{media}$): Playback presentation timestamp (PTS in milliseconds) active at the moment the message was dispatched.

If a message is sent while media playback is inactive (e.g., in a pre-watch lobby), the record MUST store the sentinel value `u64::MAX` (`0xFFFF_FFFF_FFFF_FFFF`).

#### Scenario: Anchor In-Player Reaction to Video Timeline
- **Given** video playback currently paused at presentation time $00:14:32.100$ ($872,100\text{ ms}$)
- **When** a user submits a chat reaction `"🔥"`
- **Then** the created record has `media_timestamp_ms = 872100`
- **And** `wall_timestamp_utc_ms` contains the current UTC epoch time
- **And** the reaction can be retrieved by seeking to $00:14:32$ on the media timeline.

#### Scenario: Handle Unanchored Lobby Message
- **Given** a watch-party room with no active media loaded
- **When** a user sends `"Hello everyone"`
- **Then** the created record has `media_timestamp_ms = u64::MAX`
- **And** the message is indexed for chronological chat history but excluded from media overlay queries.

---

### Requirement: Binary Log File Header & Record Wire Layout
The storage engine SHALL write records to an append-only file named `syncplay_chat.bin`. The file MUST start with a 16-byte fixed header:
- `magic`: `[u8; 4]` = `b"SPCH"` (`0x53, 0x50, 0x43, 0x48`)
- `version`: `u16` little-endian = `0x0001`
- `flags`: `u16` little-endian
- `reserved`: `u32` little-endian = `0x00000000`
- `header_crc32`: `u32` little-endian (IEEE 802.3 CRC32 over the preceding 12 bytes).

Every record written to the log MUST begin with a 2-byte sync word `[0xAA, 0x55]`, followed by:
- `record_total_len`: `u32` LE (total record byte size, $\ge 48\text{ bytes}$)
- `record_type`: `u8`
- `flags`: `u8`
- `wall_timestamp_utc_ms`: `u64` LE
- `media_timestamp_ms`: `u64` LE
- `sender_id`: `[u8; 16]` (UUIDv4)
- `sender_name_len`: `u8`
- `sender_name`: UTF-8 bytes ($1 \le len \le 64$)
- `payload_len`: `u32` LE
- `payload`: raw payload bytes
- `crc32`: `u32` LE (IEEE 802.3 CRC32 calculated over all bytes from `record_type` through the last byte of `payload`).

#### Scenario: Validate Binary Record Serialization and Checksum
- **Given** a chat message with sender `"Alice"`, text `"Hello world"`, and media PTS $12,000\text{ ms}$
- **When** serialized to binary bytes
- **Then** the byte stream starts with `0xAA, 0x55`
- **And** the trailing 4 bytes match the IEEE 802.3 CRC32 of the record body
- **And** total length accurately reflects the variable payload and header size.

---

### Requirement: Crash Recovery & Torn-Write Resynchronization
The recovery scanner SHALL validate the 16-byte fixed file header and sequentially scan records from byte offset 16 to the physical end of file (`file_len`). To prevent premature truncation caused by false sync markers in corrupted payloads, the scanner MUST adhere to the following resynchronization state machine:

1. **Mid-Stream Byte Corruption & False-Sync Disambiguation**:
   - If the 2-byte sync word `[0xAA, 0x55]` is not present at `current_offset`, the scanner MUST advance `current_offset` by 1 byte and increment the corrupted byte count.
   - If `[0xAA, 0x55]` is present, but reading the subsequent 4-byte `record_total_len` would exceed `file_len`, or `record_total_len < 48`, or `current_offset + record_total_len > file_len`, the scanner MUST NOT terminate recovery or truncate the file. The scanner MUST classify this occurrence as a **false sync word** or corrupted candidate, advance `current_offset` by 1 byte, increment `RecoveryStats::corrupted_chunks`, and continue forward scanning for subsequent valid records.
   - If the candidate record's payload is fully readable within `file_len` but its CRC32 checksum fails, the scanner MUST advance `current_offset` by 1 byte, increment `RecoveryStats::corrupted_chunks`, and resume byte-by-byte scanning.
2. **Authoritative Torn Write at EOF**:
   - A torn write at EOF is declared IF AND ONLY IF the scanner reaches the physical end of file (`current_offset == file_len` or `current_offset + 6 > file_len`) with unparsed trailing bytes following the last validated record, and no subsequent valid `0xAA55` record could be established.
   - In this state, the scanner SHALL preserve all preceding valid records, increment `RecoveryStats::torn_writes` by 1, and successfully finalize database recovery.

#### Scenario: Recover Log With Incomplete Record at EOF
- **Given** a `syncplay_chat.bin` file containing 10 valid records followed by an incomplete record whose trailing 20 bytes were truncated during power loss
- **When** `recover_and_index` is invoked
- **Then** exactly 10 valid records are recovered and inserted into the inverted index
- **And** `RecoveryStats::torn_writes` equals 1
- **And** the parser terminates successfully without panicking.

#### Scenario: Resilient Recovery Across Mid-Stream Corruption and False Sync Words
- **Given** a `syncplay_chat.bin` file containing 2 valid records, followed by a corrupted payload containing the bytes `0xAA55` with an oversized length field (`record_total_len > file_len`), followed by 7 valid subsequent records
- **When** `recover_and_index` is invoked
- **Then** all 9 valid records (records 0, 1, and 3 through 9) are fully recovered and indexed
- **And** `RecoveryStats::corrupted_chunks` is at least 1
- **And** the false sync word does NOT abort recovery or drop subsequent records.

---

### Requirement: In-Memory Inverted Index & Full-Text Search
The chat subsystem SHALL maintain an in-memory inverted index for all valid text messages. To support multi-language watch parties without external database dependencies, the tokenization pipeline MUST:
1. Apply Unicode case folding to convert text to lowercase (handling multi-character expansions such as `İ` $\to$ `i\u0307`).
2. Strip punctuation marks while preserving hashtags (`#`), user mentions (`@`), and Unicode alphanumeric characters.
3. Perform language-aware token segmentation:
   - **CJK Segmentation**: For characters in the CJK Unified Ideographs block (`U+4E00`–`U+9FFF`), generate character unigrams ($c_i$) and overlapping bigrams ($c_i c_{i+1}$) across unspaced ideograph sequences.
   - **Western / Non-CJK Words**: Split along whitespace and boundary punctuation.
4. Apply token length bounds:
   - For Western / non-CJK tokens: discard tokens shorter than 2 characters or longer than 48 characters.
   - For CJK ideographs (`U+4E00`–`U+9FFF`): exempt from the 2-character minimum, indexing single CJK ideographs ($1 \le \text{char\_count} \le 48$).
5. Remove standard stop-words from Western tokens (`["the", "is", "at", "which", "on", "and", "a", "an", "to", "for", "in", "of", "it", "this", "that", "with", "as", "by"]`). Stop-word filtering SHALL NOT discard CJK ideographs.

The search engine SHALL execute multi-term Boolean AND queries across posting lists using linear two-pointer intersections in $O(\sum |L_i|)$ time.

#### Scenario: Search Chat Messages via Multi-Term Boolean AND
- **Given** an indexed chat store containing:
  - Message 1: `"Check out that awesome guitar solo at 14:30!"`
  - Message 2: `"The guitar tone is okay, but the bass is awesome."`
  - Message 3: `"Check out the drums at 20:00."`
- **When** searching for `"guitar awesome"`
- **Then** both Message 1 and Message 2 are returned
- **And** Message 3 is excluded because it lacks the term `"guitar"`.

#### Scenario: Search CJK Chat Messages via Unigram and Bigram Tokenization
- **Given** an indexed chat store containing:
  - Message 1: `"这是一段非常精彩的吉他独奏"`
  - Message 2: `"我喜欢猫"`
- **When** searching for CJK bigram `"吉他"`
- **Then** Message 1 is returned
- **When** searching for single CJK ideograph `"猫"`
- **Then** Message 2 is returned because single CJK characters are preserved as valid index tokens.

---

### Requirement: Timeline Window Range Queries for OSD Overlays
The index SHALL support temporal spatial queries (`query_timeline_window(current_pts, past_ms, future_ms)`) over an internal `BTreeMap<u64, Vec<u64>>` mapping media timestamps to record byte offsets.
The query MUST return all active records in the range:
$$[\text{current\_pts} - \text{past\_ms}, \, \text{current\_pts} + \text{future\_ms}]$$
The operation MUST execute in $O(\log N + K)$ time, where $N$ is total indexed timestamps and $K$ is the number of matching records in the window. Records flagged as deleted (tombstone bit set) MUST be excluded.

#### Scenario: Retrieve Floating Overlay Comments for Current Video Time
- **Given** chat records anchored at media timestamps:
  - Record A: $10,000\text{ ms}$
  - Record B: $12,500\text{ ms}$
  - Record C: $18,000\text{ ms}$
- **When** querying with `current_pts = 12000`, `past_ms = 3000`, `future_ms = 1000` (window: $[9000, 13000]$)
- **Then** Record A ($10,000$) and Record B ($12,500$) are returned
- **And** Record C ($18,000$) is excluded.

---

### Requirement: Slash-Command Lexer & Media Triggers
The chat engine SHALL parse slash-commands matching the following grammar:
- `/seek <time>`: Parses `HH:MM:SS`, `MM:SS`, decimal seconds, or relative offsets (`+15s`, `-10s`). Emits a seek synchronization event.
- `/bookmark [label]`: Creates a timeline bookmark at current playback PTS.
- `/clip <start> <end> [label]`: Defines a shared media clip interval.
- `/poll <"Question"> <"Opt1"> <"Opt2"> ...`: Initiates a room voting poll.
- `/sync-status`: Generates a local diagnostic report detailing current RTT, clock offset $\theta$, PLL speed multiplier, and peer status.

Commands MUST be strictly validated before dispatch. Invalid commands SHALL return descriptive syntax error messages without mutating room state.

#### Scenario: Parse Seek Slash-Commands in Various Formats
- **Given** the chat command input `"/seek 01:23:45.500"`
- **When** parsed by the slash-command lexer
- **Then** output is `ParsedSlashCommand::Seek { target_pts_ms: 5025500 }`
- **Given** the chat command input `"/seek 14:30"`
- **When** parsed by the slash-command lexer
- **Then** output is `ParsedSlashCommand::Seek { target_pts_ms: 870000 }`
- **Given** relative seek `"/seek +15s"` at current position $100,000\text{ ms}$
- **When** parsed with context
- **Then** output is `ParsedSlashCommand::Seek { target_pts_ms: 115000 }`.

---

### Requirement: Deterministic History Export
The chat engine SHALL support exporting stored conversation history in two standard formats:
1. **JSON Export**: An array of structured JSON objects containing sender UUID, sender name, ISO-8601 UTC timestamp, `HH:MM:SS.mmm` formatted media timestamp, and payload.
2. **Markdown Transcript**: Human-readable chronological text log with synchronized headers.

#### Scenario: Export Room Transcript to Markdown
- **Given** a session with two anchored messages
- **When** `export_transcript(TranscriptFormat::Markdown)` is invoked
- **Then** the generated string contains header `# Syncplay-Alt Room Chat Transcript`
- **And** each line includes the formatted media timestamp `[HH:MM:SS.mmm]` and sender handle `<Name>`.
