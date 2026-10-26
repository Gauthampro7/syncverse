# Handoff Report: Adversarial Challenge of Matroska / EBML Demuxer (`crates/syncplay-demux/src/ebml.rs`)

**Target**: `crates/syncplay-demux/src/ebml.rs`  
**Challenger**: `challenger_p2_demux_1` (teamwork_preview_challenger / critic)  
**Date**: 2026-10-25T09:50:00Z  
**Verdict**: **`REQUEST_CHANGES`**  
**Type**: Hard Handoff  

---

## 1. Observation

1. **Defect 1 — EBML Lacing Signed Delta `parse_vint(..., true)` Rejection**:
   - Location: `crates/syncplay-demux/src/ebml.rs:172`
   - Code:
     ```rust
     let (raw_vint, _, _) = parse_vint(&payload[offset..offset + w], true)?;
     ```
   - In `crates/syncplay-demux/src/vint.rs:49-56`:
     ```rust
     if is_element_id {
         let marker_shift = (octet_width - 1) * 8 + (8 - octet_width);
         let data_mask = (1u64 << marker_shift) - 1;
         if (raw_val & data_mask) == data_mask {
             return Err(VintError::ReservedElementId);
         }
         Ok((raw_val, None, octet_width))
     }
     ```
   - When a valid EBML signed delta VINT has all data bits set to 1 (e.g. `0xFF` for a 1-octet delta of $+64$, since $val = 127, bias = 63, delta = 127 - 63 = 64$), `parse_vint` returns `Err(VintError::ReservedElementId)`.
   - Tool execution result (`crates/syncplay-demux/tests/ebml_adversarial_challenge.rs:test_lacing_mode_3_ebml_defect_reserved_element_id_rejection`):
     ```
     Unpack 3-frame lacing with delta 0xFF result: Err(Vint(ReservedElementId))
     ```

2. **Defect 2 — EBML Lacing with `frame_count == 1` Produces Phantom Extra Frame**:
   - Location: `crates/syncplay-demux/src/ebml.rs:141-209`
   - Code:
     ```rust
     let frame_count = (payload[0] as usize) + 1;
     ...
     let mut prev_size = first_size_opt.ok_or_else(...) ? as usize;
     offset += vint_len;
     sizes.push(prev_size);

     for _ in 1..frame_count - 1 { ... } // Empty loop for frame_count = 1

     let sum_sizes: usize = sizes.iter().sum();
     ...
     let last_sz = payload.len() - offset - sum_sizes;
     sizes.push(last_sz);
     ```
   - When `payload[0] == 0` (`frame_count = 1`), `sizes.push(prev_size)` pushes the first frame's parsed size, the delta loop does not run, and `sizes.push(last_sz)` pushes the remaining 0 bytes.
   - `sizes.len()` becomes 2, returning `vec![frame0, vec![]]`.
   - Tool execution result (`crates/syncplay-demux/tests/ebml_adversarial_challenge.rs:test_lacing_mode_3_ebml_defect_frame_count_one_generates_two_frames`):
     ```
     Returned 2 frames for frame_count=1!
     ```

3. **Demuxer Resilience on Cues Seeking and Multi-Track Interleaving**:
   - `seek_to(target_pts)` binary search correctly clamps to `cues[0]` when `target_pts` is before the first cue, lands on preceding cue when between cues, lands on exact cue, clamps to last cue when target is beyond last cue up to `u64::MAX`, and falls back to PTS `0` when `cues` is empty.
   - `self.packet_queue.clear()` in `seek_to` guarantees that pre-seek buffered packets are flushed.
   - Multi-track interleaved stream extraction preserves FIFO ordering across Video Track 1 and Audio Track 2.
   - `build_synthetic_full_mkv()` demuxes 4 packets sequentially across both tracks with exact PTS and keyframe alignment.

---

## 2. Logic Chain

1. **RFC 8794 & Matroska Specification on Signed VINTs vs Element IDs**:
   - RFC 8794 Section 4.1 specifies that an *Element ID* with all data bits set to 1 is reserved (e.g. `0xFF`, `0x7FFF`).
   - In Matroska EBML lacing, the size difference of frame $n$ relative to frame $n-1$ is encoded as a *signed VINT*, where the value is biased by $2^{7w-1}-1$.
   - A signed VINT is NOT an Element ID. All data bit combinations are valid delta representations. For example, `0xFF` in 1 octet is the maximum positive 1-octet delta $+64$.
   - By calling `parse_vint(..., true)` on delta bytes (Observation 1), the parser mistakenly enforces the Element ID reservation rule on a size delta, causing valid laced blocks to fail demuxing.

2. **Lacing Frame Count Invariant**:
   - In Matroska, byte 0 of a laced block specifies `frame_count - 1`.
   - When byte 0 is `0`, the block declares 1 frame.
   - In Observation 2, `unpack_lacing(3, ...)` unconditionally appends both `prev_size` and `last_sz`, yielding 2 frames for a declared 1-frame block.
   - When pushed to `DemuxPacket` queue, this injects a corrupted 0-byte non-keyframe packet into playback.

3. **Necessity of Change Request**:
   - While normal sequential playback of unlaced or Fixed/Xiph laced blocks succeeds, real-world Matroska files frequently use EBML lacing with positive deltas (e.g. audio frames of varying packet sizes). Encountering delta $+64$ or a 1-frame lace will trigger demux errors or decoder crashes.
   - Therefore, a formal `REQUEST_CHANGES` verdict is warranted.

---

## 3. Caveats

- **No Caveats on Tested Surface**: Frame lacing, Cues seeking boundaries, and multi-track packet queue invariants were directly and empirically verified through the test suite at `crates/syncplay-demux/tests/ebml_adversarial_challenge.rs`.
- **Untested Angles**: Very large files (> 4GB) on disk could not be created within sandbox memory limits, though 64-bit integer calculations for offsets and timestamps were verified against overflow.

---

## 4. Conclusion

**Verdict: `REQUEST_CHANGES`**

The implementation of `crates/syncplay-demux/src/ebml.rs` contains two concrete defects in EBML frame lacing:
1. `crates/syncplay-demux/src/ebml.rs:172`: Signed delta VINTs are decoded with `parse_vint(..., true)`, causing valid deltas whose data bits are all 1 (`0xFF` for $+64$, `0x7FFF` for $+8192$) to be falsely rejected as `VintError::ReservedElementId`.
2. `crates/syncplay-demux/src/ebml.rs:141-209`: When `payload[0] == 0` (`frame_count = 1`), `unpack_lacing` returns 2 frames (1 payload frame + 1 empty frame), injecting phantom packets into the playback stream.

The worker must address these two issues before Milestone 1 Matroska demuxing can be approved.

---

## 5. Verification Method

To independently verify the defects and reproducing test suite:

```bash
# 1. Run the empirical adversarial challenge test suite (15 tests)
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p syncplay-demux --test ebml_adversarial_challenge -- --nocapture

# 2. Run Python test runner
python3 tests/empirical_challenge/test_ebml_adversarial.py

# 3. Specifically verify Defect 1 (ReservedElementId on delta 0xFF)
cargo test -p syncplay-demux --test ebml_adversarial_challenge -- test_lacing_mode_3_ebml_defect_reserved_element_id_rejection --nocapture

# 4. Specifically verify Defect 2 (frame_count=1 emits 2 frames)
cargo test -p syncplay-demux --test ebml_adversarial_challenge -- test_lacing_mode_3_ebml_defect_frame_count_one_generates_two_frames --nocapture
```

### Invalidation Conditions:
- If `unpack_lacing(3, &[2, 0x8A, 0xFF, ...])` cleanly returns 3 valid frames without returning `ReservedElementId`.
- If `unpack_lacing(3, &[0, 0x85, ...])` returns either an error or exactly 1 frame without emitting an empty second frame.
