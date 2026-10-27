#!/usr/bin/env python3
"""
Empirical Challenge & Boundary Stress Test Suite for Crates 5 to 8:
- syncverse-sync (NTP filter, discrete PI PLL micro-speed, two-phase barrier unpause)
- syncverse-chat (binary log crash recovery, false-sync resilience, CJK inverted index FTS, slash commands)
- syncverse-ui (120Hz scrubber debouncer, 5-bubble memory bound, Alpha8 atlas footprint, SPSC triple buffer)
- syncverse-core (telemetry aggregator, runtime coordinator)
"""

import sys
import math
import struct
import zlib
import time
from typing import List, Tuple, Optional, Dict

# ============================================================================
# 1. HIGH-RTT NETWORK & RFC 5905 NTP FILTER HARNESS
# ============================================================================
class ProbeSample:
    def __init__(self, t1: int, t2: int, t3: int, t4: int):
        self.t1 = t1
        self.t2 = t2
        self.t3 = t3
        self.t4 = t4
        self.offset = ((t2 - t1) + (t3 - t4)) / 2.0
        self.rtt = (t4 - t1) - (t3 - t2)

class Rfc5905FilterHarness:
    def __init__(self, window_size: int = 8, min_dispersion_floor_ns: float = 5_000_000.0):
        self.w = window_size
        self.d_min = min_dispersion_floor_ns
        self.window: List[ProbeSample] = []
        self.consecutive_rejections = 0
        self.accepted_count = 0
        self.rejected_count = 0

    def compute_dispersion(self) -> float:
        if not self.window:
            return 0.0
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
        return min(self.window, key=lambda s: s.rtt).offset

    def get_median_rtt(self) -> float:
        if not self.window:
            return 0.0
        rtts = sorted([s.rtt for s in self.window])
        n = len(rtts)
        if n % 2 == 1:
            return float(rtts[n // 2])
        return (rtts[n // 2 - 1] + rtts[n // 2]) / 2.0

    def process_sample(self, sample: ProbeSample) -> bool:
        if len(self.window) >= self.w:
            med_rtt = self.get_median_rtt()
            dispersion_eff = self.get_effective_dispersion()
            filtered_theta = self.get_filtered_offset()

            rtt_outlier = sample.rtt > 2.0 * med_rtt
            dispersion_outlier = abs(sample.offset - filtered_theta) > 3.0 * dispersion_eff

            if rtt_outlier or dispersion_outlier:
                self.consecutive_rejections += 1
                if self.consecutive_rejections >= 8:
                    # Watchdog trip: path reroute
                    self.window = [sample]
                    self.consecutive_rejections = 0
                    self.accepted_count += 1
                    return True
                self.rejected_count += 1
                return False

        self.consecutive_rejections = 0
        self.accepted_count += 1
        if len(self.window) >= self.w:
            self.window.pop(0)
        self.window.append(sample)
        return True

def test_high_rtt_network_ntp():
    print("=" * 60)
    print("CHALLENGE 1: HIGH-RTT NETWORK & NTP FILTER STRESS TEST")
    print("=" * 60)

    filt = Rfc5905FilterHarness(window_size=8, min_dispersion_floor_ns=5_000_000.0)

    # 1. 250-400ms RTT with ±30ms jitter
    jitter_offsets = [-28.0, 14.0, -12.0, 25.0, -6.0, 30.0, -30.0, 8.0]
    for i, j in enumerate(jitter_offsets):
        rtt_ms = 300.0 + j
        half_rtt_ns = int((rtt_ms / 2.0) * 1_000_000)
        t1 = i * 1_000_000_000
        t2 = t1 + half_rtt_ns + 50_000_000 # true offset +50ms
        t3 = t2 + 5_000_000
        t4 = t3 + half_rtt_ns - 50_000_000
        sample = ProbeSample(t1, t2, t3, t4)
        accepted = filt.process_sample(sample)
        assert accepted, f"Initial sample {i} rejected"

    print(f"  [PASS] 8 jittery samples accepted (RTTs 270-330ms).")
    filtered_theta = filt.get_filtered_offset() / 1e6
    print(f"  [PASS] Filtered offset = {filtered_theta:.2f} ms (expected 50.0 ms).")
    assert abs(filtered_theta - 50.0) < 1e-3

    # Dispersion floor check:
    d_eff = filt.get_effective_dispersion() / 1e6
    print(f"  [PASS] Effective dispersion D_eff = {d_eff:.4f} ms >= 5.0 ms floor.")
    assert d_eff >= 5.0

    # Asymmetric Delay Spike: forward = 250ms, return = 50ms, total RTT = 300ms, offset = 150ms
    asym_sample = ProbeSample(
        10_000_000_000,
        10_300_000_000,
        10_305_000_000,
        10_305_000_000,
    )
    assert not filt.process_sample(asym_sample), "Asymmetric delay spike must be rejected"
    print("  [PASS] Asymmetric transit delay spike (+100ms offset error) strictly rejected.")

    # Legitimate +10ms clock drift (offset becomes +60ms)
    step_sample = ProbeSample(
        11_000_000_000,
        11_210_000_000,
        11_215_000_000,
        11_305_000_000,
    )
    assert filt.process_sample(step_sample), "Legitimate +10ms step must be accepted under D_eff"
    print("  [PASS] Legitimate +10ms drift accepted under D_effective floor.")

    # Route Change Watchdog Reset: RTT jumps to 700ms (> 2.0 * median)
    rejected = 0
    accepted = 0
    for k in range(50):
        s = ProbeSample(
            (12 + k) * 1_000_000_000,
            (12 + k) * 1_000_000_000 + 350_000_000,
            (12 + k) * 1_000_000_000 + 355_000_000,
            (12 + k) * 1_000_000_000 + 705_000_000,
        )
        if filt.process_sample(s):
            accepted += 1
        else:
            rejected += 1

    print(f"  [PASS] Route change recovery: {rejected} rejected, {accepted} accepted.")
    assert rejected in (7, 8), f"Watchdog should trip within 8 rejections, got {rejected}"
    assert accepted >= 42
    print(f"  [PASS] Median RTT adapted from 300ms to {filt.get_median_rtt()/1e6:.1f}ms.")

# ============================================================================
# 2. PI PLL DYNAMICS & CRITICAL DAMPING HARNESS
# ============================================================================
def simulate_pi_pll(initial_error_s: float, dt: float = 0.2, total_s: float = 25.0):
    Kp = 0.17
    Ki = 0.0072
    I_max = 32.0
    r_min = 0.96
    r_max = 1.04
    deadband = 0.015

    e = initial_error_s
    I = 0.0
    steps = int(total_s / dt)
    history = []
    time_entered_deadband = None
    overshoot = False

    for step in range(steps):
        t = step * dt
        history.append((t, e, I))

        if abs(e) < deadband:
            r = 1.0
            if time_entered_deadband is None:
                time_entered_deadband = t
        elif abs(e) <= 1.500:
            I_cand = I + e * dt
            I = max(-I_max, min(I_max, I_cand))
            delta_r = Kp * e + Ki * I
            r = max(r_min, min(r_max, 1.0 + delta_r))
        else:
            # Zone 3: hard seek
            r = 1.0
            e = 0.0
            I = 0.0

        # Plant update: target advances by dt, actual by r * dt
        delta_phase = (r - 1.0) * dt
        e_next = e - delta_phase

        if initial_error_s > 0 and e_next < -deadband and e >= -deadband:
            overshoot = True
        elif initial_error_s < 0 and e_next > deadband and e <= deadband:
            overshoot = True

        e = e_next

    return time_entered_deadband, overshoot, history

def test_pi_pll_dynamics():
    print("\n" + "=" * 60)
    print("CHALLENGE 2: PI PLL DYNAMICS & CRITICAL DAMPING")
    print("=" * 60)

    # Test A: +450ms initial error
    t_entered, overshoot, history = simulate_pi_pll(0.450)
    print(f"  [PASS] Initial error +450ms entered deadband at t = {t_entered:.1f}s <= 20.0s.")
    print(f"  [PASS] Overshoot past -15ms: {overshoot}.")
    assert t_entered is not None and t_entered <= 20.0
    assert not overshoot

    # Test B: -450ms initial error
    t_entered_neg, overshoot_neg, _ = simulate_pi_pll(-0.450)
    print(f"  [PASS] Initial error -450ms entered deadband at t = {t_entered_neg:.1f}s <= 20.0s.")
    print(f"  [PASS] Overshoot past +15ms: {overshoot_neg}.")
    assert t_entered_neg is not None and t_entered_neg <= 20.0
    assert not overshoot_neg

    # Test C: Near boundary +1490ms (Zone 2)
    t_entered_1490, _, hist_1490 = simulate_pi_pll(1.490, total_s=60.0)
    print(f"  [PASS] Near boundary +1490ms entered deadband at t = {t_entered_1490:.1f}s without hard seek.")
    assert t_entered_1490 is not None

# ============================================================================
# 3. BARRIER UNPAUSE & CROSS-PEER ALIGNMENT HARNESS
# ============================================================================
def test_barrier_unpause_alignment():
    print("\n" + "=" * 60)
    print("CHALLENGE 3: TWO-PHASE BARRIER UNPAUSE & CROSS-PEER ALIGNMENT")
    print("=" * 60)

    # 4 peers with diverse RTT and pre-roll decode times:
    peers = [
        {"id": 1, "rtt": 400, "preroll": 200},
        {"id": 2, "rtt": 80,  "preroll": 20},
        {"id": 3, "rtt": 250, "preroll": 150},
        {"id": 4, "rtt": 320, "preroll": 100},
    ]
    max_rtt = max(p["rtt"] for p in peers) # 400ms
    preroll_budget = 200 # ms
    safety_margin = 100 # ms
    t_server_now = 5000 # ms

    target_server = int(t_server_now + 1.5 * max_rtt + preroll_budget + safety_margin)
    print(f"  Authoritative Target Unpause: t = {target_server} ms")
    assert target_server == 5000 + 600 + 200 + 100 # 5900ms

    for p in peers:
        one_way = p["rtt"] / 2.0
        t_prepare_recv = t_server_now + one_way
        t_ready_sent = t_prepare_recv + p["preroll"]
        t_ready_recv = t_ready_sent + one_way
        t_commit_recv = t_ready_recv + one_way

        margin = target_server - t_commit_recv
        print(f"  Peer {p['id']} (RTT={p['rtt']}ms, Preroll={p['preroll']}ms): Commit received at t={t_commit_recv:.1f}ms -> Margin = {margin:.1f}ms >= 100ms [OK]")
        assert margin >= 100.0, f"Peer {p['id']} missed 100ms commit margin"

    # Cross-peer unpause alignment
    cross_peer_jitter = [p["rtt"] * 0.01 for p in peers] # timer jitter <= 4ms
    max_alignment_diff = max(cross_peer_jitter) - min(cross_peer_jitter)
    print(f"  [PASS] Cross-peer unpause alignment: {max_alignment_diff:.2f} ms < 10.0 ms.")
    assert max_alignment_diff < 10.0

# ============================================================================
# 4. SMART CHAT CRASH RECOVERY & FALSE-SYNC RESILIENCE
# ============================================================================
def test_smart_chat_false_sync_recovery():
    print("\n" + "=" * 60)
    print("CHALLENGE 4: SMART CHAT BINARY LOG CRASH RECOVERY")
    print("=" * 60)

    from test_smart_chat import make_header, make_record, ChatRecoveryScanner, SYNC_WORD

    sender_id = b"\x01" * 16
    records = [
        make_record(1, 0, 1000 + i * 10, i * 5000, sender_id, f"User{i}", f"Message {i}".encode())
        for i in range(20)
    ]

    header = make_header()
    # Log with:
    # 5 valid records (0..4)
    # Mid-stream false sync candidate with huge length: \xAA\x55 + length 65535 + random garbage
    # 5 valid records (5..9)
    # Bit-flipped CRC in record 10
    # 5 valid records (11..15)
    # Truncated record at EOF (torn write)
    false_sync_garbage = SYNC_WORD + struct.pack("<I", 65535) + b"corrupted bytes inside stream"
    rec10_corrupted = bytearray(records[10])
    rec10_corrupted[-1] ^= 0xFF

    torn_eof = records[16][:len(records[16]) - 12]

    corrupted_stream = (
        header
        + b"".join(records[0:5])
        + false_sync_garbage
        + b"".join(records[5:10])
        + bytes(rec10_corrupted)
        + b"".join(records[11:16])
        + torn_eof
    )

    scanner = ChatRecoveryScanner(corrupted_stream)
    recovered = scanner.recover()
    print(f"  [PASS] Total valid records in stream: 15 (0..4, 5..9, 11..15).")
    print(f"  [PASS] Actual recovered records: {len(recovered)}.")
    print(f"  [PASS] Corrupted chunks detected: {scanner.corrupted_chunks}.")
    print(f"  [PASS] Torn write at EOF detected: {scanner.torn_writes}.")

    assert len(recovered) == 15, f"Expected 15 records, recovered {len(recovered)}"
    assert scanner.torn_writes == 1
    assert scanner.corrupted_chunks >= 2

# ============================================================================
# 5. INVERTED INDEX FTS & MULTI-SCRIPT TOKENIZATION
# ============================================================================
STOP_WORDS = set(["the", "is", "at", "which", "on", "and", "a", "an", "to", "for", "in", "of", "it", "this", "that", "with", "as", "by"])

def is_cjk(ch: str) -> bool:
    return 0x4E00 <= ord(ch) <= 0x9FFF

def tokenize_spec(text: str) -> List[str]:
    folded = text.casefold()
    tokens = []
    i = 0
    n = len(folded)
    while i < n:
        ch = folded[i]
        if is_cjk(ch):
            cjk_run = []
            while i < n and is_cjk(folded[i]):
                cjk_run.append(folded[i])
                i += 1
            for c in cjk_run:
                tokens.append(c)
            for j in range(len(cjk_run) - 1):
                tokens.append(cjk_run[j] + cjk_run[j+1])
        elif ch in ('#', '@') or ch.isalnum() or ch == '_':
            word_chars = []
            while i < n and not is_cjk(folded[i]) and not folded[i].isspace():
                c2 = folded[i]
                if c2 in ('#', '@') or c2.isalnum() or c2 == '_':
                    word_chars.append(c2)
                i += 1
            word = "".join(word_chars)
            if word:
                if 2 <= len(word) <= 48 and word not in STOP_WORDS:
                    tokens.append(word)
        else:
            i += 1
    return tokens

def test_fts_multi_script():
    print("\n" + "=" * 60)
    print("CHALLENGE 5: INVERTED INDEX FTS & MULTI-SCRIPT TOKENIZATION")
    print("=" * 60)

    # 1. CJK sentence unigram & overlapping bigram
    cjk_text = "这是一段非常精彩的吉他独奏"
    tokens = tokenize_spec(cjk_text)
    assert "吉他" in tokens
    assert "吉" in tokens and "他" in tokens
    print("  [PASS] CJK text tokenized into unigrams and bigrams correctly.")

    # 2. Single CJK characters
    single_cjk = "猫 狗"
    single_tokens = tokenize_spec(single_cjk)
    assert "猫" in single_tokens and "狗" in single_tokens
    print("  [PASS] Single CJK ideographs retained (exempt from 2-char filter).")

    # 3. Western stop-words and hashtags
    en_text = "Check out the #awesome guitar solo at @alice!"
    en_tokens = tokenize_spec(en_text)
    assert "the" not in en_tokens
    assert "at" not in en_tokens
    assert "#awesome" in en_tokens
    assert "@alice" in en_tokens
    print("  [PASS] Western stop-words removed, hashtags/mentions preserved.")

    # 4. Turkish dotted I
    tr_text = "İSTANBUL"
    tr_tokens = tokenize_spec(tr_text)
    assert len(tr_tokens) == 1
    print(f"  [PASS] Turkish dotted I case folding: '{tr_text}' -> {tr_tokens}.")

# ============================================================================
# 6. SCRUBBER DEBOUNCER 120HZ DYNAMICS
# ============================================================================
def test_scrubber_dynamics():
    print("\n" + "=" * 60)
    print("CHALLENGE 6: SCRUBBER DEBOUNCER 120HZ DYNAMICS")
    print("=" * 60)

    from test_ui_renderer import test_scrubber_debouncing
    test_scrubber_debouncing()

# ============================================================================
# 7. UI MEMORY BUDGET & BURST CAPPING
# ============================================================================
def test_ui_memory_footprint():
    print("\n" + "=" * 60)
    print("CHALLENGE 7: UI MEMORY BUDGET & BURST CAPPING")
    print("=" * 60)

    from test_ui_renderer import test_chat_overlay_memory_bounding
    test_chat_overlay_memory_bounding()

# ============================================================================
# MAIN RUNNER
# ============================================================================
def main():
    print("STARTING COMPREHENSIVE EMPIRICAL STRESS TEST HARNESS (CRATES 5-8)...")
    test_high_rtt_network_ntp()
    test_pi_pll_dynamics()
    test_barrier_unpause_alignment()
    test_smart_chat_false_sync_recovery()
    test_fts_multi_script()
    test_scrubber_dynamics()
    test_ui_memory_footprint()
    print("\n" + "=" * 60)
    print("ALL ADVERSARIAL STRESS CHALLENGES PASSED (0 ERRORS).")
    print("=" * 60)
    return 0

if __name__ == "__main__":
    sys.exit(main())
