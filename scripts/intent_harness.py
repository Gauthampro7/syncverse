#!/usr/bin/env python3
"""
Syncverse Pi Framework Intent Verification Harness (Phase 4 Realigned)
========================================================================
Authority: AGENTS.md & .pi/harness/intent_matrix.json

Purpose:
  Validates that all foundational user and architectural intents are fulfilled,
  guards against intent drift across agent modifications, and computes the
  quantitative Intent Compliance Score.
"""

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

def find_root_dir() -> Path:
    # Walk upwards from current script until Cargo.toml is found
    cur = Path(__file__).resolve().parent
    for _ in range(5):
        if (cur / "Cargo.toml").exists():
            return cur
        cur = cur.parent
    return Path(__file__).resolve().parent.parent

ROOT_DIR = find_root_dir()
MATRIX_PATH = ROOT_DIR / ".pi" / "harness" / "intent_matrix.json"
REPORT_PATH = ROOT_DIR / ".pi" / "harness" / "intent_report.json"

class Colors:
    GREEN = "\033[92m"
    RED = "\033[91m"
    YELLOW = "\033[93m"
    BLUE = "\033[94m"
    CYAN = "\033[96m"
    BOLD = "\033[1m"
    RESET = "\033[0m"

def log_info(msg):
    print(f"{Colors.CYAN}[INTENT-HARNESS]{Colors.RESET} {msg}")

def log_pass(msg):
    print(f"{Colors.GREEN}[PASS]{Colors.RESET} {msg}")

def log_fail(msg):
    print(f"{Colors.RED}[FAIL]{Colors.RESET} {msg}")

def log_warn(msg):
    print(f"{Colors.YELLOW}[WARN]{Colors.RESET} {msg}")

def run_intent_test(intent, timeout=120):
    cmd = intent["verification"]["command"]
    expected_code = intent["verification"]["expected_exit_code"]
    
    env = os.environ.copy()
    cargo_bin = os.path.expanduser("~/.cargo/bin")
    if cargo_bin not in env.get("PATH", ""):
        env["PATH"] = f"{cargo_bin}:{env.get('PATH', '')}"

    start_time = time.perf_counter()
    try:
        res = subprocess.run(
            cmd,
            shell=True,
            cwd=str(ROOT_DIR),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=env,
            timeout=timeout
        )
        elapsed = time.perf_counter() - start_time
        success = (res.returncode == expected_code)
        output_snippet = (res.stdout + res.stderr)[-1500:] if not success else ""
        return success, res.returncode, elapsed, output_snippet
    except subprocess.TimeoutExpired:
        elapsed = time.perf_counter() - start_time
        return False, -1, elapsed, f"Execution timed out after {timeout}s"
    except Exception as e:
        elapsed = time.perf_counter() - start_time
        return False, -2, elapsed, str(e)

def parse_args():
    parser = argparse.ArgumentParser(
        description="Syncverse Pi Framework Intent Verification Harness"
    )
    parser.add_argument(
        "--intent", "-i",
        type=str,
        default=None,
        help="Run only the specified intent (e.g. INTENT-01, INTENT-05)"
    )
    parser.add_argument(
        "--list", "-l",
        action="store_true",
        help="List all registered intents from matrix and exit"
    )
    parser.add_argument(
        "--matrix",
        type=str,
        default=str(MATRIX_PATH),
        help="Custom path to intent_matrix.json"
    )
    parser.add_argument(
        "--report",
        type=str,
        default=str(REPORT_PATH),
        help="Custom path to output intent_report.json"
    )
    parser.add_argument(
        "--quiet", "-q",
        action="store_true",
        help="Quiet mode: suppress per-test output"
    )
    return parser.parse_args()

def main():
    args = parse_args()
    matrix_file = Path(args.matrix)
    report_file = Path(args.report)

    if not matrix_file.exists():
        log_fail(f"Intent matrix not found at {matrix_file}")
        sys.exit(1)

    with open(matrix_file, "r", encoding="utf-8") as f:
        matrix = json.load(f)

    all_intents = matrix.get("intents", [])

    if args.list:
        print(f"{Colors.BOLD}Registered Intents ({len(all_intents)} total):{Colors.RESET}")
        for intent in all_intents:
            print(f"  {Colors.BOLD}{intent['id']}{Colors.RESET}: {intent['name']}")
            print(f"    Ref: {intent.get('openspec_ref', 'N/A')}")
            print(f"    Command: {intent['verification']['command']}")
            print(f"    Invariants ({len(intent.get('invariants', []))}):")
            for inv in intent.get("invariants", []):
                print(f"      - {inv}")
            print("")
        sys.exit(0)

    if args.intent:
        target_id = args.intent.strip().upper()
        intents = [i for i in all_intents if i["id"].upper() == target_id]
        if not intents:
            log_fail(f"Intent '{args.intent}' not found in matrix. Use --list to see available intents.")
            sys.exit(1)
    else:
        intents = all_intents

    if not args.quiet:
        print(f"{Colors.BOLD}{Colors.BLUE}=============================================================================={Colors.RESET}")
        print(f"{Colors.BOLD}{Colors.BLUE} SYNCVERSE: PI FRAMEWORK INTENT VERIFICATION HARNESS (PHASE 4) {Colors.RESET}")
        print(f"{Colors.BOLD}{Colors.BLUE} Authority: AGENTS.md & .pi/harness/intent_matrix.json {Colors.RESET}")
        print(f"{Colors.BOLD}{Colors.BLUE}=============================================================================={Colors.RESET}\n")
        log_info(f"Loaded {len(intents)} intent(s) to evaluate. Beginning verification...\n")

    total_intents = len(intents)
    passed_intents = 0
    results = []

    for idx, intent in enumerate(intents, 1):
        intent_id = intent["id"]
        name = intent["name"]
        if not args.quiet:
            print(f"--> [{idx}/{total_intents}] Verifying {Colors.BOLD}{intent_id}: {name}{Colors.RESET}")
            print(f"    Ref: {intent.get('openspec_ref', 'N/A')}")
            print(f"    Command: `{intent['verification']['command']}`")
        
        success, code, elapsed, err_snip = run_intent_test(intent)
        
        result_entry = {
            "id": intent_id,
            "name": name,
            "success": success,
            "exit_code": code,
            "duration_s": round(elapsed, 3),
            "invariants_count": len(intent.get("invariants", [])),
            "error_snippet": err_snip if not success else None
        }
        results.append(result_entry)

        if success:
            passed_intents += 1
            if not args.quiet:
                log_pass(f"{intent_id} verified in {elapsed:.2f}s ({len(intent.get('invariants', []))} invariants active)")
        else:
            if not args.quiet:
                log_fail(f"{intent_id} FAILED with code {code} in {elapsed:.2f}s")
                if err_snip:
                    print(f"    {Colors.RED}Details:{Colors.RESET} {err_snip.strip()}")
        if not args.quiet:
            print("")

    compliance_score = (passed_intents / total_intents) * 100.0

    # Write report only if running full suite (not filtered)
    if not args.intent:
        report = {
            "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "total_intents": total_intents,
            "passed_intents": passed_intents,
            "compliance_score_percent": round(compliance_score, 2),
            "status": "PASS" if passed_intents == total_intents else "FAIL",
            "results": results
        }
        report_file.parent.mkdir(parents=True, exist_ok=True)
        with open(report_file, "w", encoding="utf-8") as f:
            json.dump(report, f, indent=2)

    if not args.quiet:
        print(f"{Colors.BOLD}{Colors.BLUE}=============================================================================={Colors.RESET}")
        print(f" INTENT HARNESS SUMMARY:")
        print(f" Total Intents Evaluated: {total_intents}")
        print(f" Passed Intents:          {passed_intents}/{total_intents}")
        print(f" Compliance Score:        {Colors.BOLD}{compliance_score:.1f}%{Colors.RESET}")
        if not args.intent:
            print(f" Audit Report written to: {report_file}")
        print(f"{Colors.BOLD}{Colors.BLUE}=============================================================================={Colors.RESET}")

    if passed_intents == total_intents:
        if not args.quiet:
            log_pass("All Intent Contracts satisfied with 0 drift. Safe to proceed.")
        sys.exit(0)
    else:
        if not args.quiet:
            log_fail(f"Intent Drift detected! {total_intents - passed_intents} intents failed.")
        sys.exit(1)

if __name__ == "__main__":
    main()
