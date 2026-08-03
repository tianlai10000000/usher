#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

if ! command -v slint-viewer >/dev/null 2>&1; then
    echo "error: slint-viewer is required for the fast UI check" >&2
    echo "install the Slint 1.17.1 viewer, then retry" >&2
    exit 1
fi

exec slint-viewer \
    --check \
    -L material=material-1.7.1-fixed/material.slint \
    -L ui-components=ui/features/ui_components_enabled.slint \
    ui/main.slint
