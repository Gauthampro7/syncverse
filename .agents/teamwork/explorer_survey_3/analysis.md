# Exhaustive Technical Survey & System Specification: Smart Chat, Workspace Scaffold, Pi Framework & Verification Harness

**Author**: `explorer_survey_3` (teamwork_preview_explorer)  
**Date**: 2026-10-24T23:06:00Z  
**Project**: Syncplay-Alt (Phase 1 Foundational Architecture)  
**Status**: Authoritative Technical Investigation & Architectural Blueprint  

---

## 1. Executive Summary & System Context

Syncplay-Alt is an open-source, ultra-efficient standalone media player and long-distance synchronized watch-party platform built in pure Rust. Unlike existing synchronization tools that operate as wrapper scripts around external video players (e.g. `mpv`, `vlc`) or rely on pre-built multimedia libraries (`ffmpeg`, `libav*`, `gstreamer`, `symphonia`), Syncplay-Alt implements its container demuxers, bitstream decoders, A/V presentation clock, long-distance network sync protocol, and locally persisted smart chat from the ground up with **zero external media or database engine dependencies**.

This technical exploration focuses on three core pillars of the Phase 1 deliverable:
1. **Locally Persisted Smart In-Player Chat & Custom Storage Engine**:
   - An append-only binary log format (`syncplay_chat.bin`) with dual-timestamp anchoring (physical wall-clock time and media presentation time), sync word framing (`0xAA55`), and strict CRC32 checksums for crash resilience.
   - A zero-dependency in-memory inverted index and full-text search (FTS) engine featuring Unicode normalization, multi-term Boolean AND intersection, prefix matching, and temporal timeline range queries (`[T_media - window, T_media + window]`) for on-screen overlay display (e.g. "danmaku" bullet comments and bookmark annotations).
   - A smart slash-command parser and event dispatcher supporting `/seek`, `/bookmark`, `/clip`, `/poll`, and `/sync-status` mapped directly to party transport synchronization events.
2. **Pure-Rust Multi-Crate Workspace Architecture & Dependency Rules**:
   - A modular, compiling 8-crate workspace (`syncplay-common`, `syncplay-demux`, `syncplay-codecs`, `syncplay-clock`, `syncplay-sync`, `syncplay-chat`, `syncplay-ui`, and `syncplay-core`) enforcing a clean Directed Acyclic Graph (DAG) with explicit trait boundaries.
   - Strict zero-dependency compliance banning external media, codec, and database libraries (`ffmpeg`, `libav*`, `gstreamer`, `mpv`, `vlc`, `symphonia`, `dav1d`, `sqlite`, `rusqlite`, `sqlx`, `tantivy`, `rocksdb`).
3. **Pi Coding-Agent Framework (`.pi/` & `AGENTS.md`) & Automated Verification Harness (`scripts/verify_phase1.sh`)**:
   - Architectural invariants, specialized agent role definitions, execution skills, and OpenSpec-aligned workflows.
   - A self-contained, automated end-to-end verification harness executing OpenSpec validation, dependency audits, workspace compilation, synthetic binary parsing vectors, and a deterministic high-RTT network sync simulation verifying PLL convergence to $< 15\text{ms}$ error under 250–400ms RTT and $\pm 30\text{ms}$ jitter.

---

## 2. Smart In-Player Chat & Custom Storage Engine

### 2.1 Architectural Rationale: Zero-Dependency & Dual-Timestamp Model

Traditional chat implementations rely on general-purpose embedded relational databases (such as SQLite via `rusqlite` or `sqlx`) or key-value stores (such as RocksDB or Sled). In the context of a high-performance synchronized watch-party player:
- **Zero-Dependency Mandate**: C-FFI bindings (e.g. `libsqlite3`) compromise cross-compilation simplicity, introduce dynamic linking hazards, and violate the pure-from-scratch architectural constraint.
- **Access Pattern Optimization**: In-player watch party chat has distinct write and read characteristics:
  1. *Writes*: Strictly append-only sequential streams of messages, reactions, bookmarks, and poll events during playback.
  2. *Reads (Playback Overlay)*: Real-time queries anchored to the active media presentation time ($T_{media}$) fetching messages within a moving time window $[T_{media} - \Delta t, T_{media} + \Delta t]$ for on-screen subtitles/bullet comments.
  3. *Reads (Search & Transcripts)*: On-demand full-text search queries filtering by sender, keywords, or time range, and export to JSON/Markdown transcripts.
- **Dual-Timestamp Architecture**:
  Every record captures two distinct temporal coordinates:
  - `wall_timestamp_utc_ms` ($t_{wall}$): Physical UTC timestamp (milliseconds since Unix epoch) recorded by the client upon message broadcast/receipt. Guarantees immutable chronological ordering for room audit logs, conversation flow, and chat transcript exports.
  - `media_timestamp_ms` ($t_{media}$): Playback presentation timestamp (PTS in milliseconds from the start of the media file). If a message is sent while paused at minute 14:32.100, $t_{media} = 872100$. Unanchored messages (e.g. general lobby chat before media playback begins) use the sentinel value `u64::MAX` (`0xFFFF_FFFF_FFFF_FFFF`).

```
+---------------------------------------------------------------------------------------------------+
|                                      DUAL-TIMESTAMP MODEL                                         |
+---------------------------------------------------------------------------------------------------+
|  Lobby Chat (Unanchored):                                                                         |
|    wall_time = 1729814400000 (2026-10-24 23:00:00 UTC)                                            |
|    media_time = 0xFFFFFFFFFFFFFFFF (u64::MAX -> Not anchored to playback)                         |
|                                                                                                   |
|  In-Player Reaction (Anchored):                                                                   |
|    wall_time = 1729814725000 (2026-10-24 23:05:25 UTC)                                            |
|    media_time = 0x00000000000D4A60 (871008 ms -> 00:14:31.008 in video)                          |
+---------------------------------------------------------------------------------------------------+
```

---

### 2.2 Custom Binary Log Format (`syncplay_chat.bin`)

The storage engine persists records into an append-only binary file named `syncplay_chat.bin`. The format consists of a 16-byte fixed file header followed by variable-length binary records.

#### 2.2.1 File Header Specification (16 Bytes Fixed)

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|       Magic: 'S' 'P' 'C' 'H' (0x53, 0x50, 0x43, 0x48)         |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|          Version (u16)        |          Flags (u16)          |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                         Reserved (u32)                        |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                     Header CRC32 Checksum                     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

| Byte Offset | Field | Type | Description |
|---|---|---|---|
| `0..4` | `magic` | `[u8; 4]` | ASCII bytes `b"SPCH"` (`0x53, 0x50, 0x43, 0x48`). |
| `4..6` | `version` | `u16` (LE) | Format version: `0x0001` (Phase 1 specification). |
| `6..8` | `flags` | `u16` (LE) | Bitmask: Bit 0 = `COMPRESSION_ENABLED` (0 = raw), Bit 1 = `CRC_STRICT` (1 = mandatory), Bits 2..15 = Reserved. |
| `8..12` | `reserved` | `u32` (LE) | Must be initialized to `0x00000000`. |
| `12..16` | `header_crc32` | `u32` (LE) | IEEE 802.3 CRC32 calculated over bytes `0..12`. |

#### 2.2.2 Record Wire Layout

Every record begins with a 2-byte synchronization word `0xAA, 0x55` (binary `10101010 01010101`). This provides framing demarcation and permits the recovery parser to scan forward and re-align if bytes are corrupted.

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    Sync Word (0xAA, 0x55)     |       Record Total Length     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|   (cont.) Total Len (u32)     |  Record Type  |  Flags (u8)   |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                                                               |
+                 Wall Timestamp UTC ms (u64 LE)                +
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                                                               |
+                  Media Timestamp ms (u64 LE)                  +
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                                                               |
+                   Sender ID (16 bytes UUID)                   +
|                                                               |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
| Sndr Name Len |     Sender Name Bytes (0..64 UTF-8) ...       |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                      Payload Length (u32 LE)                  |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                   Payload Bytes (variable length)             |
|                                 ...                           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                      Record CRC32 Checksum                    |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

| Field Name | Type | Size (Bytes) | Semantics |
|---|---|---|---|
| `sync_word` | `[u8; 2]` | 2 | Constant `[0xAA, 0x55]`. Resync anchor. |
| `record_total_len` | `u32` | 4 | Total byte count of the record (from `sync_word` to end of `crc32`). Must be $\ge 48$. |
| `record_type` | `u8` | 1 | Enumeration tag identifying record payload schema. |
| `flags` | `u8` | 1 | Bit 0: `TOMBSTONE` (1 = deleted); Bit 1: `EDITED` (1 = replaces prior record); Bit 2: `TIMELINE_ANCHORED`; Bit 3: `PINNED`. |
| `wall_timestamp_utc_ms` | `u64` | 8 | Milliseconds since Unix Epoch (UTC). Little-endian. |
| `media_timestamp_ms` | `u64` | 8 | Playback presentation time in milliseconds. Sentinel `u64::MAX` if unanchored. |
| `sender_id` | `[u8; 16]` | 16 | UUIDv4 binary identifier of the emitting client. |
| `sender_name_len` | `u8` | 1 | Byte length $N_{name} \in [1, 64]$ of the sender handle. |
| `sender_name` | `[u8; N]` | $N_{name}$ | UTF-8 encoded display name. |
| `payload_len` | `u32` | 4 | Byte length $N_{payload}$ of the record body. |
| `payload` | `[u8; M]` | $N_{payload}$ | Record payload content (text or binary structure). |
| `crc32` | `u32` | 4 | IEEE 802.3 CRC32 checksum computed over all bytes from `record_type` up to the last byte of `payload`. |

#### 2.2.3 Record Types Specification

```rust
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRecordType {
    Message      = 0x01, // Standard user text chat message
    Reaction     = 0x02, // Quick emoji reaction tied to a message ID or media PTS
    Bookmark     = 0x03, // User-placed timeline marker / cue point
    PollCreate   = 0x04, // Interactive poll initiation
    PollVote     = 0x05, // Poll vote submission
    ClipAnchor   = 0x06, // Media clip interval reference [start_pts, end_pts]
    SystemNotice = 0x07, // Peer join, leave, seek announcement, or sync alert
}
```

- **Message (`0x01`) Payload**: Plain UTF-8 encoded text string.
- **Reaction (`0x02`) Payload**:
  - `target_record_offset`: `u64` (offset of reacted message, or 0 if anchored purely to media PTS).
  - `emoji_code`: `[u8; 4]` (UTF-8 bytes of emoji, e.g. `b"🔥\0\0"` or `b"❤️\0"`).
- **Bookmark (`0x03`) Payload**:
  - `label_len`: `u16` (LE).
  - `label_bytes`: UTF-8 string describing the bookmark (e.g. `"Epic guitar solo"`).
- **PollCreate (`0x04`) Payload**:
  - `poll_id`: `[u8; 16]` (UUID).
  - `duration_seconds`: `u32`.
  - `question_len`: `u16`, followed by `question` (UTF-8).
  - `option_count`: `u8` ($2 \le count \le 10$).
  - For each option: `option_len: u16`, `option_text: UTF-8`.
- **PollVote (`0x05`) Payload**:
  - `poll_id`: `[u8; 16]`.
  - `selected_option_index`: `u8`.
- **ClipAnchor (`0x06`) Payload**:
  - `start_pts_ms`: `u64` (LE).
  - `end_pts_ms`: `u64` (LE).
  - `title_len`: `u16`, followed by `title` (UTF-8).
- **SystemNotice (`0x07`) Payload**:
  - `notice_code`: `u16` (LE) (e.g. `0x0001` = PeerJoin, `0x0002` = PeerLeave, `0x0003` = PlaybackSeek, `0x0004` = DriftWarning).
  - `message`: UTF-8 text string.

#### 2.2.4 Recovery, Resynchronization & Torn-Write Handling

A critical requirement for local persistence is fault tolerance: system reboots, crashes, or abrupt power loss must never corrupt previously written chat logs.

```
+---------------------------------------------------------------------------------------------------+
|                            BINARY LOG RECOVERY & FORWARD SCANNER                                  |
+---------------------------------------------------------------------------------------------------+
|  [File Header (16B)] -> Verify Magic "SPCH" & Header CRC32                                        |
|         |                                                                                         |
|         v                                                                                         |
|  [Read 2B Sync Word] == [0xAA, 0x55]?                                                             |
|         |-- NO  --> Scan byte-by-byte for next [0xAA, 0x55] (Resynchronization Mode)              |
|         |-- YES -> Read record_total_len                                                          |
|                      |                                                                            |
|                      +-- If offset + total_len > file_len:                                        |
|                      |     TORN WRITE AT EOF DETECTED!                                            |
|                      |     Action: Truncate file at current offset; salvage all prior records.    |
|                      |                                                                            |
|                      +-- If offset + total_len <= file_len:                                       |
|                            Read record payload; Compute IEEE 802.3 CRC32;                         |
|                            Compare with stored trailing CRC32:                                    |
|                            |-- MATCH: Commit to In-Memory Index; Advance offset by total_len.     |
|                            +-- MISMATCH: Record Corrupted! Advance 1 byte, resume resync scan.    |
+---------------------------------------------------------------------------------------------------+
```

- **Resynchronization State Machine**:
  ```rust
  pub fn recover_and_index<R: Read + Seek>(
      reader: &mut R,
      index: &mut InvertedIndex,
  ) -> Result<RecoveryStats, StorageError> {
      let mut stats = RecoveryStats::default();
      let file_len = reader.seek(SeekFrom::End(0))?;
      reader.seek(SeekFrom::Start(0))?;
      
      // Step 1: Validate 16-byte header
      let mut header_buf = [0u8; 16];
      reader.read_exact(&mut header_buf)?;
      validate_header(&header_buf)?;
      
      let mut current_offset = 16u64;
      let mut buf = Vec::with_capacity(65536);
      
      while current_offset < file_len {
          reader.seek(SeekFrom::Start(current_offset))?;
          let mut sync_bytes = [0u8; 2];
          if reader.read_exact(&mut sync_bytes).is_err() {
              break; // EOF reached
          }
          
          if sync_bytes != [0xAA, 0x55] {
              stats.corrupted_bytes += 1;
              current_offset += 1;
              continue;
          }
          
          let mut len_bytes = [0u8; 4];
          if reader.read_exact(&mut len_bytes).is_err() {
              // Torn write right at length header
              stats.torn_writes += 1;
              break;
          }
          let total_len = u32::from_le_bytes(len_bytes) as u64;
          if total_len < 48 || current_offset + total_len > file_len {
              // Torn write: record extends past EOF or invalid length
              stats.torn_writes += 1;
              break;
          }
          
          // Read record body and CRC
          buf.resize((total_len - 6) as usize, 0);
          reader.read_exact(&mut buf)?;
          
          let expected_crc = u32::from_le_bytes(buf[buf.len() - 4..].try_into().unwrap());
          let calculated_crc = crc32_ieee(&buf[..buf.len() - 4]);
          
          if expected_crc == calculated_crc {
              let record = parse_record_body(current_offset, &buf[..buf.len() - 4])?;
              index.insert_record(&record, current_offset);
              stats.valid_records += 1;
              current_offset += total_len;
          } else {
              // CRC mismatch: corrupt payload! Advance by 1 byte to seek next 0xAA55
              stats.corrupted_records += 1;
              current_offset += 1;
          }
      }
      
      Ok(stats)
  }
  ```

---

### 2.3 Inverted Index & Full-Text Search (FTS)

To provide sub-millisecond full-text search and temporal timeline querying without external search libraries (e.g. `tantivy`), `syncplay-chat` incorporates a memory-efficient inverted index.

#### 2.3.1 Tokenization Pipeline

The tokenizer converts unstructured message strings into normalized search terms:
1. **Unicode Case Folding**: Converts text to lowercase using Unicode standard mapping (`char::to_lowercase()`).
2. **Punctuation & Delimiter Stripping**: Strips ASCII and Unicode punctuation (`!`, `?`, `.`, `,`, `:`, `;`, `"`, `'`, `(`, `)`, `[`, `]`, `{`, `}`), while preserving inner `#` (hashtags) and `@` (mentions).
3. **Length Bounding**: Discards tokens shorter than 2 characters or longer than 48 characters to eliminate trivial noise and protect against adversarial buffer bloat.
4. **Stop-Word Removal**: Strips high-frequency English stop-words from index insertion and query parsing:
   `["the", "is", "at", "which", "on", "and", "a", "an", "to", "for", "in", "of", "it", "this", "that", "with", "as", "by"]`.

```
Input: "Alice: Check out that awesome guitar solo at 14:30! #rock"
Token Stream: ["alice", "check", "awesome", "guitar", "solo", "14", "30", "#rock"]
```

#### 2.3.2 Posting List & Spatial Index Data Structures

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostingEntry {
    pub record_offset: u64,       // Byte offset in syncplay_chat.bin
    pub media_timestamp_ms: u64,  // Playback PTS for rapid timeline filtering
    pub term_frequency: u16,      // Occurrences of term within this message
}

pub struct InvertedIndex {
    // Term -> Vector of postings sorted strictly by record_offset ascending
    postings: BTreeMap<String, Vec<PostingEntry>>,
    
    // Timeline Spatial Index: Media Timestamp (ms) -> Vector of record offsets
    timeline_index: BTreeMap<u64, Vec<u64>>,
    
    // Metadata lookup: Record offset -> Compact Record Metadata
    record_table: BTreeMap<u64, RecordHeaderMeta>,
    
    // Total indexed document count
    total_docs: usize,
}

#[derive(Debug, Clone)]
pub struct RecordHeaderMeta {
    pub wall_timestamp_utc_ms: u64,
    pub media_timestamp_ms: u64,
    pub sender_id: [u8; 16],
    pub record_type: ChatRecordType,
    pub flags: u8,
    pub payload_offset: u64,
    pub payload_len: u32,
}
```

#### 2.3.3 Query Parser & Intersection Engine

The query engine supports three search modes:
1. **Single-Term & Prefix Search**:
   - Exact term: $O(\log |\text{Vocabulary}|)$ lookup in `postings`.
   - Prefix term (e.g. `"guit*"`): Uses `BTreeMap::range("guit"..)` until keys no longer match prefix, returning the union of posting lists.
2. **Multi-Term Boolean AND Intersection**:
   - Given terms $Q = \{t_1, t_2, \dots, t_k\}$, retrieve posting lists $L_1, L_2, \dots, L_k$.
   - Sort lists by length: $|L_1| \le |L_2| \le \dots \le |L_k|$.
   - Execute two-pointer linear merge between $L_1$ and $L_2$, then intersect result with subsequent lists.
   - Computational complexity: $O(\sum |L_i|)$, typically completed in $< 100\mu\text{s}$ for room chat corpora.
3. **Scoring & Ranking (BM25 Lightweight)**:
   Matching documents are ranked using a simplified Okapi BM25 formula:
   $$\text{Score}(D, Q) = \sum_{t \in Q} \text{IDF}(t) \cdot \frac{f(t, D) \cdot (k_1 + 1)}{f(t, D) + k_1 \cdot \left(1 - b + b \cdot \frac{|D|}{\text{avgdl}}\right)}$$
   where $k_1 = 1.2$, $b = 0.75$, and $\text{IDF}(t) = \ln\left(1 + \frac{N - n(t) + 0.5}{n(t) + 0.5}\right)$.

#### 2.3.4 Timeline Range Queries (On-Screen Overlay / Danmaku)

When the media player is rendering frame at presentation time $T_{media}$, the UI subsystem queries for active chat messages to display as on-screen text overlays or subtitle popups.

- **Query Interval**: $[T_{media} - W_{past}, T_{media} + W_{future}]$ (e.g. $W_{past} = 4000\text{ms}$, $W_{future} = 1000\text{ms}$).
- **Algorithm**:
  ```rust
  pub fn query_timeline_window(
      &self,
      current_media_pts: u64,
      window_past_ms: u64,
      window_future_ms: u64,
  ) -> Vec<&RecordHeaderMeta> {
      let start_pts = current_media_pts.saturating_sub(window_past_ms);
      let end_pts = current_media_pts.saturating_add(window_future_ms);
      
      let mut results = Vec::new();
      for (_pts, offsets) in self.timeline_index.range(start_pts..=end_pts) {
          for &offset in offsets {
              if let Some(meta) = self.record_table.get(&offset) {
                  if meta.flags & 0x01 == 0 { // Skip tombstones (deleted messages)
                      results.push(meta);
                  }
              }
          }
      }
      results
  }
  ```
  Retrieval is completed in $O(\log N + K)$ operations, guaranteeing zero frame-drop impact on the $60\text{ FPS}$ UI presentation thread.

#### 2.3.5 Room History Transcripts (JSON & Markdown)

The chat engine provides two deterministic export formats:
1. **JSON Export**: Machine-readable array of structured message objects with ISO-8601 wall times and formatted `HH:MM:SS.mmm` media timestamps.
2. **Markdown / Plaintext Transcript**: Human-readable chronological log suitable for sharing:
   ```markdown
   # Syncplay-Alt Room Chat Transcript
   - Exported: 2026-10-24 23:30:00 UTC
   - Total Messages: 142 | Media Duration: 01:45:12

   [00:04:15.200 | 2026-10-24 23:05:12 UTC] <Alice>: Movie starting!
   [00:14:31.008 | 2026-10-24 23:15:28 UTC] <Bob>: Incredible cinematography here.
   [00:14:35.500 | 2026-10-24 23:15:32 UTC] <Alice> reacted with 🔥
   [00:45:00.000 | 2026-10-24 23:46:00 UTC] [POLL] <Alice> created: "Intermission break?" (Yes: 3, No: 1)
   ```

---

### 2.4 Smart Slash-Commands & Media Triggers

In-player chat is not merely passive text; it acts as an interactive command console for room participants. Slash-commands are parsed client-side, validated, and translated into network synchronization events.

#### 2.4.1 Command Grammar & Lexer Specification

```ebnf
Command        ::= '/' CommandVerb ( ' ' ArgumentList )?
CommandVerb    ::= 'seek' | 'bookmark' | 'clip' | 'poll' | 'sync-status'

(* /seek <timestamp_or_hms> *)
SeekArgs       ::= TimeString | RelativeOffset
TimeString     ::= ( ( Hours ':' )? Minutes ':' )? Seconds ( '.' Millis )?
RelativeOffset ::= ( '+' | '-' ) Integer ( 's' | 'm' | 'h' )

(* /bookmark [optional_label] *)
BookmarkArgs   ::= [^\n\r]*

(* /clip <start_time> <end_time> [optional_label] *)
ClipArgs       ::= TimeString ' ' TimeString ( ' ' [^\n\r]* )?

(* /poll <"Question"> <"Opt1"> <"Opt2"> [ "Opt3" ... ] *)
PollArgs       ::= QuotedString ( ' ' QuotedString ){2,10}
QuotedString   ::= '"' ( [^"\\] | '\\"' )* '"'

(* /sync-status (no arguments) *)
```

#### 2.4.2 Timestamp Normalization Rules

The lexer normalizes all timestamp representations into absolute unsigned 64-bit integer milliseconds (`u64`):

| Input Format | Example | Calculation | Normalized `media_timestamp_ms` |
|---|---|---|---|
| Full HMS | `01:23:45.500` | $1\times 3600000 + 23\times 60000 + 45\times 1000 + 500$ | `5025500` |
| Minutes:Seconds | `14:30` | $14\times 60000 + 30\times 1000$ | `870000` |
| Raw Seconds | `95.25` | $95.25 \times 1000$ | `95250` |
| Relative Ahead | `+15s` | $\text{current\_media\_pts} + 15000$ | Saturating add |
| Relative Behind | `-10s` | $\text{current\_media\_pts} - 10000$ | Saturating sub |

#### 2.4.3 Parser Data Model & Dispatch Mapping

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedSlashCommand {
    Seek {
        target_pts_ms: u64,
    },
    Bookmark {
        pts_ms: u64,
        label: Option<String>,
    },
    Clip {
        start_pts_ms: u64,
        end_pts_ms: u64,
        label: Option<String>,
    },
    Poll {
        question: String,
        options: Vec<String>,
    },
    SyncStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandDispatchTarget {
    LocalOnly,                      // /sync-status diagnostic
    BroadcastSync(SyncWireMessage), // Distributed state mutation
}
```

#### 2.4.4 Media Trigger & Sync Event Flow

```
User Types in Chat: "/seek 00:15:00"
        |
        v
[ChatCommandParser::parse] -> ParsedSlashCommand::Seek { target_pts_ms: 900000 }
        |
        v
[CommandDispatcher]
        |-- 1. Creates Local Chat Record: Type = 0x07 (SystemNotice), Message = "Alice initiated seek to 00:15:00"
        |-- 2. Writes Record to syncplay_chat.bin
        |-- 3. Emits SyncMessage::InitiateSeek { target_pts: 900000, issuer: alice_uuid } to syncplay-sync
        v
[syncplay-sync Engine]
        |-- Triggers two-phase barrier unpause / seek coordinator across all peers
        v
[syncplay-ui]
        +-- Displays immediate visual OSD toast: "Seeking to 00:15:00..."
```

---

## 3. Multi-Crate Workspace Architecture & Dependency Rules

### 3.1 Workspace Topology & Cargo Configuration

The workspace root resides at `/home/gautham/syncplay-alt` and enforces a unified compilation environment with strict warnings, high optimization for bitstream math, and zero external media dependencies.

```
/home/gautham/syncplay-alt/
├── Cargo.toml                  <-- Root Workspace Manifest
├── Cargo.lock                  <-- Fully Resolved Lockfile
├── AGENTS.md                   <-- Pi Framework Architecture Invariants
├── .pi/                        <-- Pi Framework Skills, Prompts, Workflows
├── openspec/                   <-- OpenSpec Specifications & Roadmaps
├── scripts/
│   ├── verify_phase1.sh        <-- Self-Contained Automated Verification Harness
│   └── audit_dependencies.sh   <-- Zero-Dependency AST/Grep Checker
└── crates/
    ├── syncplay-common/        <-- Types, Timestamps, CRC32, Primitives
    ├── syncplay-demux/         <-- MKV/EBML & MP4/ISOBMFF Zero-Dep Demuxers
    ├── syncplay-codecs/        <-- Codec Interfaces, Frame Types, YUV->RGB Math
    ├── syncplay-clock/         <-- Monotonic Clock, SPSC Lock-Free Ring Buffer
    ├── syncplay-sync/          <-- NTP Filter, Discrete PLL, Barrier Unpause
    ├── syncplay-chat/          <-- Binary Log Engine, Inverted Index FTS, Slash Cmds
    ├── syncplay-ui/            <-- UI State Model, OSD Layout Primitives
    └── syncplay-core/          <-- Top-Level Coordinator & Application Engine
```

#### 3.1.1 Root `Cargo.toml` Specification

```toml
[workspace]
resolver = "2"
members = [
    "crates/syncplay-common",
    "crates/syncplay-demux",
    "crates/syncplay-codecs",
    "crates/syncplay-clock",
    "crates/syncplay-sync",
    "crates/syncplay-chat",
    "crates/syncplay-ui",
    "crates/syncplay-core",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
authors = ["Gautham <developer@syncplay-alt.local>"]
license = "MIT OR Apache-2.0"
repository = "https://github.com/gautham/syncplay-alt"

[workspace.dependencies]
# Permitted zero-dependency utility crates:
thiserror = "1.0"

[profile.dev]
opt-level = 1
overflow-checks = true

[profile.test]
opt-level = 2
overflow-checks = true

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
panic = "abort"
```

---

### 3.2 Crate-by-Crate Specifications & Inter-Crate Trait Boundaries

```
                         +-------------------+
                         |   syncplay-core   |
                         +---------+---------+
                                   |
                +------------------+------------------+
                |                  |                  |
                v                  v                  v
        +---------------+  +---------------+  +---------------+
        |  syncplay-ui  |  | syncplay-sync |  | syncplay-chat |
        +-------+-------+  +-------+-------+  +-------+-------+
                |                  |                  |
                +--------+---------+------------------+
                         |
                         v
                +-------------------+
                |  syncplay-clock   |
                +---------+---------+
                         |
                +--------+--------+
                |                 |
                v                 v
        +---------------+ +---------------+
        |syncplay-demux | |syncplay-codecs|
        +-------+-------+ +-------+-------+
                |                 |
                +--------+--------+
                         |
                         v
                +-------------------+
                |  syncplay-common  |
                +-------------------+
```

#### 1. `crates/syncplay-common`
- **Role**: Foundational types, mathematical abstractions, bitwise utilities, and CRC32 checksums.
- **Key Modules**:
  - `time`: `MediaTime` (newtype over milliseconds), `WallTime` (UTC epoch ms), `Rational { num: u32, den: u32 }` (timebase conversions).
  - `crc32`: Zero-dependency, table-driven IEEE 802.3 CRC32 calculation (`0xEDB88320` polynomial) executing at $> 1\text{ GB/s}$.
  - `io`: `ByteReader`, `BitReader` (MSB/LSB variable bit extraction for bitstream parsing).
  - `error`: Unified error hierarchies using `thiserror`.

#### 2. `crates/syncplay-demux`
- **Role**: Zero-dependency container demuxing from raw byte streams.
- **Key Traits**:
  ```rust
  pub trait Demuxer: Send {
      fn probe(reader: &mut (dyn SeekableByteStream + Send)) -> DemuxResult<bool> where Self: Sized;
      fn parse_headers(&mut self, reader: &mut (dyn SeekableByteStream + Send)) -> DemuxResult<ContainerInfo>;
      fn next_packet(&mut self, reader: &mut (dyn SeekableByteStream + Send)) -> DemuxResult<Option<DemuxPacket>>;
      fn seek_to(&mut self, reader: &mut (dyn SeekableByteStream + Send), target: MediaTime) -> DemuxResult<MediaTime>;
  }
  ```
- **Implementations**:
  - `EbmlDemuxer`: EBML variable-length integer (VINT) parser, Element ID mapping, Matroska Segment, Info, Track, and SimpleBlock/BlockGroup extraction.
  - `IsoBmffDemuxer`: MP4 box tree parser (`ftyp`, `moov`, `mvhd`, `trak`, `mdia`, `minf`, `stbl`, `stsd`, `stts`, `stsc`, `stsz`, `stco`/`co64`, `mdat`).

#### 3. `crates/syncplay-codecs`
- **Role**: Pixel format representations, baseline frame decoding abstractions, and color space transformations.
- **Frame Structures**:
  ```rust
  pub struct VideoFrame {
      pub width: u32,
      pub height: u32,
      pub format: PixelFormat, // Yuv420p, Rgb24, Rgba32
      pub pts: MediaTime,
      pub planes: [Vec<u8>; 4],
      pub strides: [usize; 4],
  }
  ```
- **Color Space Math**: Fixed-point integer ITU-R BT.601 / BT.709 YUV420p-to-RGB conversion:
  Using 14-bit integer fixed-point coefficients ($16384$ multiplier) with branchless clamp to $[0, 255]$:
  $$C = Y - 16, \quad D = U - 128, \quad E = V - 128$$
  $$R = \text{clamp}\big((298 \times C + 409 \times E + 128) \gg 8\big)$$
  $$G = \text{clamp}\big((298 \times C - 100 \times D - 208 \times E + 128) \gg 8\big)$$
  $$B = \text{clamp}\big((298 \times C + 516 \times D + 128) \gg 8\big)$$

#### 4. `crates/syncplay-clock`
- **Role**: Monotonic time generation, PTS drift computation, and lock-free thread synchronization.
- **SPSC Lock-Free Ring Buffer**:
  ```rust
  pub struct SpscRingBuffer<T, const CAPACITY: usize> {
      buffer: [UnsafeCell<MaybeUninit<T>>; CAPACITY],
      head: CachePadded<AtomicUsize>, // Written by Producer
      tail: CachePadded<AtomicUsize>, // Written by Consumer
  }
  ```
  Uses `Acquire`/`Release` memory ordering semantics to exchange decoded `VideoFrame` and `AudioFrame` buffers between demux/decode threads and the rendering loop with zero lock contention or mutex priority inversion.

#### 5. `crates/syncplay-sync`
- **Role**: Long-distance high-RTT network synchronization engine.
- **Core Components**:
  - **NTP RFC 5905 Clock Filter**: 8-sample sliding window computing clock offset $\theta = \frac{(T_2 - T_1) + (T_3 - T_4)}{2}$ and round-trip delay $\delta = (T_4 - T_1) - (T_3 - T_2)$. Selects minimum-delay sample and applies statistical outlier dispersion rejection.
  - **Discrete Phase-Locked Loop (PLL) Speed Controller**: Proportional-Integral controller dynamically modulating media playback velocity $v(t) \in [0.96, 1.04]$:
    $$v(t) = 1.0 + K_p \cdot e(t) + K_i \int_0^t e(\tau) d\tau$$
    Eliminates drift without audio pitch distortion or stutter. Issues hard seeks only when $|e(t)| > 1500\text{ms}$.
  - **Two-Phase Barrier Unpause State Machine**: `ReadyRequest` $\to$ `ReadyAck` consensus $\to$ atomic `UnpauseCommit` scheduled at future wall time $T_{unpause} = T_{now} + \Delta t_{margin}$, synchronizing video startup across peers to $< 10\text{ms}$.

#### 6. `crates/syncplay-chat`
- **Role**: The local chat storage, FTS search, and slash-command engine specified in Section 2.
- **Dependencies**: `syncplay-common` (for `MediaTime`, `CRC32`, `ByteReader/Writer`).

#### 7. `crates/syncplay-ui`
- **Role**: Native UI state model and OSD overlay layout primitives.
- **Features**: VLC-grade responsiveness, zero allocation in hot render loops, triple-buffered snapshot passing between core engine and presentation surface.

#### 8. `crates/syncplay-core`
- **Role**: Top-level coordinator binding demuxer, codecs, monotonic clock, network sync, chat, and UI into a coherent application.

---

### 3.3 Strict Zero-Dependency Audit & Compliance

To guarantee architectural integrity, the project enforces a strict blacklist of third-party libraries. Any introduction of these crates in `Cargo.toml` or `Cargo.lock` triggers an immediate build failure:

#### Blacklisted Crates & Libraries

| Category | Banned Crates / Bindings |
|---|---|
| **Media & Containers** | `ffmpeg`, `libav*`, `ffmpeg-sys*`, `gstreamer*`, `mpv`, `libmpv*`, `vlc`, `symphonia*`, `matroska`, `mp4`, `nom-ebml` |
| **Video & Audio Codecs** | `dav1d*`, `rav1e`, `openh264*`, `minimp3*`, `hound`, `image` |
| **Database Engines** | `sqlite`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `heed` |
| **Search Engines** | `tantivy`, `meilisearch` |

#### Audit Script Specification (`scripts/audit_dependencies.sh`)

```bash
#!/usr/bin/env bash
set -euo pipefail

BANNED_PATTERNS=(
    "ffmpeg" "libav" "gstreamer" "mpv" "vlc"
    "symphonia" "dav1d" "sqlite" "rusqlite" "sqlx"
    "tantivy" "rocksdb" "sled"
)

echo "[AUDIT] Scanning workspace Cargo manifests for banned dependencies..."
for manifest in Cargo.toml crates/*/Cargo.toml; do
    if [ -f "$manifest" ]; then
        for pattern in "${BANNED_PATTERNS[@]}"; do
            if grep -inE "(^|[ =\"'_])${pattern}" "$manifest" >/dev/null; then
                echo "[-] VIOLATION: Prohibited dependency pattern '${pattern}' found in ${manifest}!"
                exit 1
            fi
        done
    fi
done

if [ -f "Cargo.lock" ]; then
    echo "[AUDIT] Scanning Cargo.lock for banned package resolutions..."
    for pattern in "${BANNED_PATTERNS[@]}"; do
        if grep -inE "name = \"${pattern}\"" Cargo.lock >/dev/null; then
            echo "[-] VIOLATION: Banned crate '${pattern}' resolved in Cargo.lock!"
            exit 1
        fi
    done
fi

echo "[+] AUDIT PASSED: Zero prohibited media or database dependencies found."
exit 0
```

---

## 4. Pi Coding-Agent Framework (`.pi/` & `AGENTS.md`)

The Pi framework facilitates autonomous, spec-driven development where coding agents iteratively implement, verify, and benchmark features against the OpenSpec specification suite.

### 4.1 Root `AGENTS.md` Specification

The root `AGENTS.md` acts as the constitution for all agent runs. It defines:
1. **Architectural Invariants**: Pure Rust, zero external media/codec/database engines, modular crate separation.
2. **Coding Standards**:
   - `#![deny(unsafe_code)]` enforced by default across all crates (exceptions isolated to SIMD color converters and SPSC atomic ring buffers, strictly marked with `// SAFETY:` justifications).
   - Zero `unwrap()` or `expect()` in production library code; all errors mapped to typed `enum` variants with `thiserror`.
   - Comprehensive Given/When/Then unit tests accompanying every public function.
3. **Verification Command Reference**:
   - Fast syntax/type check: `cargo check --workspace`
   - Test suite execution: `cargo test --workspace`
   - Verification harness: `./scripts/verify_phase1.sh`
   - Dependency audit: `./scripts/audit_dependencies.sh`

### 4.2 `.pi/` Framework Directory Layout

```
.pi/
├── agents/
│   ├── architect.md             <-- System design, traits, workspace topology
│   ├── demuxer-specialist.md     <-- EBML/MKV & ISOBMFF bitstream parsing
│   ├── codec-specialist.md       <-- Baseline bitstreams, YUV->RGB SIMD math
│   ├── sync-specialist.md        <-- NTP filter, discrete PLL, barrier unpause
│   ├── chat-specialist.md        <-- Custom binary log, inverted index, slash cmds
│   └── verifier.md               <-- Test runner, fuzzing, compliance auditing
├── skills/
│   ├── run-phase1-validation.md  <-- Instructions for running scripts/verify_phase1.sh
│   ├── check-dependencies.md     <-- Automated zero-dependency verification
│   ├── simulate-network-sync.md  <-- High-RTT jitter simulation runner
│   ├── parse-binary-log.md       <-- Chat binary format inspector & validator
│   └── validate-ebml-vint.md     <-- Synthetic EBML VINT test vector runner
├── workflows/
│   ├── spec-to-code.md           <-- Spec reading -> Test drafting -> Implementation
│   ├── sync-simulation-tune.md   <-- Tuning Kp/Ki gains under high-jitter conditions
│   └── zero-dep-audit.md         <-- Auditing crates and Cargo.lock
└── prompts/
    ├── system-invariant.md       <-- System prompt injection enforcing zero-dep rules
    └── review-gate.md            <-- Multi-gate verification & forensic review checklist
```

---

## 5. Self-Contained Verification Harness (`scripts/verify_phase1.sh`)

### 5.1 Verification Script Architecture

`scripts/verify_phase1.sh` provides an automated end-to-end acceptance gate. It is divided into 7 distinct stages, terminating with exit code `0` on total success or a non-zero code with targeted diagnostic logging on any failure.

```
+---------------------------------------------------------------------------------------------------+
|                            PHASE 1 VERIFICATION HARNESS PIPELINE                                  |
+---------------------------------------------------------------------------------------------------+
|  [Stage 1: Toolchain & Environment Verification]                                                  |
|         |-- Checks bash, standard coreutils, and Rust environment.                                |
|         v                                                                                         |
|  [Stage 2: OpenSpec Completeness Audit]                                                           |
|         |-- Validates openspec/project.md, 6 capability specs, RFC-2119 keywords, Given/When/Then.|
|         v                                                                                         |
|  [Stage 3: Pi Framework Completeness Audit]                                                       |
|         |-- Verifies AGENTS.md, .pi/agents/, .pi/skills/, .pi/workflows/.                         |
|         v                                                                                         |
|  [Stage 4: Zero-Dependency Compliance Check]                                                      |
|         |-- Executes audit_dependencies.sh; ensures zero blacklisted crates.                     |
|         v                                                                                         |
|  [Stage 5: Multi-Crate Workspace Compilation & Unit Tests]                                        |
|         |-- cargo check --workspace & cargo test --workspace (zero warnings).                    |
|         v                                                                                         |
|  [Stage 6: Synthetic Binary Test Vector Parsing]                                                  |
|         |-- Exercises EBML VINT parser, MP4 box headers, and chat binary log recovery.            |
|         v                                                                                         |
|  [Stage 7: Deterministic Network Sync Simulation]                                                 |
|         |-- Simulates 250-400ms RTT link with +-30ms jitter.                                      |
|         |-- Verifies NTP filter convergence.                                                      |
|         |-- Verifies PLL phase error converges to < 15ms.                                         |
|         |-- Verifies barrier unpause aligns playback start within < 10ms.                         |
|         v                                                                                         |
|  [STAGE 8: 100% VERIFICATION PASSED -> EXIT 0]                                                    |
+---------------------------------------------------------------------------------------------------+
```

### 5.2 Deterministic Network Sync Simulation Math

Stage 7 implements a mathematical proof test simulating the distributed playback of two peers ($Peer_A$ and $Peer_B$) connected across an asymmetric, jittery WAN link:
- **Link Characteristics**:
  - Base RTT: $R_0 = 300\text{ms}$ (Forward delay $D_{AB} \in [120, 180]\text{ms}$, Return delay $D_{BA} \in [130, 220]\text{ms}$).
  - Jitter: Gaussian or bounded uniform noise $J \sim \mathcal{U}(-30\text{ms}, +30\text{ms})$.
  - Packet loss: $2\%$ random drop with retransmission.
- **NTP Filter Validation**:
  - Over a sliding window of 8 exchanges, the minimum-delay sample yields clock offset error $|\hat{\theta} - \theta_{true}| < 5\text{ms}$.
- **PLL Speed Modulation Validation**:
  - Peer initial phase offset $e(0) = 450\text{ms}$ (behind master).
  - The discrete PI controller modulates rate $v(t) = 1.0 + K_p \cdot e(t) + K_i \int e(t) dt$ (clamped to $[0.96, 1.04]$).
  - Over a 30-second simulation window ($1800$ frames at $60\text{ FPS}$), the phase error $|e(t)|$ converges monotonically to $< 15\text{ms}$ without issuing a hard seek.
- **Barrier Unpause Validation**:
  - When unpause is triggered, the host calculates target unpause wall timestamp $T_{target} = T_{now} + \max(RTT) + 50\text{ms}$.
  - Both clients unpause at their respective local wall times mapping to $T_{target}$.
  - Simulated presentation skew at start of playback: $\Delta T_{start} < 10\text{ms}$.

---

## 6. Implementation Roadmap & Phase-by-Phase Integration Plan

To guide subsequent implementation workers, the roadmap is broken down into ordered, test-verified milestones:

```
+---------------------------------------------------------------------------------------------------+
|                                   PHASE 1 IMPLEMENTATION ROADMAP                                  |
+---------------------------------------------------------------------------------------------------+
|  Milestone 1 (M1): OpenSpec Specification Hierarchy & Capability Specs                            |
|    - openspec/project.md, master roadmap                                                          |
|    - 6 Capability specs: demuxer, codecs, av-pipeline, sync-engine, smart-chat, ui-renderer       |
|    - openspec/changes/ dependency graph                                                           |
|                                                                                                   |
|  Milestone 2 (M2): Pi Coding-Agent Framework Setup                                                |
|    - AGENTS.md constitution & invariants                                                          |
|    - .pi/agents/, .pi/skills/, .pi/workflows/, .pi/prompts/                                       |
|                                                                                                   |
|  Milestone 3 (M3): Multi-Crate Workspace Scaffold & Foundational Primitives                       |
|    - Cargo.toml workspace definition & 8 crates                                                   |
|    - syncplay-common (CRC32, media timebase, error types)                                         |
|    - syncplay-demux (EBML VINT parser, MP4 box header parser, synthetic test vectors)             |
|    - syncplay-codecs (Frame definitions, YUV420p->RGB fixed-point conversion)                     |
|    - syncplay-clock (Monotonic clock, SPSC lock-free ring buffer)                                 |
|    - syncplay-sync (NTP clock filter, discrete PLL controller, barrier unpause state machine)     |
|    - syncplay-chat (Binary log engine, inverted index FTS, slash commands, exporters)            |
|    - syncplay-ui (UI state model & OSD layout primitives)                                         |
|    - syncplay-core (Coordinator engine)                                                           |
|                                                                                                   |
|  Milestone 4 (M4): Verification Harness & Deterministic Simulation Suite                         |
|    - scripts/audit_dependencies.sh                                                                |
|    - scripts/verify_phase1.sh                                                                     |
|    - High-RTT network sync simulation test (250-400ms RTT, +-30ms jitter, PLL < 15ms)             |
+---------------------------------------------------------------------------------------------------+
```

---

## 7. Synthesis & Architectural Alignment

| Subsystem | Explorer 1 Spec | Explorer 2 Spec | Explorer 3 Spec (This Analysis) | Consensus State |
|---|---|---|---|---|
| **Container Demuxing** | EBML RFC 8794 & ISOBMFF Box Parser | N/A | `syncplay-demux` crate with synthetic test vectors | Unified |
| **Codecs & Color Space** | Planar YUV420p & Fixed-Point BT.601/709 | N/A | `syncplay-codecs` zero-dep SIMD/fixed-point math | Unified |
| **A/V Clock & Buffering** | Master Monotonic Clock & SPSC Ring Buffer | N/A | `syncplay-clock` lock-free circular queue | Unified |
| **Network Sync & PLL** | N/A | NTP RFC 5905, Discrete PI PLL, Barrier Unpause | `syncplay-sync` crate, simulation harness math | Unified |
| **UI & OSD Overlay** | N/A | VLC-grade responsive layout, triple buffer | `syncplay-ui` crate, chat overlay integration | Unified |
| **Smart In-Player Chat** | N/A | N/A | Custom binary log (`SPCH`), FTS index, slash commands | New Specification |
| **Workspace Topology** | Crate structure | Crate structure | 8-crate workspace scaffold, zero-dep audit | Unified |
| **Pi Framework & Verification** | N/A | N/A | `AGENTS.md`, `.pi/`, `scripts/verify_phase1.sh` | New Specification |

---

*End of Technical Analysis.*
