# Syncplay-Alt Phase 4 User-Space Dependency Environment
# Usage: source "/home/gautham/syncplay-alt/.deps/env.sh"
export DEPS_DIR="/home/gautham/syncplay-alt/.deps"
export LIBCLANG_PATH="/home/gautham/syncplay-alt/.deps/usr/lib64"
export PKG_CONFIG_PATH="/home/gautham/syncplay-alt/.deps/usr/lib64/pkgconfig:${PKG_CONFIG_PATH:-}"
export BINDGEN_EXTRA_CLANG_ARGS="-I/usr/lib/gcc/x86_64-redhat-linux/16/include"
export RUSTFLAGS="-L /home/gautham/syncplay-alt/.deps/usr/lib64 ${RUSTFLAGS:-}"
