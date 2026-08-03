#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

export SLINT_LIVE_PREVIEW=1
exec cargo run --locked --features live-preview,ui-components "$@"
