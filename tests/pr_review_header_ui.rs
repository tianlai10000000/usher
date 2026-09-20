use i_slint_backend_testing::ElementHandle;
use slint::platform::WindowEvent;
use slint::{ComponentHandle, LogicalSize};
use usher_lib::{MainWindow, Route};

fn find(window: &MainWindow, id: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(window, id)
        .next()
        .unwrap_or_else(|| panic!("missing UI element {id}"))
}

#[test]
fn wide_pr_summary_clears_the_header_divider() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().expect("main window");
    slint::select_bundled_translation("en").expect("English test translations");
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(2048.0, 900.0),
    });
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    window.set_show_breadcrumb(false);
    window.set_current_route(Route::PrReview);
    window.set_workflow_pr_review_title(
        "feat(bancassurance-sales-activity-management): 新增个人详情页面、网点详情页面".into(),
    );
    window.set_workflow_pr_review_number("165".into());
    window.set_workflow_pr_review_author("author".into());
    window.set_workflow_pr_review_commit_count(1);
    window.set_workflow_pr_review_target_branch(
        "feature/p11_202608060018_bankInsActivityManagement".into(),
    );
    window.set_workflow_pr_review_source_branch(
        "feature/p11_202608060018_bankInsActivityManagement_gy".into(),
    );
    window.set_workflow_pr_review_updated_at("yesterday".into());

    let summary = ElementHandle::find_by_element_type_name(&window, "GitHubPullRequestSummaryRow")
        .next()
        .expect("wide PR summary row");
    let divider = find(
        &window,
        "GitHubPullRequestReviewPage::pr-header-bottom-rule",
    );

    let gap =
        divider.absolute_position().y - (summary.absolute_position().y + summary.size().height);
    assert!(
        gap >= 6.0,
        "wide PR summary needs at least 6px before the header divider, got {gap}px"
    );
}
