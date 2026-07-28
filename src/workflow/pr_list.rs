use chrono::{DateTime, Datelike, FixedOffset, Timelike};

use crate::git::{PullRequestFilters, PullRequestItem, PullRequestState, get_pull_requests};
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
    filters: Arc<Mutex<PullRequestFilters>>,
    request_sequence: Arc<AtomicU64>,
    count_sequence: Arc<AtomicU64>,
}

pub(crate) fn bind_pr_list(ui: &MainWindow) -> WorkflowPrListController {
    let controller = WorkflowPrListController {
        context: Arc::new(Mutex::new(None)),
        visited_pages: Arc::new(Mutex::new(HashMap::new())),
        filters: Arc::new(Mutex::new(PullRequestFilters {
            reviews: "all".to_string(),
            assignee: "any".to_string(),
            sort: "newest".to_string(),
        })),
        request_sequence: Arc::new(AtomicU64::new(0)),
        count_sequence: Arc::new(AtomicU64::new(0)),
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

    let review_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_list_review_filter_change(move |value| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        if let Ok(mut filters) = review_controller.filters.lock() {
            filters.reviews = value.to_string();
        }
        ui.set_workflow_pr_list_review_filter(value);
        let state = PullRequestState::parse(ui.get_workflow_pr_list_state().as_str())
            .unwrap_or(PullRequestState::Open);
        review_controller.clear_and_load_page(&ui, state, 1);
        review_controller.refresh_state_totals(&ui);
    });

    let assignee_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_list_assignee_filter_change(move |value| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        if let Ok(mut filters) = assignee_controller.filters.lock() {
            filters.assignee = value.to_string();
        }
        ui.set_workflow_pr_list_assignee_filter(value);
        let state = PullRequestState::parse(ui.get_workflow_pr_list_state().as_str())
            .unwrap_or(PullRequestState::Open);
        assignee_controller.clear_and_load_page(&ui, state, 1);
        assignee_controller.refresh_state_totals(&ui);
    });

    let sort_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_list_sort_filter_change(move |value| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        if let Ok(mut filters) = sort_controller.filters.lock() {
            filters.sort = value.to_string();
        }
        ui.set_workflow_pr_list_sort_filter(value);
        let state = PullRequestState::parse(ui.get_workflow_pr_list_state().as_str())
            .unwrap_or(PullRequestState::Open);
        sort_controller.clear_and_load_page(&ui, state, 1);
        sort_controller.refresh_state_totals(&ui);
    });

    controller
}

impl WorkflowPrListController {
    pub(crate) fn select_project(&self, ui: &MainWindow, project_alias: String) {
        let request = self.request_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        self.count_sequence.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut context) = self.context.lock() {
            *context = None;
        }
        if let Ok(mut filters) = self.filters.lock() {
            *filters = PullRequestFilters {
                reviews: "all".to_string(),
                assignee: "any".to_string(),
                sort: "newest".to_string(),
            };
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
                        controller_for_ui.refresh_state_totals(&ui);
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

    fn refresh_state_totals(&self, ui: &MainWindow) {
        let context = self.context.lock().ok().and_then(|guard| guard.clone());
        let Some(context) = context else {
            return;
        };
        let filters = self
            .filters
            .lock()
            .map(|filters| filters.clone())
            .unwrap_or_default();
        let request = self.count_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        ui.set_workflow_pr_list_open_total(-1);
        ui.set_workflow_pr_list_closed_total(-1);

        let count_sequence = self.count_sequence.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let open_request = get_pull_requests(
                context.repo_type.as_str(),
                context.repo_project_id.as_str(),
                context.remote_repo.as_str(),
                Some(PullRequestState::Open),
                &filters,
                1,
                1,
            );
            let closed_request = get_pull_requests(
                context.repo_type.as_str(),
                context.repo_project_id.as_str(),
                context.remote_repo.as_str(),
                Some(PullRequestState::Closed),
                &filters,
                1,
                1,
            );
            let (open_result, closed_result) = tokio::join!(open_request, closed_request);
            let _ = slint::invoke_from_event_loop(move || {
                if count_sequence.load(Ordering::SeqCst) != request {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                match open_result {
                    Ok(page) => ui.set_workflow_pr_list_open_total(page.total as i32),
                    Err(error) => {
                        // Primer: hide unavailable counts; keep sentinel -1 so
                        // the label renders without a placeholder number.
                        log::warn!("[Workflow] Failed to load open pull request count: {error}");
                        ui.set_workflow_pr_list_open_total(-1);
                    }
                }
                match closed_result {
                    Ok(page) => ui.set_workflow_pr_list_closed_total(page.total as i32),
                    Err(error) => {
                        log::warn!("[Workflow] Failed to load closed pull request count: {error}");
                        ui.set_workflow_pr_list_closed_total(-1);
                    }
                }
            });
        });
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
        let filters = self
            .filters
            .lock()
            .map(|filters| filters.clone())
            .unwrap_or_default();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let result = get_pull_requests(
                context.repo_type.as_str(),
                context.repo_project_id.as_str(),
                context.remote_repo.as_str(),
                Some(state),
                &filters,
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
                            state,
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
    state: PullRequestState,
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
            let total = previous_page * PAGE_SIZE;
            ui.set_workflow_pr_list_total(total as i32);
            set_state_total(ui, state, total);
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
    set_state_total(ui, state, total);
}

fn set_state_total(ui: &MainWindow, state: PullRequestState, total: u32) {
    match state {
        PullRequestState::Open => ui.set_workflow_pr_list_open_total(total as i32),
        PullRequestState::Closed | PullRequestState::Merged => {
            ui.set_workflow_pr_list_closed_total(total as i32);
        }
        PullRequestState::All => {}
    }
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
        state: item.state.clone().into(),
        author: item.author.clone().into(),
        source_branch: item.source_branch.clone().into(),
        target_branch: item.target_branch.clone().into(),
        updated_at: format_github_relative_time(item.updated_at.as_str()).into(),
        updated_at_raw: item.updated_at.clone().into(),
        updated_at_tooltip: format_github_absolute_tooltip(item.updated_at.as_str()).into(),
        web_url: item.web_url.clone().into(),
    }
}

#[cfg(test)]
pub(super) fn format_updated_at_in_utc_plus_eight(value: &str) -> String {
    match parse_provider_timestamp(value) {
        Some(timestamp) => {
            let utc_plus_eight =
                FixedOffset::east_opt(8 * 60 * 60).expect("UTC+08:00 must be a valid fixed offset");
            timestamp
                .with_timezone(&utc_plus_eight)
                .format("%Y-%m-%d %H:%M:%S %:z")
                .to_string()
        }
        None => value.to_string(),
    }
}

/// GitHub-style relative label shown in the PR review workspace.
///
/// Mirrors Primer `<relative-time>` defaults (`format=relative`,
/// `threshold=P30D`, `prefix=on`): within 30 days use relative phrases
/// (`just now`, `3 hours ago`, `yesterday`, `last week`); past that switch to
/// absolute short dates (`on Jul 15`, `on Jul 15, 2025`) in UTC+08:00.
pub(super) fn format_github_relative_time(value: &str) -> String {
    format_github_relative_time_at(value, chrono::Utc::now())
}

pub(super) fn format_github_absolute_tooltip(value: &str) -> String {
    match parse_provider_timestamp(value) {
        Some(timestamp) => {
            let utc_plus_eight =
                FixedOffset::east_opt(8 * 60 * 60).expect("UTC+08:00 must be a valid fixed offset");
            let local = timestamp.with_timezone(&utc_plus_eight);
            let hour24 = local.hour();
            let (hour12, meridiem) = match hour24 {
                0 => (12, "AM"),
                1..=11 => (hour24, "AM"),
                12 => (12, "PM"),
                _ => (hour24 - 12, "PM"),
            };
            format!(
                "{} {}, {}, {}:{:02} {} GMT+8",
                local.format("%b"),
                local.day(),
                local.year(),
                hour12,
                local.minute(),
                meridiem
            )
        }
        None => value.to_string(),
    }
}

fn format_github_absolute_short_at(
    value: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<String> {
    let timestamp = parse_provider_timestamp(value)?;
    let utc_plus_eight =
        FixedOffset::east_opt(8 * 60 * 60).expect("UTC+08:00 must be a valid fixed offset");
    let local = timestamp.with_timezone(&utc_plus_eight);
    let now_local = now.with_timezone(&utc_plus_eight);
    // Primer defaults: month=short, day=numeric, year only when different.
    if local.year() == now_local.year() {
        Some(format!("on {} {}", local.format("%b"), local.day()))
    } else {
        Some(format!(
            "on {} {}, {}",
            local.format("%b"),
            local.day(),
            local.year()
        ))
    }
}

fn format_github_relative_time_at(value: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    let Some(timestamp) = parse_provider_timestamp(value) else {
        return value.to_string();
    };
    let instant = timestamp.with_timezone(&chrono::Utc);
    let delta = now.signed_duration_since(instant);
    let seconds = delta.num_seconds();
    if seconds < 0 {
        return format_github_absolute_tooltip(value);
    }
    // Past P30D: absolute `on Mar 19` instead of `N months ago`.
    let days = delta.num_days();
    if days >= 30 {
        return format_github_absolute_short_at(value, now).unwrap_or_else(|| value.to_string());
    }
    if seconds < 60 {
        return "just now".to_string();
    }
    let minutes = delta.num_minutes();
    if minutes < 60 {
        return if minutes == 1 {
            "1 minute ago".to_string()
        } else {
            format!("{minutes} minutes ago")
        };
    }
    let hours = delta.num_hours();
    if hours < 24 {
        return if hours == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{hours} hours ago")
        };
    }
    if days == 1 {
        return "yesterday".to_string();
    }
    if days < 7 {
        return format!("{days} days ago");
    }
    let weeks = days / 7;
    if weeks == 1 {
        "last week".to_string()
    } else {
        format!("{weeks} weeks ago")
    }
}

fn parse_provider_timestamp(value: &str) -> Option<DateTime<FixedOffset>> {
    let trimmed = value.trim();
    DateTime::parse_from_rfc3339(trimmed)
        .ok()
        .or_else(|| DateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S %:z").ok())
        .or_else(|| DateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S %z").ok())
        .or_else(|| {
            // Internal Gitee enterprise diffs sometimes omit the offset.
            chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S")
                .ok()
                .and_then(|naive| {
                    FixedOffset::east_opt(8 * 60 * 60)
                        .and_then(|offset| naive.and_local_timezone(offset).single())
                })
        })
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
    ui.set_workflow_pr_list_open_total(-1);
    ui.set_workflow_pr_list_closed_total(-1);
    ui.set_workflow_pr_list_total_exact(false);
    ui.set_workflow_pr_list_page(1);
    ui.set_workflow_pr_list_page_count(1);
    ui.set_workflow_pr_list_state("open".into());
    ui.set_workflow_pr_list_review_filter("all".into());
    ui.set_workflow_pr_list_assignee_filter("any".into());
    ui.set_workflow_pr_list_sort_filter("newest".into());
    ui.set_workflow_pr_list_error("".into());
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::{
        format_github_absolute_tooltip, format_github_relative_time_at,
        format_updated_at_in_utc_plus_eight,
    };

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

    #[test]
    fn formats_github_style_relative_and_tooltip_times() {
        let now = Utc.with_ymd_and_hms(2026, 7, 21, 6, 0, 0).unwrap();
        assert_eq!(
            format_github_relative_time_at("2026-07-21T05:59:20Z", now),
            "just now"
        );
        assert_eq!(
            format_github_relative_time_at("2026-07-21T03:00:00Z", now),
            "3 hours ago"
        );
        assert_eq!(
            format_github_relative_time_at("2026-07-20T06:00:00Z", now),
            "yesterday"
        );
        assert_eq!(
            format_github_relative_time_at("2026-07-15 13:26:44 +08:00", now),
            "6 days ago"
        );
        // Inside P30D: still relative weeks, never months-ago.
        assert_eq!(
            format_github_relative_time_at("2026-06-25T06:00:00Z", now),
            "3 weeks ago"
        );
        // Past P30D: Primer absolute short date with `on` prefix.
        assert_eq!(
            format_github_relative_time_at("2026-03-15T05:26:44Z", now),
            "on Mar 15"
        );
        assert_eq!(
            format_github_relative_time_at("2025-03-15T05:26:44Z", now),
            "on Mar 15, 2025"
        );
        assert_eq!(
            format_github_absolute_tooltip("2026-07-15T05:26:44Z"),
            "Jul 15, 2026, 1:26 PM GMT+8"
        );
    }
}
