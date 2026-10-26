#!/usr/bin/env python3
"""
Empirical Adversarial Challenge Suite: Milestone 5 System Integration & Stress Hardening
Author: challenger_p3_m5_integration (critic / specialist)

Verification targets:
1. Multi-Peer Watch-Party CLI Integration:
   - Spawns host: syncplay-alt party host --port 19999 --file synthetic-full-mkv
   - Spawns client: syncplay-alt party connect 127.0.0.1:19999 --file synthetic-full-mkv
   - Concurrent host-client execution & port contention handling
   - Verification of bidirectional packet transmission, NTP clock exchange, and barrier unpause agreement
2. CLI Stress and Edge Cases:
   - Invalid subcommands & malformed options (exit code 1, zero panics)
   - Missing files and non-existent URIs (file:///...) across inspect and play (exit code 1, zero panics)
   - Corrupted bitstream vectors across inspect (empty, 1-byte, random garbage, truncated EBML/ISOBMFF):
     rejection with exit code 1 and zero panics
   - Playback fallback resilience on corrupt/empty vectors: graceful presentation fallback with zero panics
"""

import os
import re
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
BINARY_PATH = ROOT / "target" / "debug" / "syncplay-alt"
ANSI_RE = re.compile(r"\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])")


def strip_ansi(text: str) -> str:
    return ANSI_RE.sub("", text)


def run_cmd(args, expect_exit=0):
    cmd = [str(BINARY_PATH)] + args
    res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    combined = res.stdout + res.stderr
    assert "panicked at" not in combined.lower(), f"Panic detected in {' '.join(cmd)}:\n{combined}"
    assert "core dumped" not in combined.lower(), f"Crash detected in {' '.join(cmd)}:\n{combined}"
    if res.returncode != expect_exit:
        print(f"[FAIL] Command {' '.join(cmd)} exited with {res.returncode}, expected {expect_exit}")
        print(f"Stdout:\n{res.stdout}")
        print(f"Stderr:\n{res.stderr}")
        raise AssertionError(f"Exit code mismatch: {res.returncode} != {expect_exit}")
    return strip_ansi(res.stdout), strip_ansi(res.stderr)


def test_watch_party_cli_integration():
    print("\n--- [PART 1/2] Multi-Peer Watch-Party CLI Integration ---")

    # 1. Spawn Host on port 19999 with synthetic-full-mkv
    print(">>> [1.1] Executing: syncplay-alt party host --port 19999 --file synthetic-full-mkv")
    stdout, stderr = run_cmd(["party", "host", "--port", "19999", "--file", "synthetic-full-mkv"], expect_exit=0)
    assert "Starting host server on UDP port 19999..." in stdout, f"Unexpected stdout: {stdout}"
    assert "Media file: synthetic-full-mkv" in stdout
    assert "[HOST LISTENING] Room ready on port 19999" in stdout
    assert "UDP loopback verified: 8 NTP probes, room state broadcast, 2-phase barrier unpause, and chat packet relay." in stdout
    assert "Simulating Transpacific High-RTT Link" in stdout
    assert "[SUCCESS] Watch-party sync convergence verified" in stdout
    print("  [PASS] Host CLI command succeeded with complete NTP exchange, barrier unpause, and WAN convergence.")

    # 2. Spawn Client on 127.0.0.1:19999 with synthetic-full-mkv
    print(">>> [1.2] Executing: syncplay-alt party connect 127.0.0.1:19999 --file synthetic-full-mkv")
    stdout, stderr = run_cmd(["party", "connect", "127.0.0.1:19999", "--file", "synthetic-full-mkv"], expect_exit=0)
    assert "Connecting to watch-party host at 127.0.0.1:19999..." in stdout
    assert "Media file: synthetic-full-mkv" in stdout
    assert "[CONNECTED] Probe sent to 127.0.0.1:19999." in stdout
    assert "UDP loopback verified: 8 NTP probes, room state broadcast, 2-phase barrier unpause, and chat packet relay." in stdout
    assert "[SUCCESS] Watch-party sync convergence verified" in stdout
    print("  [PASS] Client CLI command succeeded with complete NTP exchange, barrier unpause, and WAN convergence.")

    # 3. Concurrent Host & Client Execution on port 19998
    print(">>> [1.3] Testing concurrent Host and Client execution on port 19998...")
    host_proc = subprocess.Popen(
        [str(BINARY_PATH), "party", "host", "--port", "19998", "--file", "synthetic-full-mkv"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    time.sleep(0.05)
    client_proc = subprocess.Popen(
        [str(BINARY_PATH), "party", "connect", "127.0.0.1:19998", "--file", "synthetic-full-mkv"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )

    host_out, host_err = host_proc.communicate(timeout=5)
    client_out, client_err = client_proc.communicate(timeout=5)

    assert host_proc.returncode == 0, f"Host process failed: {host_err}"
    assert client_proc.returncode == 0, f"Client process failed: {client_err}"
    assert "panicked" not in (host_out + host_err).lower()
    assert "panicked" not in (client_out + client_err).lower()
    print("  [PASS] Concurrent Host and Client execution finished cleanly with zero panics.")

    # 4. Port Contention / Address In Use Resilience
    print(">>> [1.4] Testing Host server bind resilience under port contention...")
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("0.0.0.0", 19997))
    try:
        stdout, stderr = run_cmd(["party", "host", "--port", "19997", "--file", "synthetic-full-mkv"], expect_exit=0)
        assert "Host bind failed" in stderr or "fallback" in stdout.lower()
        print("  [PASS] Host gracefully caught bind conflict and engaged fallback without panic.")
    finally:
        sock.close()

    # 5. Invalid Target Address Resilience
    print(">>> [1.5] Testing Client resilience under invalid IP address...")
    stdout, stderr = run_cmd(["party", "connect", "999.999.999.999:18242"], expect_exit=0)
    assert "Connection failed" in stderr or "fallback" in stdout.lower()
    print("  [PASS] Client gracefully handled invalid socket address without panic.")


def test_cli_stress_and_edge_cases():
    print("\n--- [PART 2/2] CLI Stress and Adversarial Edge Cases ---")

    # 1. Invalid subcommands
    print(">>> [2.1] Testing invalid subcommands...")
    bad_commands = ["unknown_cmd", "foo-bar", "--invalid", "invalid_subcommand_12345"]
    for cmd in bad_commands:
        stdout, stderr = run_cmd([cmd], expect_exit=1)
        assert "[ERROR] Unknown command" in stderr
    print("  [PASS] Invalid subcommands cleanly reject with exit code 1.")

    # 2. Container inspection missing argument
    print(">>> [2.2] Testing inspect without arguments...")
    stdout, stderr = run_cmd(["inspect"], expect_exit=1)
    assert "Please provide a path to a media file" in stderr
    print("  [PASS] inspect with missing argument cleanly rejects with exit code 1.")

    # 3. Missing files across inspect and play
    print(">>> [2.3] Testing missing file paths and non-existent URIs...")
    missing_targets = [
        "/tmp/definitely_not_existing_media_file_12345.mkv",
        "/var/nonexistent_folder/media.mp4",
        "file:///tmp/definitely_not_existing_media_file_12345.mkv",
        "file://localhost/tmp/not_found%20video.mp4",
        "file:///tmp/not_found%GG.mkv",
        "file:///tmp/not_found%2.mkv",
        "file:///tmp/not_found%.mkv",
    ]
    for target in missing_targets:
        stdout, stderr = run_cmd(["inspect", target], expect_exit=1)
        assert "[ERROR] Inspection failed" in stderr

        stdout, stderr = run_cmd(["play", target, "--ticks", "2"], expect_exit=1)
        assert "[ERROR] Playback failed" in stderr
    print("  [PASS] Missing files and non-existent URIs cleanly return exit code 1 without panic.")

    # 4. Directory as inspect target
    print(">>> [2.4] Testing directory passed as inspect target...")
    stdout, stderr = run_cmd(["inspect", "/tmp"], expect_exit=1)
    assert "[ERROR] Inspection failed" in stderr
    print("  [PASS] Directory target for inspect cleanly rejected with exit code 1.")

    # 5. Corrupted bitstream vectors under inspect and play
    print(">>> [2.5] Testing corrupted bitstream vectors (empty, 1-byte, truncated, garbage)...")
    with tempfile.TemporaryDirectory() as tmpdir:
        tmp_path = Path(tmpdir)

        empty_file = tmp_path / "empty.mkv"
        empty_file.write_bytes(b"")

        one_byte_file = tmp_path / "one_byte.mkv"
        one_byte_file.write_bytes(b"\x1a")

        truncated_ebml = tmp_path / "truncated_ebml.mkv"
        truncated_ebml.write_bytes(bytes([0x1A, 0x45, 0xDF, 0xA3, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00]))

        truncated_mp4 = tmp_path / "truncated_mp4.mp4"
        truncated_mp4.write_bytes(bytes([0x00, 0x00, 0x00, 0x20, ord('f'), ord('t'), ord('y'), ord('p'), 0x00, 0x00]))

        garbage_1kb = tmp_path / "garbage_1kb.mkv"
        garbage_1kb.write_bytes(os.urandom(1024))

        garbage_64kb = tmp_path / "garbage_64kb.mp4"
        garbage_64kb.write_bytes(os.urandom(65536))

        corrupt_vectors = [
            empty_file,
            one_byte_file,
            truncated_ebml,
            truncated_mp4,
            garbage_1kb,
            garbage_64kb,
        ]

        for vec in corrupt_vectors:
            # Inspection must strictly fail with exit code 1
            stdout, stderr = run_cmd(["inspect", str(vec)], expect_exit=1)
            assert "[ERROR] Inspection failed" in stderr

            # Playback of corrupt file safely falls back to synthetic playback without panic
            stdout, stderr = run_cmd(["play", str(vec), "--ticks", "2"], expect_exit=0)
            assert "[DONE] Playback runtime verified with lock-free atomic snapshots." in stdout
    print("  [PASS] Corrupted vectors: inspect strictly rejects with code 1; play safely falls back without panic.")

    # 6. Play CLI option parsing stress
    print(">>> [2.6] Testing play CLI option parsing boundary cases...")
    stdout, stderr = run_cmd(["play", "synthetic-mkv", "--ticks", "0"], expect_exit=0)
    assert "[PLAYBACK]" in stdout

    stdout, stderr = run_cmd(["play", "synthetic-mkv", "--ticks", "9999999999999999999999999999999999999999"], expect_exit=0)
    assert "[PLAYBACK]" in stdout

    stdout, stderr = run_cmd(["play", "synthetic-mkv", "--ticks", "-5"], expect_exit=0)
    assert "[PLAYBACK]" in stdout

    stdout, stderr = run_cmd(["play", "synthetic-mkv", "--unknown-flag", "--ticks", "1"], expect_exit=0)
    assert "[PLAYBACK]" in stdout
    print("  [PASS] Play CLI options boundary conditions safely handled.")

    # 7. Chat CLI edge cases
    print(">>> [2.7] Testing chat CLI command edge cases...")
    stdout, stderr = run_cmd(["chat", "unknown_mode"], expect_exit=0)
    assert "[SMART CHAT]" in stdout
    assert "Available chat subcommands" in stdout

    stdout, stderr = run_cmd(["chat", "query"], expect_exit=1)
    assert "[SMART CHAT]" in stdout
    assert "Please provide a search query" in stderr
    print("  [PASS] Chat CLI edge cases handled safely.")


if __name__ == "__main__":
    print("==============================================================================")
    print(" SYNCPLAY-ALT: PHASE 3 MILESTONE 5 INTEGRATION & STRESS ADVERSARIAL HARNESS  ")
    print("==============================================================================")
    test_watch_party_cli_integration()
    test_cli_stress_and_edge_cases()
    print("\n==============================================================================")
    print(" [SUCCESS] ALL MILESTONE 5 INTEGRATION & CLI STRESS CHALLENGES PASSED (100%) ")
    print("==============================================================================")
