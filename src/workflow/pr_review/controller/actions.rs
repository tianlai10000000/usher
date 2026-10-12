//! Remote review mutations and provider capability presentation.
use super::super::state::ReviewRequestKind;
use super::WorkflowPrReviewController;
use crate::MainWindow;
use crate::git::{
    GitResult, PullRequestActionState, PullRequestMergeBlockKind, PullRequestMergeMethod,
    PullRequestReadinessKind, PullRequestRevertStrategy, PullRequestReviewAction,
    dismiss_pull_request_review, get_pull_request_action_state, merge_pull_request,
    revert_pull_request, submit_pull_request_review,
};
use crate::project::ProjectRepositoryMetadata;
use crate::workflow::bindings::{show_error, show_success};
use crate::workflow::pr_data::{PrKey, shared as shared_pr_data};
use slint::ComponentHandle;
use std::future::Future;
use std::sync::atomic::Ordering;

#[derive(Clone)]
struct PrHookContext {
    metadata: ProjectRepositoryMetadata,
    request: super::ReviewRequest,
    author_identity: crate::git::PullRequestAuthorIdentity,
    project: String,
    cwd: String,
}

fn pull_request_hook_event(
    context: &PrHookContext,
    name: &str,
    source: &str,
    action: &str,
) -> crate::hooks::HookTrigger {
    let mut event = crate::hooks::HookTrigger::new(
        name,
        source,
        serde_json::json!({
            "number": context.request.number,
            "provider": context.metadata.repo_type,
            "repository": context.metadata.remote_repo,
            "action": action,
            "title": context.request.title,
            "source_branch": context.request.source_branch,
            "target_branch": context.request.target_branch,
            "author": context.request.author,
            "author_name": context.author_identity.name,
            "author_email": context.author_identity.email,
        }),
    );
    event.project = Some(context.project.clone());
    event.cwd = (!context.cwd.is_empty()).then(|| context.cwd.clone().into());
    event
}

fn review_hook_event(
    context: &PrHookContext,
    action: PullRequestReviewAction,
) -> Option<crate::hooks::HookTrigger> {
    (action == PullRequestReviewAction::Approve).then(|| {
        let mut event = pull_request_hook_event(
            context,
            crate::hooks::PR_REVIEW_APPROVED,
            "pr.review",
            "approved",
        );
        event.outcome = "approved".into();
        event
    })
}

fn merged_hook_event(
    context: &PrHookContext,
    method: PullRequestMergeMethod,
    remove_source_branch: bool,
) -> crate::hooks::HookTrigger {
    let mut event = pull_request_hook_event(context, crate::hooks::PR_MERGED, "pr.merge", "merged");
    let payload = event
        .payload
        .as_object_mut()
        .expect("PR Hook payload must be an object");
    payload.insert("merge_method".into(), method.as_str().into());
    payload.insert("remove_source_branch".into(), remove_source_branch.into());
    event
}

impl WorkflowPrReviewController {
    fn action_context(&self) -> Option<(ProjectRepositoryMetadata, String)> {
        self.cache.lock().ok().and_then(|guard| {
            let cache = guard.as_ref()?;
            Some((cache.metadata.clone(), cache.request.number.clone()))
        })
    }

    fn hook_context(&self, ui: &MainWindow) -> Option<PrHookContext> {
        self.cache.lock().ok().and_then(|guard| {
            let cache = guard.as_ref()?;
            Some(PrHookContext {
                metadata: cache.metadata.clone(),
                request: cache.request.clone(),
                author_identity: cache.all_detail.author_identity.clone(),
                project: ui.get_workflow_project_id().to_string(),
                cwd: ui.get_workflow_project_local_repo().to_string(),
            })
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
        let context = self.action_context();
        let alias = self.project_alias.lock().unwrap().clone();
        let number = self
            .active_request
            .lock()
            .unwrap()
            .as_ref()
            .map(|r| r.number.clone())
            .unwrap_or_default();
        if context.is_none() && (alias.is_empty() || number.is_empty()) {
            return;
        }
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
        let ticket = shared_pr_data().ticket();
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let context = match context {
                Some(context) => Ok(context),
                None => tokio::task::spawn_blocking(move || {
                    crate::project::load_repository_metadata(&alias)
                })
                .await
                .map_err(|_| {
                    crate::git::GitError::configuration("Repository metadata worker stopped")
                })
                .and_then(|result| result.map_err(crate::git::GitError::configuration))
                .map(|metadata| (metadata, number)),
            };
            let result = match context {
                Ok((metadata, pull_request_iid)) => {
                    let key = PrKey::new(&metadata, &pull_request_iid);
                    let result = get_pull_request_action_state(
                        metadata.repo_type.as_str(),
                        metadata.repo_project_id.as_str(),
                        metadata.remote_repo.as_str(),
                        pull_request_iid.as_str(),
                    )
                    .await;
                    let accepted = match &result {
                        Ok(state) => shared_pr_data().publish_action(ticket, &key, state.clone()),
                        Err(_) => shared_pr_data().publish_action_unavailable(ticket, &key),
                    };
                    if !accepted {
                        return;
                    }
                    result
                }
                Err(error) => Err(error),
            };
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
                        if controller.action_context().is_none() {
                            reset_action_capabilities(&ui);
                        }
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
        let Some(context) = self.hook_context(ui) else {
            show_error(
                ui,
                crate::i18n::tr("pr_review.repository_context_unavailable").as_ref(),
            );
            return;
        };
        let pull_request_iid = context.request.number.clone();
        let body = body.to_string();
        let hook_event = review_hook_event(&context, action);
        let metadata = context.metadata;
        let key = PrKey::new(&metadata, &pull_request_iid);
        self.run_action(
            ui,
            async move {
                let result = submit_pull_request_review(
                    metadata.repo_type.as_str(),
                    metadata.repo_project_id.as_str(),
                    metadata.remote_repo.as_str(),
                    pull_request_iid.as_str(),
                    action,
                    body.as_str(),
                )
                .await;
                if result.is_ok() {
                    let data = shared_pr_data();
                    data.publish_review(data.ticket(), &key, action);
                    if let Some(hook_event) = hook_event {
                        if let Err(error) = crate::hooks::emit(hook_event).await {
                            log::warn!("[Hooks] PR approval dispatch failed: {error}");
                        }
                    }
                }
                result
            },
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
        let key = PrKey::new(&metadata, &pull_request_iid);
        self.run_action(
            ui,
            async move {
                let result = dismiss_pull_request_review(
                    metadata.repo_type.as_str(),
                    metadata.repo_project_id.as_str(),
                    metadata.remote_repo.as_str(),
                    pull_request_iid.as_str(),
                    review_id.as_str(),
                    reason.as_str(),
                )
                .await;
                if result.is_ok() {
                    let data = shared_pr_data();
                    data.publish_dismissal(data.ticket(), &key);
                }
                result
            },
            |controller, ui, result| match result {
                Ok(()) => {
                    ui.set_workflow_pr_dismiss_review_dialog_open(false);
                    let count = ui.get_workflow_pr_review_conversation_count();
                    ui.set_workflow_pr_review_conversation_count(count.saturating_add(1));
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

    fn refresh_after_review_mutation(&self, ui: &MainWindow) {
        self.pr_list_refresh_pending.store(true, Ordering::SeqCst);
        self.refresh_action_state(ui);
        self.refresh_conversation_after_remote_action(ui);
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
        let Some(context) = self.hook_context(ui) else {
            show_error(
                ui,
                crate::i18n::tr("pr_review.repository_context_unavailable").as_ref(),
            );
            return;
        };
        let pull_request_iid = context.request.number.clone();
        let hook_event = merged_hook_event(&context, method, remove_source_branch);
        let metadata = context.metadata;
        let key = PrKey::new(&metadata, &pull_request_iid);
        self.run_action(
            ui,
            async move {
                let result = merge_pull_request(
                    metadata.repo_type.as_str(),
                    metadata.repo_project_id.as_str(),
                    metadata.remote_repo.as_str(),
                    pull_request_iid.as_str(),
                    method,
                    remove_source_branch,
                )
                .await;
                if result.is_ok() {
                    let data = shared_pr_data();
                    data.publish_merge(data.ticket(), &key);
                    if let Err(error) = crate::hooks::emit(hook_event).await {
                        log::warn!("[Hooks] PR merge dispatch failed: {error}");
                    }
                }
                result
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

fn apply_submitted_review(ui: &MainWindow, action: PullRequestReviewAction) {
    ui.set_workflow_pr_review_dialog_open(false);
    match action {
        PullRequestReviewAction::Comment => {
            let count = ui.get_workflow_pr_review_conversation_count();
            ui.set_workflow_pr_review_conversation_count(count.saturating_add(1));
            // Comment completion belongs to the composer, not the aggregate
            // review decision. Repeated comments must each clear the composer.
            let revision = ui.get_workflow_pr_comment_submission_revision();
            ui.set_workflow_pr_comment_submission_revision(revision.wrapping_add(1));
        }
        PullRequestReviewAction::Approve => {
            ui.set_workflow_pr_remote_review_state("approved".into());
        }
        PullRequestReviewAction::RequestChanges => {
            ui.set_workflow_pr_remote_review_state("rejected".into());
        }
    }
}

pub(super) fn apply_action_state(ui: &MainWindow, state: &PullRequestActionState) {
    ui.set_workflow_pr_can_comment(state.can_comment);
    ui.set_workflow_pr_can_approve(state.can_approve);
    ui.set_workflow_pr_can_request_changes(state.can_request_changes);
    ui.set_workflow_pr_can_merge(state.can_merge);
    ui.set_workflow_pr_can_revert(state.can_revert);
    // An empty authoritative decision clears a withdrawn or dismissed review.
    ui.set_workflow_pr_remote_review_state(state.review_state.clone().into());
    ui.set_workflow_pr_merge_readiness_kind(state.readiness.kind.as_str().into());
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
    retranslate_action_state(ui, state);
}

pub(super) fn retranslate_action_state(ui: &MainWindow, state: &PullRequestActionState) {
    // The cached provider snapshot can predate a successful local mutation.
    // Language refresh changes copy without restoring stale decisions/capabilities.
    let readiness = projected_readiness_kind(ui.get_workflow_pr_merge_readiness_kind().as_str());
    ui.set_workflow_pr_merge_readiness_label(
        crate::i18n::tr(readiness.label_key()).into_owned().into(),
    );
    let readiness_detail = crate::i18n::tr(readiness.detail_key()).into_owned();
    ui.set_workflow_pr_merge_readiness_detail(readiness_detail.clone().into());
    let merge_block_kind = if ui.get_workflow_pr_can_merge() {
        PullRequestMergeBlockKind::None
    } else if readiness != state.readiness.kind {
        PullRequestMergeBlockKind::Readiness
    } else {
        state.merge_block_kind
    };
    let merge_block_reason = match merge_block_kind {
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
        if ui.get_workflow_pr_can_revert() {
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
}

pub(super) fn readiness_label_key(kind: &str) -> &'static str {
    projected_readiness_kind(kind).label_key()
}

fn projected_readiness_kind(kind: &str) -> PullRequestReadinessKind {
    match kind {
        "checking" => PullRequestReadinessKind::Checking,
        "ready" => PullRequestReadinessKind::Ready,
        "conflicts" => PullRequestReadinessKind::Conflicts,
        "draft" => PullRequestReadinessKind::Draft,
        "review_required" => PullRequestReadinessKind::ReviewRequired,
        "changes_requested" => PullRequestReadinessKind::ChangesRequested,
        "checks_pending" => PullRequestReadinessKind::ChecksPending,
        "checks_failed" => PullRequestReadinessKind::ChecksFailed,
        "branch_out_of_date" => PullRequestReadinessKind::BranchOutOfDate,
        "discussions_unresolved" => PullRequestReadinessKind::DiscussionsUnresolved,
        "blocked" => PullRequestReadinessKind::Blocked,
        "merged" => PullRequestReadinessKind::Merged,
        "closed" => PullRequestReadinessKind::Closed,
        _ => PullRequestReadinessKind::Unavailable,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_approval_does_not_merge_or_change_merge_readiness() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        ui.set_workflow_pr_review_dialog_open(true);
        ui.set_workflow_pr_merge_readiness_kind("conflicts".into());
        ui.set_workflow_pr_can_merge(false);

        apply_submitted_review(&ui, PullRequestReviewAction::Approve);

        assert_eq!(ui.get_workflow_pr_remote_review_state(), "approved");
        assert_eq!(ui.get_workflow_pr_merge_readiness_kind(), "conflicts");
        assert!(!ui.get_workflow_pr_can_merge());
        assert!(!ui.get_workflow_pr_review_dialog_open());
        assert_eq!(ui.get_workflow_pr_comment_submission_revision(), 0);
    }

    #[test]
    fn consecutive_comments_preserve_review_decision_and_complete_each_composer_submit() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        for decision in ["approved", "rejected", ""] {
            ui.set_workflow_pr_remote_review_state(decision.into());
            let initial_revision = ui.get_workflow_pr_comment_submission_revision();
            let initial_count = ui.get_workflow_pr_review_conversation_count();
            for submission in 1..=2 {
                apply_submitted_review(&ui, PullRequestReviewAction::Comment);
                assert_eq!(ui.get_workflow_pr_remote_review_state(), decision);
                assert_eq!(
                    ui.get_workflow_pr_comment_submission_revision(),
                    initial_revision + submission
                );
                assert_eq!(
                    ui.get_workflow_pr_review_conversation_count(),
                    initial_count + submission
                );
            }
        }
    }

    #[test]
    fn authoritative_empty_review_state_clears_a_dismissed_approval() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let mut state = PullRequestActionState {
            can_comment: true,
            can_approve: true,
            can_request_changes: true,
            can_merge: false,
            can_revert: false,
            review_state: "approved".into(),
            readiness: crate::git::PullRequestReadiness::new(
                crate::git::PullRequestReadinessKind::ReviewRequired,
                "Review required",
            ),
            merge_block_kind: PullRequestMergeBlockKind::Readiness,
            revert_strategy: PullRequestRevertStrategy::Unsupported,
            allowed_merge_methods: vec![PullRequestMergeMethod::Merge],
        };
        apply_action_state(&ui, &state);
        assert_eq!(ui.get_workflow_pr_remote_review_state(), "approved");

        state.review_state.clear();
        apply_action_state(&ui, &state);

        assert_eq!(ui.get_workflow_pr_remote_review_state(), "");
        assert_eq!(ui.get_workflow_pr_merge_readiness_kind(), "review_required");
        assert!(!ui.get_workflow_pr_can_merge());
    }

    #[test]
    fn review_mutations_invalidate_the_retained_list_and_conversation() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        ui.set_workflow_pr_review_tab(2);
        let list = crate::workflow::pr_list::bind_pr_list(&ui);
        let controller = super::super::bind_pr_review(&ui, list);
        *controller.conversation_loaded.lock().unwrap() = true;
        assert!(!controller.pr_list_refresh_pending.load(Ordering::SeqCst));

        controller.refresh_after_review_mutation(&ui);

        assert!(controller.pr_list_refresh_pending.load(Ordering::SeqCst));
        assert!(!*controller.conversation_loaded.lock().unwrap());
    }

    #[test]
    fn pending_list_refresh_is_consumed_only_when_the_pr_list_returns() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let list = crate::workflow::pr_list::bind_pr_list(&ui);
        let controller = super::super::bind_pr_review(&ui, list);
        controller
            .pr_list_refresh_pending
            .store(true, Ordering::SeqCst);
        ui.set_current_route(crate::Route::PrReview);
        controller.refresh_list_on_return(&ui);
        assert!(controller.pr_list_refresh_pending.load(Ordering::SeqCst));
        ui.set_current_route(crate::Route::Workflow);
        ui.set_workflow_selected_flow(0);
        controller.refresh_list_on_return(&ui);
        assert!(controller.pr_list_refresh_pending.load(Ordering::SeqCst));
        ui.set_workflow_selected_flow(2);
        controller.refresh_list_on_return(&ui);
        assert!(!controller.pr_list_refresh_pending.load(Ordering::SeqCst));
    }

    #[test]
    fn worker_publishes_latest_approval_without_rebuilding_the_hidden_list() {
        use slint::{Model, ModelRc, VecModel};

        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let list = crate::workflow::pr_list::bind_pr_list(&ui);
        let _controller = super::super::bind_pr_review(&ui, list);
        ui.set_current_route(crate::Route::PrReview);
        let retained = ModelRc::new(VecModel::from(vec![crate::WorkflowPullRequestItem {
            number: "815".into(),
            title: "Worker-published approval".into(),
            state: "open".into(),
            review_status: "".into(),
            ..Default::default()
        }]));
        ui.set_workflow_pr_list_items(retained.clone());
        let metadata = ProjectRepositoryMetadata {
            repo_project_id: "hidden-list-worker-publish".into(),
            repo_type: "github".into(),
            remote_repo: "https://example.invalid/hidden-list-worker-publish".into(),
        };
        let key = PrKey::new(&metadata, "815");
        let published = std::thread::spawn(move || {
            let data = shared_pr_data();
            assert!(data.publish_review(data.ticket(), &key, PullRequestReviewAction::Approve));
            data.get(&key).unwrap()
        })
        .join()
        .unwrap();

        assert_eq!(
            published.review_status,
            Some(crate::git::PullRequestReviewStatus::Approved)
        );
        assert_eq!(ui.get_current_route(), crate::Route::PrReview);
        assert!(ui.get_workflow_pr_list_items() == retained);
        assert_eq!(retained.row_data(0).unwrap().review_status, "");
        assert_eq!(retained.row_data(0).unwrap().state, "open");
    }

    #[test]
    fn locale_refresh_cannot_restore_the_snapshot_before_approval_or_merge() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let list = crate::workflow::pr_list::bind_pr_list(&ui);
        let controller = super::super::bind_pr_review(&ui, list);
        let state = PullRequestActionState {
            can_comment: true,
            can_approve: true,
            can_request_changes: true,
            can_merge: true,
            can_revert: false,
            review_state: String::new(),
            readiness: crate::git::PullRequestReadiness::new(
                PullRequestReadinessKind::Ready,
                "Ready",
            ),
            merge_block_kind: PullRequestMergeBlockKind::None,
            revert_strategy: PullRequestRevertStrategy::PullRequest,
            allowed_merge_methods: vec![PullRequestMergeMethod::Merge],
        };
        apply_action_state(&ui, &state);
        *controller.action_state.lock().unwrap() = Some(state);
        apply_submitted_review(&ui, PullRequestReviewAction::Approve);

        for locale in ["en", "zh-CN"] {
            crate::i18n::apply_preference(locale).unwrap();
            controller.sync_language(&ui);
            assert_eq!(ui.get_workflow_pr_remote_review_state(), "approved");
            assert_eq!(ui.get_workflow_pr_merge_readiness_kind(), "ready");
            assert!(ui.get_workflow_pr_can_merge());
            assert_eq!(
                ui.get_workflow_pr_merge_readiness_label(),
                usher_i18n::translate_for(locale, "pr_review.readiness.ready")
            );
        }

        // A successful merge changes current semantic state before the next
        // provider snapshot arrives. A language switch must retain that state.
        ui.set_workflow_pr_merge_readiness_kind("merged".into());
        ui.set_workflow_pr_can_merge(false);
        ui.set_workflow_pr_can_approve(false);
        ui.set_workflow_pr_can_revert(true);
        for locale in ["en", "zh-CN"] {
            crate::i18n::apply_preference(locale).unwrap();
            controller.sync_language(&ui);
            assert_eq!(ui.get_workflow_pr_remote_review_state(), "approved");
            assert_eq!(ui.get_workflow_pr_merge_readiness_kind(), "merged");
            assert!(!ui.get_workflow_pr_can_merge());
            assert!(!ui.get_workflow_pr_can_approve());
            assert!(ui.get_workflow_pr_can_revert());
            assert_eq!(
                ui.get_workflow_pr_merge_readiness_label(),
                usher_i18n::translate_for(locale, "pr_review.readiness.merged")
            );
            assert_eq!(
                ui.get_workflow_pr_merge_block_reason(),
                usher_i18n::translate_for(locale, "pr_review.readiness_detail.merged")
            );
            assert_eq!(ui.get_workflow_pr_revert_block_reason(), "");
        }
        crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
    }

    fn context() -> PrHookContext {
        PrHookContext {
            metadata: ProjectRepositoryMetadata {
                repo_project_id: "repo-1".into(),
                repo_type: "github".into(),
                remote_repo: "https://github.com/example/repo".into(),
            },
            request: super::super::ReviewRequest {
                number: "42".into(),
                title: "Separate approval and merge".into(),
                author: "author".into(),
                source_branch: "feature/hooks".into(),
                target_branch: "main".into(),
                updated_at: String::new(),
                ignore_whitespace: false,
            },
            author_identity: crate::git::PullRequestAuthorIdentity {
                name: Some("Example Author".into()),
                email: Some("author@example.com".into()),
            },
            project: "project-1".into(),
            cwd: "/tmp/project-1".into(),
        }
    }

    #[test]
    fn only_approve_builds_a_review_hook_event() {
        let context = context();
        assert!(review_hook_event(&context, PullRequestReviewAction::Comment).is_none());
        assert!(review_hook_event(&context, PullRequestReviewAction::RequestChanges).is_none());
        let event = review_hook_event(&context, PullRequestReviewAction::Approve).unwrap();
        assert_eq!(event.name, crate::hooks::PR_REVIEW_APPROVED);
        assert_eq!(event.source_module, "pr.review");
        assert_eq!(event.outcome, "approved");
        assert_eq!(event.payload["action"], "approved");
    }

    #[test]
    fn every_successful_merge_method_uses_the_merged_hook_event() {
        let context = context();
        for method in [
            PullRequestMergeMethod::Merge,
            PullRequestMergeMethod::Squash,
            PullRequestMergeMethod::Rebase,
            PullRequestMergeMethod::FastForward,
        ] {
            let event = merged_hook_event(&context, method, true);
            assert_eq!(event.name, crate::hooks::PR_MERGED);
            assert_eq!(event.source_module, "pr.merge");
            assert_eq!(event.outcome, "success");
            assert_eq!(event.payload["action"], "merged");
            assert_eq!(event.payload["merge_method"], method.as_str());
            assert_eq!(event.payload["remove_source_branch"], true);
        }
    }
}
