#![cfg(all(not(feature = "live-preview"), not(feature = "dev-shell")))]

use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use i_slint_core::window::WindowInner;
use slint::platform::WindowEvent;
use slint::{ComponentHandle, LogicalSize, ModelRc, VecModel};
use usher_lib::{
    MainWindow, PrReviewBridge, PrReviewUiRoot, Route, WorkflowPrCommit, install_pr_review_fixture,
    pr_review_fixture_view,
};

fn flush(window: &MainWindow) {
    WindowInner::from_pub(window.window()).ensure_tree_instantiated();
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(20));
}

fn find(page: &PrReviewUiRoot, id: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(page, id)
        .next()
        .unwrap_or_else(|| panic!("missing mounted PR element {id}"))
}

fn near(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.5,
        "expected {expected}, got {actual}"
    );
}

fn window() -> (MainWindow, PrReviewUiRoot) {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().expect("shell");
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(1280.0, 900.0),
    });
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    install_pr_review_fixture(&window);
    let page = pr_review_fixture_view(&window);
    let review = page.global::<PrReviewBridge>();
    review.set_workflow_pr_review_visible(true);
    review.set_workflow_pr_review_loading(false);
    review.set_workflow_pr_review_commits_loading(false);
    review.set_workflow_pr_review_tab(2);
    window.set_current_route(Route::PrReview);
    flush(&window);
    review.set_workflow_pr_commit_range_dialog_open(true);
    flush(&window);
    (window, page)
}

fn commits(page: &PrReviewUiRoot, titles: &[String]) {
    let rows = titles
        .iter()
        .enumerate()
        .map(|(index, title)| WorkflowPrCommit {
            sha: format!("{index:040x}").into(),
            short_sha: format!("{index:07x}").into(),
            title: title.as_str().into(),
            identity_action: format!("Author {index} authored").into(),
            date_label: "Oct 20, 2026".into(),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    let review = page.global::<PrReviewBridge>();
    review.set_workflow_pr_review_commit_count(rows.len() as i32);
    review.set_workflow_pr_review_commits(ModelRc::new(VecModel::from(rows)));
}

fn scroll(window: &MainWindow, page: &PrReviewUiRoot, delta: f32) {
    find(page, "GitHubPullRequestReviewPage::commit-range-scroll").scroll(0.0, delta);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(600));
    flush(window);
}

fn locale(window: &MainWindow, page: &PrReviewUiRoot, locale: &str) {
    slint::select_bundled_translation(locale).expect("packaged locale");
    i_slint_core::translations::mark_all_translations_dirty();
    flush(window);
    let expected = usher_i18n::translate_for(locale, "pr_review.select_commits");
    assert!(
        find(page, "GitHubPullRequestReviewPage::commit-dialog-frame")
            .query_descendants()
            .match_predicate(move |element| {
                element.accessible_label().as_deref() == Some(expected.as_ref())
            })
            .find_first()
            .is_some(),
        "the already-open Commit dialog must immediately use {locale}"
    );
}

#[test]
fn short_commit_range_body_uses_the_full_dialog_width_in_both_locales() {
    let (window, page) = window();
    commits(&page, &["A short Commit / 简短提交".into()]);
    flush(&window);

    for language in ["en", "zh-CN", "en"] {
        locale(&window, &page, language);
        let scroll_view = find(&page, "GitHubPullRequestReviewPage::commit-range-scroll");
        let frame = find(&page, "GitHubPullRequestReviewPage::commit-dialog-frame");
        let surface = frame
            .query_descendants()
            .match_id("GitHubDialog::dialog-surface")
            .find_first()
            .expect("the real Commit dialog surface");
        let row = find(&page, "GitHubPullRequestReviewPage::all-commits-row");
        near(
            scroll_view.absolute_position().x,
            surface.absolute_position().x,
        );
        near(scroll_view.size().width, surface.size().width);
        near(row.absolute_position().x, scroll_view.absolute_position().x);
        near(row.size().width, scroll_view.size().width);
        let top = row.absolute_position().y;
        scroll(&window, &page, -100_000.0);
        near(row.absolute_position().y, top);
        scroll(&window, &page, 100_000.0);
        near(row.absolute_position().y, top);
    }
}

#[test]
fn wrapped_commit_range_reaches_the_last_row_and_clamps_after_resize_and_shortening() {
    let (window, page) = window();
    let titles = (0..24)
        .map(|index| {
            format!(
                "Commit {index}: feat(bancassurance-sales-activity-management): 中文提交标题验证与较长英文内容共同换行，确保最后一行及其作者信息可以完整滚动到弹窗正文内，同时保持实际文本高度"
            )
        })
        .collect::<Vec<_>>();
    commits(&page, &titles);
    flush(&window);
    let title = find(&page, "GitHubPullRequestReviewPage::commit-title");
    assert!(title.size().height > 20.0, "the fixture must wrap");

    for language in ["en", "zh-CN"] {
        locale(&window, &page, language);
        for size in [(1280.0, 900.0), (800.0, 600.0)] {
            window.window().dispatch_event(WindowEvent::Resized {
                size: LogicalSize::new(size.0, size.1),
            });
            flush(&window);
            scroll(&window, &page, 100_000.0);
            let scroll_view = find(&page, "GitHubPullRequestReviewPage::commit-range-scroll");
            let row = find(&page, "GitHubPullRequestReviewPage::all-commits-row");
            near(row.absolute_position().y, scroll_view.absolute_position().y);
            scroll(&window, &page, -100_000.0);
            assert!(row.absolute_position().y < scroll_view.absolute_position().y);
            let last_author = scroll_view
                .query_descendants()
                .match_predicate(|element| {
                    element.accessible_label().as_deref()
                        == Some("Author 23 authored on Oct 20, 2026")
                })
                .find_first()
                .expect("the final Commit's metadata");
            let viewport_bottom = scroll_view.absolute_position().y + scroll_view.size().height;
            assert!(last_author.absolute_position().y >= scroll_view.absolute_position().y);
            assert!(
                last_author.absolute_position().y + last_author.size().height
                    <= viewport_bottom + 0.5,
                "the final metadata line must fit above the footer"
            );
        }
    }

    // Keep the same mounted dialog after scrolling to its end. Shrinking the
    // model and restoring the window must retire the old viewport offset.
    commits(&page, &["A short Commit again / 恢复简短提交".into()]);
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(1280.0, 900.0),
    });
    flush(&window);
    let scroll_view = find(&page, "GitHubPullRequestReviewPage::commit-range-scroll");
    let row = find(&page, "GitHubPullRequestReviewPage::all-commits-row");
    near(row.absolute_position().y, scroll_view.absolute_position().y);
    near(row.size().width, scroll_view.size().width);
    scroll(&window, &page, -100_000.0);
    near(row.absolute_position().y, scroll_view.absolute_position().y);
}
