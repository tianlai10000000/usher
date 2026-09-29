//! Derived presentation can be rebuilt without discarding review navigation.
//! Kept opt-in until controlled native Release A/B acceptance is complete.
use super::*;

pub(crate) fn candidate_enabled() -> bool {
    cfg!(feature = "test-fixtures")
        && crate::app_paths::benchmark_offline_mode()
        && std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_some()
        && std::env::var("USHER_PR_PRESENTATION_LIFECYCLE").as_deref() == Ok("release-derived-v1")
}

impl WorkflowPrReviewController {
    pub(crate) fn route_changed(&self, ui: &MainWindow, previous: Route, current: Route) {
        if previous == Route::PrReview && current != Route::PrReview {
            self.conflicts.close(ui);
        } else if current == Route::PrReview && previous != Route::PrReview {
            let request = self.active_request.lock().unwrap().clone();
            if let Some(request) = request {
                self.conflicts.prefetch(
                    ui,
                    self.project_alias.lock().unwrap().clone(),
                    request.number,
                    request.source_branch,
                    request.target_branch,
                );
            }
        }
        if !candidate_enabled() {
            return;
        }
        if previous == Route::PrReview && current != Route::PrReview {
            self.suspend_presentation(ui);
        } else if current == Route::PrReview {
            self.resume_presentation(ui);
        }
    }

    fn suspend_presentation(&self, ui: &MainWindow) {
        if self.state.presentation_suspended() {
            return;
        }
        // Invalidates only derived work. Provider reads, remote mutations and
        // their Hook/notification delivery must not be cancelled by hiding UI.
        self.state.set_presentation_suspended(true);
        super::super::find_bridge::cancel();
        self.highlight_resources.detach();
        if let Ok(mut highlighted) = self.highlighted_files.lock() {
            highlighted.clear();
        }
        set_empty_line_model(ui);
        ui.set_workflow_pr_review_max_code_columns(0);
        ui.set_workflow_pr_review_longest_code_line("".into());
        ui.set_workflow_pr_review_code_loading(false);
        ui.set_workflow_pr_review_large_diff_loading(false);
        self.log_memory_diagnostics("presentation.suspended");
    }

    fn resume_presentation(&self, ui: &MainWindow) {
        if !self.state.presentation_suspended() {
            return;
        }
        self.state.set_presentation_suspended(false);
        let index = ui.get_workflow_pr_review_selected_file_index();
        if index < 0 {
            return;
        }
        // Detail, range, expanded context, filters and selected path stay in
        // their existing owners; do not call apply_detail/reset_review_ui here.
        if ui.get_workflow_pr_review_viewing_file() {
            ui.set_workflow_pr_review_viewing_file(false);
            self.toggle_view_file(ui);
        } else {
            self.select_file(ui, index);
        }
        self.log_memory_diagnostics("presentation.resumed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(ui: &MainWindow) -> (WorkflowPrReviewController, SharedDiffFile) {
        let list = super::super::super::super::pr_list::bind_pr_list(ui);
        let controller = bind_pr_review(ui, list);
        let request = ReviewRequest {
            number: "1".into(),
            title: "fixture".into(),
            author: "fixture".into(),
            source_branch: "head".into(),
            target_branch: "base".into(),
            updated_at: String::new(),
            ignore_whitespace: false,
        };
        let mut detail = ai_review_fixture_diff_detail("1");
        detail.files.truncate(1);
        detail.files[0].lines.truncate(1);
        let detail = Arc::new(SharedDiffDetail::from(detail));
        let file = detail.files[0].clone();
        *controller.project_alias.lock().unwrap() = "fixture".into();
        *controller.active_request.lock().unwrap() = Some(request.clone());
        *controller.cache.lock().unwrap() = Some(ReviewCache {
            project_alias: "fixture".into(),
            request,
            metadata: ProjectRepositoryMetadata {
                repo_project_id: "fixture".into(),
                repo_type: "github".into(),
                remote_repo: "https://example.invalid/fixture".into(),
            },
            all_detail: detail.clone(),
            base_detail: detail,
            range_diffs: BoundedCache::new(RANGE_DIFF_CACHE_CAPACITY),
            expanded_files: HashMap::new(),
            loaded_large_diffs: HashSet::new(),
        });
        ui.set_workflow_pr_review_selected_file_index(0);
        ui.set_workflow_pr_review_selected_file_path(file.path.clone().into());
        (controller, file)
    }

    fn tokens() -> Arc<Vec<HighlightedLine>> {
        Arc::new(vec![HighlightedLine {
            tokens: Vec::new(),
            columns: 1,
        }])
    }

    #[test]
    fn commit_range_keeps_whole_pr_totals_in_the_tab_row() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let (controller, _) = fixture(&ui);
        let mut detail = ai_review_fixture_diff_detail("1");
        detail.files.clear();
        detail.added_lines = 32842;
        detail.removed_lines = 2277;
        let all = Arc::new(SharedDiffDetail::from(detail.clone()));
        controller
            .cache
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .all_detail = all.clone();
        detail.added_lines = 12;
        detail.removed_lines = 3;
        let range = Arc::new(SharedDiffDetail::from(detail));
        controller.activate_commit_range(&ui, range);
        assert_eq!(ui.get_workflow_pr_review_added_lines(), 32842);
        assert_eq!(ui.get_workflow_pr_review_removed_lines(), 2277);
        controller.activate_commit_range(&ui, all);
        assert_eq!(ui.get_workflow_pr_review_added_lines(), 32842);
        assert_eq!(ui.get_workflow_pr_review_removed_lines(), 2277);
    }

    #[test]
    fn ordinary_review_entry_starts_conflict_count_without_opening_conflicts() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let (controller, _) = fixture(&ui);
        let request = controller.active_request.lock().unwrap().clone().unwrap();
        controller.open(&ui, request, false);
        let b = ui.global::<crate::PrConflictsBridge>();
        assert!(b.get_loading());
        assert!(!b.get_active());
        assert_eq!(b.get_file_count(), -1);
        controller.conflicts.close(&ui);
        controller.route_changed(&ui, Route::Workflow, Route::PrReview);
        assert!(b.get_loading());
        assert!(!b.get_active());
        controller.conflicts.close(&ui);
    }

    #[test]
    fn replacing_synthetic_reviews_retains_only_the_latest_models_and_cache() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let (controller, initial_file) = fixture(&ui);
        drop(initial_file);
        let template = controller.active_request.lock().unwrap().clone().unwrap();
        let mut old_files = Vec::new();
        let mut old_tokens = Vec::new();
        let mut old_details = Vec::new();
        for cycle in 0..36 {
            // Exercise the same reset boundary as opening a different PR,
            // without network I/O or claiming this is a native RSS benchmark.
            controller.invalidate_pending_requests();
            *controller.cache.lock().unwrap() = None;
            controller.reset_request_scoped_state(&ui, true);
            assert!(controller.source_files.lock().unwrap().entries.is_empty());
            assert!(
                controller
                    .highlighted_files
                    .lock()
                    .unwrap()
                    .entries
                    .is_empty()
            );
            let number = format!("synthetic-{}", cycle % 3);
            let mut request = template.clone();
            request.number = number.clone();
            let mut detail = ai_review_fixture_diff_detail(&number);
            detail.files.truncate(1);
            let line = detail.files[0].lines[0].clone();
            let count = [16, 64, 256][cycle % 3];
            detail.files[0].path = format!("synthetic/{number}.rs");
            detail.files[0].lines = vec![line; count];
            let detail = Arc::new(SharedDiffDetail::from(detail));
            let file = detail.files[0].clone();
            let highlighted = Arc::new(vec![tokens()[0].clone(); count]);
            let cache = ReviewCache {
                project_alias: "fixture".into(),
                request: request.clone(),
                metadata: ProjectRepositoryMetadata {
                    repo_project_id: "fixture".into(),
                    repo_type: "github".into(),
                    remote_repo: "https://example.invalid/fixture".into(),
                },
                all_detail: detail.clone(),
                base_detail: detail.clone(),
                range_diffs: BoundedCache::new(RANGE_DIFF_CACHE_CAPACITY),
                expanded_files: HashMap::new(),
                loaded_large_diffs: HashSet::new(),
            };
            *controller.active_request.lock().unwrap() = Some(request);
            *controller.cache.lock().unwrap() = Some(cache);
            controller.source_files.lock().unwrap().insert(
                format!("{number}:head:{}", file.path),
                "// synthetic source\n".repeat(count),
            );
            controller
                .highlighted_files
                .lock()
                .unwrap()
                .insert(file.path.clone(), highlighted.clone());
            old_files.push(Arc::downgrade(&file));
            old_tokens.push(Arc::downgrade(&highlighted));
            old_details.push(Arc::downgrade(&detail));
            apply_highlighted_file(&ui, file, highlighted, true);
            drop(detail);
            let lines = ui.get_workflow_pr_review_lines();
            controller.route_changed(&ui, Route::PrReview, Route::Home);
            assert!(lines == ui.get_workflow_pr_review_lines());
            assert_eq!(lines.row_count(), count);
            assert_eq!(controller.source_files.lock().unwrap().entries.len(), 1);
            assert_eq!(
                controller.highlighted_files.lock().unwrap().entries.len(),
                1
            );
            assert!(
                old_files[..cycle]
                    .iter()
                    .all(|weak| weak.strong_count() == 0)
            );
            assert!(
                old_tokens[..cycle]
                    .iter()
                    .all(|weak| weak.strong_count() == 0)
            );
            assert!(
                old_details[..cycle]
                    .iter()
                    .all(|weak| weak.strong_count() == 0)
            );
        }
        controller.clear_owned_state(&ui);
        assert!(old_files.iter().all(|weak| weak.strong_count() == 0));
        assert!(old_tokens.iter().all(|weak| weak.strong_count() == 0));
        assert!(old_details.iter().all(|weak| weak.strong_count() == 0));
    }

    #[test]
    fn sidebar_navigation_retains_resources_presentation_and_request_generations() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        for (locale, full_file) in [("en", false), ("zh-CN", true)] {
            let ui = MainWindow::new().unwrap();
            crate::i18n::apply_preference(locale).unwrap();
            let (controller, file) = fixture(&ui);
            let highlighted = tokens();
            let token_weak = Arc::downgrade(&highlighted);
            controller
                .highlighted_files
                .lock()
                .unwrap()
                .insert(file.path.clone(), highlighted.clone());
            apply_highlighted_file(&ui, file.clone(), highlighted, false);
            ui.set_workflow_pr_review_viewing_file(full_file);
            ui.set_workflow_pr_review_commit_range_start(2);
            ui.set_workflow_pr_review_commit_range_end(4);
            let lines = ui.get_workflow_pr_review_lines();
            let metrics = ui.get_workflow_pr_review_line_metrics();
            let revision = ui.get_workflow_pr_review_line_content_revision();
            let kinds = [
                ReviewRequestKind::Highlight,
                ReviewRequestKind::Expand,
                ReviewRequestKind::RemoteMutation,
            ];
            let generations = kinds.map(|kind| controller.state.begin(kind));
            let lease = controller.highlight_resources.lease();
            let weak = Arc::downgrade(lease.get_or_init(|| Arc::new(HighlightResources::load())));
            drop(lease);

            controller.route_changed(&ui, Route::PrReview, Route::Home);
            assert!(weak.upgrade().is_some());
            assert!(controller.highlight_resources.is_loaded());
            assert!(token_weak.upgrade().is_some());
            assert!(lines == ui.get_workflow_pr_review_lines());
            assert!(metrics == ui.get_workflow_pr_review_line_metrics());
            assert_eq!(revision, ui.get_workflow_pr_review_line_content_revision());
            assert_eq!(full_file, ui.get_workflow_pr_review_viewing_file());
            assert_eq!(ui.get_workflow_pr_review_commit_range_start(), 2);
            assert_eq!(ui.get_workflow_pr_review_commit_range_end(), 4);
            assert!(!controller.state.presentation_suspended());
            for (kind, generation) in kinds.into_iter().zip(generations) {
                assert!(controller.state.is_current(kind, generation));
            }
            controller.route_changed(&ui, Route::Home, Route::PrReview);
            assert!(lines == ui.get_workflow_pr_review_lines());
            assert!(controller.highlight_resources.is_loaded());
            assert!(Arc::ptr_eq(
                &controller
                    .cache
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .base_detail
                    .files[0],
                &file
            ));
            controller.clear_owned_state(&ui);
            assert!(weak.upgrade().is_none());
        }
        crate::i18n::apply_preference(usher_i18n::source_locale().id).unwrap();
    }

    #[test]
    fn leave_releases_derived_owners_and_reentry_preserves_review_context() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        for locale in ["en", "zh-CN"] {
            let ui = MainWindow::new().unwrap();
            crate::i18n::apply_preference(locale).unwrap();
            let (controller, file) = fixture(&ui);
            ui.set_workflow_pr_review_tab(1);
            ui.set_workflow_pr_review_commit_range_start(2);
            ui.set_workflow_pr_review_commit_range_end(4);
            *controller.file_filter.lock().unwrap() = "fixture".into();
            controller
                .collapsed_folders
                .lock()
                .unwrap()
                .insert("src".into());
            let highlighted = tokens();
            let weak = Arc::downgrade(&highlighted);
            let grammar = controller.highlight_resources.lease();
            let grammar_weak =
                Arc::downgrade(grammar.get_or_init(|| Arc::new(HighlightResources::load())));
            drop(grammar);
            controller
                .highlighted_files
                .lock()
                .unwrap()
                .insert(file.path.clone(), highlighted.clone());
            apply_highlighted_file(&ui, file.clone(), highlighted, false);
            assert!(weak.upgrade().is_some()); // baseline retains actual lazy-model/cache owners
            assert_eq!(ui.get_workflow_pr_review_lines().row_count(), 1);
            controller.suspend_presentation(&ui);
            assert!(weak.upgrade().is_none()); // candidate releases final token owner
            assert!(grammar_weak.upgrade().is_none());
            assert_eq!(ui.get_workflow_pr_review_lines().row_count(), 0);
            assert_eq!(ui.get_workflow_pr_review_line_metrics().row_count(), 0);
            assert!(!controller.highlight_resources.is_loaded());
            assert!(
                controller
                    .highlighted_files
                    .lock()
                    .unwrap()
                    .entries
                    .is_empty()
            );
            assert!(Arc::ptr_eq(
                &controller
                    .cache
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .base_detail
                    .files[0],
                &file
            ));
            assert_eq!(ui.get_workflow_pr_review_tab(), 1);
            assert_eq!(ui.get_workflow_pr_review_commit_range_start(), 2);
            assert_eq!(ui.get_workflow_pr_review_commit_range_end(), 4);
            assert_eq!(*controller.file_filter.lock().unwrap(), "fixture");
            assert!(controller.collapsed_folders.lock().unwrap().contains("src"));
            // Deterministic completion fixture avoids network/native rendering.
            controller
                .highlighted_files
                .lock()
                .unwrap()
                .insert(file.path.clone(), tokens());
            controller.resume_presentation(&ui);
            assert!(!controller.state.presentation_suspended());
            assert_eq!(ui.get_workflow_pr_review_lines().row_count(), 1);
            assert_eq!(
                ui.get_workflow_pr_review_selected_file_path().as_str(),
                file.path
            );
            assert_eq!(ui.get_workflow_pr_review_tab(), 1);
            assert_eq!(ui.get_workflow_pr_review_commit_range_start(), 2);
            controller.suspend_presentation(&ui);
            controller.clear_owned_state(&ui);
            assert!(controller.active_request.lock().unwrap().is_none());
            assert!(controller.cache.lock().unwrap().is_none());
            assert!(!controller.state.presentation_suspended());
        }
        crate::i18n::apply_preference(usher_i18n::source_locale().id).unwrap();
    }

    #[test]
    fn late_diff_selection_does_not_rebuild_hidden_highlighting() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let (controller, _) = fixture(&ui);
        controller.suspend_presentation(&ui);
        controller.select_file(&ui, 0);
        assert_eq!(ui.get_workflow_pr_review_lines().row_count(), 0);
        assert!(!ui.get_workflow_pr_review_code_loading());
        assert!(!controller.highlight_resources.is_loaded());
    }
}
