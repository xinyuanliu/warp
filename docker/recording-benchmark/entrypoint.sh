#!/usr/bin/env bash
set -euo pipefail

if [[ -n "${GH_TOKEN:-}" ]]; then
  git config --global url."https://x-access-token:${GH_TOKEN}@github.com/".insteadOf "https://github.com/"
fi

resolution="${WARP_RECORDING_BENCHMARK_RESOLUTION:-1920x1080}"
exec xvfb-run -a -s "-screen 0 ${resolution}x24 -nolisten tcp" \
  cargo test --locked --release -p computer_use \
  benchmark_recording_capture_paths -- \
  --ignored --nocapture --test-threads=1
