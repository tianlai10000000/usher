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
check_max "AI Review controller lines" "$(line_count src/ai_review/controller.rs)" 3850
check_max "AI Review controller functions" "$(rust_function_count src/ai_review/controller.rs)" 120
check_max "root Slint lines" "$(line_count ui/main.slint)" 3000
check_max "root Slint callbacks" "$(slint_declaration_count ui/main.slint '^[[:space:]]*(pure[[:space:]]+)?callback[[:space:]]+')" 140
check_max "root Slint properties" "$(slint_declaration_count ui/main.slint '^[[:space:]]*(in|out|in-out|private)[[:space:]]+property')" 410
check_max "PR Review Slint lines" "$(line_count ui/views/workflow/pr_review_workspace.slint)" 4500

# P5-D owners have their own tripwires so extraction cannot hide regrowth.
check_max "PR Review remote actions lines" "$(line_count src/workflow/pr_review/controller/actions.rs)" 580
check_max "PR Review Diff bindings lines" "$(line_count src/workflow/pr_review/controller/diff_bindings.rs)" 430
check_max "PR Review diagnostics lines" "$(line_count src/workflow/pr_review/controller/diagnostics.rs)" 190
check_max "Chat viewport policy lines" "$(line_count src/chat/viewport_policy.rs)" 390
check_max "Chat message model lines" "$(line_count src/chat/message_model.rs)" 180
check_max "AI Review sessions lines" "$(line_count src/ai_review/sessions.rs)" 120
check_max "Settings save feedback lines" "$(line_count src/settings_feedback.rs)" 110
check_max "PR Diff bridge callbacks" "$(slint_declaration_count ui/features/pr_review/diff_bridge.slint '^[[:space:]]*(pure[[:space:]]+)?callback[[:space:]]+')" 16

# Batch 6 keeps every original limit above and gives the new owners their own
# tripwires. Moving an API or method is not permission for unbounded regrowth.
check_max "AI Review input lines" "$(line_count src/ai_review/review_input.rs)" 330
check_max "PR Review action presentation lines" "$(line_count src/workflow/pr_review/controller/actions/presentation.rs)" 460
check_max "PR Review action remote lines" "$(line_count src/workflow/pr_review/controller/actions/remote.rs)" 310
check_max "Project bridge lines" "$(line_count ui/features/project/bridge.slint)" 70
check_max "Project bridge callbacks" "$(slint_declaration_count ui/features/project/bridge.slint '^[[:space:]]*(pure[[:space:]]+)?callback[[:space:]]+')" 9
check_max "Project bridge properties" "$(slint_declaration_count ui/features/project/bridge.slint '^[[:space:]]*(in|out|in-out|private)[[:space:]]+property')" 24
check_max "Project dialog host lines" "$(line_count ui/features/project/dialog.slint)" 90

reverse_controller_dependencies=$(rg -n 'interactive::|controller::|use super::\*' \
    src/chat/viewport_policy.rs src/chat/message_model.rs src/ai_review/sessions.rs \
    src/workflow/pr_review/controller/diff_bindings.rs \
    src/workflow/pr_review/controller/diagnostics.rs \
    src/ai_review/review_input.rs \
    src/workflow/pr_review/controller/actions/presentation.rs \
    src/workflow/pr_review/controller/actions/remote.rs || true)
if [ -n "$reverse_controller_dependencies" ]; then
    echo "Architecture structure: extracted leaf owners depend on controller internals:" >&2
    echo "$reverse_controller_dependencies" >&2
    failed=1
fi

unexpected_leaf_ui=$(rg -n 'MainWindow|WorkflowPrReviewController|ControllerInner' \
    src/ai_review/review_input.rs \
    src/workflow/pr_review/controller/actions/remote.rs || true)
if [ -n "$unexpected_leaf_ui" ]; then
    echo "Architecture structure: domain/remote leaves depend on UI or controller types:" >&2
    echo "$unexpected_leaf_ui" >&2
    failed=1
fi

# A lexical tripwire, not Rust name/type resolution. Require explicit safe
# reqwest imports outside the HTTP owner: rejecting client/shortcut imports,
# namespace aliases and globs prevents the common alias constructor escapes.
# Comments/strings can match; macro expansion, dependency renaming and indirect
# re-exports still require review. Keep every client in the one owner file.
unexpected_http_clients=$(rg -n -U \
    -e '\breqwest\s*::\s*(Client|ClientBuilder|blocking|get)\b' \
    -e '\b(use|extern\s+crate)\s+(::\s*)?reqwest\s*(as\s+\w+)?\s*;' \
    -e '\breqwest\s+as\s+\w+' \
    -e '\breqwest\s*::\s*(\*|\{[^;]*(\b(Client|ClientBuilder|blocking|get|self)\b|\*))' \
    src crates --glob '*.rs' --glob '!src/http/mod.rs' || true)
if [ -n "$unexpected_http_clients" ]; then
    echo "Architecture structure: HTTP client/shortcut access escaped src/http/mod.rs:" >&2
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
