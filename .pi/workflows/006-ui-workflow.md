# Workflow: 006-ui-workflow

## 1. Metadata & OpenSpec Mapping
- **Workflow ID**: `006-ui-workflow`
- **Target Change Proposal**: `openspec/changes/006-ui-osd-renderer.md`
- **Capability Specification**: `openspec/specs/ui-renderer/spec.md`
- **Target Crates**: `syncplay-ui`, `syncplay-core`
- **Primary Agents**: `spec_implementer`
- **Required Skills**: `openspec-change-mapping`, `zero-dependency-audit`

---

## 2. Prerequisites & Dependencies
- **Upstream Dependencies**: `003-clock-workflow`, `004-sync-workflow`, `005-chat-workflow`.
- **Blocks**: None (Terminal crate implementation phase).
- **Pre-flight Checks**:
  1. Confirm clock, sync, and chat crates compile and pass tests.
  2. Confirm SPSC ring buffer primitives are available.

---

## 3. Step-by-Step Execution Plan

### Step 1: Triple-Buffered Atomic Snapshot Exchange (`syncplay-ui/src/snapshot/`)
1. Implement `PlayerUiSnapshot` (`model.rs`):
   - Encapsulate monotonic timestamp, PTS position, duration, playback state, RTT, clock offset, phase error, PLL speed ratio (Q16), peer counts, and room sequence.
   - Constrain memory footprint to 72 bytes (fitting within a single 128-byte cache line pair).
2. Enforce SPSC publication topology:
   - A single centralized `TelemetryAggregator` in `syncplay-core` collects metrics from decode, clock, and sync loops and publishes snapshots to the triple buffer.
   - The UI rendering thread reads via atomic pointer swap with zero locking and zero partial/torn reads.

### Step 2: Lock-Free UI Command Queue (`syncplay-ui/src/command/`)
Implement lock-free SPSC queue (`queue.rs`) passing user actions (`TogglePause`, `SeekRelative`, `SendChatMessage`) from UI thread to the core coordinator.

### Step 3: OSD Overlay Architecture (`syncplay-ui/src/osd/`)
1. Implement Monospace Timecode HUD (`hud.rs`).
2. Implement Sync Health Badge (`sync_badge.rs`):
   - Green ($|e| < 15\text{ ms}$): In-sync deadband.
   - Amber ($15\text{ ms} \le |e| \le 1500\text{ ms}$): Active PLL micro-speed slew.
   - Red ($|e| > 1500\text{ ms}$): Out of sync / hard seek.
3. Implement Floating Chat Overlay (`chat_overlay.rs`):
   - Cap visible messages to 5 active bubbles (4.0s dwell, 1.0s exponential fade) to prevent unbounded memory growth during high-traffic chat bursts.
4. Implement Scrub-Bar Debouncer (`scrub_bar.rs`):
   - 10 Hz rate limiter on drag events.
   - For a 500ms continuous drag at 120 Hz, emit exactly 5 throttled intermediate seek commands.
   - On mouse release, immediately emit the 6th final authoritative seek command (total 6 commands).

### Step 4: Alpha8 Font Atlas & Memory Bounding
1. Model font texture atlas using single-channel Alpha8 (`R8_UNORM`) format:
   - $2048 \times 2048 \times 1\text{ byte} = 4.0\text{ MB}$ texture footprint (saving 12 MB compared to RGBA8).
2. Verify total UI memory consumption (Atlas 4MB + Framebuffer 8.5MB + State 1MB) strictly conforms to $\le 15.0\text{ MB}$.

### Step 5: Core Application Coordinator (`syncplay-core/src/coordinator.rs`)
Implement the top-level application coordinator binding all subsystem threads and driving the Telemetry Aggregator.

---

## 4. Verification & Acceptance Criteria
Execute the automated test suite:
```bash
# Crate unit test verification
cargo test -p syncplay-ui -- --nocapture
cargo test -p syncplay-core -- --nocapture

# Adversarial UI renderer empirical test
python3 tests/empirical_challenge/test_ui_renderer.py

# Lint inspection
cargo clippy -p syncplay-ui --all-targets -- -D warnings
cargo clippy -p syncplay-core --all-targets -- -D warnings
```
Criteria for success:
- 100% test pass rate with 0 errors.
- Verified 0 torn reads across high-frequency snapshot swaps.
- Verified exactly 6 seek commands emitted during 500ms drag + release.
- Total UI memory budget verified $\le 15.0\text{ MB}$.
- 0 compiler warnings.
