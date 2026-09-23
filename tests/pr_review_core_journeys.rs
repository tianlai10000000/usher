use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalSize, ModelRc, VecModel};
use usher_lib::{
    MainWindow, PrDiffBridge, Route, WorkflowPrDiffLine, WorkflowPrDiffRowMetric,
    WorkflowPrDiffSelectionHit, WorkflowPrDiffToken, WorkflowPrDiffVirtualRow,
    WorkflowPrDiffVirtualWindow,
};

fn window() -> MainWindow {
    i_slint_backend_testing::init_no_event_loop();
    MainWindow::new().expect("main window")
}

fn press_control_shortcut(window: &slint::Window, key: &str) {
    window.dispatch_event(WindowEvent::KeyPressed {
        text: Key::Control.into(),
    });
    window.dispatch_event(WindowEvent::KeyPressed { text: key.into() });
    window.dispatch_event(WindowEvent::KeyReleased { text: key.into() });
    window.dispatch_event(WindowEvent::KeyReleased {
        text: Key::Control.into(),
    });
}

#[test]
fn opening_a_pull_request_preserves_context_and_initial_tab() {
    let window = window();
    let opened = Rc::new(RefCell::new(None));
    let observed = opened.clone();
    window.on_workflow_pr_review_open(
        move |number, title, author, source, target, updated_at, initial_tab| {
            *observed.borrow_mut() = Some((
                number.to_string(),
                title.to_string(),
                author.to_string(),
                source.to_string(),
                target.to_string(),
                updated_at.to_string(),
                initial_tab,
            ));
        },
    );

    window.invoke_workflow_pr_review_open(
        "42".into(),
        "Keep the review journey stable".into(),
        "octocat".into(),
        "feature/journey".into(),
        "main".into(),
        "2 hours ago".into(),
        2,
    );

    assert_eq!(
        opened.borrow().as_ref(),
        Some(&(
            "42".to_string(),
            "Keep the review journey stable".to_string(),
            "octocat".to_string(),
            "feature/journey".to_string(),
            "main".to_string(),
            "2 hours ago".to_string(),
            2,
        ))
    );
}

#[test]
fn files_changed_commands_keep_their_order_and_payloads() {
    let window = window();
    let commands = Rc::new(RefCell::new(Vec::new()));

    let observed = commands.clone();
    window.on_workflow_pr_review_select_file(move |index| {
        observed.borrow_mut().push(format!("file:{index}"));
    });
    let observed = commands.clone();
    window.on_workflow_pr_review_apply_diff_settings(move |view, ignore_whitespace| {
        observed
            .borrow_mut()
            .push(format!("settings:{view}:{ignore_whitespace}"));
    });
    let observed = commands.clone();
    window.on_workflow_pr_review_expand_hunk(move |line, direction| {
        observed
            .borrow_mut()
            .push(format!("hunk:{line}:{direction}"));
    });

    window.invoke_workflow_pr_review_select_file(3);
    window.invoke_workflow_pr_review_apply_diff_settings("split".into(), true);
    window.invoke_workflow_pr_review_expand_hunk(17, "down".into());

    assert_eq!(
        commands.borrow().as_slice(),
        ["file:3", "settings:split:true", "hunk:17:down"]
    );
}

#[test]
fn diff_find_starts_its_debounce_for_the_first_query() {
    let window = window();
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(1200.0, 800.0),
    });
    window.set_current_route(usher_lib::Route::PrReview);
    window.set_workflow_pr_review_tab(2);
    window.set_workflow_pr_review_selected_file_index(0);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let queries = Rc::new(RefCell::new(Vec::new()));
    let observed = queries.clone();
    window
        .global::<usher_lib::PrDiffBridge>()
        .on_workflow_pr_review_find_diff_matches(move |query, _| {
            observed.borrow_mut().push(query.to_string());
            ModelRc::new(VecModel::from(Vec::new()))
        });

    ElementHandle::find_by_element_id(&window, "GitHubFileFilterField::filter-input")
        .next()
        .expect("file filter input")
        .mock_single_click(PointerEventButton::Left);
    press_control_shortcut(window.window(), "f");
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(20));
    let input = ElementHandle::find_by_element_id(&window, "GitHubDiffFindBar::find-input")
        .next()
        .expect("open diff find input");
    input.set_accessible_value("the");
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    // Opening initializes Find; typing immediately cancels the old generation
    // with an empty query before the debounced replacement is submitted.
    assert_eq!(queries.borrow().as_slice(), ["", ""]);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(180));
    assert_eq!(queries.borrow().as_slice(), ["", "", "the"]);
}

#[test]
fn diff_code_click_dismisses_the_open_diff_settings_popover() {
    let window = window();
    slint::select_bundled_translation("en").expect("English test translations");
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(1200.0, 860.0),
    });
    window.set_show_navigation_drawer(false);
    window.set_show_breadcrumb(false);
    window.set_current_route(Route::PrReview);
    window.set_workflow_pr_review_visible(true);
    window.set_workflow_pr_review_tab(2);
    window.set_workflow_pr_review_selected_file_index(0);
    window.set_workflow_pr_review_selected_file_path("src/lib.rs".into());
    window.set_workflow_pr_review_loading(false);
    window.set_workflow_pr_review_code_loading(false);

    let empty_tokens = || ModelRc::new(VecModel::from(Vec::<WorkflowPrDiffToken>::new()));
    let line = WorkflowPrDiffLine {
        old_line: 1,
        new_line: 1,
        kind: "context".into(),
        content: "let value = 1;".into(),
        tokens: empty_tokens(),
        expandable: false,
        expand_all: false,
        expand_up: false,
        expand_down: false,
        source_index: 0,
        left_line: 1,
        left_columns: 14,
        left_kind: "context".into(),
        left_content: "let value = 1;".into(),
        left_tokens: empty_tokens(),
        right_line: 1,
        right_columns: 14,
        right_kind: "context".into(),
        right_content: "let value = 1;".into(),
        right_tokens: empty_tokens(),
    };
    window.set_workflow_pr_review_lines(ModelRc::new(VecModel::from(vec![line.clone()])));
    window.set_workflow_pr_review_line_metrics(ModelRc::new(VecModel::from(vec![
        WorkflowPrDiffRowMetric {
            kind: "context".into(),
            columns: 14,
            double_hunk: false,
        },
    ])));
    let visible_line = line.clone();
    window
        .global::<PrDiffBridge>()
        .on_workflow_pr_review_layout_diff_virtual_window(move |_, _, _, _, _, _, _, _| {
            WorkflowPrDiffVirtualWindow {
                rows: ModelRc::new(VecModel::from(vec![WorkflowPrDiffVirtualRow {
                    index: 0,
                    start: 0.0,
                    size: 22.0,
                    line: visible_line.clone(),
                }])),
                scroll_offset: 0.0,
                content_height: 22.0,
            }
        });
    window
        .global::<PrDiffBridge>()
        .on_workflow_pr_review_resolve_diff_selection(|_, _, _, _, _, _, _, _, _, _| {
            WorkflowPrDiffSelectionHit { line: 0, column: 0 }
        });
    window.set_workflow_pr_review_line_content_revision(1);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    ElementHandle::find_by_accessible_label(&window, "Open Diff view settings")
        .next()
        .expect("Files changed diff settings button")
        .mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert!(
        ElementHandle::find_by_element_id(
            &window,
            "GitHubDiffSettingsPopover::popover-pointer-shield",
        )
        .next()
        .is_some(),
        "diff settings popover should be visible before the code click"
    );

    ElementHandle::find_by_element_id(&window, "GitHubPullRequestReviewPage::unified-code")
        .next()
        .expect("visible unified diff code")
        .mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    assert!(
        ElementHandle::find_by_element_id(
            &window,
            "GitHubDiffSettingsPopover::popover-pointer-shield",
        )
        .next()
        .is_none(),
        "clicking selectable diff code must dismiss the open popover"
    );
}

#[test]
fn review_submission_never_implicitly_merges() {
    let window = window();
    let review = Rc::new(RefCell::new(None));
    let merge = Rc::new(RefCell::new(None));
    let merge_count = Rc::new(Cell::new(0));

    let observed = review.clone();
    window.on_workflow_pr_submit_review(move |action, body| {
        *observed.borrow_mut() = Some((action.to_string(), body.to_string()));
    });
    let observed = merge.clone();
    let observed_count = merge_count.clone();
    window.on_workflow_pr_merge(move |method, remove_source_branch| {
        observed_count.set(observed_count.get() + 1);
        *observed.borrow_mut() = Some((method.to_string(), remove_source_branch));
    });

    window.invoke_workflow_pr_submit_review("approve".into(), "Looks good.".into());
    assert_eq!(
        review.borrow().as_ref(),
        Some(&("approve".to_string(), "Looks good.".to_string()))
    );
    assert_eq!(merge_count.get(), 0);

    window.invoke_workflow_pr_merge("squash".into(), true);
    assert_eq!(merge_count.get(), 1);
    assert_eq!(merge.borrow().as_ref(), Some(&("squash".to_string(), true)));
}
