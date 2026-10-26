#!/usr/bin/env python3
"""
Empirical Challenge Harness 2: Smart Chat Storage & FTS (Remediated)
Tests:
1. Binary log layout (SPCH magic, version, CRC32, 0xAA55 sync framing)
2. Remediated recovery algorithm:
   - Torn write at EOF handling (authoritative trailing incomplete record)
   - CRC32 bit-flip corruption handling (byte-by-byte forward scan)
   - False-sync resilience: mid-stream payload containing 0xAA55 with oversized length
     is treated as false candidate and does NOT abort recovery or drop subsequent records!
3. Remediated Unicode Inverted Index FTS:
   - Case folding (Latin, Turkish dotted I)
   - Punctuation stripping with # and @ preserved
   - CJK unigram + overlapping bigram segmentation (Unicode U+4E00..U+9FFF)
   - Preserves single CJK ideographs (length bounds 1..48 for CJK, 2..48 for Western)
"""

import struct
import zlib
import re
import unicodedata
from typing import List, Dict, Optional, Tuple

HEADER_MAGIC = b"SPCH"
HEADER_VERSION = 1
SYNC_WORD = bytes([0xAA, 0x55])

def make_header(flags: int = 0) -> bytes:
    # 16-byte fixed header: magic (4), version (2), flags (2), reserved (4), crc32 (4)
    hdr_prefix = struct.pack("<4sHHI", HEADER_MAGIC, HEADER_VERSION, flags, 0)
    crc = zlib.crc32(hdr_prefix) & 0xFFFFFFFF
    return hdr_prefix + struct.pack("<I", crc)

def make_record(
    record_type: int,
    flags: int,
    wall_ms: int,
    media_ms: int,
    sender_id: bytes,
    sender_name: str,
    payload: bytes
) -> bytes:
    name_bytes = sender_name.encode("utf-8")
    assert len(name_bytes) <= 64
    assert len(sender_id) == 16
    
    # Body starts from record_type through payload
    body = struct.pack(
        "<BBQQ16sB",
        record_type,
        flags,
        wall_ms,
        media_ms,
        sender_id,
        len(name_bytes)
    ) + name_bytes + struct.pack("<I", len(payload)) + payload
    
    crc = zlib.crc32(body) & 0xFFFFFFFF
    
    # record_total_len = sync_word (2) + len (4) + body + crc (4)
    total_len = 2 + 4 + len(body) + 4
    
    record = SYNC_WORD + struct.pack("<I", total_len) + body + struct.pack("<I", crc)
    return record


class ChatRecoveryScanner:
    def __init__(self, data: bytes):
        self.data = data
        self.offset = 0
        self.valid_records = []
        self.torn_writes = 0
        self.corrupted_chunks = 0
        self.last_valid_end = 16

    def recover(self) -> List[dict]:
        file_len = len(self.data)
        if file_len < 16:
            return []
            
        # Check header
        hdr = self.data[:16]
        magic, ver, flags, reserved, hdr_crc = struct.unpack("<4sHHII", hdr[:16])
        if magic != HEADER_MAGIC:
            raise ValueError("Invalid magic")
        calc_crc = zlib.crc32(hdr[:12]) & 0xFFFFFFFF
        if calc_crc != hdr_crc:
            raise ValueError("Corrupt header CRC")
            
        self.offset = 16
        self.last_valid_end = 16
        
        while self.offset < file_len:
            # If sync word is absent, advance forward
            if self.data[self.offset:self.offset+2] != SYNC_WORD:
                next_sync = self.data.find(SYNC_WORD, self.offset)
                if next_sync == -1:
                    # No more sync candidates
                    break
                self.corrupted_chunks += 1
                self.offset = next_sync
                continue
                
            # Found SYNC_WORD candidate
            if self.offset + 6 > file_len:
                # Incomplete candidate header at physical EOF
                break
                
            total_len = struct.unpack("<I", self.data[self.offset+2:self.offset+6])[0]
            
            # REMEDIATED SPEC:
            # If total_len < 48 or offset + total_len > file_len:
            # Treat as false sync word or mid-stream corruption candidate.
            # Advance byte-by-byte and continue scanning without terminating!
            if total_len < 48 or self.offset + total_len > file_len:
                self.corrupted_chunks += 1
                self.offset += 1
                continue
                
            # Try parsing the candidate record
            record_bytes = self.data[self.offset : self.offset + total_len]
            body_and_crc = record_bytes[6:]
            body = body_and_crc[:-4]
            stored_crc = struct.unpack("<I", body_and_crc[-4:])[0]
            calc_crc = zlib.crc32(body) & 0xFFFFFFFF
            
            if calc_crc != stored_crc:
                # CRC mismatch: mid-stream corruption candidate
                self.corrupted_chunks += 1
                self.offset += 1 # Scan forward byte-by-byte
                continue
                
            # Valid record!
            rec_type, flags, wall_ms, media_ms, sender_id, name_len = struct.unpack("<BBQQ16sB", body[:35])
            name = body[35 : 35 + name_len].decode("utf-8", errors="replace")
            payload_len = struct.unpack("<I", body[35 + name_len : 39 + name_len])[0]
            payload = body[39 + name_len : 39 + name_len + payload_len]
            
            self.valid_records.append({
                "type": rec_type,
                "wall_ms": wall_ms,
                "media_ms": media_ms,
                "sender_name": name,
                "payload": payload
            })
            
            self.offset += total_len
            self.last_valid_end = self.offset
            
        # Declare torn write at EOF if unparsed trailing bytes remain after last valid record
        if self.last_valid_end < file_len:
            self.torn_writes = 1
            
        return self.valid_records


def test_storage_recovery():
    print("=" * 60)
    print("TEST 4: BINARY LOG STORAGE RECOVERY & FALSE-SYNC RESILIENCE")
    print("=" * 60)
    
    sender_id = b"\x01" * 16
    
    # Generate 10 valid records
    records = []
    for i in range(10):
        rec = make_record(
            record_type=1,
            flags=0,
            wall_ms=1000 + i * 10,
            media_ms=i * 5000,
            sender_id=sender_id,
            sender_name=f"User{i}",
            payload=f"Message {i}".encode("utf-8")
        )
        records.append(rec)
        
    full_log = make_header() + b"".join(records)
    print(f"Generated clean log with 10 records: {len(full_log)} bytes.")
    
    # 4.1 Clean log recovery
    scanner1 = ChatRecoveryScanner(full_log)
    recovered1 = scanner1.recover()
    print(f"Clean log: recovered {len(recovered1)} / 10 records. Torn writes: {scanner1.torn_writes}")
    assert len(recovered1) == 10
    assert scanner1.torn_writes == 0
    
    # 4.2 Torn write at EOF (truncate last record by 15 bytes)
    truncated_log = full_log[:-15]
    scanner2 = ChatRecoveryScanner(truncated_log)
    recovered2 = scanner2.recover()
    print(f"Truncated EOF log: recovered {len(recovered2)} / 9 expected records. Torn writes: {scanner2.torn_writes}")
    assert len(recovered2) == 9
    assert scanner2.torn_writes == 1
    
    # 4.3 Mid-stream CRC32 corruption in Record 3 (index 2)
    corrupted_log_bytes = bytearray(full_log)
    rec3_offset = 16 + len(records[0]) + len(records[1])
    corrupted_log_bytes[rec3_offset + 25] ^= 0xFF
    scanner3 = ChatRecoveryScanner(bytes(corrupted_log_bytes))
    recovered3 = scanner3.recover()
    print(f"CRC bit-flip in record 3: recovered {len(recovered3)} records (expected 9). Corrupted chunks: {scanner3.corrupted_chunks}")
    assert len(recovered3) == 9
    assert scanner3.corrupted_chunks >= 1
    
    # 4.4 False-sync sequence resilience test
    print("\n--- Verifying False-Sync Resilience ---")
    attack_payload = b"Corrupted data with \xAA\x55\xFF\xFF\xFF\x00 and more text"
    bad_rec = make_record(
        record_type=1,
        flags=0,
        wall_ms=1020,
        media_ms=10000,
        sender_id=sender_id,
        sender_name="BadUser",
        payload=attack_payload
    )
    bad_rec_corrupted = bytearray(bad_rec)
    bad_rec_corrupted[10] ^= 0x55 # Corrupt body
    
    attack_log = make_header() + records[0] + records[1] + bytes(bad_rec_corrupted) + b"".join(records[3:])
    scanner_attack = ChatRecoveryScanner(attack_log)
    recovered_attack = scanner_attack.recover()
    print(f"Total valid records in file: 9 (records 0, 1, and 3..9)")
    print(f"Actual recovered records: {len(recovered_attack)}")
    print(f"Torn writes reported: {scanner_attack.torn_writes}")
    assert len(recovered_attack) == 9, f"Error: False sync word prematurely truncated file; recovered {len(recovered_attack)}/9"
    assert scanner_attack.torn_writes == 0
    assert scanner_attack.corrupted_chunks >= 1
    print(">>> TEST 4 PASSED: False sync word treated as false candidate; all 9 valid records salvaged.")


def test_fts_unicode_and_tokenization():
    print("\n" + "=" * 60)
    print("TEST 5: INVERTED INDEX FTS & UNICODE BOUNDARIES (REMEDIATED)")
    print("=" * 60)
    
    STOP_WORDS = set(["the", "is", "at", "which", "on", "and", "a", "an", "to", "for", "in", "of", "it", "this", "that", "with", "as", "by"])
    
    def is_cjk(ch: str) -> bool:
        return 0x4E00 <= ord(ch) <= 0x9FFF

    def tokenize_spec(text: str) -> List[str]:
        # 1. Unicode case folding
        folded = text.casefold()
        
        # 2. Token segmentation with CJK unigrams/bigrams and Western word bounding
        tokens = []
        i = 0
        n = len(folded)
        while i < n:
            ch = folded[i]
            if is_cjk(ch):
                cjk_run = []
                while i < n and is_cjk(folded[i]):
                    cjk_run.append(folded[i])
                    i += 1
                # Unigrams (exempt from 2-char length filter)
                for c in cjk_run:
                    tokens.append(c)
                # Overlapping bigrams
                for j in range(len(cjk_run) - 1):
                    tokens.append(cjk_run[j] + cjk_run[j+1])
            elif ch in ('#', '@') or ch.isalnum() or ch == '_':
                word_chars = []
                while i < n and not is_cjk(folded[i]) and not folded[i].isspace():
                    c2 = folded[i]
                    if c2 in ('#', '@') or c2.isalnum() or c2 == '_':
                        word_chars.append(c2)
                    i += 1
                word = "".join(word_chars)
                if word:
                    # Western token: 2 <= len <= 48 and not in stop words
                    if 2 <= len(word) <= 48 and word not in STOP_WORDS:
                        tokens.append(word)
            else:
                i += 1
                
        return tokens

    # Test Case A: Standard English
    text_en = "Check out that awesome guitar solo at 14:30! #riff @alice"
    tokens_en = tokenize_spec(text_en)
    print(f"English: '{text_en}'")
    print(f"Tokens: {tokens_en}")
    assert "guitar" in tokens_en
    assert "awesome" in tokens_en
    assert "#riff" in tokens_en
    assert "@alice" in tokens_en
    
    # Test Case B: CJK non-segmented text (Chinese / Japanese)
    text_cjk = "这是一段非常精彩的吉他独奏"
    tokens_cjk = tokenize_spec(text_cjk)
    print(f"\nCJK sentence: '{text_cjk}'")
    print(f"Tokens: {tokens_cjk}")
    query_cjk = tokenize_spec("吉他")
    print(f"Query '吉他' tokens: {query_cjk}")
    match = all(q in tokens_cjk for q in query_cjk)
    print(f"Does FTS match '吉他' in CJK text? {match}")
    assert match, "Error: CJK query '吉他' must match text via bigram tokenization"

    # Test Case C: Token length boundary in Unicode chars (Single CJK Ideographs)
    text_single_cjk = "猫 狗"
    tokens_cjk_single = tokenize_spec(text_single_cjk)
    print(f"\nSingle CJK characters '猫 狗': tokens = {tokens_cjk_single}")
    assert "猫" in tokens_cjk_single
    assert "狗" in tokens_cjk_single

    # Test Case D: Turkish dotted / dotless I case folding
    text_tr = "İSTANBUL"
    tokens_tr = tokenize_spec(text_tr)
    print(f"\nTurkish case folding 'İSTANBUL': tokens = {tokens_tr}")
    assert len(tokens_tr) == 1
    assert "i\u0307stanbul" in tokens_tr or "istanbul" in tokens_tr[0]
    
    print(">>> TEST 5 PASSED: Inverted index FTS supports multi-language CJK unigram/bigrams and single ideographs.")


if __name__ == "__main__":
    test_storage_recovery()
    test_fts_unicode_and_tokenization()
    print("\nALL SMART CHAT TESTS PASSED (0 ERRORS).")
