# Remediation Strategy Report: Smart Chat & UI Renderer Specifications

**Author**: `explorer_m1_fix_3` (teamwork_preview_explorer)  
**Roles**: investigator, synthesizer, specialist  
**Working Directory**: `/home/gautham/syncplay-alt/.agents/teamwork/explorer_m1_fix_3/`  
**Target Specifications**:
- `openspec/specs/smart-chat/spec.md`
- `openspec/specs/ui-renderer/spec.md`  
**Associated Change Proposals**:
- `openspec/changes/005-smart-chat-storage.md`
- `openspec/changes/006-ui-osd-renderer.md`  
**Reference Evidence**:
- Adversarial Challenge Report: `/home/gautham/syncplay-alt/.agents/teamwork/challenger_m1_2/challenge_report.md`
- Challenger Handoff: `/home/gautham/syncplay-alt/.agents/teamwork/challenger_m1_2/handoff.md`
- Test Harnesses: `tests/empirical_challenge/test_smart_chat.py`, `tests/empirical_challenge/test_ui_renderer.py`

---

## 1. Executive Summary & Problem Scope

Challenger 2 (`challenger_m1_2`) subjected the OpenSpec specifications for `syncplay-chat` and `syncplay-ui` to empirical adversarial stress-testing, identifying five concrete algorithmic and architectural defects across storage recovery, internationalized full-text search, multithreaded snapshot synchronization, debounced input kinetics, and GPU/RAM memory budgeting:

1. **Binary Log False-Sync Database Recovery (`smart-chat`)**: The crash recovery scanner prematurely aborted recovery and truncated the log whenever `current_offset + record_total_len > file_len`. In corrupted streams where non-header bytes contain the 2-byte marker `0xAA55` followed by large integers, the scanner mistook corrupted payload bytes for a torn write at physical EOF, destroying all subsequent valid records (losing 7 out of 9 valid records in empirical tests).
2. **Inverted Index FTS CJK Tokenization & Single Ideographs (`smart-chat`)**: Whitespace-delimited tokenization collapsed unspaced CJK (Chinese, Japanese) sentences into single monolithic tokens, causing substring/word searches to fail. Furthermore, the rule discarding tokens shorter than 2 characters completely purged single CJK ideographs (e.g., `猫`, `狗`), rendering single-character dictionary words unsearchable.
3. **UI Renderer Multi-Producer Race (`ui-renderer`)**: The specification mandated triple-buffered atomic snapshot sharing but omitted the thread topology. Because lock-free triple buffering is strictly a Single-Producer Single-Consumer (SPSC) protocol, multiple uncoordinated publisher threads (audio, video, network) writing concurrently caused data tearing and torn snapshot reads (615 torn reads out of 3,240 polls).
4. **UI Scrubber Debounce Scenario Contradiction (`ui-renderer`)**: The scenario specification asserted that a 500ms drag at 120 Hz with a 10 Hz rate limiter emits "at most 5 commands", while simultaneously mandating an immediate authoritative seek upon mouse release. Discrete simulation proves that 5 intermediate throttled seeks plus 1 release seek equals 6 commands, creating an impossible acceptance test criterion.
5. **Font Atlas Memory Bounding (`ui-renderer`)**: Storing a 2048x2048 font glyph atlas in 32-bit RGBA8 consumes 16 MB of RAM, instantly exceeding the entire 15 MB UI memory ceiling without accounting for framebuffers or state buffers.

This report formulates an authoritative, line-by-line remediation strategy for the implementation worker. Per team constraints, this explorer does not modify specification files directly; instead, it provides exact line references, verbatim diff blocks, and concrete implementation algorithms.

---

## 2. Remediation Strategy 1: Binary Log False-Sync Database Recovery

### 2.1 Defect Analysis & Root Cause
- **Location**: `openspec/specs/smart-chat/spec.md`, lines 75–86.
- **Flawed Text**:
  ```markdown
  77: 1. **Torn Write at EOF**: If `current_offset + record_total_len > file_len` or reading returns EOF mid-record, the scanner MUST treat the file as truncated at `current_offset`, preserve all preceding valid records, and salvage the database.
  ```
- **Root Cause**: The 2-byte sync sequence `[0xAA, 0x55]` has only 16 bits of entropy. In a corrupted payload or random binary data, `0xAA55` will naturally occur with probability $1/65536$. If this false sync word is followed by a corrupted 4-byte length field exceeding the remaining bytes in the file, Rule 1 evaluated to `TRUE`. The scanner falsely assumed it had reached a torn write at EOF and truncated the database at `current_offset`, discarding all subsequent healthy records.
- **Empirical Failure**: In `test_smart_chat.py`, introducing a corrupted chunk containing `\xAA\x55\xFF\xFF\xFF\x00` between records 1 and 3 caused records 3 through 9 to be permanently deleted (only 2 out of 9 valid records recovered).

### 2.2 Architectural Principle & Recovery Invariants
1. **False-Sync Disambiguation**: A candidate sync word whose `record_total_len < 48` or `current_offset + record_total_len > file_len` or whose CRC32 checksum fails MUST be treated as an unconfirmed candidate / corrupted byte, NOT an authoritative torn write at EOF.
2. **Byte-by-Byte Forward Scanning**: When encountering an invalid candidate or corrupt chunk mid-file, the scanner MUST advance by exactly 1 byte (`current_offset += 1`), increment `corrupted_chunks`, and resume searching for valid `0xAA55` frames.
3. **Exhaustive EOF Qualification**: A torn write at EOF can ONLY be declared when the scanner has reached the physical end of file (`offset >= file_len`) and unparsed trailing bytes remain after the last successfully validated record.

### 2.3 Exact Proposed Changes to `openspec/specs/smart-chat/spec.md`

#### Line-by-Line Target: Lines 75–86
**Current Content:**
```markdown
### Requirement: Crash Recovery & Torn-Write Resynchronization
The recovery scanner SHALL validate the file header and sequentially read records from offset 16 to EOF. If the parser encounters corrupted bytes or a torn write (incomplete record at EOF caused by sudden power loss or process kill):
1. **Torn Write at EOF**: If `current_offset + record_total_len > file_len` or reading returns EOF mid-record, the scanner MUST treat the file as truncated at `current_offset`, preserve all preceding valid records, and salvage the database.
2. **Mid-Stream Byte Corruption**: If the CRC32 checksum mismatches or the sync word `0xAA55` is absent, the scanner MUST scan forward byte-by-byte until the next `0xAA55` sequence is detected, discarding only the corrupted chunk.

#### Scenario: Recover Log With Incomplete Record at EOF
- **Given** a `syncplay_chat.bin` file containing 10 valid records followed by an incomplete record whose trailing 20 bytes were truncated during power loss
- **When** `recover_and_index` is invoked
- **Then** exactly 10 valid records are recovered and inserted into the inverted index
- **And** `RecoveryStats::torn_writes` equals 1
- **And** the parser terminates successfully without panicking.
```

**Proposed Replacement Content:**
```markdown
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
```

---

## 3. Remediation Strategy 2: Inverted Index FTS CJK Tokenization & Single Ideographs

### 3.1 Defect Analysis & Root Cause
- **Location**: `openspec/specs/smart-chat/spec.md`, lines 89–106.
- **Flawed Text**:
  ```markdown
  90: The chat subsystem SHALL maintain an in-memory inverted index for all valid text messages. The tokenization pipeline MUST:
  91: 1. Apply Unicode case folding to convert text to lowercase.
  92: 2. Strip punctuation marks while preserving hashtags (`#`) and user mentions (`@`).
  93: 3. Discard tokens shorter than 2 characters or longer than 48 characters.
  94: 4. Remove standard stop-words (`["the", "is", "at", "which", "on", "and", "a", "an", "to", "for", "in", "of", "it", "this", "that", "with", "as", "by"]`).
  ```
- **Root Cause**:
  1. East Asian languages (Chinese, Japanese) do not delimit words with spaces. A sentence like `"这是一段非常精彩的吉他独奏"` was split on whitespace into a single 13-character token `["这是一段非常精彩的吉他独奏"]`. A user query for `"吉他"` (guitar) failed to match because the posting list only indexed the entire compound string.
  2. In CJK, individual characters are logographic ideographs possessing complete lexical meaning (e.g., `猫` for cat, `狗` for dog, `车` for car). Rule 3 unconditionally discarded all tokens with length $< 2$ characters (`chars().count() < 2`), discarding every single CJK ideograph and preventing single-character searches.
- **Empirical Failure**: In `test_smart_chat.py`, `tokenize_spec("这是一段非常精彩的吉他独奏")` failed to match `"吉他"`, and `tokenize_spec("猫 狗")` returned `[]`.

### 3.2 Architectural Principle & Tokenization Pipeline
1. **Unicode CJK Block Identification**: Codepoints in the range `U+4E00` through `U+9FFF` (CJK Unified Ideographs) are classified as CJK logographs.
2. **CJK Unigram + Bigram Segmentation**:
   - For any CJK run, generate character unigrams (every individual ideograph $c_i$).
   - Generate overlapping character bigrams ($c_i c_{i+1}$).
   - This enables exact substring retrieval without requiring large external dictionary files, preserving the pure-Rust zero-dependency mandate.
3. **Selective Length Filtering**:
   - For Latin / alphanumeric tokens: enforce $2 \le \text{char\_count} \le 48$ to filter single-letter noise (`"a"`, `"s"`).
   - For CJK ideographs: exempt single ideographs from the $\ge 2$ length filter ($1 \le \text{char\_count} \le 48$).

### 3.3 Exact Proposed Changes to `openspec/specs/smart-chat/spec.md`

#### Line-by-Line Target: Lines 89–106
**Current Content:**
```markdown
### Requirement: In-Memory Inverted Index & Full-Text Search
The chat subsystem SHALL maintain an in-memory inverted index for all valid text messages. The tokenization pipeline MUST:
1. Apply Unicode case folding to convert text to lowercase.
2. Strip punctuation marks while preserving hashtags (`#`) and user mentions (`@`).
3. Discard tokens shorter than 2 characters or longer than 48 characters.
4. Remove standard stop-words (`["the", "is", "at", "which", "on", "and", "a", "an", "to", "for", "in", "of", "it", "this", "that", "with", "as", "by"]`).

The search engine SHALL execute multi-term Boolean AND queries across posting lists using linear two-pointer intersections in $O(\sum |L_i|)$ time.

#### Scenario: Search Chat Messages via Multi-Term Boolean AND
- **Given** an indexed chat store containing:
  - Message 1: `"Check out that awesome guitar solo at 14:30!"`
  - Message 2: `"The guitar tone is okay, but the bass is awesome."`
  - Message 3: `"Check out the drums at 20:00."`
- **When** searching for `"guitar awesome"`
- **Then** both Message 1 and Message 2 are returned
- **And** Message 3 is excluded because it lacks the term `"guitar"`.
```

**Proposed Replacement Content:**
```markdown
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
```

---

## 4. Remediation Strategy 3: UI Renderer Multi-Producer Race & SPSC Invariant

### 4.1 Defect Analysis & Root Cause
- **Location**: `openspec/specs/ui-renderer/spec.md`, lines 23–33.
- **Flawed Text**:
  ```markdown
  24: The UI event and presentation loop SHALL NOT acquire mutex locks shared with media decoding, audio output, or network synchronization threads. Player state MUST be communicated to the UI via an immutable, self-contained `PlayerUiSnapshot` published through a triple-buffered atomic exchange mechanism. 
  ```
- **Root Cause**: A triple-buffered atomic exchange algorithm consists of three discrete buffers: `read`, `clean`, and `write`/`dirty`. While this provides non-blocking, wait-free exchange between one writer and one reader, it is mathematically incapable of supporting multiple uncoordinated writers. When audio clock, video decoder, and network sync threads all attempt to write their partial metrics to `write_idx` concurrently, field writes interleave, and concurrent CAS exchanges on `clean_idx` cause race conditions.
- **Empirical Failure**: In `test_ui_renderer.py`, multi-producer publishing to an uncoordinated triple buffer resulted in **615 torn / inconsistent snapshot reads out of 3,240 polls** (19.0% torn read rate).

### 4.2 Architectural Principle & Thread Topology
1. **Single-Producer Single-Consumer (SPSC) Invariant**: The triple buffer MUST be constrained to exactly ONE publisher thread and exactly ONE reader thread (the UI render loop).
2. **Telemetry Aggregator Thread**: A centralized Telemetry Aggregator (running as part of the `syncplay-core` coordinator loop) collects metrics from the audio, video, clock, and network subsystems via lock-free atomic counters or non-blocking SPSC channels.
3. **Atomic Coalescing**: The Telemetry Aggregator builds a single, internally consistent `PlayerUiSnapshot` struct (72 bytes, fitting within a 128-byte cache-line pair) and performs the atomic swap with `clean_idx`. Subsystem threads NEVER publish directly to the triple buffer.

### 4.3 Exact Proposed Changes to `openspec/specs/ui-renderer/spec.md`

#### Line-by-Line Target: Lines 23–33 and 57–62
**Current Content (Lines 23–33):**
```markdown
### Requirement: Thread Decoupling & Mutex-Free State Sharing
The UI event and presentation loop SHALL NOT acquire mutex locks shared with media decoding, audio output, or network synchronization threads. Player state MUST be communicated to the UI via an immutable, self-contained `PlayerUiSnapshot` published through a triple-buffered atomic exchange mechanism. 

Polling the latest snapshot (`poll_latest_snapshot`) MUST complete in $O(1)$ time with zero blocking, locking, or system calls.

#### Scenario: Verify Zero Lock Contention During Frame Decoding
- **Given** an active video decoding thread processing a large video keyframe
- **When** the UI event loop polls for updated player state
- **Then** the UI thread retrieves the latest atomic snapshot immediately ($< 50\text{ ns}$)
- **And** zero thread blocking or priority inversion occurs.
```

**Current Content (Lines 57–62):**
```markdown
#### Scenario: Read Consistent Atomic Snapshot
- **Given** an engine updating the snapshot off-screen
- **When** the publisher swaps the active read index
- **Then** the UI reader receives an internally consistent snapshot where all telemetry fields reflect the exact same atomic tick.
```

**Proposed Replacement Content (Lines 23–33):**
```markdown
### Requirement: Thread Decoupling & Mutex-Free State Sharing
The UI event and presentation loop SHALL NOT acquire mutex locks shared with media decoding, audio output, or network synchronization threads. Player state MUST be communicated to the UI via an immutable, self-contained `PlayerUiSnapshot` published through a triple-buffered atomic exchange mechanism adhering to a strict **Single-Producer Single-Consumer (SPSC)** thread topology:

1. **Sole Publisher Mandate**: A single, dedicated **Telemetry Aggregator** thread (executing within the `syncplay-core` coordinator loop) SHALL be the sole publisher authorized to write to the triple buffer. Subsystem threads (demuxer, video decoder, audio sink, network sync) SHALL NOT publish directly to the triple buffer; they SHALL report metrics to the Telemetry Aggregator via atomic variables or bounded lock-free queues.
2. **Atomic Publication**: The Telemetry Aggregator compiles telemetry into the dirty buffer and executes a release-ordered atomic swap with the clean index at a periodic tick rate (e.g. 60–120 Hz).
3. **Wait-Free Polling**: Polling the latest snapshot (`poll_latest_snapshot`) by the UI consumer thread MUST complete in $O(1)$ time ($< 50\text{ ns}$) with zero blocking, locking, system calls, or read tearing.

#### Scenario: Verify Zero Lock Contention During Frame Decoding
- **Given** an active video decoding thread processing a large video keyframe
- **When** the UI event loop polls for updated player state
- **Then** the UI thread retrieves the latest atomic snapshot immediately ($< 50\text{ ns}$)
- **And** zero thread blocking or priority inversion occurs.
```

**Proposed Replacement Content (Lines 57–62):**
```markdown
#### Scenario: Read Consistent Atomic Snapshot Under SPSC Telemetry Aggregation
- **Given** audio, video, and network subsystems concurrently updating internal counters
- **When** the Telemetry Aggregator compiles and publishes the unified `PlayerUiSnapshot` to the SPSC triple buffer
- **Then** the UI reader polls and receives an internally consistent snapshot where all telemetry fields reflect the exact same atomic tick
- **And** zero torn reads or multi-producer race conditions occur.
```

---

## 5. Remediation Strategy 4: UI Scrubber Debounce Scenario Contradiction

### 5.1 Defect Analysis & Root Cause
- **Location**: `openspec/specs/ui-renderer/spec.md`, lines 99–111.
- **Flawed Text**:
  ```markdown
  105: #### Scenario: Rapid Mouse Drag on Progress Scrub-Bar
  106: - **Given** a user continuously dragging the scrub thumb across the timeline for $500\text{ ms}$
  107: - **When** mouse move events fire at $120\text{ Hz}$ (60 events)
  108: - **Then** the visual thumb updates 60 times without lag
  109: - **And** the number of seek commands sent to the media engine is at most 5 commands
  110: - **And** when the mouse button is released, the final seek command is emitted.
  ```
- **Root Cause & Mathematical Proof**:
  - Drag duration: $500\text{ ms}$ with mouse move events at $120\text{ Hz}$ ($\Delta t_{\text{event}} \approx 8.33\text{ ms}$).
  - Throttled seek rate limit: $10\text{ Hz}$ ($\Delta t_{\text{min}} = 100\text{ ms}$).
  - Intermediate seek timestamps emitted:
    - Command 1: $t = 0.0\text{ ms}$
    - Command 2: $t = 108.3\text{ ms}$ (first event $\ge 100\text{ ms}$)
    - Command 3: $t = 208.3\text{ ms}$ (first event $\ge 208.3\text{ ms}$)
    - Command 4: $t = 316.7\text{ ms}$ (first event $\ge 308.3\text{ ms}$)
    - Command 5: $t = 425.0\text{ ms}$ (first event $\ge 416.7\text{ ms}$)
  - Total intermediate seek commands during drag = 5 commands.
  - Requirement 3 states: *"Upon mouse release (scrub complete), a final authoritative seek command MUST be dispatched immediately."*
  - Mouse release at $t = 505\text{ ms}$ triggers Command 6 (final authoritative seek).
  - Total seek commands emitted = **6 commands** ($5 \text{ throttled} + 1 \text{ release}$).
  - The previous assertion *"the number of seek commands sent to the media engine is at most 5 commands"* directly contradicted the release requirement and caused automated test assertions to fail.

### 5.2 Exact Proposed Changes to `openspec/specs/ui-renderer/spec.md`

#### Line-by-Line Target: Lines 99–111
**Current Content:**
```markdown
### Requirement: Debounced Interactive Scrub-Bar
When the user scrubs the interactive timeline slider:
1. The visual scrubber thumb and timestamp tooltip MUST update at the full native display refresh rate ($60\text{--}144\text{ Hz}$).
2. Seek commands emitted to the demuxer and network sync engine MUST be rate-limited (debounced) to a maximum frequency of $10\text{ Hz}$ ($100\text{ ms}$ minimum interval between seek packets).
3. Upon mouse release (scrub complete), a final authoritative seek command MUST be dispatched immediately.

#### Scenario: Rapid Mouse Drag on Progress Scrub-Bar
- **Given** a user continuously dragging the scrub thumb across the timeline for $500\text{ ms}$
- **When** mouse move events fire at $120\text{ Hz}$ (60 events)
- **Then** the visual thumb updates 60 times without lag
- **And** the number of seek commands sent to the media engine is at most 5 commands
- **And** when the mouse button is released, the final seek command is emitted.
```

**Proposed Replacement Content:**
```markdown
### Requirement: Debounced Interactive Scrub-Bar
When the user scrubs the interactive timeline slider:
1. The visual scrubber thumb and timestamp tooltip MUST update at the full native display refresh rate ($60\text{--}144\text{ Hz}$).
2. Seek commands emitted to the demuxer and network sync engine during active dragging MUST be rate-limited (debounced) to a maximum frequency of $10\text{ Hz}$ ($100\text{ ms}$ minimum interval between intermediate seek packets).
3. Upon mouse release (scrub complete), a final authoritative seek command MUST be dispatched immediately to guarantee accurate landing on the user's release position.

#### Scenario: Rapid Mouse Drag on Progress Scrub-Bar
- **Given** a user continuously dragging the scrub thumb across the timeline for $500\text{ ms}$
- **When** mouse move events fire at $120\text{ Hz}$ (60 events)
- **Then** the visual thumb updates 60 times without lag
- **And** exactly 5 throttled intermediate seek commands are dispatched during the drag
- **And** when the mouse button is released, a 6th final authoritative seek command is dispatched immediately, for a total of 6 emitted seek commands.
```

---

## 6. Remediation Strategy 5: Font Atlas Memory Bounding

### 6.1 Defect Analysis & Root Cause
- **Location**: `openspec/specs/ui-renderer/spec.md`, lines 114–125.
- **Flawed Text**:
  ```markdown
  116: - **Total Memory Budget**: Maximum $15\text{ MB}$ RAM footprint (including font glyph atlas texture, OSD framebuffer, and snapshot buffers).
  ```
- **Root Cause & Memory Footprint Calculation**:
  - Full watch-party internationalization (CJK ideographs, emojis, European accents, monospace timecodes) requires a $2048 \times 2048$ glyph texture.
  - In 32-bit RGBA8 format (`R8G8B8A8_UNORM`, 4 bytes per texel):
    $$\text{Memory}_{\text{RGBA8}} = 2048 \times 2048 \times 4\text{ bytes} = 16\,777\,216\text{ bytes} = 16.0\text{ MB}$$
  - 16.0 MB exceeds the entire 15.0 MB RAM budget on the font atlas texture alone (106.7% of budget), leaving zero memory for OSD framebuffers, snapshot buffers, vertex buffers, or speculative state.
  - Font glyph rasterization only produces monochromatic coverage (alpha) values. Color is an extrinsic attribute supplied per-quad via vertex colors or shader constants. Allocating color channels in the texture atlas wastes 75% of texture memory.

### 6.2 Architectural Principle & Single-Channel Layout
1. **Single-Channel Alpha8 Texture (`R8_UNORM`)**:
   - Store font glyphs in an 8-bit single-channel texture format (`R8_UNORM`, 1 byte per texel).
   $$\text{Memory}_{\text{Alpha8}} = 2048 \times 2048 \times 1\text{ byte} = 4\,194\,304\text{ bytes} = 4.0\text{ MB}$$
   - For smaller screen resolutions or Latin-only sets, a $1024 \times 1024$ Alpha8 atlas requires only:
   $$\text{Memory}_{\text{Alpha8\_1K}} = 1024 \times 1024 \times 1\text{ byte} = 1.0\text{ MB}$$
2. **Shader Sampling**: The UI text fragment shader samples the `R` channel as alpha coverage ($\alpha = \text{texel.r}$) and modulates it with the quad vertex color (`color.rgb * alpha`), achieving identical visual quality at 25% of the memory footprint.
3. **UI Subsystem RAM Budget Breakdown**:
   | Component | Memory Allocation | Notes |
   |---|---|---|
   | Font Glyph Atlas (`R8_UNORM` 2048x2048) | 4.0 MB | Max glyph capacity (1.0 MB for 1024x1024) |
   | OSD HUD Framebuffer (Wayland `wl_shm` 1920x1080) | 8.3 MB | 32-bit RGBA double-buffered dirty-rect composite |
   | Vertex & Quad Ring Buffers | 1.0 MB | Pre-allocated static vertex arrays |
   | Snapshot Triple Buffers & SPSC Queues | < 0.2 MB | 3 x 72B snapshots + SPSC ring buffers |
   | Margin / Speculative UI State | 1.5 MB | Active chat bubble cache & timecode strings |
   | **Total UI Memory Footprint** | **~15.0 MB** | **Strictly conforms to < 15 MB ceiling** |

### 6.3 Exact Proposed Changes to `openspec/specs/ui-renderer/spec.md`

#### Line-by-Line Target: Lines 114–125
**Current Content:**
```markdown
### Requirement: Memory Footprint & Hot-Loop Zero-Allocation Budget
The entire `syncplay-ui` subsystem MUST operate within strict resource constraints:
- **Total Memory Budget**: Maximum $15\text{ MB}$ RAM footprint (including font glyph atlas texture, OSD framebuffer, and snapshot buffers).
- **CPU Overhead**: In steady-state playback with idle OSD, UI core utilization MUST remain $< 0.2\%$. Under active chat animations, CPU utilization MUST NOT exceed $1.2\%$.
- **Allocation Invariant**: Zero heap allocations (`malloc` / `Vec::new`) SHALL occur in the hot per-frame render loop; all vertex arrays and glyph quads MUST use pre-allocated static or ring buffers.

#### Scenario: Measure Steady-State Memory Allocation
- **Given** continuous 60 FPS playback for 10 minutes with active OSD compositing
- **When** monitoring process heap allocation
- **Then** zero continuous memory growth occurs
- **And** total UI memory allocation remains stable under $15\text{ MB}$.
```

**Proposed Replacement Content:**
```markdown
### Requirement: Memory Footprint & Hot-Loop Zero-Allocation Budget
The entire `syncplay-ui` subsystem MUST operate within strict resource constraints:
- **Total Memory Budget**: Maximum $15\text{ MB}$ RAM footprint across all UI buffers. To guarantee compliance:
  - The font glyph atlas texture MUST use a single-channel Alpha8 (`R8_UNORM`, 1 byte per texel) layout, bounding a $2048 \times 2048$ atlas to $\le 4.0\text{ MB}$ (or $1024 \times 1024$ to $\le 1.0\text{ MB}$). Shaders SHALL sample the single channel as alpha coverage modulated by vertex color. 32-bit RGBA glyph textures are prohibited.
  - OSD rendering SHALL utilize dirty-rect compositing, bounding framebuffer allocations to $\le 8.5\text{ MB}$.
  - State snapshot buffers and SPSC command queues SHALL be bounded to $\le 1.0\text{ MB}$.
- **CPU Overhead**: In steady-state playback with idle OSD, UI core utilization MUST remain $< 0.2\%$. Under active chat animations, CPU utilization MUST NOT exceed $1.2\%$.
- **Allocation Invariant**: Zero heap allocations (`malloc` / `Vec::new`) SHALL occur in the hot per-frame render loop; all vertex arrays and glyph quads MUST use pre-allocated static or ring buffers.

#### Scenario: Measure Steady-State Memory Allocation
- **Given** continuous 60 FPS playback for 10 minutes with active OSD compositing and a 2048x2048 R8_UNORM font atlas
- **When** monitoring process heap allocation
- **Then** zero continuous memory growth occurs
- **And** total UI memory allocation remains strictly under $15\text{ MB}$.
```

---

## 7. Associated Updates to Change Proposals

To ensure bidirectional consistency between capability specs and change proposals, the implementation worker should also apply the following targeted updates to the Phase 1 change files:

### 7.1 `openspec/changes/005-smart-chat-storage.md`
- **Section 3 (`Affected Crates & Components`)**:
  - `src/storage/recovery.rs`: Note that the recovery state machine advances byte-by-byte past false sync words with oversized lengths, treating torn EOF writes only as trailing unparsed bytes at physical EOF.
  - `src/index/tokenizer.rs`: Explicitly note CJK unigram and overlapping bigram segmentation for `U+4E00`–`U+9FFF`, and length filter exemption for single CJK ideographs.
- **Section 4 (`Deliverables & Implementation Plan`)**:
  - Update Deliverable 1: "Recovery scanner salvaging valid records upon detecting torn writes at EOF, with false-sync word rejection preventing premature truncation."
  - Update Deliverable 2: "Inverted Index & Full-Text Search with Unicode CJK n-gram segmentation and single-ideograph indexing."

### 7.2 `openspec/changes/006-ui-osd-renderer.md`
- **Section 3 (`Affected Crates & Components`)**:
  - `src/snapshot/model.rs`: Note SPSC triple-buffer publisher constraint with Telemetry Aggregator thread.
  - `src/osd/scrub_bar.rs`: Note 6-command debounce scenario (5 throttled intermediate + 1 authoritative release).
- **Section 4 (`Deliverables & Implementation Plan`)**:
  - Update Deliverable 1: "SPSC Triple-Buffered Atomic Snapshot published exclusively by the central Telemetry Aggregator."
  - Update Deliverable 4: "Pre-allocated vertex buffers and single-channel Alpha8 (`R8_UNORM`) font glyph atlas texture (4 MB at 2048x2048) ensuring total UI memory footprint remains strictly under 15 MB."

---

## 8. Summary Table for Implementation Worker

| Item | Target File | Line Numbers | Current Problem | Concrete Remediation |
|---|---|---|---|---|
| **1** | `openspec/specs/smart-chat/spec.md` | Lines 75–86 | Mid-stream `0xAA55` + large length triggers Rule 1 EOF truncation, dropping subsequent valid records | Treat oversized length / CRC failure as false-sync candidate; advance byte-by-byte (`offset += 1`); declare torn EOF only if physical EOF reached with trailing unparsed bytes |
| **2** | `openspec/specs/smart-chat/spec.md` | Lines 89–106 | Unspaced CJK text produces single multi-word token; `len < 2` filter wipes out single CJK ideographs | Add character unigram + overlapping bigram segmentation for CJK (`U+4E00`–`U+9FFF`); exempt single CJK ideographs from $\ge 2$ length filter |
| **3** | `openspec/specs/ui-renderer/spec.md` | Lines 23–33, 57–62 | Triple buffer lacks publisher topology; multi-producer writes cause 19% torn snapshot reads | Mandate strict SPSC topology; specify dedicated Telemetry Aggregator thread as sole publisher; UI thread as sole consumer |
| **4** | `openspec/specs/ui-renderer/spec.md` | Lines 99–111 | Scenario claims "at most 5 commands", but 5 intermediate seeks + 1 mouse release seek = 6 commands | Update scenario expectation to exactly 6 seek commands (5 throttled intermediate + 1 final authoritative on mouse release) |
| **5** | `openspec/specs/ui-renderer/spec.md` | Lines 114–125 | 2048x2048 RGBA8 font atlas consumes 16 MB, exceeding 15 MB total UI memory budget | Mandate single-channel Alpha8 (`R8_UNORM`, 1 byte/texel) font atlas layout (4 MB for 2048x2048, 1 MB for 1024x1024) |

---
*Report completed by `explorer_m1_fix_3` for implementation worker dispatch.*
