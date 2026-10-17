//! Offline fixture through the production selection and full-file paths.
use super::*;
use crate::git::PullRequestDiffLine;
use crate::pr_review_ui::PrReviewUiExt;
use std::time::{Duration, Instant};

pub(super) fn setup(ui: &MainWindow, controller: WorkflowPrReviewController) {
    if std::env::var("USHER_P6_BENCHMARK").as_deref() != Ok("66")
        || !crate::app_paths::benchmark_offline_mode()
        || std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_none()
    {
        return;
    }
    let weak = ui.as_weak();
    crate::startup::run_after_first_frame(move || {
        slint::Timer::single_shot(Duration::from_secs(2), move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.window().set_size(slint::LogicalSize::new(1100.0, 700.0));
            heartbeat(ui.as_weak(), Instant::now(), Instant::now());
            cycle(ui.as_weak(), controller, 0);
        });
    });
}

fn heartbeat(weak: slint::Weak<MainWindow>, start: Instant, last: Instant) {
    slint::Timer::single_shot(Duration::from_millis(50), move || {
        if weak.upgrade().is_none() || !crate::p6_benchmark::heartbeat_active(26) {
            return;
        }
        log::info!("[P6][66] heartbeat_us={}", last.elapsed().as_micros());
        heartbeat(weak, start, Instant::now());
    });
}

fn install(ui: &MainWindow, controller: &WorkflowPrReviewController) {
    controller.clear_owned_state(ui);
    let request = ReviewRequest {
        number: "66".into(),
        title: "P6 highlight fixture".into(),
        author: "fixture".into(),
        source_branch: "head".into(),
        target_branch: "base".into(),
        updated_at: String::new(),
        ignore_whitespace: false,
    };
    let detail = Arc::new(SharedDiffDetail::from(PullRequestDiffDetail {
        files: (0..8)
            .map(|file| PullRequestDiffFile {
                path: format!("fixture-{file}.rs"),
                lines: (0..if file == 7 { 3000 } else { 12000 })
                    .map(|index| PullRequestDiffLine {
                        old_line: index + 1,
                        new_line: index + 1,
                        kind: "context".into(),
                        content: format!(
                            "let value_{index} = Some(\"highlight fixture {file}\"); // typed value"
                        ),
                    })
                    .collect(),
                old_path: String::new(),
                change_type: "edit_file".into(),
                added_lines: 0,
                removed_lines: 0,
                viewed: false,
            })
            .collect(),
        author_identity: Default::default(),
        added_lines: 0,
        removed_lines: 0,
        commit_count: 1,
        conversation_count: None,
        base_sha: "base".into(),
        head_sha: "head".into(),
        start_sha: "base".into(),
        too_many_commits: false,
        files_truncated: false,
    }));
    *controller.cache.lock().unwrap() = Some(ReviewCache {
        project_alias: "p6".into(),
        request: request.clone(),
        metadata: ProjectRepositoryMetadata {
            repo_project_id: "p6".into(),
            repo_type: "github".into(),
            remote_repo: "https://example.invalid/p6".into(),
        },
        all_detail: detail.clone(),
        base_detail: detail,
        range_diffs: BoundedCache::new(RANGE_DIFF_CACHE_CAPACITY),
        expanded_files: HashMap::new(),
        loaded_large_diffs: (0..8).collect(),
    });
    apply_review_header(ui, &request);
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_diff_view("unified".into());
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_loading(false);
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_visible(true);
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_tab(2);
    ui.set_current_route(Route::PrReview);
}

fn cycle(weak: slint::Weak<MainWindow>, controller: WorkflowPrReviewController, index: usize) {
    slint::Timer::single_shot(Duration::from_millis(500), move || {
        let Some(ui) = weak.upgrade() else { return };
        install(&ui, &controller);
        let started = Instant::now();
        for file_index in 0..8 {
            controller.select_file(&ui, file_index);
            if index % 3 == 1 {
                let file = controller
                    .cache
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .shared_file(file_index as usize)
                    .unwrap();
                let count = if file_index == 7 { 3001 } else { 12000 };
                controller.render_full_file(
                    &ui,
                    file_index as usize,
                    file,
                    "let full_source = Some(42); // full file\n".repeat(count),
                );
            }
        }
        if index % 3 == 2 {
            controller.clear_owned_state(&ui);
            install(&ui, &controller);
            controller.select_file(&ui, 7);
        }
        await_result(weak, controller, index, started);
    });
}

fn await_result(
    weak: slint::Weak<MainWindow>,
    controller: WorkflowPrReviewController,
    index: usize,
    started: Instant,
) {
    slint::Timer::single_shot(Duration::from_millis(50), move || {
        let Some(ui) = weak.upgrade() else { return };
        let expected = if index % 3 == 1 { 3001 } else { 3000 };
        let current = ui
            .pr_review()
            .global::<crate::PrReviewBridge>()
            .get_workflow_pr_review_selected_file_path()
            == "fixture-7.rs"
            && ui
                .pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_review_lines()
                .row_count()
                == expected
            && ui
                .pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_review_viewing_file()
                == (index % 3 == 1);
        if !current
            || ui
                .pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_review_code_loading()
        {
            if !current
                && !ui
                    .pr_review()
                    .global::<crate::PrReviewBridge>()
                    .get_workflow_pr_review_code_loading()
            {
                log::warn!("[P6][66] premature_loading_clear cycle={index}");
            }
            if started.elapsed() < Duration::from_secs(20) {
                await_result(weak, controller, index, started);
            }
            return;
        }
        log::info!(
            "[P6][66] cycle={index} completed_ms={}",
            started.elapsed().as_millis()
        );
        if !crate::p6_benchmark::next_cycle(index + 1) {
            controller.clear_owned_state(&ui);
            ui.set_current_route(Route::Project);
            log::info!("[P6][66] complete");
        } else {
            cycle(weak, controller, index + 1);
        }
    });
}
