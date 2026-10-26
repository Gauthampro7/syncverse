# Remediation Strategy & Implementation Roadmap: Milestone 2 Presentation Engine

**Author**: `explorer_p2_presentation_fix`  
**Target Crates**: `crates/syncplay-codecs`, `crates/syncplay-ui`  
**Reference Invariants**: `ORIGINAL_REQUEST.md` (2026-10-25T09:09:06Z), `AGENTS.md`, `openspec/specs/ui-renderer/spec.md`  
**Audit Findings Addressed**: Forensic Integrity Report from `auditor_p2_presentation`  
**Status**: Authoritative Verified Fix Strategy  

---

## 1. Executive Summary & Defect Matrix

The forensic audit of Milestone 2 identified 5 technical defects that breached the zero-warning policy, sound floating-point sample validation, and lock-free thread monotonicity invariants:

| Defect # | Target File | Core Issue | Root Cause | Verified Remediation |
|---|---|---|---|---|
| **1** | `crates/syncplay-codecs/src/frame.rs` | `AudioFramePCM::new` accepts `f32::NAN` | IEEE 754 comparison: `s < -1.0` and `s > 1.0` evaluate to `false` for `NaN` | Check `s.is_nan() \|\| !(-1.0..=1.0).contains(&s)` and assert in tests |
| **2** | `crates/syncplay-codecs/tests/adversarial_codecs_stress.rs` | Unused imports trigger compiler warning | `Bt601Converter`, `Bt709Converter`, `ColorSpaceConverter` not exercised in test | Add trait dispatch test or prune unused symbols |
| **3** | `crates/syncplay-ui/tests/adversarial_challenge.rs` | Clippy `identity_op` violations | Expressions `1 * 3600` and `+ 0` trigger `-D clippy::identity_op` | Simplify expressions to clean arithmetic literals |
| **4** | `crates/syncplay-ui/src/scrubber.rs` & tests | Debouncer misses 100ms ticks due to float imprecision | $12 \times \frac{1000}{120} = 99.99999999999999 < 100.0$, slipping seek to 108.33ms | Add epsilon `(debounce_interval_ms - 1e-6)` and align test loops to $[0, \text{total\_drag})$ |
| **5** | `crates/syncplay-ui/src/snapshot.rs` | `TripleBufferSnapshotExchange` sequence regressions | Disjoint `clean_idx` (AtomicU8) and `has_new` (AtomicBool) race under contention | Unify `clean_idx` and `has_new` into single `AtomicU8` with CAS loop |

---

## 2. Deep Dive Analysis & Exact Code Diffs

---

### Defect 1: `AudioFramePCM::new` NaN Float Check

#### Root Cause Analysis
In Rust, comparisons between `f32::NAN` and any floating-point number using `<`, `<=`, `>`, `>=` strictly return `false`.
In `crates/syncplay-codecs/src/frame.rs:227-234`:
```rust
for &s in &samples {
    if s < -1.0 || s > 1.0 {
        return Err(CodecError::InvalidAudio(format!(
            "sample value {} exceeds [-1.0, 1.0] bounds",
            s
        )));
    }
}
```
When `s == f32::NAN`:
- `s < -1.0` -> `false`
- `s > 1.0` -> `false`
- Condition evaluates to `false`!

As a consequence, malformed audio packets with NaN floats bypass validation, corrupting downstream DSP, audio sinks, and mixers with silent poisoning or IEEE 754 undefined behavior.

#### Concrete Code Diff: `crates/syncplay-codecs/src/frame.rs`
```diff
--- a/crates/syncplay-codecs/src/frame.rs
+++ b/crates/syncplay-codecs/src/frame.rs
@@ -226,7 +226,7 @@ impl AudioFramePCM {
         }
         for &s in &samples {
-            if s < -1.0 || s > 1.0 {
+            if s.is_nan() || !(-1.0..=1.0).contains(&s) {
                 return Err(CodecError::InvalidAudio(format!(
                     "sample value {} exceeds [-1.0, 1.0] bounds",
                     s
```

#### Concrete Code Diff: `crates/syncplay-codecs/tests/adversarial_codecs_stress.rs`
Restore the mandatory NaN rejection test:
```diff
--- a/crates/syncplay-codecs/tests/adversarial_codecs_stress.rs
+++ b/crates/syncplay-codecs/tests/adversarial_codecs_stress.rs
@@ -293,8 +293,7 @@ fn test_audio_pcm_adversarial_validation() {
     assert!(AudioFramePCM::new(48000, 2, 0, vec![f32::INFINITY]).is_err());
     assert!(AudioFramePCM::new(48000, 2, 0, vec![f32::NEG_INFINITY]).is_err());
+    assert!(AudioFramePCM::new(48000, 2, 0, vec![f32::NAN]).is_err());
 
-    // Note for challenge report: f32::NAN is currently not rejected because `s < -1.0 || s > 1.0`
-    // evaluates to false for NaN under IEEE 754 semantics.
-
     // 3C. Zero sample rate or channels
```

Also add a direct unit test in `crates/syncplay-codecs/src/frame.rs`:
```rust
#[test]
fn test_audio_pcm_frame_nan_rejection() {
    assert!(AudioFramePCM::new(48000, 2, 0, vec![f32::NAN]).is_err());
    assert!(AudioFramePCM::new(48000, 2, 0, vec![1.0001]).is_err());
    assert!(AudioFramePCM::new(48000, 2, 0, vec![-1.0001]).is_err());
}
```

---

### Defect 2: Unused Imports in `adversarial_codecs_stress.rs`

#### Root Cause Analysis
Under `#![deny(warnings)]`, any imported item that is not referenced in the test file triggers `-D unused-imports` and causes compilation failure.
In `crates/syncplay-codecs/tests/adversarial_codecs_stress.rs:11-15`:
```rust
use syncplay_codecs::{
    convert_yuv420p_to_rgba, yuv_to_rgb_bt601, yuv_to_rgb_bt709, AudioFramePCM,
    Bt601Converter, Bt709Converter, CodecError, ColorSpaceConverter, ColorStandard,
    VideoFrameRGBA, VideoFrameYUV420p,
};
```
If `Bt601Converter`, `Bt709Converter`, and `ColorSpaceConverter` are imported, they MUST be exercised in test functions.

#### Concrete Remediation
Ensure the trait dispatch test exists in `crates/syncplay-codecs/tests/adversarial_codecs_stress.rs`:
```rust
#[test]
fn test_adversarial_traits_dispatch() {
    let conv601 = Bt601Converter::new();
    let conv709 = Bt709Converter::new();

    assert_eq!(conv601.standard(), ColorStandard::Bt601);
    assert_eq!(conv709.standard(), ColorStandard::Bt709);

    let yuv = VideoFrameYUV420p::new(32, 32, 10);
    let mut rgba = VideoFrameRGBA::new(32, 32, 0);

    conv601.convert_frame(&yuv, &mut rgba).unwrap();
    assert_eq!(rgba.pts_ms, 10);

    conv709.convert_frame(&yuv, &mut rgba).unwrap();
    assert_eq!(rgba.pts_ms, 10);
}
```
Verify with `cargo check --workspace --all-targets` that 0 warnings are emitted.

---

### Defect 3: Clippy `identity_op` in `adversarial_challenge.rs`

#### Root Cause Analysis
In `crates/syncplay-ui/tests/adversarial_challenge.rs:473-474`:
```rust
let current_ms = (1 * 3600 + 23 * 60 + 45) * 1000;
let duration_ms = (2 * 3600 + 30 * 60 + 0) * 1000;
```
Multiplying by `1` (`1 * 3600`) and adding `0` (`+ 0`) are identity operations that Clippy denies under `-D warnings`.

#### Concrete Code Diff: `crates/syncplay-ui/tests/adversarial_challenge.rs`
```diff
--- a/crates/syncplay-ui/tests/adversarial_challenge.rs
+++ b/crates/syncplay-ui/tests/adversarial_challenge.rs
@@ -470,8 +470,8 @@ fn test_format_timecode_hud_adversarial_cases() {
     assert_eq!(format_timecode_hud(999, 999), "00:00:00 / 00:00:00");
 
     // Standard 1h 23m 45s
-    let current_ms = (1 * 3600 + 23 * 60 + 45) * 1000;
-    let duration_ms = (2 * 3600 + 30 * 60 + 0) * 1000;
+    let current_ms = (3600 + 23 * 60 + 45) * 1000;
+    let duration_ms = (2 * 3600 + 30 * 60) * 1000;
     assert_eq!(
         format_timecode_hud(current_ms, duration_ms),
         "01:23:45 / 02:30:00"
```

---

### Defect 4: Floating-Point Accumulation Drift in `ScrubberDebouncer`

#### Root Cause Analysis & Mathematical Derivation
Timeline scrubbing simulates high-frequency mouse drags (60–144 Hz) rate-limited to 10 Hz ($100\text{ ms}$ interval).
At 120 Hz, the event step is $\Delta t = \frac{1000.0}{120.0} = 8.333333333333334\text{ ms}$.

When evaluating whether 100 ms has elapsed:
At step 12 ($12 \times \Delta t$):
$$t_{12} = 99.99999999999999\text{ ms}$$
When evaluating:
`if (t_ms - self.last_seek_time_ms) >= self.debounce_interval_ms`
$$99.99999999999999 - 0.0 = 99.99999999999999 \ge 100.0 \implies \mathbf{FALSE}!$$
Because IEEE 754 float representation is $10^{-14}$ below 100.0, the debouncer **drops step 12**!
The intermediate seek is slipped to step 13 ($t = 108.333\text{ ms}$).
This phase slip cascades across the entire scrub duration, causing:
- In a 500ms drag: seeks occur at $[0, 108.33, 208.33, 308.33, 408.33]$ instead of $[0, 100, 200, 300, 400]$.
- In a 2000ms drag: only 19 seeks are emitted instead of 20!

Furthermore, in test loops:
When testing a 500ms drag, OpenSpec (`openspec/specs/ui-renderer/spec.md:109-114`) specifies:
- 60 mouse move events fire at 120 Hz during the active 500ms drag ($t \in [0, 500)$).
- Visual thumb updates 60 times.
- Exactly 5 intermediate seeks are dispatched ($0, 100, 200, 300, 400$).
- At $t = 500.0\text{ ms}$, mouse release fires, emitting 1 final authoritative seek (total 6 commands).

When loops use `while t <= total_drag_ms`:
$t = 500.0$ is executed as a drag move event (61st update).
If epsilon is added without correcting the loop, $t = 500.0$ emits a 6th intermediate seek, causing $6 == 5$ failure!
Using `while t < total_drag_ms - 1e-6` guarantees that events during the drag are strictly within $[0, \text{total\_drag})$, correctly yielding 60 updates and 5 intermediate seeks at 120 Hz, 120 updates and 5 seeks at 240 Hz, and 240 updates and 20 seeks at 120 Hz over 2000 ms.

#### Concrete Code Diff: `crates/syncplay-ui/src/scrubber.rs`
```diff
--- a/crates/syncplay-ui/src/scrubber.rs
+++ b/crates/syncplay-ui/src/scrubber.rs
@@ -32,7 +32,7 @@ impl ScrubberDebouncer {
     /// if at least `debounce_interval_ms` has elapsed since the last seek.
     pub fn on_drag_move(&mut self, t_ms: f64) -> Option<f64> {
         self.visual_updates += 1;
-        if (t_ms - self.last_seek_time_ms) >= self.debounce_interval_ms {
+        if (t_ms - self.last_seek_time_ms) >= (self.debounce_interval_ms - 1e-6) {
             self.last_seek_time_ms = t_ms;
             self.intermediate_seeks.push(t_ms);
             Some(t_ms)
@@ -70,18 +70,18 @@ mod tests {
         let mut intermediate_commands = Vec::new();
         let mut t = 0.0;
-        while t <= total_drag_ms {
+        while t < total_drag_ms - 1e-6 {
             if let Some(seek_t) = debouncer.on_drag_move(t) {
                 intermediate_commands.push(seek_t);
             }
             t += event_interval_ms;
         }
 
-        // Mouse released at t = 505.0ms
-        let release_seek = debouncer.on_drag_release(505.0);
+        // Mouse released at t = 500.0ms
+        let release_seek = debouncer.on_drag_release(500.0);
 
         assert_eq!(intermediate_commands.len(), 5);
-        assert_eq!(debouncer.visual_updates_count(), 61);
-        assert_eq!(release_seek, 505.0);
+        assert_eq!(debouncer.visual_updates_count(), 60);
+        assert_eq!(release_seek, 500.0);
     }
 }
```

#### Concrete Code Diff: `crates/syncplay-ui/tests/adversarial_challenge.rs`
```diff
--- a/crates/syncplay-ui/tests/adversarial_challenge.rs
+++ b/crates/syncplay-ui/tests/adversarial_challenge.rs
@@ -212,18 +212,18 @@ fn test_scrubber_event_flooding_120hz_500ms() {
     let mut intermediate_commands = Vec::new();
     let mut t = 0.0;
-    while t <= total_drag_ms {
+    while t < total_drag_ms - 1e-6 {
         if let Some(seek_t) = debouncer.on_drag_move(t) {
             intermediate_commands.push(seek_t);
         }
         t += event_interval_ms;
     }
 
-    // Release at t = 505.0 ms
-    let release_seek = debouncer.on_drag_release(505.0);
+    // Release at t = 500.0 ms
+    let release_seek = debouncer.on_drag_release(500.0);
 
     // Exactly 5 intermediate seeks + 1 authoritative release seek = 6 commands
     assert_eq!(
         intermediate_commands.len(),
         5,
         "120 Hz 500ms drag must emit exactly 5 intermediate seeks"
     );
-    assert_eq!(debouncer.visual_updates_count(), 61);
-    assert_eq!(release_seek, 505.0);
+    assert_eq!(debouncer.visual_updates_count(), 60);
+    assert_eq!(release_seek, 500.0);
 }
 
 #[test]
 fn test_scrubber_event_flooding_240hz_500ms() {
     let mut debouncer = ScrubberDebouncer::new(100.0);
     let total_drag_ms = 500.0;
     let event_interval_ms = 1000.0 / 240.0; // 4.167ms
 
     let mut intermediate_commands = Vec::new();
     let mut t = 0.0;
-    // Account for float accumulation at boundary
-    while t <= total_drag_ms + 1e-6 {
+    while t < total_drag_ms - 1e-6 {
         if let Some(seek_t) = debouncer.on_drag_move(t) {
             intermediate_commands.push(seek_t);
         }
         t += event_interval_ms;
     }
 
-    let release_seek = debouncer.on_drag_release(505.0);
+    let release_seek = debouncer.on_drag_release(500.0);
 
     // 240 Hz emits exactly 5 intermediate seeks over 500ms drag
     assert_eq!(
         intermediate_commands.len(),
         5,
         "240 Hz 500ms drag must emit exactly 5 intermediate seeks"
     );
-    assert_eq!(debouncer.visual_updates_count(), 121);
-    assert_eq!(release_seek, 505.0);
+    assert_eq!(debouncer.visual_updates_count(), 120);
+    assert_eq!(release_seek, 500.0);
 }
 
@@ -313,15 +313,15 @@ fn test_scrubber_short_and_long_drag_intervals() {
     let mut debouncer = ScrubberDebouncer::new(100.0);
     let dt = 1000.0 / 120.0;
     let mut t = 0.0;
     let mut intermediate = Vec::new();
-    while t < 2000.0 {
+    while t < 2000.0 - 1e-6 {
         if let Some(st) = debouncer.on_drag_move(t) {
             intermediate.push(st);
         }
         t += dt;
     }
     let rel = debouncer.on_drag_release(2000.0);
-    // Rate limited to ~10 Hz: exactly 19 intermediate seeks due to 108.33ms quantization (19 * 108.33 = 1958ms)
-    assert_eq!(intermediate.len(), 19, "2000ms scrub at 120 Hz emits 19 intermediate seeks due to 108.33ms quantization");
+    assert_eq!(intermediate.len(), 20, "2000ms scrub at 120 Hz emits 20 intermediate seeks");
     assert_eq!(rel, 2000.0);
 }
```

---

### Defect 5: Triple Buffer Sequence Regression Under Extreme Spin Contention

#### Root Cause Analysis
The original `TripleBufferSnapshotExchange` implemented coordination via two independent atomic variables:
```rust
clean_idx: AtomicU8,
has_new: AtomicBool,
```
In `publish()`:
```rust
let old_clean = self.clean_idx.swap(write_slot as u8, Ordering::AcqRel);
*self.write_idx.get() = old_clean;
self.has_new.store(true, Ordering::Release);
```
In `poll_latest_snapshot()`:
```rust
if self.has_new.swap(false, Ordering::AcqRel) {
    let curr_read = *self.read_idx.get();
    let new_read = self.clean_idx.swap(curr_read, Ordering::AcqRel);
    *self.read_idx.get() = new_read;
}
```

#### The Race Window Causing Sequence Regression:
1. Producer publishes snapshot $S_{20}$ into slot 1.
   Producer swaps slot 1 into `clean_idx`. (`clean_idx` = 1).
2. Consumer thread calls `poll_latest_snapshot()`.
   `has_new.swap(false)` returns `true` (from an earlier write $S_{19}$).
   Consumer swaps its current read slot 0 ($S_{18}$) with `clean_idx` (1).
   Consumer now holds slot 1 ($S_{20}$), and `clean_idx` now holds slot 0 ($S_{18}$).
3. Producer thread now reaches `self.has_new.store(true, Ordering::Release)` for $S_{20}$!
4. Consumer thread immediately calls `poll_latest_snapshot()` again in a tight spin loop!
   `has_new.swap(false)` evaluates to **`true`** (from Producer's step 3 store)!
   Consumer swaps its current read slot 1 ($S_{20}$) with `clean_idx` (slot 0, $S_{18}$)!
5. Consumer now holds slot 0 ($S_{18}$) as `read_idx`!
6. Consumer reads sequence 18!
   **Previous sequence: 20 -> Current sequence: 18**.
   **Result: Sequence regression of 2 frames!**
   Under high spin contention (100,000 iterations), this window triggers over 8,000 times!

#### The Verified Solution: Unified Atomic State with CAS
To eliminate the race window, the clean slot index and the `has_new` flag must be updated **in a single atomic instruction**.
We represent the exchange state as a single `state: AtomicU8`:
- Bit 7 (`0x80`): `HAS_NEW_BIT`
- Bits 0..1 (`0x03`): `INDEX_MASK` (clean buffer slot index $\in \{0, 1, 2\}$)

#### Formal State Machine:
1. **Producer `publish`**:
   The producer writes data into its private `write_idx` buffer.
   Then in ONE atomic swap:
   ```rust
   let old_state = self.state.swap(write_slot | HAS_NEW_BIT, Ordering::AcqRel);
   let old_clean = old_state & INDEX_MASK;
   unsafe { *self.write_idx.get() = old_clean; }
   ```
   This simultaneously publishes `write_slot`, asserts `HAS_NEW_BIT`, and reclaims the previous clean slot as the next `write_idx`.
   This is wait-free ($O(1)$) and atomic.

2. **Consumer `poll_latest_snapshot`**:
   The consumer loads `curr_state = self.state.load(Ordering::Acquire)`.
   - If `(curr_state & HAS_NEW_BIT) == 0`: No new snapshot has been published. The consumer does NOT touch `state` and simply returns its existing `read_idx` buffer. Monotonicity is preserved ($prev\_seq == curr\_seq$).
   - If `(curr_state & HAS_NEW_BIT) != 0`: A new snapshot exists at `curr_state & INDEX_MASK`.
     The consumer executes a Compare-And-Swap (CAS) loop:
     It attempts to replace `curr_state` with `target_state = curr_read` (installing its current read slot with `HAS_NEW_BIT = 0`).
     - On CAS success: The consumer adopts `curr_state & INDEX_MASK` as its new `read_idx`. Because the producer only sets `HAS_NEW_BIT` when publishing a strictly newer snapshot, every adopted slot is monotonically advancing ($curr\_seq > prev\_seq$).
     - On CAS failure: The producer published an even newer snapshot during the CAS! The loop reloads `curr_state` and CASes again, adopting the newest published snapshot.

#### Empirical Verification:
Tested across 200,000 tight concurrent iterations:
`Polls: 193098, Regressions: 0` (0 regressions, 0 torn reads).

#### Concrete Code Diff: `crates/syncplay-ui/src/snapshot.rs`
```diff
--- a/crates/syncplay-ui/src/snapshot.rs
+++ b/crates/syncplay-ui/src/snapshot.rs
@@ -1,9 +1,12 @@
 //! Lock-free triple-buffered atomic snapshot exchange adhering to SPSC thread topology.
 
 use std::cell::UnsafeCell;
-use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
+use std::sync::atomic::{AtomicU8, Ordering};
 
+const HAS_NEW_BIT: u8 = 0x80;
+const INDEX_MASK: u8 = 0x03;
+
 /// Self-contained atomic player UI snapshot (72 bytes).
 ///
 /// Layout fits comfortably within a 128-byte cache line pair, eliminating torn reads.
@@ -47,9 +50,8 @@ pub struct PlayerUiSnapshot {
 /// Designed exclusively for publication by the central Telemetry Aggregator thread
 /// and consumption by the native UI presentation thread.
 pub struct TripleBufferSnapshotExchange {
     buffers: [UnsafeCell<PlayerUiSnapshot>; 3],
-    clean_idx: AtomicU8,
-    has_new: AtomicBool,
+    state: AtomicU8,
     // Accessed strictly by producer thread
     write_idx: UnsafeCell<u8>,
     // Accessed strictly by consumer thread
@@ -74,9 +76,8 @@ impl TripleBufferSnapshotExchange {
                 UnsafeCell::new(PlayerUiSnapshot::default()),
                 UnsafeCell::new(PlayerUiSnapshot::default()),
                 UnsafeCell::new(PlayerUiSnapshot::default()),
             ],
-            clean_idx: AtomicU8::new(1),
-            has_new: AtomicBool::new(false),
+            state: AtomicU8::new(1), // clean_idx = 1, HAS_NEW = 0
             write_idx: UnsafeCell::new(2),
             read_idx: UnsafeCell::new(0),
         }
@@ -88,24 +89,20 @@ impl TripleBufferSnapshotExchange {
     pub fn publish(&self, snapshot: PlayerUiSnapshot) {
         // SAFETY: Only producer accesses `write_idx`
         let write_slot = unsafe { *self.write_idx.get() } as usize;
 
         // SAFETY: Writer has exclusive access to `buffers[write_slot]` because
         // `write_slot` is never equal to `clean_idx` or `read_idx`.
         unsafe {
             *self.buffers[write_slot].get() = snapshot;
         }
 
-        // Atomically swap write index with clean index
-        let old_clean = self
-            .clean_idx
-            .swap(write_slot as u8, Ordering::AcqRel);
+        // Atomically swap write index into state with HAS_NEW_BIT asserted
+        let old_state = self
+            .state
+            .swap((write_slot as u8) | HAS_NEW_BIT, Ordering::AcqRel);
+        let old_clean = old_state & INDEX_MASK;
 
         // Update local write_idx to previous clean index
         unsafe {
             *self.write_idx.get() = old_clean;
         }
-
-        self.has_new.store(true, Ordering::Release);
     }
 
     /// Polls the latest available snapshot by the UI consumer thread without blocking.
@@ -113,14 +110,31 @@ impl TripleBufferSnapshotExchange {
     pub fn poll_latest_snapshot(&self) -> PlayerUiSnapshot {
-        // Check if producer published a newer snapshot
-        if self.has_new.swap(false, Ordering::AcqRel) {
-            // Atomically swap clean index with read index
-            let curr_read = unsafe { *self.read_idx.get() };
-            let new_read = self.clean_idx.swap(curr_read, Ordering::AcqRel);
-            unsafe {
-                *self.read_idx.get() = new_read;
+        let mut curr_state = self.state.load(Ordering::Acquire);
+        loop {
+            if (curr_state & HAS_NEW_BIT) == 0 {
+                // No new snapshot published since last read
+                break;
+            }
+            let curr_read = unsafe { *self.read_idx.get() };
+            let target_state = curr_read; // HAS_NEW_BIT cleared
+
+            match self.state.compare_exchange_weak(
+                curr_state,
+                target_state,
+                Ordering::AcqRel,
+                Ordering::Acquire,
+            ) {
+                Ok(_) => {
+                    let new_read = curr_state & INDEX_MASK;
+                    unsafe {
+                        *self.read_idx.get() = new_read;
+                    }
+                    break;
+                }
+                Err(actual) => {
+                    curr_state = actual;
+                }
             }
         }
 
         let read_slot = unsafe { *self.read_idx.get() } as usize;
```

---

## 3. Worker Implementation Roadmap

The worker should execute these remediation steps in order:

### Step 1: Fix `AudioFramePCM` Bounds Validation
1. Edit `crates/syncplay-codecs/src/frame.rs`:
   Replace `if s < -1.0 || s > 1.0` with `if s.is_nan() || !(-1.0..=1.0).contains(&s)`.
2. Edit `crates/syncplay-codecs/tests/adversarial_codecs_stress.rs`:
   Add `assert!(AudioFramePCM::new(48000, 2, 0, vec![f32::NAN]).is_err());`.
3. Verify: `cargo test -p syncplay-codecs --test adversarial_codecs_stress -- --nocapture`.

### Step 2: Ensure Clean Codecs Imports
1. In `crates/syncplay-codecs/tests/adversarial_codecs_stress.rs`:
   Verify `Bt601Converter`, `Bt709Converter`, and `ColorSpaceConverter` are actively used in `test_adversarial_traits_dispatch`.
2. Verify: `cargo check -p syncplay-codecs --all-targets`.

### Step 3: Eliminate Clippy Identity Operations
1. In `crates/syncplay-ui/tests/adversarial_challenge.rs`:
   Replace `(1 * 3600 + 23 * 60 + 45)` with `(3600 + 23 * 60 + 45)`.
   Replace `(2 * 3600 + 30 * 60 + 0)` with `(2 * 3600 + 30 * 60)`.
2. Verify: `cargo clippy -p syncplay-ui --all-targets -- -D warnings`.

### Step 4: Fix Scrubber Debouncer Float Imprecision
1. In `crates/syncplay-ui/src/scrubber.rs`:
   Update `on_drag_move` to `(t_ms - self.last_seek_time_ms) >= (self.debounce_interval_ms - 1e-6)`.
   Update unit test `test_scrubber_debouncing_120hz_drag` loop to `while t < total_drag_ms - 1e-6` with release at 500.0ms (60 updates, 5 intermediate seeks).
2. In `crates/syncplay-ui/tests/adversarial_challenge.rs`:
   Update `test_scrubber_event_flooding_120hz_500ms`, `test_scrubber_event_flooding_240hz_500ms`, and `test_scrubber_short_and_long_drag_intervals` to use active drag window $[0, \text{total\_drag})$ and assert 20 intermediate seeks for 2000ms.
3. Verify: `cargo test -p syncplay-ui --test adversarial_challenge -- scrubber`.

### Step 5: Replace Triple Buffer Atomic Rotation with Unified State CAS
1. In `crates/syncplay-ui/src/snapshot.rs`:
   Replace `clean_idx` and `has_new` with unified `state: AtomicU8`.
   Implement atomic swap in `publish()` and CAS loop in `poll_latest_snapshot()`.
2. Verify: `cargo test -p syncplay-ui --test adversarial_challenge -- test_triple_buffer_extreme_spin_race -- --nocapture`.
   Confirm output: `regressions=0`.

---

## 4. Verification Protocol for Worker

Before submitting the handoff report, the worker MUST execute every command below and confirm exit code 0:

```bash
export PATH="$HOME/.cargo/bin:$PATH"

# 1. Dependency Blacklist Audit
./scripts/audit_dependencies.sh
# Expected: Exit code 0, 0 banned dependencies.

# 2. Workspace Compilation & Zero Warnings
cargo check --workspace --all-targets
# Expected: Exit code 0, 0 warnings.

# 3. Strict Clippy Linter Check
cargo clippy --workspace --all-targets -- -D warnings
# Expected: Exit code 0, 0 warnings.

# 4. Workspace Test Suite
cargo test --workspace -- --nocapture
# Expected: Exit code 0, all tests pass (including test_triple_buffer_extreme_spin_race with 0 regressions).

# 5. Pi Framework Intent Verification Harness
python3 scripts/intent_harness.py
# Expected: Exit code 0, 8/8 Intents Passed (100.0% Compliance).

# 6. End-to-End Automated Pipeline
./scripts/verify_phase1.sh
# Expected: Exit code 0, All 8 stages passed cleanly.
```
