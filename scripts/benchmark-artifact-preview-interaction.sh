#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/benchmark-artifact-preview-interaction.sh [options]

Run reproducible Artifact Preview docked-divider and detached-window resize A/B traces on macOS.

Options:
  --runs N       Isolated processes per variant/host (default: 3; minimum: 3)
  --output DIR   Result root (default: artifact-preview-interaction-benchmark-results)
  --binary PATH  Release binary (default: target/release/usher)
  --no-build     Reuse the selected binary
  -h, --help     Show this help
EOF
}

die() {
  echo "error: $*" >&2
  exit 1
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runs=3
output_root="$repo_root/artifact-preview-interaction-benchmark-results"
binary="$repo_root/target/release/usher"
build_binary=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --runs) runs="${2:?missing value for --runs}"; shift 2 ;;
    --output) output_root="${2:?missing value for --output}"; shift 2 ;;
    --binary) binary="${2:?missing value for --binary}"; shift 2 ;;
    --no-build) build_binary=0; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

[[ "$(uname -s)" == "Darwin" ]] || die "this benchmark requires macOS"
[[ "$runs" =~ ^[1-9][0-9]*$ ]] || die "--runs must be a positive integer"
(( runs >= 3 )) || die "at least three isolated runs per variant/host are required"
command -v python3 >/dev/null 2>&1 || die "python3 is required"
command -v swiftc >/dev/null 2>&1 || die "swiftc is required"
command -v shasum >/dev/null 2>&1 || die "shasum is required"

if [[ "$binary" != /* ]]; then
  binary="$repo_root/$binary"
fi
[[ "$binary" == */release/* ]] || die "the interaction A/B requires a release binary"

cd "$repo_root"
if (( build_binary == 1 )); then
  cargo build --release --locked
fi
[[ -x "$binary" ]] || die "release binary not found: $binary"

timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
result_dir="$output_root/$timestamp"
mkdir -p "$result_dir/runs"
window_control="$result_dir/macos-window-control"
swiftc -O -module-cache-path /tmp/usher-preview-benchmark-swift-cache \
  "$repo_root/scripts/macos-window-control.swift" -o "$window_control"

git_dirty=false
[[ -z "$(git status --porcelain)" ]] || git_dirty=true
{
  printf 'key\tvalue\n'
  printf 'created_utc\t%s\n' "$timestamp"
  printf 'git_commit\t%s\n' "$(git rev-parse HEAD)"
  printf 'git_dirty\t%s\n' "$git_dirty"
  printf 'binary\t%s\n' "$binary"
  printf 'binary_sha256\t%s\n' "$(shasum -a 256 "$binary" | awk '{print $1}')"
  printf 'macos\t%s (%s)\n' "$(sw_vers -productVersion)" "$(sw_vers -buildVersion)"
  printf 'hardware\t%s\n' "$(sysctl -n hw.model 2>/dev/null || uname -m)"
  printf 'renderer\twinit-femtovg\n'
  printf 'fixture\tartifact-preview-markdown-256k-371-blocks-v1\n'
  printf 'runs_per_variant_host\t%s\n' "$runs"
  printf 'docked_input\tcontinuous divider +180/-180 px, 160 steps per leg, 8 ms\n'
  printf 'detached_input\tcontinuous native AXSize width commits 920 -> 560 -> 920, fixed outer height, 80 steps per leg, 16 ms\n'
} > "$result_dir/metadata.tsv"

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

set_main_window_size() {
  local pid="$1" width="$2" height="$3"
  printf 'resize %s %s\n' "$width" "$height" > "$active_control_file"
  kill -USR1 "$pid"
  "$window_control" wait-size "$pid" "$width" "$height" 10000
}

stop_process() {
  local pid="$1" attempts=0
  kill -TERM "$pid"
  while kill -0 "$pid" 2>/dev/null && (( attempts < 100 )); do
    sleep 0.1
    ((attempts += 1))
  done
  kill -0 "$pid" 2>/dev/null && die "process $pid did not exit cleanly"
  wait "$pid" || true
  active_pid=""
}

for run in $(seq 1 "$runs"); do
  for host in docked detached; do
    for variant in baseline optimized; do
      scenario="chat-preview-$host"
      expected_host="$host"
      [[ "$host" == "docked" ]] && expected_host="docked-divider"
      [[ "$host" == "detached" ]] && expected_host="detached-window"
      trace="$result_dir/runs/$variant-$host-run-$run.tsv"
      log="$result_dir/runs/$variant-$host-run-$run.log"
      active_control_file="$result_dir/runs/$variant-$host-run-$run.control"
      : > "$active_control_file"
      echo "Running host=$host variant=$variant run=$run/$runs ..."
      USHER_MEMORY_DIAGNOSTICS=1 \
        USHER_MEMORY_SCENARIO="$scenario" \
        USHER_MEMORY_BENCHMARK_RUN="$run" \
        USHER_MEMORY_WINDOW_CONTROL=signal-file-v1 \
        USHER_MEMORY_WINDOW_CONTROL_FILE="$active_control_file" \
        USHER_BENCHMARK_INSTANCE_NAMESPACE="preview-resize-$timestamp" \
        USHER_PREVIEW_PERF_VARIANT="$variant" \
        USHER_PREVIEW_PERF_TRACE="$trace" \
        USHER_PREVIEW_PERF_TRACE_WARMUP_MS=0 \
        SLINT_BACKEND=winit-femtovg \
        RUST_LOG=info \
        "$binary" > "$log" 2>&1 &
      active_pid=$!

      if ! wait_for_log "$active_pid" "$log" '\[Startup\] Window revealed'; then
        sed -n '1,180p' "$log" >&2
        die "window did not become ready for $host/$variant run $run"
      fi
      set_main_window_size "$active_pid" 1280 860
      # Reuse the established memory-benchmark navigation gesture. The fixture
      # is intentionally route-gated and only mounts after Chat becomes active.
      "$window_control" click-burst "$active_pid" 64 150 2 200
      if ! wait_for_log "$active_pid" "$log" '\[ArtifactPreview::PerformanceFixture\] settled'; then
        sed -n '1,220p' "$log" >&2
        die "Preview fixture did not settle for $host/$variant run $run"
      fi
      sleep 1

      if [[ "$host" == "docked" ]]; then
        divider_x="$(sed -n 's/.*divider_x=\([0-9][0-9.]*\).*/\1/p' "$log" | tail -n 1)"
        [[ -n "$divider_x" ]] || die "could not read divider position from $log"
        "$window_control" pointer-drag-cycle "$active_pid" "$divider_x" 360 180 160 8
      else
        "$window_control" move-smallest-window "$active_pid" 120 80
        sleep 0.2
        detached_size="$("$window_control" smallest-window-size "$active_pid")"
        detached_width="${detached_size%x*}"
        detached_height="${detached_size#*x}"
        (( detached_width > 560 )) || die "detached window is too narrow for width cycle: $detached_size"
        "$window_control" ax-resize-smallest-cycle \
          "$active_pid" 560 "$detached_height" 80 16
        "$window_control" wait-smallest-size \
          "$active_pid" "$detached_width" "$detached_height" 10000
      fi
      sleep 0.8
      stop_process "$active_pid"

      [[ -s "$trace" ]] || die "trace was not written for $host/$variant run $run"
      grep -q $'\t'"$expected_host"$'\t' "$trace" || die "trace has no $expected_host samples: $trace"
    done
  done
done

python3 "$repo_root/scripts/analyze-artifact-preview-interaction.py" "$result_dir"
printf '\nResults: %s\n' "$result_dir"
