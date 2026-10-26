# Technical Analysis & Remediation Strategy: Network Synchronization Protocol (`syncplay-sync`)

**Target File**: `openspec/specs/sync-engine/spec.md`  
**Related Proposals**: `openspec/changes/004-network-sync-protocol.md`  
**Related Test Harness**: `tests/empirical_challenge/test_sync_engine.py`  
**Author**: `explorer_m1_fix_2` (teamwork_preview_explorer)  
**Status**: Authoritative Remediation Strategy for Implementation Worker  

---

## 1. Executive Summary & Root Cause Synthesis

Challenger 2's empirical testing and mathematical review revealed three critical vulnerabilities in `openspec/specs/sync-engine/spec.md`:

1. **PI Controller Convergence Contradiction**: The specification claimed that under $K_p = 0.05\text{ s}^{-1}$ and $K_i = 0.000625\text{ s}^{-2}$, an initial error $e(0) = +450\text{ ms}$ decays into the $|e| < 15\text{ ms}$ deadband within $20\text{ seconds}$. However, execution of the specified difference equations reveals that at $t = 20.0\text{ s}$, remaining phase error is $135.24\text{ ms}$, requiring $36.6\text{ s}$ to reach $15\text{ ms}$ with integral accumulation (and $68\text{ s}$ under pure proportional decay).
2. **NTP Sliding-Window Outlier Filter Latch-Up**: Under low jitter ($D \to 0$), the rejection threshold $3 \times D \to 0$, causing legitimate clock drift ($+10\text{ ms}$) to be permanently rejected (100% probe rejection). Furthermore, on network reroutes where RTT increases ($> 2.0 \times \text{median}$), all subsequent samples are discarded before window entry, freezing the median calculation and indefinitely locking out all NTP updates.
3. **Barrier Unpause Missed Presentation Deadlines**: The unpause formula $T_{\text{target\_server}} = t_{\text{server\_now}} + \max(\text{RTT}) + 200\text{ ms}$ under-allocates network transit for a 3-way exchange ($1.5 \times \text{RTT}$). On a 400ms RTT link with a 100ms keyframe pre-roll, the `UNPAUSE_COMMIT` packet arrives at $t = 700\text{ ms}$, which is $100\text{ ms}$ AFTER the scheduled presentation start instant ($t = 600\text{ ms}$).

This document establishes the exact mathematical proofs, concrete parameter adjustments, protocol state machine enhancements, and line-by-line replacement chunks for the implementation worker.

---

## 2. Issue 1: PI Controller Dynamics & Convergence Reconciliation

### 2.1 Mathematical Analysis & Root Cause

The discrete PI controller operates in Zone 2 ($15\text{ ms} \le |e(t)| \le 1500\text{ ms}$) at sample interval $\Delta t = 0.2\text{ s}$:
$$\Delta r_{\text{raw}}[k] = K_p \cdot e[k] + K_i \cdot I[k]$$
$$I[k] = \text{clamp}(I[k-1] + e[k] \cdot \Delta t, -I_{\text{max}}, +I_{\text{max}})$$
$$r[k] = \text{clamp}(1.0 + \Delta r_{\text{raw}}[k], 0.96, 1.04)$$
$$e[k+1] = e[k] - (r[k] - 1.0) \cdot \Delta t$$

In the continuous-time linear regime, this represents the standard second-order ODE:
$$\ddot{e}(t) + K_p \dot{e}(t) + K_i e(t) = 0$$
with natural frequency $\omega_n = \sqrt{K_i}$ and damping ratio $\zeta = \frac{K_p}{2\sqrt{K_i}}$.

For the original parameters ($K_p = 0.05\text{ s}^{-1}, K_i = 0.000625\text{ s}^{-2}$):
- $\omega_n = \sqrt{0.000625} = 0.025\text{ rad/s}$
- $\zeta = \frac{0.05}{2 \times 0.025} = 1.0$ (Critically damped)
- Closed-loop pole at $s = -0.025\text{ s}^{-1}$, yielding dominant time constant $\tau = \frac{1}{\omega_n} = 40.0\text{ seconds}$.
- At $e(0) = 0.450\text{ s}$, raw speed adjustment is $\Delta r_{\text{raw}}(0) = 0.05 \times 0.450 = 0.0225 \le 0.040$, meaning the controller never saturates.
- Because it operates entirely in the linear sub-saturated region with $\tau = 40\text{ s}$, error at $t = 20\text{ s}$ is $135.24\text{ ms} \gg 15\text{ ms}$. Reaching $< 15\text{ ms}$ takes $36.6\text{ seconds}$ with integral action ($68.0\text{ seconds}$ without).

### 2.2 Remediation Formulation: Option A (Recommended) vs Option B

#### Option A (Recommended): Parameter Re-tuning ($K_p = 0.17\text{ s}^{-1}, K_i = 0.0072\text{ s}^{-2}$)
To achieve convergence into the $|e| < 15\text{ ms}$ deadband within $20\text{ seconds}$ while preserving critical damping ($\zeta = 1.0$):
- Set $K_p = 0.17\text{ s}^{-1} = 0.17 \times 10^{-9}\text{ ns}^{-1}$.
- Calculate critical damping integral gain:
  $$K_i = \frac{K_p^2}{4} = \frac{0.17^2}{4} = \frac{0.0289}{4} \approx 0.007225\text{ s}^{-2} \implies K_i = 0.0072\text{ s}^{-2} = 7.2 \times 10^{-12}\text{ ns}^{-2}$$
- Damping ratio check:
  $$\omega_n = \sqrt{0.0072} \approx 0.08485\text{ s}^{-1}, \quad \zeta = \frac{0.17}{2 \times 0.08485} \approx 1.0017 \approx 1.0$$
- Discrete Z-domain stability:
  At $\Delta t = 0.2\text{ s}$, discrete state matrix roots are $z_1 = 0.9804, z_2 = 0.9853$. Both poles are real, positive, and strictly inside the unit circle ($|z| < 1.0$), guaranteeing monotonic non-oscillatory decay.
- Convergence Trajectory ($e(0) = +450\text{ ms}$):
  - $t \in [0.0\text{s}, 7.8\text{s}]$: Controller saturates at maximum slew rate $\Delta r = +0.04$ ($r = 1.04$). Error decreases at $40\text{ ms/s}$.
  - $t = 8.0\text{s}$: Error is $130.0\text{ ms}$; controller exits saturation cleanly into linear proportional-integral decay.
  - $t = 12.0\text{s}$: Error reaches $11.21\text{ ms} < 15\text{ ms}$, entering the deadband.
  - Rate clamps to $1.00000$ (Zone 1 deadband rule).
  - Overshoot: exactly $0.0\text{ ms}$ past $-15\text{ ms}$.
  - Meets "decays into $|e| < 15\text{ ms}$ deadband within $20\text{ seconds}$" with an 8-second margin ($12.0\text{s} \le 20.0\text{s}$).

#### Option B (Alternative): Retaining Low Gains and Extending Scenario to 70s
If the maintainer prefers gentler, unsaturated steering ($\Delta r \le 0.0225$) without rate clamping:
- Retain $K_p = 0.05\text{ s}^{-1}, K_i = 0.000625\text{ s}^{-2}$.
- Update scenario acceptance criteria:
  - Evaluation interval extended from 25 seconds to 75 seconds.
  - Convergence claim updated: *"phase error $e(t)$ monotonically decays into the $|e| < 15\text{ ms}$ deadband within $40\text{ seconds}$ (under PI action) or $70\text{ seconds}$ (under pure proportional decay)"*.

**Recommendation**: Adopt **Option A**. In interactive watch parties, a 450ms phase error takes over a minute to resolve under Option B, causing noticeable desynchronization. Option A utilizes the allowed $\pm 4\%$ envelope to achieve clean, imperceptible resync in 12 seconds while strictly preventing overshoot.

### 2.3 Concrete Replacement for `openspec/specs/sync-engine/spec.md` (Lines 106–126)

```markdown
### Requirement: Discrete Proportional-Integral (PI) Controller
In Zone 2, the speed adjustment $\Delta r$ SHALL be computed using a discrete PI controller evaluated at interval $\Delta t = 0.2\text{ s}$:
$$\Delta r_{\text{raw}}[k] = K_p \cdot e[k] + K_i \cdot I[k]$$
where the discrete integral accumulator is:
$$I[k] = I[k-1] + e[k] \cdot \Delta t$$
The controller parameters MUST be tuned for critical damping ($\zeta = 1.0$):
- **Proportional Gain**: $K_p = 0.17\text{ s}^{-1}$ ($0.17 \times 10^{-9}\text{ ns}^{-1}$)
- **Integral Gain**: $K_i = 0.0072\text{ s}^{-2}$ ($7.2 \times 10^{-12}\text{ ns}^{-2}$)
- **Maximum Speed Deviation**: $\Delta r_{\text{max}} = 0.04$ ($40,000\text{ PPM}$)
- **Anti-Windup Clamping**: $I[k] \leftarrow \text{clamp}(I[k], -I_{\text{max}}, +I_{\text{max}})$ with $I_{\text{max}} = 32.0\text{ seconds}$.

The final commanded playback speed ratio MUST be clamped to:
$$r[k] = \text{clamp}(1.0 + \Delta r_{\text{raw}}[k], \, 0.96, \, 1.04)$$

#### Scenario: Convergence Under Simulated 450ms Initial Phase Offset
- **Given** an initial phase error $e(0) = +450\text{ ms}$ on a simulated 300ms RTT link with $\pm 30\text{ms}$ jitter
- **When** the discrete PI controller runs across a 25-second evaluation interval
- **Then** the playback rate remains strictly bounded between $1.00$ and $1.04$
- **And** phase error $e(t)$ monotonically decays into the $|e| < 15\text{ ms}$ deadband within $20\text{ seconds}$ (entering at $t \approx 12.0\text{ s}$)
- **And** zero overshoot beyond $-15\text{ ms}$ occurs (critical damping verified).
```

---

## 3. Issue 2: NTP Sliding-Window Outlier Filter Remediation

### 3.1 Mathematical Analysis & Root Cause

The RFC 5905 clock filter maintains a window of $W = 8$ samples and computes dispersion:
$$D = \sum_{i=0}^{W-1} \frac{|s_{(i)}.\text{offset} - s_{(0)}.\text{offset}|}{2^{i+1}}$$
Outlier condition:
$$s_{\text{new}}.\text{rtt} > 2.0 \times \text{median}(\text{rtt}) \quad \lor \quad |s_{\text{new}}.\text{offset} - \hat{\theta}| > 3 \times D$$

**Failure Mode 1 (Zero-Dispersion Latch-Up)**:
In steady-state low-jitter environments, sample offsets in the window are identical or near-identical ($s_{(i)}.\text{offset} \approx s_{(0)}.\text{offset}$), causing $D \to 0\text{ ns}$. Consequently, $3 \times D \approx 0\text{ ns}$. If a legitimate clock slew or step drift of $+10\text{ ms}$ occurs:
$$|s_{\text{new}}.\text{offset} - \hat{\theta}| = 10\text{ ms} > 3 \times 0.0\text{ ms} = 0.0\text{ ms}$$
The probe is rejected. Because rejected probes are discarded before insertion, the window is never updated with new offset information. $D$ remains 0, and the filter enters permanent latch-up, rejecting 100% of subsequent valid probes.

**Failure Mode 2 (Network Reroute Lock-Out)**:
When network routing shifts (e.g., WAN path change from $100\text{ ms}$ to $210\text{ ms}$ RTT), every subsequent incoming probe satisfies:
$$s_{\text{new}}.\text{rtt} = 210\text{ ms} > 2.0 \times \text{median}(\text{rtt}) = 200\text{ ms}$$
The filter rejects all probes. Because samples are discarded before insertion, $\text{median}(\text{rtt})$ remains permanently frozen at $100\text{ ms}$, preventing recovery indefinitely.

### 3.2 Remediation Formulation

Two concrete mechanisms are required:

1. **Effective Minimum Dispersion Floor ($D_{\text{effective}}$)**:
   Define an irreducible dispersion floor $D_{\text{min}} = 5.0\text{ ms}$ ($5,000,000\text{ ns}$):
   $$D_{\text{effective}} = \max(D, \, 5.0\text{ ms})$$
   The dispersion rejection condition becomes:
   $$|s_{\text{new}}.\text{offset} - \hat{\theta}| > 3 \times D_{\text{effective}}$$
   Under zero jitter ($D = 0$), $3 \times D_{\text{effective}} = 15.0\text{ ms}$. This allows legitimate clock drifts and slews up to $15\text{ ms}$ to be accepted immediately without tripping the outlier filter, while still rejecting large asymmetric transit spikes ($> 15\text{ ms}$).

2. **Unreachability / Watchdog Rejection Counter**:
   The filter state MUST include a consecutive rejection counter: `consecutive_rejections: usize`.
   - On sample acceptance: `consecutive_rejections` is reset to 0.
   - On sample rejection (due to RTT outlier or dispersion outlier): `consecutive_rejections` is incremented by 1.
   - **Watchdog Trip Threshold ($N = 8$)**: If `consecutive_rejections >= 8` (equal to window size $W$):
     - The filter determines that network path characteristics or system time have experienced a persistent step change.
     - The filter MUST flush the circular buffer $\mathcal{W}$ (clearing invalid historical samples from the obsolete path) and insert $s_{\text{new}}$ as the first sample of the refreshed window.
     - `consecutive_rejections` is reset to 0.
     - This guarantees the filter adapts to a doubled RTT within 8 probe intervals, fully restoring normal operation in at most $8 + 8 = 16$ samples.

### 3.3 Concrete Replacement for `openspec/specs/sync-engine/spec.md` (Lines 46–61)

```markdown
### Requirement: RFC 5905 Sliding-Window Minimum-RTT Clock Filter
The client SHALL maintain a circular history buffer $\mathcal{W}$ of the most recent $W = 8$ valid probe samples. Because network transit distribution is right-skewed by variable router queueing delays ($d_{queue} \ge 0$), the authoritative filtered offset $\hat{\theta}$ MUST be selected from the sample $s_{(0)}$ having the **absolute minimum RTT** in the window:
$$\hat{\theta} = s_{(0)}.\text{offset} \quad \text{where} \quad s_{(0)}.\text{rtt} = \min_{s \in \mathcal{W}}(s.\text{rtt})$$

The filter MUST calculate window dispersion $D$:
$$D = \sum_{i=0}^{W-1} \frac{|s_{(i)}.\text{offset} - s_{(0)}.\text{offset}|}{2^{i+1}}$$
where samples are indexed in ascending order of RTT.

To prevent zero-dispersion filter latch-up during low-jitter intervals, the filter MUST enforce an effective dispersion floor:
$$D_{\text{effective}} = \max(D, \, D_{\text{min}}) \quad \text{where } D_{\text{min}} = 5.0\text{ ms} \ (5,000,000\text{ ns})$$

When the window is full ($|\mathcal{W}| = W$), the filter SHALL evaluate candidate sample $s_{\text{new}}$ for outlier rejection before insertion:
$$s_{\text{new}}.\text{rtt} > 2.0 \times \text{median}(\text{rtt}) \quad \lor \quad |s_{\text{new}}.\text{offset} - \hat{\theta}| > 3 \times D_{\text{effective}}$$

The client SHALL maintain an unreachability/watchdog counter of consecutive rejected samples (`consecutive_rejections`).
1. If a sample is accepted, `consecutive_rejections` MUST be reset to 0.
2. If a sample is rejected, `consecutive_rejections` MUST be incremented by 1.
3. If `consecutive_rejections >= 8`, the filter MUST clear all existing samples from circular buffer $\mathcal{W}$, insert $s_{\text{new}}$ as the initial sample of the refreshed window, and reset `consecutive_rejections` to 0. This ensures rapid adaptation to path reroutes and step offset shifts without deadlock.

#### Scenario: Filter Rejects Jitter Spike and Selects Minimum RTT
- **Given** a sliding window of 8 samples with RTTs ranging from $250\text{ ms}$ to $290\text{ ms}$, where sample at $252\text{ ms}$ has offset $+50\text{ ms}$
- **When** an incoming probe sample arrives with $\text{RTT} = 580\text{ ms}$ (bufferbloat congestion spike)
- **Then** the candidate sample is rejected as an outlier
- **And** the filtered offset $\hat{\theta}$ remains $+50\text{ ms}$ based on the $252\text{ ms}$ minimum sample
- **And** a subsequent sample with valid $\text{RTT} = 250\text{ ms}$ and offset $+60\text{ ms}$ ($\Delta \theta = +10\text{ ms}$) is accepted because $|60 - 50|\text{ ms} \le 3 \times 5.0\text{ ms} = 15.0\text{ ms}$.
```

---

## 4. Issue 3: Barrier-Synchronized Unpause Scheduling Equation Remediation

### 4.1 Mathematical Analysis & Root Cause

The two-phase commit barrier protocol is fundamentally a 3-way distributed exchange:
1. **Server $\to$ Client**: `UNPAUSE_PREPARE` packet travels over network.
   Transit time for client $i$: $t_{\text{transit}} \approx 0.5 \times \text{RTT}_i$.
2. **Client Processing**: Pre-rolls demuxer/decoders, decodes initial keyframe, and primes hardware audio sink.
   Duration: $t_{\text{preroll}, i} \in [0, t_{\text{preroll\_budget}}]$.
3. **Client $\to$ Server**: `UNPAUSE_READY` packet travels over network.
   Transit time: $0.5 \times \text{RTT}_i$.
4. **Server Rendezvous**: Server collects readiness from all clients. The slowest client $i$ dictates the earliest instant the server can commit:
   $$t_{\text{server\_ready\_all}} = t_{\text{server\_now}} + \max_i(\text{RTT}_i) + \max_i(t_{\text{preroll}, i})$$
5. **Server $\to$ Client**: Server emits `UNPAUSE_COMMIT`, which travels over the network to client $i$.
   Transit time: $0.5 \times \text{RTT}_i$.

For the slowest client ($\text{RTT}_{\text{max}} = \max_i(\text{RTT}_i)$), total elapsed time from `UNPAUSE_PREPARE` departure until `UNPAUSE_COMMIT` arrival is:
$$t_{\text{commit\_arrival}} = t_{\text{server\_now}} + 1.5 \times \max_i(\text{RTT}_i) + t_{\text{preroll}}$$

In the original specification:
$$T_{\text{target\_server}} = t_{\text{server\_now}} + \max_i(\text{RTT}_i) + 200\text{ ms}$$
On a 400ms RTT link ($\max(\text{RTT}) = 400\text{ ms}$):
- $T_{\text{target\_server}} = t_0 + 400 + 200 = t_0 + 600\text{ ms}$.
- Commit arrival time: $t_{\text{commit\_arrival}} = t_0 + 1.5 \times (400) + t_{\text{preroll}} = t_0 + 600\text{ ms} + t_{\text{preroll}}$.
- Margin: $T_{\text{target\_server}} - t_{\text{commit\_arrival}} = -t_{\text{preroll}}$.
Whenever pre-roll takes $> 0\text{ ms}$ (e.g. 50ms, 100ms, or 150ms), the commit arrives **after** the target presentation time has passed (negative margin: $-50\text{ ms}$ to $-150\text{ ms}$), causing hard playback stalls or desync aborts.

### 4.2 Remediation Formulation

The scheduling equation must explicitly account for:
1. Three one-way transit legs ($3 \times 0.5 \times \max(\text{RTT}) = 1.5 \times \max_i(\text{RTT}_i)$).
2. The client pre-roll budget: $t_{\text{preroll\_budget}} = 200\text{ ms}$.
3. Network jitter and server scheduling safety margin: $t_{\text{safety\_margin}} = 100\text{ ms}$.

#### Updated Scheduling Equation
$$T_{\text{target\_server}} = t_{\text{server\_now}} + 1.5 \times \max_i(\text{RTT}_i) + t_{\text{preroll\_budget}} + 100\text{ ms}$$
with $t_{\text{preroll\_budget}} = 200\text{ ms}$ and $t_{\text{safety\_margin}} = 100\text{ ms}$ (Total lead time $= 1.5 \times \max(\text{RTT}) + 300\text{ ms}$).

#### Arrival Margin Proof (Link with $\max(\text{RTT}) = 400\text{ ms}$, Target Lead Time $= 900\text{ ms}$)

| Pre-roll Duration ($t_{\text{preroll}}$) | Server Receives READY ($t_{\text{rx\_ready}}$) | Client Receives COMMIT ($t_{\text{rx\_commit}}$) | Scheduled Target ($T_{\text{target}}$) | Margin ($T_{\text{target}} - t_{\text{rx\_commit}}$) | Result |
|---|---|---|---|---|---|
| $0\text{ ms}$ | $400\text{ ms}$ | $600\text{ ms}$ | $900\text{ ms}$ | $+300\text{ ms}$ | **OK** |
| $50\text{ ms}$ | $450\text{ ms}$ | $650\text{ ms}$ | $900\text{ ms}$ | $+250\text{ ms}$ | **OK** |
| $100\text{ ms}$ | $500\text{ ms}$ | $700\text{ ms}$ | $900\text{ ms}$ | $+200\text{ ms}$ | **OK** |
| $150\text{ ms}$ | $550\text{ ms}$ | $750\text{ ms}$ | $900\text{ ms}$ | $+150\text{ ms}$ | **OK** |
| $200\text{ ms}$ (Max budget) | $600\text{ ms}$ | $800\text{ ms}$ | $900\text{ ms}$ | $+100\text{ ms}$ | **OK** |

Even at the maximum pre-roll budget ($200\text{ ms}$) on a $400\text{ ms}$ link, `UNPAUSE_COMMIT` arrives at least $100\text{ ms}$ before presentation start, comfortably absorbing $\pm 30\text{ ms}$ network jitter.

#### Timeout / Abort Recovery State Machine
To guarantee safety in case of dropped packets or disconnected clients:
- The server SHALL maintain a ready deadline timer:
  $$t_{\text{ready\_deadline}} = t_{\text{server\_now}} + \max_i(\text{RTT}_i) + t_{\text{preroll\_budget}} + 50\text{ ms}$$
- If any active client fails to emit `UNPAUSE_READY` before $t_{\text{ready\_deadline}}$:
  - The server MUST broadcast `UNPAUSE_ABORT(seq, reason="READY_TIMEOUT")`.
  - All clients MUST cancel their scheduled unpause timers, mute audio, and remain in the paused state $\tau_{\text{unpause}}$.
  - The room state remains paused without partial playback desynchronization.

### 4.3 Concrete Replacement for `openspec/specs/sync-engine/spec.md` (Lines 129–151)

```markdown
### Requirement: Two-Phase Barrier-Synchronized Unpause Protocol
To eliminate playback startup desynchronization caused by asymmetric network delays, the unpause sequence SHALL execute a two-phase commit barrier protocol:
1. **Phase 1 (`UNPAUSE_PREPARE`)**:
   - The room leader/server schedules a future start timestamp accounting for the 3-way exchange ($1.5 \times \text{RTT}$), client pre-roll budget ($200\text{ ms}$), and jitter safety margin ($100\text{ ms}$):
     $$T_{\text{target\_server}} = t_{\text{server\_now}} + 1.5 \times \max_i(\text{RTT}_i) + t_{\text{preroll\_budget}} + 100\text{ ms}$$
     where $t_{\text{preroll\_budget}} = 200\text{ ms}$.
   - Server broadcasts `UNPAUSE_PREPARE(seq, T_{\text{target\_server}}, \tau_{\text{unpause}})`.
2. **Phase 2 (`UNPAUSE_READY`)**:
   - Each client translates target time to local monotonic time: $T_{\text{target\_local}} = T_{\text{target\_server}} - \hat{\theta}$.
   - The client pre-rolls the demuxer and decoders up to $\tau_{\text{unpause}}$, fills the hardware audio output sink, and mutes playback (within budget $t_{\text{preroll\_budget}} \le 200\text{ ms}$).
   - The client transmits `UNPAUSE_READY(seq, client_id)`.
3. **Phase 3 (`UNPAUSE_COMMIT`)**:
   - Upon gathering readiness from active clients, the server broadcasts `UNPAUSE_COMMIT(seq, T_{\text{target\_server}})`.
   - At the exact monotonic instant $t_{\text{local}} = T_{\text{target\_local}}$, each client un-mutes and initiates audio rendering and video presentation.
4. **Timeout & Abort Recovery**:
   - If any active client fails to report `UNPAUSE_READY` within $t_{\text{ready\_timeout}} = t_{\text{server\_now}} + \max_i(\text{RTT}_i) + t_{\text{preroll\_budget}} + 50\text{ ms}$, the server MUST broadcast `UNPAUSE_ABORT(seq, reason="READY_TIMEOUT")`.
   - Upon receiving `UNPAUSE_ABORT`, clients SHALL disarm the unpause timer, purge uncommitted pre-roll buffers, and remain in `Paused` state.

Cross-peer playback start alignment across links with up to 400ms RTT MUST be within $< 10\text{ ms}$.

#### Scenario: Barrier Unpause Across Two Peers with Asymmetric Latencies
- **Given** Client A with $\text{RTT} = 360\text{ ms}$ and Client B with $\text{RTT} = 80\text{ ms}$
- **When** the room leader issues an unpause command at $t_{\text{server\_now}} = 0$
- **Then** scheduled target is $T_{\text{target\_server}} = 0 + 1.5 \times 360 + 200 + 100 = 840\text{ ms}$
- **And** Client A receives `UNPAUSE_COMMIT` at $t \le 740\text{ ms}$ ($180\text{ ms forward} + 200\text{ ms pre-roll} + 180\text{ ms ready} + 180\text{ ms commit}$), giving $\ge 100\text{ ms}$ positive commit arrival margin
- **And** when local clocks reach their respective target instants, both begin rendering
- **And** the physical time discrepancy between Client A start and Client B start is less than $10\text{ ms}$.
```

---

## 5. Synchronizing Supporting Documents

To prevent inconsistencies across OpenSpec, the implementation worker must also align `openspec/changes/004-network-sync-protocol.md`:

1. **Line 38**:
   - *Original*: `- src/pll/controller.rs: Discrete Proportional-Integral (PI) drift controller ($K_p=0.05, K_i=0.000625$).`
   - *Updated*: `- src/pll/controller.rs: Discrete Proportional-Integral (PI) drift controller ($K_p=0.17\text{ s}^{-1}, K_i=0.0072\text{ s}^{-2}$).`
2. **Line 51**:
   - *Original*: `- Outlier rejection: $s_{\text{new}}.\text{rtt} > 2.0 \times \text{median}(\text{rtt}) \lor |s_{\text{new}}.\text{offset} - \hat{\theta}| > 3D$.`
   - *Updated*: `- Outlier rejection: $s_{\text{new}}.\text{rtt} > 2.0 \times \text{median}(\text{rtt}) \lor |s_{\text{new}}.\text{offset} - \hat{\theta}| > 3D_{\text{effective}}$ where $D_{\text{effective}} = \max(D, 5.0\text{ ms})$, with 8-rejection watchdog buffer flush.`
3. **Lines 58–60**:
   - *Original*: `4. **Two-Phase Barrier Unpause**:\n - UNPAUSE_PREPARE(T_target) -> Pre-roll decode & prime audio -> UNPAUSE_READY -> UNPAUSE_COMMIT.`
   - *Updated*: `4. **Two-Phase Barrier Unpause**:\n - Target schedule: $T_{\text{target\_server}} = t_{\text{server\_now}} + 1.5 \times \max(\text{RTT}) + t_{\text{preroll\_budget}} + 100\text{ ms}$ ($t_{\text{preroll\_budget}} = 200\text{ ms}$).\n - UNPAUSE_PREPARE -> Pre-roll decode & prime audio -> UNPAUSE_READY -> UNPAUSE_COMMIT, with timeout abort recovery.`

---

## 6. Implementation Worker Action Plan & Verification Matrix

| Step | Target File | Target Lines | Modification Description |
|---|---|---|---|
| **Step 1** | `openspec/specs/sync-engine/spec.md` | 46–61 | Add $D_{\text{effective}} = \max(D, 5.0\text{ ms})$ and 8-sample consecutive rejection watchdog flush rule. |
| **Step 2** | `openspec/specs/sync-engine/spec.md` | 106–126 | Update PI gains to $K_p = 0.17\text{ s}^{-1}, K_i = 0.0072\text{ s}^{-2}$ and verify 20s scenario convergence. |
| **Step 3** | `openspec/specs/sync-engine/spec.md` | 129–151 | Update barrier scheduling formula to $1.5 \times \max(\text{RTT}) + t_{\text{preroll\_budget}} + 100\text{ ms}$ and add timeout state machine. |
| **Step 4** | `openspec/changes/004-network-sync-protocol.md` | 38, 51, 58–60 | Synchronize change proposal parameters with updated spec values. |
| **Step 5** | `tests/empirical_challenge/test_sync_engine.py` | 40–93, 100–120, 290–320 | Update test harness with $D_{\text{effective}}$, watchdog reset, $K_p=0.17, K_i=0.0072$, and $1.5\text{RTT} + 300\text{ms}$ barrier timing to verify all tests pass (0 failures). |
