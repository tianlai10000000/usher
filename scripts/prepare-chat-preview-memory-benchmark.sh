#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
"$script_dir/prepare-chat-memory-benchmark.sh" "$@"
sleep "${USHER_MEMORY_PREVIEW_WAIT_SECONDS:-2}"

if [[ -n "${USHER_MEMORY_CAPTURE_PATH:-}" ]]; then
    pid="${1:?missing PID}"
    window_control="${USHER_MEMORY_WINDOW_CONTROL_BINARY:?missing window-control helper}"
    window_id="$("$window_control" window-id "$pid")"
    screencapture -x -l "$window_id" "$USHER_MEMORY_CAPTURE_PATH"
fi
