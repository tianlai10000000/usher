use chrono::{DateTime, Datelike, FixedOffset, Timelike, Utc};

use crate::git::{PullRequestFilters, PullRequestItem, PullRequestState, get_pull_requests};
use crate::project::{ProjectRepositoryMetadata, load_repository_metadata};
use crate::{MainWindow, WorkflowPullRequestItem};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::pr_list_state::{PrListGenerationFacade, PrListRequestKind};

const PAGE_SIZE: u32 = 10;
const REVIEW_STATUS_FIXTURE: &str = "review-status-v1";

#[derive(Clone)]
pub(crate) struct WorkflowPrListController {
    context: Arc<Mutex<Option<ProjectRepositoryMetadata>>>,
    visited_pages: Arc<Mutex<HashMap<u32, Vec<PullRequestItem>>>>,
    filters: Arc<Mutex<PullRequestFilters>>,
    generations: PrListGenerationFacade,
    fixture_enabled: bool,
}

pub(crate) fn bind_pr_list(ui: &MainWindow) -> WorkflowPrListController {
    let controller = WorkflowPrListController {
        context: Arc::new(Mutex::new(None)),
        visited_pages: Arc::new(Mutex::new(HashMap::new())),
        filters: Arc::new(Mutex::new(default_pull_request_filters())),
        generations: PrListGenerationFacade::new(),
        fixture_enabled: review_status_fixture_enabled(),
    };

    let conflict_controller = controller.clone();
    let weak = ui.as_weak();
    ui.global::<crate::PrConflictsBridge>()
        .on_refresh_list(move || {
            if let Some(ui) = weak.upgrade()
                && let Some(metadata) = conflict_controller
                    .context
                    .lock()
                    .ok()
                    .and_then(|c| c.clone())
            {
                super::pr_conflict_status::enrich(&ui, metadata);
            }
        });
    let state_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_list_state_change(move |state| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let state = PullRequestState::parse(state.as_str()).unwrap_or(PullRequestState::Open);
        let filters = default_pull_request_filters();
        if let Ok(mut current) = state_controller.filters.lock() {
            *current = filters.clone();
        }
        ui.set_workflow_pr_list_query(format_pull_request_query(state, &filters).into());
        ui.set_workflow_pr_list_review_filter(filters.reviews.into());
        ui.set_workflow_pr_list_assignee_filter(filters.assignee.into());
        ui.set_workflow_pr_list_sort_filter(filters.sort.into());
        state_controller.clear_and_load_page(&ui, state, 1);
        state_controller.refresh_state_totals(&ui);
    });

    let query_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_list_query_change(move |query| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let current_sort = query_controller
            .filters
            .lock()
            .map(|filters| filters.sort.clone())
            .unwrap_or_else(|_| "newest".to_string());
        let (state, filters) = match parse_pull_request_query(query.as_str(), current_sort.as_str())
        {
            Ok(parsed) => parsed,
            Err(error_key) => {
                ui.set_workflow_pr_list_error(crate::i18n::tr(error_key).into_owned().into());
                return;
            }
        };
        ui.set_workflow_pr_list_query(format_pull_request_query(state, &filters).into());
        ui.set_workflow_pr_list_review_filter(filters.reviews.clone().into());
        ui.set_workflow_pr_list_assignee_filter(filters.assignee.clone().into());
        ui.set_workflow_pr_list_sort_filter(filters.sort.clone().into());
        if let Ok(mut current) = query_controller.filters.lock() {
            *current = filters;
        }
        query_controller.clear_and_load_page(&ui, state, 1);
        query_controller.refresh_state_totals(&ui);
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
            ui.set_workflow_pr_list_query(
                format_pull_request_query(
                    PullRequestState::parse(ui.get_workflow_pr_list_state().as_str())
                        .unwrap_or(PullRequestState::Open),
                    &filters,
                )
                .into(),
            );
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
            ui.set_workflow_pr_list_query(
                format_pull_request_query(
                    PullRequestState::parse(ui.get_workflow_pr_list_state().as_str())
                        .unwrap_or(PullRequestState::Open),
                    &filters,
                )
                .into(),
            );
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
            ui.set_workflow_pr_list_query(
                format_pull_request_query(
                    PullRequestState::parse(ui.get_workflow_pr_list_state().as_str())
                        .unwrap_or(PullRequestState::Open),
                    &filters,
                )
                .into(),
            );
        }
        ui.set_workflow_pr_list_sort_filter(value);
        let state = PullRequestState::parse(ui.get_workflow_pr_list_state().as_str())
            .unwrap_or(PullRequestState::Open);
        sort_controller.clear_and_load_page(&ui, state, 1);
        sort_controller.refresh_state_totals(&ui);
    });

    #[cfg(feature = "test-fixtures")]
    super::pr_list_fixture::setup(ui);

    controller
}

impl WorkflowPrListController {
    pub(crate) fn sync_language(&self, ui: &MainWindow) {
        let state = PullRequestState::parse(ui.get_workflow_pr_list_state().as_str())
            .unwrap_or(PullRequestState::Open);
        let page = ui.get_workflow_pr_list_page().max(1) as u32;
        if self.fixture_enabled {
            self.load_fixture_page(ui, state, page);
            return;
        }

        let items = self
            .visited_pages
            .lock()
            .ok()
            .and_then(|pages| pages.get(&page).cloned());
        if let Some(items) = items {
            apply_page_to_ui(
                ui,
                page,
                items,
                ui.get_workflow_pr_list_page_count().max(1) as u32,
                ui.get_workflow_pr_list_total_exact(),
            );
        }
    }

    pub(crate) fn release_unreachable(&self, ui: &MainWindow) {
        super::pr_conflict_status::invalidate(ui);
        self.generations.invalidate_all();
        if let Ok(mut context) = self.context.lock() {
            *context = None;
        }
        clear_pages(&self.visited_pages);
        reset_ui(ui);
    }

    pub(crate) fn refresh_after_remote_mutation(&self, ui: &MainWindow) {
        let state = PullRequestState::parse(ui.get_workflow_pr_list_state().as_str())
            .unwrap_or(PullRequestState::Open);
        self.clear_and_load_page(ui, state, 1);
        self.refresh_state_totals(ui);
    }

    pub(crate) fn select_project(&self, ui: &MainWindow, project_alias: String) {
        super::pr_conflict_status::invalidate(ui);
        let request = self.generations.begin(PrListRequestKind::Page);
        self.generations.invalidate(PrListRequestKind::Totals);
        if let Ok(mut context) = self.context.lock() {
            *context = None;
        }
        if let Ok(mut filters) = self.filters.lock() {
            *filters = default_pull_request_filters();
        }
        clear_pages(&self.visited_pages);
        reset_ui(ui);
        ui.set_workflow_pr_list_loading(true);

        if self.fixture_enabled {
            ui.set_workflow_pr_repo_project_id(REVIEW_STATUS_FIXTURE.into());
            ui.set_workflow_pr_repo_type("github".into());
            self.clear_and_load_page(ui, PullRequestState::Open, 1);
            self.refresh_state_totals(ui);
            return;
        }

        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let metadata = tokio::task::spawn_blocking(move || {
                load_repository_metadata(project_alias.as_str())
            })
            .await
            .map_err(|error| format!("Failed to read project configuration: {error}"))
            .and_then(|result| result);

            if !controller
                .generations
                .is_current(PrListRequestKind::Page, request)
            {
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
                        if !controller_for_ui
                            .generations
                            .is_current(PrListRequestKind::Page, request)
                        {
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
                        log::warn!("[Workflow] Failed to discover repository metadata: {error}");
                        ui.set_workflow_pr_list_error(
                            crate::i18n::tr("pr_list.metadata_unavailable")
                                .into_owned()
                                .into(),
                        );
                        ui.set_workflow_pr_list_error_kind("configuration".into());
                    });
                }
            }
        });
    }

    fn clear_and_load_page(&self, ui: &MainWindow, state: PullRequestState, page: u32) {
        super::pr_conflict_status::invalidate(ui);
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
        if self.fixture_enabled {
            self.refresh_fixture_state_totals(ui);
            return;
        }
        let context = self.context.lock().ok().and_then(|guard| guard.clone());
        let Some(context) = context else {
            return;
        };
        let filters = self
            .filters
            .lock()
            .map(|filters| filters.clone())
            .unwrap_or_default();
        let request = self.generations.begin(PrListRequestKind::Totals);
        ui.set_workflow_pr_list_open_total(-1);
        ui.set_workflow_pr_list_closed_total(-1);

        let generations = self.generations.clone();
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
                if !generations.is_current(PrListRequestKind::Totals, request) {
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
        if self.fixture_enabled {
            self.load_fixture_page(ui, state, page);
            return;
        }
        let context = self.context.lock().ok().and_then(|guard| guard.clone());
        let Some(context) = context else {
            ui.set_workflow_pr_list_loading(false);
            ui.set_workflow_pr_list_error(
                crate::i18n::tr("pr_list.metadata_unavailable")
                    .into_owned()
                    .into(),
            );
            ui.set_workflow_pr_list_error_kind("configuration".into());
            return;
        };

        let request = self.generations.begin(PrListRequestKind::Page);
        ui.set_workflow_pr_list_loading(true);
        ui.set_workflow_pr_list_error("".into());
        ui.set_workflow_pr_list_error_kind("".into());
        ui.set_workflow_pr_list_state(state.as_str().into());

        let generations = self.generations.clone();
        let visited_pages = self.visited_pages.clone();
        let filters = self
            .filters
            .lock()
            .map(|filters| filters.clone())
            .unwrap_or_default();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let conflict_identity = super::pr_conflict_status::Key::new(&context, "");
            let conflict_scope = super::pr_conflict_status::shared().scope();
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
            if generations.is_current(PrListRequestKind::Page, request)
                && conflict_identity == super::pr_conflict_status::Key::new(&context, "")
            {
                if let Ok(page) = &result {
                    for item in &page.items {
                        if item.conflict_status != crate::git::conflicts::ConflictStatus::Unknown {
                            super::pr_conflict_status::shared().observe(
                                conflict_scope,
                                super::pr_conflict_status::Key::new(&context, &item.number),
                                item.conflict_status,
                            );
                        }
                    }
                }
            }
            let _ = slint::invoke_from_event_loop(move || {
                if !generations.is_current(PrListRequestKind::Page, request) {
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
                        ui.set_workflow_pr_list_error(
                            crate::i18n::tr(error.user_message_key())
                                .into_owned()
                                .into(),
                        );
                        ui.set_workflow_pr_list_error_kind(error.error_kind().into());
                    }
                }
            });
        });
    }

    fn load_fixture_page(&self, ui: &MainWindow, state: PullRequestState, page: u32) {
        let filters = self
            .filters
            .lock()
            .map(|filters| filters.clone())
            .unwrap_or_default();
        let items = match filtered_review_status_fixture(state, &filters, Utc::now()) {
            Ok(items) => items,
            Err(error) => {
                ui.set_workflow_pr_list_loading(false);
                ui.set_workflow_pr_list_items(ModelRc::new(VecModel::from(Vec::<
                    WorkflowPullRequestItem,
                >::new(
                ))));
                ui.set_workflow_pr_list_error(error.into());
                return;
            }
        };
        let total = items.len() as u32;
        let page_count = total.div_ceil(PAGE_SIZE).max(1);
        let requested_page = page.max(1).min(page_count);
        let start = requested_page.saturating_sub(1).saturating_mul(PAGE_SIZE) as usize;
        let visible_items = items
            .into_iter()
            .skip(start)
            .take(PAGE_SIZE as usize)
            .collect::<Vec<_>>();
        ui.set_workflow_pr_list_loading(false);
        ui.set_workflow_pr_list_error("".into());
        ui.set_workflow_pr_list_state(state.as_str().into());
        apply_page_to_ui(ui, requested_page, visible_items, page_count, true);
        ui.set_workflow_pr_list_total(total as i32);
        set_state_total(ui, state, total);
    }

    fn refresh_fixture_state_totals(&self, ui: &MainWindow) {
        let filters = self
            .filters
            .lock()
            .map(|filters| filters.clone())
            .unwrap_or_default();
        let now = Utc::now();
        let open_total = filtered_review_status_fixture(PullRequestState::Open, &filters, now)
            .map(|items| items.len() as i32)
            .unwrap_or(-1);
        let closed_total = filtered_review_status_fixture(PullRequestState::Closed, &filters, now)
            .map(|items| items.len() as i32)
            .unwrap_or(-1);
        ui.set_workflow_pr_list_open_total(open_total);
        ui.set_workflow_pr_list_closed_total(closed_total);
    }
}

fn parse_pull_request_query(
    value: &str,
    _current_sort: &str,
) -> Result<(PullRequestState, PullRequestFilters), &'static str> {
    let mut state = None;
    let mut filters = PullRequestFilters {
        query: String::new(),
        reviews: "all".to_string(),
        assignee: "any".to_string(),
        sort: "newest".to_string(),
    };
    let mut query_terms = Vec::new();
    let mut unmerged = false;

    for token in value.split_whitespace() {
        let normalized = token.to_ascii_lowercase();
        let parsed_state = match normalized.as_str() {
            "is:open" | "state:open" => Some(PullRequestState::Open),
            "is:closed" | "state:closed" => Some(PullRequestState::Closed),
            "is:merged" | "state:merged" => Some(PullRequestState::Merged),
            _ => None,
        };
        if let Some(parsed_state) = parsed_state {
            if state.is_some_and(|current| current != parsed_state) {
                return Err("pr_list.single_state_qualifier");
            }
            state = Some(parsed_state);
            continue;
        }

        match normalized.as_str() {
            "is:pr" | "type:pr" => {}
            "is:unmerged" => unmerged = true,
            "review:required" => filters.reviews = "required".to_string(),
            "review:approved" => filters.reviews = "approved".to_string(),
            "review:changes_requested" | "review:changes-requested" => {
                filters.reviews = "changes-requested".to_string();
            }
            "no:assignee" => filters.assignee = "none".to_string(),
            "assignee:@me" => filters.assignee = "me".to_string(),
            "sort:created-desc" => filters.sort = "newest".to_string(),
            "sort:created-asc" => filters.sort = "oldest".to_string(),
            "sort:updated-desc" => filters.sort = "updated-desc".to_string(),
            "sort:updated-asc" => filters.sort = "updated-asc".to_string(),
            _ => query_terms.push(token),
        }
    }

    filters.query = query_terms.join(" ");
    let state = match (state, unmerged) {
        (Some(PullRequestState::Closed), true) => PullRequestState::ClosedUnmerged,
        (Some(PullRequestState::Merged), true) => {
            return Err("pr_list.merged_unmerged_conflict");
        }
        (Some(state), _) => state,
        (None, true) => {
            return Err("pr_list.unmerged_requires_state");
        }
        (None, false) => PullRequestState::Open,
    };
    Ok((state, filters))
}

fn format_pull_request_query(state: PullRequestState, filters: &PullRequestFilters) -> String {
    let mut parts = vec!["is:pr".to_string()];
    let state_query = state.github_query();
    if !state_query.is_empty() {
        parts.push(state_query.to_string());
    }
    if !filters.query.trim().is_empty() {
        parts.push(filters.query.trim().to_string());
    }
    match filters.reviews.as_str() {
        "required" => parts.push("review:required".to_string()),
        "approved" => parts.push("review:approved".to_string()),
        "changes-requested" => parts.push("review:changes_requested".to_string()),
        _ => {}
    }
    match filters.assignee.as_str() {
        "none" => parts.push("no:assignee".to_string()),
        "me" => parts.push("assignee:@me".to_string()),
        _ => {}
    }
    match filters.sort.as_str() {
        "oldest" => parts.push("sort:created-asc".to_string()),
        "updated-desc" => parts.push("sort:updated-desc".to_string()),
        "updated-asc" => parts.push("sort:updated-asc".to_string()),
        _ => {}
    }
    parts.join(" ")
}

fn default_pull_request_filters() -> PullRequestFilters {
    PullRequestFilters {
        query: String::new(),
        reviews: "all".to_string(),
        assignee: "any".to_string(),
        sort: "newest".to_string(),
    }
}

pub(super) fn review_status_fixture_enabled() -> bool {
    cfg!(feature = "test-fixtures")
        && std::env::var("USHER_PR_LIST_FIXTURE").as_deref() == Ok(REVIEW_STATUS_FIXTURE)
}

fn filtered_review_status_fixture(
    state: PullRequestState,
    filters: &PullRequestFilters,
    now: DateTime<Utc>,
) -> Result<Vec<PullRequestItem>, String> {
    #[cfg(feature = "test-fixtures")]
    {
        return super::pr_list_fixture::filtered_review_status_fixture(state, filters, now);
    }
    #[cfg(not(feature = "test-fixtures"))]
    {
        let _ = (state, filters, now);
        Err("PR list fixtures are not compiled into this build.".to_string())
    }
}

// Arguments correspond to the remote-page response and its pagination metadata.
#[allow(clippy::too_many_arguments)]
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
        PullRequestState::Closed | PullRequestState::ClosedUnmerged | PullRequestState::Merged => {
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
    ui.global::<crate::PrConflictsBridge>()
        .invoke_refresh_list();
    ui.invoke_ai_review_refresh_pr_rows();
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
        review_status: item.review_status.as_str().into(),
        conflict_status: if matches!(item.state.as_str(), "closed" | "merged") {
            "not_applicable"
        } else {
            item.conflict_status.as_str()
        }
        .into(),
        conversation_count: item
            .conversation_count
            .and_then(|count| i32::try_from(count).ok())
            .unwrap_or(-1),
        updated_at: format_github_relative_time(item.updated_at.as_str()).into(),
        updated_at_raw: item.updated_at.clone().into(),
        updated_at_tooltip: format_github_absolute_tooltip(item.updated_at.as_str()).into(),
        web_url: item.web_url.clone().into(),
        ai_review_status: "".into(),
        ai_review_task_id: "".into(),
        ai_review_finding_count: 0,
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
    format_github_absolute_tooltip_for_locale(value, usher_i18n::active_locale().id)
}

fn format_github_absolute_tooltip_for_locale(value: &str, locale: &str) -> String {
    if value.trim().is_empty() {
        return usher_i18n::translate_for(locale, "common.unavailable").into_owned();
    }
    match parse_provider_timestamp(value) {
        Some(timestamp) => {
            let utc_plus_eight =
                FixedOffset::east_opt(8 * 60 * 60).expect("UTC+08:00 must be a valid fixed offset");
            let local = timestamp.with_timezone(&utc_plus_eight);
            let hour24 = local.hour();
            let (hour12, meridiem_key) = match hour24 {
                0 => (12, "time.am"),
                1..=11 => (hour24, "time.am"),
                12 => (12, "time.pm"),
                _ => (hour24 - 12, "time.pm"),
            };
            let month = localized_short_month(locale, local.month());
            let day = local.day().to_string();
            let year = local.year().to_string();
            let hour = hour12.to_string();
            let minute = format!("{:02}", local.minute());
            let meridiem = usher_i18n::translate_for(locale, meridiem_key).into_owned();
            usher_i18n::translate_for_with(
                locale,
                "time.absolute_tooltip",
                &[
                    ("month", month.as_str()),
                    ("day", day.as_str()),
                    ("year", year.as_str()),
                    ("hour", hour.as_str()),
                    ("minute", minute.as_str()),
                    ("meridiem", meridiem.as_str()),
                ],
            )
        }
        None => value.to_string(),
    }
}

fn format_github_absolute_short_at(
    value: &str,
    now: chrono::DateTime<chrono::Utc>,
    locale: &str,
) -> Option<String> {
    let timestamp = parse_provider_timestamp(value)?;
    let utc_plus_eight =
        FixedOffset::east_opt(8 * 60 * 60).expect("UTC+08:00 must be a valid fixed offset");
    let local = timestamp.with_timezone(&utc_plus_eight);
    let now_local = now.with_timezone(&utc_plus_eight);
    let month = localized_short_month(locale, local.month());
    let day = local.day().to_string();
    // Primer defaults: month=short, day=numeric, year only when different.
    if local.year() == now_local.year() {
        Some(usher_i18n::translate_for_with(
            locale,
            "time.on_month_day",
            &[("month", month.as_str()), ("day", day.as_str())],
        ))
    } else {
        let year = local.year().to_string();
        Some(usher_i18n::translate_for_with(
            locale,
            "time.on_month_day_year",
            &[
                ("month", month.as_str()),
                ("day", day.as_str()),
                ("year", year.as_str()),
            ],
        ))
    }
}

fn format_github_relative_time_at(value: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    format_github_relative_time_at_for_locale(value, now, usher_i18n::active_locale().id)
}

fn format_github_relative_time_at_for_locale(
    value: &str,
    now: chrono::DateTime<chrono::Utc>,
    locale: &str,
) -> String {
    if value.trim().is_empty() {
        return usher_i18n::translate_for(locale, "common.unavailable").into_owned();
    }
    let Some(timestamp) = parse_provider_timestamp(value) else {
        return value.to_string();
    };
    let instant = timestamp.with_timezone(&chrono::Utc);
    let delta = now.signed_duration_since(instant);
    let seconds = delta.num_seconds();
    if seconds < 0 {
        return format_github_absolute_tooltip_for_locale(value, locale);
    }
    // Past P30D: absolute `on Mar 19` instead of `N months ago`.
    let days = delta.num_days();
    if days >= 30 {
        return format_github_absolute_short_at(value, now, locale)
            .unwrap_or_else(|| value.to_string());
    }
    if seconds < 60 {
        return usher_i18n::translate_for(locale, "time.just_now").into_owned();
    }
    let minutes = delta.num_minutes();
    if minutes < 60 {
        return if minutes == 1 {
            usher_i18n::translate_for(locale, "time.minute_ago").into_owned()
        } else {
            let count = minutes.to_string();
            usher_i18n::translate_for_with(locale, "time.minutes_ago", &[("count", count.as_str())])
        };
    }
    let hours = delta.num_hours();
    if hours < 24 {
        return if hours == 1 {
            usher_i18n::translate_for(locale, "time.hour_ago").into_owned()
        } else {
            let count = hours.to_string();
            usher_i18n::translate_for_with(locale, "time.hours_ago", &[("count", count.as_str())])
        };
    }
    if days == 1 {
        return usher_i18n::translate_for(locale, "time.yesterday").into_owned();
    }
    if days < 7 {
        let count = days.to_string();
        return usher_i18n::translate_for_with(
            locale,
            "time.days_ago",
            &[("count", count.as_str())],
        );
    }
    let weeks = days / 7;
    if weeks == 1 {
        usher_i18n::translate_for(locale, "time.last_week").into_owned()
    } else {
        let count = weeks.to_string();
        usher_i18n::translate_for_with(locale, "time.weeks_ago", &[("count", count.as_str())])
    }
}

pub(super) fn format_github_calendar_date(value: &str) -> String {
    let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(value) else {
        return value.split('T').next().unwrap_or(value).to_string();
    };
    let utc_plus_eight =
        FixedOffset::east_opt(8 * 60 * 60).expect("UTC+08:00 must be a valid fixed offset");
    let local = timestamp.with_timezone(&utc_plus_eight);
    let locale = usher_i18n::active_locale().id;
    let month = localized_short_month(locale, local.month());
    let day = local.day().to_string();
    let year = local.year().to_string();
    usher_i18n::translate_for_with(
        locale,
        "time.month_day_year",
        &[
            ("month", month.as_str()),
            ("day", day.as_str()),
            ("year", year.as_str()),
        ],
    )
}

fn localized_short_month(locale: &str, month: u32) -> String {
    let key = match month {
        1 => "time.month.jan",
        2 => "time.month.feb",
        3 => "time.month.mar",
        4 => "time.month.apr",
        5 => "time.month.may",
        6 => "time.month.jun",
        7 => "time.month.jul",
        8 => "time.month.aug",
        9 => "time.month.sep",
        10 => "time.month.oct",
        11 => "time.month.nov",
        12 => "time.month.dec",
        _ => return month.to_string(),
    };
    usher_i18n::translate_for(locale, key).into_owned()
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
    ui.set_workflow_pr_list_query("is:pr is:open".into());
    ui.set_workflow_pr_list_review_filter("all".into());
    ui.set_workflow_pr_list_assignee_filter("any".into());
    ui.set_workflow_pr_list_sort_filter("newest".into());
    ui.set_workflow_pr_list_error("".into());
    ui.set_workflow_pr_list_error_kind("".into());
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::{
        format_github_absolute_tooltip_for_locale, format_github_relative_time_at_for_locale,
        format_pull_request_query, format_updated_at_in_utc_plus_eight, parse_pull_request_query,
    };
    use crate::git::PullRequestState;

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
            format_github_relative_time_at_for_locale("2026-07-21T05:59:20Z", now, "en"),
            "just now"
        );
        assert_eq!(
            format_github_relative_time_at_for_locale("2026-07-21T03:00:00Z", now, "en"),
            "3 hours ago"
        );
        assert_eq!(
            format_github_relative_time_at_for_locale("2026-07-20T06:00:00Z", now, "en"),
            "yesterday"
        );
        assert_eq!(
            format_github_relative_time_at_for_locale("2026-07-15 13:26:44 +08:00", now, "en",),
            "6 days ago"
        );
        // Inside P30D: still relative weeks, never months-ago.
        assert_eq!(
            format_github_relative_time_at_for_locale("2026-06-25T06:00:00Z", now, "en"),
            "3 weeks ago"
        );
        // Past P30D: Primer absolute short date with `on` prefix.
        assert_eq!(
            format_github_relative_time_at_for_locale("2026-03-15T05:26:44Z", now, "en"),
            "on Mar 15"
        );
        assert_eq!(
            format_github_relative_time_at_for_locale("2025-03-15T05:26:44Z", now, "en"),
            "on Mar 15, 2025"
        );
        assert_eq!(
            format_github_absolute_tooltip_for_locale("2026-07-15T05:26:44Z", "en"),
            "Jul 15, 2026, 1:26 PM GMT+8"
        );
        assert_eq!(
            format_github_relative_time_at_for_locale("2026-07-21T03:00:00Z", now, "zh-CN"),
            "3 小时前"
        );
        assert_eq!(
            format_github_relative_time_at_for_locale("2026-03-15T05:26:44Z", now, "zh-CN"),
            "3月15日"
        );
        assert_eq!(
            format_github_absolute_tooltip_for_locale("2026-07-15T05:26:44Z", "zh-CN"),
            "2026年7月15日 下午1:26 GMT+8"
        );
    }

    #[test]
    fn parses_and_formats_github_style_merged_queries() {
        let (state, filters) = parse_pull_request_query(
            "toolbar parity is:pr is:merged review:approved assignee:@me sort:updated-desc",
            "updated-desc",
        )
        .unwrap();

        assert_eq!(state, PullRequestState::Merged);
        assert_eq!(filters.query, "toolbar parity");
        assert_eq!(filters.reviews, "approved");
        assert_eq!(filters.assignee, "me");
        assert_eq!(filters.sort, "updated-desc");
        assert_eq!(
            format_pull_request_query(state, &filters),
            "is:pr is:merged toolbar parity review:approved assignee:@me sort:updated-desc"
        );
    }

    #[test]
    fn rejects_conflicting_pull_request_state_qualifiers() {
        let error = parse_pull_request_query("is:pr is:open is:merged", "newest").unwrap_err();
        assert_eq!(error, "pr_list.single_state_qualifier");

        let error = parse_pull_request_query("is:pr is:merged is:unmerged", "newest").unwrap_err();
        assert_eq!(error, "pr_list.merged_unmerged_conflict");

        let error = parse_pull_request_query("is:pr is:unmerged", "newest").unwrap_err();
        assert_eq!(error, "pr_list.unmerged_requires_state");
    }

    #[test]
    fn distinguishes_closed_without_merge_from_the_closed_union() {
        let (state, filters) =
            parse_pull_request_query("is:pr is:closed is:unmerged", "newest").unwrap();

        assert_eq!(state, PullRequestState::ClosedUnmerged);
        assert!(filters.query.is_empty());
        assert_eq!(
            format_pull_request_query(state, &filters),
            "is:pr is:closed is:unmerged"
        );
    }

    #[test]
    fn parses_github_sort_qualifiers_and_resets_missing_sort_to_newest() {
        let (_, oldest) =
            parse_pull_request_query("is:pr is:open sort:created-asc", "updated-desc").unwrap();
        assert_eq!(oldest.sort, "oldest");
        assert_eq!(
            format_pull_request_query(PullRequestState::Open, &oldest),
            "is:pr is:open sort:created-asc"
        );

        let (_, cleared) = parse_pull_request_query("is:pr is:open", "updated-desc").unwrap();
        assert_eq!(cleared.sort, "newest");
        assert_eq!(
            format_pull_request_query(PullRequestState::Open, &cleared),
            "is:pr is:open"
        );
    }
}
