#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

failed=0

check_max() {
    label=$1
    actual=$2
    maximum=$3
    if [ "$actual" -gt "$maximum" ]; then
        echo "Architecture structure: $label is $actual (budget $maximum)." >&2
        failed=1
    fi
}

line_count() {
    wc -l < "$1" | tr -d ' '
}

rust_function_count() {
    rg -c '^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+' "$1" || true
}

slint_declaration_count() {
    rg -c "$2" "$1" || true
}

# Budgets are the 2026-08-29 regression baseline plus limited headroom. They
# are tripwires for another structural growth cycle, not style limits: a change
# may update a budget together with an ownership review and this audit's data.
check_max "src/lib.rs lines" "$(line_count src/lib.rs)" 1100
check_max "src/lib.rs functions" "$(rust_function_count src/lib.rs)" 8
check_max "PR Review controller lines" "$(line_count src/workflow/pr_review/controller.rs)" 4600
check_max "PR Review controller functions" "$(rust_function_count src/workflow/pr_review/controller.rs)" 95
check_max "Gitee adapter lines" "$(line_count src/git/gitee/mod.rs)" 3825
check_max "Gitee adapter functions" "$(rust_function_count src/git/gitee/mod.rs)" 125
check_max "Chat controller lines" "$(line_count src/chat/interactive.rs)" 2200
check_max "Chat controller functions" "$(rust_function_count src/chat/interactive.rs)" 70
check_max "root Slint lines" "$(line_count ui/main.slint)" 3000
check_max "root Slint callbacks" "$(slint_declaration_count ui/main.slint '^[[:space:]]*callback[[:space:]]+')" 140
check_max "root Slint properties" "$(slint_declaration_count ui/main.slint '^[[:space:]]*(in|out|in-out|private)[[:space:]]+property')" 410
check_max "PR Review Slint lines" "$(line_count ui/views/workflow/pr_review_workspace.slint)" 4500

unexpected_http_clients=$(rg -n \
    'reqwest::Client::new|reqwest::blocking::get|reqwest::Client::builder|Client::builder' \
    src crates --glob '*.rs' | rg -v '^src/http/mod\.rs:' || true)
if [ -n "$unexpected_http_clients" ]; then
    echo "Architecture structure: HTTP client creation escaped src/http/mod.rs:" >&2
    echo "$unexpected_http_clients" >&2
    failed=1
fi

unexpected_runtimes=$(rg -n \
    'Runtime::new|Builder::new_(current_thread|multi_thread)' \
    src crates --glob '*.rs' | rg -v '^src/app_runtime\.rs:' || true)
if [ -n "$unexpected_runtimes" ]; then
    echo "Architecture structure: Tokio runtime creation escaped src/app_runtime.rs:" >&2
    echo "$unexpected_runtimes" >&2
    failed=1
fi

if [ "$failed" -ne 0 ]; then
    exit 1
fi

echo "Architecture structure checks passed."
