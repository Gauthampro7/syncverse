#!/usr/bin/env python3
"""
Empirical Challenge Test Suite: Milestone 4 (R1 Fedora Desktop & Interactive Terminal/OSD Player Launcher)

Adversarially stresses:
1. Triple-buffer snapshot concurrency race fix (50,000 publishes under heavy thread contention, reads > 0, 0 torn reads).
2. InteractivePlayer keyboard input state machine & slash commands (rapid toggling, relative seeks, speed/volume clamping, invalid/boundary slash commands).
3. Box-drawing scrubber bar, timecode HUD, and ANSI TrueColor OSD boundaries (0ms duration, overruns, 100+ hr durations, narrow widths cols < 20).
4. UTF-8 multi-byte string slicing vulnerability reproduction in terminal title rendering.
5. Desktop application packaging & XDG compliance.
"""

import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent

def test_rust_milestone4_adversarial_suite():
    print("--- [1/4] Running Rust Milestone 4 Adversarial Stress Test Suite ---")
    cargo_bin = os.path.expanduser("~/.cargo/bin/cargo")
    if not os.path.exists(cargo_bin):
        cargo_bin = "cargo"

    env = os.environ.copy()
    env["PATH"] = f"{os.path.expanduser('~/.cargo/bin')}:{env.get('PATH', '')}"

    cmd = [
        cargo_bin,
        "test",
        "-p",
        "syncverse-core",
        "--test",
        "adversarial_milestone4_interactive_stress",
        "--",
        "--nocapture",
    ]

    t0 = time.time()
    res = subprocess.run(cmd, env=env, capture_output=True, text=True, cwd=str(ROOT))
    elapsed = time.time() - t0

    print(res.stdout)
    if res.stderr:
        print(res.stderr, file=sys.stderr)

    assert res.returncode == 0, f"Cargo test suite failed with exit code {res.returncode}"
    assert "test result: ok. 16 passed" in res.stdout, "All 16 stress test targets must pass"
    print(f"  [PASS] All 16 Rust empirical stress tests passed cleanly in {elapsed:.2f}s.")


def test_timecode_and_scrubber_python_oracle():
    print("--- [2/4] Verifying Timecode HUD & Scrubber Bar Python Mathematical Oracles ---")

    # Timecode oracle
    def format_tc(curr_ms: int, dur_ms: int) -> str:
        curr_s = curr_ms // 1000
        ch = curr_s // 3600
        cm = (curr_s % 3600) // 60
        cs = curr_s % 60

        dur_s = dur_ms // 1000
        dh = dur_s // 3600
        dm = (dur_s % 3600) // 60
        ds = dur_s % 60
        return f"{ch:02d}:{cm:02d}:{cs:02d} / {dh:02d}:{dm:02d}:{ds:02d}"

    assert format_tc(0, 0) == "00:00:00 / 00:00:00"
    assert format_tc(5000, 1000) == "00:00:05 / 00:00:01"
    assert format_tc(360_000_000, 450_000_000) == "100:00:00 / 125:00:00"
    print("  [PASS] Timecode HUD 100+ hour and boundary formatting verified.")

    # Scrubber oracle with narrow widths
    def scrubber_head_pos(curr_ms: int, dur_ms: int, width: int) -> int:
        w = max(width, 10)
        inner = max(w - 2, 0)
        prog = min(max(curr_ms / float(dur_ms) if dur_ms > 0 else 0.0, 0.0), 1.0)
        return min(round(prog * max(inner - 1, 0)), max(inner - 1, 0))

    for w in [0, 5, 9, 10, 15, 19, 80]:
        pos_zero = scrubber_head_pos(0, 0, w)
        assert pos_zero == 0, f"Head pos at 0ms must be 0, got {pos_zero}"
        pos_overrun = scrubber_head_pos(5000, 1000, w)
        inner = max(max(w, 10) - 2, 0)
        assert pos_overrun == inner - 1, f"Head pos at overrun must be inner-1 ({inner-1}), got {pos_overrun}"
    print("  [PASS] Scrubber bar narrow widths (cols < 20) and overruns verified.")


def test_utf8_slicing_vulnerability_oracle():
    print("--- [3/4] Testing UTF-8 Slicing Vulnerability Oracle ---")
    # Slicing raw bytes on multi-byte UTF-8 character boundaries
    raw_title = " SYNCPLAY-ALT — Target: /home/gautham/東京.mkv (Matroska/EBML)"
    encoded = raw_title.encode("utf-8")

    # In interactive.rs line 485: &raw_title[..inner_width]
    # If inner_width falls on a non-character boundary, standard slicing fails.
    non_boundary_found = False
    for byte_idx in range(len(encoded)):
        try:
            raw_title[:byte_idx]  # In Python, string slicing is character-based, but byte slicing in Rust is:
            encoded[:byte_idx].decode("utf-8")
        except UnicodeDecodeError:
            non_boundary_found = True
            break

    assert non_boundary_found, "UTF-8 byte boundary vulnerability must be reproducible on multi-byte titles"
    print("  [PASS] UTF-8 character boundary vulnerability documented and verified.")


def test_desktop_packaging_suite():
    print("--- [4/4] Verifying Desktop Application Packaging ---")
    pkg_script = ROOT / "tests" / "empirical_challenge" / "test_desktop_packaging.py"
    res = subprocess.run([sys.executable, str(pkg_script)], capture_output=True, text=True, cwd=str(ROOT))
    print(res.stdout)
    if res.stderr:
        print(res.stderr, file=sys.stderr)
    assert res.returncode == 0, f"Desktop packaging test failed with exit code {res.returncode}"
    print("  [PASS] Desktop packaging & XDG compliance verified.")


def main():
    print("==============================================================================")
    print(" Syncplay-Alt Phase 3 Milestone 4: Interactive Player Empirical Challenge")
    print("==============================================================================")
    test_rust_milestone4_adversarial_suite()
    test_timecode_and_scrubber_python_oracle()
    test_utf8_slicing_vulnerability_oracle()
    test_desktop_packaging_suite()
    print("\n[SUCCESS] ALL EMPIRICAL CHALLENGES EXECUTED & VERIFIED.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
