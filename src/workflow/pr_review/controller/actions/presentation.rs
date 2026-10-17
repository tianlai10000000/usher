//! Event-loop-only projection of provider capabilities and review command outcomes.
//!
//! This leaf receives normalized domain values; it never reads controller state,
//! starts remote work, or changes review request generations.
use crate::MainWindow;
use crate::git::{
    PullRequestActionState, PullRequestMergeBlockKind, PullRequestMergeMethod,
    PullRequestReadinessKind, PullRequestRevertStrategy, PullRequestReviewAction,
};
use crate::pr_review_ui::PrReviewUiExt;
use slint::ComponentHandle;

pub(in crate::workflow::pr_review::controller) fn apply_submitted_review(
    ui: &MainWindow,
    action: PullRequestReviewAction,
) {
    let page = ui.pr_review();
    let pr_state = page.global::<crate::PrReviewBridge>();
    pr_state.set_workflow_pr_review_dialog_open(false);
    match action {
        PullRequestReviewAction::Comment => {
            let count = pr_state.get_workflow_pr_review_conversation_count();
            pr_state.set_workflow_pr_review_conversation_count(count.saturating_add(1));
            // Comment completion belongs to the composer, not the aggregate
            // review decision. Repeated comments must each clear the composer.
            let revision = pr_state.get_workflow_pr_comment_submission_revision();
            pr_state.set_workflow_pr_comment_submission_revision(revision.wrapping_add(1));
        }
        PullRequestReviewAction::Approve => {
            pr_state.set_workflow_pr_remote_review_state("approved".into());
        }
        PullRequestReviewAction::RequestChanges => {
            pr_state.set_workflow_pr_remote_review_state("rejected".into());
        }
    }
}

fn select_merge_method(
    allowed: &[PullRequestMergeMethod],
    preferred: PullRequestMergeMethod,
    current: Option<PullRequestMergeMethod>,
) -> Option<PullRequestMergeMethod> {
    current
        .filter(|method| allowed.contains(method))
        .or_else(|| allowed.contains(&preferred).then_some(preferred))
        .or_else(|| {
            [
                PullRequestMergeMethod::Merge,
                PullRequestMergeMethod::Squash,
                PullRequestMergeMethod::Rebase,
                PullRequestMergeMethod::FastForward,
            ]
            .into_iter()
            .find(|method| allowed.contains(method))
        })
}

pub(in crate::workflow::pr_review::controller) fn default_merge_method(
    ui: &MainWindow,
) -> slint::SharedString {
    let page = ui.pr_review();
    let pr_state = page.global::<crate::PrReviewBridge>();
    let allowed = [
        (
            PullRequestMergeMethod::Merge,
            pr_state.get_workflow_pr_merge_method_merge(),
        ),
        (
            PullRequestMergeMethod::Squash,
            pr_state.get_workflow_pr_merge_method_squash(),
        ),
        (
            PullRequestMergeMethod::Rebase,
            pr_state.get_workflow_pr_merge_method_rebase(),
        ),
        (
            PullRequestMergeMethod::FastForward,
            pr_state.get_workflow_pr_merge_method_fast_forward(),
        ),
    ]
    .into_iter()
    .filter_map(|(method, allowed)| allowed.then_some(method))
    .collect::<Vec<_>>();
    select_merge_method(
        &allowed,
        crate::settings::load_default_pr_merge_method(),
        None,
    )
    .map_or("", PullRequestMergeMethod::as_str)
    .into()
}

pub(in crate::workflow::pr_review::controller) fn apply_action_state(
    ui: &MainWindow,
    state: &PullRequestActionState,
) {
    let page = ui.pr_review();
    let pr_state = page.global::<crate::PrReviewBridge>();
    pr_state.set_workflow_pr_can_comment(state.can_comment);
    pr_state.set_workflow_pr_can_approve(state.can_approve);
    pr_state.set_workflow_pr_can_request_changes(state.can_request_changes);
    pr_state.set_workflow_pr_can_merge(state.can_merge);
    pr_state.set_workflow_pr_can_revert(state.can_revert);
    // An empty authoritative decision clears a withdrawn or dismissed review.
    pr_state.set_workflow_pr_remote_review_state(state.review_state.clone().into());
    pr_state.set_workflow_pr_merge_readiness_kind(state.readiness.kind.as_str().into());
    pr_state.set_workflow_pr_merge_method_merge(
        state
            .allowed_merge_methods
            .contains(&PullRequestMergeMethod::Merge),
    );
    pr_state.set_workflow_pr_merge_method_squash(
        state
            .allowed_merge_methods
            .contains(&PullRequestMergeMethod::Squash),
    );
    pr_state.set_workflow_pr_merge_method_rebase(
        state
            .allowed_merge_methods
            .contains(&PullRequestMergeMethod::Rebase),
    );
    pr_state.set_workflow_pr_merge_method_fast_forward(
        state
            .allowed_merge_methods
            .contains(&PullRequestMergeMethod::FastForward),
    );
    // Capability refresh must not overwrite a supported explicit dialog choice.
    // A new dialog reads the current Settings preference through its callback.
    let current = pr_state
        .get_workflow_pr_merge_dialog_open()
        .then(|| {
            PullRequestMergeMethod::parse(pr_state.get_workflow_pr_selected_merge_method().as_str())
        })
        .flatten();
    let selected = select_merge_method(
        &state.allowed_merge_methods,
        crate::settings::load_default_pr_merge_method(),
        current,
    );
    pr_state.set_workflow_pr_selected_merge_method(
        selected.map_or("", PullRequestMergeMethod::as_str).into(),
    );
    retranslate_action_state(ui, state);
}

pub(in crate::workflow::pr_review::controller) fn retranslate_action_state(
    ui: &MainWindow,
    state: &PullRequestActionState,
) {
    let page = ui.pr_review();
    let pr_state = page.global::<crate::PrReviewBridge>();
    // The cached provider snapshot can predate a successful local mutation.
    // Language refresh changes copy without restoring stale decisions/capabilities.
    let readiness =
        projected_readiness_kind(pr_state.get_workflow_pr_merge_readiness_kind().as_str());
    pr_state.set_workflow_pr_merge_readiness_label(
        crate::i18n::tr(readiness.label_key()).into_owned().into(),
    );
    let readiness_detail = crate::i18n::tr(readiness.detail_key()).into_owned();
    pr_state.set_workflow_pr_merge_readiness_detail(readiness_detail.clone().into());
    let merge_block_kind = if pr_state.get_workflow_pr_can_merge() {
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
    pr_state.set_workflow_pr_merge_block_reason(merge_block_reason.into());
    pr_state.set_workflow_pr_revert_block_reason(
        if pr_state.get_workflow_pr_can_revert() {
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
    pr_state.set_workflow_pr_revert_action_label(
        crate::i18n::tr(revert_action_key).into_owned().into(),
    );
    pr_state.set_workflow_pr_revert_confirmation(
        crate::i18n::tr(revert_confirmation_key).into_owned().into(),
    );
}

pub(in crate::workflow::pr_review::controller) fn readiness_label_key(kind: &str) -> &'static str {
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

pub(in crate::workflow::pr_review::controller) fn reset_action_capabilities(ui: &MainWindow) {
    let page = ui.pr_review();
    let pr_state = page.global::<crate::PrReviewBridge>();
    pr_state.set_workflow_pr_can_comment(false);
    pr_state.set_workflow_pr_can_approve(false);
    pr_state.set_workflow_pr_can_request_changes(false);
    pr_state.set_workflow_pr_can_merge(false);
    pr_state.set_workflow_pr_can_revert(false);
    pr_state.set_workflow_pr_merge_method_merge(false);
    pr_state.set_workflow_pr_merge_method_squash(false);
    pr_state.set_workflow_pr_merge_method_rebase(false);
    pr_state.set_workflow_pr_merge_method_fast_forward(false);
    pr_state.set_workflow_pr_selected_merge_method("".into());
}

pub(in crate::workflow::pr_review::controller) fn reset_action_ui(ui: &MainWindow) {
    let page = ui.pr_review();
    let pr_state = page.global::<crate::PrReviewBridge>();
    pr_state.set_workflow_pr_review_dialog_open(false);
    pr_state.set_workflow_pr_dismiss_review_dialog_open(false);
    pr_state.set_workflow_pr_merge_dialog_open(false);
    pr_state.set_workflow_pr_revert_dialog_open(false);
    pr_state.set_workflow_pr_action_loading(false);
    pr_state.set_workflow_pr_remote_review_state("".into());
    pr_state.set_workflow_pr_merge_readiness_kind("".into());
    pr_state.set_workflow_pr_merge_readiness_label("".into());
    pr_state.set_workflow_pr_merge_readiness_detail("".into());
    pr_state.set_workflow_pr_merge_block_reason("".into());
    pr_state.set_workflow_pr_revert_block_reason("".into());
    pr_state.set_workflow_pr_revert_action_label(
        crate::i18n::tr("pr_review.revert_pull_request")
            .into_owned()
            .into(),
    );
    pr_state.set_workflow_pr_revert_confirmation("".into());
    reset_action_capabilities(ui);
}

#[cfg(test)]
mod tests {
    use super::{ComponentHandle, apply_action_state, apply_submitted_review, select_merge_method};
    use crate::git::{
        PullRequestActionState, PullRequestMergeBlockKind, PullRequestMergeMethod,
        PullRequestReadinessKind, PullRequestRevertStrategy, PullRequestReviewAction,
    };
    use crate::{MainWindow, PrReviewUiExt};

    #[test]
    fn every_supported_preference_selects_its_method() {
        let allowed = [
            PullRequestMergeMethod::Merge,
            PullRequestMergeMethod::Squash,
            PullRequestMergeMethod::Rebase,
            PullRequestMergeMethod::FastForward,
        ];
        for preferred in allowed {
            assert_eq!(
                select_merge_method(&allowed, preferred, None),
                Some(preferred)
            );
        }
    }

    #[test]
    fn unsupported_preference_falls_back_in_dialog_order() {
        let allowed = [
            PullRequestMergeMethod::Rebase,
            PullRequestMergeMethod::Squash,
        ];
        assert_eq!(
            select_merge_method(&allowed, PullRequestMergeMethod::FastForward, None),
            Some(PullRequestMergeMethod::Squash),
        );
        assert_eq!(
            select_merge_method(&[], PullRequestMergeMethod::Merge, None),
            None,
        );
    }

    #[test]
    fn an_open_dialog_keeps_a_supported_explicit_choice() {
        let allowed = [
            PullRequestMergeMethod::Merge,
            PullRequestMergeMethod::Squash,
        ];
        assert_eq!(
            select_merge_method(
                &allowed,
                PullRequestMergeMethod::Merge,
                Some(PullRequestMergeMethod::Squash),
            ),
            Some(PullRequestMergeMethod::Squash),
        );
        assert_eq!(
            select_merge_method(
                &allowed,
                PullRequestMergeMethod::Merge,
                Some(PullRequestMergeMethod::Rebase),
            ),
            Some(PullRequestMergeMethod::Merge),
        );
    }

    #[test]
    fn action_state_refresh_preserves_choice_until_the_provider_disables_it() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let page = ui.pr_review();
        let pr_state = page.global::<crate::PrReviewBridge>();
        let mut state = PullRequestActionState {
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
            revert_strategy: PullRequestRevertStrategy::Unsupported,
            allowed_merge_methods: vec![
                PullRequestMergeMethod::Merge,
                PullRequestMergeMethod::Squash,
            ],
        };
        apply_action_state(&ui, &state);
        pr_state.set_workflow_pr_merge_dialog_open(true);
        pr_state.set_workflow_pr_selected_merge_method("squash".into());

        for locale in ["en", "zh-CN"] {
            crate::i18n::apply_preference(locale).unwrap();
            apply_action_state(&ui, &state);
            assert_eq!(pr_state.get_workflow_pr_selected_merge_method(), "squash");
        }

        state.allowed_merge_methods = vec![PullRequestMergeMethod::Rebase];
        apply_action_state(&ui, &state);
        assert_eq!(pr_state.get_workflow_pr_selected_merge_method(), "rebase");
        state.allowed_merge_methods.clear();
        apply_action_state(&ui, &state);
        assert_eq!(pr_state.get_workflow_pr_selected_merge_method(), "");
        crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
    }

    #[test]
    fn successful_approval_does_not_merge_or_change_merge_readiness() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let page = ui.pr_review();
        let pr_state = page.global::<crate::PrReviewBridge>();
        pr_state.set_workflow_pr_review_dialog_open(true);
        pr_state.set_workflow_pr_merge_readiness_kind("conflicts".into());
        pr_state.set_workflow_pr_can_merge(false);

        apply_submitted_review(&ui, PullRequestReviewAction::Approve);

        assert_eq!(pr_state.get_workflow_pr_remote_review_state(), "approved");
        assert_eq!(pr_state.get_workflow_pr_merge_readiness_kind(), "conflicts");
        assert!(!pr_state.get_workflow_pr_can_merge());
        assert!(!pr_state.get_workflow_pr_review_dialog_open());
        assert_eq!(pr_state.get_workflow_pr_comment_submission_revision(), 0);
    }

    #[test]
    fn consecutive_comments_preserve_review_decision_and_complete_each_composer_submit() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let page = ui.pr_review();
        let pr_state = page.global::<crate::PrReviewBridge>();
        for decision in ["approved", "rejected", ""] {
            pr_state.set_workflow_pr_remote_review_state(decision.into());
            let initial_revision = pr_state.get_workflow_pr_comment_submission_revision();
            let initial_count = pr_state.get_workflow_pr_review_conversation_count();
            for submission in 1..=2 {
                apply_submitted_review(&ui, PullRequestReviewAction::Comment);
                assert_eq!(pr_state.get_workflow_pr_remote_review_state(), decision);
                assert_eq!(
                    pr_state.get_workflow_pr_comment_submission_revision(),
                    initial_revision + submission
                );
                assert_eq!(
                    pr_state.get_workflow_pr_review_conversation_count(),
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
        let page = ui.pr_review();
        let pr_state = page.global::<crate::PrReviewBridge>();
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
        assert_eq!(pr_state.get_workflow_pr_remote_review_state(), "approved");

        state.review_state.clear();
        apply_action_state(&ui, &state);

        assert_eq!(pr_state.get_workflow_pr_remote_review_state(), "");
        assert_eq!(
            pr_state.get_workflow_pr_merge_readiness_kind(),
            "review_required"
        );
        assert!(!pr_state.get_workflow_pr_can_merge());
    }
}
