# Change Proposal: 003-clock-pipeline

**Change ID**: `003-clock-pipeline`  
**Title**: Monotonic Presentation Clock & Cache-Aligned Lock-Free SPSC Ring Buffer  
**Status**: Ready for Implementation  
**Target Milestone**: Phase 1 Foundational Primitives  

---

## 1. Context & Motivation

To maintain smooth 60 FPS video presentation and avoid audio buffer underruns, media threads must communicate without mutex locks or condition variable contention. Furthermore, the master presentation clock must advance monotonically without discontinuities when the sync engine commands micro-speed PLL rate adjustments.

---

## 2. Dependency Graph

```
[001-demuxer-bitstream] ──┐
                          ├─► [003-clock-pipeline] (syncverse-clock)
[002-codecs-presentation] ┘          │
                                     ├─► [004-network-sync-protocol]
                                     └─► [006-ui-osd-renderer]
```

- **Dependencies**: `001-demuxer-bitstream`, `002-codecs-presentation`.
- **Blocks**: `004-network-sync-protocol`, `006-ui-osd-renderer`.

---

## 3. Affected Crates & Components

- **`crates/syncverse-clock`**:
  - `src/clock/master.rs`: Monotonic clock time base, PTS tracking, continuous re-anchoring.
  - `src/clock/scheduler.rs`: Presentation decision window (early wait, on-time present, late drop).
  - `src/spsc/ring_buffer.rs`: 128-byte cache-aligned SPSC lock-free ring buffer.
  - `src/spsc/cache_pad.rs`: `#[repr(align(128))]` atomic wrappers to eliminate false sharing.

---

## 4. Deliverables & Implementation Plan

1. **Master Monotonic Clock**:
   - Monotonic time tracking backed by `std::time::Instant` (`CLOCK_MONOTONIC`).
   - Atomic state snapshot publication on rate shifts using a lock-free `SeqLock` or `AtomicU128`:
     $$\text{PublishSnapshot}(\text{anchor\_pts}' \leftarrow \text{CurrentPTS}(t_{\text{now}}), \quad \text{anchor\_wall}' \leftarrow t_{\text{now}}, \quad \text{speed} \leftarrow s_{\text{new}})$$
     guaranteeing disjoint multi-field atomic read race conditions are mathematically eliminated:
     $$\lim_{\epsilon \to 0} |\text{CurrentPTS}(t_{\text{now}} + \epsilon) - \text{CurrentPTS}(t_{\text{now}} - \epsilon)| = 0$$
2. **Presentation Scheduler & Reorder Buffer**:
   - Decision thresholds:
     - $\Delta > +10\text{ ms}$: Early (sleep/yield).
     - $-15\text{ ms} \le \Delta \le +10\text{ ms}$: On-time (present immediately).
     - $-100\text{ ms} < \Delta < -15\text{ ms}$: Slightly late (render immediately, metric logged).
     - $\Delta \le -100\text{ ms}$: Severely late (drop frame).
   - Bounded 16-frame presentation reorder buffer resolving DTS to PTS frame order.
3. **Lock-Free SPSC Ring Buffer**:
   - Explicit method signatures: `try_push(&self, item: T) -> Result<(), BufferFullError<T>>` and `try_pop(&self) -> Option<T>`.
   - Power-of-two capacity with bitwise index masking (`index & (CAPACITY - 1)`).
   - Unbounded monotonic `AtomicUsize` counters for `head` and `tail` with full/empty detection.
   - Producer `head` and consumer `tail` placed on separate 128-byte cache lines.
   - `Acquire`/`Release` atomic memory ordering semantics (no `SeqCst`).

---

## 5. Verification Commands

Execute the following commands from the repository root:

```bash
# 1. Type and borrow check affected crate
cargo check -p syncverse-clock

# 2. Run unit and multi-threaded stress tests
cargo test -p syncverse-clock -- --nocapture

# 3. Verify zero banned dependencies
cargo tree -p syncverse-clock --edges no-dev
```

Expected output: SPSC multi-threaded stress test passing 1,000,000 items across threads without deadlocks or data loss; clock continuity verified under rate adjustments.
