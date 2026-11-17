#!/usr/bin/env bash
# Benchmark software vs VA-API decode CPU time (decode + conversion to RGBA, like the player does).
# Usage: scripts/bench_hwdec.sh clip.mkv [seconds-of-media=30]
# Prints user+sys CPU seconds for SYNCVERSE_HWDEC=off and =vaapi and the ratio.
# NOTE: this is hardware decode with CPU readback (not GPU zero-copy); swscale NV12->RGBA still
# runs on the CPU, so the ratio is expected to be well above the raw decoder-only saving.
set -euo pipefail
CLIP="${1:?usage: bench_hwdec.sh clip [seconds]}"
SECS="${2:-30}"
cd "$(dirname "$0")/.."
BIN=$(cargo test --release -p syncverse-codecs --test hwaccel_vaapi --no-run 2>&1 \
      | grep -o 'target/release/deps/hwaccel_vaapi-[0-9a-f]*' | tail -1)
[ -x "$BIN" ] || { echo "could not locate benchmark binary" >&2; exit 1; }
TIMEBIN=/usr/bin/time
[ -x "$TIMEBIN" ] || { echo "$TIMEBIN not found (dnf install time)" >&2; exit 1; }

run() {
  local policy="$1" out
  out=$( { SYNCVERSE_HWDEC="$policy" BENCH_CLIP="$CLIP" BENCH_SECS="$SECS" \
      "$TIMEBIN" -f 'CPU %U %S wall %e' "$BIN" bench_decode_clip --ignored --nocapture --test-threads=1 2>&1; } )
  local info cpu user sys
  info=$(echo "$out" | grep '^BENCH' | head -1)
  cpu=$(echo "$out" | grep '^CPU ' | tail -1)
  user=$(echo "$cpu" | awk '{print $2}'); sys=$(echo "$cpu" | awk '{print $3}')
  echo "[$policy] $info" >&2
  echo "[$policy] user=${user}s sys=${sys}s wall=$(echo "$cpu" | awk '{print $5}')s" >&2
  awk -v u="$user" -v s="$sys" 'BEGIN{printf "%.2f", u+s}'
}
OFF=$(run off)
VA=$(run vaapi)
echo "software CPU (user+sys): ${OFF}s"
echo "vaapi    CPU (user+sys): ${VA}s"
awk -v o="$OFF" -v v="$VA" 'BEGIN{ if (o>0) printf "vaapi/software CPU ratio: %.1f%% (target <= 60%% for 1080p H.264)\n", 100*v/o }'
