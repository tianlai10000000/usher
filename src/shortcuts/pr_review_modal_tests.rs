//! The native monitor calls the shared handler before a Slint event-loop turn.
use super::*;
use crate::{PrReviewBridge, PrReviewUiExt, ProjectBridge};
use std::{cell::Cell, rc::Rc, time::Duration};

fn set_modal(state: &PrReviewBridge<'_>, kind: usize, open: bool) {
    match kind {
        0 => state.set_workflow_pr_review_dialog_open(open),
        1 => state.set_workflow_pr_dismiss_review_dialog_open(open),
        2 => state.set_workflow_pr_merge_dialog_open(open),
        3 => state.set_workflow_pr_revert_dialog_open(open),
        4 => state.set_workflow_pr_commit_range_dialog_open(open),
        _ => unreachable!(),
    }
}

#[test]
fn native_shortcut_admission_uses_current_pr_modal_without_waiting_for_changed() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    window.set_current_route(Route::PrReview);
    let page = window.pr_review();
    let state = page.global::<PrReviewBridge>();
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    window.on_navigate(move |route| {
        assert_eq!(route, Route::Settings);
        observed.set(observed.get() + 1);
    });
    let settings = KeyStroke::from_slint(",", true, false, false, false);
    for kind in 0..5 {
        set_modal(&state, kind, true);
        // Before-case control: the former admission path sees a stale false
        // summary and permits OpenSettings. There is intentionally no UI turn.
        assert!(!window.get_modal_overlay_visible());
        assert_eq!(
            application_registry().resolve(
                &settings,
                ShortcutContext {
                    route: Route::PrReview,
                    modal_overlay_visible: window.get_modal_overlay_visible(),
                },
            ),
            Some(CommandId::OpenSettings),
        );
        assert!(
            !handle_key_pressed(&window, ",", true, false, false, false),
            "modal kind {kind} must fence the actual native shortcut entry",
        );
        assert_eq!(calls.get(), kind);
        assert!(window.get_modal_overlay_visible());

        // A closed PR dialog must not leave the input guard stale in the
        // opposite direction. This remains the same synchronous event turn.
        set_modal(&state, kind, false);
        assert!(window.get_modal_overlay_visible());
        assert!(handle_key_pressed(&window, ",", true, false, false, false));
        assert!(!window.get_modal_overlay_visible());
        assert_eq!(calls.get(), kind + 1);
    }
    // The PR-only synchronization must not clear another feature's modal.
    window.global::<ProjectBridge>().set_show_add_modal(true);
    assert!(!handle_key_pressed(&window, ",", true, false, false, false));
    assert_eq!(calls.get(), 5);
}
