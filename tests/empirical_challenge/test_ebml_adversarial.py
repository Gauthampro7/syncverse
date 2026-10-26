#!/usr/bin/env python3
"""
Empirical Adversarial Challenge Test Harness for Matroska / EBML Demuxer
Crate: crates/syncplay-demux/src/ebml.rs

Executes tests for:
1. Frame lacing unpacking (Xiph, Fixed, EBML signed deltas, VINT overflows, and reserved ID defect).
2. Cues-based seeking across boundary conditions (before first cue, on cue, between cues, after last cue, 0 cues).
3. Multi-track interleaved packet queue invariants.
"""

import sys
import subprocess

def run_cargo_test(test_name: str = "") -> bool:
    cmd = ["cargo", "test", "-p", "syncplay-demux", "--test", "ebml_adversarial_challenge"]
    if test_name:
        cmd.extend(["--", test_name, "--nocapture"])
    else:
        cmd.extend(["--", "--nocapture"])
    
    env = dict(subprocess.os.environ)
    cargo_bin = subprocess.os.path.expanduser("~/.cargo/bin")
    env["PATH"] = f"{cargo_bin}:{env.get('PATH', '')}"
    
    result = subprocess.run(cmd, capture_output=True, text=True, env=env)
    print(result.stdout)
    if result.returncode != 0:
        print(result.stderr, file=sys.stderr)
        return False
    return True

def main():
    print("==============================================================================")
    print(" EMPIRICAL ADVERSARIAL CHALLENGE: MATROSKA / EBML DEMUXER")
    print(" Target: crates/syncplay-demux/src/ebml.rs")
    print("==============================================================================")

    print("\n>>> [1/3] Running Frame Lacing Adversarial Stress & Defect Tests...")
    ok1 = run_cargo_test("test_lacing")
    if not ok1:
        print("[FAIL] Frame lacing tests failed.")
        sys.exit(1)
    print("[PASS] Frame lacing stress tests completed.")

    print("\n>>> [2/3] Running Cues Seeking Boundary Tests...")
    ok2 = run_cargo_test("test_seeking")
    if not ok2:
        print("[FAIL] Cues seeking tests failed.")
        sys.exit(1)
    print("[PASS] Cues seeking boundary tests completed.")

    print("\n>>> [3/3] Running Multi-Track Queue Invariant Tests...")
    ok3 = run_cargo_test("test_multi_track")
    if not ok3:
        print("[FAIL] Multi-track queue invariant tests failed.")
        sys.exit(1)
    print("[PASS] Multi-track queue invariant tests completed.")

    print("\n==============================================================================")
    print(" ALL EMPIRICAL ADVERSARIAL CHALLENGE TESTS EXECUTED SUCCESSFULLY")
    print("==============================================================================")

if __name__ == "__main__":
    main()
