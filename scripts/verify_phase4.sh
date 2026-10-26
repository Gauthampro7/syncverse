#!/usr/bin/env bash
# ==============================================================================
# Syncplay-Alt Phase 4 End-to-End Automated Verification Harness
# Authority: ORIGINAL_REQUEST.md (Phase 4 2026-10-25T20:12:42Z) & AGENTS.md
#
# Validates:
#   Stage 1: OpenSpec specification completeness & Given/When/Then scenarios
#   Stage 2: Pi coding-agent framework integrity (prompts, skills, workflows)
#   Stage 3: Zero forbidden player/database dependency audit (audit_dependencies.sh)
#   Stage 4: Workspace compilation and clippy zero-warning policy (-D warnings)
#   Stage 5: Workspace unit & integration test suite execution
#   Stage 6: Synthetic container demuxing, codecs, and clock empirical suite
#   Stage 7: Deterministic network sync, smart chat, and desktop graphical UI suite
#   Stage 8: Pi Framework Intent Verification Harness (anti-drift validation, 100%)
# ==============================================================================
set -euo pipefail

# Ensure directory resolution finds the workspace root containing Cargo.toml
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$SCRIPT_DIR"
while [ "$ROOT_DIR" != "/" ] && [ ! -f "$ROOT_DIR/Cargo.toml" ]; do
    ROOT_DIR="$(dirname "$ROOT_DIR")"
done
if [ ! -f "$ROOT_DIR/Cargo.toml" ]; then
    ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
fi
cd "$ROOT_DIR"

# Ensure Cargo/Rust toolchain is available in PATH
if ! command -v cargo >/dev/null 2>&1; then
    if [ -f "$HOME/.cargo/env" ]; then
        # shellcheck source=/dev/null
        . "$HOME/.cargo/env"
    fi
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "[FAIL] cargo not found in PATH and $HOME/.cargo/env not present." >&2
    exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
    echo "[FAIL] python3 not found in PATH." >&2
    exit 1
fi

echo "=============================================================================="
echo " Syncplay-Alt Phase 4 End-to-End Automated Verification Harness"
echo " Authority: ORIGINAL_REQUEST.md (Phase 4) & AGENTS.md"
echo " Root directory: $ROOT_DIR"
echo "=============================================================================="

# ------------------------------------------------------------------------------
# STAGE 1: OpenSpec Specification Suite Completeness Audit
# ------------------------------------------------------------------------------
echo ""
echo ">>> [STAGE 1/8] Auditing OpenSpec Specification Suite..."

if [ ! -f "openspec/project.md" ]; then
    echo "[FAIL] Missing master project specification: openspec/project.md" >&2
    exit 1
fi
echo "  [OK] Found openspec/project.md"

REQUIRED_SUBSYSTEM_SPECS=(
    "openspec/specs/demuxer/spec.md"
    "openspec/specs/codecs/spec.md"
    "openspec/specs/av-pipeline/spec.md"
    "openspec/specs/sync-engine/spec.md"
    "openspec/specs/smart-chat/spec.md"
    "openspec/specs/ui-renderer/spec.md"
)

for spec in "${REQUIRED_SUBSYSTEM_SPECS[@]}"; do
    if [ ! -f "$spec" ]; then
        echo "[FAIL] Missing required OpenSpec subsystem specification: $spec" >&2
        exit 1
    fi
    if ! grep -q "### Requirement:" "$spec"; then
        echo "[FAIL] Missing '### Requirement:' blocks in $spec" >&2
        exit 1
    fi
    if ! grep -q "#### Scenario:" "$spec"; then
        echo "[FAIL] Missing '#### Scenario:' blocks in $spec" >&2
        exit 1
    fi
    req_count=$(grep -c "### Requirement:" "$spec")
    scen_count=$(grep -c "#### Scenario:" "$spec")
    echo "  [OK] $spec verified ($req_count requirements, $scen_count scenarios)"
done

echo "[PASS] Stage 1: OpenSpec specifications verified."

# ------------------------------------------------------------------------------
# STAGE 2: Pi Coding-Agent Framework Integrity Audit
# ------------------------------------------------------------------------------
echo ""
echo ">>> [STAGE 2/8] Auditing Pi Coding-Agent Framework..."

REQUIRED_PI_FILES=(
    "AGENTS.md"
    ".pi/agents/spec_implementer.md"
    ".pi/agents/bitstream_tester.md"
    ".pi/agents/sync_simulator.md"
    ".pi/agents/storage_auditor.md"
    ".pi/agents/intent_guardian.md"
    ".pi/skills/zero-dependency-audit/SKILL.md"
    ".pi/skills/network-sync-simulation/SKILL.md"
    ".pi/skills/bitstream-vector-testing/SKILL.md"
    ".pi/skills/openspec-change-mapping/SKILL.md"
    ".pi/skills/intent-verification-harness/SKILL.md"
    ".pi/harness/intent_matrix.json"
    ".pi/workflows/intent-verification-workflow.md"
    ".pi/workflows/e2e-verification-workflow.md"
)

for pifile in "${REQUIRED_PI_FILES[@]}"; do
    if [ ! -e "$pifile" ]; then
        echo "[FAIL] Missing required Pi framework file: $pifile" >&2
        exit 1
    fi
    echo "  [OK] $pifile verified"
done
echo "[PASS] Stage 2: Pi framework prompts, skills, and workflows verified."

# ------------------------------------------------------------------------------
# STAGE 3: Dependency Audit for Banned Monolithic Player & Database Engines
# ------------------------------------------------------------------------------
echo ""
echo ">>> [STAGE 3/8] Auditing Dependencies for Banned Monolithic Player & Database Crates..."
./scripts/audit_dependencies.sh
echo "[PASS] Stage 3: Prohibited player and database dependency audit clean."

# ------------------------------------------------------------------------------
# STAGE 4: Workspace Compilation & Static Linting (-D warnings)
# ------------------------------------------------------------------------------
echo ""
echo ">>> [STAGE 4/8] Checking Workspace Compilation and Lints (-D warnings)..."
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
echo "[PASS] Stage 4: Workspace compilation and clippy lints passed with 0 warnings."

# ------------------------------------------------------------------------------
# STAGE 5: Rust Unit & Integration Test Suite Execution
# ------------------------------------------------------------------------------
echo ""
echo ">>> [STAGE 5/8] Running Workspace Unit & Integration Test Suite..."
cargo test --workspace -- --nocapture
echo "[PASS] Stage 5: All workspace unit and integration tests passed."

# ------------------------------------------------------------------------------
# STAGE 6: Container Demuxing, Codecs & Clock Empirical Suite
# ------------------------------------------------------------------------------
echo ""
echo ">>> [STAGE 6/8] Running Demux/Codec/Clock Empirical Suite..."
python3 tests/empirical_challenge/test_demux_codecs_clock.py
echo "[PASS] Stage 6: Demux/Codec/Clock empirical challenges passed."

# ------------------------------------------------------------------------------
# STAGE 7: Deterministic Sync, Smart Chat & Desktop UI Empirical Suite
# ------------------------------------------------------------------------------
echo ""
echo ">>> [STAGE 7/8] Running Deterministic Sync, Smart Chat & Desktop UI Empirical Suite..."
python3 tests/empirical_challenge/test_sync_engine.py
python3 tests/empirical_challenge/test_crates_5_to_8_adversarial.py
python3 tests/empirical_challenge/test_smart_chat.py
python3 tests/empirical_challenge/test_ui_renderer.py
echo "[PASS] Stage 7: Deterministic network sync, barrier unpause, smart chat, and desktop UI passed."

# ------------------------------------------------------------------------------
# STAGE 8: Pi Framework Intent Verification Harness (Anti-Drift Validation)
# ------------------------------------------------------------------------------
echo ""
echo ">>> [STAGE 8/8] Running Pi Framework Intent Verification Harness..."
python3 scripts/intent_harness.py
echo "[PASS] Stage 8: 100% Intent Compliance verified. Zero intent drift detected."

echo ""
echo "=============================================================================="
echo " [SUCCESS] Phase 4 Verification Succeeded! All 8 Stages Passed Cleanly."
echo "=============================================================================="
exit 0
