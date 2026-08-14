#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runs="${1:-3}"
[[ "$runs" =~ ^[0-9]+$ ]] && (( runs >= 3 )) || {
  echo "runs must be at least 3" >&2
  exit 1
}

cd "$repo_root"
cargo build --release --locked
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
result_dir="$repo_root/artifact-preview-transition-benchmark-results/$timestamp"
mkdir -p "$result_dir/runs"
window_control="$result_dir/macos-window-control"
swiftc -O -module-cache-path /tmp/usher-preview-transition-swift-cache \
  scripts/macos-window-control.swift -o "$window_control"

active_pid=""
cleanup() {
  if [[ -n "$active_pid" ]] && kill -0 "$active_pid" 2>/dev/null; then
    kill -TERM "$active_pid" 2>/dev/null || true
    wait "$active_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

wait_for_log() {
  local pid="$1" log="$2" pattern="$3" attempts=0
  while (( attempts < 300 )); do
    kill -0 "$pid" 2>/dev/null || return 1
    grep -a -q "$pattern" "$log" && return 0
    sleep 0.1
    ((attempts += 1))
  done
  return 1
}

for run in $(seq 1 "$runs"); do
  for variant in baseline optimized; do
    trace="$result_dir/runs/$variant-run-$run.tsv"
    log="$result_dir/runs/$variant-run-$run.log"
    echo "Running transition variant=$variant run=$run/$runs ..."
    USHER_MEMORY_DIAGNOSTICS=1 \
      USHER_MEMORY_SCENARIO=chat-preview-docked \
      USHER_PREVIEW_TRANSITION_BENCHMARK=1 \
      USHER_PREVIEW_PERF_VARIANT="$variant" \
      USHER_PREVIEW_PERF_TRACE="$trace" \
      USHER_PREVIEW_PERF_TRACE_WARMUP_MS=0 \
      SLINT_BACKEND=winit-femtovg \
      RUST_LOG=info \
      target/release/usher > "$log" 2>&1 &
    active_pid=$!
    wait_for_log "$active_pid" "$log" '\[Startup\] Window revealed' || exit 1
    "$window_control" click-burst "$active_pid" 64 150 2 200
    wait_for_log "$active_pid" "$log" '\[ArtifactPreview::TransitionFixture\] complete' || exit 1
    sleep 0.5
    kill -TERM "$active_pid"
    wait "$active_pid" || true
    active_pid=""
    [[ -s "$trace" ]] || { echo "missing trace: $trace" >&2; exit 1; }
  done
done

python3 scripts/analyze-artifact-preview-transition.py "$result_dir"
printf '\nResults: %s\n' "$result_dir"
