//! Event-loop command validation and guarded remote-review orchestration.
//!
//! Provider work and capability projection are separate leaves. Keep request
//! generations, completion delivery and cross-controller refresh decisions here.
use crate::pr_review_ui::PrReviewUiExt;
mod presentation;
mod remote;
mod reviews;

use super::super::state::ReviewRequestKind;
use super::WorkflowPrReviewController;
use crate::MainWindow;
use crate::git::{GitResult, PullRequestMergeMethod, PullRequestReviewAction};
use crate::workflow::bindings::{show_error, show_success};
use crate::workflow::pr_data::shared as shared_pr_data;
use presentation::{apply_action_state, apply_submitted_review, reset_action_capabilities};
pub(super) use presentation::{
    default_merge_method, readiness_label_key, reset_action_ui, retranslate_action_state,
};
use remote::{PrHookContext, RemotePullRequest};
use slint::ComponentHandle;
use std::future::Future;
use std::sync::atomic::Ordering;

impl WorkflowPrReviewController {
    fn action_context(&self) -> Option<RemotePullRequest> {
        self.cache.lock().ok().and_then(|guard| {
            let cache = guard.as_ref()?;
            Some(RemotePullRequest {
                metadata: cache.metadata.clone(),
                number: cache.request.number.clone(),
            })
        })
    }

    fn hook_context(&self, ui: &MainWindow) -> Option<PrHookContext> {
        self.cache.lock().ok().and_then(|guard| {
            let cache = guard.as_ref()?;
            Some(PrHookContext {
                target: RemotePullRequest {
                    metadata: cache.metadata.clone(),
                    number: cache.request.number.clone(),
                },
                title: cache.request.title.clone(),
                author: cache.request.author.clone(),
                source_branch: cache.request.source_branch.clone(),
                target_branch: cache.request.target_branch.clone(),
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
        ui.pr_review()
            .global::<crate::PrReviewBridge>()
            .set_workflow_pr_action_loading(true);
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
                ui.pr_review()
                    .global::<crate::PrReviewBridge>()
                    .set_workflow_pr_action_loading(false);
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
        if ui
            .pr_review()
            .global::<crate::PrReviewBridge>()
            .get_workflow_pr_merge_readiness_label()
            .trim()
            .is_empty()
        {
            ui.pr_review()
                .global::<crate::PrReviewBridge>()
                .set_workflow_pr_merge_readiness_kind("checking".into());
            ui.pr_review()
                .global::<crate::PrReviewBridge>()
                .set_workflow_pr_merge_readiness_label(
                    crate::i18n::tr("pr_review.checking_mergeability")
                        .into_owned()
                        .into(),
                );
            ui.pr_review()
                .global::<crate::PrReviewBridge>()
                .set_workflow_pr_merge_readiness_detail(
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
            let Some(result) = remote::load_action_state(context, alias, number, ticket).await
            else {
                return;
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
                        ui.pr_review()
                            .global::<crate::PrReviewBridge>()
                            .set_workflow_pr_merge_readiness_kind("unavailable".into());
                        ui.pr_review()
                            .global::<crate::PrReviewBridge>()
                            .set_workflow_pr_merge_readiness_label(
                                crate::i18n::tr("pr_review.merge_status_unavailable")
                                    .into_owned()
                                    .into(),
                            );
                        ui.pr_review()
                            .global::<crate::PrReviewBridge>()
                            .set_workflow_pr_merge_readiness_detail(
                                crate::i18n::tr("pr_review.mergeability_unavailable")
                                    .into_owned()
                                    .into(),
                            );
                        ui.pr_review()
                            .global::<crate::PrReviewBridge>()
                            .set_workflow_pr_merge_block_reason(
                                crate::i18n::tr(error.user_message_key())
                                    .into_owned()
                                    .into(),
                            );
                    }
                }
            });
        });
    }

    pub(super) fn merge(&self, ui: &MainWindow, method: &str, remove_source_branch: bool) {
        if !ui
            .pr_review()
            .global::<crate::PrReviewBridge>()
            .get_workflow_pr_can_merge()
        {
            let reason = ui
                .pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_merge_block_reason();
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
            PullRequestMergeMethod::Merge => ui
                .pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_merge_method_merge(),
            PullRequestMergeMethod::Squash => ui
                .pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_merge_method_squash(),
            PullRequestMergeMethod::Rebase => ui
                .pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_merge_method_rebase(),
            PullRequestMergeMethod::FastForward => ui
                .pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_merge_method_fast_forward(),
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
        self.run_action(
            ui,
            context.merge(method, remove_source_branch),
            |controller, ui, result| match result {
                Ok(()) => {
                    ui.pr_review()
                        .global::<crate::PrReviewBridge>()
                        .set_workflow_pr_merge_dialog_open(false);
                    ui.pr_review()
                        .global::<crate::PrReviewBridge>()
                        .set_workflow_pr_can_merge(false);
                    ui.pr_review()
                        .global::<crate::PrReviewBridge>()
                        .set_workflow_pr_merge_readiness_kind("merged".into());
                    ui.pr_review()
                        .global::<crate::PrReviewBridge>()
                        .set_workflow_pr_merge_readiness_label(
                            crate::i18n::tr("pr_review.merged").into_owned().into(),
                        );
                    ui.pr_review()
                        .global::<crate::PrReviewBridge>()
                        .set_workflow_pr_merge_readiness_detail(
                            crate::i18n::tr("pr_review.merged_detail")
                                .into_owned()
                                .into(),
                        );
                    ui.pr_review()
                        .global::<crate::PrReviewBridge>()
                        .set_workflow_pr_merge_block_reason(
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
        if !ui
            .pr_review()
            .global::<crate::PrReviewBridge>()
            .get_workflow_pr_can_revert()
        {
            let reason = ui
                .pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_revert_block_reason();
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
        let Some(target) = self.action_context() else {
            show_error(
                ui,
                crate::i18n::tr("pr_review.repository_context_unavailable").as_ref(),
            );
            return;
        };
        self.run_action(
            ui,
            target.revert(),
            |_controller, ui, result| match result {
                    Ok(revert) => {
                        ui.pr_review().global::<crate::PrReviewBridge>().set_workflow_pr_revert_dialog_open(false);
                        ui.pr_review().global::<crate::PrReviewBridge>().set_workflow_pr_can_revert(false);
                        ui.pr_review().global::<crate::PrReviewBridge>().set_workflow_pr_revert_block_reason(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{
        PullRequestActionState, PullRequestMergeBlockKind, PullRequestReadinessKind,
        PullRequestRevertStrategy,
    };
    use crate::project::ProjectRepositoryMetadata;
    use crate::workflow::pr_data::PrKey;

    #[test]
    fn review_mutations_invalidate_the_retained_list_and_conversation() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        ui.pr_review()
            .global::<crate::PrReviewBridge>()
            .set_workflow_pr_review_tab(2);
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
        let page = ui.pr_review();
        let pr_state = page.global::<crate::PrReviewBridge>();
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
            assert_eq!(pr_state.get_workflow_pr_remote_review_state(), "approved");
            assert_eq!(pr_state.get_workflow_pr_merge_readiness_kind(), "ready");
            assert!(pr_state.get_workflow_pr_can_merge());
            assert_eq!(
                pr_state.get_workflow_pr_merge_readiness_label(),
                usher_i18n::translate_for(locale, "pr_review.readiness.ready")
            );
        }

        // A successful merge changes current semantic state before the next
        // provider snapshot arrives. A language switch must retain that state.
        pr_state.set_workflow_pr_merge_readiness_kind("merged".into());
        pr_state.set_workflow_pr_can_merge(false);
        pr_state.set_workflow_pr_can_approve(false);
        pr_state.set_workflow_pr_can_revert(true);
        for locale in ["en", "zh-CN"] {
            crate::i18n::apply_preference(locale).unwrap();
            controller.sync_language(&ui);
            assert_eq!(pr_state.get_workflow_pr_remote_review_state(), "approved");
            assert_eq!(pr_state.get_workflow_pr_merge_readiness_kind(), "merged");
            assert!(!pr_state.get_workflow_pr_can_merge());
            assert!(!pr_state.get_workflow_pr_can_approve());
            assert!(pr_state.get_workflow_pr_can_revert());
            assert_eq!(
                pr_state.get_workflow_pr_merge_readiness_label(),
                usher_i18n::translate_for(locale, "pr_review.readiness.merged")
            );
            assert_eq!(
                pr_state.get_workflow_pr_merge_block_reason(),
                usher_i18n::translate_for(locale, "pr_review.readiness_detail.merged")
            );
            assert_eq!(pr_state.get_workflow_pr_revert_block_reason(), "");
        }
        crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
    }
}
