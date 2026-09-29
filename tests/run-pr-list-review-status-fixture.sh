#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repository_root"

export USHER_PR_LIST_FIXTURE=review-status-v1
export USHER_PR_LIST_AUTOSTART=1
export USHER_BENCHMARK_OFFLINE=1
export USHER_BENCHMARK_STATE_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/usher-pr-list.XXXXXX")
exec cargo run --locked --features test-fixtures "$@"
