#!/usr/bin/env bash
set -euo pipefail

echo "=================================================="
echo " Syncplay-Alt Windows Cross-Compilation (Fedora -> Windows)"
echo "=================================================="

# Check for mingw toolchain
if ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
    echo "[NOTICE] mingw64-gcc is required on Fedora for cross-compiling."
    echo "         Run: sudo dnf install -y mingw64-gcc"
    echo ""
fi

export PATH="$HOME/.cargo/bin:$PATH"

echo "[1/2] Ensuring x86_64-pc-windows-gnu target is installed..."
rustup target add x86_64-pc-windows-gnu

echo "[2/2] Compiling Windows release binary..."
cargo build --release --target x86_64-pc-windows-gnu --workspace

echo "=================================================="
echo " [SUCCESS] Windows binary created!"
echo " Location: target/x86_64-pc-windows-gnu/release/syncplay-alt.exe"
echo "=================================================="
