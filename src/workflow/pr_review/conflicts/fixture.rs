//! Opt-in offline fixture uses the production parser, worker and UI models.
use super::*;

fn snapshot() -> ConflictSnapshot {
    let content = format!(
        "{}\n{}\n{}",
        (1..69)
            .map(|i| format!("// context line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
        "<<<<<<< feature/conflict-preview\nlet label = \"Source 中文\";\n=======\nlet label = \"Target\";\n>>>>>>> main\n\nfn render() {\n    let ready = true;\n}\n",
        "<<<<<<< feature/conflict-preview\nconst LIMIT: usize = 50;\n=======\nconst LIMIT: usize = 100;\n>>>>>>> main\n"
    );
    let blocks = conflicts::parse_blocks(&content).unwrap();
    ConflictSnapshot {
        status: conflicts::ConflictStatus::Conflicting,
        head: "a".repeat(40),
        target: "b".repeat(40),
        source_branch: "feature/conflict-preview".into(),
        target_branch: "main".into(),
        local: false,
        files: vec![
            conflicts::ConflictFile {
                path: "src/components/pull_request.rs".into(),
                target_path: "src/components/pull_request.rs".into(),
                content: Some(content.into()),
                blocks,
            },
            conflicts::ConflictFile {
                path: "assets/logo.png".into(),
                target_path: "assets/old-logo.png".into(),
                content: None,
                blocks: vec![],
            },
        ],
    }
}

fn long_path_snapshot() -> ConflictSnapshot {
    let mut data = snapshot();
    let text = data.files[0].clone();
    let renamed = data.files[1].clone();
    data.files = [
        "apps/enterprise-insurance-management/test/COVERAGE.en.md",
        "apps/enterprise-insurance-management/test/SCENARIOS.md",
        "apps/enterprise-insurance-management/src/components/review.tsx",
        "apps/enterprise-insurance-management/test/components/review.tsx",
        "bug-history.md",
        "docs/中文/冲突说明.md",
    ]
    .into_iter()
    .map(|path| conflicts::ConflictFile {
        path: path.into(),
        target_path: path.into(),
        ..text.clone()
    })
    .chain(std::iter::once(renamed))
    .collect();
    data
}

#[cfg(feature = "test-fixtures")]
fn selected_snapshot() -> ConflictSnapshot {
    if std::env::var("USHER_PR_CONFLICTS_FILES").as_deref() == Ok("long-paths") {
        long_path_snapshot()
    } else {
        snapshot()
    }
}

#[cfg(feature = "test-fixtures")]
pub(super) async fn load_snapshot(alias: &str) -> Option<ConflictSnapshot> {
    if alias != "conflicts-fixture"
        || std::env::var("USHER_PR_CONFLICTS_FIXTURE").as_deref() != Ok("1")
        || !crate::app_paths::benchmark_offline_mode()
        || std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_none()
    {
        return None;
    }
    let delay_ms = std::env::var("USHER_PR_CONFLICTS_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(300)
        .min(60_000);
    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
    let mut data = selected_snapshot();
    if let Ok(count) = std::env::var("USHER_PR_CONFLICTS_COUNT")
        && let Ok(count) = count.parse::<usize>()
    {
        let template = data
            .files
            .iter()
            .find(|file| file.content.is_none())?
            .clone();
        data.files = (0..count.min(1000))
            .map(|index| {
                let mut file = template.clone();
                file.path = format!("assets/conflict-{index}.png");
                file.target_path = file.path.clone();
                file
            })
            .collect();
    }
    Some(data)
}

#[cfg(feature = "test-fixtures")]
pub(super) fn setup(controller: &ConflictController, ui: &MainWindow) {
    if std::env::var("USHER_PR_CONFLICTS_FIXTURE").as_deref() != Ok("1")
        || !crate::app_paths::benchmark_offline_mode()
        || std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_none()
    {
        return;
    }
    let controller = controller.clone();
    let weak = ui.as_weak();
    crate::startup::run_after_first_frame(move || {
        slint::Timer::single_shot(std::time::Duration::from_secs(2), move || {
            let Some(ui) = weak.upgrade() else { return };
            if let Ok(width) = std::env::var("USHER_PR_CONFLICTS_WIDTH")
                && let Ok(width) = width.parse::<f32>()
            {
                ui.window()
                    .set_size(slint::LogicalSize::new(width.clamp(800.0, 1800.0), 900.0));
            }
            if let Ok(locale) = std::env::var("USHER_PR_CONFLICTS_LOCALE") {
                let _ = crate::i18n::apply_preference(&locale);
            }
            if std::env::var("USHER_PR_CONFLICTS_DARK").as_deref() == Ok("1") {
                ui.global::<crate::AppState>().set_effective_dark_mode(true);
            }
            if let Ok(locale) = std::env::var("USHER_PR_CONFLICTS_SWITCH_LOCALE") {
                slint::Timer::single_shot(std::time::Duration::from_secs(60), move || {
                    let _ = crate::i18n::apply_preference(&locale);
                });
            }
            if std::env::var("USHER_PR_CONFLICTS_SCENE").as_deref() == Ok("list")
                && std::env::var("USHER_PR_LIST_FIXTURE").as_deref() == Ok("review-status-v1")
            {
                ui.set_workflow_project_name("Conflicts fixture".into());
                ui.set_workflow_pr_repo_project_id("review-status-v1".into());
                ui.set_workflow_pr_repo_type("github".into());
                ui.set_workflow_selected_flow(2);
                ui.invoke_navigate(crate::Route::Workflow);
                ui.invoke_workflow_pr_list_state_change("open".into());
                return;
            }
            #[cfg(feature = "ui-components")]
            if std::env::var("USHER_PR_CONFLICTS_SCENE").as_deref() == Ok("tabs") {
                ui.invoke_navigate(crate::Route::UiComponents);
                return;
            }
            ui.set_workflow_pr_review_title("Add conflict preview to Pull Request review".into());
            ui.set_workflow_pr_review_number("42".into());
            ui.set_workflow_pr_review_author("yuankai".into());
            ui.set_workflow_pr_review_source_branch("feature/conflict-preview".into());
            ui.set_workflow_pr_review_target_branch("main".into());
            ui.set_workflow_pr_review_visible(true);
            ui.set_workflow_pr_review_loading(false);
            ui.set_workflow_pr_merge_readiness_kind("conflicts".into());
            ui.set_workflow_pr_merge_readiness_label(
                crate::i18n::tr("pr_review.readiness.conflicts")
                    .into_owned()
                    .into(),
            );
            ui.set_workflow_pr_review_commit_count(2);
            ui.set_workflow_pr_review_added_lines(32842);
            ui.set_workflow_pr_review_removed_lines(2277);
            ui.invoke_navigate(crate::Route::PrReview);
            if std::env::var("USHER_PR_CONFLICTS_SCENE").as_deref() == Ok("review") {
                // The isolated fixture has no provider-backed active PR request.
                let open_controller = controller.clone();
                let open_window = ui.as_weak();
                ui.global::<PrConflictsBridge>().on_open(move || {
                    if let Some(ui) = open_window.upgrade() {
                        open_controller.open(
                            &ui,
                            "conflicts-fixture".into(),
                            "42".into(),
                            "feature/conflict-preview".into(),
                            "main".into(),
                        );
                    }
                });
                controller.prefetch(
                    &ui,
                    "conflicts-fixture".into(),
                    "42".into(),
                    "feature/conflict-preview".into(),
                    "main".into(),
                );
                return;
            }
            ui.global::<PrConflictsBridge>().set_active(true);
            let generation = controller.generation.fetch_add(1, Ordering::SeqCst) + 1;
            let snapshot = Arc::new(selected_snapshot());
            let state = controller.state.clone();
            crate::app_runtime::shared_runtime().spawn_blocking(move || {
                state.dispatch(Action::Reset(generation));
                state.dispatch(Action::Loaded(generation, Ok(snapshot.clone())));
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        controller.present(&ui, &snapshot);
                    }
                });
            });
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gutter_includes_opening_lane_and_all_markers() {
        i_slint_backend_testing::init_no_event_loop();
        let snapshot = snapshot();
        let blocks = snapshot.files[0].blocks.clone();
        let rows = projection::prepare(snapshot.files[0].clone()).into_model();
        for block in blocks {
            for index in block.start..=block.end {
                assert!(rows.row_data(index).unwrap().conflict);
            }
            assert!(rows.row_data(block.start).unwrap().start);
            assert!(rows.row_data(block.separator).unwrap().separator);
            assert!(rows.row_data(block.end).unwrap().end);
            assert!(!rows.row_data(block.start - 1).unwrap().conflict);
        }
    }
    #[test]
    fn new_conflict_copy_is_available_in_both_locales() {
        let _guard = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let _ui = MainWindow::new().unwrap();
        for (locale, expected) in [("en", "Conflicts"), ("zh-CN", "冲突")] {
            crate::i18n::apply_preference(locale).unwrap();
            assert_eq!(crate::i18n::tr("conflicts.title"), expected);
            assert!(!crate::i18n::tr("conflicts.stale").is_empty());
        }
        crate::i18n::apply_preference("en").unwrap();
    }

    #[test]
    fn copy_completion_rejects_old_sessions_and_file_selections() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let controller = ConflictController::default();
        let b = ui.global::<PrConflictsBridge>();
        b.set_active(true);
        b.set_copy_busy(true);
        controller.generation.store(2, Ordering::SeqCst);
        controller.selection.store(3, Ordering::SeqCst);
        for (generation, selection) in [(1, 3), (2, 2)] {
            controller.finish_copy(&ui, generation, selection, true);
            assert_eq!(b.get_copy_completed(), 0);
            assert!(b.get_copy_busy());
        }
        controller.finish_copy(&ui, 2, 3, false);
        assert_eq!(b.get_copy_completed(), 0);
        assert!(!b.get_copy_busy());
        controller.finish_copy(&ui, 2, 3, true);
        assert_eq!(b.get_copy_completed(), 1);
        b.set_active(false);
        controller.finish_copy(&ui, 2, 3, true);
        assert_eq!(b.get_copy_completed(), 1);
    }

    #[test]
    fn conflict_entry_stays_in_the_tab_row_at_all_supported_widths() {
        use i_slint_backend_testing::ElementHandle;
        let _guard = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        ui.set_current_route(crate::Route::PrReview);
        ui.set_show_navigation_drawer(false);
        ui.set_workflow_pr_review_loading(false);
        ui.set_workflow_pr_merge_readiness_kind("conflicts".into());
        ui.set_workflow_pr_review_added_lines(123456789);
        ui.set_workflow_pr_review_removed_lines(987654321);
        ui.global::<PrConflictsBridge>().set_file_count(17);
        for locale in ["en", "zh-CN"] {
            crate::i18n::apply_preference(locale).unwrap();
            for (width, drawer) in [
                (800.0, false),
                (1200.0, false),
                (1800.0, false),
                (800.0, true),
            ] {
                ui.set_show_navigation_drawer(drawer);
                ui.window()
                    .dispatch_event(slint::platform::WindowEvent::Resized {
                        size: slint::LogicalSize::new(width, 900.0),
                    });
                let nav = ElementHandle::find_by_element_id(
                    &ui,
                    "GitHubPullRequestReviewPage::review-nav",
                )
                .next()
                .expect("review navigation");
                let viewport =
                    ElementHandle::find_by_element_id(&ui, "GitHubUnderlineNav::viewport")
                        .next()
                        .expect("navigation viewport");
                assert_eq!(nav.size().height, 48.0);
                let stats = ElementHandle::find_by_element_id(
                    &ui,
                    "GitHubPullRequestReviewPage::aggregate-diffstat",
                )
                .next()
                .expect("PR totals remain visible at every supported width");
                assert!(
                    nav.absolute_position().x + nav.size().width + 8.0
                        <= stats.absolute_position().x + 0.5
                );
                assert!(
                    (stats.absolute_position().x + stats.size().width - (width - 16.0)).abs() < 0.5
                );
                assert_eq!(nav.absolute_position().y, stats.absolute_position().y);
                assert!(viewport.absolute_position().x >= nav.absolute_position().x + 16.0);
                assert!(viewport.absolute_position().x + viewport.size().width <= width - 16.0);
            }
        }
        crate::i18n::apply_preference("en").unwrap();
    }

    #[test]
    fn conflict_file_rows_keep_names_paths_and_status_in_separate_columns() {
        use i_slint_backend_testing::ElementHandle;
        use slint::platform::{Key, PointerEventButton, WindowEvent};
        let _guard = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        ui.set_current_route(crate::Route::PrReview);
        ui.set_show_navigation_drawer(false);
        ui.set_workflow_pr_review_loading(false);
        let bridge = ui.global::<PrConflictsBridge>();
        bridge.set_active(true);
        let data = long_path_snapshot();
        ConflictController::default().present(&ui, &data);
        let weak = ui.as_weak();
        bridge.on_select_file(move |index| {
            weak.upgrade()
                .unwrap()
                .global::<PrConflictsBridge>()
                .set_selected_file(index);
        });
        assert_eq!(
            bridge.get_files().row_data(0).unwrap().name,
            "COVERAGE.en.md"
        );
        assert_eq!(
            bridge.get_files().row_data(4).unwrap().name,
            "bug-history.md"
        );
        assert_eq!(bridge.get_files().row_data(5).unwrap().name, "冲突说明.md");
        assert_eq!(bridge.get_files().row_data(6).unwrap().count, 0);
        for locale in ["en", "zh-CN"] {
            crate::i18n::apply_preference(locale).unwrap();
            for width in [800.0, 1200.0] {
                ui.window().dispatch_event(WindowEvent::Resized {
                    size: slint::LogicalSize::new(width, 900.0),
                });
                let names: Vec<_> =
                    ElementHandle::find_by_element_id(&ui, "PrReviewConflicts::file-name")
                        .collect();
                let paths: Vec<_> =
                    ElementHandle::find_by_element_id(&ui, "PrReviewConflicts::file-path")
                        .collect();
                let icons: Vec<_> =
                    ElementHandle::find_by_element_id(&ui, "PrReviewConflicts::conflict-icon")
                        .collect();
                assert_eq!(names.len(), 7);
                assert_eq!(
                    icons.len(),
                    7,
                    "nontext conflicts also have an unresolved icon"
                );
                for ((name, path), icon) in names.iter().zip(&paths).zip(&icons) {
                    assert!(
                        name.absolute_position().x + name.size().width + 12.0
                            <= icon.absolute_position().x + 0.5
                    );
                    assert!(
                        path.absolute_position().x + path.size().width + 12.0
                            <= icon.absolute_position().x + 0.5
                    );
                    assert!(
                        name.absolute_position().y + name.size().height
                            <= path.absolute_position().y
                    );
                }
                let label = format!(
                    "{}, {}",
                    data.files[4].path,
                    crate::i18n::tr("conflicts.title")
                );
                let row = ElementHandle::find_by_accessible_label(&ui, &label)
                    .next()
                    .unwrap();
                row.mock_single_click(PointerEventButton::Left);
                assert_eq!(bridge.get_selected_file(), 4);
                bridge.set_selected_file(0);
                ui.window().dispatch_event(WindowEvent::KeyPressed {
                    text: Key::Return.into(),
                });
                assert_eq!(bridge.get_selected_file(), 4);
                bridge.set_selected_file(0);
                ui.window()
                    .dispatch_event(WindowEvent::KeyPressed { text: " ".into() });
                assert_eq!(bridge.get_selected_file(), 4);
            }
        }
        crate::i18n::apply_preference("en").unwrap();
    }

    #[test]
    fn file_count_survives_back_but_not_review_reset() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let controller = ConflictController::default();
        let b = ui.global::<PrConflictsBridge>();
        assert_eq!(b.get_file_count(), -1);
        let mut data = snapshot();
        data.files.push(data.files[0].clone());
        controller.present(&ui, &data);
        assert_eq!(b.get_file_count(), 3);
        controller.leave_workspace(&ui);
        assert_eq!(b.get_files().row_count(), 0);
        assert_eq!(b.get_file_count(), 3);
        controller.close(&ui);
        assert_eq!(b.get_file_count(), -1);
        data.files.clear();
        data.status = conflicts::ConflictStatus::Clear;
        controller.present(&ui, &data);
        assert_eq!(b.get_file_count(), 0);
    }

    #[test]
    fn background_count_does_not_mount_files_or_change_the_review_tab() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let controller = ConflictController::default();
        let b = ui.global::<PrConflictsBridge>();
        ui.set_workflow_pr_review_tab(1);
        controller.generation.store(1, Ordering::SeqCst);
        b.set_loading(true);
        controller.finish_load(&ui, 1, Ok(Arc::new(snapshot())));
        assert_eq!(b.get_file_count(), 2);
        assert!(!b.get_active());
        assert!(!b.get_loading());
        assert_eq!(b.get_files().row_count(), 0);
        assert_eq!(b.get_rows().row_count(), 0);
        assert_eq!(ui.get_workflow_pr_review_tab(), 1);
        controller.close(&ui);
        controller.finish_load(&ui, 1, Ok(Arc::new(snapshot())));
        assert_eq!(b.get_file_count(), -1);
        controller.finish_load(&ui, 1, Err(ConflictError::Unavailable));
        assert!(b.get_error_key().is_empty());
    }

    #[test]
    fn opening_conflicts_reuses_pending_and_completed_prefetch() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let controller = ConflictController::default();
        let context = Context {
            alias: "fixture".into(),
            number: "42".into(),
            source: "head".into(),
            target: "main".into(),
        };
        *controller.context.lock().unwrap() = Some(context.clone());
        controller.generation.store(1, Ordering::SeqCst);
        let b = ui.global::<PrConflictsBridge>();
        b.set_loading(true);
        controller.open(
            &ui,
            context.alias.clone(),
            context.number.clone(),
            context.source.clone(),
            context.target.clone(),
        );
        assert!(b.get_active());
        assert_eq!(controller.generation.load(Ordering::SeqCst), 1);
        b.set_active(false);
        let mut data = snapshot();
        data.files.clear();
        data.status = conflicts::ConflictStatus::Clear;
        let data = Arc::new(data);
        controller.state.dispatch(Action::Reset(1));
        controller
            .state
            .dispatch(Action::Loaded(1, Ok(data.clone())));
        controller.finish_load(&ui, 1, Ok(data));
        controller.open(
            &ui,
            context.alias,
            context.number,
            context.source,
            context.target,
        );
        assert!(b.get_active());
        assert!(!b.get_loading());
        assert_eq!(b.get_file_count(), 0);
        assert_eq!(controller.generation.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn opening_the_active_conflict_session_preserves_selection_and_generation() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let controller = ConflictController::default();
        let context = Context {
            alias: "fixture".into(),
            number: "42".into(),
            source: "feature".into(),
            target: "main".into(),
        };
        *controller.context.lock().unwrap() = Some(context.clone());
        controller.generation.store(7, Ordering::SeqCst);
        controller.selection.store(4, Ordering::SeqCst);
        let b = ui.global::<PrConflictsBridge>();
        b.set_active(true);
        b.set_selected_file(1);
        b.set_selected_block(2);
        b.set_revision(5);
        for loading in [true, false] {
            b.set_loading(loading);
            controller.open(
                &ui,
                context.alias.clone(),
                context.number.clone(),
                context.source.clone(),
                context.target.clone(),
            );
            assert_eq!(controller.generation.load(Ordering::SeqCst), 7);
            assert_eq!(controller.selection.load(Ordering::SeqCst), 4);
            assert_eq!(b.get_selected_file(), 1);
            assert_eq!(b.get_selected_block(), 2);
            assert_eq!(b.get_revision(), 5);
            assert_eq!(b.get_loading(), loading);
        }
    }

    #[test]
    fn conflict_splitter_drags_clamps_and_preserves_width_when_toggled() {
        use i_slint_backend_testing::ElementHandle;
        use slint::platform::{PointerEventButton, WindowEvent};
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        bind_geometry(&ui);
        ui.set_current_route(crate::Route::PrReview);
        ui.set_show_navigation_drawer(false);
        ui.set_workflow_pr_review_loading(false);
        ui.window().dispatch_event(WindowEvent::Resized {
            size: slint::LogicalSize::new(1200.0, 900.0),
        });
        ui.global::<PrConflictsBridge>().set_active(true);
        let handle = || {
            ElementHandle::find_by_element_id(&ui, "PrReviewConflicts::files-resize-handle")
                .next()
                .unwrap()
        };
        let pane = || {
            ElementHandle::find_by_element_id(&ui, "PrReviewConflicts::files-pane")
                .next()
                .unwrap()
        };
        let drag = |delta: f32| {
            let h = handle();
            let start = slint::LogicalPosition::new(
                h.absolute_position().x + 4.0,
                h.absolute_position().y + 100.0,
            );
            let end = slint::LogicalPosition::new(start.x + delta, start.y);
            ui.window().dispatch_event(WindowEvent::PointerPressed {
                position: start,
                button: PointerEventButton::Left,
            });
            ui.window()
                .dispatch_event(WindowEvent::PointerMoved { position: end });
            ui.window().dispatch_event(WindowEvent::PointerReleased {
                position: end,
                button: PointerEventButton::Left,
            });
        };
        assert_eq!(pane().size().width, 260.0);
        drag(120.0);
        assert_eq!(pane().size().width, 380.0);
        drag(500.0);
        assert_eq!(pane().size().width, 520.0);
        drag(-400.0);
        assert_eq!(pane().size().width, 220.0);
        drag(80.0);
        let toggle = ElementHandle::find_by_element_id(&ui, "PrReviewConflicts::files-button")
            .next()
            .unwrap();
        let p = toggle.absolute_position();
        for _ in 0..2 {
            let position = slint::LogicalPosition::new(p.x + 16.0, p.y + 16.0);
            ui.window().dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            });
            ui.window().dispatch_event(WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            });
        }
        assert_eq!(pane().size().width, 300.0);
        ui.window().dispatch_event(WindowEvent::Resized {
            size: slint::LogicalSize::new(800.0, 900.0),
        });
        assert_eq!(pane().size().width, 300.0);
    }

    #[test]
    fn repeated_scroll_and_selection_keep_conflict_document_geometry_stable() {
        use i_slint_backend_testing::{ElementHandle, mock_elapsed_time};
        use slint::LogicalPosition;
        use slint::platform::{PointerEventButton, WindowEvent};
        use std::time::Duration;
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        bind_geometry(&ui);
        ui.set_current_route(crate::Route::PrReview);
        ui.set_show_navigation_drawer(false);
        ui.set_workflow_pr_review_loading(false);
        ui.window().dispatch_event(WindowEvent::Resized {
            size: slint::LogicalSize::new(1200.0, 900.0),
        });
        let b = ui.global::<PrConflictsBridge>();
        b.set_active(true);
        b.set_text_available(true);
        b.set_rows(ModelRc::new(
            projection::prepare(snapshot().files[0].clone()).into_model(),
        ));
        b.set_max_columns(200);
        let surface = ElementHandle::find_by_element_id(&ui, "PrReviewConflicts::code-surface")
            .next()
            .unwrap();
        let position = LogicalPosition::new(
            surface.absolute_position().x + 120.0,
            surface.absolute_position().y + 200.0,
        );
        let first_y = || {
            ElementHandle::find_by_element_id(&ui, "PrReviewConflicts::number-cell")
                .next()
                .unwrap()
                .absolute_position()
                .y
        };
        let initial_y = first_y();
        for delta_y in [-240.0, -360.0, 120.0, -2000.0, 300.0, 4000.0] {
            ui.window().dispatch_event(WindowEvent::PointerScrolled {
                position,
                delta_x: 0.0,
                delta_y,
            });
            for _ in 0..12 {
                mock_elapsed_time(Duration::from_millis(16));
                assert!(first_y().is_finite());
                assert_eq!(
                    b.invoke_layout(b.get_rows().row_count() as i32, 0.0, 1.0)
                        .total,
                    83.0 * 28.0
                );
            }
        }
        assert!((initial_y - first_y()).abs() < 1.0);
        // Code selection owns pointer drags; it must coexist with wheel scrolling.
        ui.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        for step in 1..8 {
            mock_elapsed_time(Duration::from_millis(16));
            ui.window().dispatch_event(WindowEvent::PointerMoved {
                position: LogicalPosition::new(position.x, position.y - step as f32 * 20.0),
            });
            let _ = first_y();
        }
        ui.window().dispatch_event(WindowEvent::PointerReleased {
            position: LogicalPosition::new(position.x, position.y - 140.0),
            button: PointerEventButton::Left,
        });
        for _ in 0..60 {
            mock_elapsed_time(Duration::from_millis(16));
            assert!(first_y().is_finite());
        }
        b.set_rows(ModelRc::default());
        mock_elapsed_time(Duration::from_millis(16));
        assert_eq!(b.invoke_layout(0, 2000.0, 600.0).total, 0.0);
    }

    #[test]
    fn native_layout_keeps_gutters_outside_code_at_narrow_and_wide_sizes() {
        use i_slint_backend_testing::ElementHandle;
        let _guard = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        bind_geometry(&ui);
        ui.set_current_route(crate::Route::PrReview);
        ui.set_show_navigation_drawer(false);
        ui.set_show_breadcrumb(false);
        ui.set_workflow_pr_review_loading(false);
        let bridge = ui.global::<PrConflictsBridge>();
        bridge.set_active(true);
        let snapshot = snapshot();
        let prepared = projection::prepare(snapshot.files[0].clone());
        bridge.set_rows(ModelRc::new(prepared.into_model()));
        bridge.set_max_columns(90);
        bridge.set_files(ModelRc::new(VecModel::from(vec![PrConflictFile {
            name: "pull_request.rs".into(),
            path: snapshot.files[0].path.clone().into(),
            target_path: snapshot.files[0].target_path.clone().into(),
            count: 2,
        }])));
        bridge.set_selected_file(0);
        bridge.set_text_available(true);
        for locale in ["en", "zh-CN"] {
            crate::i18n::apply_preference(locale).unwrap();
            for width in [800.0, 1200.0, 1800.0] {
                ui.window()
                    .dispatch_event(slint::platform::WindowEvent::Resized {
                        size: slint::LogicalSize::new(width, 900.0),
                    });
                let code =
                    ElementHandle::find_by_element_type_name(&ui, "GitHubSelectableDiffCode")
                        .next()
                        .expect("visible conflict code");
                let number =
                    ElementHandle::find_by_element_id(&ui, "PrReviewConflicts::number-cell")
                        .next()
                        .expect("line numbers");
                let lane =
                    ElementHandle::find_by_element_id(&ui, "PrReviewConflicts::bracket-lane")
                        .next()
                        .expect("bracket lane");
                assert!(
                    (number.absolute_position().x + number.size().width
                        - lane.absolute_position().x)
                        .abs()
                        < 1.0
                );
                assert!(
                    lane.absolute_position().x + lane.size().width <= code.absolute_position().x
                );
                assert!((code.absolute_position().y - number.absolute_position().y).abs() < 1.0);
                assert_eq!(number.size().height, 28.0);
                let element = |name: &str| {
                    ElementHandle::find_by_element_id(&ui, name)
                        .next()
                        .unwrap_or_else(|| panic!("missing layout element {name}"))
                };
                let title = element("PrReviewConflicts::title");
                let back = element("PrReviewConflicts::back-button");
                let status = element("PrReviewConflicts::status");
                let refresh = element("PrReviewConflicts::refresh-button");
                let center_y =
                    |item: &ElementHandle| item.absolute_position().y + item.size().height / 2.0;
                for item in [&back, &status, &refresh] {
                    assert!((center_y(item) - center_y(&title)).abs() < 1.0);
                }
                assert!(
                    title.absolute_position().x + title.size().width
                        <= status.absolute_position().x
                );
                assert!(
                    status.absolute_position().x + status.size().width
                        < refresh.absolute_position().x
                );
                let previous = element("PrReviewConflicts::previous-button");
                let next = element("PrReviewConflicts::next-button");
                let copy = element("PrReviewConflicts::copy-button");
                assert!(
                    previous.absolute_position().x + previous.size().width
                        < next.absolute_position().x
                );
                assert!(
                    next.absolute_position().x + next.size().width < copy.absolute_position().x
                );
                assert!((center_y(&previous) - center_y(&copy)).abs() < 1.0);
            }
        }
        crate::i18n::apply_preference("en").unwrap();
    }
}
