#!/usr/bin/env bash
# Frame-step / queue verification.
#
# Stage 1 (always): planner + queue acceptance tests (no GUI, no media needed).
# Stage 2 (opt-in, needs ffmpeg CLI AND the StepFrame arm wired to frame_step::plan_step):
#   generates a 25 fps clip and checks 5x forward step == +200 ms (+-1 ms) through a core
#   integration test. `syncverse --headless/--ticks` only simulates presentation ticks and
#   cannot receive PlayerCommand::StepFrame, so no CLI-level check is possible; the stage-2
#   test (crates/syncverse-core/tests/frame_step_e2e.rs) must be added by the wiring owner.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo test -p syncverse-core --test frame_step
cargo test -p syncverse-ui --test playlist_queue
cargo test -p syncverse-ui --lib gui::ab_loop

if [[ "${E2E_STEP_CLIP:-0}" == "1" ]]; then
  command -v ffmpeg >/dev/null || { echo "ffmpeg CLI required for E2E_STEP_CLIP=1"; exit 2; }
  if [[ ! -f crates/syncverse-core/tests/frame_step_e2e.rs ]]; then
    echo "SKIP: frame_step_e2e.rs not present (StepFrame wiring not landed)"; exit 0
  fi
  clip="$(mktemp -d)/step25.mkv"
  ffmpeg -loglevel error -f lavfi -i testsrc=size=320x240:rate=25 -t 4 -c:v libx264 -pix_fmt yuv420p "$clip"
  SYNCVERSE_STEP_CLIP="$clip" cargo test -p syncverse-core --test frame_step_e2e -- --nocapture
fi
echo "frame-step checks passed"
