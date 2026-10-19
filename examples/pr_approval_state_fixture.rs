//! Offline manual acceptance fixture using the production PR list/review UI.
use slint::{ComponentHandle, LogicalSize, Model, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc, time::Duration};
use usher_lib::PrReviewUiExt;
use usher_lib::{MainWindow, Route, WorkflowPullRequestItem};

type Rows = Rc<RefCell<Vec<WorkflowPullRequestItem>>>;

fn copy(locale: &str, key: &str) -> slint::SharedString {
    usher_i18n::translate_for(locale, key).into_owned().into()
}

fn show_list(ui: &MainWindow, rows: &Rows) {
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_review_visible(false);
    let rows = rows.borrow();
    let closed = ui.get_workflow_pr_list_state() == "closed";
    ui.set_workflow_pr_list_query(
        if closed {
            "is:pr is:closed"
        } else {
            "is:pr is:open"
        }
        .into(),
    );
    let visible = rows
        .iter()
        .filter(|row| (row.state != "open") == closed)
        .cloned()
        .collect::<Vec<_>>();
    ui.set_workflow_pr_list_open_total(rows.iter().filter(|row| row.state == "open").count() as i32);
    ui.set_workflow_pr_list_closed_total(
        rows.iter().filter(|row| row.state != "open").count() as i32
    );
    ui.set_workflow_pr_list_total(visible.len() as i32);
    ui.set_workflow_pr_list_total_exact(true);
    ui.set_workflow_pr_list_page(1);
    ui.set_workflow_pr_list_page_count(1);
    ui.set_workflow_pr_list_items(ModelRc::new(VecModel::from(visible)));
    ui.set_current_route(Route::Workflow);
}

fn show_review(ui: &MainWindow, row: &WorkflowPullRequestItem, locale: &str) {
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_review_visible(true);
    let terminal = row.state != "open";
    let kind = if terminal {
        row.state.as_str()
    } else if row.review_status == "changes_requested" {
        "changes_requested"
    } else if row.number == "43" {
        "review_required"
    } else {
        "ready"
    };
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_review_title(row.title.clone());
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_review_number(row.number.clone());
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_review_author(row.author.clone());
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_review_source_branch(row.source_branch.clone());
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_review_target_branch(row.target_branch.clone());
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_review_updated_at(row.updated_at.clone());
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_review_conversation_count(row.conversation_count);
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_remote_review_state(if row.review_status == "changes_requested" {
            "rejected".into()
        } else {
            row.review_status.clone()
        });
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_can_comment(true);
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_can_approve(!terminal);
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_can_request_changes(!terminal);
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_can_merge(!terminal && kind == "ready");
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_merge_method_merge(true);
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_merge_method_squash(true);
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_merge_readiness_kind(kind.into());
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_merge_readiness_label(copy(
            locale,
            &format!("pr_review.readiness.{kind}"),
        ));
    let detail = copy(locale, &format!("pr_review.readiness_detail.{kind}"));
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_merge_readiness_detail(detail.clone());
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_merge_block_reason(if kind == "ready" { "".into() } else { detail });
    ui.set_current_route(Route::PrReview);
}

fn return_after_preview(ui: &MainWindow, rows: Rows) {
    let weak = ui.as_weak();
    let number = ui
        .pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .get_workflow_pr_review_number();
    slint::Timer::single_shot(Duration::from_secs(2), move || {
        if let Some(ui) = weak.upgrade()
            && ui.get_current_route() == Route::PrReview
            && ui
                .pr_review()
                .global::<usher_lib::PrReviewBridge>()
                .get_workflow_pr_review_number()
                == number
        {
            show_list(&ui, &rows);
        }
    });
}

fn main() -> Result<(), slint::PlatformError> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let locale = args
        .windows(2)
        .find(|pair| pair[0] == "--locale")
        .map(|pair| pair[1].clone())
        .unwrap_or_else(|| "en".into());
    let ui = MainWindow::new()?;
    usher_lib::install_floating_overlay_positioning(&ui);
    slint::select_bundled_translation(&locale).expect("use a packaged locale such as en or zh-CN");
    ui.window().set_size(LogicalSize::new(1200.0, 900.0));
    // The production review command strip is mounted in the macOS title bar.
    ui.set_platform_is_macos(true);
    ui.set_show_navigation_drawer(false);
    ui.set_show_breadcrumb(false);
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_review_visible(true);
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_review_tab(0);
    ui.set_workflow_project_name("Offline PR approval fixtures".into());
    ui.set_workflow_pr_repo_type("github".into());
    ui.set_workflow_selected_flow(2);
    ui.set_workflow_pr_list_state("open".into());
    let rows: Rows = Rc::new(RefCell::new(
        [
            (
                "43",
                "open",
                "review_required",
                "Approval still needs another reviewer",
            ),
            (
                "42",
                "open",
                "approved",
                "Approved and ready for an explicit merge",
            ),
            (
                "41",
                "open",
                "changes_requested",
                "Changes requested before merging",
            ),
            ("40", "merged", "approved", "Already merged pull request"),
            ("39", "closed", "", "Closed without merging"),
        ]
        .into_iter()
        .map(|(number, state, review, title)| WorkflowPullRequestItem {
            number: number.into(),
            title: title.into(),
            state: state.into(),
            author: "reviewer".into(),
            source_branch: "feature/approval".into(),
            target_branch: "main".into(),
            review_status: review.into(),
            conflict_status: "clear".into(),
            updated_at: copy(&locale, "time.just_now"),
            ..Default::default()
        })
        .collect(),
    ));
    let (weak, data, language) = (ui.as_weak(), rows.clone(), locale.clone());
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .on_workflow_pr_review_open(move |number, _, _, _, _, _, tab| {
            if let Some(ui) = weak.upgrade()
                && let Some(row) = data.borrow().iter().find(|row| row.number == number)
            {
                ui.pr_review()
                    .global::<usher_lib::PrReviewBridge>()
                    .set_workflow_pr_review_tab(tab.clamp(0, 2));
                show_review(&ui, row, &language);
            }
        });
    let (weak, data, language) = (ui.as_weak(), rows.clone(), locale.clone());
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .on_workflow_pr_submit_review(move |action, _| {
            let Some(ui) = weak.upgrade() else { return };
            let retained = ui.get_workflow_pr_list_items();
            let hidden_review = (0..retained.row_count()).find_map(|index| {
                let row = retained.row_data(index)?;
                (row.number
                    == ui
                        .pr_review()
                        .global::<usher_lib::PrReviewBridge>()
                        .get_workflow_pr_review_number())
                .then_some((index, row.review_status))
            });
            let mut rows = data.borrow_mut();
            let Some(row) = rows.iter_mut().find(|row| {
                row.number
                    == ui
                        .pr_review()
                        .global::<usher_lib::PrReviewBridge>()
                        .get_workflow_pr_review_number()
            }) else {
                return;
            };
            if action == "comment" {
                row.conversation_count += 1;
                ui.pr_review()
                    .global::<usher_lib::PrReviewBridge>()
                    .set_workflow_pr_comment_submission_revision(
                        ui.pr_review()
                            .global::<usher_lib::PrReviewBridge>()
                            .get_workflow_pr_comment_submission_revision()
                            .wrapping_add(1),
                    );
            } else {
                row.review_status = if action == "approve" {
                    "approved"
                } else {
                    "changes_requested"
                }
                .into();
            }
            ui.pr_review()
                .global::<usher_lib::PrReviewBridge>()
                .set_workflow_pr_review_dialog_open(false);
            show_review(&ui, row, &language);
            assert!(
                ui.get_workflow_pr_list_items() == retained,
                "hidden list must retain its model"
            );
            if let Some((index, previous_review)) = hidden_review {
                assert_eq!(
                    retained.row_data(index).unwrap().review_status,
                    previous_review
                );
            }
            if action != "comment" {
                assert_eq!(row.state, "open", "Approve must not merge");
            }
            eprintln!(
                "Offline underlying PR #{}: lifecycle={}, review={}",
                row.number, row.state, row.review_status
            );
            drop(rows);
            if action != "comment" {
                return_after_preview(&ui, data.clone());
            }
        });
    let (weak, data, language) = (ui.as_weak(), rows.clone(), locale.clone());
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .on_workflow_pr_merge(move |_, _| {
            let Some(ui) = weak.upgrade() else { return };
            if !ui
                .pr_review()
                .global::<usher_lib::PrReviewBridge>()
                .get_workflow_pr_can_merge()
            {
                return;
            }
            let mut rows = data.borrow_mut();
            let Some(row) = rows.iter_mut().find(|row| {
                row.number
                    == ui
                        .pr_review()
                        .global::<usher_lib::PrReviewBridge>()
                        .get_workflow_pr_review_number()
            }) else {
                return;
            };
            row.state = "merged".into();
            ui.pr_review()
                .global::<usher_lib::PrReviewBridge>()
                .set_workflow_pr_merge_dialog_open(false);
            show_review(&ui, row, &language);
            drop(rows);
            return_after_preview(&ui, data.clone());
        });
    for callback in [false, true] {
        let (weak, data) = (ui.as_weak(), rows.clone());
        let back = move || {
            if let Some(ui) = weak.upgrade() {
                show_list(&ui, &data);
            }
        };
        if callback {
            ui.on_router_back(back);
        } else {
            ui.pr_review()
                .global::<usher_lib::PrReviewBridge>()
                .on_workflow_pr_review_back(back);
        }
    }
    let weak = ui.as_weak();
    ui.pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .on_workflow_pr_review_select_tab(move |tab| {
            if let Some(ui) = weak.upgrade() {
                ui.pr_review()
                    .global::<usher_lib::PrReviewBridge>()
                    .set_workflow_pr_review_tab(tab.clamp(0, 2));
            }
        });
    let (weak, data) = (ui.as_weak(), rows.clone());
    ui.on_workflow_pr_list_state_change(move |state| {
        if let Some(ui) = weak.upgrade() {
            ui.set_workflow_pr_list_state(state);
            show_list(&ui, &data);
        }
    });
    show_list(&ui, &rows);
    if args.iter().any(|arg| arg == "--detail") {
        show_review(&ui, &rows.borrow()[0], &locale);
    }
    ui.show()?;
    // This fixture has no native chrome monitor. Forward real window pointer
    // events through the same post-dispatch channel as the application.
    usher_lib::install_transient_input_bridge(&ui)?;
    slint::run_event_loop()
}
