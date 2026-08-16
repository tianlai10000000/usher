#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

# Keep the development shell's Cargo identity separate from full builds.
exec cargo check \
    --locked \
    --features dev-shell \
    --target-dir target/dev-shell \
    "$@"
