//! Event-loop capture of the PR and active Agent before work is queued.

use super::review_input::PendingReviewRequest;
use crate::pr_review_ui::PrReviewUiExt;
use crate::{MainWindow, WorkflowPullRequestItem};
use slint::ComponentHandle;
use slint::Model;
use usher_ai_review::ProjectReviewKey;

/// Capture current PR fields from their actual UI owner at the caller's
/// existing event-loop boundary, before producer admission or worker dispatch.
/// A pre-captured number stays paired with the caller's existing-task lookup.
pub(super) fn request_from_review_ui(
    ui: &MainWindow,
    number: Option<&str>,
    extra_requirements: &str,
) -> PendingReviewRequest {
    let page = ui.pr_review();
    let state = page.global::<crate::PrReviewBridge>();
    let current_number = state.get_workflow_pr_review_number();
    request_from_ui(
        ui,
        number.unwrap_or(current_number.as_str()),
        state.get_workflow_pr_review_title().as_str(),
        state.get_workflow_pr_review_source_branch().as_str(),
        state.get_workflow_pr_review_target_branch().as_str(),
        extra_requirements,
    )
}

pub(super) fn request_from_ui(
    ui: &MainWindow,
    number: &str,
    title: &str,
    source_branch: &str,
    target_branch: &str,
    extra_requirements: &str,
) -> PendingReviewRequest {
    let row = current_pr_row(ui, number);
    PendingReviewRequest {
        project_alias: ui.get_workflow_project_alias().to_string(),
        project_key: ProjectReviewKey(ui.get_workflow_project_id().to_string()),
        project_name: ui.get_workflow_project_name().to_string(),
        local_repo: ui.get_workflow_project_local_repo().to_string(),
        remote_repo: ui.get_workflow_project_remote_repo().to_string(),
        repo_type: ui.get_workflow_pr_repo_type().to_string(),
        repo_project_id: ui.get_workflow_pr_repo_project_id().to_string(),
        number: number.to_string(),
        title: title.to_string(),
        author: row
            .as_ref()
            .map(|row| row.author.to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                ui.pr_review()
                    .global::<crate::PrReviewBridge>()
                    .get_workflow_pr_review_author()
                    .to_string()
            }),
        source_branch: source_branch.to_string(),
        target_branch: target_branch.to_string(),
        updated_at: row
            .as_ref()
            .map(|row| row.updated_at_raw.to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                ui.pr_review()
                    .global::<crate::PrReviewBridge>()
                    .get_workflow_pr_review_updated_at()
                    .to_string()
            }),
        profile: crate::ai::settings_state::current_profile(),
        extra_requirements: extra_requirements.trim().to_string(),
    }
}

fn current_pr_row(ui: &MainWindow, number: &str) -> Option<WorkflowPullRequestItem> {
    let model = ui.get_workflow_pr_list_items();
    (0..model.row_count()).find_map(|index| {
        model
            .row_data(index)
            .filter(|row| row.number.as_str() == number)
    })
}
