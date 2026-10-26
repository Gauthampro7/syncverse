# Investigation Report: EBML Timescale Arithmetic Defect & Integer Safety Audit

## 1. Observation

### 1.1 Defect in `compute_simpleblock_pts`
- **File**: `crates/syncplay-demux/src/ebml.rs`
- **Lines**: 21–36
- **Verbatim Source Code**:
  ```rust
  pub fn compute_simpleblock_pts(
      cluster_timecode_ticks: i64,
      block_timecode_ticks: i16,
      timestamp_scale_ns: u64,
  ) -> Result<(i64, i64), DemuxError> {
      let total_ticks = cluster_timecode_ticks
          .checked_add(block_timecode_ticks as i64)
          .ok_or(DemuxError::IntegerOverflow)?;

      let pts_ns = total_ticks
          .checked_mul(timestamp_scale_ns as i64)
          .ok_or(DemuxError::IntegerOverflow)?;

      let pts_ms = pts_ns.div_euclid(1_000_000);
      Ok((pts_ns, pts_ms))
  }
  ```
- **Observed Behavior**:
  Calling `compute_simpleblock_pts(1000, 0, u64::MAX)` produces `Ok((-1000, -1))` instead of `Err(DemuxError::IntegerOverflow)`.
  As recorded in `crates/syncplay-core/tests/adversarial_crates_1_to_4.rs` (lines 574–576):
  ```rust
  let (pts_ns_flawed, pts_ms_flawed) = compute_simpleblock_pts(1000, 0, u64::MAX).unwrap();
  assert_eq!(pts_ns_flawed, -1000);
  assert_eq!(pts_ms_flawed, -1);
  ```

### 1.2 Additional Integer Cast Observations in `crates/syncplay-demux`
Exhaustive inspection and grep search across `crates/syncplay-demux/src/` revealed three additional unsafe integer casts in `crates/syncplay-demux/src/ebml.rs`:
1. **Cluster Timecode Cast** (`crates/syncplay-demux/src/ebml.rs:276`):
   ```rust
   let (_, pts_ms) = compute_simpleblock_pts(
       self.current_cluster_timecode as i64,
       rel_timecode,
       self.timestamp_scale_ns,
   )?;
   ```
   `self.current_cluster_timecode` is `u64`. If `current_cluster_timecode > i64::MAX as u64`, `as i64` wraps to a negative value.
2. **Negative PTS Cast to Unsigned** (`crates/syncplay-demux/src/ebml.rs:283`):
   ```rust
   return Ok(Some(DemuxPacket {
       track_id,
       pts_ms: pts_ms as u64,
       dts_ms: None,
       is_keyframe,
       data: payload,
   }));
   ```
   If `pts_ms < 0` (e.g. from negative relative block timecodes in pre-roll), `pts_ms as u64` performs two's-complement cast, yielding values close to $2^{64}-1$ ($\approx 1.84 \times 10^{19}$ ms).
3. **Seek Skip Cast** (`crates/syncplay-demux/src/ebml.rs:292`):
   ```rust
   reader
       .seek(SeekFrom::Current(data_size as i64))
       .map_err(|e| DemuxError::Io(e.to_string()))?;
   ```
   `data_size` is `u64`. If `data_size > i64::MAX as u64`, `as i64` becomes negative, instructing the stream reader to seek backward rather than forward.

### 1.3 Inspection of `vint.rs` and `isobmff.rs`
- In `crates/syncplay-demux/src/vint.rs`:
  - `leading_zeros = first_byte.leading_zeros() as usize`: safe because `first_byte != 0` limits `leading_zeros` to $0..7$.
  - `octet_width = leading_zeros + 1`: bounded in $1..8$.
  - `marker_shift = (octet_width - 1) * 8 + (8 - octet_width)`: range is $0..56$, strictly $< 64$.
  - `data_mask = (1u64 << marker_shift) - 1`: cannot overflow.
- In `crates/syncplay-demux/src/isobmff.rs`:
  - Box header parsing already bounds 64-bit largesize explicitly against `i64::MAX`:
    ```rust
    const I64_MAX_U64: u64 = i64::MAX as u64;
    if largesize > I64_MAX_U64 {
        return Err(DemuxError::MalformedBox(...));
    }
    ```
  - Sample table offset accumulation uses `checked_add` with `DemuxError::IntegerOverflow`.
  - Duration calculation `(dur as u64 * 1000) / (ts as u64)`: since `dur` is `u32`, $\max(\text{dur} \times 1000) \approx 4.29 \times 10^{12} \ll \text{u64::MAX}$.

---

## 2. Logic Chain

### 2.1 Mechanism of the Defect in `compute_simpleblock_pts`
1. In Rust, `val as i64` where `val: u64` is a bitwise two's-complement reinterpretation.
2. For `timestamp_scale_ns = u64::MAX` (`0xFFFF_FFFF_FFFF_FFFF`), bit 63 (the sign bit) is set. As an `i64`, this bit pattern evaluates to `-1i64`. More generally, for any `timestamp_scale_ns > i64::MAX as u64` ($2^{63}-1$), `as i64` maps the value into $[-2^{63}, -1]$.
3. When `total_ticks` is positive (e.g., $1000$), `total_ticks.checked_mul(timestamp_scale_ns as i64)` computes $1000 \times (-1) = -1000$.
4. Because $-1000$ falls within $[\text{i64::MIN}, \text{i64::MAX}]$, `checked_mul` returns `Some(-1000)` without triggering overflow.
5. `pts_ms = (-1000).div_euclid(1_000_000)` produces $-1$.
6. The actual mathematical timestamp is $1000 \times (2^{64}-1) \approx 1.84 \times 10^{22} \text{ ns}$, which exceeds $\text{i64::MAX} \approx 9.22 \times 10^{18} \text{ ns}$ by over 2000-fold.
7. Thus, the unchecked narrowing/sign cast completely circumvents the `checked_mul` guard, turning an overflow condition into an erroneous negative timestamp.

### 2.2 Proof of Correctness of the Fix
By replacing `timestamp_scale_ns as i64` with:
```rust
let scale = i64::try_from(timestamp_scale_ns)
    .map_err(|_| DemuxError::IntegerOverflow)?;
```
1. `i64::try_from(timestamp_scale_ns)` checks whether `0 <= timestamp_scale_ns <= i64::MAX as u64`.
2. Any value $> 2^{63}-1$ (including `u64::MAX` and `i64::MAX as u64 + 1`) fails conversion and immediately returns `Err(DemuxError::IntegerOverflow)`.
3. Once converted, `scale` is guaranteed to be a non-negative `i64` ($0 \le \text{scale} \le 2^{63}-1$).
4. In `total_ticks.checked_mul(scale)`:
   - For positive `total_ticks`, multiplication by non-negative `scale` yields a positive product or zero. If the product exceeds `i64::MAX`, `checked_mul` returns `None` and evaluates to `Err(DemuxError::IntegerOverflow)`.
   - For negative `total_ticks`, the product is negative or zero. If it underflows `i64::MIN`, `checked_mul` returns `None` and evaluates to `Err(DemuxError::IntegerOverflow)`.
5. Euclidean division by $1{,}000{,}000$ cannot overflow for any valid `i64`.

### 2.3 Hardening of Auxiliary Casts in `ebml.rs`
1. **Cluster Timecode**: Converting `self.current_cluster_timecode` via `i64::try_from(...)` guarantees that corrupted cluster timecodes $> 2^{63}-1$ cannot bypass `compute_simpleblock_pts`.
2. **PTS to Unsigned**: Clamping `pts_ms.max(0) as u64` ensures that any negative pre-roll timestamp is mapped safely to 0 rather than wrapping to $\approx 1.84 \times 10^{19}$ ms.
3. **Seek Skip**: Converting `data_size` via `i64::try_from(...)` ensures `SeekFrom::Current` cannot seek backward when skipping unknown elements.

---

## 3. Caveats

1. The Matroska specification (RFC 8794 / Matroska Spec) defines `TimestampScale` as an unsigned integer element with a default value of $1{,}000{,}000$ (1 ms). Legitimate Matroska files typically use values between $1$ and $1{,}000{,}000$. Timescales exceeding `i64::MAX` ($> 9.22 \times 10^{18}$) are malformed or adversarial.
2. In rare multi-track pre-roll conditions, MKV SimpleBlock relative timecodes can theoretically produce negative PTS if `cluster_timecode + block_timecode < 0`. DemuxPacket's public contract specifies `pts_ms: u64`. Clamping negative timestamps to `0` prevents catastrophic wrap-around while maintaining playback monotonicity.

---

## 4. Conclusion

The root cause of the timescale defect is conclusively identified as an unsafe bitcast `timestamp_scale_ns as i64` in `crates/syncplay-demux/src/ebml.rs:31`.

### Actionable Fix Specification

#### File 1: `crates/syncplay-demux/src/ebml.rs`
1. Update `compute_simpleblock_pts`:
   ```rust
   pub fn compute_simpleblock_pts(
       cluster_timecode_ticks: i64,
       block_timecode_ticks: i16,
       timestamp_scale_ns: u64,
   ) -> Result<(i64, i64), DemuxError> {
       let total_ticks = cluster_timecode_ticks
           .checked_add(block_timecode_ticks as i64)
           .ok_or(DemuxError::IntegerOverflow)?;

       let scale = i64::try_from(timestamp_scale_ns)
           .map_err(|_| DemuxError::IntegerOverflow)?;

       let pts_ns = total_ticks
           .checked_mul(scale)
           .ok_or(DemuxError::IntegerOverflow)?;

       let pts_ms = pts_ns.div_euclid(1_000_000);
       Ok((pts_ns, pts_ms))
   }
   ```
2. Update `EbmlDemuxer::read_packet` around lines 276, 283, and 292:
   ```rust
   let cluster_ticks = i64::try_from(self.current_cluster_timecode)
       .map_err(|_| DemuxError::IntegerOverflow)?;
   let (_, pts_ms) = compute_simpleblock_pts(
       cluster_ticks,
       rel_timecode,
       self.timestamp_scale_ns,
   )?;

   return Ok(Some(DemuxPacket {
       track_id,
       pts_ms: pts_ms.max(0) as u64,
       dts_ms: None,
       is_keyframe,
       data: payload,
   }));
   ...
   _ => {
       // Skip unrecognized element
       let seek_offset = i64::try_from(data_size)
           .map_err(|_| DemuxError::IntegerOverflow)?;
       reader
           .seek(SeekFrom::Current(seek_offset))
           .map_err(|e| DemuxError::Io(e.to_string()))?;
   }
   ```
3. Add Unit Test Suite to `crates/syncplay-demux/src/ebml.rs`:
   ```rust
   #[test]
   fn test_compute_simpleblock_pts_boundary_and_overflow_rejection() {
       // 1. Valid baseline computations
       let (pts_ns, pts_ms) = compute_simpleblock_pts(1000, 0, 1_000_000).unwrap();
       assert_eq!(pts_ns, 1_000_000_000);
       assert_eq!(pts_ms, 1000);

       // 2. Reject u64::MAX timescale (defect reproduction)
       assert_eq!(
           compute_simpleblock_pts(1000, 0, u64::MAX),
           Err(DemuxError::IntegerOverflow)
       );
       assert_eq!(
           compute_simpleblock_pts(0, 0, u64::MAX),
           Err(DemuxError::IntegerOverflow)
       );

       // 3. Reject timescale strictly exceeding i64::MAX
       let overflow_scale = (i64::MAX as u64) + 1;
       assert_eq!(
           compute_simpleblock_pts(1, 0, overflow_scale),
           Err(DemuxError::IntegerOverflow)
       );

       // 4. Accept boundary timescale == i64::MAX with tick 1, reject with tick >= 2
       let max_scale = i64::MAX as u64;
       let (max_ns, max_ms) = compute_simpleblock_pts(1, 0, max_scale).unwrap();
       assert_eq!(max_ns, i64::MAX);
       assert_eq!(max_ms, i64::MAX / 1_000_000);

       assert_eq!(
           compute_simpleblock_pts(2, 0, max_scale),
           Err(DemuxError::IntegerOverflow)
       );

       // 5. Total ticks addition overflow
       assert_eq!(
           compute_simpleblock_pts(i64::MAX, 1, 1_000_000),
           Err(DemuxError::IntegerOverflow)
       );
       assert_eq!(
           compute_simpleblock_pts(i64::MIN, -1, 1_000_000),
           Err(DemuxError::IntegerOverflow)
       );

       // 6. Multiplication overflow with standard timescale (1_000_000 ns = 1 ms)
       let max_safe_ticks = i64::MAX / 1_000_000;
       let (pts_ns_safe, pts_ms_safe) = compute_simpleblock_pts(max_safe_ticks, 0, 1_000_000).unwrap();
       assert_eq!(pts_ns_safe, max_safe_ticks * 1_000_000);
       assert_eq!(pts_ms_safe, max_safe_ticks);

       assert_eq!(
           compute_simpleblock_pts(max_safe_ticks + 1, 0, 1_000_000),
           Err(DemuxError::IntegerOverflow)
       );

       // 7. Negative multiplication underflow
       assert_eq!(
           compute_simpleblock_pts(-(max_safe_ticks + 1), 0, 1_000_000),
           Err(DemuxError::IntegerOverflow)
       );
   }
   ```

#### File 2: `crates/syncplay-core/tests/adversarial_crates_1_to_4.rs`
Update lines 572–577 to assert strict overflow rejection:
```rust
// 3. Extreme timescale overflow detection:
// u64::MAX and timescales > i64::MAX MUST strictly return Err(DemuxError::IntegerOverflow)
assert_eq!(
    compute_simpleblock_pts(1000, 0, u64::MAX),
    Err(DemuxError::IntegerOverflow)
);
assert_eq!(
    compute_simpleblock_pts(i64::MAX / 2, 0, u64::MAX),
    Err(DemuxError::IntegerOverflow)
);
```

#### Patch File
A complete unified patch is saved at:
`/home/gautham/syncplay-alt/.agents/teamwork/explorer_m3_fix_1/fix_demux_timescale.patch`

---

## 5. Verification Method

To verify the fix once applied:

```bash
# 1. Environment setup
. "$HOME/.cargo/env"

# 2. Test syncplay-demux unit tests (including new timescale tests)
cargo test -p syncplay-demux -- --nocapture

# 3. Test adversarial integration test suite
cargo test --test adversarial_crates_1_to_4 -- --nocapture

# 4. Strict clippy check
cargo clippy --workspace --all-targets -- -D warnings

# 5. Invalidation Condition:
# If compute_simpleblock_pts(1000, 0, u64::MAX) returns Ok((-1000, -1)) or any Ok value, the verification fails.
# It MUST return Err(DemuxError::IntegerOverflow).
```
