#!/usr/bin/env bash
# ==============================================================================
# Syncplay-Alt Phase 1 End-to-End Automated Verification Harness
# Authority: ORIGINAL_REQUEST.md (Requirement 4) & AGENTS.md
#
# Validates:
#   Stage 1: OpenSpec specification completeness & Given/When/Then scenarios
#   Stage 2: Pi coding-agent framework integrity (prompts, skills, workflows)
#   Stage 3: Zero forbidden dependencies audit (audit_dependencies.sh)
#   Stage 4: Workspace compilation and clippy zero-warning policy (-D warnings)
#   Stage 5: Workspace unit & integration test suite execution
#   Stage 6: Synthetic test vector parsing (EBML and MP4 headers)
#   Stage 7: Deterministic network synchronization simulation & empirical suite
#   Stage 8: Pi Framework Intent Verification Harness (anti-drift validation)
# ==============================================================================
set -euo pipefail

# Ensure directory resolution handles execution from any directory
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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
echo " Syncplay-Alt Phase 1 End-to-End Automated Verification Harness"
echo " Authority: ORIGINAL_REQUEST.md & AGENTS.md"
echo " Root directory: $ROOT_DIR"
echo "=============================================================================="

# ------------------------------------------------------------------------------
# STAGE 1: OpenSpec Specification Suite Completeness Audit
# ------------------------------------------------------------------------------
echo ""
echo ">>> [STAGE 1/7] Auditing OpenSpec Specification Suite..."

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

REQUIRED_CHANGES=(
    "openspec/changes/001-demuxer-bitstream.md"
    "openspec/changes/002-codecs-presentation.md"
    "openspec/changes/003-clock-pipeline.md"
    "openspec/changes/004-network-sync-protocol.md"
    "openspec/changes/005-smart-chat-storage.md"
    "openspec/changes/006-ui-osd-renderer.md"
)

for change in "${REQUIRED_CHANGES[@]}"; do
    if [ ! -f "$change" ]; then
        echo "[FAIL] Missing required OpenSpec change proposal: $change" >&2
        exit 1
    fi
    echo "  [OK] $change verified"
done
echo "[PASS] Stage 1: OpenSpec specifications and change proposals verified."

# ------------------------------------------------------------------------------
# STAGE 2: Pi Coding-Agent Framework Integrity Audit
# ------------------------------------------------------------------------------
echo ""
echo ">>> [STAGE 2/7] Auditing Pi Coding-Agent Framework..."

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
    ".pi/workflows/001-demuxer-workflow.md"
    ".pi/workflows/002-codecs-workflow.md"
    ".pi/workflows/003-clock-pipeline-workflow.md"
    ".pi/workflows/004-sync-workflow.md"
    ".pi/workflows/005-chat-workflow.md"
    ".pi/workflows/006-ui-workflow.md"
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
# STAGE 3: Zero Forbidden Dependencies Audit
# ------------------------------------------------------------------------------
echo ""
echo ">>> [STAGE 3/7] Auditing Dependencies for Banned Multimedia & Database Crates..."
./scripts/audit_dependencies.sh
echo "[PASS] Stage 3: Zero-dependency invariant verified."

# ------------------------------------------------------------------------------
# STAGE 4: Workspace Compilation & Static Linting (-D warnings)
# ------------------------------------------------------------------------------
echo ""
echo ">>> [STAGE 4/7] Checking Workspace Compilation and Lints (-D warnings)..."
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
echo "[PASS] Stage 4: Workspace compilation and clippy lints passed with 0 warnings."

# ------------------------------------------------------------------------------
# STAGE 5: Rust Unit & Integration Test Suite Execution
# ------------------------------------------------------------------------------
echo ""
echo ">>> [STAGE 5/7] Running Workspace Unit & Integration Test Suite..."
cargo test --workspace -- --nocapture
echo "[PASS] Stage 5: All workspace unit and integration tests passed."

# ------------------------------------------------------------------------------
# STAGE 6: Synthetic Test Vector Parsing (EBML and MP4 Headers)
# ------------------------------------------------------------------------------
echo ""
echo ">>> [STAGE 6/7] Running Synthetic Test Vector Parsing & Demux/Codec/Clock Empirical Suite..."
python3 tests/empirical_challenge/test_demux_codecs_clock.py
echo "[PASS] Stage 6: Synthetic test vector parsing and demux/codec/clock empirical challenges passed."

# ------------------------------------------------------------------------------
# STAGE 7: Deterministic Network Synchronization Simulation & Empirical Suite
# ------------------------------------------------------------------------------
echo ""
echo ">>> [STAGE 7/7] Running Deterministic Network Synchronization & Adversarial Simulation Suite..."
python3 tests/empirical_challenge/test_sync_engine.py
python3 tests/empirical_challenge/test_crates_5_to_8_adversarial.py
python3 tests/empirical_challenge/test_smart_chat.py
python3 tests/empirical_challenge/test_ui_renderer.py
python3 tests/empirical_challenge/test_pi_adversarial_stress.py
echo "[PASS] Stage 7: Deterministic network sync, PLL convergence, barrier unpause, and empirical challenges passed."

# ------------------------------------------------------------------------------
# STAGE 8: Pi Framework Intent Verification Harness (Anti-Drift Validation)
# ------------------------------------------------------------------------------
echo ""
echo ">>> [STAGE 8/8] Running Pi Framework Intent Verification Harness..."
python3 scripts/intent_harness.py
echo "[PASS] Stage 8: 100% Intent Compliance verified. Zero intent drift detected."

echo ""
echo "=============================================================================="
echo " [SUCCESS] Phase 1 Verification Succeeded! All 8 Stages Passed Cleanly."
echo "=============================================================================="
exit 0
