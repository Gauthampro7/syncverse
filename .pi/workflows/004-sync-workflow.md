# Workflow: 004-sync-workflow

## 1. Metadata & OpenSpec Mapping
- **Workflow ID**: `004-sync-workflow`
- **Target Change Proposal**: `openspec/changes/004-network-sync-protocol.md`
- **Capability Specification**: `openspec/specs/sync-engine/spec.md`
- **Target Crates**: `syncverse-sync`
- **Primary Agents**: `spec_implementer`, `sync_simulator`
- **Required Skills**: `openspec-change-mapping`, `network-sync-simulation`, `zero-dependency-audit`

---

## 2. Prerequisites & Dependencies
- **Upstream Dependencies**: `003-clock-workflow` (Needs monotonic master clock & re-anchoring primitives).
- **Blocks**: `005-chat-workflow`, `006-ui-workflow`.
- **Pre-flight Checks**:
  1. Confirm `syncverse-clock` unit tests pass.
  2. Confirm `syncverse-common` IEEE 802.3 CRC32 is available.

---

## 3. Step-by-Step Execution Plan

### Step 1: Wire Packet Encoding & UDP Protocol (`syncverse-sync/src/wire/`)
1. Implement binary wire formats (`packets.rs`):
   - `WireProbePacket`: NTP 4-timestamp exchange payload ($T_1, T_2, T_3, T_4$).
   - `WireRoomStatePacket`: Reference timeline parameters ($\tau_{\text{ref\_media}}, t_{\text{ref\_server}}, r_{\text{target}}$).
   - `WireBarrierPacket`: Two-phase unpause handshake (`UNPAUSE_PREPARE`, `UNPAUSE_READY`, `UNPAUSE_COMMIT`, `UNPAUSE_ABORT`).
2. Add IEEE 802.3 CRC32 checksum validation to each wire message.

### Step 2: RFC 5905 NTP Filter (`syncverse-sync/src/ntp/`)
1. Implement probe timestamp offset and delay calculations (`probe.rs`):
   $$\theta = \frac{(T_2 - T_1) + (T_3 - T_4)}{2}, \quad \text{RTT} = (T_4 - T_1) - (T_3 - T_2)$$
2. Implement 8-sample sliding window minimum RTT filter (`filter.rs`):
   - Calculate window dispersion $D = \sum_{i=0}^{W-1} \frac{|\theta_i - \theta_0|}{2^{i+1}}$ with floor $D_{\text{effective}} = \max(D, 5.0\text{ ms})$.
   - Implement outlier rejection: reject samples with $\text{RTT} > 2.0 \times \text{median}(\text{RTT})$ or $|\theta - \hat{\theta}| > 3 D_{\text{effective}}$.
   - Implement watchdog recovery: flush filter buffer upon 8 consecutive rejections.

### Step 3: Reference Timeline Mapping
Implement the deterministic target presentation equation:
$$\tau_{\text{target}}(t_{\text{local}}) = \tau_{\text{ref\_media}} + r_{\text{target}} \cdot \left[ (t_{\text{local}} + \hat{\theta}) - t_{\text{ref\_server}} \right]$$

### Step 4: Discrete PI PLL Drift Controller & Tri-Zone Policy (`syncverse-sync/src/pll/`)
1. Implement discrete PI controller (`controller.rs`):
   - Sample rate: $\Delta t = 0.2\text{ s}$.
   - Gains: $K_p = 0.17\text{ s}^{-1}, \quad K_i = 0.0072\text{ s}^{-2}$.
   - Anti-windup clamping: $|I[k]| \le 32.0\text{ s}$.
   - Speed ratio clamping: $r[k] \in [0.96, 1.04]$.
2. Implement Tri-Zone drift policy (`tri_zone.rs`):
   - **Zone 1** (Deadband, $|e| < 15\text{ ms}$): $r = 1.0\times$.
   - **Zone 2** (Continuous Micro-Speed, $15\text{ ms} \le |e| \le 1500\text{ ms}$): discrete PI slew.
   - **Zone 3** (Hard Seek, $|e| > 1500\text{ ms}$): trigger hard seek to target PTS.
3. Verify critical damping: $e(0) = +450\text{ ms}$ converges to $< 15\text{ ms}$ within $20\text{ s}$ ($t \approx 12.0\text{ s}$) without overshooting past $-15\text{ ms}$.

### Step 5: Two-Phase Barrier Unpause Coordinator (`syncverse-sync/src/barrier/`)
1. Implement server target schedule generation (`unpause.rs`):
   $$T_{\text{target\_server}} = t_{\text{server\_now}} + 1.5 \times \max_i(\text{RTT}_i) + t_{\text{preroll\_budget}} + 100\text{ ms}$$
   with $t_{\text{preroll\_budget}} = 200\text{ ms}$.
2. Implement client prepare/ready/commit unpause state machine.
3. Verify that commit arrives with $\ge 100\text{ ms}$ positive margin across all pre-roll intervals and peer start times align within $< 10\text{ ms}$.

---

## 4. Verification & Acceptance Criteria
Execute the automated test suite:
```bash
# Crate unit test verification
cargo test -p syncverse-sync -- --nocapture

# Adversarial network sync simulation test
python3 tests/empirical_challenge/test_sync_engine.py

# Lint inspection
cargo clippy -p syncverse-sync --all-targets -- -D warnings
```
Criteria for success:
- 100% test pass rate with 0 errors.
- Verified PI convergence into $< 15\text{ ms}$ deadband at $t \approx 12.0\text{ s}$ with zero overshoot past $-15\text{ ms}$.
- Verified $\ge 100\text{ ms}$ commit margin and $< 10\text{ ms}$ unpause alignment.
- 0 compiler warnings.
