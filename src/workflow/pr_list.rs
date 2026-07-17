use chrono::{DateTime, FixedOffset};

use crate::git::{PullRequestItem, PullRequestState, get_pull_requests};
use crate::project::{ProjectRepositoryMetadata, load_repository_metadata};
use crate::{MainWindow, WorkflowPullRequestItem};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const PAGE_SIZE: u32 = 10;

#[derive(Clone)]
pub(crate) struct WorkflowPrListController {
    context: Arc<Mutex<Option<ProjectRepositoryMetadata>>>,
    visited_pages: Arc<Mutex<HashMap<u32, Vec<PullRequestItem>>>>,
    request_sequence: Arc<AtomicU64>,
}

pub(crate) fn bind_pr_list(ui: &MainWindow) -> WorkflowPrListController {
    let controller = WorkflowPrListController {
        context: Arc::new(Mutex::new(None)),
        visited_pages: Arc::new(Mutex::new(HashMap::new())),
        request_sequence: Arc::new(AtomicU64::new(0)),
    };

    let state_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_list_state_change(move |state| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        state_controller.clear_and_load_page(
            &ui,
            PullRequestState::parse(state.as_str()).unwrap_or(PullRequestState::Open),
            1,
        );
    });

    let page_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_list_page_change(move |page| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let state = PullRequestState::parse(ui.get_workflow_pr_list_state().as_str())
            .unwrap_or(PullRequestState::Open);
        page_controller.load_page(&ui, state, page.max(1) as u32);
    });

    controller
}

impl WorkflowPrListController {
    pub(crate) fn select_project(&self, ui: &MainWindow, project_alias: String) {
        let request = self.request_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut context) = self.context.lock() {
            *context = None;
        }
        clear_pages(&self.visited_pages);
        reset_ui(ui);
        ui.set_workflow_pr_list_loading(true);

        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let metadata = tokio::task::spawn_blocking(move || {
                load_repository_metadata(project_alias.as_str())
            })
            .await
            .map_err(|error| format!("Failed to read project configuration: {error}"))
            .and_then(|result| result);

            if controller.request_sequence.load(Ordering::SeqCst) != request {
                return;
            }

            match metadata {
                Ok(metadata) => {
                    if let Ok(mut context) = controller.context.lock() {
                        *context = Some(metadata.clone());
                    }
                    let controller_for_ui = controller.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else {
                            return;
                        };
                        if controller_for_ui.request_sequence.load(Ordering::SeqCst) != request {
                            return;
                        }
                        ui.set_workflow_pr_repo_project_id(metadata.repo_project_id.into());
                        ui.set_workflow_pr_repo_type(metadata.repo_type.into());
                        controller_for_ui.clear_and_load_page(&ui, PullRequestState::Open, 1);
                    });
                }
                Err(error) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else {
                            return;
                        };
                        ui.set_workflow_pr_list_loading(false);
                        ui.set_workflow_pr_list_error(error.into());
                    });
                }
            }
        });
    }

    fn clear_and_load_page(&self, ui: &MainWindow, state: PullRequestState, page: u32) {
        clear_pages(&self.visited_pages);
        ui.set_workflow_pr_list_items(ModelRc::new(VecModel::from(
            Vec::<WorkflowPullRequestItem>::new(),
        )));
        ui.set_workflow_pr_list_total(0);
        ui.set_workflow_pr_list_total_exact(false);
        ui.set_workflow_pr_list_page(1);
        ui.set_workflow_pr_list_page_count(1);
        self.load_page(ui, state, page);
    }

    /// Fetch exactly the page chosen by the user. The internal endpoint has no
    /// total-count contract, so a full page means "there may be a next page".
    fn load_page(&self, ui: &MainWindow, state: PullRequestState, page: u32) {
        let context = self.context.lock().ok().and_then(|guard| guard.clone());
        let Some(context) = context else {
            ui.set_workflow_pr_list_loading(false);
            ui.set_workflow_pr_list_error("Repository metadata is not available.".into());
            return;
        };

        let request = self.request_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        ui.set_workflow_pr_list_loading(true);
        ui.set_workflow_pr_list_error("".into());
        ui.set_workflow_pr_list_state(state.as_str().into());

        let request_sequence = self.request_sequence.clone();
        let visited_pages = self.visited_pages.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let result = get_pull_requests(
                context.repo_type.as_str(),
                context.repo_project_id.as_str(),
                context.remote_repo.as_str(),
                Some(state),
                page,
                PAGE_SIZE,
            )
            .await;
            let _ = slint::invoke_from_event_loop(move || {
                if request_sequence.load(Ordering::SeqCst) != request {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_workflow_pr_list_loading(false);
                match result {
                    Ok(result) => {
                        let is_final_page = result.items.len() < PAGE_SIZE as usize;
                        if let Ok(mut pages) = visited_pages.lock() {
                            pages.insert(page, result.items.clone());
                        }
                        render_remote_page(
                            &ui,
                            &visited_pages,
                            page,
                            result.items,
                            result.total,
                            result.total_exact,
                            result.page_count,
                            is_final_page,
                        );
                    }
                    Err(error) => {
                        log::warn!("[Workflow] Failed to load pull requests: {error}");
                        ui.set_workflow_pr_list_error(error.into());
                    }
                }
            });
        });
    }
}

fn render_remote_page(
    ui: &MainWindow,
    visited_pages: &Arc<Mutex<HashMap<u32, Vec<PullRequestItem>>>>,
    requested_page: u32,
    items: Vec<PullRequestItem>,
    response_total: u32,
    total_exact: bool,
    response_page_count: Option<u32>,
    is_final_page: bool,
) {
    // A full last known page can only be disproved by requesting the next page.
    // If it is empty, restore the prior cached page instead of showing a blank
    // phantom page to the user.
    if items.is_empty() && requested_page > 1 {
        let previous_page = requested_page - 1;
        let previous_items = visited_pages
            .lock()
            .ok()
            .and_then(|pages| pages.get(&previous_page).cloned())
            .unwrap_or_default();
        if !previous_items.is_empty() {
            apply_page_to_ui(ui, previous_page, previous_items, previous_page, true);
            ui.set_workflow_pr_list_total((previous_page * PAGE_SIZE) as i32);
            return;
        }
    }

    let total_lower_bound = requested_page
        .saturating_sub(1)
        .saturating_mul(PAGE_SIZE)
        .saturating_add(items.len() as u32);
    let total = if total_exact {
        response_total
    } else {
        total_lower_bound
    };
    let visible_page_count = response_page_count.unwrap_or_else(|| {
        if total_exact {
            total.div_ceil(PAGE_SIZE).max(1)
        } else if is_final_page {
            requested_page.max(1)
        } else {
            requested_page.saturating_add(1)
        }
    });
    apply_page_to_ui(
        ui,
        requested_page.min(visible_page_count).max(1),
        items,
        visible_page_count,
        total_exact || is_final_page,
    );
    ui.set_workflow_pr_list_total(total as i32);
}

fn apply_page_to_ui(
    ui: &MainWindow,
    page: u32,
    items: Vec<PullRequestItem>,
    page_count: u32,
    total_is_exact: bool,
) {
    let items = items.iter().map(to_slint_item).collect::<Vec<_>>();
    ui.set_workflow_pr_list_items(ModelRc::new(VecModel::from(items)));
    ui.set_workflow_pr_list_page(page as i32);
    ui.set_workflow_pr_list_page_count(page_count.max(1) as i32);
    ui.set_workflow_pr_list_total_exact(total_is_exact);
}

fn to_slint_item(item: &PullRequestItem) -> WorkflowPullRequestItem {
    WorkflowPullRequestItem {
        number: item.number.clone().into(),
        title: item.title.clone().into(),
        author: item.author.clone().into(),
        source_branch: item.source_branch.clone().into(),
        target_branch: item.target_branch.clone().into(),
        updated_at: format_updated_at_in_utc_plus_eight(item.updated_at.as_str()).into(),
        web_url: item.web_url.clone().into(),
    }
}

fn format_updated_at_in_utc_plus_eight(value: &str) -> String {
    let utc_plus_eight =
        FixedOffset::east_opt(8 * 60 * 60).expect("UTC+08:00 must be a valid fixed offset");
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| {
            timestamp
                .with_timezone(&utc_plus_eight)
                .format("%Y-%m-%d %H:%M:%S %:z")
                .to_string()
        })
        .unwrap_or_else(|_| value.to_string())
}

fn clear_pages(visited_pages: &Arc<Mutex<HashMap<u32, Vec<PullRequestItem>>>>) {
    if let Ok(mut pages) = visited_pages.lock() {
        pages.clear();
    }
}

fn reset_ui(ui: &MainWindow) {
    ui.set_workflow_pr_repo_project_id("".into());
    ui.set_workflow_pr_repo_type("".into());
    ui.set_workflow_pr_list_items(ModelRc::new(VecModel::from(
        Vec::<WorkflowPullRequestItem>::new(),
    )));
    ui.set_workflow_pr_list_total(0);
    ui.set_workflow_pr_list_total_exact(false);
    ui.set_workflow_pr_list_page(1);
    ui.set_workflow_pr_list_page_count(1);
    ui.set_workflow_pr_list_state("open".into());
    ui.set_workflow_pr_list_error("".into());
}

#[cfg(test)]
mod tests {
    use super::format_updated_at_in_utc_plus_eight;

    #[test]
    fn formats_provider_timestamps_in_utc_plus_eight() {
        assert_eq!(
            format_updated_at_in_utc_plus_eight("2026-07-15T05:26:44Z"),
            "2026-07-15 13:26:44 +08:00"
        );
    }

    #[test]
    fn preserves_an_unparseable_timestamp() {
        assert_eq!(
            format_updated_at_in_utc_plus_eight("unavailable"),
            "unavailable"
        );
    }
}
