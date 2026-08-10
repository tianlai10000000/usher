#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repository_root"

export USHER_PR_LIST_FIXTURE=review-status-v1
exec cargo run --locked --features test-fixtures "$@"
