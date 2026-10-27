# Change Proposal: 004-network-sync-protocol

**Change ID**: `004-network-sync-protocol`  
**Title**: High-RTT Network Synchronization Protocol & Discrete PI PLL Controller  
**Status**: Ready for Implementation  
**Target Milestone**: Phase 1 Foundational Primitives  

---

## 1. Context & Motivation

Long-distance watch parties across transatlantic or transpacific links (250–400ms RTT, $\pm 30\text{ms}$ jitter) fail when using naive playback sync. Hard seeks cause stutter, and arithmetic averaging overestimates delay due to router queueing. This change introduces an NTP-style sliding-window minimum RTT filter and a critically damped discrete PI PLL controller steering playback within an imperceptible $\pm 4\%$ micro-speed envelope.

---

## 2. Dependency Graph

```
[003-clock-pipeline]
         │
         ▼
[004-network-sync-protocol] (syncverse-sync)
         │
         ├─► [005-smart-chat-storage] (chat slash commands dispatch to sync)
         └─► [006-ui-osd-renderer] (sync badges & telemetry display)
```

- **Dependencies**: `003-clock-pipeline`.
- **Blocks**: `005-smart-chat-storage`, `006-ui-osd-renderer`.

---

## 3. Affected Crates & Components

- **`crates/syncverse-sync`**:
  - `src/ntp/probe.rs`: 4-timestamp probe packet encoding/decoding and offset math.
  - `src/ntp/filter.rs`: RFC 5905 8-sample sliding-window minimum RTT filter with dispersion rejection, $D_{\text{effective}}$ floor, and watchdog reset.
  - `src/pll/controller.rs`: Discrete Proportional-Integral (PI) drift controller ($K_p=0.17\text{ s}^{-1}, K_i=0.0072\text{ s}^{-2}$).
  - `src/pll/tri_zone.rs`: Tri-zone drift policy (Deadband $< 15\text{ms}$, Micro-speed $[15, 1500]\text{ms}$, Hard seek $> 1500\text{ms}$).
  - `src/barrier/unpause.rs`: Two-phase barrier-synchronized unpause coordinator with timeout abort recovery.
  - `src/wire/packets.rs`: Binary wire representations (`WireProbePacket`, `WireRoomStatePacket`, `WireBarrierPacket`).

---

## 4. Deliverables & Implementation Plan

1. **NTP Probe & Filter**:
   - Offset and delay formulas:
     $$\theta = \frac{(T_2 - T_1) + (T_3 - T_4)}{2}, \quad \text{RTT} = (T_4 - T_1) - (T_3 - T_2)$$
   - Minimum RTT sample selection over $W = 8$ window.
   - Outlier rejection: $s_{\text{new}}.\text{rtt} > 2.0 \times \text{median}(\text{rtt}) \lor |s_{\text{new}}.\text{offset} - \hat{\theta}| > 3D_{\text{effective}}$ where $D_{\text{effective}} = \max(D, 5.0\text{ ms})$, with 8-rejection watchdog buffer flush.
2. **Deterministic Reference Timeline**:
   $$\tau_{\text{target}}(t_{\text{local}}) = \tau_{\text{ref\_media}} + r_{\text{target}} \cdot \left[ (t_{\text{local}} + \hat{\theta}) - t_{\text{ref\_server}} \right]$$
3. **Discrete PI PLL Drift Steering**:
   - Error evaluation: $e[k] = \tau_{\text{target}}[k] - \tau_{\text{actual}}[k]$.
   - Control law: $\Delta r[k] = K_p \cdot e[k] + K_i \cdot I[k]$ with anti-windup clamping to $I_{\text{max}} = 32.0\text{ s}$, tuned to $K_p = 0.17\text{ s}^{-1}$ and $K_i = 0.0072\text{ s}^{-2}$ for critical damping.
   - Speed clamping: $r[k] \in [0.96, 1.04]$.
4. **Two-Phase Barrier Unpause**:
   - Target schedule: $T_{\text{target\_server}} = t_{\text{server\_now}} + 1.5 \times \max_i(\text{RTT}_i) + t_{\text{preroll\_budget}} + 100\text{ ms}$ ($t_{\text{preroll\_budget}} = 200\text{ ms}$).
   - `UNPAUSE_PREPARE` $\to$ Pre-roll decode & prime audio $\to$ `UNPAUSE_READY` $\to$ `UNPAUSE_COMMIT`, with timeout abort recovery.
   - Start alignment across peers within $< 10\text{ ms}$.

---

## 5. Verification Commands

Execute the following commands from the repository root:

```bash
# 1. Type and borrow check affected crate
cargo check -p syncverse-sync

# 2. Run unit tests including NTP filter and PLL mathematical proofs
cargo test -p syncverse-sync -- --nocapture

# 3. Verify zero external networking or codec dependencies
cargo tree -p syncverse-sync --edges no-dev
```

Expected output: All unit tests pass; mathematical convergence tests verify phase error reduces to $< 15\text{ ms}$ under 250–400ms simulated RTT without issuing hard seeks.
