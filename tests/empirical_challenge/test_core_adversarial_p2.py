#!/usr/bin/env python3
"""
Empirical Adversarial Challenge Suite: Milestone 5 Core Engine & CLI Binary (Phase 2)
Author: challenger_p2_core (critic / specialist)

Stress Tests:
1. CLI Banner, Usage, Version, and Error Handling:
   - Zero-arg invocation, help flags (--help, -h, help).
   - Version flags (version, --version, -v).
   - Unknown commands and malformed flags reject with code 1 without panicking.
2. Container Inspection Subcommand (`inspect`):
   - Missing argument rejection.
   - Synthetic target probing: synthetic-mkv, synthetic-mp4, synthetic-full-mkv, synthetic-full-mp4.
   - Non-existent file path: strongly typed error without panic.
   - Empty file, random garbage bytes, directory path: cleanly rejected without panic.
3. Playback Presentation Subcommand (`play`):
   - Default target execution.
   - Synthetic targets execution with Master Clock, Q8 color conversion, SPSC ring buffer, and live OSD HUD.
   - Non-existent file rejection.
   - Corrupt/empty file fallback without panic.
4. Watch-Party UDP Networking Subcommand (`party`):
   - Demo mode (real UDP loopback, NTP probes, PI PLL convergence, barrier unpause).
   - Host mode with default port, custom port, invalid port strings.
   - Connect mode with valid addresses and invalid socket address strings.
5. Smart Chat Persistence & Query Subcommand (`chat`):
   - Demo execution (persistence, FTS, slash command, markdown transcript).
   - Query on missing database cleanly handled.
   - Log append with Unicode, hashtags, CJK, mentions.
   - Multi-term search queries across persistent binary records.
   - Markdown transcript export to file.
6. Verification Subcommand (`verify`):
   - Invocation of Pi Framework Intent Verification Harness through CLI.
"""

import os
import re
import subprocess
import tempfile
import sys

BINARY_PATH = os.path.abspath("target/debug/syncplay-alt")
ANSI_RE = re.compile(r"\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])")


def strip_ansi(text: str) -> str:
    return ANSI_RE.sub("", text)


def run_cmd(args, expect_exit=0):
    cmd = [BINARY_PATH] + args
    res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    if res.returncode != expect_exit:
        print(f"[FAIL] Command {' '.join(cmd)} exited with {res.returncode}, expected {expect_exit}")
        print(f"Stdout:\n{res.stdout}")
        print(f"Stderr:\n{res.stderr}")
        raise AssertionError(f"Exit code mismatch: {res.returncode} != {expect_exit}")
    # Verify no panics occurred
    combined = res.stdout + res.stderr
    assert "panicked at" not in combined, f"Panic detected in {' '.join(cmd)}: {combined}"
    return strip_ansi(res.stdout), strip_ansi(res.stderr)


def test_1_cli_banner_usage_version():
    print(">>> [TEST 1] Testing CLI Banner, Usage, Version, and Unknown Command Handling...")

    # 1A. No arguments -> Banner + Usage, exit 0
    out, _ = run_cmd([], expect_exit=0)
    assert "SYNCPLAY-ALT" in out
    assert "USAGE:" in out

    # 1B. Help flags
    for flag in ["--help", "-h", "help"]:
        out, _ = run_cmd([flag], expect_exit=0)
        assert "USAGE:" in out

    # 1C. Version flags
    for flag in ["version", "--version", "-v"]:
        out, _ = run_cmd([flag], expect_exit=0)
        assert "syncplay-alt v0.1.0" in out
        assert "pure-Rust" in out

    # 1D. Unknown commands
    for bad in ["bogus", "unknown_cmd", "--invalid-flag"]:
        _, err = run_cmd([bad], expect_exit=1)
        assert f"Unknown command '{bad}'" in err

    print("  [PASS] CLI basic invocation, usage, version, and unknown commands verified.")


def test_2_cli_inspect_adversarial():
    print(">>> [TEST 2] Testing 'inspect' subcommand under valid and adversarial inputs...")

    # 2A. Missing argument -> exit 1
    _, err = run_cmd(["inspect"], expect_exit=1)
    assert "Please provide a path" in err

    # 2B. Synthetic targets
    out, _ = run_cmd(["inspect", "synthetic-mkv"])
    assert "EBML/MKV probe match:  YES" in out
    assert "V_VP9" in out

    out, _ = run_cmd(["inspect", "synthetic-mp4"])
    assert "ISOBMFF/MP4 probe match: YES" in out
    assert "avc1" in out

    out, _ = run_cmd(["inspect", "synthetic-full-mkv"])
    assert "Keyframe index (Cues): 2 seek points" in out

    out, _ = run_cmd(["inspect", "synthetic-full-mp4"])
    assert "Keyframe sample tables (stss): 1 sync sample(s)" in out

    # 2C. Non-existent file
    _, err = run_cmd(["inspect", "/tmp/does_not_exist_98765.mkv"], expect_exit=1)
    assert "Inspection failed: Io" in err

    # 2D. Empty file
    with tempfile.NamedTemporaryFile() as tf:
        _, err = run_cmd(["inspect", tf.name], expect_exit=1)
        assert "Unrecognized container format" in err

    # 2E. Garbage bytes file
    with tempfile.NamedTemporaryFile() as tf:
        tf.write(b"GARBAGE_RANDOM_BYTES_WITHOUT_EBML_OR_MP4_BOXES" * 10)
        tf.flush()
        _, err = run_cmd(["inspect", tf.name], expect_exit=1)
        assert "Unrecognized container format" in err

    # 2F. Directory path
    _, err = run_cmd(["inspect", "/tmp"], expect_exit=1)
    assert "Unrecognized container format" in err

    print("  [PASS] 'inspect' correctly validates synthetic containers and rejects corrupt/empty files.")


def test_3_cli_play_adversarial():
    print(">>> [TEST 3] Testing 'play' subcommand under valid and adversarial targets...")

    # 3A. Default play target (synthetic-mkv)
    out, _ = run_cmd(["play"])
    assert "Initializing playback runtime for: synthetic-mkv" in out
    assert "HUD: [00:00:00 / 00:00:01]" in out
    assert "Post-Seek t_pos: 15000.00ms" in out

    # 3B. Explicit synthetic full mkv and mp4
    out, _ = run_cmd(["play", "synthetic-full-mkv"])
    assert "Simulating 5 consecutive ticks with live OSD Telemetry:" in out
    assert "[DONE] Playback runtime verified with lock-free atomic snapshots." in out

    out, _ = run_cmd(["play", "synthetic-full-mp4"])
    assert "[DONE] Playback runtime verified with lock-free atomic snapshots." in out

    # 3C. Non-existent file -> exit 1
    _, err = run_cmd(["play", "/tmp/does_not_exist_112233.mkv"], expect_exit=1)
    assert "Playback failed: Io" in err

    # 3D. Empty and corrupt files -> safely fallback without panic
    with tempfile.NamedTemporaryFile() as tf:
        out, _ = run_cmd(["play", tf.name])
        assert "[DONE] Playback runtime verified with lock-free atomic snapshots." in out

    with tempfile.NamedTemporaryFile() as tf:
        tf.write(b"\xFF\xFE\x00\x01\x02\x03" * 50)
        tf.flush()
        out, _ = run_cmd(["play", tf.name])
        assert "[DONE] Playback runtime verified with lock-free atomic snapshots." in out

    print("  [PASS] 'play' runs Master Clock presentation loop and handles edge targets.")


def test_4_cli_party_adversarial():
    print(">>> [TEST 4] Testing 'party' subcommand networking and fallback modes...")

    # 4A. Default party mode
    out, _ = run_cmd(["party"])
    assert "Launching watch-party synchronization mode: demo" in out
    assert "UDP loopback verified: 8 NTP probes" in out
    assert "Watch-party sync convergence verified" in out

    # 4B. Host mode with custom port
    out, _ = run_cmd(["party", "host", "19999"])
    assert "Starting host server on UDP port 19999..." in out
    assert "[HOST LISTENING] Room ready on port 19999" in out

    # 4C. Host mode with non-numeric port string -> falls back to default 18242
    out, _ = run_cmd(["party", "host", "not_a_port"])
    assert "Starting host server on UDP port 18242..." in out

    # 4D. Connect mode with valid address
    out, _ = run_cmd(["party", "connect", "127.0.0.1:19999"])
    assert "Connecting to watch-party host at 127.0.0.1:19999..." in out
    assert "[CONNECTED] Probe sent to 127.0.0.1:19999." in out

    # 4E. Connect mode with invalid address string -> reports error and runs fallback demo safely
    out, err = run_cmd(["party", "connect", "invalid_address_format"])
    assert "invalid socket address" in err or "invalid socket address" in out
    assert "Watch-party sync convergence verified" in out

    print("  [PASS] 'party' verified for host, connect, and demo modes under valid and malformed addresses.")


def test_5_cli_chat_persistence_and_queries():
    print(">>> [TEST 5] Testing 'chat' subcommand persistence, FTS queries, and export...")

    chat_log = "syncplay_chat.bin"
    if os.path.exists(chat_log):
        os.remove(chat_log)

    # 5A. Demo mode
    out, _ = run_cmd(["chat", "demo"])
    assert "Appended 2 framed records with IEEE 802.3 CRC32 checksums." in out
    assert "Search 'intro': 1 hits" in out
    assert "Search hashtag '#epic': 1 hits" in out
    assert "Search CJK '吉他' (guitar): 1 hits" in out
    assert "Seek { target_pts_ms: 83450 }" in out

    # 5B. Query when no chat log exists
    out, _ = run_cmd(["chat", "query", "anything"])
    assert "No chat log found at syncplay_chat.bin. 0 hits." in out

    # 5C. Export when no chat log exists -> exit 1
    _, err = run_cmd(["chat", "export"], expect_exit=1)
    assert "Chat log syncplay_chat.bin does not exist." in err

    # 5D. Log without argument -> exit 1
    _, err = run_cmd(["chat", "log"], expect_exit=1)
    assert "Please provide a message to log." in err

    # 5E. Query without argument -> exit 1
    _, err = run_cmd(["chat", "query"], expect_exit=1)
    assert "Please provide a search query." in err

    # 5F. Log multiple messages across separate process runs
    run_cmd(["chat", "log", "First message introducing the movie #opening"])
    run_cmd(["chat", "log", "Second message with CJK characters 极佳的电影配乐"])
    run_cmd(["chat", "log", "Third message mention @director about scene"])

    # 5G. Multi-term and specialized queries
    out, _ = run_cmd(["chat", "query", "introducing"])
    assert "Found 1 matching record(s):" in out
    assert "First message introducing" in out

    out, _ = run_cmd(["chat", "query", "#opening"])
    assert "Found 1 matching record(s):" in out

    out, _ = run_cmd(["chat", "query", "电影配乐"])
    assert "Found 1 matching record(s):" in out
    assert "极佳的电影配乐" in out

    out, _ = run_cmd(["chat", "query", "@director"])
    assert "Found 1 matching record(s):" in out

    out, _ = run_cmd(["chat", "query", "completely_absent_token"])
    assert "Found 0 matching record(s):" in out

    # 5H. Export transcript to Markdown file
    with tempfile.NamedTemporaryFile(suffix=".md") as tf:
        out, _ = run_cmd(["chat", "export", tf.name])
        assert f"Transcript exported to {tf.name}" in out
        with open(tf.name, "r", encoding="utf-8") as f:
            content = f.read()
        assert "Syncplay-Alt Room Chat Transcript" in content
        assert "First message introducing" in content
        assert "极佳的电影配乐" in content

    # 5I. Unknown chat subcommand
    out, _ = run_cmd(["chat", "nonexistent_subcmd"])
    assert "Available chat subcommands: demo, log <msg>, query <search_term>, export [file]" in out

    # Clean up
    if os.path.exists(chat_log):
        os.remove(chat_log)

    print("  [PASS] 'chat' persistence, multi-turn logging, FTS queries, and export verified.")


def test_6_cli_verify_intent_harness():
    print(">>> [TEST 6] Testing 'verify' subcommand...")
    out, _ = run_cmd(["verify"])
    assert "Invoking Pi Framework Intent Verification Harness..." in out
    assert "All Intent Contracts verified with 100% compliance." in out
    print("  [PASS] 'verify' runs Intent Harness successfully.")


def main():
    print("==============================================================================")
    print(" EMPIRICAL ADVERSARIAL CHALLENGE SUITE: syncplay-core (CLI & Coordinator) ")
    print("==============================================================================")

    assert os.path.exists(BINARY_PATH), f"Binary {BINARY_PATH} does not exist. Run 'cargo build --bin syncplay-alt' first."

    test_1_cli_banner_usage_version()
    test_2_cli_inspect_adversarial()
    test_3_cli_play_adversarial()
    test_4_cli_party_adversarial()
    test_5_cli_chat_persistence_and_queries()
    test_6_cli_verify_intent_harness()

    print("\n==============================================================================")
    print(" ALL EMPIRICAL CHALLENGES FOR syncplay-core PASSED CLEANLY (0 ERRORS). ")
    print("==============================================================================")


if __name__ == "__main__":
    main()
