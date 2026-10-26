---
name: network-sync-simulation
description: Operational skill for executing deterministic clock-synchronization simulations, validating discrete PI PLL convergence and barrier unpause alignment
version: 1.0.0
---

# Skill: Network Sync Simulation

## Overview
This skill defines the procedures for building and running deterministic, in-memory simulations of the network synchronization protocol (`syncplay-sync`). It allows agents to verify the accuracy of the NTP clock filter, the stability and convergence of the discrete PI PLL speed controller, and the timing margins of the two-phase barrier unpause coordinator without live sockets or external network infrastructure.

---

## 1. Simulation Architecture

The simulation environment runs as a discrete-event loop:
1. **Time Base**: Monotonic simulation clock stepping in discrete intervals ($\Delta t = 200\text{ ms}$).
2. **Channel Model**:
   - Base RTT: Uniformly distributed between $250\text{ ms}$ and $400\text{ ms}$.
   - Jitter: Asymmetric delays added to uplink and downlink ($\pm 30\text{ ms}$).
   - Route Reroute: Instantaneous step changes in RTT (e.g., $+100\text{ ms}$) to test watchdog recovery.
3. **Deterministic Seed**: Use deterministic PRNG sequences to ensure bit-exact reproducibility.

---

## 2. Core Simulation Procedures

### 2.1 NTP Probe & Outlier Filter Verification
1. **Initialize Filter**: Create an `Rfc5905Filter` with window size $W = 8$ and dispersion floor $D_{\text{min}} = 5.0\text{ ms}$.
2. **Feed Probes**: Generate probe exchanges with timestamps $T_1, T_2, T_3, T_4$:
   $$\theta = \frac{(T_2 - T_1) + (T_3 - T_4)}{2}, \quad \text{RTT} = (T_4 - T_1) - (T_3 - T_2)$$
3. **Outlier Filtering Test**:
   - Inject an asymmetric delay spike (e.g., downlink $+150\text{ ms}$).
   - Verify that the filter detects $\text{RTT} > 2.0 \times \text{median}(\text{RTT})$ or $|\theta - \hat{\theta}| > 3 D_{\text{effective}}$ and rejects the probe.
4. **Legitimate Step & Watchdog Test**:
   - Inject a sustained $+10\text{ ms}$ offset step with normal RTT. Confirm accepted.
   - Inject a sustained route change (RTT jumps to $350\text{ ms}$).
   - Verify that after 8 consecutive rejections, the watchdog flushes the buffer and adopts the new route without manual intervention.

### 2.2 Discrete PI PLL Controller Convergence Test
Simulate media timeline tracking under micro-speed steering:
1. **Configuration**:
   - Sample rate: $\Delta t = 0.2\text{ s}$ ($5\text{ Hz}$).
   - Controller gains: $K_p = 0.17\text{ s}^{-1}, \quad K_i = 0.0072\text{ s}^{-2}$.
   - Anti-windup limit: $I_{\text{max}} = 32.0\text{ s}$.
   - Initial condition: Local clock is $450\text{ ms}$ behind target ($e(0) = +450\text{ ms}$).
2. **Execution**:
   - At each step $k$:
     * Calculate error $e[k] = \tau_{\text{target}}[k] - \tau_{\text{actual}}[k]$.
     * If $|e[k]| < 15\text{ ms}$, enter Zone 1 deadband: $r = 1.0$, freeze $I$.
     * If $15\text{ ms} \le |e[k]| \le 1500\text{ ms}$, enter Zone 2:
       $$I[k] = \text{clamp}(I[k-1] + e[k] \cdot \Delta t, -I_{\text{max}}, I_{\text{max}})$$
       $$\Delta r[k] = K_p \cdot e[k] + K_i \cdot I[k]$$
       $$r[k] = \text{clamp}(1.0 + \Delta r[k], 0.96, 1.04)$$
     * Advance $\tau_{\text{actual}}$ by $r[k] \cdot \Delta t$.
3. **Assertions**:
   - Phase error MUST enter the $< 15\text{ ms}$ deadband within $20\text{ s}$ (target: $t \approx 12.0\text{ s}$).
   - Phase error MUST NOT overshoot past $-15\text{ ms}$ (critically damped, no oscillation).

### 2.3 Barrier Unpause Timing Margin Test
Verify the two-phase commit unpause sequence across simulated peers:
1. **Server Calculation**:
   $$T_{\text{target\_server}} = t_{\text{server\_now}} + 1.5 \times \max_i(\text{RTT}_i) + t_{\text{preroll\_budget}} + 100\text{ ms}$$
   with $t_{\text{preroll\_budget}} = 200\text{ ms}$.
2. **Evaluate across pre-roll durations**:
   - Test pre-roll times: $0\text{ ms}, 50\text{ ms}, 100\text{ ms}, 150\text{ ms}, 200\text{ ms}$.
3. **Assertions**:
   - Every peer receives the `UNPAUSE_COMMIT` packet with a positive arrival margin of at least $100\text{ ms}$ before $T_{\text{target\_server}}$.
   - Peer playback start timestamps align within $< 10\text{ ms}$.

---

## 3. Verification Commands

Run the sync simulation harness:
```bash
cargo test -p syncplay-sync -- --nocapture
python3 tests/empirical_challenge/test_sync_engine.py
```
Assert that all convergence, stability, and margin tests pass with 0 errors.
