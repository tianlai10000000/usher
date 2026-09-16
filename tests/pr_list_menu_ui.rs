use std::{cell::RefCell, rc::Rc, time::Duration};

use i_slint_backend_testing::ElementHandle;
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalSize, Model, ModelRc, VecModel};
use usher_lib::{AiReviewRuleChoice, MainWindow, Route};

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(250));
}

fn find(window: &MainWindow, id: &'static str) -> ElementHandle {
    ElementHandle::find_by_element_id(window, id)
        .next()
        .unwrap_or_else(|| panic!("missing UI element {id}"))
}

fn resize(window: &MainWindow, width: f32, height: f32) {
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(width, height),
    });
    settle();
}

fn assert_anchored(window: &MainWindow, trigger: &'static str, above: bool) {
    let button = find(window, trigger);
    let menu = find(window, "GitHubPullRequestMenuOverlay::filter-menu");
    let vertical_gap = if above {
        button.absolute_position().y - menu.absolute_position().y - menu.size().height
    } else {
        menu.absolute_position().y - button.absolute_position().y - button.size().height
    };
    assert!(
        (vertical_gap - 4.0).abs() < 0.5,
        "{trigger}: gap {vertical_gap}"
    );
    let right_delta = menu.absolute_position().x + menu.size().width
        - button.absolute_position().x
        - button.size().width;
    assert!(
        right_delta.abs() < 0.5,
        "{trigger}: right offset {right_delta}"
    );
    let viewport = find(window, "MainWindow::page-scroll-view");
    assert!(menu.absolute_position().y >= viewport.absolute_position().y);
    assert!(
        menu.absolute_position().y + menu.size().height
            <= viewport.absolute_position().y + viewport.size().height
    );
}

#[test]
fn pr_filter_menus_follow_triggers_and_visible_viewport() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    usher_lib::install_floating_overlay_positioning(&window);
    window.set_show_navigation_drawer(false);
    window.set_platform_is_macos(false);
    window.set_current_route(Route::Workflow);
    window.set_workflow_pr_repo_type("github".into());
    window.set_ai_review_rule_options(ModelRc::new(VecModel::from(vec![AiReviewRuleChoice {
        id: "test".into(),
        name: "Review rules".into(),
        selected: true,
        inherited: false,
    }])));
    resize(&window, 1200.0, 1000.0);

    for locale in ["en", "zh-CN"] {
        slint::select_bundled_translation(locale).unwrap();
        for query in ["is:pr is:open", "is:pr is:open review:required"] {
            window.set_workflow_pr_list_query(query.into());
            settle();
            for trigger in [
                "GitHubPullRequestFlow::rule-btn",
                "GitHubPullRequestFlow::reviews-btn",
                "GitHubPullRequestFlow::assignee-btn",
                "GitHubPullRequestFlow::sort-btn",
            ] {
                find(&window, trigger).mock_single_click(PointerEventButton::Left);
                settle();
                assert_anchored(&window, trigger, false);
            }
            // Keep the menu open while both trigger position and label width change.
            resize(&window, 1000.0, 900.0);
            slint::select_bundled_translation(if locale == "en" { "zh-CN" } else { "en" }).unwrap();
            settle();
            assert_anchored(&window, "GitHubPullRequestFlow::sort-btn", false);
            resize(&window, 1200.0, 1000.0);
            find(&window, "GitHubPullRequestFlow::sort-btn")
                .mock_single_click(PointerEventButton::Left);
            settle();
            assert!(
                ElementHandle::find_by_element_id(
                    &window,
                    "GitHubPullRequestMenuOverlay::filter-menu"
                )
                .next()
                .is_none()
            );
        }
    }

    window.set_workflow_pr_list_query("is:pr is:open".into());
    resize(&window, 1200.0, 420.0);
    find(&window, "GitHubPullRequestFlow::reviews-btn").mock_single_click(PointerEventButton::Left);
    settle();
    assert_anchored(&window, "GitHubPullRequestFlow::reviews-btn", true);
    find(&window, "MainWindow::page-scroll-view").scroll(0.0, -80.0);
    settle();
    assert_anchored(&window, "GitHubPullRequestFlow::reviews-btn", false);

    // The toolbar replaces Reviews/Sort with one overflow trigger when narrow.
    resize(&window, 640.0, 1000.0);
    settle();
    assert!(
        ElementHandle::find_by_element_id(&window, "GitHubPullRequestMenuOverlay::filter-menu")
            .next()
            .is_none()
    );
    for trigger in [
        "GitHubPullRequestFlow::compact-assignee-btn",
        "GitHubPullRequestFlow::more-btn",
    ] {
        find(&window, trigger).mock_single_click(PointerEventButton::Left);
        settle();
        assert_anchored(&window, trigger, false);
    }

    // The scroll extent is the entire model, independently of the overlay cap.
    // Previously a 320px content cap made the final rules unreachable.
    resize(&window, 1200.0, 800.0);
    window.set_ai_review_project_rule_name(
        "Accessibility and keyboard navigation across all project workflows".into(),
    );
    let weak = window.as_weak();
    window.on_workflow_pr_list_sort_filter_change(move |value| {
        weak.unwrap().set_workflow_pr_list_sort_filter(value);
    });
    for (field, expected) in [
        ("PullRequestSortOptions::sort-updated", "updated-desc"),
        ("PullRequestSortOptions::sort-oldest", "updated-asc"),
        ("PullRequestSortOptions::sort-created", "oldest"),
        ("PullRequestSortOptions::sort-newest", "newest"),
    ] {
        find(&window, "GitHubPullRequestFlow::sort-btn")
            .mock_single_click(PointerEventButton::Left);
        settle();
        let heading = find(&window, "PullRequestSortOptions::sort-heading");
        let first = find(&window, "PullRequestSortOptions::sort-created");
        assert!(
            heading.absolute_position().y + heading.size().height <= first.absolute_position().y
        );
        find(&window, field).mock_single_click(PointerEventButton::Left);
        settle();
        assert_eq!(window.get_workflow_pr_list_sort_filter().as_str(), expected);
    }
    for width in [1000.0, 640.0] {
        resize(&window, width, 800.0);
        let rule = find(&window, "GitHubPullRequestFlow::rule-btn");
        let state = find(&window, "GitHubPullRequestFlow::closed-state-btn");
        assert!(rule.absolute_position().x >= state.absolute_position().x + state.size().width);
    }
    resize(&window, 1200.0, 800.0);
    let chosen = Rc::new(RefCell::new(String::new()));
    let selected = chosen.clone();
    window.on_ai_review_set_project_rule(move |id| *selected.borrow_mut() = id.to_string());
    window.set_ai_review_rule_options(ModelRc::new(VecModel::from(
        (0..14)
            .map(|index| AiReviewRuleChoice {
                id: format!("rule-{index}").into(),
                name: format!("Review rule {index}").into(),
                inherited: index == 0,
                selected: index == 0,
            })
            .collect::<Vec<_>>(),
    )));
    for locale in ["en", "zh-CN"] {
        slint::select_bundled_translation(locale).unwrap();
        find(&window, "GitHubPullRequestFlow::rule-btn")
            .mock_single_click(PointerEventButton::Left);
        settle();
        let menu = find(&window, "GitHubPullRequestMenuOverlay::filter-menu");
        assert_eq!(menu.size().width, 320.0);
        assert!(menu.size().height <= 432.0);
        let rows = || {
            ElementHandle::find_by_element_id(&window, "GitHubPullRequestRuleMenu::rule-item")
                .collect::<Vec<_>>()
        };
        assert_eq!(window.get_ai_review_rule_options().row_count(), 14);
        assert!((rows()[0].absolute_position().x - menu.absolute_position().x - 8.0).abs() < 0.5);
        find(&window, "GitHubActionMenu::menu-scroll").scroll(0.0, -1000.0);
        settle();
        let last = rows().pop().unwrap();
        assert!(
            last.accessible_label()
                .unwrap()
                .starts_with("Review rule 13.")
        );
        assert!(last.absolute_position().y >= menu.absolute_position().y);
        assert!(
            last.absolute_position().y + last.size().height
                <= menu.absolute_position().y + menu.size().height
        );
        last.mock_single_click(PointerEventButton::Left);
        settle();
        assert_eq!(*chosen.borrow(), "rule-13");
        assert!(
            ElementHandle::find_by_element_id(&window, "GitHubPullRequestMenuOverlay::filter-menu")
                .next()
                .is_none()
        );
    }
}
