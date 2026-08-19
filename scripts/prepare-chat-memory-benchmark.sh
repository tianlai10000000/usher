#!/usr/bin/env bash
set -euo pipefail

pid="${1:?missing PID}"
window_control="${USHER_MEMORY_WINDOW_CONTROL_BINARY:?missing window-control helper}"
route_click_count="${USHER_MEMORY_ROUTE_CLICK_COUNT:-2}"
route_click_interval_ms="${USHER_MEMORY_ROUTE_CLICK_INTERVAL_MS:-200}"

"$window_control" click-burst "$pid" 64 150 "$route_click_count" "$route_click_interval_ms"
sleep "${USHER_MEMORY_ROUTE_SETTLE_SECONDS:-1}"
