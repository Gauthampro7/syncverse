#!/usr/bin/env bash
set -euo pipefail

echo "=================================================="
echo " Syncverse Automated Release & Deployment Pipeline"
echo "=================================================="

VERSION=$(grep -m1 '^version = ' Cargo.toml | cut -d '"' -f2)
TAG="v${VERSION}"

echo "Detected workspace version: ${VERSION} (${TAG})"

# 1. Dependency Blacklist Audit
echo "[1/7] Auditing dependencies..."
./scripts/audit_dependencies.sh

# 2. Workspace Compilation & Linting
echo "[2/7] Checking compilation and clippy..."
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings

# 3. Unit and Integration Test Suite
echo "[3/7] Running workspace tests..."
cargo test --workspace -- --nocapture

# 4. Empirical Challenge Suites
echo "[4/7] Running empirical challenge test harnesses..."
python3 tests/empirical_challenge/test_demux_codecs_clock.py
python3 tests/empirical_challenge/test_sync_engine.py
python3 tests/empirical_challenge/test_smart_chat.py
python3 tests/empirical_challenge/test_ui_renderer.py

# 5. Anti-drift Intent Verification
echo "[5/7] Evaluating Pi framework intent harness..."
python3 scripts/intent_harness.py

# 6. Optimized Release Binary Build & Local Installation
echo "[6/7] Building and installing release binary to ~/.local/bin/syncverse..."
./scripts/install_linux.sh

# 7. Git Commit, Tag & Push
echo "[7/7] Staging git artifacts, committing, tagging, and pushing..."
git add -A
if git diff-index --quiet HEAD --; then
    echo "No uncommitted changes."
else
    git commit -m "feat(release): release version ${VERSION} with DSP audio suite and subtitle engine (${TAG})"
fi

if git rev-parse "${TAG}" >/dev/null 2>&1; then
    echo "Tag ${TAG} already exists."
else
    git tag "${TAG}"
fi

echo "Pushing commits and tags to origin..."
git push origin main
git push origin --tags

echo "=================================================="
echo " [SUCCESS] Syncverse ${VERSION} released and deployed!"
echo "=================================================="
