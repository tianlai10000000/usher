#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

failed=0

private_material=$(rg -n 'material-1\.7\.1-fixed' ui --glob '*.slint' || true)
if [ -n "$private_material" ]; then
    echo "UI policy: private vendored Material imports are forbidden:" >&2
    echo "$private_material" >&2
    failed=1
fi

material_imports=$(rg -n '@material' ui --glob '*.slint' || true)
unexpected_material=$(echo "$material_imports" \
    | rg -v '^ui/main\.slint:[0-9]+:export \{ MaterialWindowAdapter \} from "@material";' \
    | rg -v '^ui/views/workflow/pr_review_workspace\.slint:[0-9]+:import \{ ListView, ScrollView \} from "@material";' \
    || true)
if [ -n "$unexpected_material" ]; then
    echo "UI policy: unexpected @material usage:" >&2
    echo "$unexpected_material" >&2
    failed=1
fi

legacy_icons=$(rg -n \
    '@image-url\("[^"]*assets/icons/(?!octicon-|github-|lucide-|provider-)[^"]+"\)' \
    ui --glob '*.slint' --pcre2 || true)
unexpected_icons=$(echo "$legacy_icons" \
    | rg -v '^ui/components/title_bar\.slint:[0-9]+:.*assets/icons/(settings|about)\.svg' \
    || true)
if [ -n "$unexpected_icons" ]; then
    echo "UI policy: unregistered application icon usage is forbidden:" >&2
    echo "$unexpected_icons" >&2
    failed=1
fi

# Slint globals do not cross ComponentFactory/library-module boundaries. Every
# independently generated route must therefore mount a factory-local tooltip
# portal at its root, even when its current controls happen not to show tips.
factory_build_scripts=$(rg -l '\.as_library\(' crates --glob 'build.rs' || true)
for factory_build_script in $factory_build_scripts; do
    factory_views=$(rg -o 'ui/views/[^" ]+\.slint' "$factory_build_script" || true)
    if [ -z "$factory_views" ]; then
        echo "UI policy: cannot locate the factory root view in $factory_build_script" >&2
        failed=1
        continue
    fi

    for factory_view in $factory_views; do
        if ! rg -q 'GitHub(Message)?TooltipOverlayHost' "$factory_view"; then
            echo "UI policy: independent factory root lacks a tooltip portal: $factory_view" >&2
            failed=1
        fi
    done
done

# Tooltip surfaces may differ in factory-local state/labels, but all placement
# must flow through the same content-neutral geometry component.
floating_geometry_consumers=$(rg -l 'GitHubFloatingOverlayHost[[:space:]]*\{' \
    ui --glob '*.slint' || true)
unexpected_floating_geometry=$(echo "$floating_geometry_consumers" \
    | rg -v '^ui/components/github_tooltip_overlay\.slint$' \
    | rg -v '^ui/components/messages/github_message_tooltip\.slint$' \
    || true)
if [ -n "$unexpected_floating_geometry" ]; then
    echo "UI policy: Tooltip placement must use the shared portal geometry:" >&2
    echo "$unexpected_floating_geometry" >&2
    failed=1
fi

builtin_tooltips=$(rg -n '(^|[^A-Za-z0-9_-])Tooltip[[:space:]]*\{' \
    ui --glob '*.slint' || true)
if [ -n "$builtin_tooltips" ]; then
    echo "UI policy: built-in/local Tooltip surfaces are forbidden:" >&2
    echo "$builtin_tooltips" >&2
    failed=1
fi

if [ "$failed" -ne 0 ]; then
    exit 1
fi

echo "UI policy checks passed."
