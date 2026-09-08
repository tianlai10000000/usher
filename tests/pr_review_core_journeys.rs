use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalSize, ModelRc, VecModel};
use usher_lib::MainWindow;

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
