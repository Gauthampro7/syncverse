# Challenge Report: Milestone 1 Matroska / EBML Parser (`crates/syncplay-demux/src/ebml.rs`)

**Target**: `crates/syncplay-demux/src/ebml.rs`  
**Challenger**: `challenger_p2_demux_1` (teamwork_preview_challenger / critic)  
**Date**: 2026-10-25T09:50:00Z  
**Verdict**: **`REQUEST_CHANGES`**

---

## Challenge Summary

**Overall risk assessment**: **HIGH**

Empirical challenge testing against `crates/syncplay-demux/src/ebml.rs` uncovered **2 confirmed defects** in the EBML frame lacing unpacking implementation:
1. **False Rejection of Valid Laced Frames (`ReservedElementId`)**: In EBML lacing (`mode = 3`), signed delta VINTs are parsed with `is_element_id = true` (line 172). Any signed delta whose data bits are all 1 (such as `0xFF` representing $+64$ in a 1-octet VINT, or `0x7FFF` representing $+8192$ in a 2-octet VINT) is rejected with `DemuxError::Vint(VintError::ReservedElementId)`. This causes valid Matroska laced SimpleBlocks to fail to demux.
2. **Phantom Extra Frame Emission on Single-Frame EBML Lace**: When an EBML laced payload declares `payload[0] == 0` (`frame_count = 1`), `unpack_lacing` parses the first frame size, skips the empty delta loop, and unconditionally calculates and appends `last_sz = payload.len() - offset - sum_sizes`. This emits 2 frames (`[[payload], []]`) instead of 1, resulting in a phantom 0-byte non-keyframe `DemuxPacket` pushed into the player's packet queue.

Cues-based seeking across boundary conditions and multi-track packet queue FIFO interleaving were found to be robust.

---

## Challenges

### [High] Challenge 1: Signed Delta VINT with All Data Bits = 1 Incorrectly Rejected as `ReservedElementId`

- **Assumption challenged**: The implementation assumes `parse_vint(slice, true)` is suitable for decoding EBML signed delta VINTs.
- **Attack scenario**:
  In a 3-frame EBML-laced SimpleBlock:
  - Frame 0 size: 10 bytes (`0x8A`)
  - Frame 1 delta: $+64$ bytes relative to frame 0
  - In a 1-octet signed VINT, raw byte `0xFF` has marker bit at bit 7 (`0x80`) and data bits `0x7F` (127). The bias is $2^6 - 1 = 63$. The delta is $127 - 63 = +64$.
  - At line 172:
    ```rust
    let (raw_vint, _, _) = parse_vint(&payload[offset..offset + w], true)?;
    ```
  - Because `is_element_id = true`, `parse_vint` checks `(raw_val & data_mask) == data_mask`. Since all data bits are 1, it returns `Err(VintError::ReservedElementId)`.
- **Blast radius**:
  Any Matroska file where frame deltas happen to equal $+64$ (1 octet), $+8192$ (2 octets), $+1048576$ (3 octets), or $+134217728$ (4 octets) fails demuxing with an error, aborting packet playback.
- **Empirical test**: `test_lacing_mode_3_ebml_defect_reserved_element_id_rejection` in `crates/syncplay-demux/tests/ebml_adversarial_challenge.rs`:
  ```rust
  let mut payload = vec![2, 0x8A, 0xFF];
  payload.extend(vec![0xAA; 10]);
  payload.extend(vec![0xBB; 74]);
  payload.extend(vec![0xCC; 5]);
  let res = unpack_lacing(3, &payload);
  assert!(matches!(res, Err(DemuxError::Vint(VintError::ReservedElementId))));
  ```
  Result: **REPRODUCED**.
- **Mitigation**:
  Change line 172 to `parse_vint(&payload[offset..offset + w], false)?;` so that the element ID reservation check is not executed on signed size delta values, or parse the raw bytes directly without `parse_vint`.

---

### [Medium] Challenge 2: EBML Lacing with `frame_count == 1` Emits Phantom 0-Byte Frame

- **Assumption challenged**: The implementation assumes `sizes.push(last_sz)` should be called unconditionally after `sizes.push(prev_size)` for all `frame_count` values.
- **Attack scenario**:
  A laced SimpleBlock payload with lacing flag `0b11` (EBML lacing) and `payload[0] == 0`:
  - `frame_count = (payload[0] as usize) + 1 = 1`.
  - Line 153 pushes `prev_size` (the first frame's parsed size).
  - Loop `for _ in 1..frame_count - 1` (i.e. `1..0`) is empty and skipped.
  - Line 198 pushes `last_sz = payload.len() - offset - sum_sizes`.
  - `sizes.len()` becomes 2.
  - `unpack_lacing` returns `vec![frame0_data, vec![]]`.
- **Blast radius**:
  Injects a phantom 0-byte packet with `is_keyframe = false` into `packet_queue`, causing audio/video decoders downstream to receive empty buffers and potentially error out.
- **Empirical test**: `test_lacing_mode_3_ebml_defect_frame_count_one_generates_two_frames` in `crates/syncplay-demux/tests/ebml_adversarial_challenge.rs`:
  ```rust
  let payload = vec![0, 0x85, 1, 2, 3, 4, 5];
  let res = unpack_lacing(3, &payload).unwrap();
  assert_eq!(res.len(), 2);
  assert_eq!(res[1], Vec::<u8>::new());
  ```
  Result: **REPRODUCED**.
- **Mitigation**:
  In `unpack_lacing(3, payload)`:
  - If `frame_count == 1`: reject as invalid block (`return Err(DemuxError::CorruptedBlock("EBML lacing requires at least 2 frames".into()));`), OR
  - Do not parse first size VINT if `frame_count == 1`, returning `vec![payload[1..].to_vec()]`.

---

### [Low] Challenge 3: Reader Desynchronization on Unknown Cluster Children with Undefined Size

- **Assumption challenged**: All unknown cluster child elements have defined `size_opt`.
- **Attack scenario**:
  In `read_packet()` at lines 1014–1023:
  ```rust
  _ => {
      if let Some(sz) = size_opt {
          let seek_offset = i64::try_from(sz).map_err(|_| DemuxError::IntegerOverflow)?;
          reader.seek(SeekFrom::Current(seek_offset)).map_err(|e| DemuxError::Io(e.to_string()))?;
      }
  }
  ```
  If an unknown element inside a Cluster has an undefined size (`size_opt == None`), the `_` arm does not advance the reader. The next loop iteration immediately reads the unknown element's body bytes as the next element ID, leading to stream parsing errors or false-element interpretations.
- **Blast radius**: Minor in compliant files (cluster children usually have defined sizes), but causes demuxer parsing desynchronization on malformed files.
- **Mitigation**:
  If `size_opt` is `None` for an unknown element inside a cluster, return `Err(DemuxError::CorruptedBlock("unknown element in cluster cannot have undefined size".into()))`.

---

## Stress Test Results

| Test Scenario | Target Function | Expected Behavior | Actual Behavior | Result |
|---|---|---|---|---|
| Mode 0 (No lacing) empty buffer | `unpack_lacing(0, &[])` | Return `vec![vec![]]` | `Ok([[]])` | **PASS** |
| Mode 1 (Xiph) empty buffer | `unpack_lacing(1, &[])` | Reject with `CorruptedBlock` | `Err(CorruptedBlock)` | **PASS** |
| Mode 1 (Xiph) truncated header | `unpack_lacing(1, &[2, 50])` | Reject with `CorruptedBlock` | `Err(CorruptedBlock)` | **PASS** |
| Mode 1 (Xiph) 255 accumulation | `unpack_lacing(1, ...)` | Decode 265-byte frame | `Ok(3 frames)` | **PASS** |
| Mode 2 (Fixed) empty buffer | `unpack_lacing(2, &[])` | Reject with `CorruptedBlock` | `Err(CorruptedBlock)` | **PASS** |
| Mode 2 (Fixed) indivisible | `unpack_lacing(2, &[3, 1..5])` | Reject with `CorruptedBlock` | `Err(CorruptedBlock)` | **PASS** |
| Mode 2 (Fixed) divisible | `unpack_lacing(2, &[3, 1..8])` | Partition into 4 uniform frames | `Ok(4 frames of 2B)` | **PASS** |
| Mode 3 (EBML) delta `0xFF` | `unpack_lacing(3, &[2, 0x8A, 0xFF, ..])` | Decode +64 signed delta | `Err(Vint(ReservedElementId))` | **FAIL (DEFECT 1)** |
| Mode 3 (EBML) frame count = 1 | `unpack_lacing(3, &[0, 0x85, ..])` | Return 1 frame or error | Emits 2 frames (`[5B, 0B]`) | **FAIL (DEFECT 2)** |
| Mode 3 (EBML) negative size | `unpack_lacing(3, &[2, 0x85, 0xB5, ..])` | Reject negative frame size | `Err(CorruptedBlock)` | **PASS** |
| Mode 3 (EBML) 2-octet delta | `unpack_lacing(3, &[2, 0x40, 0x64, 0x60, 0x31, ..])` | Decode +50 signed delta | `Ok(3 frames: 100B, 150B, 20B)` | **PASS** |
| Pseudo-random fuzzing (0..128B) | `unpack_lacing(0..=4, random)` | Zero panics across all inputs | Handled without panic | **PASS** |
| Seek before first cue (0ms) | `EbmlDemuxer::seek_to(0)` | Lands on 0ms keyframe | Lands on 0ms | **PASS** |
| Seek between cues (250ms) | `EbmlDemuxer::seek_to(250)` | Lands on preceding cue (0ms) | Lands on 0ms | **PASS** |
| Seek on cue (500ms) | `EbmlDemuxer::seek_to(500)` | Lands on exact cue (500ms) | Lands on 500ms | **PASS** |
| Seek after last cue (1000ms) | `EbmlDemuxer::seek_to(1000)` | Lands on last cue (500ms) | Lands on 500ms | **PASS** |
| Seek beyond bounds (`u64::MAX`) | `EbmlDemuxer::seek_to(u64::MAX)` | Clamps to last cue (500ms) | Lands on 500ms | **PASS** |
| Seek with 0 cues | `EbmlDemuxer::seek_to(500)` | Fallback to start (0ms) | Lands on 0ms | **PASS** |
| Rapid alternating seeks (20x) | `EbmlDemuxer::seek_to` | Clear queue, land on keyframe | Clean packet reads | **PASS** |
| Multi-track interleaving FIFO | `EbmlDemuxer::read_packet` | Alternate Track 1 & 2 in order | Exact track order [1, 2, 1, 2] | **PASS** |
| Packet queue flush on seek | `EbmlDemuxer::seek_to` | Discard queued packets from old PTS | Stale packets discarded | **PASS** |

---

## Unchallenged Areas

- **Cues with Segment Position > 4GB**: Tested within standard 64-bit integer limits, but physical multi-gigabyte disk files were not tested due to memory/disk budget in sandbox.
- **Hardware I/O error injection during `read_exact`**: Relies on `std::io::Error` propagation; hardware failure simulation was out of scope.
