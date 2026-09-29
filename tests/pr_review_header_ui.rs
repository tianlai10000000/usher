use i_slint_backend_testing::ElementHandle;
use slint::platform::WindowEvent;
use slint::{ComponentHandle, LogicalSize, ModelRc, VecModel};
use usher_lib::{MainWindow, Route, WorkflowPrCommit};

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

#[test]
fn commit_identity_and_timestamp_share_one_shaping_node() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().expect("main window");
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(1200.0, 900.0),
    });
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    window.set_current_route(Route::PrReview);
    window.set_workflow_pr_review_tab(1);
    window.set_workflow_pr_review_commits_loading(false);
    let commit = |message_body: &str, message_expanded| WorkflowPrCommit {
        sha: "ba19ff1".into(),
        short_sha: "ba19ff1".into(),
        title: "Align commit timestamp".into(),
        message: "Align commit timestamp".into(),
        message_body: message_body.into(),
        message_body_height: if message_body.is_empty() { 0 } else { 18 },
        message_expanded,
        group_expand_extra: 0,
        author: "Alice".into(),
        avatar: Default::default(),
        avatar_label: "A".into(),
        authored_at: "Sep 29, 2026, 10:00 AM GMT+8".into(),
        committer: "Alice".into(),
        committer_avatar: Default::default(),
        committer_avatar_label: "A".into(),
        committed_at: "Sep 29, 2026, 10:00 AM GMT+8".into(),
        show_committer: false,
        identity_action: "Alice authored".into(),
        date_label: "Sep 29, 2026".into(),
        short_date_label: "13 minutes ago".into(),
        group_start: true,
        group_end: true,
        group_size: 1,
        verified: false,
    };

    for (row_id, row) in [
        (
            "GitHubPullRequestReviewPage::collapsed-commit-timestamp",
            commit("", false),
        ),
        (
            "GitHubPullRequestReviewPage::expanded-commit-timestamp",
            commit("Additional commit details", true),
        ),
    ] {
        window.set_workflow_pr_review_commits(ModelRc::new(VecModel::from(vec![row])));

        let timestamp = find(&window, row_id);
        let label = find(&window, "GitHubRelativeTimestamp::time-label");
        let anchor = find(&window, "GitHubRelativeTimestamp::time-anchor");
        assert_eq!(
            label.accessible_label().as_deref(),
            Some("Alice authored 13 minutes ago"),
            "identity and relative time must be painted by one text node"
        );

        let timestamp_left = timestamp.absolute_position().x;
        let timestamp_right = timestamp_left + timestamp.size().width;
        let anchor_left = anchor.absolute_position().x;
        let anchor_right = anchor_left + anchor.size().width;
        assert!(
            anchor_left > timestamp_left,
            "the timestamp-only Tooltip target must exclude the leading identity"
        );
        assert!(
            (anchor_right - timestamp_right).abs() < 0.01,
            "the timestamp-only Tooltip target must end at the combined text edge"
        );
    }

    let mut without_timestamp = commit("", false);
    without_timestamp.short_date_label = "".into();
    window.set_workflow_pr_review_commits(ModelRc::new(VecModel::from(vec![without_timestamp])));
    let label = find(&window, "GitHubRelativeTimestamp::time-label");
    let anchor = find(&window, "GitHubRelativeTimestamp::time-anchor");
    assert_eq!(label.accessible_label().as_deref(), Some("Alice authored"));
    assert_eq!(
        anchor.size().width,
        0.0,
        "a missing timestamp must not make the identity prose hoverable"
    );

    let mut wrapped = commit("", false);
    wrapped.title = "feat(bancassurance-sales-activity-management): 实现初版银保活动量管理改造功能并补充多行标题排版验证，同时覆盖较长英文分支语义与中文说明在提交列表中的连续换行效果"
        .into();
    window.set_workflow_pr_review_commits(ModelRc::new(VecModel::from(vec![wrapped])));
    let title = find(&window, "GitHubPullRequestReviewPage::commit-title-text");
    let author_row = find(
        &window,
        "GitHubPullRequestReviewPage::collapsed-commit-timestamp",
    );
    assert!(
        title.size().height > 20.0,
        "the fixture title must wrap so the line-height contract is exercised"
    );
    assert!(
        author_row.absolute_position().y >= title.absolute_position().y + title.size().height + 3.9,
        "the measured title height must keep the author row below the expanded line boxes"
    );
}
