#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runs=3
output_root="$repo_root/artifact-preview-scroll-benchmark-results"
binary="$repo_root/target/release/usher"
build_binary=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --runs) runs="${2:?missing value for --runs}"; shift 2 ;;
    --output) output_root="${2:?missing value for --output}"; shift 2 ;;
    --binary) binary="${2:?missing value for --binary}"; shift 2 ;;
    --no-build) build_binary=0; shift ;;
    *) echo "error: unknown option: $1" >&2; exit 1 ;;
  esac
done

[[ "$(uname -s)" == "Darwin" ]] || { echo "error: macOS required" >&2; exit 1; }
[[ "$runs" =~ ^[1-9][0-9]*$ ]] && (( runs >= 3 )) || {
  echo "error: at least three runs are required" >&2; exit 1;
}
if [[ "$binary" != /* ]]; then binary="$repo_root/$binary"; fi
[[ "$binary" == */release/* ]] || { echo "error: release binary required" >&2; exit 1; }

cd "$repo_root"
if (( build_binary == 1 )); then cargo build --release --locked; fi
[[ -x "$binary" ]] || { echo "error: missing binary: $binary" >&2; exit 1; }

timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
result_dir="$output_root/$timestamp"
mkdir -p "$result_dir/runs"
window_control="$result_dir/macos-window-control"
swiftc -O -module-cache-path /tmp/usher-preview-scroll-swift-cache \
  "$repo_root/scripts/macos-window-control.swift" -o "$window_control"
printf 'key\tvalue\nfixture\tartifact-preview-markdown-256k-371-blocks-v1\nruns\t%s\nresize_input\tdocked divider +180/-180 px, 160 steps per leg, 8 ms\nscroll_input\t-120 pixel wheel delta, 420 events, 8 ms\n' \
  "$runs" > "$result_dir/metadata.tsv"

active_pid=""
active_control_file=""
cleanup() {
  if [[ -n "$active_pid" ]] && kill -0 "$active_pid" 2>/dev/null; then
    kill -TERM "$active_pid" 2>/dev/null || true
    wait "$active_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

wait_for_log() {
  local pid="$1" log="$2" pattern="$3" attempts=0
  while (( attempts < 240 )); do
    kill -0 "$pid" 2>/dev/null || return 1
    grep -a -q "$pattern" "$log" && return 0
    sleep 0.1
    ((attempts += 1))
  done
  return 1
}

stop_process() {
  local pid="$1" attempts=0
  kill -TERM "$pid"
  while kill -0 "$pid" 2>/dev/null && (( attempts < 100 )); do
    sleep 0.1
    ((attempts += 1))
  done
  if kill -0 "$pid" 2>/dev/null; then
    kill -KILL "$pid"
  fi
  wait "$pid" || true
  active_pid=""
}

for run in $(seq 1 "$runs"); do
  for variant in baseline optimized; do
    trace="$result_dir/runs/$variant-run-$run.tsv"
    log="$result_dir/runs/$variant-run-$run.log"
    active_control_file="$result_dir/runs/$variant-run-$run.control"
    : > "$active_control_file"
    echo "Running Preview scroll variant=$variant run=$run/$runs ..."
    USHER_MEMORY_DIAGNOSTICS=1 \
      USHER_MEMORY_SCENARIO=chat-preview-docked \
      USHER_MEMORY_BENCHMARK_RUN="$run" \
      USHER_MEMORY_WINDOW_CONTROL=signal-file-v1 \
      USHER_MEMORY_WINDOW_CONTROL_FILE="$active_control_file" \
      USHER_BENCHMARK_INSTANCE_NAMESPACE="preview-scroll-$timestamp" \
      USHER_PREVIEW_PERF_VARIANT=optimized \
      USHER_PREVIEW_SCROLL_PERF_VARIANT="$variant" \
      USHER_PREVIEW_PERF_TRACE="$trace" \
      USHER_PREVIEW_PERF_TRACE_WARMUP_MS=0 \
      SLINT_BACKEND=winit-femtovg \
      RUST_LOG=info \
      "$binary" > "$log" 2>&1 &
    active_pid=$!
    wait_for_log "$active_pid" "$log" '\[Startup\] Window revealed' || exit 1
    printf 'resize 1280 860\n' > "$active_control_file"
    kill -USR1 "$active_pid"
    "$window_control" wait-size "$active_pid" 1280 860 10000
    "$window_control" click-burst "$active_pid" 64 150 2 200
    wait_for_log "$active_pid" "$log" '\[ArtifactPreview::PerformanceFixture\] settled' || exit 1
    sleep 1
    divider_x="$(sed -n 's/.*divider_x=\([0-9][0-9.]*\).*/\1/p' "$log" | tail -n 1)"
    "$window_control" pointer-drag-cycle "$active_pid" "$divider_x" 360 180 160 8
    sleep 0.8
    scroll_x="$(awk -v divider="$divider_x" 'BEGIN { printf "%.1f", divider + (1280 - divider) / 2 }')"
    "$window_control" scroll-burst "$active_pid" "$scroll_x" 420 -120 420 8
    sleep 0.8
    stop_process "$active_pid"
    [[ -s "$trace" ]] || { echo "error: missing trace $trace" >&2; exit 1; }
    grep -q $'\tscroll\t' "$trace" || { echo "error: no scroll samples in $trace" >&2; exit 1; }
  done
done

python3 "$repo_root/scripts/analyze-artifact-preview-scroll.py" "$result_dir"
printf '\nResults: %s\n' "$result_dir"
