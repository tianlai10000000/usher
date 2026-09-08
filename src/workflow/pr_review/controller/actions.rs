//! Remote review mutations and provider capability presentation.
use super::super::state::ReviewRequestKind;
use super::WorkflowPrReviewController;
use crate::MainWindow;
use crate::git::{
    GitResult, PullRequestActionState, PullRequestMergeBlockKind, PullRequestMergeMethod,
    PullRequestRevertStrategy, PullRequestReviewAction, dismiss_pull_request_review,
    get_pull_request_action_state, merge_pull_request, revert_pull_request,
    submit_pull_request_review,
};
use crate::project::ProjectRepositoryMetadata;
use crate::workflow::bindings::{show_error, show_success};
use slint::ComponentHandle;
use std::future::Future;
use std::sync::atomic::Ordering;

impl WorkflowPrReviewController {
    fn action_context(&self) -> Option<(ProjectRepositoryMetadata, String)> {
        self.cache.lock().ok().and_then(|guard| {
            let cache = guard.as_ref()?;
            Some((cache.metadata.clone(), cache.request.number.clone()))
        })
    }

    fn run_action<T, F, C>(&self, ui: &MainWindow, future: F, completion: C)
    where
        T: Send + 'static,
        F: Future<Output = GitResult<T>> + Send + 'static,
        C: FnOnce(&WorkflowPrReviewController, &MainWindow, GitResult<T>) + Send + 'static,
    {
        let generation = self.state.begin(ReviewRequestKind::RemoteMutation);
        ui.set_workflow_pr_action_loading(true);
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let result = future.await;
            let _ = slint::invoke_from_event_loop(move || {
                if !controller
                    .state
                    .is_current(ReviewRequestKind::RemoteMutation, generation)
                {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_workflow_pr_action_loading(false);
                completion(&controller, &ui, result);
            });
        });
    }

    pub(super) fn refresh_action_state(&self, ui: &MainWindow) {
        let Some((metadata, pull_request_iid)) = self.action_context() else {
            return;
        };
        if ui.get_workflow_pr_merge_readiness_label().trim().is_empty() {
            ui.set_workflow_pr_merge_readiness_kind("checking".into());
            ui.set_workflow_pr_merge_readiness_label(
                crate::i18n::tr("pr_review.checking_mergeability")
                    .into_owned()
                    .into(),
            );
            ui.set_workflow_pr_merge_readiness_detail(
                crate::i18n::tr("pr_review.mergeability_waiting")
                    .into_owned()
                    .into(),
            );
        }
        let generation = self.state.begin(ReviewRequestKind::RemoteActionState);
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let result = get_pull_request_action_state(
                metadata.repo_type.as_str(),
                metadata.repo_project_id.as_str(),
                metadata.remote_repo.as_str(),
                pull_request_iid.as_str(),
            )
            .await;
            let _ = slint::invoke_from_event_loop(move || {
                if !controller
                    .state
                    .is_current(ReviewRequestKind::RemoteActionState, generation)
                {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(state) => {
                        if let Ok(mut cached) = controller.action_state.lock() {
                            *cached = Some(state.clone());
                        }
                        apply_action_state(&ui, &state);
                    }
                    Err(error) => {
                        log::warn!(
                            "[Workflow][PR Review] Failed to load remote action state: {error}"
                        );
                        reset_action_capabilities(&ui);
                        ui.set_workflow_pr_merge_readiness_kind("unavailable".into());
                        ui.set_workflow_pr_merge_readiness_label(
                            crate::i18n::tr("pr_review.merge_status_unavailable")
                                .into_owned()
                                .into(),
                        );
                        ui.set_workflow_pr_merge_readiness_detail(
                            crate::i18n::tr("pr_review.mergeability_unavailable")
                                .into_owned()
                                .into(),
                        );
                        ui.set_workflow_pr_merge_block_reason(
                            crate::i18n::tr(error.user_message_key())
                                .into_owned()
                                .into(),
                        );
                    }
                }
            });
        });
    }

    pub(super) fn submit_review(&self, ui: &MainWindow, action: &str, body: &str) {
        let Some(action) = PullRequestReviewAction::parse(action) else {
            show_error(
                ui,
                crate::i18n::tr("pr_review.invalid_review_action").as_ref(),
            );
            return;
        };
        let allowed = match action {
            PullRequestReviewAction::Comment => ui.get_workflow_pr_can_comment(),
            PullRequestReviewAction::Approve => ui.get_workflow_pr_can_approve(),
            PullRequestReviewAction::RequestChanges => ui.get_workflow_pr_can_request_changes(),
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
        let Some((metadata, pull_request_iid)) = self.action_context() else {
            show_error(
                ui,
                crate::i18n::tr("pr_review.repository_context_unavailable").as_ref(),
            );
            return;
        };
        let body = body.to_string();
        self.run_action(
            ui,
            async move {
                submit_pull_request_review(
                    metadata.repo_type.as_str(),
                    metadata.repo_project_id.as_str(),
                    metadata.remote_repo.as_str(),
                    pull_request_iid.as_str(),
                    action,
                    body.as_str(),
                )
                .await
            },
            move |controller, ui, result| match result {
                Ok(()) => {
                    ui.set_workflow_pr_review_dialog_open(false);
                    if action == PullRequestReviewAction::Comment {
                        let count = ui.get_workflow_pr_review_conversation_count();
                        ui.set_workflow_pr_review_conversation_count(count.saturating_add(1));
                    }
                    ui.set_workflow_pr_remote_review_state(
                        match action {
                            PullRequestReviewAction::Comment => "commented",
                            PullRequestReviewAction::Approve => "approved",
                            PullRequestReviewAction::RequestChanges => "rejected",
                        }
                        .into(),
                    );
                    show_success(
                        ui,
                        if action == PullRequestReviewAction::Comment {
                            crate::i18n::tr("pr_review.comment_submitted")
                        } else {
                            crate::i18n::tr("pr_review.review_submitted")
                        }
                        .as_ref(),
                    );
                    controller.refresh_action_state(ui);
                    controller.refresh_conversation_after_remote_action(ui);
                }
                Err(error) => {
                    log::warn!("[Workflow][PR Review] Failed to submit review: {error}");
                    show_error(ui, crate::i18n::tr(error.user_message_key()).as_ref());
                }
            },
        );
    }

    pub(super) fn dismiss_review(&self, ui: &MainWindow, review_id: &str, reason: &str) {
        if reason.trim().is_empty() {
            show_error(
                ui,
                crate::i18n::tr("pr_review.dismiss_reason_required").as_ref(),
            );
            return;
        }
        let Some((metadata, pull_request_iid)) = self.action_context() else {
            show_error(
                ui,
                crate::i18n::tr("pr_review.repository_context_unavailable").as_ref(),
            );
            return;
        };
        let review_id = review_id.to_string();
        let reason = reason.to_string();
        self.run_action(
            ui,
            async move {
                dismiss_pull_request_review(
                    metadata.repo_type.as_str(),
                    metadata.repo_project_id.as_str(),
                    metadata.remote_repo.as_str(),
                    pull_request_iid.as_str(),
                    review_id.as_str(),
                    reason.as_str(),
                )
                .await
            },
            |controller, ui, result| match result {
                Ok(()) => {
                    ui.set_workflow_pr_dismiss_review_dialog_open(false);
                    let count = ui.get_workflow_pr_review_conversation_count();
                    ui.set_workflow_pr_review_conversation_count(count.saturating_add(1));
                    show_success(ui, crate::i18n::tr("pr_review.dismissed").as_ref());
                    controller.refresh_action_state(ui);
                    controller.refresh_conversation_after_remote_action(ui);
                }
                Err(error) => {
                    log::warn!("[Workflow][PR Review] Failed to dismiss review: {error}");
                    show_error(ui, crate::i18n::tr(error.user_message_key()).as_ref());
                }
            },
        );
    }

    pub(super) fn merge(&self, ui: &MainWindow, method: &str, remove_source_branch: bool) {
        if !ui.get_workflow_pr_can_merge() {
            let reason = ui.get_workflow_pr_merge_block_reason();
            show_error(
                ui,
                if reason.trim().is_empty() {
                    crate::i18n::tr("pr_review.not_mergeable")
                } else {
                    std::borrow::Cow::Borrowed(reason.as_str())
                }
                .as_ref(),
            );
            return;
        }
        let Some(method) = PullRequestMergeMethod::parse(method) else {
            show_error(
                ui,
                crate::i18n::tr("pr_review.invalid_merge_method").as_ref(),
            );
            return;
        };
        let method_allowed = match method {
            PullRequestMergeMethod::Merge => ui.get_workflow_pr_merge_method_merge(),
            PullRequestMergeMethod::Squash => ui.get_workflow_pr_merge_method_squash(),
            PullRequestMergeMethod::Rebase => ui.get_workflow_pr_merge_method_rebase(),
            PullRequestMergeMethod::FastForward => ui.get_workflow_pr_merge_method_fast_forward(),
        };
        if !method_allowed {
            show_error(
                ui,
                crate::i18n::tr("pr_review.merge_method_disabled").as_ref(),
            );
            return;
        }
        let Some((metadata, pull_request_iid)) = self.action_context() else {
            show_error(
                ui,
                crate::i18n::tr("pr_review.repository_context_unavailable").as_ref(),
            );
            return;
        };
        self.run_action(
            ui,
            async move {
                merge_pull_request(
                    metadata.repo_type.as_str(),
                    metadata.repo_project_id.as_str(),
                    metadata.remote_repo.as_str(),
                    pull_request_iid.as_str(),
                    method,
                    remove_source_branch,
                )
                .await
            },
            |controller, ui, result| match result {
                Ok(()) => {
                    ui.set_workflow_pr_merge_dialog_open(false);
                    ui.set_workflow_pr_can_merge(false);
                    ui.set_workflow_pr_merge_readiness_kind("merged".into());
                    ui.set_workflow_pr_merge_readiness_label(
                        crate::i18n::tr("pr_review.merged").into_owned().into(),
                    );
                    ui.set_workflow_pr_merge_readiness_detail(
                        crate::i18n::tr("pr_review.merged_detail")
                            .into_owned()
                            .into(),
                    );
                    ui.set_workflow_pr_merge_block_reason(
                        crate::i18n::tr("pr_review.merged_block_reason")
                            .into_owned()
                            .into(),
                    );
                    controller
                        .pr_list_refresh_pending
                        .store(true, Ordering::SeqCst);
                    show_success(
                        ui,
                        crate::i18n::tr("pr_review.merged_successfully").as_ref(),
                    );
                    controller.refresh_action_state(ui);
                    controller.refresh_conversation_after_remote_action(ui);
                }
                Err(error) => {
                    log::warn!("[Workflow][PR Review] Failed to merge pull request: {error}");
                    show_error(ui, crate::i18n::tr(error.user_message_key()).as_ref());
                    controller.refresh_action_state(ui);
                }
            },
        );
    }

    pub(super) fn revert(&self, ui: &MainWindow) {
        if !ui.get_workflow_pr_can_revert() {
            let reason = ui.get_workflow_pr_revert_block_reason();
            show_error(
                ui,
                if reason.trim().is_empty() {
                    crate::i18n::tr("pr_review.revert_not_available")
                } else {
                    std::borrow::Cow::Borrowed(reason.as_str())
                }
                .as_ref(),
            );
            return;
        }
        let Some((metadata, pull_request_iid)) = self.action_context() else {
            show_error(
                ui,
                crate::i18n::tr("pr_review.repository_context_unavailable").as_ref(),
            );
            return;
        };
        self.run_action(
            ui,
            async move {
                revert_pull_request(
                    metadata.repo_type.as_str(),
                    metadata.repo_project_id.as_str(),
                    metadata.remote_repo.as_str(),
                    pull_request_iid.as_str(),
                )
                .await
            },
            |_controller, ui, result| match result {
                    Ok(revert) => {
                        ui.set_workflow_pr_revert_dialog_open(false);
                        ui.set_workflow_pr_can_revert(false);
                        ui.set_workflow_pr_revert_block_reason(
                            crate::i18n::tr("pr_review.revert_completed")
                                .into_owned()
                                .into(),
                        );
                        show_success(ui, revert.message.as_str());
                        log::info!(
                            "[Workflow][PR Review] Completed revert action reference={} title_present={} url_present={}",
                            revert.number,
                            !revert.title.trim().is_empty(),
                            !revert.web_url.trim().is_empty()
                        );
                    }
                    Err(error) => {
                        log::warn!("[Workflow][PR Review] Revert action failed: {error}");
                        show_error(ui, crate::i18n::tr(error.user_message_key()).as_ref());
                    }
                },
        );
    }
}

pub(super) fn apply_action_state(ui: &MainWindow, state: &PullRequestActionState) {
    ui.set_workflow_pr_can_comment(state.can_comment);
    ui.set_workflow_pr_can_approve(state.can_approve);
    ui.set_workflow_pr_can_request_changes(state.can_request_changes);
    ui.set_workflow_pr_can_merge(state.can_merge);
    ui.set_workflow_pr_can_revert(state.can_revert);
    if !state.review_state.trim().is_empty() {
        ui.set_workflow_pr_remote_review_state(state.review_state.clone().into());
    }
    ui.set_workflow_pr_merge_readiness_kind(state.readiness.kind.as_str().into());
    ui.set_workflow_pr_merge_readiness_label(
        crate::i18n::tr(state.readiness.kind.label_key())
            .into_owned()
            .into(),
    );
    let readiness_detail = crate::i18n::tr(state.readiness.kind.detail_key()).into_owned();
    ui.set_workflow_pr_merge_readiness_detail(readiness_detail.clone().into());
    let merge_block_reason = match state.merge_block_kind {
        PullRequestMergeBlockKind::None => String::new(),
        PullRequestMergeBlockKind::ConfigureProvider => {
            crate::i18n::tr("pr_review.configure_provider_for_merge").into_owned()
        }
        PullRequestMergeBlockKind::Readiness => readiness_detail,
        PullRequestMergeBlockKind::Permission => {
            crate::i18n::tr("pr_review.merge_permission_required").into_owned()
        }
    };
    ui.set_workflow_pr_merge_block_reason(merge_block_reason.into());
    ui.set_workflow_pr_revert_block_reason(
        if state.can_revert {
            String::new()
        } else {
            crate::i18n::tr("pr_review.revert_not_available").into_owned()
        }
        .into(),
    );
    let (revert_action_key, revert_confirmation_key) = match state.revert_strategy {
        PullRequestRevertStrategy::PullRequest => (
            "pr_review.create_revert_pr",
            "pr_review.revert_confirmation_pull_request",
        ),
        PullRequestRevertStrategy::DirectCommit => (
            "pr_review.revert_pull_request",
            "pr_review.revert_confirmation_direct_commit",
        ),
        PullRequestRevertStrategy::Unsupported => (
            "pr_review.revert_changes",
            "pr_review.revert_confirmation_unsupported",
        ),
    };
    ui.set_workflow_pr_revert_action_label(crate::i18n::tr(revert_action_key).into_owned().into());
    ui.set_workflow_pr_revert_confirmation(
        crate::i18n::tr(revert_confirmation_key).into_owned().into(),
    );
    ui.set_workflow_pr_merge_method_merge(
        state
            .allowed_merge_methods
            .contains(&PullRequestMergeMethod::Merge),
    );
    ui.set_workflow_pr_merge_method_squash(
        state
            .allowed_merge_methods
            .contains(&PullRequestMergeMethod::Squash),
    );
    ui.set_workflow_pr_merge_method_rebase(
        state
            .allowed_merge_methods
            .contains(&PullRequestMergeMethod::Rebase),
    );
    ui.set_workflow_pr_merge_method_fast_forward(
        state
            .allowed_merge_methods
            .contains(&PullRequestMergeMethod::FastForward),
    );
}

pub(super) fn readiness_label_key(kind: &str) -> &'static str {
    match kind {
        "checking" => "pr_review.readiness.checking",
        "ready" => "pr_review.readiness.ready",
        "conflicts" => "pr_review.readiness.conflicts",
        "draft" => "pr_review.readiness.draft",
        "review_required" => "pr_review.readiness.review_required",
        "changes_requested" => "pr_review.readiness.changes_requested",
        "checks_pending" => "pr_review.readiness.checks_pending",
        "checks_failed" => "pr_review.readiness.checks_failed",
        "branch_out_of_date" => "pr_review.readiness.branch_out_of_date",
        "discussions_unresolved" => "pr_review.readiness.discussions_unresolved",
        "blocked" => "pr_review.readiness.blocked",
        "merged" => "pr_review.readiness.merged",
        "closed" => "pr_review.readiness.closed",
        _ => "pr_review.readiness.unavailable",
    }
}

pub(super) fn reset_action_capabilities(ui: &MainWindow) {
    ui.set_workflow_pr_can_comment(false);
    ui.set_workflow_pr_can_approve(false);
    ui.set_workflow_pr_can_request_changes(false);
    ui.set_workflow_pr_can_merge(false);
    ui.set_workflow_pr_can_revert(false);
    ui.set_workflow_pr_merge_method_merge(false);
    ui.set_workflow_pr_merge_method_squash(false);
    ui.set_workflow_pr_merge_method_rebase(false);
    ui.set_workflow_pr_merge_method_fast_forward(false);
}

pub(super) fn reset_action_ui(ui: &MainWindow) {
    ui.set_workflow_pr_review_dialog_open(false);
    ui.set_workflow_pr_dismiss_review_dialog_open(false);
    ui.set_workflow_pr_merge_dialog_open(false);
    ui.set_workflow_pr_revert_dialog_open(false);
    ui.set_workflow_pr_action_loading(false);
    ui.set_workflow_pr_remote_review_state("".into());
    ui.set_workflow_pr_merge_readiness_kind("".into());
    ui.set_workflow_pr_merge_readiness_label("".into());
    ui.set_workflow_pr_merge_readiness_detail("".into());
    ui.set_workflow_pr_merge_block_reason("".into());
    ui.set_workflow_pr_revert_block_reason("".into());
    ui.set_workflow_pr_revert_action_label(
        crate::i18n::tr("pr_review.revert_pull_request")
            .into_owned()
            .into(),
    );
    ui.set_workflow_pr_revert_confirmation("".into());
    reset_action_capabilities(ui);
}
