#!/usr/bin/env bash
set -euo pipefail

pid="${1:?missing PID}"
window_control="${USHER_MEMORY_WINDOW_CONTROL_BINARY:?missing window-control helper}"

"$window_control" click-burst "$pid" 64 150 2 200
sleep 1
