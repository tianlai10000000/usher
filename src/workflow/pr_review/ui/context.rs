#[cfg(feature = "live-preview")]
use usher_ui::PrReviewHostBridge as PageHostBridge;
#[cfg(not(feature = "live-preview"))]
use usher_ui_pr_review::PrReviewHostBridge as PageHostBridge;
// Scalar shell geometry and chrome summaries; PR models never cross this file.
use super::PrReviewUiRoot;
use crate::MainWindow;
use slint::ComponentHandle;

pub(super) fn bind(window: &MainWindow, page: &PrReviewUiRoot) {
    let host = window.as_weak();
    let product = page.as_weak();
    window
        .global::<usher_ui::PrReviewHostBridge>()
        .on_sync(move || {
            if let (Some(window), Some(page)) = (host.upgrade(), product.upgrade()) {
                sync(&window, &page);
            }
        });
    let host = window.as_weak();
    let product = page.as_weak();
    page.global::<crate::PrReviewBridge>()
        .on_shell_summary_changed(move || {
            if let (Some(window), Some(page)) = (host.upgrade(), product.upgrade()) {
                summary(&window, &page);
            }
        });
    let product = page.as_weak();
    window
        .global::<usher_ui::PrReviewHostBridge>()
        .on_header_action(move |action| {
            let Some(page) = product.upgrade() else {
                return;
            };
            let state = page.global::<crate::PrReviewBridge>();
            match action.as_str() {
                "review" => state.set_workflow_pr_review_dialog_open(true),
                "merge" => state.set_workflow_pr_merge_dialog_open(true),
                "revert" => state.set_workflow_pr_revert_dialog_open(true),
                "conflicts" => page.global::<crate::PrConflictsBridge>().invoke_open(),
                _ => (),
            }
        });
    let product = page.as_weak();
    window
        .global::<usher_ui::PrReviewHostBridge>()
        .on_open_review(move |a, b, c, d, e, f, g| {
            if let Some(page) = product.upgrade() {
                page.global::<crate::PrReviewBridge>()
                    .invoke_workflow_pr_review_open(a, b, c, d, e, f, g);
            }
        });
    let host = window.as_weak();
    page.global::<PageHostBridge>().on_open_settings(move || {
        if let Some(window) = host.upgrade() {
            window.invoke_navigate(crate::Route::Settings);
        }
    });
    let host = window.as_weak();
    page.global::<PageHostBridge>()
        .on_toggle_ai_review(move || {
            if let Some(window) = host.upgrade() {
                window.invoke_ai_review_toggle_current();
            }
        });
}

pub(super) fn sync(window: &MainWindow, page: &PrReviewUiRoot) {
    #[cfg(not(feature = "live-preview"))]
    {
        let host = window
            .global::<usher_ui::PrReviewHostBridge>()
            .get_context();
        let target = usher_ui_pr_review::PrReviewHostContext {
            active: host.active,
            width: host.width,
            height: host.height,
            titlebar_inset: host.titlebar_inset,
            content_x: host.content_x,
            content_width: host.content_width,
            content_height: host.content_height,
            live_resizing: host.live_resizing,
            project_name: host.project_name.clone(),
            provider: host.provider.clone(),
            code_font_family: host.code_font_family.clone(),
            code_font_size: host.code_font_size,
            tree_font_family: host.tree_font_family.clone(),
            font_family: host.font_family.clone(),
            theme_mode: host.theme_mode,
            dark_mode: host.dark_mode,
            ai_status: host.ai_status.clone(),
            ai_finding_count: host.ai_finding_count,
            ai_visible: host.ai_visible,
            user_avatar: host.user_avatar.clone(),
        };
        let app = page.global::<usher_ui_pr_review::AppState>();
        app.set_theme_mode(target.theme_mode);
        app.set_effective_dark_mode(target.dark_mode);
        app.set_user_avatar(target.user_avatar.clone());
        let leaving = page.global::<PageHostBridge>().get_context().active && !target.active;
        page.global::<usher_ui_pr_review::PrReviewHostBridge>()
            .set_context(target);
        if leaving {
            // This root is retained after its document is destroyed. Revoke
            // metadata leases immediately instead of waiting for their timer.
            page.global::<usher_ui_pr_review::TooltipOverlayState>()
                .invoke_dismiss_all();
            page.global::<usher_ui_pr_review::TransientOverlayState>()
                .invoke_dismiss_all();
        }
    }
    #[cfg(feature = "live-preview")]
    let _ = window;
    summary(window, page);
}

fn summary(window: &MainWindow, page: &PrReviewUiRoot) {
    let source = page.global::<crate::PrReviewBridge>();
    let target = window.global::<usher_ui::PrReviewHostBridge>();
    target.set_review_identity(source.get_workflow_pr_review_number());
    target.set_workflow_pr_review_visible(source.get_workflow_pr_review_visible());
    target.set_workflow_pr_action_loading(source.get_workflow_pr_action_loading());
    target.set_workflow_pr_can_comment(source.get_workflow_pr_can_comment());
    target.set_workflow_pr_can_approve(source.get_workflow_pr_can_approve());
    target.set_workflow_pr_can_request_changes(source.get_workflow_pr_can_request_changes());
    target.set_workflow_pr_remote_review_state(source.get_workflow_pr_remote_review_state());
    target.set_workflow_pr_can_merge(source.get_workflow_pr_can_merge());
    target.set_workflow_pr_can_revert(source.get_workflow_pr_can_revert());
    target.set_workflow_pr_merge_readiness_kind(source.get_workflow_pr_merge_readiness_kind());
    target.set_workflow_pr_merge_readiness_label(source.get_workflow_pr_merge_readiness_label());
    target.set_workflow_pr_merge_readiness_detail(source.get_workflow_pr_merge_readiness_detail());
    target.set_workflow_pr_merge_block_reason(source.get_workflow_pr_merge_block_reason());
    target.set_workflow_pr_revert_action_label(source.get_workflow_pr_revert_action_label());
    target.set_workflow_pr_merge_method_merge(source.get_workflow_pr_merge_method_merge());
    target.set_workflow_pr_merge_method_squash(source.get_workflow_pr_merge_method_squash());
    target.set_workflow_pr_merge_method_rebase(source.get_workflow_pr_merge_method_rebase());
    target.set_workflow_pr_merge_method_fast_forward(
        source.get_workflow_pr_merge_method_fast_forward(),
    );
    synchronize_modal(window, page);
}

pub(super) fn synchronize_modal(window: &MainWindow, page: &PrReviewUiRoot) {
    let source = page.global::<crate::PrReviewBridge>();
    let modal_visible = source.get_workflow_pr_review_dialog_open()
        || source.get_workflow_pr_dismiss_review_dialog_open()
        || source.get_workflow_pr_merge_dialog_open()
        || source.get_workflow_pr_revert_dialog_open()
        || source.get_workflow_pr_commit_range_dialog_open();
    window
        .global::<usher_ui::PrReviewHostBridge>()
        .set_modal_visible(modal_visible);
    #[cfg(not(feature = "live-preview"))]
    page.global::<PageHostBridge>()
        .set_modal_visible(modal_visible);
}
