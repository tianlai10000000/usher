#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/benchmark-chat-interaction.sh [options]

Run reproducible baseline/optimized Chat resize, scrollbar-drag, and wheel traces on macOS.

Options:
  --runs N       Isolated processes per variant (default: 3; minimum: 3)
  --output DIR   Result root (default: chat-interaction-benchmark-results)
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
output_root="$repo_root/chat-interaction-benchmark-results"
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
(( runs >= 3 )) || die "at least three isolated runs per variant are required"
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
swiftc -O -module-cache-path /tmp/usher-chat-benchmark-swift-cache \
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
  printf 'fixture\tchat-performance-v2 (180 messages; three consecutive near-limit assistant rows per user row)\n'
  printf 'runs_per_variant\t%s\n' "$runs"
  printf 'instance_namespace\tchat-interaction-%s\n' "$timestamp"
  printf 'resize_input\tcontinuous width-only 1280x860 -> 800x860 -> 1280x860, 80 steps per leg, 8 ms\n'
  printf 'scrollbar_drag_input\ttwo fast thumb drags bottom-to-top and top-to-bottom, 520 px, 24 steps, 8 ms\n'
  printf 'scroll_input\tcontinuous +320/-320 px, 3200 events per leg, 8 ms\n'
  printf 'post_stop_guard\tno optimized model commit within 800 ms after final viewport movement\n'
  printf 'startup_bottom_guard\tbottom distance <= 1 px after initial bottom positioning and before resize input\n'
  printf 'commit_latency_guard\toptimized commit-adjacent p95 <= 50 ms and every mutation-to-render latency <= 50 ms\n'
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
  while (( attempts < 200 )); do
    kill -0 "$pid" 2>/dev/null || return 1
    grep -a -q "$pattern" "$log" && return 0
    sleep 0.1
    ((attempts += 1))
  done
  return 1
}

set_window_size() {
  local pid="$1" width="$2" height="$3" attempts=0
  printf 'resize %s %s\n' "$width" "$height" > "$active_control_file"
  kill -USR1 "$pid"
  while (( attempts < 100 )); do
    kill -0 "$pid" 2>/dev/null || return 1
    grep -a -q "Resized native window to ${width}x${height}" "$log" && return 0
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
  kill -0 "$pid" 2>/dev/null && die "process $pid did not exit cleanly"
  wait "$pid" || true
  active_pid=""
}

for run in $(seq 1 "$runs"); do
  for variant in baseline optimized; do
    trace="$result_dir/runs/$variant-run-$run.tsv"
    log="$result_dir/runs/$variant-run-$run.log"
    active_control_file="$result_dir/runs/$variant-run-$run.control"
    : > "$active_control_file"
    echo "Running variant=$variant run=$run/$runs ..."
    USHER_MEMORY_DIAGNOSTICS=1 \
      USHER_MEMORY_SCENARIO=chat-performance \
      USHER_MEMORY_BENCHMARK_RUN="$run" \
      USHER_MEMORY_WINDOW_CONTROL=signal-file-v1 \
      USHER_MEMORY_WINDOW_CONTROL_FILE="$active_control_file" \
      USHER_BENCHMARK_INSTANCE_NAMESPACE="chat-interaction-$timestamp" \
      USHER_CHAT_PERF_VARIANT="$variant" \
      USHER_CHAT_PERF_TRACE="$trace" \
      USHER_CHAT_PERF_TRACE_WARMUP_MS=0 \
      SLINT_BACKEND=winit-femtovg \
      RUST_LOG=info \
      "$binary" > "$log" 2>&1 &
    active_pid=$!

    if ! wait_for_log "$active_pid" "$log" '\[Startup\] Window revealed'; then
      sed -n '1,160p' "$log" >&2
      die "window did not become ready for $variant run $run"
    fi
    set_window_size "$active_pid" 1280 860 \
      || die "application did not confirm logical size=1280x860"
    # Use the same deterministic Chat navigation gesture as the Preview
    # interaction benchmarks, then require route-level evidence before input.
    "$window_control" click-burst "$active_pid" 64 150 2 200
    if ! wait_for_log "$active_pid" "$log" '\[Memory\]\[UI\] route=Chat'; then
      sed -n '1,180p' "$log" >&2
      die "Chat fixture was not mounted for $variant run $run"
    fi

    sleep 2
    "$window_control" resize-cycle "$active_pid" 800 860 80 8
    sleep 0.8
    "$window_control" pointer-drag "$active_pid" 1275 680 0 -520 24 8
    sleep 0.4
    "$window_control" pointer-drag "$active_pid" 1275 160 0 520 24 8
    sleep 0.8
    "$window_control" scroll-cycle "$active_pid" 900 400 320 3200 8
    sleep 0.8
    stop_process "$active_pid"

    [[ -s "$trace" ]] || die "trace was not written for $variant run $run"
    grep -q $'\tresize\t' "$trace" || die "trace has no resize samples: $trace"
    grep -q $'\tscroll\t' "$trace" || die "trace has no scroll samples: $trace"
    awk -F '\t' 'NR > 1 && $16 == 1 { seen = 1 } END { if (!seen) exit 1 }' "$trace" \
      || die "trace has no scrollbar-drag samples: $trace"
    awk -F '\t' '
      NR > 1 && $3 == "scroll" {
        if (!seen || $7 > nearest_top) nearest_top = $7
        if (!seen || $8 < min_commits) min_commits = $8
        if (!seen || $8 > max_commits) max_commits = $8
        seen = 1
      }
      END {
        if (!seen || nearest_top < -1500 || max_commits <= min_commits) exit 1
      }
    ' "$trace" || die "scroll did not cover the full virtual history with model commits: $trace"
    awk -F '\t' '
      NR > 1 {
        if (!seen || $22 < minimum) minimum = $22
        if (!seen || $22 > maximum) maximum = $22
        seen = 1
      }
      END { if (!seen || maximum - minimum < 10) exit 1 }
    ' "$trace" || die "trace has insufficient Flickable wheel-direction samples: $trace"
    if [[ "$variant" == "optimized" ]]; then
      awk -F '\t' '
        NR > 1 && $16 == 1 && $9 > 1 { exit 1 }
      ' "$trace" || die "optimized trace exposed a virtual spacer during direct scrollbar drag: $trace"
      awk -F '\t' '
        NR > 1 && $3 == "scroll" {
          if (seen && $8 == previous_commits && $9 > 1) exit 1
          previous_commits = $8
          seen = 1
        }
      ' "$trace" || die "optimized trace exposed a virtual spacer in the viewport: $trace"
      awk -F '\t' '
        NR > 1 {
          rows += 1
          elapsed[rows] = $2
          position[rows] = $7
          commits[rows] = $8
        }
        END {
          last_motion = 1
          for (sample_index = 2; sample_index <= rows; sample_index += 1) {
            delta = position[sample_index] - position[sample_index - 1]
            if (delta > 0.25 || delta < -0.25) last_motion = sample_index
          }
          stop_commits = commits[last_motion]
          cutoff = elapsed[last_motion] + 800000
          for (sample_index = last_motion + 1; sample_index <= rows; sample_index += 1) {
            if (elapsed[sample_index] <= cutoff && commits[sample_index] > stop_commits) exit 1
          }
        }
      ' "$trace" || die "optimized trace committed a virtual window after scrolling stopped: $trace"
    fi
  done
done

python3 "$repo_root/scripts/analyze-chat-interaction.py" "$result_dir"
printf '\nResults: %s\n' "$result_dir"
