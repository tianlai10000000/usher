#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

export SLINT_ENABLE_EXPERIMENTAL_FEATURES=1

if ! command -v slint-viewer >/dev/null 2>&1; then
    echo "error: slint-viewer is required for the fast UI check" >&2
    echo "install a Slint 1.18.x viewer, then retry" >&2
    exit 1
fi

viewer_version=$(slint-viewer --version 2>/dev/null || true)
case "$viewer_version" in
    *" 1.18."*) ;;
    *)
        echo "error: ui-check requires a Slint 1.18.x viewer; found: ${viewer_version:-unknown}" >&2
        exit 1
        ;;
esac

cargo run --quiet --locked -p usher-i18n --example prepare-slint -- \
    target/i18n-dev usher-ui >/dev/null

exec slint-viewer \
    --check \
    -L material=material-1.18.0/material.slint \
    -L ui-components=ui/features/ui_components_enabled.slint \
    -L pr-review=ui/features/pr_review/full.slint \
    -L ai-review=ui/features/ai_review/full.slint \
    -L i18n=target/i18n-dev/usher-i18n/i18n.slint \
    ui/main.slint
