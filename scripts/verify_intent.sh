#!/usr/bin/env bash
# ==============================================================================
# Syncplay-Alt Pi Framework Intent Verification Script
# Authority: AGENTS.md & .pi/harness/intent_matrix.json
# ==============================================================================
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if ! command -v python3 >/dev/null 2>&1; then
    echo "[FAIL] python3 not found in PATH." >&2
    exit 1
fi

python3 scripts/intent_harness.py "$@"
