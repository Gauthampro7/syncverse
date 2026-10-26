#!/usr/bin/env bash
set -euo pipefail

# Ensure Cargo/Rust toolchain is available in PATH
if ! command -v cargo >/dev/null 2>&1; then
    if [ -f "$HOME/.cargo/env" ]; then
        # shellcheck source=/dev/null
        . "$HOME/.cargo/env"
    fi
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

echo "=============================================================================="
echo " Syncplay-Alt Phase 1 End-to-End Automated Verification Harness"
echo " Authority: ORIGINAL_REQUEST.md & AGENTS.md"
echo "=============================================================================="

# STAGE 1: OpenSpec Specification Suite Audit
echo ""
echo ">>> [STAGE 1/7] Auditing OpenSpec Specification Suite..."
REQUIRED_SPECS=(
    "openspec/project.md"
    "openspec/specs/demuxer/spec.md"
    "openspec/specs/codecs/spec.md"
    "openspec/specs/av-pipeline/spec.md"
    "openspec/specs/sync-engine/spec.md"
    "openspec/specs/smart-chat/spec.md"
    "openspec/specs/ui-renderer/spec.md"
)
for spec in "${REQUIRED_SPECS[@]}"; do
    if [ ! -f "$spec" ]; then
        echo "[FAIL] Missing required OpenSpec file: $spec"
        exit 1
    fi
    # Check for formal Requirement and Scenario blocks
    if ! grep -q "### Requirement:" "$spec" && [ "$spec" != "openspec/project.md" ]; then
        echo "[FAIL] Missing '### Requirement:' blocks in $spec"
        exit 1
    fi
    if ! grep -q "#### Scenario:" "$spec" && [ "$spec" != "openspec/project.md" ]; then
        echo "[FAIL] Missing '#### Scenario:' blocks in $spec"
        exit 1
    fi
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
        echo "[FAIL] Missing required OpenSpec change proposal: $change"
        exit 1
    fi
done
echo "[PASS] Stage 1: OpenSpec specifications and change proposals verified."

# STAGE 2: Pi Coding-Agent Framework Audit
echo ""
echo ">>> [STAGE 2/7] Auditing Pi Coding-Agent Framework..."
REQUIRED_PI_FILES=(
    "AGENTS.md"
    ".pi/agents/spec_implementer.md"
    ".pi/agents/bitstream_tester.md"
    ".pi/agents/sync_simulator.md"
    ".pi/agents/storage_auditor.md"
    ".pi/skills/zero-dependency-audit/SKILL.md"
    ".pi/skills/network-sync-simulation/SKILL.md"
    ".pi/skills/bitstream-vector-testing/SKILL.md"
    ".pi/skills/openspec-change-mapping/SKILL.md"
    ".pi/workflows/001-demuxer-workflow.md"
    ".pi/workflows/002-codecs-workflow.md"
    ".pi/workflows/003-clock-pipeline-workflow.md"
    ".pi/workflows/004-sync-workflow.md"
    ".pi/workflows/005-chat-workflow.md"
    ".pi/workflows/006-ui-workflow.md"
    ".pi/workflows/e2e-verification-workflow.md"
)
for pifile in "${REQUIRED_PI_FILES[@]}"; do
    if [ ! -f "$pifile" ]; then
        echo "[FAIL] Missing required Pi framework file: $pifile"
        exit 1
    fi
done
echo "[PASS] Stage 2: Pi framework prompts, skills, and workflows verified."

# STAGE 3: Zero-Dependency Blacklist Audit
echo ""
echo ">>> [STAGE 3/7] Auditing Dependencies for Banned Multimedia & Database Crates..."
./scripts/audit_dependencies.sh
echo "[PASS] Stage 3: Zero-dependency invariant verified."

# STAGE 4: Workspace Compilation & Static Linting
echo ""
echo ">>> [STAGE 4/7] Checking Workspace Compilation and Lints (-D warnings)..."
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
echo "[PASS] Stage 4: Workspace compilation and clippy lints passed with 0 warnings."

# STAGE 5: Rust Unit & Integration Test Suite
echo ""
echo ">>> [STAGE 5/7] Running Workspace Unit & Integration Test Suite..."
cargo test --workspace -- --nocapture
echo "[PASS] Stage 5: All workspace unit and integration tests passed."

# STAGE 6: Synthetic Bitstream & Storage Tests
echo ""
echo ">>> [STAGE 6/7] Running Synthetic Bitstream & Storage Empirical Challenges..."
python3 tests/empirical_challenge/test_demux_codecs_clock.py
python3 tests/empirical_challenge/test_smart_chat.py
echo "[PASS] Stage 6: Demuxer, codecs, clock, and smart-chat empirical challenges passed."

# STAGE 7: High-RTT Network Sync & UI Tests
echo ""
echo ">>> [STAGE 7/7] Running High-RTT Network Sync, PLL & UI Empirical Challenges..."
python3 tests/empirical_challenge/test_sync_engine.py
python3 tests/empirical_challenge/test_ui_renderer.py
python3 tests/empirical_challenge/test_pi_adversarial_stress.py
python3 tests/empirical_challenge/test_crates_5_to_8_adversarial.py
echo "[PASS] Stage 7: Network sync, PI controller, and UI empirical challenges passed."

echo ""
echo "=============================================================================="
echo " [SUCCESS] Phase 1 Verification Succeeded! All 7 Stages Passed."
echo "=============================================================================="
