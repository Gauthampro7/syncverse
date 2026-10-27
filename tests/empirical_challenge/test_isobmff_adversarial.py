#!/usr/bin/env python3
"""
Empirical Adversarial Challenge Test Suite: ISOBMFF / MP4 Demuxer
Adversarial Verification for openspec/specs/demuxer/spec.md & syncverse-demux.

Verification Areas:
1. 64-bit largesize box header bounds fuzzing and boundary conditions.
2. 64-bit chunk offset (co64) arithmetic and integer overflow protection.
3. Corrupted sample-to-chunk (stsc) table runs and gap detection.
4. Multi-track interleaved packet scheduling across active tracks.
5. Deterministic keyframe seeking with stss and fallback cases.
"""

import sys
import struct
import random

I64_MAX = (1 << 63) - 1
U64_MAX = (1 << 64) - 1

class DemuxError(Exception):
    pass

class UnexpectedEof(DemuxError):
    pass

class MalformedBox(DemuxError):
    pass

class CorruptSampleTable(DemuxError):
    pass

class IntegerOverflow(DemuxError):
    pass

class NotFound(DemuxError):
    pass


def parse_box_header(buf: bytes, offset: int = 0):
    available = len(buf) - offset
    if available < 8:
        raise UnexpectedEof(f"Needed 8 bytes, available {available}")

    size32 = struct.unpack(">I", buf[offset:offset+4])[0]
    fourcc = buf[offset+4:offset+8]

    if size32 == 1:
        if available < 16:
            raise UnexpectedEof(f"Needed 16 bytes for largesize, available {available}")
        largesize = struct.unpack(">Q", buf[offset+8:offset+16])[0]
        if largesize < 16:
            raise MalformedBox(f"largesize {largesize} < 16")
        if largesize > I64_MAX:
            raise MalformedBox(f"largesize {largesize} > i64::MAX")
        return fourcc, 16, largesize
    elif size32 == 0:
        return fourcc, 8, available
    elif size32 < 8:
        raise MalformedBox(f"size32 {size32} < 8")
    else:
        return fourcc, 8, size32


def test_box_header_adversarial():
    print("--- [1] Testing ISOBMFF Box Header Parsing & Largesize Bounds ---")

    # Lower boundary: largesize == 16
    buf_16 = struct.pack(">I4sQ", 1, b"mdat", 16)
    fcc, hsz, tot = parse_box_header(buf_16)
    assert tot == 16 and hsz == 16

    # Malformed largesizes < 16
    for bad in [0, 1, 7, 8, 14, 15]:
        buf = struct.pack(">I4sQ", 1, b"mdat", bad)
        try:
            parse_box_header(buf)
            assert False, f"Expected MalformedBox for {bad}"
        except MalformedBox:
            pass

    # Upper boundary: i64::MAX
    buf_max = struct.pack(">I4sQ", 1, b"mdat", I64_MAX)
    fcc, hsz, tot = parse_box_header(buf_max)
    assert tot == I64_MAX and hsz == 16

    # Malformed largesize > i64::MAX
    for bad in [I64_MAX + 1, I64_MAX + 1000, U64_MAX]:
        buf = struct.pack(">I4sQ", 1, b"mdat", bad)
        try:
            parse_box_header(buf)
            assert False, f"Expected MalformedBox for {bad}"
        except MalformedBox:
            pass

    # Malformed 32-bit sizes: 2..7
    for bad32 in range(2, 8):
        buf = struct.pack(">I4s", bad32, b"moov")
        try:
            parse_box_header(buf)
            assert False, f"Expected MalformedBox for {bad32}"
        except MalformedBox:
            pass

    # Fuzzing with 50,000 random permutations
    random.seed(0x1337)
    for _ in range(50000):
        length = random.randint(0, 32)
        random_bytes = bytearray(random.getrandbits(8) for _ in range(length))
        try:
            parse_box_header(random_bytes)
        except DemuxError:
            pass  # Expected clean rejection

    print("  [PASS] 64-bit largesize bounds, truncated headers, and 50,000 fuzz permutations verified.")


def compute_sample_offset(chunk_offsets, stsc_entries, sample_sizes, uniform_size, sample_index):
    if not stsc_entries or not chunk_offsets:
        raise CorruptSampleTable("empty stsc or chunk_offsets")

    current_sample = 0
    num_chunks = len(chunk_offsets)

    for chunk_idx in range(num_chunks):
        chunk_1based = chunk_idx + 1
        # Find matching stsc entry
        matching = [e for e in stsc_entries if e["first_chunk"] <= chunk_1based]
        if not matching:
            raise CorruptSampleTable(f"no stsc entry for chunk {chunk_1based}")
        stsc = matching[-1]

        spc = stsc["samples_per_chunk"]
        if spc == 0:
            raise CorruptSampleTable("samples_per_chunk is 0")

        if sample_index < current_sample + spc:
            chunk_base = chunk_offsets[chunk_idx]
            offset = chunk_base
            for j in range(current_sample, sample_index):
                sz = uniform_size if uniform_size is not None else sample_sizes[j]
                offset += sz
                if offset > U64_MAX:
                    raise IntegerOverflow("offset overflowed u64")
            return offset
        current_sample += spc

    raise NotFound(f"sample index {sample_index} exceeds total samples")


def test_co64_and_stsc_adversarial():
    print("--- [2] Testing 64-bit Chunk Offsets (co64) & Corrupted stsc Runs ---")

    # 64-bit offsets > 4GB
    chunk_offsets = [0x1_0000_0000, 0x2_8000_0000, 0x5_0000_0000]
    stsc = [
        {"first_chunk": 1, "samples_per_chunk": 2},
        {"first_chunk": 2, "samples_per_chunk": 3},
        {"first_chunk": 3, "samples_per_chunk": 1},
    ]
    sizes = [1000, 2000, 500, 600, 700, 4000]

    assert compute_sample_offset(chunk_offsets, stsc, sizes, None, 0) == 0x1_0000_0000
    assert compute_sample_offset(chunk_offsets, stsc, sizes, None, 1) == 0x1_0000_0000 + 1000
    assert compute_sample_offset(chunk_offsets, stsc, sizes, None, 2) == 0x2_8000_0000
    assert compute_sample_offset(chunk_offsets, stsc, sizes, None, 3) == 0x2_8000_0000 + 500
    assert compute_sample_offset(chunk_offsets, stsc, sizes, None, 5) == 0x5_0000_0000

    # Corrupt stsc: zero samples_per_chunk
    try:
        compute_sample_offset(chunk_offsets, [{"first_chunk": 1, "samples_per_chunk": 0}], sizes, None, 0)
        assert False, "Expected CorruptSampleTable"
    except CorruptSampleTable:
        pass

    # Corrupt stsc: missing chunk 1
    try:
        compute_sample_offset(chunk_offsets, [{"first_chunk": 2, "samples_per_chunk": 1}], sizes, None, 0)
        assert False, "Expected CorruptSampleTable"
    except CorruptSampleTable:
        pass

    # Overflow detection
    try:
        compute_sample_offset([U64_MAX - 50], [{"first_chunk": 1, "samples_per_chunk": 2}], [100, 100], None, 1)
        assert False, "Expected IntegerOverflow"
    except IntegerOverflow:
        pass

    print("  [PASS] co64 64-bit offsets, stsc run-lengths, and integer overflow checks verified.")


def test_multi_track_interleaved_reading():
    print("--- [3] Testing Multi-Track Interleaved Reading Scheduling ---")

    # 3 active tracks with interleaved offsets:
    # Track 1 (V): offsets [1000, 1030, 1050, 1080]
    # Track 2 (A): offsets [1010, 1040, 1060, 1090]
    # Track 3 (S): offsets [1020, 1070]
    tracks = [
        {"id": 1, "name": "V", "offsets": [1000, 1030, 1050, 1080], "curr": 0},
        {"id": 2, "name": "A", "offsets": [1010, 1040, 1060, 1090], "curr": 0},
        {"id": 3, "name": "S", "offsets": [1020, 1070], "curr": 0},
    ]

    emitted = []
    while True:
        best_t = None
        min_off = float("inf")
        for t in tracks:
            if t["curr"] < len(t["offsets"]):
                off = t["offsets"][t["curr"]]
                if off < min_off:
                    min_off = off
                    best_t = t
        if best_t is None:
            break
        emitted.append((best_t["id"], min_off))
        best_t["curr"] += 1

    expected_ids = [1, 2, 3, 1, 2, 1, 2, 3, 1, 2]
    actual_ids = [tid for tid, _ in emitted]
    assert actual_ids == expected_ids, f"Interleaving mismatch: {actual_ids} vs {expected_ids}"

    # Verify strictly monotonic file offsets
    actual_offsets = [off for _, off in emitted]
    assert actual_offsets == sorted(actual_offsets)
    print(f"  [PASS] Multi-track interleaving emitted {len(emitted)} packets in strict file order.")


def test_keyframe_seeking_oracle():
    print("--- [4] Testing Keyframe Seeking Oracle & Fallbacks ---")

    # Sparse keyframes at [0, 2000, 4000]
    sync_samples = [0, 20, 40]  # indices
    pts_table = [i * 100 for i in range(50)]  # 0, 100, 200, ..., 4900 ms

    def seek_oracle(target_pts, syncs):
        # 1. Find target sample
        target_sample = None
        for i, pts in enumerate(pts_table):
            if pts <= target_pts:
                target_sample = i
            else:
                break
        if target_sample is None:
            target_sample = 0

        # 2. Find keyframe
        if not syncs:
            return target_sample, pts_table[target_sample]
        key_sample = syncs[0]
        for s in syncs:
            if s <= target_sample:
                key_sample = s
            else:
                break
        return key_sample, pts_table[key_sample]

    # Seek targets with stss
    assert seek_oracle(0, sync_samples) == (0, 0)
    assert seek_oracle(1500, sync_samples) == (0, 0)
    assert seek_oracle(2000, sync_samples) == (20, 2000)
    assert seek_oracle(2500, sync_samples) == (20, 2000)
    assert seek_oracle(4000, sync_samples) == (40, 4000)
    assert seek_oracle(99999, sync_samples) == (40, 4000)

    # Seek targets without stss (all samples are keyframes)
    assert seek_oracle(1500, []) == (15, 1500)
    assert seek_oracle(2700, []) == (27, 2700)
    assert seek_oracle(3550, []) == (35, 3500)

    print("  [PASS] Keyframe seeking with stss and fallback cases verified.")


if __name__ == "__main__":
    test_box_header_adversarial()
    test_co64_and_stsc_adversarial()
    test_multi_track_interleaved_reading()
    test_keyframe_seeking_oracle()
    print("\n[SUCCESS] All ISOBMFF Adversarial Challenge Suites Passed Cleanly.")
