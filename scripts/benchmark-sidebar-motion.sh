#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
binary="${USHER_SIDEBAR_BENCH_BINARY:-$repository_root/target/release/usher}"
run_count="${USHER_SIDEBAR_BENCH_RUNS:-3}"
click_count="${USHER_SIDEBAR_BENCH_CLICKS:-24}"
click_interval_ms="${USHER_SIDEBAR_BENCH_CLICK_INTERVAL_MS:-50}"
output_directory="${1:-$repository_root/sidebar-motion-benchmark-results/$(date +%Y%m%d-%H%M%S)}"
window_control="$repository_root/scripts/macos-window-control.swift"
active_pid=""

cleanup() {
    if [[ -n "$active_pid" ]] && kill -0 "$active_pid" 2>/dev/null; then
        kill -TERM "$active_pid" 2>/dev/null || true
        wait "$active_pid" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "error: sidebar motion benchmark currently requires macOS" >&2
    exit 1
fi
if [[ ! -x "$binary" ]]; then
    echo "error: release binary not found: $binary" >&2
    exit 1
fi
if [[ "$run_count" -lt 3 ]]; then
    echo "error: at least three isolated runs per mode are required" >&2
    exit 1
fi

mkdir -p "$output_directory"
{
    echo "binary=$binary"
    echo "binary_sha256=$(shasum -a 256 "$binary" | awk '{print $1}')"
    echo "runs_per_mode=$run_count"
    echo "click_count=$click_count"
    echo "click_interval_ms=$click_interval_ms"
    echo "toggle_offset=96,23"
    echo "os=$(sw_vers -productVersion)"
    echo "hardware=$(sysctl -n hw.model)"
} > "$output_directory/metadata.txt"

for mode in timer display-link; do
    for run in $(seq 1 "$run_count"); do
        trace="$output_directory/$mode-run-$run.tsv"
        log="$output_directory/$mode-run-$run.log"
        USHER_SIDEBAR_MOTION="$mode" \
            USHER_SIDEBAR_MOTION_TRACE="$trace" \
            RUST_LOG=info \
            "$binary" > "$log" 2>&1 &
        active_pid=$!

        window_ready=false
        for _ in $(seq 1 80); do
            if [[ "$(swift "$window_control" count "$active_pid" 2>/dev/null || true)" == "1" ]]; then
                window_ready=true
                break
            fi
            sleep 0.05
        done
        if [[ "$window_ready" != true ]]; then
            echo "error: window did not appear for $mode run $run" >&2
            exit 1
        fi

        sleep 1.5
        swift "$window_control" click-burst \
            "$active_pid" 96 23 "$click_count" "$click_interval_ms"
        sleep 1.5
        kill -TERM "$active_pid"
        wait "$active_pid"
        active_pid=""
    done
done

python3 "$repository_root/scripts/analyze-sidebar-motion.py" "$output_directory"
