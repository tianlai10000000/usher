#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

export SLINT_ENABLE_EXPERIMENTAL_FEATURES=1

if ! command -v slint-viewer >/dev/null 2>&1; then
    echo "error: slint-viewer is required for the fast UI check" >&2
    echo "install the Slint 1.17.1 viewer, then retry" >&2
    exit 1
fi

cargo run --quiet --locked -p usher-i18n --example prepare-slint -- \
    target/i18n-dev usher-ui >/dev/null

exec slint-viewer \
    --check \
    -L material=material-1.7.1-fixed/material.slint \
    -L ui-components=ui/features/ui_components_enabled.slint \
    -L pr-review=ui/features/pr_review/full.slint \
    -L ai-review=ui/features/ai_review/full.slint \
    -L i18n=target/i18n-dev/usher-i18n/i18n.slint \
    ui/main.slint
