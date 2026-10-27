# Syncverse Phase 4 User-Space Dependency Environment
# Usage: source "/home/gautham/syncverse/.deps/env.sh"
export DEPS_DIR="/home/gautham/syncverse/.deps"
export LIBCLANG_PATH="/home/gautham/syncverse/.deps/usr/lib64"
export PKG_CONFIG_PATH="/home/gautham/syncverse/.deps/usr/lib64/pkgconfig:${PKG_CONFIG_PATH:-}"
export BINDGEN_EXTRA_CLANG_ARGS="-I/usr/lib/gcc/x86_64-redhat-linux/16/include"
export RUSTFLAGS="-L /home/gautham/syncverse/.deps/usr/lib64 ${RUSTFLAGS:-}"
