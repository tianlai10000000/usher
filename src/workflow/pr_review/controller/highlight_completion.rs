use super::{MainWindow, ReviewRequestKind, ReviewStateFacade};
use crate::pr_review_ui::PrReviewUiExt;
use slint::ComponentHandle;

pub(super) fn finish_expand(
    state: &ReviewStateFacade,
    ui: &MainWindow,
    generation: u64,
    index: usize,
) -> bool {
    if !state.is_current(ReviewRequestKind::Expand, generation)
        || ui
            .pr_review()
            .global::<crate::PrReviewBridge>()
            .get_workflow_pr_review_selected_file_index()
            != index as i32
    {
        return false;
    }
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_code_loading(false);
    true
}

pub(super) fn finish(
    state: &ReviewStateFacade,
    ui: &MainWindow,
    generation: u64,
    index: usize,
    path: &str,
    full_file: bool,
) -> bool {
    if !state.is_current(ReviewRequestKind::Highlight, generation)
        || ui
            .pr_review()
            .global::<crate::PrReviewBridge>()
            .get_workflow_pr_review_selected_file_index()
            != index as i32
        || ui
            .pr_review()
            .global::<crate::PrReviewBridge>()
            .get_workflow_pr_review_selected_file_path()
            .as_str()
            != path
        || ui
            .pr_review()
            .global::<crate::PrReviewBridge>()
            .get_workflow_pr_review_viewing_file()
            != full_file
    {
        return false;
    }
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_code_loading(false);
    if !full_file {
        ui.pr_review()
            .global::<crate::PrReviewBridge>()
            .set_workflow_pr_review_large_diff_loading(false);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suspended_or_old_source_read_cannot_clear_new_loading() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let state = ReviewStateFacade::new();
        ui.pr_review()
            .global::<crate::PrReviewBridge>()
            .set_workflow_pr_review_selected_file_index(0);
        let old = state.begin(ReviewRequestKind::Expand);
        state.set_presentation_suspended(true);
        state.set_presentation_suspended(false);
        let current = state.begin(ReviewRequestKind::Expand);
        ui.pr_review()
            .global::<crate::PrReviewBridge>()
            .set_workflow_pr_review_code_loading(true);
        assert!(!finish_expand(&state, &ui, old, 0));
        assert!(!finish_expand(&state, &ui, current, 1));
        assert!(
            ui.pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_review_code_loading()
        );
        assert!(finish_expand(&state, &ui, current, 0));
        assert!(
            !ui.pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_review_code_loading()
        );
    }

    #[test]
    fn stale_highlight_completion_preserves_current_loading() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let state = ReviewStateFacade::new();
        ui.pr_review()
            .global::<crate::PrReviewBridge>()
            .set_workflow_pr_review_selected_file_index(3);
        ui.pr_review()
            .global::<crate::PrReviewBridge>()
            .set_workflow_pr_review_selected_file_path("current.rs".into());
        for (locale, full_file) in [
            ("en", false),
            ("en", true),
            ("zh-CN", false),
            ("zh-CN", true),
        ] {
            crate::i18n::apply_preference(locale).unwrap();
            ui.pr_review()
                .global::<crate::PrReviewBridge>()
                .set_workflow_pr_review_viewing_file(full_file);
            ui.pr_review()
                .global::<crate::PrReviewBridge>()
                .set_workflow_pr_review_code_loading(true);
            ui.pr_review()
                .global::<crate::PrReviewBridge>()
                .set_workflow_pr_review_large_diff_loading(true);
            let old = state.begin(ReviewRequestKind::Highlight);
            let current = state.begin(ReviewRequestKind::Highlight);
            for (generation, index, path, mode) in [
                (old, 3, "current.rs", full_file),
                (current, 2, "current.rs", full_file),
                (current, 3, "other.rs", full_file),
                (current, 3, "current.rs", !full_file),
            ] {
                assert!(!finish(&state, &ui, generation, index, path, mode));
                assert!(
                    ui.pr_review()
                        .global::<crate::PrReviewBridge>()
                        .get_workflow_pr_review_code_loading()
                );
                assert!(
                    ui.pr_review()
                        .global::<crate::PrReviewBridge>()
                        .get_workflow_pr_review_large_diff_loading()
                );
            }
            assert!(finish(&state, &ui, current, 3, "current.rs", full_file));
            assert!(
                !ui.pr_review()
                    .global::<crate::PrReviewBridge>()
                    .get_workflow_pr_review_code_loading()
            );
            assert_eq!(
                ui.pr_review()
                    .global::<crate::PrReviewBridge>()
                    .get_workflow_pr_review_large_diff_loading(),
                full_file
            );
            ui.pr_review()
                .global::<crate::PrReviewBridge>()
                .set_workflow_pr_review_code_loading(true);
            state.invalidate_all();
            assert!(!finish(&state, &ui, current, 3, "current.rs", full_file));
            assert!(
                ui.pr_review()
                    .global::<crate::PrReviewBridge>()
                    .get_workflow_pr_review_code_loading()
            );
        }
        crate::i18n::apply_preference(usher_i18n::source_locale().id).unwrap();
    }
}
