//! Offline real callback fixture, identical on both sides of the Find A/B.
use super::diff_model::LazyPrDiffLineModel;
use crate::git::{PullRequestDiffFile, PullRequestDiffLine};
use crate::pr_review_ui::PrReviewUiExt;
use crate::workflow::syntax_highlight::HighlightedLine;
use crate::{MainWindow, Route, WorkflowPrDiffFindMatch};
use slint::{ComponentHandle, Model, ModelRc};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) fn enabled() -> bool {
    std::env::var("USHER_P5_BENCHMARK").as_deref() == Ok("61")
        && crate::app_paths::benchmark_offline_mode()
        && std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_some()
}
pub(super) fn setup(ui: &MainWindow) {
    if !enabled() {
        return;
    }
    let weak = ui.as_weak();
    crate::startup::run_after_first_frame(move || {
        slint::Timer::single_shot(Duration::from_secs(2), move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            install(&ui, false, 30000);
            round(weak, 0);
        });
    });
}
fn install(ui: &MainWindow, split: bool, count: usize) {
    let lines = (0..count)
        .map(|index| PullRequestDiffLine {
            old_line: index as i32 + 1,
            new_line: index as i32 + 1,
            kind: "context".into(),
            content: format!("line {index:05}: alpha alpha 中文 İSTANBUL\tend"),
        })
        .collect::<Vec<_>>();
    let highlighted = Arc::new(
        lines
            .iter()
            .map(|line| HighlightedLine {
                tokens: Vec::new(),
                columns: line.content.chars().count() as u32 + 3,
            })
            .collect::<Vec<_>>(),
    );
    let file = Arc::new(PullRequestDiffFile {
        path: "p5-find.txt".into(),
        old_path: "p5-find.txt".into(),
        change_type: "edit_file".into(),
        added_lines: 0,
        removed_lines: 0,
        viewed: false,
        lines,
    });
    let model = Rc::new(LazyPrDiffLineModel::new(file, highlighted, split));
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_line_metrics(ModelRc::new(slint::VecModel::from(
            model.row_metrics(),
        )));
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_lines(model.into());
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_line_content_revision(
            ui.pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_review_line_content_revision()
                .wrapping_add(1),
        );
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_selected_file_path("p5-find.txt".into());
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_selected_file_index(0);
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_diff_view(if split { "split" } else { "unified" }.into());
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_loading(false);
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_code_loading(false);
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_visible(true);
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_tab(2);
    ui.invoke_navigate(Route::PrReview);
}
fn round(weak: slint::Weak<MainWindow>, index: usize) {
    slint::Timer::single_shot(Duration::from_millis(600), move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if index == 4 {
            install(&ui, true, 30000);
        }
        if index == 8 {
            install(&ui, false, 7000);
        }
        let split = (4..8).contains(&index);
        let started = Instant::now();
        for query in ["a", "alpha", "İ", "中文", "none", "end"] {
            let _ = ui
                .pr_review()
                .global::<crate::PrDiffBridge>()
                .invoke_workflow_pr_review_find_diff_matches(query.into(), split);
        }
        let model = ui
            .pr_review()
            .global::<crate::PrDiffBridge>()
            .invoke_workflow_pr_review_find_diff_matches("中文".into(), split);
        log::info!(
            "[P5][61] layout_us={} round={index}",
            started.elapsed().as_micros()
        );
        let expected = if index >= 8 {
            7000
        } else if split {
            60000
        } else {
            30000
        };
        check(weak, index, model, expected, Instant::now());
    });
}
fn check(
    weak: slint::Weak<MainWindow>,
    index: usize,
    model: ModelRc<WorkflowPrDiffFindMatch>,
    expected: usize,
    started: Instant,
) {
    slint::Timer::single_shot(Duration::from_millis(50), move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if model.row_count() != expected {
            if started.elapsed() < Duration::from_secs(5) {
                check(weak, index, model, expected, started);
            } else {
                log::error!(
                    "[P5][61] timed_out round={index} count={} expected={expected}",
                    model.row_count()
                );
            }
            return;
        }
        let first = model.row_data(0).unwrap();
        let last = model.row_data(expected - 1).unwrap();
        let valid = first.line == 0
            && first.text == "中文"
            && last.text == "中文"
            && last.line == if index >= 8 { 6999 } else { 29999 };
        log::info!(
            "[P5][61] round={index} count={expected} valid={valid} latency_ms={}",
            started.elapsed().as_millis()
        );
        if !valid {
            return;
        }
        if index == 11 {
            log::info!("[P5][61] complete");
        } else {
            round(ui.as_weak(), index + 1);
        }
    });
}
