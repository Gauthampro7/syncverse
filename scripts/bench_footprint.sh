#!/usr/bin/env bash
# Usage: bench_footprint.sh <binary> [--write-baseline]
# Compares footprint against scripts/footprint_baseline_v0.4.0.txt.
# Startup = wall ms of `play <clip> --headless` (headless play is a short simulation that
# exits; it has no first-frame marker and no idle state, so idle RSS/CPU are SKIPped).
set -u
BIN=${1:?binary path}; MODE=${2:-}
DIR=$(cd "$(dirname "$0")" && pwd); BASE="$DIR/footprint_baseline_v0.4.0.txt"
CLIP=/tmp/syncverse_bench_clip.mp4
[ -f "$CLIP" ] || ffmpeg -y -loglevel error -f lavfi -i testsrc2=s=320x240:r=24:d=10 -c:v libx264 -pix_fmt yuv420p "$CLIP" || exit 2
size=$(stat -c %s "$BIN"); needed=$(readelf -d "$BIN" | grep -c NEEDED); ldd_n=$(ldd "$BIN" | wc -l)
ts=()
for _ in 1 2 3 4 5; do
  s=$(date +%s%N); "$BIN" play "$CLIP" --headless >/dev/null 2>&1; e=$(date +%s%N)
  ts+=($(( (e-s)/1000000 )))
done
sorted=($(printf '%s\n' "${ts[@]}" | sort -n)); smin=${sorted[0]}; smed=${sorted[2]}
echo "size_bytes=$size"; echo "needed_count=$needed"; echo "ldd_lines=$ldd_n"
echo "startup_ms_runs=${ts[*]}"; echo "startup_ms_min=$smin"; echo "startup_ms_median=$smed"
echo "idle_rss_kb=UNMEASURABLE"; echo "idle_cpu_pct=UNMEASURABLE"
if [ "$MODE" = "--write-baseline" ]; then
  { echo "size_bytes=$size"; echo "needed_count=$needed"; echo "ldd_lines=$ldd_n"
    echo "startup_ms_min=$smin"; echo "startup_ms_median=$smed"
    echo "# idle_rss_kb/idle_cpu_pct: not measurable; headless play exits after a simulated run"; } > "$BASE"
  echo "baseline written"; exit 0
fi
g() { grep "^$1=" "$BASE" | cut -d= -f2; }
bs=$(g size_bytes); bn=$(g needed_count); bt=$(g startup_ms_median); fail=0
chk() { if [ "$2" = 1 ]; then echo "PASS $1"; else echo "FAIL $1"; fail=1; fi; }
chk "size ($size <= $bs*1.05)" $(( size*100 <= bs*105 ))
chk "needed_count ($needed == $bn)" $(( needed == bn ))
# +10% with 5 ms absolute slack for timer noise on a sub-100ms run
chk "startup_median ($smed ms <= $bt*1.10+5)" $(( smed*100 <= bt*110+500 ))
echo "SKIP idle_cpu (<=1%): not measurable in headless mode"
exit $fail
