## 2026-10-24T23:03:10Z

From: 1a21f3c9-f57a-4ad4-b306-97b4ba0e698e (parent)
To: explorer_survey_2

You are explorer_survey_2 (teamwork_preview_explorer).
Your Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_2/
Project Root: /home/gautham/syncplay-alt
Authoritative User Request: /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md

### Objective
Conduct an exhaustive technical exploration and survey for the network synchronization and player UI/OSD subsystems of Syncplay-Alt:
1. Long-distance high-RTT synchronization protocol (NTP-style clock offset estimation with statistical outlier filtering, deterministic reference timeline equation, PLL micro-speed drift correction without hard-seek stutter, barrier-synchronized ready/commit unpause).
2. Native player UI & OSD overlay architecture with VLC-grade responsiveness and minimal memory footprint.

### Scope & Constraints
- Read-only technical investigation. Do NOT write or modify application source code.
- Write your working files exclusively in your working directory `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_2/`.
- Prioritize formal mathematical specifications (NTP RFC 5905, clock filter algorithms, discrete PI/PID controllers, phase error equations) and low-latency UI rendering architectures.

### Detailed Requirements to Survey & Specify
1. **Long-Distance High-RTT Synchronization Protocol**:
   - NTP-style 4-timestamp exchange ($T_1, T_2, T_3, T_4$):
     * Round-Trip Time $RTT = (T_4 - T_1) - (T_3 - T_2)$
     * Clock Offset $\theta = \frac{(T_2 - T_1) + (T_3 - T_4)}{2}$
   - Outlier filtering & statistical windowing: Marzullo's algorithm or sliding-window minimum-filter (best 8 samples sorted by lowest RTT, dispersion filtering, rejecting asymmetric latency bursts).
   - Deterministic Reference Timeline equation: mapping server monotonic master timeline $t_{master}(t)$ to local media playback position $t_{local}(t)$, accounting for playback rate $r$, pause state, and leader election / authoritative room state.
   - Phase-Locked Loop (PLL) micro-speed drift correction math:
     * Phase error $e(t) = t_{target}(t) - t_{actual}(t)$.
     * Drift bounds: for $|e(t)| < 15\text{ ms}$, playback speed $r = 1.0$; for $15\text{ ms} \le |e(t)| \le 1500\text{ ms}$, apply smooth micro-speed adjustments (e.g. $r \in [0.96, 1.04]$ via proportional-integral controller $r(t) = 1.0 + K_p e(t) + K_i \int e(\tau)d\tau$) to smoothly steer phase error $< 15\text{ ms}$ without audio pitch distortion or video stutter.
     * Hard seek threshold: only when $|e(t)| > 1500\text{ ms}$ does the client perform a discontinuous hard seek.
   - Barrier-synchronized Ready/Commit Unpause protocol:
     * Two-phase commit: Leader announces unpause target $T_{start} = \text{now} + \max(RTT_i) + \Delta_{margin}$ (e.g. 500ms in the future).
     * Peers pre-buffer, pre-roll decode, and send `READY(T_{start})`.
     * Leader broadcasts `COMMIT(T_{start})`. All peers schedule hardware presentation at exact monotonic timestamp $T_{start}$, achieving playback alignment $< 10\text{ ms}$ across peers despite 250–400ms RTT and ±30ms jitter.
2. **Native UI & OSD Architecture**:
   - Architecture separating UI render thread from decode and sync threads via atomic state snapshots.
   - OSD overlay pipeline: rendering play/pause icons, timecode `00:12:34 / 01:45:00`, sync health badge (`RTT: 320ms`, `Drift: +4.2ms`, `PLL: 1.002x`), floating chat bubbles, and progress bar with chapter/bookmark marks.
   - VLC-grade responsiveness: input handling latencies < 16ms, zero thread contention on the UI loop.

### Deliverables
Write a comprehensive report to `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_2/analysis.md` and complete a structured handoff report in `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_2/handoff.md`.
Notify the orchestrator via `send_message` when complete.
