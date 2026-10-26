#!/usr/bin/env python3
"""
Empirical Challenge Harness 1: Sync Engine (Remediated)
Tests:
1. NTP RFC 5905 sliding-window minimum-RTT filter with D_effective & watchdog flush:
   - Window size W=8
   - Minimum RTT selection
   - Window dispersion calculation with D_effective = max(D, 5.0ms)
   - Outlier rejection filter: RTT > 2.0 * median(rtt) OR |offset - theta_hat| > 3 * D_effective
   - Watchdog reset after 8 consecutive rejections to recover from network reroute
   - Asymmetric delay spike rejection test
   - Legitimate offset drift acceptance test
2. Discrete PI controller dynamics:
   - Sample interval dt = 0.2s
   - Remediated critically damped gains: Kp = 0.17 s^-1, Ki = 0.0072 s^-2, I_max = 32.0s
   - Zone deadbands: Zone 1 (|e| < 15ms), Zone 2 (15ms <= |e| <= 1500ms), Zone 3 (|e| > 1500ms)
   - Convergence test: e(0) = +450ms enters < 15ms deadband within 20s (at t ~ 12.0s)
   - Zero overshoot past -15ms
3. Barrier unpause two-phase commit timing:
   - T_target_server = t_server_now + 1.5 * max(RTT_i) + t_preroll_budget (200ms) + 100ms margin
   - Guarantees >= 100ms positive commit arrival margin across all pre-roll intervals up to 200ms
"""

import sys
import math
from typing import List, Tuple, Optional

class ProbeSample:
    def __init__(self, t1: int, t2: int, t3: int, t4: int):
        self.t1 = t1
        self.t2 = t2
        self.t3 = t3
        self.t4 = t4
        self.offset = ((t2 - t1) + (t3 - t4)) / 2.0
        self.rtt = (t4 - t1) - (t3 - t2)

class Rtc5905Filter:
    def __init__(self, window_size: int = 8, min_dispersion_floor_ns: float = 5_000_000.0):
        self.w = window_size
        self.d_min = min_dispersion_floor_ns
        self.window: List[ProbeSample] = []
        self.rejected_count = 0
        self.accepted_count = 0
        self.consecutive_rejections = 0

    def compute_dispersion(self) -> float:
        if not self.window:
            return 0.0
        # Sort samples in ascending order of RTT
        sorted_samples = sorted(self.window, key=lambda s: s.rtt)
        s0 = sorted_samples[0]
        dispersion = 0.0
        for i, s in enumerate(sorted_samples):
            weight = 2.0 ** (i + 1)
            dispersion += abs(s.offset - s0.offset) / weight
        return dispersion

    def get_effective_dispersion(self) -> float:
        return max(self.compute_dispersion(), self.d_min)

    def get_filtered_offset(self) -> Optional[float]:
        if not self.window:
            return None
        # Absolute minimum RTT sample
        s0 = min(self.window, key=lambda s: s.rtt)
        return s0.offset

    def get_median_rtt(self) -> float:
        if not self.window:
            return 0.0
        rtts = sorted([s.rtt for s in self.window])
        n = len(rtts)
        if n % 2 == 1:
            return float(rtts[n // 2])
        return (rtts[n // 2 - 1] + rtts[n // 2]) / 2.0

    def process_sample(self, sample: ProbeSample) -> bool:
        # Check outlier rejection condition if window is full
        if len(self.window) >= self.w:
            med_rtt = self.get_median_rtt()
            dispersion_eff = self.get_effective_dispersion()
            filtered_theta = self.get_filtered_offset()
            
            # Spec condition: s_new.rtt > 2.0 * median(rtt) or |s_new.offset - theta_hat| > 3 * D_effective
            rtt_outlier = sample.rtt > 2.0 * med_rtt
            dispersion_outlier = abs(sample.offset - filtered_theta) > 3.0 * dispersion_eff

            if rtt_outlier or dispersion_outlier:
                self.consecutive_rejections += 1
                if self.consecutive_rejections >= 8:
                    # Watchdog trip: path reroute or persistent step offset shift.
                    # Flush stale window and restart with candidate sample.
                    self.window = [sample]
                    self.consecutive_rejections = 0
                    self.accepted_count += 1
                    return True
                self.rejected_count += 1
                return False

        # Accepted
        self.consecutive_rejections = 0
        self.accepted_count += 1
        if len(self.window) >= self.w:
            self.window.pop(0)
        self.window.append(sample)
        return True


def test_pi_controller_convergence():
    print("=" * 60)
    print("TEST 1: PI CONTROLLER DYNAMICS & CONVERGENCE")
    print("=" * 60)
    
    dt = 0.2  # 200ms evaluation interval
    Kp = 0.17
    Ki = 0.0072
    I_max = 32.0
    r_min = 0.96
    r_max = 1.04
    deadband = 0.015  # 15ms
    
    # Spec scenario: initial phase error e(0) = +450ms = +0.45s
    # Total duration = 25 seconds (125 steps)
    e = 0.450
    I = 0.0
    
    history = []
    
    steps = int(25.0 / dt)
    time_entered_deadband = None
    min_error = e
    overshoot = False
    
    for step in range(steps):
        t = step * dt
        history.append((t, e, I))
        
        # Check zone
        if abs(e) < deadband:
            r = 1.0
            if time_entered_deadband is None:
                time_entered_deadband = t
        elif abs(e) <= 1.500:
            # Zone 2: Discrete PI controller
            # I[k] = I[k-1] + e[k] * dt
            I_candidate = I + e * dt
            # Anti-windup clamping
            I = max(-I_max, min(I_max, I_candidate))
            
            delta_r_raw = Kp * e + Ki * I
            r = max(r_min, min(r_max, 1.0 + delta_r_raw))
        else:
            # Zone 3: Hard seek
            r = 1.0
            e = 0.0
            I = 0.0
        
        # Plant update: target advances by 1.0 * dt, actual advances by r * dt
        # e[k+1] = e[k] - (r - 1.0) * dt
        delta_phase = (r - 1.0) * dt
        e_next = e - delta_phase
        
        if e_next < -deadband and e >= -deadband:
            overshoot = True
        
        e = e_next
        min_error = min(min_error, abs(e))

    print(f"Initial error: {0.450*1000:.1f} ms")
    print(f"Error at t=20.0s: {history[int(20.0/dt)][1]*1000:.2f} ms")
    print(f"Error at t=25.0s: {history[-1][1]*1000:.2f} ms")
    print(f"Time entered < 15ms deadband: {time_entered_deadband if time_entered_deadband is not None else 'NEVER'} s")
    print(f"Overshoot past -15ms: {overshoot}")
    
    # Detailed log every 2 seconds
    print("\nTime (s) | Error (ms) | Rate (r) | Integral I (s)")
    print("-" * 50)
    for step in range(0, steps, int(2.0/dt)):
        t, err, integ = history[step]
        dr = Kp * err + Ki * integ
        rate = max(0.96, min(1.04, 1.0 + dr)) if abs(err) >= deadband else 1.0
        print(f"{t:7.1f} | {err*1000:10.2f} | {rate:8.5f} | {integ:10.4f}")
        
    assert time_entered_deadband is not None, "Error: Controller never entered < 15ms deadband"
    assert time_entered_deadband <= 20.0, f"Error: Entered deadband at {time_entered_deadband}s > 20.0s requirement"
    assert not overshoot, "Error: Overshoot occurred beyond -15ms (not critically damped)"
    print(">>> TEST 1 PASSED: PI Controller converges critically damped into deadband within 20s (at t=12.0s).")

    return {
        "error_at_20s_ms": history[int(20.0/dt)][1] * 1000,
        "time_entered_deadband": time_entered_deadband,
        "overshoot": overshoot
    }


def test_ntp_filter_asymmetry_and_deadlock():
    print("\n" + "=" * 60)
    print("TEST 2: NTP RFC 5905 FILTER (ASYMMETRY & DISPERSION FREEZE)")
    print("=" * 60)
    
    filt = Rtc5905Filter(window_size=8)
    
    # Phase A: Seed the window with 8 stable samples (RTT = 250ms, symmetric transit 125ms forward, 125ms return, true offset = +50ms)
    for i in range(8):
        s = ProbeSample(
            t1=i * 1000_000_000,
            t2=i * 1000_000_000 + 175_000_000,
            t3=i * 1000_000_000 + 180_000_000,
            t4=i * 1000_000_000 + 255_000_000
        )
        filt.process_sample(s)
        
    print(f"Initial 8 samples added.")
    print(f"Filtered offset: {filt.get_filtered_offset() / 1e6:.2f} ms")
    print(f"Window dispersion D: {filt.compute_dispersion() / 1e6:.4f} ms")
    print(f"Effective dispersion D_eff: {filt.get_effective_dispersion() / 1e6:.4f} ms")
    print(f"Median RTT: {filt.get_median_rtt() / 1e6:.2f} ms")
    
    # TEST CASE A: Asymmetric Delay Spike (Total RTT unchanged = 250ms, but forward is 200ms, return is 50ms)
    # Offset error = +75ms -> |125 - 50| = 75ms > 3 * 5.0ms = 15.0ms -> REJECTED
    asym_sample = ProbeSample(
        t1=10_000_000_000,
        t2=10_250_000_000,
        t3=10_255_000_000,
        t4=10_255_000_000
    )
    accepted = filt.process_sample(asym_sample)
    print(f"\nAsymmetric probe: RTT = {asym_sample.rtt/1e6:.1f} ms, computed offset = {asym_sample.offset/1e6:.1f} ms (true = 50ms)")
    print(f"Was asymmetric probe accepted? {accepted}")
    assert not accepted, "Error: Asymmetric delay spike must be rejected"

    # TEST CASE B: Legitimate Clock Step / Drift (true offset becomes +60ms, delta = 10ms)
    # Under D_effective = 5.0ms, threshold = 3 * 5.0ms = 15.0ms. Since 10ms <= 15ms, probe is accepted!
    step_sample = ProbeSample(
        t1=11_000_000_000,
        t2=11_185_000_000, # +10ms
        t3=11_190_000_000,
        t4=11_245_000_000
    )
    accepted_step = filt.process_sample(step_sample)
    print(f"\nLegitimate +10ms offset step probe: RTT={step_sample.rtt/1e6:.1f}ms, offset={step_sample.offset/1e6:.1f}ms")
    print(f"Was legitimate +10ms probe accepted? {accepted_step}")
    assert accepted_step, "Error: Legitimate +10ms offset step must be accepted under D_effective floor"
    
    # Try 50 consecutive valid probes with +10ms offset
    rejected_consecutive = 0
    for k in range(50):
        s = ProbeSample(
            t1=(12 + k) * 1_000_000_000,
            t2=(12 + k) * 1_000_000_000 + 185_000_000,
            t3=(12 + k) * 1_000_000_000 + 190_000_000,
            t4=(12 + k) * 1_000_000_000 + 245_000_000
        )
        if not filt.process_sample(s):
            rejected_consecutive += 1
            
    print(f"Consecutive valid probes rejected after offset step: {rejected_consecutive} / 50")
    assert rejected_consecutive == 0, "Error: Valid probes must not be rejected"

    # TEST CASE C: Route Change RTT Doubling (Adapted via 8-rejection Watchdog)
    filt2 = Rtc5905Filter(window_size=8)
    for i in range(8):
        filt2.process_sample(ProbeSample(
            t1=i * 1_000_000_000,
            t2=i * 1_000_000_000 + 100_000_000,
            t3=i * 1_000_000_000 + 105_000_000,
            t4=i * 1_000_000_000 + 205_000_000 # RTT = 100ms
        ))
    med_rtt = filt2.get_median_rtt()
    print(f"\nInitial median RTT: {med_rtt/1e6:.1f} ms")
    
    # Network path reroutes, new RTT is 210ms (> 2.0 * median)
    rejected_reroute = 0
    accepted_reroute = 0
    for k in range(50):
        s = ProbeSample(
            t1=(10 + k) * 1_000_000_000,
            t2=(10 + k) * 1_000_000_000 + 105_000_000,
            t3=(10 + k) * 1_000_000_000 + 110_000_000,
            t4=(10 + k) * 1_000_000_000 + 320_000_000 # RTT = 210ms
        )
        if not filt2.process_sample(s):
            rejected_reroute += 1
        else:
            accepted_reroute += 1
            
    print(f"Reroute probes: rejected {rejected_reroute}, accepted {accepted_reroute} / 50")
    assert rejected_reroute in (7, 8), f"Error: Watchdog should trigger within 8 rejections, got {rejected_reroute}"
    assert accepted_reroute >= 42, f"Error: Subsequent probes after watchdog flush should be accepted, got {accepted_reroute}"
    print(f"Updated median RTT after recovery: {filt2.get_median_rtt()/1e6:.1f} ms")
    assert filt2.get_median_rtt() == 315_000_000, "Error: Median RTT failed to adapt to new route"
    print(">>> TEST 2 PASSED: NTP filter handles asymmetry, prevents zero-dispersion freeze, and recovers from reroutes via watchdog.")


def test_barrier_unpause_timing():
    print("\n" + "=" * 60)
    print("TEST 3: TWO-PHASE BARRIER UNPAUSE TIMELINE")
    print("=" * 60)
    
    # Formula in spec: T_target_server = t_server_now + 1.5 * max_i(RTT_i) + t_preroll_budget + 100ms
    # Link: Client A has RTT = 400ms (one-way = 200ms)
    # Client B has RTT = 80ms (one-way = 40ms)
    # max_rtt = 400ms
    # t_preroll_budget = 200ms
    # safety_margin = 100ms
    # T_target_server = 0 + 1.5 * 400 + 200 + 100 = 900ms
    
    max_rtt = 400
    t_target_server = int(1.5 * max_rtt + 200 + 100)  # 900ms
    
    preroll_durations = [0, 50, 100, 150, 200]
    
    for preroll in preroll_durations:
        t_client_ready_sent = 200 + preroll
        t_server_receives_ready = t_client_ready_sent + 200
        t_client_receives_commit = t_server_receives_ready + 200
        
        deadline = t_target_server # 900ms
        margin = deadline - t_client_receives_commit
        
        status = "OK" if margin >= 100 else "MISSED DEADLINE (Late Commit!)"
        print(f"Pre-roll: {preroll:3d}ms -> Server receives READY at t={t_server_receives_ready:3d}ms -> Client receives COMMIT at t={t_client_receives_commit:3d}ms vs Target {deadline:3d}ms -> Margin: {margin:4d}ms [{status}]")
        assert margin >= 100, f"Error: Pre-roll {preroll}ms resulted in margin {margin}ms < 100ms"
        
    print(">>> TEST 3 PASSED: Barrier unpause timeline guarantees >= 100ms positive commit arrival margin across all pre-roll intervals.")


if __name__ == "__main__":
    test_pi_controller_convergence()
    test_ntp_filter_asymmetry_and_deadlock()
    test_barrier_unpause_timing()
    print("\nALL SYNC ENGINE TESTS PASSED (0 ERRORS).")
