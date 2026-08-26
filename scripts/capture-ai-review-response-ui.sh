#!/usr/bin/env bash
set -euo pipefail

die() {
  echo "error: $*" >&2
  exit 1
}

usage() {
  cat <<'USAGE'
Usage: scripts/capture-ai-review-response-ui.sh [--binary PATH] [--output DIR]

Captures the formatted AI Review response fixture in light/dark, narrow/wide,
top, and scrolled states. macOS Screen Recording and Accessibility permission
are required.
USAGE
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="$repo_root/target/release/usher"
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
output_root="$repo_root/memory-benchmark-results/ai-review-response-ui/${timestamp}-$$"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary)
      binary="${2:?missing value for --binary}"
      shift 2
      ;;
    --output)
      output_root="${2:?missing value for --output}"
      shift 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      die "unknown argument: $1"
      ;;
  esac
done

[[ "$(uname -s)" == "Darwin" ]] || die "native screenshot capture requires macOS"
[[ -x "$binary" ]] || die "Release binary is not executable: $binary"
for command in swiftc screencapture sips shasum; do
  command -v "$command" >/dev/null || die "required command is unavailable: $command"
done

mkdir -p "$output_root/screenshots" "$output_root/state" "$output_root/runs"
window_control="$output_root/macos-window-control"
swiftc -module-cache-path "$output_root/swift-module-cache" \
  -O "$repo_root/scripts/macos-window-control.swift" \
  -o "$window_control"

binary_sha="$(shasum -a 256 "$binary" | awk '{print $1}')"
active_pid=""

stop_app() {
  local pid="${active_pid:-}"
  [[ -n "$pid" ]] || return 0
  if kill -0 "$pid" 2>/dev/null; then
    kill -TERM "$pid" 2>/dev/null || true
    for _ in $(seq 1 100); do
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.1
    done
  fi
  if kill -0 "$pid" 2>/dev/null; then
    kill -KILL "$pid" 2>/dev/null || true
  fi
  wait "$pid" 2>/dev/null || true
  active_pid=""
}

trap stop_app EXIT INT TERM

wait_for_log() {
  local pid="$1" log="$2" pattern="$3" description="$4"
  for _ in $(seq 1 300); do
    kill -0 "$pid" 2>/dev/null || die "app exited while waiting for $description: $log"
    grep -a -E -q "$pattern" "$log" && return 0
    sleep 0.1
  done
  die "timed out waiting for $description: $log"
}

wait_for_window_count() {
  local pid="$1" expected="$2"
  local actual=0
  for _ in $(seq 1 100); do
    actual="$($window_control window-count "$pid" 2>/dev/null || true)"
    [[ "$actual" == "$expected" ]] && return 0
    sleep 0.1
  done
  die "expected $expected native windows for PID $pid, found ${actual:-unknown}"
}

write_settings() {
  local run="$1" theme_mode="$2" presentation_mode="$3"
  local state_dir="$output_root/state/formatted/run-$run"
  mkdir -p "$state_dir"
  cat >"$state_dir/settings.toml" <<EOF
theme_mode = $theme_mode
auto_update = false
application_console_logging = true
application_file_logging = false
tracing_logger_active = false
memory_diagnostics_logging = false
ui_sound_effects = false
navigation_drawer_expanded = true

[preview]
presentation_mode = "$presentation_mode"

[preview.providers]
markdown = true
image = true
plain_text = true
EOF
}

launch_app() {
  local run="$1" theme_mode="$2" presentation_mode="$3"
  local log="$output_root/runs/$run.log"
  local control="$output_root/runs/$run.control"
  write_settings "$run" "$theme_mode" "$presentation_mode"
  : >"$control"

  # One complete 4 KiB response covers the full Markdown matrix without
  # turning visual capture into the multi-message RSS stress benchmark.
  USHER_AI_REVIEW_RESPONSE_FIXTURE=markdown-v1 \
    USHER_AI_REVIEW_RESPONSE_VARIANT=formatted \
    USHER_AI_REVIEW_RESPONSE_FIXTURE_MESSAGES=1 \
    USHER_AI_REVIEW_RESPONSE_FIXTURE_BYTES=4096 \
    USHER_BENCHMARK_OFFLINE=1 \
    USHER_BENCHMARK_STATE_ROOT="$output_root/state" \
    USHER_MEMORY_BENCHMARK_RUN="$run" \
    USHER_MEMORY_BENCHMARK_SCENARIO=ai-review-response-ui \
    USHER_BENCHMARK_INSTANCE_NAMESPACE="ai-review-ui-${timestamp}-${run}-$$" \
    USHER_MEMORY_WINDOW_CONTROL=signal-file-v1 \
    USHER_MEMORY_WINDOW_CONTROL_FILE="$control" \
    RUST_LOG=info \
    "$binary" >"$log" 2>&1 &
  active_pid=$!

  wait_for_log \
    "$active_pid" \
    "$log" \
    '\[AI Review\]\[ResponseFixture\] settled fixture=markdown-v1 variant=formatted .*blocks=[1-9][0-9]* .*fixture_persistence=false' \
    "formatted fixture settlement"
  wait_for_window_count "$active_pid" 1
}

resize_app() {
  local run="$1" width="$2" height="$3"
  printf 'resize %s %s\n' "$width" "$height" >"$output_root/runs/$run.control"
  kill -USR1 "$active_pid"
  "$window_control" wait-size "$active_pid" "$width" "$height" 10000 >/dev/null
  sleep 0.5
}

validate_png() {
  local path="$1" minimum_width="$2" minimum_height="$3"
  local pixel_width pixel_height byte_count
  pixel_width="$(sips -g pixelWidth "$path" 2>/dev/null | awk '/pixelWidth:/ {print $2}')"
  pixel_height="$(sips -g pixelHeight "$path" 2>/dev/null | awk '/pixelHeight:/ {print $2}')"
  byte_count="$(stat -f '%z' "$path")"
  [[ "$pixel_width" =~ ^[0-9]+$ && "$pixel_width" -ge "$minimum_width" ]] ||
    die "invalid screenshot width for $path: ${pixel_width:-unknown}"
  [[ "$pixel_height" =~ ^[0-9]+$ && "$pixel_height" -ge "$minimum_height" ]] ||
    die "invalid screenshot height for $path: ${pixel_height:-unknown}"
  ((byte_count > 10000)) || die "screenshot is unexpectedly small: $path ($byte_count bytes)"
}

capture_window() {
  local window_id="$1" path="$2" minimum_width="$3" minimum_height="$4"
  screencapture -x -l "$window_id" "$path"
  validate_png "$path" "$minimum_width" "$minimum_height"
}

assert_no_fixture_persistence() {
  local run="$1" state_dir="$output_root/state/formatted/run-$run"
  local persisted_file
  persisted_file="$(
    find "$state_dir" -type f \( -name 'ai-review.json' -o -path '*/ai-review/*' \) \
      -print -quit
  )"
  [[ -z "$persisted_file" ]] || die "fixture wrote AI Review persistence: $persisted_file"
}

capture_matrix_cell() {
  local theme="$1" theme_mode="$2" width="$3" height="$4"
  local run="${theme}-${width}x${height}"
  local main_window top_path scrolled_path

  echo "Capturing $theme ${width}x${height} ..."
  launch_app "$run" "$theme_mode" docked
  resize_app "$run" "$width" "$height"
  main_window="$($window_control window-id "$active_pid")"
  top_path="$output_root/screenshots/${run}-top.png"
  capture_window "$main_window" "$top_path" "$width" "$height"

  "$window_control" scroll-burst \
    "$active_pid" \
    "$((width - 140))" \
    "$((height / 2))" \
    -120 \
    18 \
    8
  wait_for_log \
    "$active_pid" \
    "$output_root/runs/$run.log" \
    '\[AI Review\]\[ResponseFixture\] scroll-consumed fixture=markdown-v1 variant=formatted' \
    "response scroll consumption"
  sleep 0.5
  scrolled_path="$output_root/screenshots/${run}-scrolled.png"
  capture_window "$main_window" "$scrolled_path" "$width" "$height"
  cmp -s "$top_path" "$scrolled_path" && die "scroll screenshot did not change: $run"

  stop_app
  assert_no_fixture_persistence "$run"
}

capture_matrix_cell light 0 800 600
capture_matrix_cell light 0 1280 860
capture_matrix_cell dark 1 800 600
capture_matrix_cell dark 1 1280 860

shasum -a 256 "$output_root"/screenshots/*.png >"$output_root/screenshot-sha256.txt"
{
  printf 'key\tvalue\n'
  printf 'created_utc\t%s\n' "$timestamp"
  printf 'binary\t%s\n' "$binary"
  printf 'binary_sha256\t%s\n' "$binary_sha"
  printf 'fixture\tmarkdown-v1\n'
  printf 'variant\tformatted\n'
  printf 'fixture_persistence\tfalse\n'
  printf 'matrix\tlight/dark x 800x600/1280x860 top+scrolled\n'
} >"$output_root/metadata.tsv"

echo "AI Review response UI capture complete: $output_root"
