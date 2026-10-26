#!/usr/bin/env bash
set -euo pipefail

echo "=================================================="
echo " Syncplay-Alt Linux Installer (Fedora & Modern Linux)"
echo "=================================================="

# 0. Validate desktop entry specification
echo "[1/5] Validating desktop entry with desktop-file-validate..."
if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate assets/syncplay-alt.desktop
    echo "  [PASS] assets/syncplay-alt.desktop is valid according to Desktop Entry Spec."
else
    echo "  [WARN] desktop-file-validate not found, skipping pre-install check."
fi

# 1. Build release binary
echo "[2/5] Building optimized release binary..."
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release --workspace

# 2. Install executable
echo "[3/5] Installing binary to ~/.local/bin/syncplay-alt..."
mkdir -p ~/.local/bin
install -m 755 target/release/syncplay-alt ~/.local/bin/syncplay-alt

# Symlink to ~/.cargo/bin if present
if [ -d "$HOME/.cargo/bin" ] && [ ! -f "$HOME/.cargo/bin/syncplay-alt" ]; then
    ln -sf "$HOME/.local/bin/syncplay-alt" "$HOME/.cargo/bin/syncplay-alt" || true
fi

# 3. Install icon and desktop file
echo "[4/5] Installing desktop entry and scalable vector icon..."
mkdir -p ~/.local/share/applications ~/.local/share/icons/hicolor/scalable/apps
install -m 644 assets/syncplay-alt.svg ~/.local/share/icons/hicolor/scalable/apps/syncplay-alt.svg
install -m 644 assets/syncplay-alt.desktop ~/.local/share/applications/syncplay-alt.desktop

# Validate installed desktop file
if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate "$HOME/.local/share/applications/syncplay-alt.desktop"
    echo "  [PASS] Installed desktop file verified."
fi

# 4. Update desktop & icon caches
echo "[5/5] Updating desktop database and icon cache..."
update-desktop-database ~/.local/share/applications/ 2>/dev/null || true
gtk-update-icon-cache -f -t ~/.local/share/icons/hicolor 2>/dev/null || true

echo "=================================================="
echo " [SUCCESS] Syncplay-Alt successfully installed!"
echo " Binary:  ~/.local/bin/syncplay-alt"
echo " Launch:  syncplay-alt play synthetic-mkv"
echo " Desktop: Press Super -> search 'Syncplay-Alt'"
echo "=================================================="
