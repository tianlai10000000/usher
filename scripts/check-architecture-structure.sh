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

# Spec-gap fixes retain bounded feature owners and keep worker code UI-free.
check_max "Preview source reader lines" "$(line_count crates/usher-preview/src/file_source.rs)" 145
check_max "AI Settings state lines" "$(line_count src/ai/settings_state.rs)" 320
check_max "AI Settings UI lines" "$(line_count src/ai/settings_ui.rs)" 220
check_max "AI provider draft selection lines" "$(line_count src/settings_page/ai_agent_drafts.rs)" 180
check_max "AI Agent profile configuration lines" "$(line_count src/settings/ai_agents.rs)" 380
check_max "AI Agent Settings projection lines" "$(line_count src/settings_page/ai_agent_projection.rs)" 160
check_max "AI Agent save admission lines" "$(line_count src/ai/settings_ui/admission.rs)" 110
check_max "AI Agent identity lines" "$(line_count src/ai/identity.rs)" 130
check_max "AI Agent request headers lines" "$(line_count src/ai/request_headers.rs)" 120
check_max "AI Agent Header object parser lines" "$(line_count src/settings_page/ai_agent_header_object.rs)" 220
check_max "AI Agent Header draft modes lines" "$(line_count src/settings_page/ai_agent_header_drafts.rs)" 220
check_max "AI Agent Header editor UI lines" "$(line_count ui/components/settings/github_ai_agent_headers.slint)" 180
check_max "AI Agent registry actions lines" "$(line_count src/ai/settings_state/actions.rs)" 140
check_max "AI Agent list projection lines" "$(line_count src/settings_page/ai_agent_list.rs)" 120
check_max "AI Agent delete confirmation lines" "$(line_count src/ai/settings_ui/delete_confirmation.rs)" 100
check_max "AI Agent save feedback lines" "$(line_count src/ai/settings_ui/feedback.rs)" 70
check_max "AI Agent connection probe lines" "$(line_count src/ai/settings_ui/connection_test.rs)" 100
check_max "AI Agent shared probe lifecycle lines" "$(line_count src/ai/settings_ui/connection_probe.rs)" 150
check_max "AI Agent list connection probe lines" "$(line_count src/ai/settings_ui/list_connection_test.rs)" 180
check_max "Fixed Tabs UI lines" "$(line_count ui/components/navigation/github_fixed_tabs.slint)" 240
check_max "Window navigation UI lines" "$(line_count ui/components/navigation/github_window_navigation.slint)" 180
window_navigation_callbacks=$(slint_declaration_count ui/components/navigation/github_window_navigation.slint '^[[:space:]]*callback[[:space:]]+')
window_navigation_properties=$(slint_declaration_count ui/components/navigation/github_window_navigation.slint '^[[:space:]]*(in|out|in-out|private)[[:space:]]+property')
check_max "Window navigation UI callbacks" "${window_navigation_callbacks:-0}" 6
check_max "Window navigation UI properties" "${window_navigation_properties:-0}" 16
check_max "AI Agent editor UI lines" "$(line_count ui/components/settings/github_ai_agent_editor.slint)" 260
check_max "AI Agent list UI lines" "$(line_count ui/components/settings/github_ai_agent_list.slint)" 160
check_max "AI Agent list row UI lines" "$(line_count ui/components/settings/github_ai_agent_list_row.slint)" 140
check_max "AI Agent fixed Tooltip UI lines" "$(line_count ui/components/settings/github_ai_agent_tooltip.slint)" 300
check_max "AI Agent Header Tooltip UI lines" "$(line_count ui/components/settings/github_ai_agent_header_tooltip.slint)" 160
check_max "AI Agent delete UI lines" "$(line_count ui/components/settings/github_ai_agent_delete.slint)" 60
check_max "Chat request capture lines" "$(line_count src/chat/request.rs)" 130
check_max "AI Review request capture lines" "$(line_count src/ai_review/request_capture.rs)" 100
check_max "AI Review Agent identity lines" "$(line_count src/ai_review/agent_identity.rs)" 160
check_max "Message command worker lines" "$(line_count src/message_center/commands.rs)" 260
check_max "Message factory focus bridge lines" "$(line_count src/message_center/focus.rs)" 200
check_max "Shared factory tab navigation lines" "$(line_count src/focus_navigation.rs)" 200
check_max "Playground keyboard bridge lines" "$(line_count src/ui_components/keyboard.rs)" 80
check_max "Notification ingress lines" "$(line_count src/notification/ingress.rs)" 370
check_max "Shutdown pipeline lines" "$(line_count src/shutdown/pipeline.rs)" 250
check_max "Shutdown phase lines" "$(line_count src/shutdown/pipeline/phase.rs)" 200
check_max "Diagnostics archive lines" "$(line_count src/diagnostics/archive.rs)" 240
check_max "Download status projection lines" "$(line_count src/update/download_status.rs)" 120
check_max "Workflow facade lines" "$(line_count src/workflow/state/facade.rs)" 400
check_max "Workflow reducer lines" "$(line_count src/workflow/state/reducer.rs)" 220
check_max "Workflow equality lines" "$(line_count src/workflow/state/equality.rs)" 80
check_max "Tag controller lines" "$(line_count src/workflow/tag/controller.rs)" 240
check_max "Tag execution lines" "$(line_count src/workflow/tag/execution.rs)" 620
check_max "Tag state lines" "$(line_count src/workflow/tag/state.rs)" 450
check_max "Tag initialization lines" "$(line_count src/workflow/tag/initialization.rs)" 200
check_max "Tag initialization diagnostics lines" "$(line_count src/workflow/tag/initialization_error.rs)" 160
check_max "Tag progress lines" "$(line_count src/workflow/tag/progress.rs)" 130
check_max "Tag activity lines" "$(line_count src/workflow/tag/activity.rs)" 90
check_max "AI Review producers lines" "$(line_count src/ai_review/producers.rs)" 200

# Window navigation owns reusable presentation only; route/history mutation
# stays behind callbacks at the shell boundary. This is a lexical tripwire.
unexpected_window_navigation_dependency=$(rg -n 'from[[:space:]]+"' \
    ui/components/navigation/github_window_navigation.slint \
    | rg -v 'from[[:space:]]+"(@i18n|\.\./button/github_icon_button\.slint|\.\./github_elevation\.slint|\.\./\.\./state/tooltip_overlay_state\.slint|\.\./\.\./theme/github_theme\.slint)"' || true)
if [ -n "$unexpected_window_navigation_dependency" ]; then
    echo "Architecture structure: window navigation depends on shell or feature internals:" >&2
    echo "$unexpected_window_navigation_dependency" >&2
    failed=1
fi

# The static development gallery owns UI composition and event-loop adapters;
# feedback still dispatches through the application controllers.
check_max "Playground factory bridge lines" "$(line_count src/ui_components/mod.rs)" 200
check_max "Playground feedback adapter lines" "$(line_count src/ui_components/feedback.rs)" 220
check_max "Playground popover bridge lines" "$(line_count src/ui_components/popover.rs)" 100
check_max "Playground popover regression lines" "$(line_count src/ui_components/popover_tests.rs)" 260
check_max "Playground native fixture lines" "$(line_count src/ui_components/fixture.rs)" 80
check_max "Playground host UI lines" "$(line_count ui/features/ui_components_bridge.slint)" 80
check_max "Playground build script lines" "$(line_count crates/usher-ui-components/build.rs)" 80
check_max "Playground factory UI lines" "$(line_count ui/views/ui_components_factory.slint)" 150
check_max "Fixed Tooltip geometry lines" "$(line_count ui/components/github_fixed_tooltip_anchor.slint)" 80
check_max "PR factory Tooltip anchor lines" "$(line_count ui/components/workflow/github_pr_factory_tooltip_anchors.slint)" 100
check_max "Factory Tooltip state lines" "$(line_count ui/state/factory_tooltip_state.slint)" 100
check_max "Playground Tooltip anchor lines" "$(line_count ui/components/showcase/github_showcase_tooltip_anchors.slint)" 200
check_max "Playground fixed Tooltip lines" "$(line_count ui/components/showcase/github_showcase_tooltip.slint)" 280
unexpected_playground_work=$(rg -n 'std::fs|std::process|std::thread|tokio::|reqwest|crate::http|LocalStateStore|PersistentStore' \
    src/ui_components/mod.rs src/ui_components/feedback.rs src/ui_components/keyboard.rs src/ui_components/popover.rs || true)
if [ -n "$unexpected_playground_work" ]; then
    echo "Architecture structure: playground UI bridge owns I/O or a duplicate Store:" >&2
    echo "$unexpected_playground_work" >&2
    failed=1
fi

# Catalog compilation owns build-only parsing; static UI must not depend on the
# generated runtime catalog or watch Rust-only copy through that dependency.
check_max "i18n runtime build lines" "$(line_count crates/usher-i18n/build.rs)" 30
check_max "i18n build facade lines" "$(line_count crates/usher-i18n-build/src/lib.rs)" 130
check_max "i18n catalog loader lines" "$(line_count crates/usher-i18n-build/src/catalog.rs)" 200
check_max "i18n validation lines" "$(line_count crates/usher-i18n-build/src/validation.rs)" 235
check_max "i18n code generation lines" "$(line_count crates/usher-i18n-build/src/codegen.rs)" 110
check_max "i18n compiler tests lines" "$(line_count crates/usher-i18n-build/src/tests.rs)" 450

# Cargo/Slint integration is separate from catalog validation. It may depend on
# the public build API only; runtime catalogs and private compilers stay outside.
check_max "Slint build integration lines" "$(line_count crates/usher-ui-build/src/lib.rs)" 150
check_max "Slint build integration tests lines" "$(line_count crates/usher-ui-build/src/tests.rs)" 160
unexpected_ui_build_dependency=$(awk '
    /^\[/ { dependencies = ($0 ~ /dependencies/); }
    dependencies && $0 !~ /^\[dependencies\]$|^slint-build[[:space:]]*=|^[[:space:]]*(#|$)/ {
        print NR ":" $0;
    }
' crates/usher-ui-build/Cargo.toml)
unexpected_ui_build_alias=$(rg -n 'package[[:space:]]*=|^build[[:space:]]*=' \
    crates/usher-ui-build/Cargo.toml || true)
unexpected_ui_build_runtime=$(rg -n \
    '\b(usher_i18n(_build)?|i_slint_compiler|slint|tokio|reqwest)\b|std[[:space:]]*::[[:space:]]*(process|thread)' \
    crates/usher-ui-build/src --glob '*.rs' --glob '!tests.rs' || true)
unexpected_direct_ui_compiler=$(rg -n '\bslint_build\b' \
    crates/usher-ui/build.rs crates/usher-ui-settings/build.rs \
    crates/usher-messages/build.rs crates/usher-ui-components/build.rs || true)
if [ -n "$unexpected_ui_build_dependency$unexpected_ui_build_alias$unexpected_ui_build_runtime$unexpected_direct_ui_compiler" ] \
    || [ -f crates/usher-ui-build/build.rs ]; then
    echo "Architecture structure: shared Slint build integration bypasses its public compiler/source-input boundary:" >&2
    echo "$unexpected_ui_build_dependency$unexpected_ui_build_alias$unexpected_ui_build_runtime$unexpected_direct_ui_compiler" >&2
    failed=1
fi

unexpected_i18n_runtime_dependency=$(rg -n \
    '(^usher-i18n[[:space:]]*=|package[[:space:]]*=[[:space:]]*"usher-i18n")' \
    crates/usher-ui/Cargo.toml crates/usher-ui-settings/Cargo.toml \
    crates/usher-messages/Cargo.toml crates/usher-ui-components/Cargo.toml \
    crates/usher-i18n-build/Cargo.toml || true)
unexpected_i18n_embedded_catalog=$(rg -n \
    'include(_str)?![[:space:]]*\(|env!\("CARGO_MANIFEST_DIR"\)|usher_i18n::|slint::' \
    crates/usher-i18n-build/src --glob '*.rs' || true)
unexpected_i18n_build_engine=$(rg -n \
    '(^build[[:space:]]*=|^slint(-build)?[[:space:]]*=|package[[:space:]]*=[[:space:]]*"slint(-build)?")' \
    crates/usher-i18n-build/Cargo.toml || true)
if [ -n "$unexpected_i18n_runtime_dependency$unexpected_i18n_embedded_catalog$unexpected_i18n_build_engine" ] \
    || [ -f crates/usher-i18n-build/build.rs ]; then
    echo "Architecture structure: i18n UI build support depends on embedded/runtime catalogs or a UI engine:" >&2
    echo "$unexpected_i18n_runtime_dependency$unexpected_i18n_embedded_catalog$unexpected_i18n_build_engine" >&2
    failed=1
fi

unexpected_focus_work=$(rg -n 'std::fs|std::process|std::thread|tokio::|reqwest|crate::http|LocalStateStore|PersistentStore' \
    src/message_center/focus.rs src/focus_navigation.rs || true)
if [ -n "$unexpected_focus_work" ]; then
    echo "Architecture structure: factory focus owners depend on I/O, workers or Store engines:" >&2
    echo "$unexpected_focus_work" >&2
    failed=1
fi

unexpected_focus_feature=$(rg -nH '\b(message_center|ui_components|settings_page|usher_messages|usher_ui_components|usher_ui_settings)\b' src/focus_navigation.rs || true)
if [ -n "$unexpected_focus_feature" ]; then
    echo "Architecture structure: shared focus navigation depends on a feature owner:" >&2
    echo "$unexpected_focus_feature" >&2
    failed=1
fi

unexpected_worker_ui=$(rg -n 'MainWindow|SettingsPageBridge|slint::|controller::' \
    crates/usher-preview/src/file_source.rs src/ai/settings_state.rs \
    src/message_center/commands.rs src/shutdown/pipeline.rs src/shutdown/pipeline/phase.rs \
    src/diagnostics/archive.rs src/ai_review/producers.rs || true)
if [ -n "$unexpected_worker_ui" ]; then
    echo "Architecture structure: source/state command workers depend on UI/controller types:" >&2
    echo "$unexpected_worker_ui" >&2
    failed=1
fi

unexpected_workflow_core_ui=$(rg -n 'MainWindow|slint::|controller::|std::process::' \
    src/workflow/state/reducer.rs src/workflow/state/equality.rs \
    src/workflow/tag/state.rs src/workflow/tag/activity.rs || true)
if [ -n "$unexpected_workflow_core_ui" ]; then
    echo "Architecture structure: Workflow reducer/state leaves depend on UI/process internals:" >&2
    echo "$unexpected_workflow_core_ui" >&2
    failed=1
fi

unexpected_download_status_io=$(rg -nH 'std::fs|reqwest::|http::|controller::' \
    src/update/download_status.rs || true)
if [ -n "$unexpected_download_status_io" ]; then
    echo "Architecture structure: download status projection owns I/O/controller dependencies:" >&2
    echo "$unexpected_download_status_io" >&2
    failed=1
fi

unexpected_ai_ui_engine=$(rg -nH 'LocalStateStore|PersistentStore|std::fs::' \
    src/ai/settings_ui.rs || true)
if [ -n "$unexpected_ai_ui_engine" ]; then
    echo "Architecture structure: AI Settings UI bypasses its feature facade:" >&2
    echo "$unexpected_ai_ui_engine" >&2
    failed=1
fi

# Provider switching owns only visit-local UI drafts. Durable mutations and
# asynchronous work remain behind the AI settings facade and save binding.
unexpected_provider_draft_work=$(rg -nUH \
    '\bstd\s*::\s*(fs|process|thread)\b|\buse\s+std\s*::\s*\{[^;]*\b(fs|process|thread)\b|\b(tokio|reqwest|LocalStateStore|PersistentStore)\b|\bcrate\s*::\s*http\b|\buse\s+crate\s*::\s*\{[^;]*\bhttp\b' \
    src/settings_page/ai_agent_drafts.rs || true)
if [ -n "$unexpected_provider_draft_work" ]; then
    echo "Architecture structure: AI provider draft selection owns I/O, workers or concrete Stores:" >&2
    echo "$unexpected_provider_draft_work" >&2
    failed=1
fi

# These owners only transform configuration, captured inputs or presentation.
# Store mutations, persistence and scheduling remain in the feature facades.
# Capture/projection may use Slint; the captured identity itself is raw data.
unexpected_ai_profile_owner_work=$(rg -nUH \
    '\bstd\s*::\s*(fs|io|net|process|thread)\b|\buse\s+std\s*::\s*\{[^;]*\b(fs|io|net|process|thread)\b|\b(tokio|reqwest|LocalStateStore|PersistentStore)\b|\bcrate\s*::\s*http\b|\buse\s+crate\s*::\s*\{[^;]*\bhttp\b' \
    src/settings/ai_agents.rs src/settings_page/ai_agent_projection.rs src/settings_page/ai_agent_list.rs \
    src/ai/settings_ui/admission.rs src/ai/identity.rs \
    src/ai/settings_state/actions.rs src/ai/settings_ui/delete_confirmation.rs src/ai/settings_ui/feedback.rs \
    src/ai_review/request_capture.rs src/ai_review/agent_identity.rs || true)
if [ -n "$unexpected_ai_profile_owner_work" ]; then
    echo "Architecture structure: AI Agent configuration, identity or capture owns I/O, workers or concrete Stores:" >&2
    echo "$unexpected_ai_profile_owner_work" >&2
    failed=1
fi

unexpected_agent_probe_work=$(rg -nUH \
    '\bstd\s*::\s*(fs|io|net|process|thread)\b|\buse\s+std\s*::\s*\{[^;]*\b(fs|io|net|process|thread)\b|\b(reqwest|LocalStateStore|PersistentStore|MainWindow|SettingsPageBridge)\b|\bcrate\s*::\s*http\b|\buse\s+crate\s*::\s*\{[^;]*\bhttp\b' \
    src/ai/settings_ui/connection_test.rs src/ai/settings_ui/list_connection_test.rs || true)
if [ -n "$unexpected_agent_probe_work" ]; then
    echo "Architecture structure: AI Agent connection probe bypasses its transport or settings facade:" >&2
    echo "$unexpected_agent_probe_work" >&2
    failed=1
fi

unexpected_agent_probe_lifecycle=$(rg -nUH \
    '\bstd\s*::\s*(fs|io|net|process|thread)\b|\buse\s+std\s*::\s*\{[^;]*\b(fs|io|net|process|thread)\b|\b(reqwest|LocalStateStore|PersistentStore|MainWindow|SettingsPageBridge|GitHubSettingsPage|AppState)\b|\bslint\s*::|\bcrate\s*::\s*(http|app_runtime)\b|\buse\s+crate\s*::\s*\{[^;]*\b(http|app_runtime)\b|\b(tokio\s*::\s*(spawn|runtime)|tokio\s*::\s*task\s*::\s*spawn)|\bagent\s*::\s*test_connection\s*\(' \
    src/ai/settings_ui/connection_probe.rs || true)
if [ -n "$unexpected_agent_probe_lifecycle" ]; then
    echo "Architecture structure: AI Agent shared probe lifecycle owns UI, transport, scheduling or I/O:" >&2
    echo "$unexpected_agent_probe_lifecycle" >&2
    failed=1
fi

unexpected_agent_headers_work=$(rg -nUH \
    '\bstd\s*::\s*(fs|io|net|process|thread)\b|\buse\s+std\s*::\s*\{[^;]*\b(fs|io|net|process|thread)\b|\b(tokio|LocalStateStore|PersistentStore|MainWindow|SettingsPageBridge)\b|\b(slint|controller)\s*::|\b(agent\s*::\s*complete(_review)?|http\s*::\s*(get|post|put|send|send_json))\s*\(' \
    src/ai/request_headers.rs || true)
if [ -n "$unexpected_agent_headers_work" ]; then
    echo "Architecture structure: AI Agent request headers owns UI, I/O, scheduling or concrete Stores:" >&2
    echo "$unexpected_agent_headers_work" >&2
    failed=1
fi

unexpected_agent_actions_ui=$(rg -nUH \
    '\b(MainWindow|SettingsPageBridge|ControllerInner)\b|\b(slint|controller)\s*::' \
    src/ai/settings_state/actions.rs || true)
if [ -n "$unexpected_agent_actions_ui" ]; then
    echo "Architecture structure: AI Agent registry actions depend on UI/controller types:" >&2
    echo "$unexpected_agent_actions_ui" >&2
    failed=1
fi

unexpected_header_object_work=$(rg -nUH \
    '\bstd\s*::\s*(fs|io|net|process|thread)\b|\buse\s+std\s*::\s*\{[^;]*\b(fs|io|net|process|thread)\b|\b(tokio|reqwest|LocalStateStore|PersistentStore|MainWindow|SettingsPageBridge|SettingsManager|app_runtime)\b|\b(slint|http)\s*::' \
    src/settings_page/ai_agent_header_object.rs || true)
if [ -n "$unexpected_header_object_work" ]; then
    echo "Architecture structure: AI Agent Header object parser owns UI, I/O, scheduling or Stores:" >&2
    echo "$unexpected_header_object_work" >&2
    failed=1
fi

unexpected_header_draft_work=$(rg -nUH \
    '\bstd\s*::\s*(fs|io|net|process|thread)\b|\buse\s+std\s*::\s*\{[^;]*\b(fs|io|net|process|thread)\b|\b(tokio|reqwest|LocalStateStore|PersistentStore|SettingsManager|app_runtime)\b|\b(http)\s*::' \
    src/settings_page/ai_agent_header_drafts.rs || true)
if [ -n "$unexpected_header_draft_work" ]; then
    echo "Architecture structure: AI Agent Header draft modes bypass their parser/validation facade:" >&2
    echo "$unexpected_header_draft_work" >&2
    failed=1
fi

# Chat's request owner may await the established transport facade and refer to
# shared HTTP error types. It does not own raw clients, files or Store engines.
unexpected_chat_request_work=$(rg -nUH \
    '\bstd\s*::\s*(fs|io|net|process|thread)\b|\buse\s+std\s*::\s*\{[^;]*\b(fs|io|net|process|thread)\b|\b(reqwest|LocalStateStore|PersistentStore)\b' \
    src/chat/request.rs || true)
if [ -n "$unexpected_chat_request_work" ]; then
    echo "Architecture structure: Chat request capture owns raw I/O, clients or concrete Stores:" >&2
    echo "$unexpected_chat_request_work" >&2
    failed=1
fi

unexpected_notification_engine=$(rg -nH 'LocalStateStore|PersistentStore|controller::|settings_state::' \
    src/notification/ingress.rs || true)
if [ -n "$unexpected_notification_engine" ]; then
    echo "Architecture structure: notification ingress bypasses its message command facade:" >&2
    echo "$unexpected_notification_engine" >&2
    failed=1
fi

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
