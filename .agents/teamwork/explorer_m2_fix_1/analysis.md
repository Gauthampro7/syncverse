# Analysis Report: Wire Framing Defect in `storage_auditor.md`

**Author**: `explorer_m2_fix_1` (teamwork_preview_explorer)  
**Date**: 2026-10-25  
**Target File**: `.pi/agents/storage_auditor.md`  
**Reference Specification**: `openspec/specs/smart-chat/spec.md`  
**Authoritative Test**: `tests/empirical_challenge/test_smart_chat.py`  
**Adversarial Harness**: `tests/empirical_challenge/test_pi_adversarial_stress.py`  

---

## 1. Executive Summary

Empirical challenge investigation identified a critical specification defect in `.pi/agents/storage_auditor.md`, line 32:
```markdown
record_total_len: u32 (4 bytes, little-endian) = length of body + 4.
```
This formula conflicts directly with the authoritative capability specification (`openspec/specs/smart-chat/spec.md` line 54) and the empirical test harness (`tests/empirical_challenge/test_smart_chat.py` lines 60–63), which define `record_total_len` as the total wire byte size of the entire record:
$$\text{record\_total\_len} = \text{sync\_word (2B)} + \text{length\_field (4B)} + \text{len(body)} + \text{crc32 (4B)} = \text{len(body)} + 10 \ge 48\text{ bytes}$$

Specifying `length of body + 4` omits the 6-byte framing header (2 bytes `sync_word` + 4 bytes `record_total_len`). Empirical execution demonstrates this causes:
1. **False-candidate rejection**: For minimal records ($len(\text{body}) = 39$), `total_len` is 43, triggering the `total_len < 48` lower-bound rejection.
2. **Body & CRC desynchronization**: For records where `len(body) + 4 >= 48`, slicing candidate records by `total_len` truncates the trailing 6 bytes of the record. The parser reads 4 bytes of body payload as the CRC, resulting in 100% CRC check failures.
3. **Recovery loop desynchronization**: The recovery scanner advances byte-by-byte into the record body, flagging every record as corrupted and destroying crash recovery resilience (0/2 records recovered, 3 corrupted chunks flagged in empirical testing).

This report formulates the exact drop-in replacement for `.pi/agents/storage_auditor.md` and provides unified diff patches for it and all affected references.

---

## 2. Root Cause & Wire Framing Breakdown

### 2.1 Binary Record Field Layout

According to `openspec/specs/smart-chat/spec.md` Section 2:

| Offset Range | Field Name | Type / Width | Value / Semantics |
|---|---|---|---|
| `0..2` (2B) | `sync_word` | `[u8; 2]` | Fixed constant `[0xAA, 0x55]` |
| `2..6` (4B) | `record_total_len` | `u32` (LE) | Total byte count of entire record ($\ge 48$) |
| `6..7` (1B) | `record_type` | `u8` | Record type identifier (start of `body`) |
| `7..8` (1B) | `flags` | `u8` | Record flags (e.g. tombstone, edited) |
| `8..16` (8B) | `wall_timestamp_utc_ms` | `u64` (LE) | UTC epoch millisecond timestamp ($t_{wall}$) |
| `16..24` (8B) | `media_timestamp_ms` | `u64` (LE) | Media presentation timestamp ($t_{media}$) |
| `24..40` (16B) | `sender_id` | `[u8; 16]` | UUIDv4 sender identifier |
| `40..41` (1B) | `sender_name_len` | `u8` | Handle byte length ($1 \le N_{\text{name}} \le 64$) |
| `41..(41 + N_name)` | `sender_name` | `[u8; N_name]` | UTF-8 encoded handle string |
| `(41 + N_name)..(45 + N_name)` (4B) | `payload_len` | `u32` (LE) | Payload byte length ($N_{\text{payload}} \ge 0$) |
| `(45 + N_name)..(45 + N_name + N_payload)` | `payload` | `[u8; N_payload]` | UTF-8 message content or binary command data |
| `End - 4..End` (4B) | `crc32` / `body_crc32` | `u32` (LE) | IEEE 802.3 CRC32 computed over `body` (offsets 6 to End - 4) |

### 2.2 Algebraic Derivation of Total Length

The `body` consists of all fields from `record_type` through `payload`:
$$\text{len}(\text{body}) = 1 + 1 + 8 + 8 + 16 + 1 + N_{\text{name}} + 4 + N_{\text{payload}} = 39 + N_{\text{name}} + N_{\text{payload}}$$
With minimal sender name ($N_{\text{name}} = 1$) and empty payload ($N_{\text{payload}} = 0$):
$$\text{len}(\text{body})_{\min} = 39 + 1 + 0 = 40\text{ bytes}$$
*(Note: If sender name length is 0, $\text{len}(\text{body}) = 39\text{ bytes}$).*

The total wire length of the record encompasses:
$$\text{Total Size} = \text{len}(\text{sync\_word}) + \text{len}(\text{record\_total\_len}) + \text{len}(\text{body}) + \text{len}(\text{crc32})$$
$$\text{Total Size} = 2 + 4 + \text{len}(\text{body}) + 4 = \text{len}(\text{body}) + 10$$
Therefore, for any valid record:
$$\text{record\_total\_len} \ge 39 + 10 = 49\text{ bytes} \quad (\text{spec enforced minimum: } \ge 48\text{ bytes})$$

### 2.3 How the Defect Arose

In `.pi/agents/storage_auditor.md` line 32:
```markdown
record_total_len: u32 (4 bytes, little-endian) = length of body + 4.
```
The author conflated the *body + checksum* byte count with the *total wire frame* byte count:
- $\text{len}(\text{body}) + \text{len}(\text{crc32}) = \text{len}(\text{body}) + 4$ (omitting the 6 prefix bytes `sync_word` [2] and `record_total_len` [4]).
- The field is explicitly named `record_total_len`, which in binary container framing (and RFC standards) must represent the distance to the next candidate boundary.

---

## 3. Failure Mechanism in Parser & Scanner

When an implementation or test writer produces records using `record_total_len = len(body) + 4`:

1. **Failure on Minimum Size Records**:
   - For a minimal record ($len(\text{body}) = 39$), `record_total_len = 43`.
   - The scanner checks `if total_len < 48: advance 1 byte`.
   - Result: Every short record is rejected as a corrupted candidate without reading the body.

2. **Failure on Normal Size Records**:
   - For an example record with $len(\text{body}) = 50$, total true wire size is $60$ bytes.
   - Flawed wire value: `record_total_len = 54`.
   - The recovery scanner reads `record_bytes = data[offset : offset + 54]`.
   - Slicing `body = record_bytes[6:-4]` yields bytes 6 through 50 (only 44 bytes of the 50-byte body).
   - Reading `stored_crc = record_bytes[-4:]` reads bytes 50..54 (the last 4 bytes of the body, NOT the CRC32).
   - Computed CRC32 over the truncated 44-byte body does not match the 4 bytes of body payload.
   - Result: CRC verification fails. The scanner advances `offset += 1` instead of `offset += 60`.

3. **Loss of Subsequent Records**:
   - Because the scanner advances `+1` into the record body, it scans byte-by-byte looking for `0xAA55`.
   - Any valid subsequent record at offset $+60$ is either corrupted by mid-stream false sync hits or flagged as an incomplete trailing write at EOF.
   - Result: 0 out of $N$ records recovered; log files are irreparably classified as corrupt.

---

## 4. Verification & Empirical Proof

Running the adversarial stress test harness:
```bash
python3 tests/empirical_challenge/test_pi_adversarial_stress.py
```
Direct output:
```text
============================================================
CHALLENGE 2: STORAGE AUDITOR PROMPT RECORD_TOTAL_LEN DEFECT
============================================================
Flawed DB byte length: 148 bytes.
Records written: 2. Records recovered by authoritative scanner: 0.
Torn writes: 1, Corrupted chunks: 3.
  [VULNERABILITY CONFIRMED] Storage Auditor prompt line 32 ('record_total_len = length of body + 4') causes catastrophic failure: authoritative parser recovered 0 / 2 records and flagged 3 corrupted chunks! Correct value MUST be length of body + 10 (or total record byte size including sync word and length field).
```

In contrast, running the authoritative specification test:
```bash
python3 tests/empirical_challenge/test_smart_chat.py
```
Output:
```text
============================================================
TEST 4: BINARY LOG STORAGE RECOVERY & FALSE-SYNC RESILIENCE
============================================================
Clean log: recovered 10 / 10 records. Torn writes: 0
Truncated EOF log: recovered 9 / 9 expected records. Torn writes: 1
CRC bit-flip in record 3: recovered 9 records (expected 9). Corrupted chunks: 2
--- Verifying False-Sync Resilience ---
Actual recovered records: 9
Torn writes reported: 0
>>> TEST 4 PASSED: False sync word treated as false candidate; all 9 valid records salvaged.
ALL SMART CHAT TESTS PASSED (0 ERRORS).
```

---

## 5. Scope of Affected References

Across the workspace, the following files define or reference wire framing and require alignment:

1. **`.pi/agents/storage_auditor.md` (Primary Defect)**:
   - Line 32: Incorrect formula `length of body + 4`.
   - Line 51: Incomplete false-sync condition (`oversized length that exceeds remaining file length` vs `record_total_len < 48 or current_offset + record_total_len > file_len`).
   - Line 12: Incomplete prohibited database list (omitting `diesel`, `sqlx`, `sqlite3`, `rocksdb-sys`).
2. **`tests/empirical_challenge/test_pi_adversarial_stress.py` (Verification Harness)**:
   - Lines 168–221: Challenge 2 currently hardcodes `flawed_total_len = len(body) + 4`. It should inspect `.pi/agents/storage_auditor.md` to ensure `length of body + 4` is eliminated and that `length of body + 10` is present and functional, turning Challenge 2 into a regression verification check.
3. **`.pi/workflows/005-chat-workflow.md` (Workflow Step 1 & 2)**:
   - Line 28: Mentions `record length (4 bytes)`. Should be clarified to `record total length (4 bytes LE, total record size = len(body) + 10 >= 48 bytes)` for zero ambiguity.
   - Line 39: Mentions only `oversized length`. Should state `invalid length (record_total_len < 48 or exceeding remaining file bytes)`.
4. **`AGENTS.md` (System Specifications)**:
   - Line 148: Mentions `record length (4 bytes)`. Clarify to `total record length (4 bytes LE, total record size = length of body + 10, >= 48 bytes)`.

---

## 6. Formulated Drop-In Replacements

### 6.1 Replacement for `.pi/agents/storage_auditor.md`

The complete, drop-in replacement file is saved at:
`/home/gautham/syncplay-alt/.agents/teamwork/explorer_m2_fix_1/proposed_storage_auditor.md`

Unified diff patch is saved at:
`/home/gautham/syncplay-alt/.agents/teamwork/explorer_m2_fix_1/storage_auditor.patch`

#### Key Snippet (Before vs After)

**Before (`.pi/agents/storage_auditor.md` lines 30–33):**
```markdown
- **Record Wire Format**:
  - `sync_word`: `[0xAA, 0x55]` (2 bytes).
  - `record_total_len`: `u32` (4 bytes, little-endian) = length of body + 4.
  - `body`:
```

**After:**
```markdown
- **Record Wire Format**:
  - `sync_word`: `[0xAA, 0x55]` (2 bytes).
  - `record_total_len`: `u32` (4 bytes, little-endian) = total record length in bytes (sync word 2B + length 4B + body + crc32 4B = length of body + 10, $\ge 48$).
  - `body`:
```

**Before (`.pi/agents/storage_auditor.md` line 51):**
```markdown
4. **False-Sync Word Resilience**:
   - If an application payload contains bytes `[0xAA, 0x55]` followed by an oversized length that exceeds remaining file length or fails CRC, the recovery scanner MUST treat it as a false candidate.
```

**After:**
```markdown
4. **False-Sync Word Resilience**:
   - If an application payload contains bytes `[0xAA, 0x55]` followed by an invalid length (`record_total_len < 48` or `current_offset + record_total_len > file_len`) or fails CRC, the recovery scanner MUST treat it as a false candidate.
```

**Before (`.pi/agents/storage_auditor.md` line 12):**
```markdown
1. **Zero Database Libraries**:
   - The storage layer MUST NOT use SQLite, RocksDB, Sled, Heed, or any embedded KV/relational engine.
```

**After:**
```markdown
1. **Zero Database Libraries**:
   - The storage layer MUST NOT use `sqlite`, `sqlite3`, `rusqlite`, `sqlx`, `sled`, `rocksdb`, `rocksdb-sys`, `heed`, `tantivy`, `meilisearch`, `diesel`, or any embedded KV, relational, or ORM framework.
```

---

### 6.2 Remediation for `tests/empirical_challenge/test_pi_adversarial_stress.py` (Challenge 2)

To enable automated CI verification that verifies `storage_auditor.md` and exits with code 0:

```python
def test_storage_auditor_framing_ambiguity():
    print("\n" + "=" * 60)
    print("CHALLENGE 2: STORAGE AUDITOR PROMPT RECORD_TOTAL_LEN VERIFICATION")
    print("=" * 60)
    
    sys.path.insert(0, "/home/gautham/syncplay-alt/tests/empirical_challenge")
    from test_smart_chat import make_header, ChatRecoveryScanner, SYNC_WORD
    
    findings = []
    prompt_path = "/home/gautham/syncplay-alt/.pi/agents/storage_auditor.md"
    try:
        with open(prompt_path, "r", encoding="utf-8") as f:
            prompt_content = f.read()
    except Exception as e:
        findings.append(f"Could not read {prompt_path}: {e}")
        return findings

    # Check for un-remediated flawed framing formula
    if "length of body + 4" in prompt_content:
        def serialize_record_flawed(record_type, flags, wall_ms, media_ms, sender_id, sender_name, payload):
            name_bytes = sender_name.encode("utf-8")
            body = struct.pack("<BBQQ16sB", record_type, flags, wall_ms, media_ms, sender_id, len(name_bytes)) \
                   + name_bytes + struct.pack("<I", len(payload)) + payload
            crc = zlib.crc32(body) & 0xFFFFFFFF
            flawed_total_len = len(body) + 4
            return SYNC_WORD + struct.pack("<I", flawed_total_len) + body + struct.pack("<I", crc)
            
        header = make_header()
        sender_uuid = b"0123456789abcdef"
        rec1 = serialize_record_flawed(1, 0, 1000, 500, sender_uuid, "Alice", b"Hello world 1")
        rec2 = serialize_record_flawed(1, 0, 1001, 501, sender_uuid, "Bob", b"Hello world 2")
        flawed_db = header + rec1 + rec2
        scanner = ChatRecoveryScanner(flawed_db)
        recovered = scanner.recover()
        err_msg = (
            f"Storage Auditor prompt line 32 ('record_total_len = length of body + 4') "
            f"causes catastrophic failure: authoritative parser recovered {len(recovered)} / 2 records "
            f"and flagged {scanner.corrupted_chunks} corrupted chunks! "
            f"Correct value MUST be length of body + 10 (or total record byte size including sync word and length field)."
        )
        findings.append(err_msg)
        print(f"  [VULNERABILITY CONFIRMED] {err_msg}")
    else:
        # Prompt is remediated. Verify that prompt specifies total record length (len(body) + 10, >= 48)
        if "length of body + 10" not in prompt_content:
            err_msg = "Storage Auditor prompt line 32 does not contain expected formula 'length of body + 10'."
            findings.append(err_msg)
            print(f"  [FAIL] {err_msg}")
        else:
            def serialize_record_remediated(record_type, flags, wall_ms, media_ms, sender_id, sender_name, payload):
                name_bytes = sender_name.encode("utf-8")
                body = struct.pack("<BBQQ16sB", record_type, flags, wall_ms, media_ms, sender_id, len(name_bytes)) \
                       + name_bytes + struct.pack("<I", len(payload)) + payload
                crc = zlib.crc32(body) & 0xFFFFFFFF
                total_len = 2 + 4 + len(body) + 4  # length of body + 10 >= 48
                return SYNC_WORD + struct.pack("<I", total_len) + body + struct.pack("<I", crc)

            header = make_header()
            sender_uuid = b"0123456789abcdef"
            rec1 = serialize_record_remediated(1, 0, 1000, 500, sender_uuid, "Alice", b"Hello world 1")
            rec2 = serialize_record_remediated(1, 0, 1001, 501, sender_uuid, "Bob", b"Hello world 2")
            valid_db = header + rec1 + rec2
            scanner = ChatRecoveryScanner(valid_db)
            recovered = scanner.recover()
            if len(recovered) == 2 and scanner.corrupted_chunks == 0 and scanner.torn_writes == 0:
                print("  [PASS] Storage Auditor prompt wire framing verified: 'length of body + 10' recovered 2/2 records with 0 corruptions.")
            else:
                findings.append(f"Storage Auditor wire framing validation failed: recovered {len(recovered)}/2 records.")

    return findings
```

---

### 6.3 Remediation for `.pi/workflows/005-chat-workflow.md`

**Before (lines 27–28):**
```markdown
2. Implement record framing and serialization (`record.rs`):
   - `[0xAA, 0x55]` sync word, record length (4 bytes), body, body CRC32 (4 bytes).
```

**After:**
```markdown
2. Implement record framing and serialization (`record.rs`):
   - `[0xAA, 0x55]` sync word (2 bytes), record total length (4 bytes LE, total record size = len(body) + 10 >= 48 bytes), body, body CRC32 (4 bytes).
```

**Before (line 39):**
```markdown
   - **False-Sync Resilience**: When payload contains `0xAA55` followed by an oversized length (exceeding remaining file bytes), treat as a false candidate; advance by 1 byte without aborting recovery or dropping subsequent records.
```

**After:**
```markdown
   - **False-Sync Resilience**: When payload contains `0xAA55` followed by an invalid length (`record_total_len < 48` or exceeding remaining file bytes) or CRC mismatch, treat as a false candidate; advance by 1 byte without aborting recovery or dropping subsequent records.
```

---

### 6.4 Remediation for `AGENTS.md`

**Before (line 148):**
```markdown
   - Record Framing: `[0xAA, 0x55]` sync word, record length (4 bytes), body (type, flags, $t_{wall}, t_{media}$, 16-byte UUID sender, sender name length + bytes, payload length + bytes), body CRC32 (4 bytes).
```

**After:**
```markdown
   - Record Framing: `[0xAA, 0x55]` sync word (2 bytes), record total length (4 bytes LE, total record size = length of body + 10, $\ge 48$ bytes), body (type, flags, $t_{wall}, t_{media}$, 16-byte UUID sender, sender name length + bytes, payload length + bytes), body CRC32 (4 bytes).
```
