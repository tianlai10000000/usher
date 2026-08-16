#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

# This mode excludes the complete PR Review render tree. Use a normal run to
# validate PR Review and before final delivery.
exec cargo run \
    --locked \
    --features dev-shell \
    --target-dir target/dev-shell \
    "$@"
