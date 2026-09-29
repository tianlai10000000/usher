use i_slint_backend_testing::ElementHandle;
use slint::{ComponentHandle, LogicalSize, ModelRc, VecModel};
use usher_lib::{MainWindow, Route, WorkflowPullRequestItem};

fn find(ui: &MainWindow, id: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(ui, id)
        .next()
        .unwrap_or_else(|| panic!("missing {id}"))
}

fn right(element: &ElementHandle) -> f32 {
    element.absolute_position().x + element.size().width
}

fn bottom(element: &ElementHandle) -> f32 {
    element.absolute_position().y + element.size().height
}

fn assert_follows(previous: &ElementHandle, next: &ElementHandle, left: f32) {
    if (next.absolute_position().y - previous.absolute_position().y).abs() < 1.0 {
        assert!(
            (next.absolute_position().x - right(previous) - 16.0).abs() < 0.5,
            "inline segments need a compact, consistent separator gap"
        );
    } else {
        assert!(
            (next.absolute_position().x - left).abs() < 0.5,
            "wrapped segments start at the content gutter"
        );
        assert!(
            (next.absolute_position().y - bottom(previous)).abs() < 0.5,
            "wrapping must not introduce an empty metadata line"
        );
    }
}

#[test]
fn metadata_flows_compactly_without_overlapping_or_reserved_status_columns() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    ui.set_current_route(Route::Workflow);
    ui.set_workflow_selected_flow(2);
    ui.set_workflow_pr_list_loading(false);
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/pr-list-review-status.json")).unwrap();
    for provider in ["github", "gitlab", "gitee"] {
        ui.set_workflow_pr_repo_type(provider.into());
        for locale in ["en", "zh-CN", "en"] {
            slint::select_bundled_translation(locale).unwrap();
            let compact_text_offset =
                usher_i18n::resolve_preference(locale).pr_list_compact_text_offset_y;
            ui.set_workflow_pr_list_compact_text_offset_y(compact_text_offset);
            for drawer in [false, true] {
                ui.set_show_navigation_drawer(drawer);
                for width in [800.0, 1000.0, 1200.0, 1800.0] {
                    ui.window()
                        .dispatch_event(slint::platform::WindowEvent::Resized {
                            size: LogicalSize::new(width, 1200.0),
                        });
                    for index in [0, 1, 4, 7, 8, 9, 23] {
                        let item = &fixture["items"][index];
                        let text = |key| item[key].as_str().unwrap_or_default().into();
                        ui.set_workflow_pr_list_items(ModelRc::new(VecModel::from(vec![
                            WorkflowPullRequestItem {
                                number: text("number"),
                                title: text("title"),
                                state: text("state"),
                                author: text("author"),
                                source_branch: text("source_branch"),
                                target_branch: text("target_branch"),
                                review_status: text("review_status"),
                                conflict_status: text("conflict_status"),
                                updated_at: "7 hours ago".into(),
                                conversation_count: item["conversation_count"].as_i64().unwrap_or(0)
                                    as i32,
                                ..Default::default()
                            },
                        ])));
                        i_slint_backend_testing::mock_elapsed_time(
                            std::time::Duration::from_millis(250),
                        );
                        let row = find(&ui, "GitHubPullRequestFlow::pr-row");
                        let title = find(&ui, "GitHubPullRequestFlow::title-clip");
                        let author = find(&ui, "GitHubPullRequestFlow::metadata-prefix");
                        let time = find(&ui, "GitHubPullRequestFlow::metadata-time");
                        let left = author.absolute_position().x;
                        if item["conversation_count"].as_i64().unwrap_or(0) > 0 {
                            let action = find(&ui, "GitHubPullRequestFlow::conversation-action");
                            let count = find(&ui, "GitHubPullRequestFlow::conversation-count");
                            assert!(right(&title) <= action.absolute_position().x);
                            assert!(right(&count) <= right(&action));
                            if index == 1 {
                                assert!(count.size().width >= 12.0);
                            }
                            if index == 23 {
                                assert!(count.size().width >= 18.0);
                            }
                        }
                        assert!(bottom(&title) <= time.absolute_position().y);
                        assert!(right(&author) <= time.absolute_position().x);
                        assert_eq!(
                            author.absolute_position().y,
                            time.absolute_position().y + compact_text_offset as f32
                        );
                        let mut previous = time.clone();
                        if item["review_status"] != "none" {
                            let review = find(&ui, "GitHubPullRequestFlow::review-status");
                            assert_follows(&previous, &review, left);
                            assert_eq!(review.size().height, 24.0);
                            previous = review;
                        }
                        if item["state"] == "open" {
                            let conflict = find(&ui, "GitHubPullRequestFlow::conflict-badge");
                            assert_follows(&previous, &conflict, left);
                            assert_eq!(conflict.size().height, 24.0);
                            assert!(right(&conflict) <= right(&row));
                            previous = conflict;
                        }
                        assert!(bottom(&previous) + 9.5 <= bottom(&row));
                        if item["source_branch"] != "" {
                            let branch = find(&ui, "GitHubPullRequestFlow::target-branch-chip");
                            let source = find(&ui, "GitHubPullRequestFlow::source-branch-chip");
                            let branch_label =
                                find(&ui, "GitHubPullRequestFlow::target-branch-label");
                            let source_label =
                                find(&ui, "GitHubPullRequestFlow::source-branch-label");
                            assert_eq!(branch.size().height, 20.0);
                            assert_eq!(source.size().height, 20.0);
                            assert_eq!(branch.absolute_position().y, source.absolute_position().y);
                            assert_eq!(
                                branch_label.absolute_position().y,
                                branch.absolute_position().y + compact_text_offset as f32
                            );
                            assert_eq!(
                                source_label.absolute_position().y,
                                source.absolute_position().y + compact_text_offset as f32
                            );
                            let same_line = (branch.absolute_position().y
                                + branch.size().height / 2.0
                                - previous.absolute_position().y
                                - previous.size().height / 2.0)
                                .abs()
                                < 0.5;
                            if same_line {
                                assert!(
                                    (branch.absolute_position().x - right(&previous) - 16.0).abs()
                                        < 0.5
                                );
                            } else {
                                assert!((branch.absolute_position().x - left).abs() < 0.5);
                                assert!(branch.absolute_position().y >= bottom(&previous));
                            }
                            assert!(right(&source) <= right(&row));
                            assert!(bottom(&branch) + 9.5 <= bottom(&row));
                        }
                        if width == 1800.0 && index == 0 {
                            assert_eq!(
                                previous.absolute_position().y,
                                time.absolute_position().y,
                                "wide short rows must retain GitHub's two-line density"
                            );
                        }
                    }
                }
            }
        }
    }

    let opened = std::rc::Rc::new(std::cell::Cell::new(-1));
    let selected = opened.clone();
    ui.on_workflow_pr_review_open(move |_, _, _, _, _, _, tab| selected.set(tab));
    for (id, tab) in [
        ("GitHubPullRequestFlow::review-status-touch", 0),
        ("GitHubPullRequestFlow::conflict-badge", 3),
        ("GitHubPullRequestFlow::title-touch", 2),
    ] {
        let target = find(&ui, id);
        target.mock_single_click(slint::platform::PointerEventButton::Left);
        assert_eq!(opened.get(), tab);
    }
}
