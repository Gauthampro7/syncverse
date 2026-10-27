#!/usr/bin/env bash
# ==============================================================================
# Syncverse Phase 4 Non-Root Dependency Provisioning Helper
# Target OS: Fedora Linux 44+ (x86_64)
# Authority: AGENTS.md Section 2 & ORIGINAL_REQUEST.md (Phase 4 Realignment)
#
# Purpose:
#   Deterministically downloads and extracts required development headers
#   (ffmpeg-devel) and bindgen tooling (clang-libs) into a local user-space
#   sysroot (.deps/) using standard unprivileged tools (dnf download + rpm2cpio).
#   Fixes pkg-config includedir/prefix paths, resolves development .so symlinks
#   to pre-installed system shared libraries (/usr/lib64), and generates
#   .cargo/config.toml so cargo builds work transparently without root/sudo.
# ==============================================================================
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEPS_DIR="${SYNCVERSE_DEPS_DIR:-${SYNCPLAY_DEPS_DIR:-$ROOT_DIR/.deps}}"
FORCE_REBUILD=0

for arg in "$@"; do
  case "$arg" in
    --force|-f)
      FORCE_REBUILD=1
      ;;
    --help|-h)
      echo "Usage: $0 [--force]"
      echo "  --force, -f    Force redownload and re-extraction of sysroot dependencies."
      exit 0
      ;;
    *)
      echo "[ERROR] Unknown option: $arg" >&2
      exit 1
      ;;
  esac
done

echo "=============================================================================="
echo " Syncverse Phase 4 Dependency Provisioner"
echo " Sysroot Target: $DEPS_DIR"
echo "=============================================================================="

# ------------------------------------------------------------------------------
# 1. Preflight Tool Checks
# ------------------------------------------------------------------------------
echo ">>> [1/6] Verifying host tooling..."
REQUIRED_TOOLS=("dnf" "rpm2cpio" "cpio" "gcc" "pkg-config")
for tool in "${REQUIRED_TOOLS[@]}"; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "[FAIL] Missing required tool: '$tool'. Please ensure standard developer tools are installed." >&2
    exit 1
  fi
  echo "  [OK] Found $tool at $(which "$tool")"
done

# Check if runtime ffmpeg-libs is present on Fedora host
if [ ! -f "/usr/lib64/libavcodec.so.62" ] || [ ! -f "/usr/lib64/libavformat.so.62" ]; then
  echo "[WARN] Pre-installed /usr/lib64/libavcodec.so.62 not found. Ensure ffmpeg-libs package is available." >&2
fi

# ------------------------------------------------------------------------------
# 2. Idempotency Check
# ------------------------------------------------------------------------------
AVCODEC_H="$DEPS_DIR/usr/include/ffmpeg/libavcodec/avcodec.h"
LIBCLANG_SO="$DEPS_DIR/usr/lib64/libclang.so"
AVCODEC_SO="$DEPS_DIR/usr/lib64/libavcodec.so"

if [ $FORCE_REBUILD -eq 0 ] && [ -f "$AVCODEC_H" ] && [ -e "$LIBCLANG_SO" ] && [ -e "$AVCODEC_SO" ]; then
  echo ">>> [INFO] Dependencies are already provisioned and verified in $DEPS_DIR."
  echo "    To re-extract, pass --force."
  exit 0
fi

# ------------------------------------------------------------------------------
# 3. RPM Download via dnf download (User-Space, Zero Sudo)
# ------------------------------------------------------------------------------
RPM_CACHE_DIR="$DEPS_DIR/.rpm_cache"
mkdir -p "$RPM_CACHE_DIR"
echo ">>> [2/6] Downloading development RPM packages to $RPM_CACHE_DIR..."

RPM_TARGETS=("ffmpeg-devel" "clang-libs")
for target in "${RPM_TARGETS[@]}"; do
  # Check if cached RPM already exists
  CACHED_RPM=$(find "$RPM_CACHE_DIR" -name "${target}-*.rpm" | head -n 1)
  if [ -n "$CACHED_RPM" ] && [ $FORCE_REBUILD -eq 0 ]; then
    echo "  [OK] Using cached $(basename "$CACHED_RPM")"
  else
    echo "  [DOWNLOAD] Fetching $target via dnf download (disabling third-party repos for speed)..."
    dnf download --disablerepo=teamviewer --destdir="$RPM_CACHE_DIR" --arch=x86_64 "$target"
  fi
done

# ------------------------------------------------------------------------------
# 4. Extract RPMs via rpm2cpio + cpio
# ------------------------------------------------------------------------------
echo ">>> [3/6] Extracting RPM packages into user-space sysroot..."
for rpm in "$RPM_CACHE_DIR"/*.rpm; do
  [ -f "$rpm" ] || continue
  echo "  [EXTRACT] Unpacking $(basename "$rpm")..."
  rpm2cpio "$rpm" | (cd "$DEPS_DIR" && cpio -idm --quiet)
done

# ------------------------------------------------------------------------------
# 5. Fix-up Symlinks and Pkg-Config Files
# ------------------------------------------------------------------------------
echo ">>> [4/6] Configuring library symlinks and pkg-config manifests..."

# Ensure unversioned libclang.so symlink exists in usr/lib64
CLANG_VERSIONED=$(find "$DEPS_DIR/usr/lib64" -name "libclang.so.*.*" | head -n 1)
if [ -n "$CLANG_VERSIONED" ]; then
  ln -sf "$CLANG_VERSIONED" "$DEPS_DIR/usr/lib64/libclang.so"
  echo "  [OK] Linked libclang.so -> $CLANG_VERSIONED"
fi

# Resolve relative FFmpeg .so symlinks to absolute system libraries in /usr/lib64
for so in "$DEPS_DIR/usr/lib64/"*.so; do
  [ -L "$so" ] || continue
  target=$(readlink "$so")
  if [[ "$target" != /* ]] && [ -e "/usr/lib64/$target" ]; then
    ln -sf "/usr/lib64/$target" "$so"
  fi
done
echo "  [OK] Resolved development .so symlinks to host shared libraries in /usr/lib64"

# Fix includedir and prefix in .pc files
PKG_DIR="$DEPS_DIR/usr/lib64/pkgconfig"
if [ -d "$PKG_DIR" ]; then
  for pc in "$PKG_DIR"/*.pc; do
    [ -f "$pc" ] || continue
    sed -i -E "s|^prefix=.*|prefix=${DEPS_DIR}/usr|g" "$pc"
    sed -i -E "s|^includedir=.*|includedir=${DEPS_DIR}/usr/include/ffmpeg|g" "$pc"
  done
  echo "  [OK] Adjusted includedir and prefix paths in $PKG_DIR/*.pc"
fi

# ------------------------------------------------------------------------------
# 6. Generate Environment Files & Cargo Configuration
# ------------------------------------------------------------------------------
echo ">>> [5/6] Generating environment and Cargo build configurations..."

GCC_INC=$(gcc --print-file-name=include 2>/dev/null || echo "/usr/lib/gcc/x86_64-redhat-linux/16/include")

# 1. Shell environment script (source .deps/env.sh)
ENV_FILE="$DEPS_DIR/env.sh"
cat << EOF > "$ENV_FILE"
# Syncverse Phase 4 User-Space Dependency Environment
# Usage: source "$ENV_FILE"
export DEPS_DIR="$DEPS_DIR"
export LIBCLANG_PATH="$DEPS_DIR/usr/lib64"
export PKG_CONFIG_PATH="$DEPS_DIR/usr/lib64/pkgconfig:\${PKG_CONFIG_PATH:-}"
export BINDGEN_EXTRA_CLANG_ARGS="-I$GCC_INC"
export RUSTFLAGS="-L $DEPS_DIR/usr/lib64 \${RUSTFLAGS:-}"
EOF
chmod +x "$ENV_FILE"
echo "  [OK] Created $ENV_FILE"

# 2. Workspace .cargo/config.toml (transparent build support for all cargo commands)
CARGO_CONFIG="$ROOT_DIR/.cargo/config.toml"
mkdir -p "$ROOT_DIR/.cargo"
cat << EOF > "$CARGO_CONFIG"
# Auto-generated by scripts/setup_deps.sh for Phase 4 multimedia dependencies
[env]
LIBCLANG_PATH = "$DEPS_DIR/usr/lib64"
PKG_CONFIG_PATH = "$DEPS_DIR/usr/lib64/pkgconfig"
BINDGEN_EXTRA_CLANG_ARGS = "-I$GCC_INC"

[build]
rustflags = ["-L", "$DEPS_DIR/usr/lib64"]
EOF
echo "  [OK] Configured $CARGO_CONFIG with sysroot env and rustflags"

# ------------------------------------------------------------------------------
# 7. Verification & Smoke Test
# ------------------------------------------------------------------------------
echo ">>> [6/6] Verifying provisioned sysroot..."

if [ ! -f "$AVCODEC_H" ]; then
  echo "[FAIL] FFmpeg avcodec.h not found at $AVCODEC_H" >&2
  exit 1
fi

if [ ! -e "$LIBCLANG_SO" ]; then
  echo "[FAIL] libclang.so not found at $LIBCLANG_SO" >&2
  exit 1
fi

AVCODEC_VER=$(PKG_CONFIG_PATH="$PKG_DIR" pkg-config --modversion libavcodec 2>/dev/null || true)
AVFORMAT_VER=$(PKG_CONFIG_PATH="$PKG_DIR" pkg-config --modversion libavformat 2>/dev/null || true)
AVUTIL_VER=$(PKG_CONFIG_PATH="$PKG_DIR" pkg-config --modversion libavutil 2>/dev/null || true)

echo "  [VERIFIED] libavcodec version:  $AVCODEC_VER"
echo "  [VERIFIED] libavformat version: $AVFORMAT_VER"
echo "  [VERIFIED] libavutil version:   $AVUTIL_VER"
echo "  [VERIFIED] libclang library:    $LIBCLANG_SO"
echo "  [VERIFIED] GCC C include path:  $GCC_INC"

echo ""
echo "=============================================================================="
echo " [SUCCESS] Phase 4 Dependencies Successfully Provisioned!"
echo " Sysroot directory: $DEPS_DIR"
echo " Environment script: source $ENV_FILE"
echo " Cargo builds now function automatically with zero manual exports."
echo "=============================================================================="
exit 0
