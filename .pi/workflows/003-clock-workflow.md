# Workflow: 003-clock-workflow

## 1. Metadata & OpenSpec Mapping
- **Workflow ID**: `003-clock-workflow`
- **Target Change Proposal**: `openspec/changes/003-clock-pipeline.md`
- **Capability Specification**: `openspec/specs/av-pipeline/spec.md`
- **Target Crates**: `syncverse-clock`
- **Primary Agents**: `spec_implementer`
- **Required Skills**: `openspec-change-mapping`, `zero-dependency-audit`

---

## 2. Prerequisites & Dependencies
- **Upstream Dependencies**: `001-demuxer-workflow`, `002-codecs-workflow`.
- **Blocks**: `004-sync-workflow`, `006-ui-workflow`.
- **Pre-flight Checks**:
  1. Confirm video frame models in `syncverse-codecs` are stable and compiling.
  2. Confirm `syncverse-common` time types are available.

---

## 3. Step-by-Step Execution Plan

### Step 1: Master Monotonic Presentation Clock (`syncverse-clock/src/clock/master.rs`)
1. Implement presentation clock time base backed by `std::time::Instant` (`CLOCK_MONOTONIC`).
2. Implement atomic state snapshot re-anchoring on rate changes using a lock-free `SeqLock` or `AtomicU128`:
   $$\text{PublishSnapshot}(\text{anchor\_pts}' \leftarrow \text{CurrentPTS}(t_{\text{now}}), \quad \text{anchor\_wall}' \leftarrow t_{\text{now}}, \quad \text{speed} \leftarrow s_{\text{new}})$$
3. Verify mathematical continuity across rate shifts:
   $$\lim_{\epsilon \to 0} |\text{CurrentPTS}(t_{\text{now}} + \epsilon) - \text{CurrentPTS}(t_{\text{now}} - \epsilon)| = 0$$
   ensuring disjoint multi-field read torn snapshots and phantom timeline jumps are eliminated.

### Step 2: Presentation Scheduler & Reorder Buffer (`syncverse-clock/src/clock/scheduler.rs`)
1. Implement presentation decision thresholds for frame timing offset $\Delta = PTS - PTS_{\text{clock}}$:
   - $\Delta > +10\text{ ms}$: Early (sleep / yield).
   - $-15\text{ ms} \le \Delta \le +10\text{ ms}$: On-time (present immediately).
   - $-100\text{ ms} < \Delta < -15\text{ ms}$: Slightly late (render immediately, metric logged).
   - $\Delta \le -100\text{ ms}$: Severely late (drop frame).
2. Implement bounded 16-frame presentation reorder buffer resolving DTS to PTS frame ordering.

### Step 3: Cache-Aligned Lock-Free SPSC Ring Buffer (`syncverse-clock/src/spsc/`)
1. Implement `#[repr(align(128))]` cache padding wrappers (`cache_pad.rs`) isolating producer `head` and consumer `tail` on separate CPU cache lines to prevent false sharing.
2. Implement power-of-two circular ring buffer (`ring_buffer.rs`):
   - Unbounded monotonic `AtomicUsize` counters.
   - Bitwise masking: `index & (CAPACITY - 1)`.
   - Explicit signatures: `try_push(&self, item: T) -> Result<(), BufferFullError<T>>` and `try_pop(&self) -> Option<T>`.
3. Add tests verifying wrapping counter arithmetic and multi-threaded SPSC stress.

---

## 4. Verification & Acceptance Criteria
Execute the automated test suite:
```bash
# Crate unit test verification
cargo test -p syncverse-clock -- --nocapture

# Concurrency & SeqLock empirical test
python3 tests/empirical_challenge/test_demux_codecs_clock.py

# Lint inspection
cargo clippy -p syncverse-clock --all-targets -- -D warnings
```
Criteria for success:
- 100% test pass rate with 0 errors.
- 0 torn snapshot reads observed during rapid rate shifts.
- Zero data loss or corruption in concurrent SPSC ring buffer stress test (50,000+ items).
- 0 compiler warnings.
