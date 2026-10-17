//! Review submission/dismissal owns validation, captured remote context and
//! completion-driven list/conversation refresh. The parent owns generation
//! guarding and merge/revert operations; the UI bridge owns no remote work.
use super::*;

impl WorkflowPrReviewController {
    pub(in crate::workflow::pr_review::controller) fn submit_review(
        &self,
        ui: &MainWindow,
        action: &str,
        body: &str,
    ) {
        let Some(action) = PullRequestReviewAction::parse(action) else {
            show_error(
                ui,
                crate::i18n::tr("pr_review.invalid_review_action").as_ref(),
            );
            return;
        };
        let allowed = match action {
            PullRequestReviewAction::Comment => ui
                .pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_can_comment(),
            PullRequestReviewAction::Approve => ui
                .pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_can_approve(),
            PullRequestReviewAction::RequestChanges => ui
                .pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_can_request_changes(),
        };
        if !allowed {
            show_error(
                ui,
                crate::i18n::tr("pr_review.review_action_unsupported").as_ref(),
            );
            return;
        }
        if action != PullRequestReviewAction::Approve && body.trim().is_empty() {
            show_error(ui, crate::i18n::tr("pr_review.comment_required").as_ref());
            return;
        }
        let Some(context) = self.hook_context(ui) else {
            show_error(
                ui,
                crate::i18n::tr("pr_review.repository_context_unavailable").as_ref(),
            );
            return;
        };
        self.run_action(
            ui,
            context.submit_review(action, body.to_string()),
            move |controller, ui, result| match result {
                Ok(()) => {
                    apply_submitted_review(ui, action);
                    show_success(
                        ui,
                        if action == PullRequestReviewAction::Comment {
                            crate::i18n::tr("pr_review.comment_submitted")
                        } else {
                            crate::i18n::tr("pr_review.review_submitted")
                        }
                        .as_ref(),
                    );
                    controller.refresh_after_review_mutation(ui);
                }
                Err(error) => {
                    log::warn!("[Workflow][PR Review] Failed to submit review: {error}");
                    show_error(ui, crate::i18n::tr(error.user_message_key()).as_ref());
                }
            },
        );
    }

    pub(in crate::workflow::pr_review::controller) fn dismiss_review(
        &self,
        ui: &MainWindow,
        review_id: &str,
        reason: &str,
    ) {
        if reason.trim().is_empty() {
            show_error(
                ui,
                crate::i18n::tr("pr_review.dismiss_reason_required").as_ref(),
            );
            return;
        }
        let Some(target) = self.action_context() else {
            show_error(
                ui,
                crate::i18n::tr("pr_review.repository_context_unavailable").as_ref(),
            );
            return;
        };
        self.run_action(
            ui,
            target.dismiss(review_id.to_string(), reason.to_string()),
            |controller, ui, result| match result {
                Ok(()) => {
                    ui.pr_review()
                        .global::<crate::PrReviewBridge>()
                        .set_workflow_pr_dismiss_review_dialog_open(false);
                    let count = ui
                        .pr_review()
                        .global::<crate::PrReviewBridge>()
                        .get_workflow_pr_review_conversation_count();
                    ui.pr_review()
                        .global::<crate::PrReviewBridge>()
                        .set_workflow_pr_review_conversation_count(count.saturating_add(1));
                    show_success(ui, crate::i18n::tr("pr_review.dismissed").as_ref());
                    controller.refresh_after_review_mutation(ui);
                }
                Err(error) => {
                    log::warn!("[Workflow][PR Review] Failed to dismiss review: {error}");
                    show_error(ui, crate::i18n::tr(error.user_message_key()).as_ref());
                }
            },
        );
    }

    pub(in crate::workflow::pr_review::controller) fn refresh_after_review_mutation(
        &self,
        ui: &MainWindow,
    ) {
        self.pr_list_refresh_pending.store(true, Ordering::SeqCst);
        self.refresh_action_state(ui);
        self.refresh_conversation_after_remote_action(ui);
    }
}
