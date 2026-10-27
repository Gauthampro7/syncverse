#!/usr/bin/env bash
set -euo pipefail

echo "=================================================="
echo " Syncverse Linux Installer (Fedora & Modern Linux)"
echo "=================================================="

# 0. Validate desktop entry specification
echo "[1/5] Validating desktop entry with desktop-file-validate..."
if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate assets/syncverse.desktop
    echo "  [PASS] assets/syncverse.desktop is valid according to Desktop Entry Spec."
else
    echo "  [WARN] desktop-file-validate not found, skipping pre-install check."
fi

# 1. Build release binary
echo "[2/5] Building optimized release binary..."
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release --workspace

# 2. Install executable
echo "[3/5] Installing binary to ~/.local/bin/syncverse..."
mkdir -p ~/.local/bin
install -m 755 target/release/syncverse ~/.local/bin/syncverse

# Symlink to ~/.cargo/bin if present
if [ -d "$HOME/.cargo/bin" ] && [ ! -f "$HOME/.cargo/bin/syncverse" ]; then
    ln -sf "$HOME/.local/bin/syncverse" "$HOME/.cargo/bin/syncverse" || true
fi

# 3. Install icon and desktop file
echo "[4/5] Installing desktop entry and scalable vector icon..."
mkdir -p ~/.local/share/applications ~/.local/share/icons/hicolor/scalable/apps
install -m 644 assets/syncverse.svg ~/.local/share/icons/hicolor/scalable/apps/syncverse.svg
install -m 644 assets/syncverse.desktop ~/.local/share/applications/syncverse.desktop

# Validate installed desktop file
if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate "$HOME/.local/share/applications/syncverse.desktop"
    echo "  [PASS] Installed desktop file verified."
fi

# 4. Update desktop & icon caches
echo "[5/5] Updating desktop database and icon cache..."
update-desktop-database ~/.local/share/applications/ 2>/dev/null || true
gtk-update-icon-cache -f -t ~/.local/share/icons/hicolor 2>/dev/null || true

echo "=================================================="
echo " [SUCCESS] Syncverse successfully installed!"
echo " Binary:  ~/.local/bin/syncverse"
echo " Launch:  syncverse play synthetic-mkv"
echo " Desktop: Press Super -> search 'Syncverse'"
echo "=================================================="
