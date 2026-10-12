#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repository_root"
exec cargo run --locked --features test-fixtures --example pr_approval_state_fixture -- "$@"
