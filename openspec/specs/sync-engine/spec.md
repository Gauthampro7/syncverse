# Capability Specification: Network Synchronization Protocol (`syncverse-sync`)

**Component**: `syncverse-sync`  
**Standards Compliance**: RFC 5905 (Network Time Protocol v4), Discrete Control Theory (Critically Damped PI Controller)  
**Status**: Authoritative OpenSpec Specification  

---

## 1. Overview & Architectural Role

The `syncverse-sync` crate implements long-distance, high-latency watch-party synchronization over wide-area networks. In transatlantic and transcontinental sessions, participants routinely experience 250–400ms RTT and $\pm 30\text{ms}$ packet jitter. Under such conditions, naive synchronization strategies that issue hard seeks or naive rate throttling cause unwatchable video stutter and jarring audio pitch warble.

`syncverse-sync` provides:
1. **NTP 4-Timestamp Probe Exchange**: Symmetric offset estimation with RFC 5905 sliding-window minimum RTT filtering to bypass asymmetric queueing noise.
2. **Deterministic Reference Timeline Equation**: Exact mapping between room master state and local presentation time.
3. **Tri-Zone Phase-Locked Loop (PLL) Micro-Speed Drift Controller**: A discrete Proportional-Integral (PI) controller operating within an imperceptible $\pm 4\%$ speed envelope ($r \in [0.96, 1.04]$) to steer phase error into a $< 15\text{ ms}$ deadband without hard seeks for drifts under $1.5\text{ seconds}$.
4. **Two-Phase Scheduled Barrier Unpause**: Pre-roll decoding and barrier rendezvous eliminating startup desynchronization to $< 10\text{ ms}$.

---

## 2. Requirements & Scenarios

### Requirement: NTP 4-Timestamp Probe Exchange & Math
The client and server SHALL execute a 4-timestamp probe handshake using 64-bit nanosecond monotonic timestamps:
- $T_1$: Client transmit monotonic timestamp.
- $T_2$: Server receive monotonic timestamp.
- $T_3$: Server reply transmit monotonic timestamp.
- $T_4$: Client reply receive monotonic timestamp.

The estimated clock offset $\theta$ and measured round-trip time $\text{RTT}$ MUST be computed as:
$$\theta = \frac{(T_2 - T_1) + (T_3 - T_4)}{2}$$
$$\text{RTT} = (T_4 - T_1) - (T_3 - T_2)$$

#### Scenario: Symmetric Delay Offset Calculation
- **Given** probe timestamps:
  - $T_1 = 1,000,000,000\text{ ns}$ (1.0s)
  - $T_2 = 1,150,000,000\text{ ns}$ (Server ahead by 50ms + 100ms transit)
  - $T_3 = 1,155,000,000\text{ ns}$ (Server processing 5ms)
  - $T_4 = 1,260,000,000\text{ ns}$ (Client receive after 100ms return transit)
- **When** the probe sample is processed
- **Then** calculated RTT is $(1,260 - 1,000) - (1,155 - 1,150) = 260 - 5 = 255\text{ ms}$ ($255,000,000\text{ ns}$)
- **And** calculated clock offset $\theta$ is $\frac{(150) + (-105)}{2} = \frac{45}{2} = +22.5\text{ ms}$ ($+22,500,000\text{ ns}$).

---

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

---

### Requirement: Deterministic Reference Timeline Evaluation
The master room state SHALL be defined by the tuple $\mathcal{S}_{\text{room}} = \langle t_{\text{ref\_server}}, \tau_{\text{ref\_media}}, r_{\text{target}}, \text{state}, \text{seq} \rangle$.
At any local monotonic instant $t_{\text{local}}$, the client MUST compute the target media presentation position $\tau_{\text{target}}(t_{\text{local}})$ as:
$$\tau_{\text{target}}(t_{\text{local}}) = \begin{cases}
\tau_{\text{ref\_media}}, & \text{if } \text{state} = \text{Paused} \\
\tau_{\text{ref\_media}} + r_{\text{target}} \cdot \left[ (t_{\text{local}} + \hat{\theta}) - t_{\text{ref\_server}} \right], & \text{if } \text{state} = \text{Playing}
\end{cases}$$

State updates with $\text{seq} \le \text{seq}_{\text{local}}$ MUST be discarded to prevent out-of-order UDP rollback.

#### Scenario: Evaluate Target Media Position During Playback
- **Given** a room state with $\tau_{\text{ref\_media}} = 60,000\text{ ms}$, $t_{\text{ref\_server}} = 5,000,000,000\text{ ns}$ (5.0s), and $r_{\text{target}} = 1.0$
- **And** client filtered offset $\hat{\theta} = +200,000,000\text{ ns}$ (+200ms)
- **When** client monotonic time is $t_{\text{local}} = 6,800,000,000\text{ ns}$ (6.8s)
- **Then** estimated server time is $6.8\text{s} + 0.2\text{s} = 7.0\text{s}$ ($7,000,000,000\text{ ns}$)
- **And** elapsed server time is $7.0\text{s} - 5.0\text{s} = 2.0\text{s}$ ($2,000\text{ ms}$)
- **And** target media position $\tau_{\text{target}}$ equals $60,000 + 2000 = 62,000\text{ ms}$.

---

### Requirement: Tri-Zone PLL Drift Policy & Thresholds
The client SHALL evaluate the phase error $e(t)$ between target and actual rendered media PTS:
$$e(t) = \tau_{\text{target}}(t) - \tau_{\text{actual}}(t)$$
The controller MUST categorize $e(t)$ into three mutually exclusive operational zones:
1. **Zone 1: Deadband In-Sync Zone ($|e(t)| < 15\text{ ms}$)**:
   - The commanded playback speed SHALL be exactly $r = r_{\text{target}} = 1.0$.
   - Prevents micro-hunting oscillations around zero.
2. **Zone 2: Micro-Speed Linear PLL Control Zone ($15\text{ ms} \le |e(t)| \le 1500\text{ ms}$)**:
   - The commanded speed SHALL be dynamically modulated within the bounds $r \in [0.96, 1.04]$.
   - The controller MUST NOT issue a hard seek.
3. **Zone 3: Discontinuous Hard Seek Threshold ($|e(t)| > 1500\text{ ms}$)**:
   - The controller SHALL issue a hard seek command to $\tau_{\text{target}}$, flushing demuxer and decoder pipelines.

#### Scenario: Execute Micro-Speed Slew for 300ms Phase Error
- **Given** an actual rendered media PTS of $10,000\text{ ms}$ and a target PTS of $10,300\text{ ms}$ ($e = +300\text{ ms}$, client lagging)
- **When** the PLL controller evaluates the drift
- **Then** it selects Zone 2 (Micro-Speed PLL)
- **And** sets the playback rate $r > 1.00$ (e.g. $1.035\times$)
- **And** does NOT issue a hard seek.

---

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

---

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
