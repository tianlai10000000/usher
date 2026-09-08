use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(feature = "test-fixtures")]
use std::time::Duration;

use slint::{ComponentHandle, Model, ModelRc, VecModel};

#[cfg(any(test, feature = "test-fixtures"))]
use crate::git::PullRequestDiffLine;
use crate::git::{
    GitError, PullRequestActionState, PullRequestCommit, PullRequestConversationItem,
    PullRequestDiffDetail, PullRequestDiffFile, get_pull_request_commit_range_diff,
    get_pull_request_commits, get_pull_request_conversation, get_pull_request_diff,
    get_repository_file_text, provider_capabilities,
};
use crate::project::{ProjectRepositoryMetadata, load_repository_metadata};
use crate::{
    MainWindow, Route, WorkflowPrDiffFile, WorkflowPrDiffLine, WorkflowPrDiffRowMetric,
    WorkflowPrFileExtensionFilter, WorkflowPrTreeItem,
};

use super::super::bindings::show_error;
use super::super::pr_list::WorkflowPrListController;
use super::super::syntax_highlight::{HighlightResources, HighlightedLine, highlight_diff_lines};

use super::avatar::{load_commit_avatars, load_conversation_avatars};
use super::cache::{
    BoundedCache, HIGHLIGHT_CACHE_CAPACITY, RANGE_DIFF_CACHE_CAPACITY, ReviewCache,
    SOURCE_CACHE_CAPACITY, SharedDiffDetail, SharedDiffFile, diff_override_storage_stats,
    diff_storage_stats, highlighted_cache_stats, owned_diff_storage_stats, source_cache_stats,
};
use super::diff_model::{LazyPrDiffLineModel, to_slint_file};
use super::diff_projection::{
    DiffLineSide, display_row_for_location, hunk_scroll_anchor_source_index,
};
use super::file_tree::{build_tree_items, extension_counts};
use super::hunk::{
    HUNK_CONTEXT_PAGE_SIZE, HunkExpansionDirection, expand_all_hunk_context_from_sources,
    expand_hunk_context_around_line_from_sources, expand_hunk_context_from_sources,
    full_source_file, hidden_hunk_for_line, hunk_is_expandable,
};
#[cfg(test)]
use super::hunk::{
    expand_all_hunk_context, expand_hunk_context, hunk_can_expand_all, hunk_can_expand_down,
    restore_original_diff,
};
use super::state::{ReviewRequestKind, ReviewStateFacade};
use super::timeline::{
    chrono_commit_index_to_display, commit_index_by_sha, commit_model, commit_range_label,
    conversation_content_height, conversation_model, display_commit_range_to_chrono,
    empty_commit_model, empty_conversation_model, patch_commit_avatars, patch_conversation_avatars,
    range_covers_all_commits, refresh_commit_group_expand_extras,
};

const LARGE_DIFF_LINE_THRESHOLD: usize = 400;
const LARGE_DIFF_BYTE_THRESHOLD: usize = 64 * 1024;
mod actions;
mod diff_bindings;
use actions::{apply_action_state, readiness_label_key, reset_action_ui};
mod diagnostics;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReviewRequest {
    number: String,
    title: String,
    author: String,
    source_branch: String,
    target_branch: String,
    updated_at: String,
    ignore_whitespace: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocationReveal {
    generation: u64,
    file_index: usize,
    path: String,
    line: i32,
    side: DiffLineSide,
    expanding_context: bool,
}

#[derive(Clone)]
pub(crate) struct WorkflowPrReviewController {
    pr_list_controller: WorkflowPrListController,
    project_alias: Arc<Mutex<String>>,
    active_request: Arc<Mutex<Option<ReviewRequest>>>,
    action_state: Arc<Mutex<Option<PullRequestActionState>>>,
    cache: Arc<Mutex<Option<ReviewCache>>>,
    file_filter: Arc<Mutex<String>>,
    selected_extensions: Arc<Mutex<HashSet<String>>>,
    only_manifest_files: Arc<Mutex<bool>>,
    show_viewed_files: Arc<Mutex<bool>>,
    collapsed_folders: Arc<Mutex<HashSet<String>>>,
    highlight_resources: Arc<Mutex<Option<Arc<HighlightResources>>>>,
    highlighted_files: Arc<Mutex<BoundedCache<Arc<Vec<HighlightedLine>>>>>,
    source_files: Arc<Mutex<BoundedCache<String>>>,
    location_reveal: Arc<Mutex<Option<LocationReveal>>>,
    state: ReviewStateFacade,
    conversation_loaded: Arc<Mutex<bool>>,
    conversation: Arc<Mutex<Vec<PullRequestConversationItem>>>,
    commits_loaded: Arc<Mutex<bool>>,
    commits: Arc<Mutex<Vec<PullRequestCommit>>>,
    pending_open_commit: Arc<Mutex<Option<String>>>,
    commit_detail_return_tab: Arc<Mutex<Option<i32>>>,
    pr_list_refresh_pending: Arc<AtomicBool>,
}

fn session_highlight_resources(
    resources: &Mutex<Option<Arc<HighlightResources>>>,
) -> Arc<HighlightResources> {
    let mut resources = resources
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    resources
        .get_or_insert_with(|| Arc::new(HighlightResources::load()))
        .clone()
}

pub(crate) fn bind_pr_review(
    ui: &MainWindow,
    pr_list_controller: WorkflowPrListController,
) -> WorkflowPrReviewController {
    super::find_benchmark::setup(ui);
    let controller = WorkflowPrReviewController {
        pr_list_controller,
        project_alias: Arc::new(Mutex::new(String::new())),
        active_request: Arc::new(Mutex::new(None)),
        action_state: Arc::new(Mutex::new(None)),
        cache: Arc::new(Mutex::new(None)),
        file_filter: Arc::new(Mutex::new(String::new())),
        selected_extensions: Arc::new(Mutex::new(HashSet::new())),
        only_manifest_files: Arc::new(Mutex::new(false)),
        show_viewed_files: Arc::new(Mutex::new(true)),
        collapsed_folders: Arc::new(Mutex::new(HashSet::new())),
        highlight_resources: Arc::new(Mutex::new(None)),
        highlighted_files: Arc::new(Mutex::new(BoundedCache::new(HIGHLIGHT_CACHE_CAPACITY))),
        source_files: Arc::new(Mutex::new(BoundedCache::new(SOURCE_CACHE_CAPACITY))),
        location_reveal: Arc::new(Mutex::new(None)),
        state: ReviewStateFacade::new(),
        conversation_loaded: Arc::new(Mutex::new(false)),
        conversation: Arc::new(Mutex::new(Vec::new())),
        commits_loaded: Arc::new(Mutex::new(false)),
        commits: Arc::new(Mutex::new(Vec::new())),
        pending_open_commit: Arc::new(Mutex::new(None)),
        commit_detail_return_tab: Arc::new(Mutex::new(None)),
        pr_list_refresh_pending: Arc::new(AtomicBool::new(false)),
    };

    diff_bindings::bind(ui);

    let open_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_open(
        move |number, title, author, source_branch, target_branch, updated_at, initial_tab| {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            open_controller.open(
                &ui,
                ReviewRequest {
                    number: number.to_string(),
                    title: title.to_string(),
                    author: author.to_string(),
                    source_branch: source_branch.to_string(),
                    target_branch: target_branch.to_string(),
                    updated_at: updated_at.to_string(),
                    ignore_whitespace: false,
                },
                false,
            );
            if initial_tab != 2 {
                ui.invoke_workflow_pr_review_select_tab(initial_tab);
            }
        },
    );

    let back_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_back(move || {
        if let Some(ui) = ui_weak.upgrade() {
            back_controller.navigate_back(&ui);
        }
    });

    let tab_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_select_tab(move |tab| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let tab = tab.clamp(0, 2);
        ui.set_workflow_pr_review_tab(tab);
        if tab != 2
            && let Ok(mut return_tab) = tab_controller.commit_detail_return_tab.lock()
        {
            *return_tab = None;
        }
        tab_controller.load_tab(&ui, tab);
    });

    let commits_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_ensure_commits(move || {
        if let Some(ui) = ui_weak.upgrade() {
            commits_controller.load_commits(&ui);
        }
    });

    let range_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_apply_commit_range(move |start, end| {
        if let Some(ui) = ui_weak.upgrade() {
            range_controller.apply_commit_range(&ui, start, end);
        }
    });

    let open_commit_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_open_commit(move |sha| {
        if let Some(ui) = ui_weak.upgrade() {
            open_commit_controller.open_commit(&ui, sha.as_str());
        }
    });

    let select_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_select_file(move |index| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        select_controller.select_file(&ui, index);
    });

    let relative_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_select_relative_file(move |offset| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let selected = ui.get_workflow_pr_review_selected_file_index();
        relative_controller.select_file(&ui, selected.saturating_add(offset));
    });

    let reveal_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_reveal_location(move |path, line, side, head_sha| {
        if let Some(ui) = ui_weak.upgrade() {
            reveal_controller.reveal_location(
                &ui,
                path.to_string(),
                line,
                DiffLineSide::parse(side.as_str()),
                head_sha.as_str(),
            );
        }
    });

    let view_location_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_view_location_file(move |path, side| {
        if let Some(ui) = ui_weak.upgrade() {
            view_location_controller.view_location_file(
                &ui,
                path.as_str(),
                DiffLineSide::parse(side.as_str()),
            );
        }
    });

    let clear_location_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_clear_location(move || {
        if let Some(ui) = ui_weak.upgrade() {
            clear_location_controller.cancel_location_reveal(&ui);
        }
    });

    let reload_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_reload(move |ignore_whitespace| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let request = reload_controller
            .active_request
            .lock()
            .ok()
            .and_then(|guard| guard.clone());
        if let Some(mut request) = request {
            request.ignore_whitespace = ignore_whitespace;
            ui.set_workflow_pr_review_commit_range_start(-1);
            ui.set_workflow_pr_review_commit_range_end(-1);
            ui.set_workflow_pr_review_commit_range_label(
                crate::i18n::tr("pr_review.all_commits").into_owned().into(),
            );
            reload_controller.open(&ui, request, true);
        }
    });

    let diff_settings_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_apply_diff_settings(move |diff_view, ignore_whitespace| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let diff_view = if diff_view.as_str() == "split" {
            "split"
        } else {
            "unified"
        };
        let ignore_whitespace =
            ui.get_workflow_pr_review_ignore_whitespace_supported() && ignore_whitespace;
        let whitespace_changed = ui.get_workflow_pr_review_ignore_whitespace() != ignore_whitespace;

        ui.set_workflow_pr_review_diff_view(diff_view.into());
        ui.set_workflow_pr_review_ignore_whitespace(ignore_whitespace);

        if whitespace_changed {
            let request = diff_settings_controller
                .active_request
                .lock()
                .ok()
                .and_then(|guard| guard.clone());
            if let Some(mut request) = request {
                request.ignore_whitespace = ignore_whitespace;
                ui.set_workflow_pr_review_commit_range_start(-1);
                ui.set_workflow_pr_review_commit_range_end(-1);
                ui.set_workflow_pr_review_commit_range_label(
                    crate::i18n::tr("pr_review.all_commits").into_owned().into(),
                );
                diff_settings_controller.open(&ui, request, true);
            }
        } else {
            let selected = ui.get_workflow_pr_review_selected_file_index();
            if selected >= 0 {
                diff_settings_controller.select_file(&ui, selected);
            }
        }
    });

    let load_large_diff_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_load_large_diff(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let selected = ui.get_workflow_pr_review_selected_file_index();
        if selected < 0 {
            return;
        }
        let marked = load_large_diff_controller
            .cache
            .lock()
            .ok()
            .and_then(|mut cache| {
                let cache = cache.as_mut()?;
                is_large_diff(cache.file(selected as usize)?)
                    .then(|| cache.loaded_large_diffs.insert(selected as usize))
            })
            .unwrap_or(false);
        if marked {
            load_large_diff_controller.select_file(&ui, selected);
            ui.set_workflow_pr_review_large_diff_loading(ui.get_workflow_pr_review_code_loading());
        }
    });

    let filter_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_filter_files(move |query| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        filter_controller.filter_files(&ui, query.to_string());
    });

    let extension_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_toggle_extension_filter(move |extension| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        extension_controller.toggle_extension_filter(&ui, extension.to_string());
    });

    let manifest_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_toggle_manifest_filter(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        if let Ok(mut enabled) = manifest_controller.only_manifest_files.lock() {
            *enabled = !*enabled;
        }
        manifest_controller.refresh_tree(&ui);
    });

    let viewed_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_toggle_viewed_files(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        if let Ok(mut enabled) = viewed_controller.show_viewed_files.lock() {
            *enabled = !*enabled;
        }
        viewed_controller.refresh_tree(&ui);
    });

    let folder_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_toggle_folder(move |path| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        folder_controller.toggle_folder(&ui, path.to_string());
    });

    let expand_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_expand_hunk(move |line_index, direction| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let direction = match direction.as_str() {
            "down" => HunkExpansionDirection::Down,
            "all" => HunkExpansionDirection::All,
            _ => HunkExpansionDirection::Up,
        };
        expand_controller.expand_hunk(&ui, line_index, direction);
    });

    let expand_all_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_expand_all_hunks(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        expand_all_controller.expand_all_hunks(&ui);
    });

    let collapse_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_collapse_non_diff_lines(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        collapse_controller.collapse_non_diff_lines(&ui);
    });

    let view_file_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_view_file(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        view_file_controller.toggle_view_file(&ui);
    });

    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_copy_file_path(move |path| {
        let path = path.trim();
        if path.is_empty() {
            return false;
        }
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(path)) {
            Ok(()) => true,
            Err(error) => {
                log::warn!("[Workflow][PR Review] Failed to copy file path: {error}");
                if let Some(ui) = ui_weak.upgrade() {
                    show_error(&ui, crate::i18n::tr("pr_review.copy_file_failed").as_ref());
                }
                false
            }
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_copy_commit_sha(move |sha| {
        let sha = sha.trim();
        if sha.is_empty() {
            return false;
        }
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(sha)) {
            Ok(()) => true,
            Err(error) => {
                log::warn!("[Workflow][PR Review] Failed to copy commit SHA: {error}");
                if let Some(ui) = ui_weak.upgrade() {
                    show_error(&ui, crate::i18n::tr("pr_review.copy_sha_failed").as_ref());
                }
                false
            }
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_toggle_commit_message(move |sha| {
        if let Some(ui) = ui_weak.upgrade() {
            toggle_commit_message_expansion(&ui, sha.as_str());
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_copy_source_branch(move |branch| {
        let branch = branch.trim();
        if branch.is_empty() {
            return false;
        }
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(branch)) {
            Ok(()) => true,
            Err(error) => {
                log::warn!("[Workflow][PR Review] Failed to copy source branch: {error}");
                if let Some(ui) = ui_weak.upgrade() {
                    show_error(
                        &ui,
                        crate::i18n::tr("pr_review.copy_branch_failed").as_ref(),
                    );
                }
                false
            }
        }
    });

    let submit_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_submit_review(move |action, body| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        submit_controller.submit_review(&ui, action.as_str(), body.as_str());
    });

    let dismiss_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_dismiss_review(move |review_id, reason| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        dismiss_controller.dismiss_review(&ui, review_id.as_str(), reason.as_str());
    });

    let merge_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_merge(move |method, remove_source_branch| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        merge_controller.merge(&ui, method.as_str(), remove_source_branch);
    });

    let revert_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_revert(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        revert_controller.revert(&ui);
    });

    controller
}

impl WorkflowPrReviewController {
    #[cfg(feature = "test-fixtures")]
    pub(crate) fn install_memory_benchmark_fixture(&self, ui: &MainWindow) {
        if std::env::var("USHER_MEMORY_PR_REVIEW_FIXTURE").as_deref() != Ok("large-ranges-v1") {
            return;
        }

        let request = ReviewRequest {
            number: "memory-fixture-1".to_string(),
            title: "Deterministic large PR memory fixture".to_string(),
            author: "memory-benchmark".to_string(),
            source_branch: "fixture/head".to_string(),
            target_branch: "fixture/base".to_string(),
            updated_at: "2026-08-10T00:00:00Z".to_string(),
            ignore_whitespace: false,
        };
        let base_detail = Arc::new(SharedDiffDetail::from(memory_benchmark_diff_detail(0)));
        let mut range_diffs = BoundedCache::new(RANGE_DIFF_CACHE_CAPACITY);
        for range in 1..=RANGE_DIFF_CACHE_CAPACITY {
            range_diffs.insert(
                format!("fixture-range-{range}"),
                Arc::new(SharedDiffDetail::from(memory_benchmark_diff_detail(range))),
            );
        }
        if let Ok(mut alias) = self.project_alias.lock() {
            *alias = "memory-fixture".to_string();
        }
        if let Ok(mut active_request) = self.active_request.lock() {
            *active_request = Some(request.clone());
        }
        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some(ReviewCache {
                project_alias: "memory-fixture".to_string(),
                request: request.clone(),
                metadata: ProjectRepositoryMetadata {
                    repo_project_id: "memory-fixture".to_string(),
                    repo_type: "github".to_string(),
                    remote_repo: "https://example.invalid/memory/fixture".to_string(),
                },
                all_detail: base_detail.clone(),
                base_detail: base_detail.clone(),
                range_diffs,
                expanded_files: HashMap::new(),
                loaded_large_diffs: HashSet::new(),
            });
        }

        apply_review_header(ui, &request);
        ui.set_workflow_pr_review_visible(true);
        ui.set_workflow_pr_review_loading(false);
        self.apply_detail(ui, &base_detail);
        ui.invoke_navigate(Route::PrReview);
        self.log_memory_diagnostics("memory_fixture.loaded");

        let ui_weak = ui.as_weak();
        slint::Timer::single_shot(Duration::from_millis(1_500), move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            for index in 0..=crate::router::MAX_ROUTE_HISTORY {
                ui.invoke_navigate(if index % 2 == 0 {
                    Route::About
                } else {
                    Route::Home
                });
            }
            crate::memory_diagnostics::log_checkpoint("memory_fixture.workflow_evicted");
        });
    }

    #[cfg(not(feature = "test-fixtures"))]
    pub(crate) fn install_memory_benchmark_fixture(&self, _ui: &MainWindow) {}

    fn clear_owned_state(&self, ui: &MainWindow) {
        self.invalidate_pending_requests();
        if let Ok(mut request) = self.active_request.lock() {
            *request = None;
        }
        if let Ok(mut cache) = self.cache.lock() {
            *cache = None;
        }
        if let Ok(mut action_state) = self.action_state.lock() {
            *action_state = None;
        }
        self.reset_request_scoped_state(ui, true);
        reset_review_ui(ui);
    }

    fn invalidate_pending_requests(&self) {
        self.state.invalidate_all();
    }

    fn reset_request_scoped_state(&self, ui: &MainWindow, reset_tab: bool) {
        super::find_bridge::cancel();
        if let Ok(mut location) = self.location_reveal.lock() {
            *location = None;
        }
        self.clear_location_presentation(ui);
        if let Ok(mut filter) = self.file_filter.lock() {
            filter.clear();
        }
        if let Ok(mut extensions) = self.selected_extensions.lock() {
            extensions.clear();
        }
        if let Ok(mut enabled) = self.only_manifest_files.lock() {
            *enabled = false;
        }
        if let Ok(mut enabled) = self.show_viewed_files.lock() {
            *enabled = true;
        }
        if let Ok(mut folders) = self.collapsed_folders.lock() {
            folders.clear();
        }
        if let Ok(mut resources) = self.highlight_resources.lock() {
            *resources = None;
        }
        if let Ok(mut highlighted) = self.highlighted_files.lock() {
            highlighted.clear();
        }
        if let Ok(mut sources) = self.source_files.lock() {
            sources.clear();
        }
        if let Ok(mut loaded) = self.conversation_loaded.lock() {
            *loaded = false;
        }
        if let Ok(mut conversation) = self.conversation.lock() {
            conversation.clear();
        }
        if let Ok(mut loaded) = self.commits_loaded.lock() {
            *loaded = false;
        }
        if let Ok(mut commits) = self.commits.lock() {
            commits.clear();
        }
        if let Ok(mut pending) = self.pending_open_commit.lock() {
            *pending = None;
        }
        if let Ok(mut return_tab) = self.commit_detail_return_tab.lock() {
            *return_tab = None;
        }
        if reset_tab {
            ui.set_workflow_pr_review_tab(2);
        }
        ui.set_workflow_pr_review_conversation_count(0);
        ui.set_workflow_pr_review_commit_count(0);
        ui.set_workflow_pr_review_conversation_items(empty_conversation_model());
        ui.set_workflow_pr_review_conversation_content_height(0);
        ui.set_workflow_pr_review_expanded_commit_body_height(0);
        ui.set_workflow_pr_review_commits(empty_commit_model());
        ui.set_workflow_pr_review_conversation_loading(false);
        ui.set_workflow_pr_review_commits_loading(false);
        ui.set_workflow_pr_review_conversation_error("".into());
        ui.set_workflow_pr_review_commits_error("".into());
        ui.set_workflow_pr_review_commit_range_start(-1);
        ui.set_workflow_pr_review_commit_range_end(-1);
        ui.set_workflow_pr_review_commit_range_label(
            crate::i18n::tr("pr_review.all_commits").into_owned().into(),
        );
        ui.set_workflow_pr_review_file_filter("".into());
        reset_action_ui(ui);
    }

    pub(crate) fn release_unreachable(&self, ui: &MainWindow) {
        self.log_memory_diagnostics("workflow_unreachable.before_clear");
        self.pr_list_refresh_pending.store(false, Ordering::SeqCst);
        if let Ok(mut alias) = self.project_alias.lock() {
            alias.clear();
        }
        self.clear_owned_state(ui);
        self.log_memory_diagnostics("workflow_unreachable.after_clear");
    }

    fn navigate_back(&self, ui: &MainWindow) {
        let return_tab = self
            .commit_detail_return_tab
            .lock()
            .ok()
            .and_then(|mut return_tab| return_tab.take());
        if let Some(tab) = return_tab {
            ui.set_workflow_pr_review_tab(tab);
            self.load_tab(ui, tab);
            return;
        }

        self.log_memory_diagnostics("review_close.before_clear");
        self.clear_owned_state(ui);
        ui.invoke_router_back();
        if self.pr_list_refresh_pending.swap(false, Ordering::SeqCst) {
            self.pr_list_controller.refresh_after_remote_mutation(ui);
        }
        self.log_memory_diagnostics("review_close.after_clear");
    }

    pub(crate) fn select_project(&self, ui: &MainWindow, project_alias: String) {
        self.log_memory_diagnostics("project_switch.before_clear");
        self.pr_list_refresh_pending.store(false, Ordering::SeqCst);
        if let Ok(mut alias) = self.project_alias.lock() {
            *alias = project_alias;
        }
        self.clear_owned_state(ui);
        self.log_memory_diagnostics("project_switch.after_clear");
    }

    pub(crate) fn sync_language(&self, ui: &MainWindow) {
        if let Some(request) = self
            .active_request
            .lock()
            .ok()
            .and_then(|request| request.clone())
        {
            apply_review_header(ui, &request);
        }
        if self
            .conversation_loaded
            .lock()
            .map(|loaded| *loaded)
            .unwrap_or(false)
            && let Ok(items) = self.conversation.lock()
        {
            ui.set_workflow_pr_review_conversation_content_height(conversation_content_height(
                items.as_slice(),
            ));
            ui.set_workflow_pr_review_expanded_commit_body_height(0);
            ui.set_workflow_pr_review_conversation_items(conversation_model(
                items.as_slice(),
                &HashMap::new(),
            ));
        }
        if self
            .commits_loaded
            .lock()
            .map(|loaded| *loaded)
            .unwrap_or(false)
            && let Ok(commits) = self.commits.lock()
        {
            ui.set_workflow_pr_review_commits(commit_model(commits.as_slice(), &HashMap::new()));
        }
        if !ui.get_workflow_pr_review_conversation_error().is_empty() {
            ui.set_workflow_pr_review_conversation_error(
                crate::i18n::tr("pr_review.conversation_load_failed")
                    .into_owned()
                    .into(),
            );
        }
        if !ui.get_workflow_pr_review_commits_error().is_empty() {
            ui.set_workflow_pr_review_commits_error(
                crate::i18n::tr("pr_review.commits_load_failed")
                    .into_owned()
                    .into(),
            );
        }
        if ui.get_workflow_pr_review_commit_range_start() < 0
            && ui.get_workflow_pr_review_commit_range_end() < 0
        {
            ui.set_workflow_pr_review_commit_range_label(
                crate::i18n::tr("pr_review.all_commits").into_owned().into(),
            );
        }
        self.refresh_tree(ui);
        let state = self
            .action_state
            .lock()
            .ok()
            .and_then(|state| state.clone());
        if let Some(state) = state {
            apply_action_state(ui, &state);
        } else if !ui.get_workflow_pr_merge_readiness_kind().is_empty() {
            let label_key = readiness_label_key(ui.get_workflow_pr_merge_readiness_kind().as_str());
            ui.set_workflow_pr_merge_readiness_label(
                crate::i18n::tr(label_key).into_owned().into(),
            );
        }
    }

    fn open(&self, ui: &MainWindow, request: ReviewRequest, force_refresh: bool) {
        let project_alias = self
            .project_alias
            .lock()
            .map(|alias| alias.clone())
            .unwrap_or_default();
        if project_alias.is_empty() {
            ui.set_workflow_pr_review_visible(true);
            ui.set_workflow_pr_review_error(
                crate::i18n::tr("pr_review.project_context_missing")
                    .into_owned()
                    .into(),
            );
            return;
        }

        let is_new_pull_request = self
            .active_request
            .lock()
            .ok()
            .and_then(|active| {
                active
                    .as_ref()
                    .map(|active| active.number != request.number)
            })
            .unwrap_or(true);
        if is_new_pull_request || force_refresh {
            self.log_memory_diagnostics("new_review.before_cache_clear");
            self.invalidate_pending_requests();
            if let Ok(mut cache) = self.cache.lock() {
                *cache = None;
            }
            self.reset_request_scoped_state(ui, is_new_pull_request);
            self.log_memory_diagnostics("new_review.after_cache_clear");
        }

        apply_review_header(ui, &request);
        ui.set_workflow_pr_review_visible(true);
        ui.invoke_navigate(Route::PrReview);
        ui.set_workflow_pr_review_error("".into());
        if let Ok(mut active_request) = self.active_request.lock() {
            *active_request = Some(request.clone());
        }
        let cached = if force_refresh {
            None
        } else {
            self.cache.lock().ok().and_then(|guard| {
                guard
                    .as_ref()
                    .filter(|cache| {
                        cache.project_alias == project_alias && cache.request == request
                    })
                    .map(|cache| cache.base_detail.clone())
            })
        };
        if let Some(detail) = cached {
            ui.set_workflow_pr_review_loading(false);
            self.apply_detail(ui, &detail);
            self.refresh_action_state(ui);
            self.log_memory_diagnostics("review_open.cache_hit_applied");
            return;
        }

        if ai_review_fixture_enabled() {
            let detail = Arc::new(SharedDiffDetail::from(ai_review_fixture_diff_detail(
                request.number.as_str(),
            )));
            ui.set_workflow_pr_review_loading(false);
            ui.set_workflow_pr_review_ignore_whitespace_supported(false);
            ui.set_workflow_pr_review_ignore_whitespace(false);
            if let Ok(mut cache) = self.cache.lock() {
                *cache = Some(ReviewCache {
                    project_alias: project_alias.clone(),
                    request: request.clone(),
                    metadata: ProjectRepositoryMetadata {
                        repo_project_id: "ai-review-fixture".to_string(),
                        repo_type: "github".to_string(),
                        remote_repo: "https://example.invalid/ai-review/fixture".to_string(),
                    },
                    all_detail: detail.clone(),
                    base_detail: detail.clone(),
                    range_diffs: BoundedCache::new(RANGE_DIFF_CACHE_CAPACITY),
                    expanded_files: HashMap::new(),
                    loaded_large_diffs: HashSet::new(),
                });
            }
            self.apply_detail(ui, &detail);
            self.refresh_action_state(ui);
            self.log_memory_diagnostics("ai_review_fixture.loaded_and_applied");
            return;
        }

        self.state.invalidate(ReviewRequestKind::Highlight);
        if let Ok(mut highlighted) = self.highlighted_files.lock() {
            highlighted.clear();
        }

        ui.set_workflow_pr_review_loading(true);
        ui.set_workflow_pr_review_code_loading(false);
        ui.set_workflow_pr_review_large_diff_deferred(false);
        ui.set_workflow_pr_review_large_diff_loading(false);
        ui.set_workflow_pr_review_files(empty_file_model());
        ui.set_workflow_pr_review_tree_items(empty_tree_model());
        set_empty_line_model(ui);
        ui.set_workflow_pr_review_selected_file_index(-1);
        ui.set_workflow_pr_review_selected_file_path("".into());
        ui.set_workflow_pr_review_context_expanded(false);

        let generation = self.state.begin(ReviewRequestKind::Diff);
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let alias_for_metadata = project_alias.clone();
            let metadata = tokio::task::spawn_blocking(move || {
                load_repository_metadata(alias_for_metadata.as_str())
            })
            .await
            .map_err(|error| format!("Failed to read project configuration: {error}"))
            .and_then(|result| result);

            let result = match metadata {
                Ok(metadata) => {
                    let capabilities = provider_capabilities(
                        metadata.repo_type.as_str(),
                        metadata.repo_project_id.as_str(),
                        metadata.remote_repo.as_str(),
                    );
                    match capabilities {
                        Ok(capabilities) => {
                            let ignore_whitespace_supported =
                                capabilities.pull_request_ignore_whitespace;
                            get_pull_request_diff(
                                metadata.repo_type.as_str(),
                                metadata.repo_project_id.as_str(),
                                metadata.remote_repo.as_str(),
                                request.number.as_str(),
                                request.ignore_whitespace && ignore_whitespace_supported,
                            )
                            .await
                            .map(|detail| {
                                (
                                    metadata,
                                    detail,
                                    ignore_whitespace_supported,
                                    capabilities.merge_delete_source_branch,
                                )
                            })
                        }
                        Err(error) => Err(error),
                    }
                }
                Err(error) => Err(GitError::configuration(error)),
            };

            let _ = slint::invoke_from_event_loop(move || {
                if !controller
                    .state
                    .is_current(ReviewRequestKind::Diff, generation)
                {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_workflow_pr_review_loading(false);
                match result {
                    Ok((
                        metadata,
                        detail,
                        ignore_whitespace_supported,
                        merge_delete_source_branch_supported,
                    )) => {
                        ui.set_workflow_pr_review_ignore_whitespace_supported(
                            ignore_whitespace_supported,
                        );
                        ui.set_workflow_pr_merge_delete_source_branch_supported(
                            merge_delete_source_branch_supported,
                        );
                        if !ignore_whitespace_supported {
                            ui.set_workflow_pr_review_ignore_whitespace(false);
                        }
                        log::info!(
                            "[Workflow][PR Review] Loaded diff: project={} pr={} files={} added={} removed={} ignore_whitespace={}",
                            project_alias,
                            request.number,
                            detail.files.len(),
                            detail.added_lines,
                            detail.removed_lines,
                            request.ignore_whitespace,
                        );
                        let detail = Arc::new(SharedDiffDetail::from(detail));
                        if let Ok(mut cache) = controller.cache.lock() {
                            *cache = Some(ReviewCache {
                                project_alias,
                                request,
                                metadata,
                                all_detail: detail.clone(),
                                base_detail: detail.clone(),
                                range_diffs: BoundedCache::new(RANGE_DIFF_CACHE_CAPACITY),
                                expanded_files: HashMap::new(),
                                loaded_large_diffs: HashSet::new(),
                            });
                        }
                        controller.apply_detail(&ui, &detail);
                        controller.refresh_action_state(&ui);
                        controller.log_memory_diagnostics("review_diff.loaded_and_applied");
                    }
                    Err(error) => {
                        ui.set_workflow_pr_review_ignore_whitespace_supported(false);
                        ui.set_workflow_pr_review_ignore_whitespace(false);
                        ui.set_workflow_pr_merge_delete_source_branch_supported(false);
                        log::warn!("[Workflow][PR Review] Failed to load diff: {error}");
                        ui.set_workflow_pr_review_error(
                            crate::i18n::tr(error.user_message_key())
                                .into_owned()
                                .into(),
                        );
                        ui.set_workflow_pr_review_error_kind(error.error_kind().into());
                    }
                }
            });
        });
        if force_refresh {
            self.load_tab(ui, ui.get_workflow_pr_review_tab());
        }
    }

    fn load_tab(&self, ui: &MainWindow, tab: i32) {
        match tab {
            0 => self.load_conversation(ui),
            1 => self.load_commits(ui),
            _ => {}
        }
    }

    fn load_conversation(&self, ui: &MainWindow) {
        if self
            .conversation_loaded
            .lock()
            .map(|loaded| *loaded)
            .unwrap_or(false)
        {
            return;
        }
        let Some(request) = self
            .active_request
            .lock()
            .ok()
            .and_then(|request| request.clone())
        else {
            return;
        };
        let project_alias = self
            .project_alias
            .lock()
            .map(|alias| alias.clone())
            .unwrap_or_default();
        let generation = self.state.begin(ReviewRequestKind::Conversation);
        ui.set_workflow_pr_review_conversation_loading(true);
        ui.set_workflow_pr_review_conversation_error("".into());
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let metadata = tokio::task::spawn_blocking(move || {
                load_repository_metadata(project_alias.as_str())
            })
            .await
            .map_err(|error| format!("Failed to read project configuration: {error}"))
            .and_then(|result| result);
            let result = match metadata {
                Ok(metadata) => {
                    get_pull_request_conversation(
                        metadata.repo_type.as_str(),
                        metadata.repo_project_id.as_str(),
                        metadata.remote_repo.as_str(),
                        request.number.as_str(),
                    )
                    .await
                }
                Err(error) => Err(GitError::configuration(error)),
            };
            match result {
                Ok(items) => {
                    let initial_items = items.clone();
                    let initial_controller = controller.clone();
                    let initial_ui_weak = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if !initial_controller
                            .state
                            .is_current(ReviewRequestKind::Conversation, generation)
                        {
                            return;
                        }
                        let Some(ui) = initial_ui_weak.upgrade() else {
                            return;
                        };
                        ui.set_workflow_pr_review_conversation_loading(false);
                        ui.set_workflow_pr_review_conversation_content_height(
                            conversation_content_height(&initial_items),
                        );
                        ui.set_workflow_pr_review_expanded_commit_body_height(0);
                        ui.set_workflow_pr_review_conversation_items(conversation_model(
                            &initial_items,
                            &HashMap::new(),
                        ));
                        if let Ok(mut stored) = initial_controller.conversation.lock() {
                            *stored = initial_items;
                        }
                        if let Ok(mut loaded) = initial_controller.conversation_loaded.lock() {
                            *loaded = true;
                        }
                    });

                    let avatars = load_conversation_avatars(&items).await;
                    if avatars.is_empty() {
                        return;
                    }
                    let _ = slint::invoke_from_event_loop(move || {
                        if !controller
                            .state
                            .is_current(ReviewRequestKind::Conversation, generation)
                        {
                            return;
                        }
                        let Some(ui) = ui_weak.upgrade() else {
                            return;
                        };
                        let model = ui.get_workflow_pr_review_conversation_items();
                        patch_conversation_avatars(&model, &items, &avatars);
                    });
                }
                Err(error) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        if !controller
                            .state
                            .is_current(ReviewRequestKind::Conversation, generation)
                        {
                            return;
                        }
                        let Some(ui) = ui_weak.upgrade() else {
                            return;
                        };
                        ui.set_workflow_pr_review_conversation_loading(false);
                        log::warn!("[Workflow][PR Review] Failed to load conversation: {error}");
                        ui.set_workflow_pr_review_conversation_error(
                            crate::i18n::tr("pr_review.conversation_load_failed")
                                .into_owned()
                                .into(),
                        );
                    });
                }
            }
        });
    }

    fn refresh_conversation_after_remote_action(&self, ui: &MainWindow) {
        self.state.invalidate(ReviewRequestKind::Conversation);
        if let Ok(mut loaded) = self.conversation_loaded.lock() {
            *loaded = false;
        }
        if ui.get_workflow_pr_review_tab() == 0 {
            self.load_conversation(ui);
        }
    }

    fn load_commits(&self, ui: &MainWindow) {
        if self
            .commits_loaded
            .lock()
            .map(|loaded| *loaded)
            .unwrap_or(false)
        {
            return;
        }
        let Some(request) = self
            .active_request
            .lock()
            .ok()
            .and_then(|request| request.clone())
        else {
            return;
        };
        let project_alias = self
            .project_alias
            .lock()
            .map(|alias| alias.clone())
            .unwrap_or_default();
        let generation = self.state.begin(ReviewRequestKind::Commits);
        ui.set_workflow_pr_review_commits_loading(true);
        ui.set_workflow_pr_review_commits_error("".into());
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let metadata = tokio::task::spawn_blocking(move || {
                load_repository_metadata(project_alias.as_str())
            })
            .await
            .map_err(|error| format!("Failed to read project configuration: {error}"))
            .and_then(|result| result);
            let result = match metadata {
                Ok(metadata) => {
                    get_pull_request_commits(
                        metadata.repo_type.as_str(),
                        metadata.repo_project_id.as_str(),
                        metadata.remote_repo.as_str(),
                        request.number.as_str(),
                    )
                    .await
                }
                Err(error) => Err(GitError::configuration(error)),
            };
            match result {
                Ok(commits) => {
                    let initial_commits = commits.clone();
                    let initial_controller = controller.clone();
                    let initial_ui_weak = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if !initial_controller
                            .state
                            .is_current(ReviewRequestKind::Commits, generation)
                        {
                            return;
                        }
                        let Some(ui) = initial_ui_weak.upgrade() else {
                            return;
                        };
                        ui.set_workflow_pr_review_commits_loading(false);
                        // Provider summaries normally populate the badge before
                        // activation. Keep the successfully loaded list as a
                        // final compatibility fallback for older enterprise APIs.
                        let current = ui.get_workflow_pr_review_commit_count();
                        ui.set_workflow_pr_review_commit_count(
                            current.max(initial_commits.len() as i32),
                        );
                        ui.set_workflow_pr_review_commits(commit_model(
                            &initial_commits,
                            &HashMap::new(),
                        ));
                        if let Ok(mut stored) = initial_controller.commits.lock() {
                            *stored = initial_commits;
                        }
                        if let Ok(mut loaded) = initial_controller.commits_loaded.lock() {
                            *loaded = true;
                        }
                        let pending_sha = initial_controller
                            .pending_open_commit
                            .lock()
                            .ok()
                            .and_then(|mut pending| pending.take());
                        if let Some(sha) = pending_sha {
                            initial_controller.open_commit(&ui, sha.as_str());
                        }
                    });

                    let avatars = load_commit_avatars(&commits).await;
                    if avatars.is_empty() {
                        return;
                    }
                    let _ = slint::invoke_from_event_loop(move || {
                        if !controller
                            .state
                            .is_current(ReviewRequestKind::Commits, generation)
                        {
                            return;
                        }
                        let Some(ui) = ui_weak.upgrade() else {
                            return;
                        };
                        let model = ui.get_workflow_pr_review_commits();
                        patch_commit_avatars(&model, &commits, &avatars);
                    });
                }
                Err(error) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        if !controller
                            .state
                            .is_current(ReviewRequestKind::Commits, generation)
                        {
                            return;
                        }
                        let Some(ui) = ui_weak.upgrade() else {
                            return;
                        };
                        ui.set_workflow_pr_review_commits_loading(false);
                        let was_opening_commit = controller
                            .pending_open_commit
                            .lock()
                            .map(|mut pending| pending.take().is_some())
                            .unwrap_or(false);
                        log::warn!("[Workflow][PR Review] Failed to load commits: {error}");
                        ui.set_workflow_pr_review_commits_error(
                            crate::i18n::tr("pr_review.commits_load_failed")
                                .into_owned()
                                .into(),
                        );
                        if was_opening_commit {
                            show_error(
                                &ui,
                                crate::i18n::tr("pr_review.commit_details_failed").as_ref(),
                            );
                        }
                    });
                }
            }
        });
    }

    fn open_commit(&self, ui: &MainWindow, sha: &str) {
        let sha = sha.trim();
        if sha.is_empty() {
            return;
        }
        let source_tab = ui.get_workflow_pr_review_tab();
        if let Some(source_tab) = commit_detail_source_tab(source_tab)
            && let Ok(mut return_tab) = self.commit_detail_return_tab.lock()
        {
            *return_tab = Some(source_tab);
        }
        ui.set_workflow_pr_review_tab(2);
        let commit_index = self.commits.lock().ok().and_then(|commits| {
            commit_index_by_sha(&commits, sha).and_then(|chrono_index| {
                chrono_commit_index_to_display(chrono_index, commits.len())
            })
        });
        if let Some(index) = commit_index {
            self.apply_commit_range(ui, index as i32, index as i32);
            return;
        }

        let commits_loaded = self
            .commits_loaded
            .lock()
            .map(|loaded| *loaded)
            .unwrap_or(false);
        if commits_loaded {
            show_error(ui, crate::i18n::tr("pr_review.commit_missing").as_ref());
            return;
        }
        if let Ok(mut pending) = self.pending_open_commit.lock() {
            *pending = Some(sha.to_string());
        }
        self.load_commits(ui);
    }

    fn apply_commit_range(&self, ui: &MainWindow, start: i32, end: i32) {
        if start < 0 || end < 0 {
            self.state.invalidate(ReviewRequestKind::Diff);
            let detail = self
                .cache
                .lock()
                .ok()
                .and_then(|cache| cache.as_ref().map(|cache| cache.all_detail.clone()));
            if let Some(detail) = detail {
                self.activate_commit_range(ui, detail);
                ui.set_workflow_pr_review_commit_range_start(-1);
                ui.set_workflow_pr_review_commit_range_end(-1);
                ui.set_workflow_pr_review_commit_range_label(
                    crate::i18n::tr("pr_review.all_commits").into_owned().into(),
                );
                ui.set_workflow_pr_review_error("".into());
            }
            return;
        }

        // UI indices are newest-first (same order as the Commits model). Convert
        // to oldest→newest before base/head selection against stored commits.
        let (display_start, display_end) = if start <= end {
            (start as usize, end as usize)
        } else {
            (end as usize, start as usize)
        };
        let commits = self
            .commits
            .lock()
            .map(|commits| commits.clone())
            .unwrap_or_default();
        let Some((start, end)) =
            display_commit_range_to_chrono(display_start, display_end, commits.len())
        else {
            show_error(
                ui,
                crate::i18n::tr("pr_review.load_commits_for_range").as_ref(),
            );
            return;
        };
        let label = commit_range_label(&commits, start, end);
        if range_covers_all_commits(commits.len(), start, end) {
            let detail = self
                .cache
                .lock()
                .ok()
                .and_then(|cache| cache.as_ref().map(|cache| cache.all_detail.clone()));
            if let Some(detail) = detail {
                self.state.invalidate(ReviewRequestKind::Diff);
                self.activate_commit_range(ui, detail);
                ui.set_workflow_pr_review_commit_range_start(display_start as i32);
                ui.set_workflow_pr_review_commit_range_end(display_end as i32);
                ui.set_workflow_pr_review_commit_range_label(label.into());
                ui.set_workflow_pr_review_error("".into());
                return;
            }
        }
        let (metadata, all_base_sha, ignore_whitespace) = match self.cache.lock() {
            Ok(cache) => {
                let Some(cache) = cache.as_ref() else {
                    return;
                };
                (
                    cache.metadata.clone(),
                    cache.all_detail.base_sha.clone(),
                    cache.request.ignore_whitespace,
                )
            }
            Err(_) => return,
        };
        let base_sha = if start == 0 {
            all_base_sha
        } else {
            commits[start - 1].sha.clone()
        };
        let head_sha = commits[end].sha.clone();
        if base_sha.is_empty() || head_sha.is_empty() {
            show_error(
                ui,
                crate::i18n::tr("pr_review.range_data_incomplete").as_ref(),
            );
            return;
        }
        let cache_key = format!("{base_sha}:{head_sha}");
        let cached = self.cache.lock().ok().and_then(|mut cache| {
            cache
                .as_mut()
                .and_then(|cache| cache.range_diffs.get_cloned(cache_key.as_str()))
        });
        if let Some(detail) = cached {
            self.state.invalidate(ReviewRequestKind::Diff);
            self.activate_commit_range(ui, detail);
            ui.set_workflow_pr_review_commit_range_start(display_start as i32);
            ui.set_workflow_pr_review_commit_range_end(display_end as i32);
            ui.set_workflow_pr_review_commit_range_label(label.into());
            ui.set_workflow_pr_review_error("".into());
            return;
        }

        let generation = self.state.begin(ReviewRequestKind::Diff);
        ui.set_workflow_pr_review_loading(true);
        ui.set_workflow_pr_review_error("".into());
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let result = get_pull_request_commit_range_diff(
                metadata.repo_type.as_str(),
                metadata.repo_project_id.as_str(),
                metadata.remote_repo.as_str(),
                base_sha.as_str(),
                head_sha.as_str(),
                commits.len() as u32,
                ignore_whitespace,
            )
            .await;
            let _ = slint::invoke_from_event_loop(move || {
                if !controller
                    .state
                    .is_current(ReviewRequestKind::Diff, generation)
                {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_workflow_pr_review_loading(false);
                match result {
                    Ok(detail) => {
                        let detail = Arc::new(SharedDiffDetail::from(detail));
                        if let Ok(mut cache) = controller.cache.lock()
                            && let Some(cache) = cache.as_mut()
                        {
                            cache
                                .range_diffs
                                .insert(cache_key, detail.clone());
                        }
                        controller.activate_commit_range(&ui, detail);
                        ui.set_workflow_pr_review_commit_range_start(display_start as i32);
                        ui.set_workflow_pr_review_commit_range_end(display_end as i32);
                        ui.set_workflow_pr_review_commit_range_label(label.into());
                        log::info!(
                            "[Workflow][PR Review] Applied commit range: start={} end={} base={} head={}",
                            display_start,
                            display_end,
                            base_sha,
                            head_sha
                        );
                    }
                    Err(error) => {
                        log::warn!(
                            "[Workflow][PR Review] Failed to load commit range diff: {error}"
                        );
                        ui.set_workflow_pr_review_error(
                            crate::i18n::tr("pr_review.range_load_failed")
                                .into_owned()
                                .into(),
                        );
                    }
                }
            });
        });
    }

    fn activate_commit_range(&self, ui: &MainWindow, detail: Arc<SharedDiffDetail>) {
        self.state.invalidate(ReviewRequestKind::Highlight);
        self.state.invalidate(ReviewRequestKind::Expand);
        if let Ok(mut highlighted) = self.highlighted_files.lock() {
            highlighted.clear();
        }
        if let Ok(mut sources) = self.source_files.lock() {
            sources.clear();
        }
        if let Ok(mut cache) = self.cache.lock()
            && let Some(cache) = cache.as_mut()
        {
            cache.base_detail = detail.clone();
            cache.expanded_files.clear();
            cache.loaded_large_diffs.clear();
        }
        self.apply_detail(ui, &detail);
    }

    fn reveal_location(
        &self,
        ui: &MainWindow,
        requested_path: String,
        line: i32,
        side: DiffLineSide,
        review_head_sha: &str,
    ) {
        let generation = self.state.begin(ReviewRequestKind::Reveal);
        self.clear_location_presentation(ui);
        ui.set_workflow_pr_review_tab(2);
        ui.invoke_workflow_pr_review_select_tab(2);

        let prepared = self.cache.lock().ok().and_then(|mut cache| {
            let cache = cache.as_mut()?;
            let file_index =
                location_file_index(&cache.all_detail.files, requested_path.as_str(), side);
            if review_head_sha.is_empty() || cache.all_detail.head_sha != review_head_sha {
                return Some(Err(file_index));
            }
            let file_index = file_index?;
            let restoring_all = !Arc::ptr_eq(&cache.base_detail, &cache.all_detail);
            if restoring_all {
                cache.base_detail = cache.all_detail.clone();
                cache.expanded_files.clear();
                cache.loaded_large_diffs.clear();
            }
            let file = cache.shared_file(file_index)?;
            if is_large_diff(&file) {
                cache.loaded_large_diffs.insert(file_index);
            }
            Some(Ok((
                file_index,
                file.path.clone(),
                restoring_all,
                cache.base_detail.clone(),
            )))
        });

        let Some(prepared) = prepared else {
            self.fail_location_reveal(
                ui,
                generation,
                "ai_review.finding_location_unavailable",
                false,
            );
            return;
        };
        let (file_index, path, restoring_all, detail) = match prepared {
            Ok(prepared) => prepared,
            Err(file_index) => {
                self.fail_location_reveal(
                    ui,
                    generation,
                    "ai_review.finding_location_outdated",
                    file_index.is_some(),
                );
                return;
            }
        };

        if restoring_all && let Ok(mut highlighted) = self.highlighted_files.lock() {
            highlighted.clear();
        }

        if let Ok(mut location) = self.location_reveal.lock() {
            *location = Some(LocationReveal {
                generation,
                file_index,
                path,
                line,
                side,
                expanding_context: false,
            });
        }
        if restoring_all {
            ui.set_workflow_pr_review_commit_range_start(-1);
            ui.set_workflow_pr_review_commit_range_end(-1);
            ui.set_workflow_pr_review_commit_range_label(
                crate::i18n::tr("pr_review.all_commits").into_owned().into(),
            );
            self.apply_detail(ui, &detail);
        }
        self.select_file(ui, file_index as i32);
    }

    fn continue_location_reveal(&self, ui: &MainWindow) {
        let location = self
            .location_reveal
            .lock()
            .ok()
            .and_then(|location| location.clone());
        let Some(location) = location else {
            return;
        };
        if !self
            .state
            .is_current(ReviewRequestKind::Reveal, location.generation)
            || ui.get_workflow_pr_review_selected_file_index() != location.file_index as i32
            || ui.get_workflow_pr_review_selected_file_path().as_str() != location.path
        {
            return;
        }
        let file = self.cache.lock().ok().and_then(|cache| {
            cache
                .as_ref()
                .and_then(|cache| cache.shared_file(location.file_index))
        });
        let Some(file) = file else {
            self.fail_location_reveal(
                ui,
                location.generation,
                "ai_review.finding_location_unavailable",
                false,
            );
            return;
        };
        if let Some(row) = display_row_for_location(
            file.lines.as_slice(),
            location.line,
            location.side,
            ui.get_workflow_pr_review_diff_view().as_str() == "split",
        ) {
            ui.set_workflow_pr_review_location_row_index(row as i32);
            ui.set_workflow_pr_review_location_side(location.side.as_str().into());
            ui.set_workflow_pr_review_location_reveal_revision(
                ui.get_workflow_pr_review_location_reveal_revision()
                    .wrapping_add(1),
            );
            ui.set_ai_review_finding_navigation_message("".into());
            ui.set_ai_review_finding_navigation_can_view_file(false);
            return;
        }

        if location.expanding_context {
            return;
        }
        let Some(hunk_index) =
            hidden_hunk_for_line(&file, location.line, location.side.uses_new_line())
        else {
            self.fail_location_reveal(
                ui,
                location.generation,
                "ai_review.finding_location_unavailable",
                true,
            );
            return;
        };
        if let Ok(mut pending) = self.location_reveal.lock()
            && let Some(pending) = pending.as_mut()
            && pending.generation == location.generation
        {
            pending.expanding_context = true;
        }
        self.load_location_context(ui, location, hunk_index);
    }

    fn load_location_context(&self, ui: &MainWindow, location: LocationReveal, hunk_index: usize) {
        let selected = self.cache.lock().ok().and_then(|cache| {
            let cache = cache.as_ref()?;
            Some((
                cache.shared_file(location.file_index)?,
                cache.base_detail.head_sha.clone(),
                cache.base_detail.base_sha.clone(),
                cache.metadata.clone(),
            ))
        });
        let Some((file, head_sha, base_sha, metadata)) = selected else {
            self.fail_location_reveal(
                ui,
                location.generation,
                "ai_review.finding_location_unavailable",
                false,
            );
            return;
        };
        let base_path = if file.old_path.is_empty() {
            file.path.clone()
        } else {
            file.old_path.clone()
        };
        let base_request = (file.change_type != "new_file")
            .then_some((base_sha, base_path))
            .filter(|(reference, path)| !reference.is_empty() && !path.is_empty());
        let head_request = (file.change_type != "deleted_file")
            .then_some((head_sha, file.path.clone()))
            .filter(|(reference, path)| !reference.is_empty() && !path.is_empty());
        let base_key = base_request
            .as_ref()
            .map(|(reference, path)| format!("{reference}:{path}"));
        let head_key = head_request
            .as_ref()
            .map(|(reference, path)| format!("{reference}:{path}"));
        let (base_cached, head_cached) = self
            .source_files
            .lock()
            .ok()
            .map(|mut sources| {
                (
                    base_key.as_deref().and_then(|key| sources.get_cloned(key)),
                    head_key.as_deref().and_then(|key| sources.get_cloned(key)),
                )
            })
            .unwrap_or_default();
        if (base_request.is_none() || base_cached.is_some())
            && (head_request.is_none() || head_cached.is_some())
        {
            self.apply_location_context(
                ui,
                &location,
                hunk_index,
                base_cached.as_deref().unwrap_or(""),
                head_cached.as_deref().unwrap_or(""),
            );
            return;
        }

        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let base_future = async {
                if let Some(source) = base_cached {
                    Ok(source)
                } else if let Some((reference, path)) = base_request.as_ref() {
                    get_repository_file_text(
                        metadata.repo_type.as_str(),
                        metadata.repo_project_id.as_str(),
                        metadata.remote_repo.as_str(),
                        path.as_str(),
                        reference.as_str(),
                    )
                    .await
                } else {
                    Ok(String::new())
                }
            };
            let head_future = async {
                if let Some(source) = head_cached {
                    Ok(source)
                } else if let Some((reference, path)) = head_request.as_ref() {
                    get_repository_file_text(
                        metadata.repo_type.as_str(),
                        metadata.repo_project_id.as_str(),
                        metadata.remote_repo.as_str(),
                        path.as_str(),
                        reference.as_str(),
                    )
                    .await
                } else {
                    Ok(String::new())
                }
            };
            let (base_result, head_result) = tokio::join!(base_future, head_future);
            let result = base_result.and_then(|base| head_result.map(|head| (base, head)));
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                if !controller
                    .state
                    .is_current(ReviewRequestKind::Reveal, location.generation)
                {
                    return;
                }
                match result {
                    Ok((base_source, head_source)) => {
                        if let Ok(mut sources) = controller.source_files.lock() {
                            if let Some(key) = base_key {
                                sources.insert(key, base_source.clone());
                            }
                            if let Some(key) = head_key {
                                sources.insert(key, head_source.clone());
                            }
                        }
                        controller.apply_location_context(
                            &ui,
                            &location,
                            hunk_index,
                            base_source.as_str(),
                            head_source.as_str(),
                        );
                    }
                    Err(error) => {
                        log::warn!(
                            "[Workflow][PR Review] Failed to reveal finding context: {error}"
                        );
                        controller.fail_location_reveal(
                            &ui,
                            location.generation,
                            "ai_review.finding_location_unavailable",
                            true,
                        );
                    }
                }
            });
        });
    }

    fn apply_location_context(
        &self,
        ui: &MainWindow,
        location: &LocationReveal,
        hunk_index: usize,
        base_source: &str,
        head_source: &str,
    ) {
        if !self
            .state
            .is_current(ReviewRequestKind::Reveal, location.generation)
        {
            return;
        }
        let updated = self.cache.lock().ok().and_then(|mut cache| {
            let cache = cache.as_mut()?;
            let file = cache.file_mut_cow(location.file_index)?;
            let hunk_index =
                hidden_hunk_for_line(file, location.line, location.side.uses_new_line())
                    .unwrap_or(hunk_index);
            expand_hunk_context_around_line_from_sources(
                file,
                hunk_index,
                location.line,
                location.side.uses_new_line(),
                base_source,
                head_source,
                HUNK_CONTEXT_PAGE_SIZE,
            )
            .then(|| cache.shared_file(location.file_index))
            .flatten()
        });
        let Some(file) = updated else {
            self.fail_location_reveal(
                ui,
                location.generation,
                "ai_review.finding_location_unavailable",
                true,
            );
            return;
        };
        if let Ok(mut pending) = self.location_reveal.lock()
            && let Some(pending) = pending.as_mut()
            && pending.generation == location.generation
        {
            pending.expanding_context = false;
        }
        if let Ok(mut highlighted) = self.highlighted_files.lock() {
            highlighted.remove(file.path.as_str());
        }
        self.select_file(ui, location.file_index as i32);
    }

    fn fail_location_reveal(
        &self,
        ui: &MainWindow,
        generation: u64,
        message_key: &str,
        can_view_file: bool,
    ) {
        if !self.state.is_current(ReviewRequestKind::Reveal, generation) {
            return;
        }
        ui.set_workflow_pr_review_location_row_index(-1);
        ui.set_workflow_pr_review_location_side("".into());
        ui.set_ai_review_finding_navigation_message(
            crate::i18n::tr(message_key).into_owned().into(),
        );
        ui.set_ai_review_finding_navigation_can_view_file(can_view_file);
    }

    fn clear_location_presentation(&self, ui: &MainWindow) {
        ui.set_workflow_pr_review_location_row_index(-1);
        ui.set_workflow_pr_review_location_side("".into());
        ui.set_ai_review_finding_navigation_message("".into());
        ui.set_ai_review_finding_navigation_can_view_file(false);
    }

    fn cancel_location_reveal(&self, ui: &MainWindow) {
        self.state.invalidate(ReviewRequestKind::Reveal);
        if let Ok(mut location) = self.location_reveal.lock() {
            *location = None;
        }
        self.clear_location_presentation(ui);
    }

    fn view_location_file(&self, ui: &MainWindow, path: &str, side: DiffLineSide) {
        self.cancel_location_reveal(ui);
        let selected = self.cache.lock().ok().and_then(|mut cache| {
            let cache = cache.as_mut()?;
            let index = location_file_index(&cache.all_detail.files, path, side)?;
            let restoring_all = !Arc::ptr_eq(&cache.base_detail, &cache.all_detail);
            if restoring_all {
                cache.base_detail = cache.all_detail.clone();
                cache.expanded_files.clear();
                cache.loaded_large_diffs.clear();
            }
            Some((index, restoring_all, cache.base_detail.clone()))
        });
        let Some((index, restoring_all, detail)) = selected else {
            show_error(
                ui,
                crate::i18n::tr("ai_review.finding_location_unavailable").as_ref(),
            );
            return;
        };
        if restoring_all {
            if let Ok(mut highlighted) = self.highlighted_files.lock() {
                highlighted.clear();
            }
            self.apply_detail(ui, &detail);
        }
        self.select_file(ui, index as i32);
        self.toggle_view_file(ui);
    }

    fn select_file(&self, ui: &MainWindow, index: i32) {
        ui.set_workflow_pr_review_large_diff_loading(false);
        let index = index.max(0) as usize;
        let selected = self.cache.lock().ok().and_then(|guard| {
            let cache = guard.as_ref()?;
            Some((
                cache.shared_file(index)?,
                cache.expanded_files.contains_key(&index),
                cache.loaded_large_diffs.contains(&index),
            ))
        });
        let Some((file, context_expanded, large_diff_loaded)) = selected else {
            return;
        };
        let location_matches = self
            .location_reveal
            .lock()
            .ok()
            .and_then(|location| {
                location
                    .as_ref()
                    .map(|location| location.file_index == index)
            })
            .unwrap_or(false);
        if !location_matches {
            ui.set_workflow_pr_review_location_row_index(-1);
            ui.set_workflow_pr_review_location_side("".into());
        }
        ui.set_workflow_pr_review_viewing_file(false);
        ui.set_workflow_pr_review_has_expandable_hunks(false);
        ui.set_workflow_pr_review_context_expanded(context_expanded);
        let reset_scroll = ui.get_workflow_pr_review_selected_file_path().as_str() != file.path;
        if reset_scroll {
            ui.set_workflow_pr_review_hunk_scroll_restore_pending(false);
        }
        apply_file_header(ui, index, &file);
        if is_large_diff(&file) && !large_diff_loaded {
            self.state.invalidate(ReviewRequestKind::Highlight);
            set_empty_line_model(ui);
            ui.set_workflow_pr_review_max_code_columns(0);
            ui.set_workflow_pr_review_longest_code_line("".into());
            ui.set_workflow_pr_review_code_loading(false);
            ui.set_workflow_pr_review_large_diff_loading(false);
            ui.set_workflow_pr_review_large_diff_deferred(true);
            if reset_scroll {
                ui.set_workflow_pr_review_line_model_revision(
                    ui.get_workflow_pr_review_line_model_revision()
                        .wrapping_add(1),
                );
            }
            self.log_memory_diagnostics("large_diff.deferred");
            return;
        }
        ui.set_workflow_pr_review_large_diff_deferred(false);
        let generation = self.state.begin(ReviewRequestKind::Highlight);
        let cached = self
            .highlighted_files
            .lock()
            .ok()
            .and_then(|mut highlighted| highlighted.get_cloned(file.path.as_str()));
        if let Some(highlighted) = cached {
            apply_highlighted_file(ui, file, highlighted, reset_scroll);
            ui.set_workflow_pr_review_code_loading(false);
            ui.set_workflow_pr_review_large_diff_loading(false);
            self.log_memory_diagnostics("file_highlight.cache_hit_applied");
            self.continue_location_reveal(ui);
            return;
        }

        // Keep the previous file/model mounted while syntax highlighting runs.
        // Clearing it here causes a full white flash on file switches and hunk
        // expansion. The completed model is applied atomically on the UI loop.
        ui.set_workflow_pr_review_code_loading(true);
        let controller = self.clone();
        let highlight_resources = self.highlight_resources.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let file_for_highlight = file.clone();
            let highlighted = tokio::task::spawn_blocking(move || {
                let resources = session_highlight_resources(&highlight_resources);
                highlight_diff_lines(
                    &resources,
                    file_for_highlight.path.as_str(),
                    file_for_highlight.lines.as_slice(),
                )
            })
            .await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_workflow_pr_review_code_loading(false);
                ui.set_workflow_pr_review_large_diff_loading(false);
                if !controller
                    .state
                    .is_current(ReviewRequestKind::Highlight, generation)
                {
                    return;
                }
                if ui.get_workflow_pr_review_selected_file_path().as_str() != file.path {
                    return;
                }
                let Ok(highlighted) = highlighted else {
                    ui.set_workflow_pr_review_error(
                        crate::i18n::tr("pr_review.syntax_highlight_failed")
                            .into_owned()
                            .into(),
                    );
                    return;
                };
                let highlighted = Arc::new(highlighted);
                if let Ok(mut cached) = controller.highlighted_files.lock() {
                    cached.insert(file.path.clone(), highlighted.clone());
                }
                apply_highlighted_file(&ui, file, highlighted, reset_scroll);
                controller.log_memory_diagnostics("file_highlight.generated_and_applied");
                controller.continue_location_reveal(&ui);
            });
        });
    }

    fn filter_files(&self, ui: &MainWindow, query: String) {
        if let Ok(mut filter) = self.file_filter.lock() {
            *filter = query;
        }
        self.refresh_tree(ui);
    }

    fn toggle_extension_filter(&self, ui: &MainWindow, extension: String) {
        if let Ok(mut selected) = self.selected_extensions.lock()
            && !selected.insert(extension.clone())
        {
            selected.remove(extension.as_str());
        }
        self.refresh_tree(ui);
    }

    fn toggle_folder(&self, ui: &MainWindow, path: String) {
        if path.is_empty() {
            return;
        }
        if let Ok(mut folders) = self.collapsed_folders.lock()
            && !folders.insert(path.clone())
        {
            folders.remove(path.as_str());
        }
        self.refresh_tree(ui);
    }

    fn expand_hunk(&self, ui: &MainWindow, line_index: i32, direction: HunkExpansionDirection) {
        let selected_index = ui.get_workflow_pr_review_selected_file_index().max(0) as usize;
        log::debug!(
            "[Workflow][PR Review] Expand hunk requested: file_index={} line_index={} direction={:?}",
            selected_index,
            line_index,
            direction
        );
        let selected = self.cache.lock().ok().and_then(|cache| {
            let cache = cache.as_ref()?;
            Some((
                cache.shared_file(selected_index)?,
                cache.base_detail.head_sha.clone(),
                cache.base_detail.base_sha.clone(),
                cache.metadata.clone(),
            ))
        });
        let Some((file, head_sha, base_sha, metadata)) = selected else {
            log::warn!(
                "[Workflow][PR Review] Ignored hunk expansion for invalid file index: {}",
                selected_index
            );
            return;
        };
        let line_index = line_index.max(0) as usize;
        if !hunk_is_expandable(&file, line_index) {
            log::debug!(
                "[Workflow][PR Review] Hunk no longer has hidden context: file={} line_index={}",
                file.path,
                line_index
            );
            return;
        }

        let base_path = if file.old_path.is_empty() {
            file.path.clone()
        } else {
            file.old_path.clone()
        };
        let base_request = (file.change_type != "new_file")
            .then_some((base_sha.clone(), base_path))
            .filter(|(reference, path)| !reference.is_empty() && !path.is_empty());
        let head_request = (file.change_type != "deleted_file")
            .then_some((head_sha.clone(), file.path.clone()))
            .filter(|(reference, path)| !reference.is_empty() && !path.is_empty());
        if base_request.is_none() && head_request.is_none() {
            ui.set_workflow_pr_review_error(
                crate::i18n::tr("pr_review.source_reference_missing")
                    .into_owned()
                    .into(),
            );
            return;
        }

        let base_key = base_request
            .as_ref()
            .map(|(reference, path)| format!("{reference}:{path}"));
        let head_key = head_request
            .as_ref()
            .map(|(reference, path)| format!("{reference}:{path}"));
        let (base_cached, head_cached) = self
            .source_files
            .lock()
            .ok()
            .map(|mut sources| {
                (
                    base_key.as_deref().and_then(|key| sources.get_cloned(key)),
                    head_key.as_deref().and_then(|key| sources.get_cloned(key)),
                )
            })
            .unwrap_or_default();
        if (base_request.is_none() || base_cached.is_some())
            && (head_request.is_none() || head_cached.is_some())
        {
            self.apply_hunk_expansion(
                ui,
                selected_index,
                line_index,
                direction,
                base_cached.as_deref().unwrap_or(""),
                head_cached.as_deref().unwrap_or(""),
            );
            return;
        }

        let generation = self.state.begin(ReviewRequestKind::Expand);
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let base_future = async {
                if let Some(source) = base_cached {
                    Ok(source)
                } else if let Some((reference, path)) = base_request.as_ref() {
                    get_repository_file_text(
                        metadata.repo_type.as_str(),
                        metadata.repo_project_id.as_str(),
                        metadata.remote_repo.as_str(),
                        path.as_str(),
                        reference.as_str(),
                    )
                    .await
                } else {
                    Ok(String::new())
                }
            };
            let head_future = async {
                if let Some(source) = head_cached {
                    Ok(source)
                } else if let Some((reference, path)) = head_request.as_ref() {
                    get_repository_file_text(
                        metadata.repo_type.as_str(),
                        metadata.repo_project_id.as_str(),
                        metadata.remote_repo.as_str(),
                        path.as_str(),
                        reference.as_str(),
                    )
                    .await
                } else {
                    Ok(String::new())
                }
            };
            let (base_result, head_result) = tokio::join!(base_future, head_future);
            let result = base_result.and_then(|base| head_result.map(|head| (base, head)));
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                if !controller
                    .state
                    .is_current(ReviewRequestKind::Expand, generation)
                {
                    return;
                }
                if ui.get_workflow_pr_review_selected_file_index() != selected_index as i32 {
                    return;
                }
                match result {
                    Ok((base_source, head_source)) => {
                        if let Ok(mut sources) = controller.source_files.lock() {
                            if let Some(key) = base_key {
                                sources.insert(key, base_source.clone());
                            }
                            if let Some(key) = head_key {
                                sources.insert(key, head_source.clone());
                            }
                        }
                        controller.apply_hunk_expansion(
                            &ui,
                            selected_index,
                            line_index,
                            direction,
                            base_source.as_str(),
                            head_source.as_str(),
                        );
                    }
                    Err(error) => {
                        log::warn!("[Workflow][PR Review] Failed to expand hunk: {error}");
                    }
                }
            });
        });
    }

    fn expand_all_hunks(&self, ui: &MainWindow) {
        let selected_index = ui.get_workflow_pr_review_selected_file_index().max(0) as usize;
        let selected = self.cache.lock().ok().and_then(|cache| {
            let cache = cache.as_ref()?;
            Some((
                cache.shared_file(selected_index)?,
                cache.base_detail.head_sha.clone(),
                cache.base_detail.base_sha.clone(),
                cache.metadata.clone(),
            ))
        });
        let Some((file, head_sha, base_sha, metadata)) = selected else {
            return;
        };
        if !file
            .lines
            .iter()
            .enumerate()
            .any(|(index, _)| hunk_is_expandable(&file, index))
        {
            return;
        }

        let base_path = if file.old_path.is_empty() {
            file.path.clone()
        } else {
            file.old_path.clone()
        };
        let base_request = (file.change_type != "new_file")
            .then_some((base_sha.clone(), base_path))
            .filter(|(reference, path)| !reference.is_empty() && !path.is_empty());
        let head_request = (file.change_type != "deleted_file")
            .then_some((head_sha.clone(), file.path.clone()))
            .filter(|(reference, path)| !reference.is_empty() && !path.is_empty());
        if base_request.is_none() && head_request.is_none() {
            ui.set_workflow_pr_review_error(
                crate::i18n::tr("pr_review.source_reference_missing")
                    .into_owned()
                    .into(),
            );
            return;
        }
        let base_key = base_request
            .as_ref()
            .map(|(reference, path)| format!("{reference}:{path}"));
        let head_key = head_request
            .as_ref()
            .map(|(reference, path)| format!("{reference}:{path}"));
        let (base_cached, head_cached) = self
            .source_files
            .lock()
            .ok()
            .map(|mut sources| {
                (
                    base_key.as_deref().and_then(|key| sources.get_cloned(key)),
                    head_key.as_deref().and_then(|key| sources.get_cloned(key)),
                )
            })
            .unwrap_or_default();
        if (base_request.is_none() || base_cached.is_some())
            && (head_request.is_none() || head_cached.is_some())
        {
            self.apply_all_hunk_expansion(
                ui,
                selected_index,
                base_cached.as_deref().unwrap_or(""),
                head_cached.as_deref().unwrap_or(""),
            );
            return;
        }

        ui.set_workflow_pr_review_code_loading(true);
        let generation = self.state.begin(ReviewRequestKind::Expand);
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let base_future = async {
                if let Some(source) = base_cached {
                    Ok(source)
                } else if let Some((reference, path)) = base_request.as_ref() {
                    get_repository_file_text(
                        metadata.repo_type.as_str(),
                        metadata.repo_project_id.as_str(),
                        metadata.remote_repo.as_str(),
                        path.as_str(),
                        reference.as_str(),
                    )
                    .await
                } else {
                    Ok(String::new())
                }
            };
            let head_future = async {
                if let Some(source) = head_cached {
                    Ok(source)
                } else if let Some((reference, path)) = head_request.as_ref() {
                    get_repository_file_text(
                        metadata.repo_type.as_str(),
                        metadata.repo_project_id.as_str(),
                        metadata.remote_repo.as_str(),
                        path.as_str(),
                        reference.as_str(),
                    )
                    .await
                } else {
                    Ok(String::new())
                }
            };
            let (base_result, head_result) = tokio::join!(base_future, head_future);
            let result = base_result.and_then(|base| head_result.map(|head| (base, head)));
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_workflow_pr_review_code_loading(false);
                if !controller
                    .state
                    .is_current(ReviewRequestKind::Expand, generation)
                {
                    return;
                }
                if ui.get_workflow_pr_review_selected_file_index() != selected_index as i32 {
                    return;
                }
                match result {
                    Ok((base_source, head_source)) => {
                        if let Ok(mut sources) = controller.source_files.lock() {
                            if let Some(key) = base_key {
                                sources.insert(key, base_source.clone());
                            }
                            if let Some(key) = head_key {
                                sources.insert(key, head_source.clone());
                            }
                        }
                        controller.apply_all_hunk_expansion(
                            &ui,
                            selected_index,
                            base_source.as_str(),
                            head_source.as_str(),
                        );
                    }
                    Err(error) => {
                        log::warn!("[Workflow][PR Review] Failed to expand all hunks: {error}");
                    }
                }
            });
        });
    }

    fn apply_all_hunk_expansion(
        &self,
        ui: &MainWindow,
        file_index: usize,
        base_source: &str,
        head_source: &str,
    ) {
        let updated = self.cache.lock().ok().and_then(|mut cache| {
            let cache = cache.as_mut()?;
            let updated = {
                let file = cache.file_mut_cow(file_index)?;
                expand_all_hunk_context_from_sources(file, base_source, head_source) > 0
            };
            updated.then(|| cache.shared_file(file_index)).flatten()
        });
        let Some(file) = updated else {
            ui.set_workflow_pr_review_code_loading(false);
            return;
        };
        if let Ok(mut highlighted) = self.highlighted_files.lock() {
            highlighted.remove(file.path.as_str());
        }
        self.select_file(ui, file_index as i32);
    }

    fn collapse_non_diff_lines(&self, ui: &MainWindow) {
        let selected_index = ui.get_workflow_pr_review_selected_file_index().max(0) as usize;
        self.state.invalidate(ReviewRequestKind::Expand);
        let restored = self.cache.lock().ok().and_then(|mut cache| {
            let cache = cache.as_mut()?;
            cache.collapse_file(selected_index)
        });
        let Some(file) = restored else {
            return;
        };
        if let Ok(mut highlighted) = self.highlighted_files.lock() {
            highlighted.remove(file.path.as_str());
        }
        ui.set_workflow_pr_review_error("".into());
        log::info!(
            "[Workflow][PR Review] Collapsed non-diff lines: file={}",
            file.path
        );
        self.select_file(ui, selected_index as i32);
    }

    fn toggle_view_file(&self, ui: &MainWindow) {
        let selected_index = ui.get_workflow_pr_review_selected_file_index().max(0) as usize;
        if ui.get_workflow_pr_review_viewing_file() {
            ui.set_workflow_pr_review_viewing_file(false);
            self.select_file(ui, selected_index as i32);
            return;
        }

        let selected = self.cache.lock().ok().and_then(|cache| {
            let cache = cache.as_ref()?;
            Some((
                cache.shared_file(selected_index)?,
                cache.base_detail.head_sha.clone(),
                cache.base_detail.base_sha.clone(),
                cache.metadata.clone(),
            ))
        });
        let Some((file, head_sha, base_sha, metadata)) = selected else {
            return;
        };
        let use_new_side = file.change_type != "deleted_file";
        let path = if use_new_side {
            file.path.clone()
        } else {
            file.old_path.clone()
        };
        let reference = if use_new_side { head_sha } else { base_sha };
        if path.is_empty() || reference.is_empty() {
            show_error(
                ui,
                crate::i18n::tr("pr_review.source_reference_missing").as_ref(),
            );
            return;
        }
        let source_key = format!("{reference}:{path}");
        if let Some(source) = self
            .source_files
            .lock()
            .ok()
            .and_then(|mut sources| sources.get_cloned(source_key.as_str()))
        {
            self.render_full_file(ui, selected_index, file, source);
            return;
        }

        ui.set_workflow_pr_review_code_loading(true);
        let generation = self.state.begin(ReviewRequestKind::Expand);
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let result = get_repository_file_text(
                metadata.repo_type.as_str(),
                metadata.repo_project_id.as_str(),
                metadata.remote_repo.as_str(),
                path.as_str(),
                reference.as_str(),
            )
            .await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_workflow_pr_review_code_loading(false);
                if !controller
                    .state
                    .is_current(ReviewRequestKind::Expand, generation)
                {
                    return;
                }
                if ui.get_workflow_pr_review_selected_file_index() != selected_index as i32 {
                    return;
                }
                match result {
                    Ok(source) => {
                        if let Ok(mut sources) = controller.source_files.lock() {
                            sources.insert(source_key, source.clone());
                        }
                        controller.render_full_file(&ui, selected_index, file, source);
                    }
                    Err(error) => {
                        log::warn!("[Workflow][PR Review] Failed to view file: {error}");
                        show_error(
                            &ui,
                            crate::i18n::tr("pr_review.load_complete_file_failed").as_ref(),
                        );
                    }
                }
            });
        });
    }

    fn render_full_file(
        &self,
        ui: &MainWindow,
        selected_index: usize,
        file: SharedDiffFile,
        source: String,
    ) {
        let full_file = Arc::new(full_source_file(&file, source.as_str()));
        ui.set_workflow_pr_review_viewing_file(true);
        ui.set_workflow_pr_review_has_expandable_hunks(false);
        ui.set_workflow_pr_review_context_expanded(false);
        ui.set_workflow_pr_review_code_loading(true);
        let generation = self.state.begin(ReviewRequestKind::Highlight);
        let controller = self.clone();
        let highlight_resources = self.highlight_resources.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let highlight_file = full_file.clone();
            let highlighted = tokio::task::spawn_blocking(move || {
                let resources = session_highlight_resources(&highlight_resources);
                highlight_diff_lines(
                    &resources,
                    highlight_file.path.as_str(),
                    highlight_file.lines.as_slice(),
                )
            })
            .await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_workflow_pr_review_code_loading(false);
                if !controller
                    .state
                    .is_current(ReviewRequestKind::Highlight, generation)
                {
                    return;
                }
                if ui.get_workflow_pr_review_selected_file_index() != selected_index as i32
                    || !ui.get_workflow_pr_review_viewing_file()
                {
                    return;
                }
                match highlighted {
                    Ok(highlighted) => {
                        apply_highlighted_file(&ui, full_file, Arc::new(highlighted), true);
                        controller.log_memory_diagnostics("full_file.highlighted_and_applied");
                    }
                    Err(error) => {
                        log::warn!("[Workflow][PR Review] Full-file highlighting failed: {error}");
                    }
                }
            });
        });
    }

    fn apply_hunk_expansion(
        &self,
        ui: &MainWindow,
        file_index: usize,
        line_index: usize,
        direction: HunkExpansionDirection,
        base_source: &str,
        head_source: &str,
    ) {
        let updated = self.cache.lock().ok().and_then(|mut cache| {
            let cache = cache.as_mut()?;
            let original = cache.shared_file(file_index)?;
            let limit = if direction == HunkExpansionDirection::All {
                i32::MAX as usize
            } else {
                HUNK_CONTEXT_PAGE_SIZE
            };
            let updated = {
                let file = cache.file_mut_cow(file_index)?;
                expand_hunk_context_from_sources(
                    file,
                    line_index,
                    base_source,
                    head_source,
                    limit,
                    direction,
                )
            };
            let updated_file = updated.then(|| cache.shared_file(file_index)).flatten()?;
            let anchor_source_index =
                hunk_scroll_anchor_source_index(&original, &updated_file, line_index);
            Some((updated_file, anchor_source_index))
        });
        let Some((file, anchor_source_index)) = updated else {
            log::warn!(
                "[Workflow][PR Review] Source loaded but hunk context could not be inserted: file_index={} line_index={}",
                file_index,
                line_index
            );
            return;
        };
        if let Ok(mut highlighted) = self.highlighted_files.lock() {
            highlighted.remove(file.path.as_str());
        }
        ui.set_workflow_pr_review_hunk_scroll_anchor_source_index(anchor_source_index);
        ui.set_workflow_pr_review_hunk_scroll_restore_pending(true);
        ui.set_workflow_pr_review_error("".into());
        log::info!(
            "[Workflow][PR Review] Expanded hunk context: file={} line_index={} direction={:?} limit={}",
            file.path,
            line_index,
            direction,
            HUNK_CONTEXT_PAGE_SIZE
        );
        self.select_file(ui, file_index as i32);
    }

    fn apply_detail(&self, ui: &MainWindow, detail: &SharedDiffDetail) {
        apply_detail_summary(ui, detail);
        if let Ok(mut selected) = self.selected_extensions.lock() {
            *selected = extension_counts(detail.files.as_slice())
                .into_keys()
                .collect();
        }
        if let Ok(mut enabled) = self.only_manifest_files.lock() {
            *enabled = false;
        }
        if let Ok(mut enabled) = self.show_viewed_files.lock() {
            *enabled = true;
        }
        self.refresh_tree(ui);
        if !detail.files.is_empty() {
            self.select_file(ui, 0);
        } else {
            clear_selected_file(ui);
        }
    }

    fn refresh_tree(&self, ui: &MainWindow) {
        let detail = self
            .cache
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|cache| cache.base_detail.clone()));
        let Some(detail) = detail else {
            ui.set_workflow_pr_review_tree_items(empty_tree_model());
            return;
        };
        let query = self
            .file_filter
            .lock()
            .map(|filter| filter.clone())
            .unwrap_or_default();
        let collapsed = self
            .collapsed_folders
            .lock()
            .map(|folders| folders.clone())
            .unwrap_or_default();
        let selected_extensions = self
            .selected_extensions
            .lock()
            .map(|extensions| extensions.clone())
            .unwrap_or_default();
        let only_manifest_files = self
            .only_manifest_files
            .lock()
            .map(|enabled| *enabled)
            .unwrap_or(false);
        let show_viewed_files = self
            .show_viewed_files
            .lock()
            .map(|enabled| *enabled)
            .unwrap_or(true);
        let counts = extension_counts(detail.files.as_slice());
        let extension_filters = counts
            .iter()
            .map(|(extension, count)| WorkflowPrFileExtensionFilter {
                extension: extension.clone().into(),
                label: if extension == "No extension" {
                    crate::i18n::tr("pr_review.no_extension")
                        .into_owned()
                        .into()
                } else {
                    extension.clone().into()
                },
                count: *count as i32,
                selected: selected_extensions.contains(extension),
            })
            .collect::<Vec<_>>();
        ui.set_workflow_pr_review_extension_filters(ModelRc::new(VecModel::from(
            extension_filters,
        )));
        ui.set_workflow_pr_review_only_manifest_files(only_manifest_files);
        ui.set_workflow_pr_review_show_viewed_files(show_viewed_files);
        ui.set_workflow_pr_review_file_filters_active(
            only_manifest_files || !show_viewed_files || selected_extensions.len() != counts.len(),
        );
        ui.set_workflow_pr_review_tree_items(ModelRc::new(VecModel::from(build_tree_items(
            detail.files.as_slice(),
            query.as_str(),
            &collapsed,
            &selected_extensions,
            only_manifest_files,
            show_viewed_files,
        ))));
    }

    fn log_memory_diagnostics(&self, reason: &str) {
        if !crate::memory_diagnostics::diagnostics_enabled() {
            return;
        }
        let (base_stats, override_stats, owned_detail_entries, owned_detail_stats) = self
            .cache
            .lock()
            .ok()
            .and_then(|cache| {
                cache.as_ref().map(|cache| {
                    (
                        diff_storage_stats(&cache.base_detail.files),
                        diff_override_storage_stats(&cache.expanded_files),
                        owned_diff_storage_stats(cache),
                    )
                })
            })
            .map(|(base, overrides, (entries, owned))| (base, overrides, entries, owned))
            .unwrap_or_default();
        let (highlight_entries, highlight_stats) = self
            .highlighted_files
            .lock()
            .map(|cache| (cache.entries.len(), highlighted_cache_stats(&cache)))
            .unwrap_or_default();
        let (source_entries, source_stats) = self
            .source_files
            .lock()
            .map(|cache| (cache.entries.len(), source_cache_stats(&cache)))
            .unwrap_or_default();
        let highlight_resources_loaded = self
            .highlight_resources
            .lock()
            .map(|resources| resources.is_some())
            .unwrap_or(false);
        log::info!(
            "[Workflow][PR Review][Memory] reason={} base_files={} base_lines={} base_text_bytes={} base_heap_estimate_bytes={} owned_detail_entries={} owned_detail_files={} owned_detail_lines={} owned_detail_text_bytes={} owned_detail_heap_estimate_bytes={} override_files={} override_lines={} override_text_bytes={} override_heap_estimate_bytes={} highlight_resources_loaded={} highlight_cache_entries={} highlight_lines={} highlight_tokens={} highlight_text_bytes={} highlight_heap_estimate_bytes={} source_cache_entries={} source_text_bytes={} source_heap_estimate_bytes={}",
            reason,
            base_stats.items,
            base_stats.children,
            base_stats.text_bytes,
            base_stats.estimated_heap_bytes,
            owned_detail_entries,
            owned_detail_stats.items,
            owned_detail_stats.children,
            owned_detail_stats.text_bytes,
            owned_detail_stats.estimated_heap_bytes,
            override_stats.items,
            override_stats.children,
            override_stats.text_bytes,
            override_stats.estimated_heap_bytes,
            highlight_resources_loaded,
            highlight_entries,
            highlight_stats.items,
            highlight_stats.children,
            highlight_stats.text_bytes,
            highlight_stats.estimated_heap_bytes,
            source_entries,
            source_stats.text_bytes,
            source_stats.estimated_heap_bytes,
        );
        crate::memory_diagnostics::log_checkpoint(reason);
    }
}

fn commit_detail_source_tab(tab: i32) -> Option<i32> {
    matches!(tab, 0 | 1).then_some(tab)
}

#[cfg(any(test, feature = "test-fixtures"))]
fn ai_review_fixture_enabled() -> bool {
    std::env::var("USHER_AI_REVIEW_FIXTURE").as_deref() == Ok("review-v1")
}

#[cfg(not(any(test, feature = "test-fixtures")))]
fn ai_review_fixture_enabled() -> bool {
    false
}

#[cfg(any(test, feature = "test-fixtures"))]
fn ai_review_fixture_diff_detail(pull_request_number: &str) -> PullRequestDiffDetail {
    PullRequestDiffDetail {
        files: vec![PullRequestDiffFile {
            path: "src/fixture_0000.rs".to_string(),
            old_path: "src/fixture_0000.rs".to_string(),
            change_type: "edit_file".to_string(),
            added_lines: 3,
            removed_lines: 0,
            viewed: false,
            lines: vec![
                PullRequestDiffLine {
                    old_line: 0,
                    new_line: 0,
                    kind: "hunk".to_string(),
                    content: "@@ -8,3 +8,6 @@ fn persist_valid_input() {".to_string(),
                },
                PullRequestDiffLine {
                    old_line: 8,
                    new_line: 8,
                    kind: "context".to_string(),
                    content: "let result = validate(input);".to_string(),
                },
                PullRequestDiffLine {
                    old_line: 0,
                    new_line: 9,
                    kind: "addition".to_string(),
                    content: "record_validation(result);".to_string(),
                },
                PullRequestDiffLine {
                    old_line: 0,
                    new_line: 10,
                    kind: "addition".to_string(),
                    content: "notify_observers(result);".to_string(),
                },
                PullRequestDiffLine {
                    old_line: 9,
                    new_line: 11,
                    kind: "context".to_string(),
                    content: "let value = normalize(input);".to_string(),
                },
                PullRequestDiffLine {
                    old_line: 0,
                    new_line: 12,
                    kind: "addition".to_string(),
                    content: "persist(input);".to_string(),
                },
                PullRequestDiffLine {
                    old_line: 10,
                    new_line: 13,
                    kind: "context".to_string(),
                    content: "Ok(value)".to_string(),
                },
            ],
        }],
        added_lines: 3,
        removed_lines: 0,
        commit_count: 1,
        conversation_count: Some(0),
        base_sha: "fixture-base".to_string(),
        head_sha: format!("fixture-head-{pull_request_number}"),
        start_sha: "fixture-base".to_string(),
        too_many_commits: false,
        files_truncated: false,
    }
}

#[cfg(not(any(test, feature = "test-fixtures")))]
fn ai_review_fixture_diff_detail(_pull_request_number: &str) -> PullRequestDiffDetail {
    unreachable!("AI Review fixtures require the test-fixtures feature")
}

#[cfg(feature = "test-fixtures")]
fn memory_benchmark_diff_detail(seed: usize) -> PullRequestDiffDetail {
    const TOTAL_FILES: usize = 206;
    const TOTAL_LINES: usize = 27_369;
    const SELECTED_LINES: usize = 2_932;
    const CONTENT_BYTES: usize = 112;

    let remaining_lines = TOTAL_LINES - SELECTED_LINES;
    let remaining_files = TOTAL_FILES - 1;
    let lines_per_file = remaining_lines / remaining_files;
    let extra_lines = remaining_lines % remaining_files;
    let files = (0..TOTAL_FILES)
        .map(|file_index| {
            let line_count = if file_index == 0 {
                SELECTED_LINES
            } else {
                lines_per_file + usize::from(file_index <= extra_lines)
            };
            PullRequestDiffFile {
                path: format!("src/fixture-{seed}/file-{file_index:03}.rs"),
                old_path: format!("src/fixture-{seed}/file-{file_index:03}.rs"),
                change_type: "edit_file".to_string(),
                added_lines: line_count as u32,
                removed_lines: 0,
                viewed: false,
                lines: (0..line_count)
                    .map(|line_index| {
                        let prefix = format!("{seed:02}:{file_index:03}:{line_index:05}:");
                        let mut content = String::with_capacity(CONTENT_BYTES);
                        content.push_str(prefix.as_str());
                        content.extend(std::iter::repeat_n(
                            'x',
                            CONTENT_BYTES.saturating_sub(prefix.len()),
                        ));
                        PullRequestDiffLine {
                            old_line: line_index as i32 + 1,
                            new_line: line_index as i32 + 1,
                            kind: "context".to_string(),
                            content,
                        }
                    })
                    .collect(),
            }
        })
        .collect();
    PullRequestDiffDetail {
        files,
        added_lines: TOTAL_LINES as u32,
        removed_lines: 0,
        commit_count: 12,
        conversation_count: Some(8),
        base_sha: format!("base-fixture-{seed}"),
        head_sha: format!("head-fixture-{seed}"),
        start_sha: format!("start-fixture-{seed}"),
        too_many_commits: false,
        files_truncated: false,
    }
}

fn apply_review_header(ui: &MainWindow, request: &ReviewRequest) {
    ui.set_workflow_pr_review_number(request.number.clone().into());
    let title = if request.title.trim().is_empty() {
        crate::i18n::tr("pr_list.untitled_pull_request").into_owned()
    } else {
        request.title.clone()
    };
    let author = if request.author.trim().is_empty() {
        crate::i18n::tr("common.unknown_author").into_owned()
    } else {
        request.author.clone()
    };
    ui.set_workflow_pr_review_title(title.into());
    ui.set_workflow_pr_review_author(author.into());
    ui.set_workflow_pr_review_source_branch(request.source_branch.clone().into());
    ui.set_workflow_pr_review_target_branch(request.target_branch.clone().into());
    ui.set_workflow_pr_review_updated_at(
        super::super::pr_list::format_github_relative_time(request.updated_at.as_str()).into(),
    );
    ui.set_workflow_pr_review_updated_at_tooltip(
        super::super::pr_list::format_github_absolute_tooltip(request.updated_at.as_str()).into(),
    );
    ui.set_workflow_pr_review_ignore_whitespace(request.ignore_whitespace);
}

fn apply_detail_summary(ui: &MainWindow, detail: &SharedDiffDetail) {
    let files = detail
        .files
        .iter()
        .map(|file| to_slint_file(file))
        .collect::<Vec<_>>();
    ui.set_workflow_pr_review_files(ModelRc::new(VecModel::from(files)));
    ui.set_workflow_pr_review_added_lines(detail.added_lines as i32);
    ui.set_workflow_pr_review_removed_lines(detail.removed_lines as i32);
    // Prefer the larger of the summary field and any already-visible badge so a
    // later range compare with a missing total cannot wipe the Commits counter.
    let current_commits = ui.get_workflow_pr_review_commit_count();
    ui.set_workflow_pr_review_commit_count(current_commits.max(detail.commit_count as i32));
    if let Some(conversation_count) = detail.conversation_count {
        let current = ui.get_workflow_pr_review_conversation_count();
        ui.set_workflow_pr_review_conversation_count(current.max(conversation_count as i32));
    }
    ui.set_workflow_pr_review_base_sha(detail.base_sha.clone().into());
    ui.set_workflow_pr_review_head_sha(detail.head_sha.clone().into());
    ui.set_workflow_pr_review_too_many_commits(detail.too_many_commits);
    ui.set_workflow_pr_review_files_truncated(detail.files_truncated);
    ui.set_workflow_pr_review_error("".into());
    ui.set_workflow_pr_review_error_kind("".into());
}

fn clear_selected_file(ui: &MainWindow) {
    ui.set_workflow_pr_review_selected_file_index(-1);
    ui.set_workflow_pr_review_selected_file_path("".into());
    ui.set_workflow_pr_review_selected_file_added_lines(0);
    ui.set_workflow_pr_review_selected_file_removed_lines(0);
    ui.set_workflow_pr_review_max_code_columns(0);
    ui.set_workflow_pr_review_longest_code_line("".into());
    set_empty_line_model(ui);
    ui.set_workflow_pr_review_code_loading(false);
    ui.set_workflow_pr_review_large_diff_deferred(false);
    ui.set_workflow_pr_review_large_diff_loading(false);
    ui.set_workflow_pr_review_context_expanded(false);
}

fn apply_file_header(ui: &MainWindow, index: usize, file: &PullRequestDiffFile) {
    ui.set_workflow_pr_review_selected_file_index(index as i32);
    ui.set_workflow_pr_review_selected_file_path(file.path.clone().into());
    ui.set_workflow_pr_review_selected_file_added_lines(file.added_lines as i32);
    ui.set_workflow_pr_review_selected_file_removed_lines(file.removed_lines as i32);
}

fn location_file_index(files: &[SharedDiffFile], path: &str, side: DiffLineSide) -> Option<usize> {
    if side == DiffLineSide::Left
        && let Some(index) = files.iter().position(|file| {
            !file.old_path.is_empty() && file.old_path != file.path && file.old_path == path
        })
    {
        return Some(index);
    }
    files.iter().position(|file| file.path == path)
}

fn is_large_diff(file: &PullRequestDiffFile) -> bool {
    file.lines.len() >= LARGE_DIFF_LINE_THRESHOLD
        || file
            .lines
            .iter()
            .map(|line| line.content.len())
            .sum::<usize>()
            >= LARGE_DIFF_BYTE_THRESHOLD
}

fn apply_highlighted_file(
    ui: &MainWindow,
    file: SharedDiffFile,
    highlighted: Arc<Vec<HighlightedLine>>,
    reset_scroll: bool,
) {
    let max_columns = highlighted
        .iter()
        .map(|line| line.columns)
        .max()
        .unwrap_or(0);
    let longest_code_line = highlighted
        .iter()
        .max_by_key(|line| line.columns)
        .map(|line| {
            line.tokens
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>()
        })
        .unwrap_or_default();
    ui.set_workflow_pr_review_max_code_columns(max_columns as i32);
    ui.set_workflow_pr_review_longest_code_line(longest_code_line.into());
    let line_model = Rc::new(LazyPrDiffLineModel::new(
        file.clone(),
        highlighted,
        ui.get_workflow_pr_review_diff_view().as_str() == "split",
    ));
    let metrics = ModelRc::new(VecModel::from(line_model.row_metrics()));
    set_line_model(ui, ModelRc::from(line_model), metrics);
    ui.set_workflow_pr_review_has_expandable_hunks(
        !ui.get_workflow_pr_review_viewing_file()
            && file
                .lines
                .iter()
                .enumerate()
                .any(|(index, _)| hunk_is_expandable(&file, index)),
    );
    if ui.get_workflow_pr_review_hunk_scroll_restore_pending() {
        ui.set_workflow_pr_review_hunk_scroll_restore_pending(false);
        ui.set_workflow_pr_review_hunk_scroll_restore_revision(
            ui.get_workflow_pr_review_hunk_scroll_restore_revision()
                .wrapping_add(1),
        );
    }
    if reset_scroll {
        ui.set_workflow_pr_review_line_model_revision(
            ui.get_workflow_pr_review_line_model_revision()
                .wrapping_add(1),
        );
    }
}

fn set_line_model(
    ui: &MainWindow,
    lines: ModelRc<WorkflowPrDiffLine>,
    metrics: ModelRc<WorkflowPrDiffRowMetric>,
) {
    super::find_bridge::cancel();
    ui.set_workflow_pr_review_lines(lines);
    ui.set_workflow_pr_review_line_metrics(metrics);
    ui.set_workflow_pr_review_line_content_revision(
        ui.get_workflow_pr_review_line_content_revision()
            .wrapping_add(1),
    );
}

fn set_empty_line_model(ui: &MainWindow) {
    set_line_model(ui, empty_line_model(), empty_line_metric_model());
}

fn empty_file_model() -> ModelRc<WorkflowPrDiffFile> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrDiffFile>::new()))
}

fn empty_line_model() -> ModelRc<WorkflowPrDiffLine> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrDiffLine>::new()))
}

fn empty_line_metric_model() -> ModelRc<WorkflowPrDiffRowMetric> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrDiffRowMetric>::new()))
}

fn empty_tree_model() -> ModelRc<WorkflowPrTreeItem> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrTreeItem>::new()))
}

fn empty_extension_filter_model() -> ModelRc<WorkflowPrFileExtensionFilter> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrFileExtensionFilter>::new()))
}

fn toggle_commit_message_expansion(ui: &MainWindow, sha: &str) {
    let sha = sha.trim();
    if sha.is_empty() {
        return;
    }

    let conversation = ui.get_workflow_pr_review_conversation_items();
    let mut conversation_items = (0..conversation.row_count())
        .filter_map(|index| conversation.row_data(index))
        .collect::<Vec<_>>();
    let commits = ui.get_workflow_pr_review_commits();
    let mut commit_items = (0..commits.row_count())
        .filter_map(|index| commits.row_data(index))
        .collect::<Vec<_>>();

    // Keep Conversation and Commits in sync for the same SHA. Use the union of
    // current expand state so a stale tab cannot invert the other.
    let currently_expanded = conversation_items.iter().any(|item| {
        item.kind.as_str() == "commit" && item.id.as_str() == sha && item.message_expanded
    }) || commit_items
        .iter()
        .any(|item| item.sha.as_str() == sha && item.message_expanded);
    let target = !currently_expanded;

    for (index, item) in conversation_items.iter_mut().enumerate() {
        if item.kind.as_str() == "commit" && item.id.as_str() == sha {
            item.message_expanded = target;
            // Preserve the ModelRc identity so an active ListView keeps its
            // viewport anchor while only this delegate changes height.
            conversation.set_row_data(index, item.clone());
        }
    }
    let expanded_body_height = conversation_items
        .iter()
        .filter(|item| item.kind.as_str() == "commit" && item.message_expanded)
        .map(|item| item.message_body_height)
        .sum::<i32>();
    ui.set_workflow_pr_review_expanded_commit_body_height(expanded_body_height);

    for item in &mut commit_items {
        if item.sha.as_str() == sha {
            item.message_expanded = target;
        }
    }
    refresh_commit_group_expand_extras(&mut commit_items);
    // Updating rows in place is essential here. Replacing the entire model
    // makes Slint rebuild the ListView and can move the clicked kebab away from
    // the pointer. Stable row identity keeps everything above the row fixed;
    // only this row grows and subsequent content moves down.
    for (index, item) in commit_items.into_iter().enumerate() {
        commits.set_row_data(index, item);
    }
}

fn reset_review_ui(ui: &MainWindow) {
    ui.set_workflow_pr_review_visible(false);
    ui.set_workflow_pr_review_loading(false);
    ui.set_workflow_pr_review_code_loading(false);
    ui.set_workflow_pr_review_large_diff_deferred(false);
    ui.set_workflow_pr_review_large_diff_loading(false);
    ui.set_workflow_pr_review_location_row_index(-1);
    ui.set_workflow_pr_review_location_side("".into());
    ui.set_workflow_pr_review_error("".into());
    ui.set_workflow_pr_review_files(empty_file_model());
    ui.set_workflow_pr_review_tree_items(empty_tree_model());
    ui.set_workflow_pr_review_extension_filters(empty_extension_filter_model());
    ui.set_workflow_pr_review_only_manifest_files(false);
    ui.set_workflow_pr_review_show_viewed_files(true);
    ui.set_workflow_pr_review_file_filters_active(false);
    set_empty_line_model(ui);
    ui.set_workflow_pr_review_selected_file_index(-1);
    ui.set_workflow_pr_review_selected_file_path("".into());
    ui.set_workflow_pr_review_selected_file_added_lines(0);
    ui.set_workflow_pr_review_selected_file_removed_lines(0);
    ui.set_workflow_pr_review_max_code_columns(0);
    ui.set_workflow_pr_review_longest_code_line("".into());
    ui.set_workflow_pr_review_added_lines(0);
    ui.set_workflow_pr_review_removed_lines(0);
    ui.set_workflow_pr_review_commit_count(0);
    ui.set_workflow_pr_review_conversation_count(0);
    ui.set_workflow_pr_review_base_sha("".into());
    ui.set_workflow_pr_review_head_sha("".into());
    ui.set_workflow_pr_review_too_many_commits(false);
    ui.set_workflow_pr_review_files_truncated(false);
    ui.set_workflow_pr_review_tab(2);
    ui.set_workflow_pr_review_conversation_items(empty_conversation_model());
    ui.set_workflow_pr_review_conversation_content_height(0);
    ui.set_workflow_pr_review_expanded_commit_body_height(0);
    ui.set_workflow_pr_review_commits(empty_commit_model());
    ui.set_workflow_pr_review_conversation_loading(false);
    ui.set_workflow_pr_review_commits_loading(false);
    ui.set_workflow_pr_review_conversation_error("".into());
    ui.set_workflow_pr_review_commits_error("".into());
    ui.set_workflow_pr_review_commit_range_start(-1);
    ui.set_workflow_pr_review_commit_range_end(-1);
    ui.set_workflow_pr_review_commit_range_label(
        crate::i18n::tr("pr_review.all_commits").into_owned().into(),
    );
    ui.set_workflow_pr_review_has_expandable_hunks(false);
    ui.set_workflow_pr_review_context_expanded(false);
    ui.set_workflow_pr_review_viewing_file(false);
    ui.set_workflow_pr_review_ignore_whitespace_supported(false);
    ui.set_workflow_pr_review_ignore_whitespace(false);
    ui.set_workflow_pr_review_file_filter("".into());
    reset_action_ui(ui);
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;
    use std::time::Duration;

    use super::{
        BoundedCache, DiffLineSide, HUNK_CONTEXT_PAGE_SIZE, HunkExpansionDirection,
        LARGE_DIFF_BYTE_THRESHOLD, LARGE_DIFF_LINE_THRESHOLD, RANGE_DIFF_CACHE_CAPACITY,
        ReviewCache, ReviewRequest, SharedDiffDetail, ai_review_fixture_diff_detail,
        build_tree_items, commit_detail_source_tab, diff_storage_stats, expand_all_hunk_context,
        expand_all_hunk_context_from_sources, expand_hunk_context,
        expand_hunk_context_from_sources, full_source_file, hunk_can_expand_all,
        hunk_can_expand_down, hunk_is_expandable, hunk_scroll_anchor_source_index, is_large_diff,
        location_file_index, owned_diff_storage_stats, restore_original_diff,
    };
    use crate::git::{PullRequestDiffDetail, PullRequestDiffFile, PullRequestDiffLine};
    use crate::project::ProjectRepositoryMetadata;

    fn file(path: &str) -> PullRequestDiffFile {
        PullRequestDiffFile {
            path: path.to_string(),
            old_path: path.to_string(),
            change_type: "edit_file".to_string(),
            added_lines: 1,
            removed_lines: 0,
            viewed: false,
            lines: Vec::new(),
        }
    }

    #[test]
    fn ai_review_fixture_finding_path_and_line_exist_in_diff() {
        let detail = ai_review_fixture_diff_detail("42");
        let file = detail.files.first().expect("fixture file");
        assert_eq!(detail.head_sha, "fixture-head-42");
        assert_eq!(file.path, "src/fixture_0000.rs");
        assert!(file.lines.iter().any(|line| {
            line.new_line == 12 && line.kind == "addition" && line.content == "persist(input);"
        }));
    }

    #[test]
    fn large_diff_budget_accounts_for_line_count_and_payload_size() {
        let line = |content: String| PullRequestDiffLine {
            old_line: 0,
            new_line: 1,
            kind: "addition".to_string(),
            content,
        };

        let mut by_lines = file("src/generated.rs");
        by_lines.lines = vec![line("small".to_string()); LARGE_DIFF_LINE_THRESHOLD];
        assert!(is_large_diff(&by_lines));

        let mut by_bytes = file("src/minified.js");
        by_bytes.lines = vec![line("x".repeat(LARGE_DIFF_BYTE_THRESHOLD))];
        assert!(is_large_diff(&by_bytes));

        let mut ordinary = file("src/lib.rs");
        ordinary.lines = vec![line("small".to_string()); 32];
        assert!(!is_large_diff(&ordinary));
    }

    #[test]
    fn renamed_file_locations_resolve_paths_on_the_requested_side() {
        let mut renamed = file("src/new_name.rs");
        renamed.old_path = "src/old_name.rs".to_string();
        let files = vec![Arc::new(file("src/old_name.rs")), Arc::new(renamed)];

        assert_eq!(
            location_file_index(&files, "src/old_name.rs", DiffLineSide::Left),
            Some(1)
        );
        assert_eq!(
            location_file_index(&files, "src/old_name.rs", DiffLineSide::Right),
            Some(0)
        );
        assert_eq!(
            location_file_index(&files, "src/new_name.rs", DiffLineSide::Right),
            Some(1)
        );
    }

    #[test]
    fn commit_detail_back_point_is_only_created_from_content_tabs() {
        assert_eq!(commit_detail_source_tab(0), Some(0));
        assert_eq!(commit_detail_source_tab(1), Some(1));
        assert_eq!(commit_detail_source_tab(2), None);
        assert_eq!(commit_detail_source_tab(99), None);
    }

    #[test]
    fn shares_selected_base_files_and_clones_only_expanded_context() {
        let original = file("src/lib.rs");
        let base_detail = Arc::new(SharedDiffDetail::from(PullRequestDiffDetail {
            files: vec![original.clone(), file("src/main.rs")],
            added_lines: 2,
            removed_lines: 0,
            commit_count: 2,
            conversation_count: Some(0),
            base_sha: "base".to_string(),
            head_sha: "head".to_string(),
            start_sha: "start".to_string(),
            too_many_commits: false,
            files_truncated: false,
        }));
        let mut cache = ReviewCache {
            project_alias: "usher".to_string(),
            request: ReviewRequest {
                number: "1".to_string(),
                title: String::new(),
                author: String::new(),
                source_branch: String::new(),
                target_branch: String::new(),
                updated_at: String::new(),
                ignore_whitespace: false,
            },
            metadata: ProjectRepositoryMetadata {
                repo_project_id: "1".to_string(),
                repo_type: "github".to_string(),
                remote_repo: "https://github.com/example/usher".to_string(),
            },
            all_detail: base_detail.clone(),
            base_detail: base_detail.clone(),
            range_diffs: BoundedCache::new(RANGE_DIFF_CACHE_CAPACITY),
            expanded_files: HashMap::new(),
            loaded_large_diffs: HashSet::new(),
        };

        let selected = cache.shared_file(0).expect("selected base file");
        assert!(Arc::ptr_eq(&selected, &base_detail.files[0]));

        cache
            .file_mut_cow(0)
            .expect("first file")
            .lines
            .push(PullRequestDiffLine {
                old_line: 1,
                new_line: 1,
                kind: "context".to_string(),
                content: "expanded".to_string(),
            });

        assert_eq!(cache.expanded_files.len(), 1);
        assert!(base_detail.files[0].lines.is_empty());
        let expanded = cache.shared_file(0).expect("expanded file");
        assert!(!Arc::ptr_eq(&expanded, &base_detail.files[0]));
        assert_eq!(expanded.lines.len(), 1);
        assert_eq!(cache.file(1), base_detail.files.get(1).map(AsRef::as_ref));
        let restored = cache.collapse_file(0).expect("restored base file");
        assert_eq!(restored.as_ref(), &original);
        assert!(Arc::ptr_eq(&restored, &base_detail.files[0]));
        assert!(cache.expanded_files.is_empty());
    }

    #[test]
    fn memory_stats_count_unique_full_and_range_details() {
        let full_detail = Arc::new(SharedDiffDetail::from(PullRequestDiffDetail {
            files: vec![file("src/lib.rs")],
            added_lines: 1,
            removed_lines: 0,
            commit_count: 2,
            conversation_count: Some(0),
            base_sha: "base".to_string(),
            head_sha: "head".to_string(),
            start_sha: "start".to_string(),
            too_many_commits: false,
            files_truncated: false,
        }));
        let range_detail = Arc::new(SharedDiffDetail::from(PullRequestDiffDetail {
            files: vec![file("src/range.rs")],
            added_lines: 1,
            removed_lines: 0,
            commit_count: 1,
            conversation_count: None,
            base_sha: "range-base".to_string(),
            head_sha: "range-head".to_string(),
            start_sha: String::new(),
            too_many_commits: false,
            files_truncated: false,
        }));
        let mut cache = ReviewCache {
            project_alias: "usher".to_string(),
            request: ReviewRequest {
                number: "1".to_string(),
                title: String::new(),
                author: String::new(),
                source_branch: String::new(),
                target_branch: String::new(),
                updated_at: String::new(),
                ignore_whitespace: false,
            },
            metadata: ProjectRepositoryMetadata {
                repo_project_id: "1".to_string(),
                repo_type: "github".to_string(),
                remote_repo: "https://github.com/example/usher".to_string(),
            },
            all_detail: full_detail,
            base_detail: range_detail.clone(),
            range_diffs: BoundedCache::new(RANGE_DIFF_CACHE_CAPACITY),
            expanded_files: HashMap::new(),
            loaded_large_diffs: HashSet::new(),
        };
        cache
            .range_diffs
            .insert("base:head".to_string(), range_detail);

        let (entries, stats) = owned_diff_storage_stats(&cache);
        assert_eq!(entries, 2);
        assert_eq!(stats.items, 2);
        assert!(stats.text_bytes >= "baseheadrange-baserange-head".len());
    }

    #[test]
    #[ignore = "run through scripts/measure-pr-review-memory-ab.sh"]
    fn selected_file_memory_ab_sample() {
        const TOTAL_FILES: usize = 206;
        const TOTAL_LINES: usize = 27_369;
        const CONTENT_BYTES: usize = 112;

        let mode = std::env::var("USHER_PR_MEMORY_AB_MODE")
            .expect("set USHER_PR_MEMORY_AB_MODE=baseline or candidate");
        let selected_lines = std::env::var("USHER_PR_MEMORY_AB_SELECTED_LINES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(2_932);
        assert!(selected_lines <= TOTAL_LINES);

        let _ = crate::memory_diagnostics::process_memory_snapshot();
        let before = crate::memory_diagnostics::process_memory_snapshot();
        let detail = synthetic_diff_detail(TOTAL_FILES, TOTAL_LINES, selected_lines, CONTENT_BYTES);

        let (after, owned_heap_bytes, checksum) = match mode.as_str() {
            "baseline" => {
                let detail = Arc::new(detail);
                let selected =
                    Arc::new(detail.files.first().expect("selected fixture file").clone());
                let base_stats = diff_storage_stats(&detail.files);
                let selected_stats = diff_storage_stats(std::slice::from_ref(selected.as_ref()));
                let owned_heap_bytes = base_stats
                    .estimated_heap_bytes
                    .saturating_add(selected_stats.estimated_heap_bytes);
                let checksum = touch_diff_files(&detail.files)
                    .saturating_add(touch_diff_files(std::slice::from_ref(selected.as_ref())));
                std::hint::black_box((&detail, &selected, checksum));
                std::thread::sleep(Duration::from_millis(50));
                (
                    crate::memory_diagnostics::process_memory_snapshot(),
                    owned_heap_bytes,
                    checksum,
                )
            }
            "candidate" => {
                let detail = Arc::new(SharedDiffDetail::from(detail));
                let selected = detail.files.first().expect("selected fixture file").clone();
                let owned_heap_bytes = diff_storage_stats(&detail.files).estimated_heap_bytes;
                let checksum = touch_diff_files(&detail.files);
                std::hint::black_box((&detail, &selected, checksum));
                std::thread::sleep(Duration::from_millis(50));
                (
                    crate::memory_diagnostics::process_memory_snapshot(),
                    owned_heap_bytes,
                    checksum,
                )
            }
            other => panic!("unsupported USHER_PR_MEMORY_AB_MODE={other}"),
        };

        println!(
            "PR_MEMORY_AB mode={} selected_lines={} total_files={} total_lines={} content_bytes={} rss_before_bytes={} rss_after_bytes={} rss_delta_bytes={} footprint_before_bytes={} footprint_after_bytes={} footprint_delta_bytes={} owned_heap_bytes={} checksum={}",
            mode,
            selected_lines,
            TOTAL_FILES,
            TOTAL_LINES,
            CONTENT_BYTES,
            before.rss_current_bytes,
            after.rss_current_bytes,
            signed_byte_delta(after.rss_current_bytes, before.rss_current_bytes),
            before.phys_footprint_bytes,
            after.phys_footprint_bytes,
            signed_byte_delta(after.phys_footprint_bytes, before.phys_footprint_bytes),
            owned_heap_bytes,
            checksum,
        );
    }

    fn synthetic_diff_detail(
        total_files: usize,
        total_lines: usize,
        selected_lines: usize,
        content_bytes: usize,
    ) -> PullRequestDiffDetail {
        let remaining_lines = total_lines.saturating_sub(selected_lines);
        let remaining_files = total_files.saturating_sub(1);
        let lines_per_file = remaining_lines / remaining_files.max(1);
        let extra_lines = remaining_lines % remaining_files.max(1);
        let files = (0..total_files)
            .map(|file_index| {
                let line_count = if file_index == 0 {
                    selected_lines
                } else {
                    lines_per_file + usize::from(file_index <= extra_lines)
                };
                PullRequestDiffFile {
                    path: format!("src/fixture/file-{file_index:03}.rs"),
                    old_path: format!("src/fixture/file-{file_index:03}.rs"),
                    change_type: "edit_file".to_string(),
                    added_lines: line_count as u32,
                    removed_lines: 0,
                    viewed: false,
                    lines: (0..line_count)
                        .map(|line_index| {
                            let prefix = format!("{file_index:03}:{line_index:05}:");
                            let mut content = String::with_capacity(content_bytes);
                            content.push_str(prefix.as_str());
                            content.extend(std::iter::repeat_n(
                                'x',
                                content_bytes.saturating_sub(prefix.len()),
                            ));
                            PullRequestDiffLine {
                                old_line: line_index as i32 + 1,
                                new_line: line_index as i32 + 1,
                                kind: "context".to_string(),
                                content,
                            }
                        })
                        .collect(),
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            files.iter().map(|file| file.lines.len()).sum::<usize>(),
            total_lines
        );
        PullRequestDiffDetail {
            files,
            added_lines: total_lines as u32,
            removed_lines: 0,
            commit_count: 12,
            conversation_count: Some(8),
            base_sha: "base-fixture".to_string(),
            head_sha: "head-fixture".to_string(),
            start_sha: "start-fixture".to_string(),
            too_many_commits: false,
            files_truncated: false,
        }
    }

    fn touch_diff_files<T>(files: &[T]) -> u64
    where
        T: std::borrow::Borrow<PullRequestDiffFile>,
    {
        files
            .iter()
            .flat_map(|file| file.borrow().lines.iter())
            .flat_map(|line| line.content.as_bytes().iter())
            .fold(0_u64, |checksum, byte| {
                checksum.wrapping_add(u64::from(*byte))
            })
    }

    fn signed_byte_delta(after: u64, before: u64) -> i64 {
        if after >= before {
            after.saturating_sub(before).min(i64::MAX as u64) as i64
        } else {
            -(before.saturating_sub(after).min(i64::MAX as u64) as i64)
        }
    }

    #[test]
    fn builds_filterable_collapsible_file_tree() {
        let files = vec![file("docs/specs/http.md"), file("src/lib.rs")];
        let extensions = HashSet::from([".md".to_string(), ".rs".to_string()]);
        let items = build_tree_items(
            &files,
            "",
            &HashSet::from(["docs".to_string()]),
            &extensions,
            false,
            true,
        );
        assert!(
            items
                .iter()
                .any(|item| item.path == "docs" && !item.expanded)
        );
        assert!(!items.iter().any(|item| item.path == "docs/specs/http.md"));

        let filtered = build_tree_items(
            &files,
            "http",
            &HashSet::from(["docs".to_string()]),
            &extensions,
            false,
            true,
        );
        assert!(
            filtered
                .iter()
                .any(|item| item.path == "docs/specs/http.md")
        );
        assert!(!filtered.iter().any(|item| item.path == "src/lib.rs"));
    }

    #[test]
    fn combines_extension_manifest_and_viewed_filters() {
        let mut viewed_manifest = file("Cargo.toml");
        viewed_manifest.viewed = true;
        let files = vec![viewed_manifest, file("src/lib.rs"), file("README.md")];
        let rust_only = HashSet::from([".rs".to_string()]);
        let items = build_tree_items(&files, "", &HashSet::new(), &rust_only, false, true);
        assert!(items.iter().any(|item| item.path == "src/lib.rs"));
        assert!(!items.iter().any(|item| item.path == "README.md"));

        let manifest_extensions = HashSet::from([".toml".to_string()]);
        let hidden_viewed = build_tree_items(
            &files,
            "",
            &HashSet::new(),
            &manifest_extensions,
            true,
            false,
        );
        assert!(hidden_viewed.is_empty());
    }

    #[test]
    fn expands_hidden_context_up_in_improved_changes_page_sized_pages() {
        let mut file = file("src/lib.rs");
        file.lines = vec![
            PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: "@@ -30,2 +30,2 @@".to_string(),
            },
            PullRequestDiffLine {
                old_line: 30,
                new_line: 30,
                kind: "context".to_string(),
                content: "line 30".to_string(),
            },
        ];
        let source = (1..=40)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(hunk_is_expandable(&file, 0));
        assert!(expand_hunk_context(
            &mut file,
            0,
            source.as_str(),
            HUNK_CONTEXT_PAGE_SIZE,
            HunkExpansionDirection::Up,
        ));
        assert_eq!(file.lines[1].new_line, 9);
        assert_eq!(file.lines[21].new_line, 29);
        assert!(hunk_is_expandable(&file, 0));

        assert!(expand_hunk_context(
            &mut file,
            0,
            source.as_str(),
            HUNK_CONTEXT_PAGE_SIZE,
            HunkExpansionDirection::Up,
        ));
        assert_eq!(file.lines[1].new_line, 1);
        assert!(!hunk_is_expandable(&file, 0));
    }

    #[test]
    fn expands_hidden_context_down_from_the_previous_hunk() {
        let mut file = file("src/lib.rs");
        file.lines = vec![
            PullRequestDiffLine {
                old_line: 10,
                new_line: 10,
                kind: "context".to_string(),
                content: "line 10".to_string(),
            },
            PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: "@@ -40,1 +40,1 @@".to_string(),
            },
            PullRequestDiffLine {
                old_line: 40,
                new_line: 40,
                kind: "context".to_string(),
                content: "line 40".to_string(),
            },
        ];
        let source = (1..=40)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(hunk_can_expand_down(&file, 1));
        assert!(expand_hunk_context(
            &mut file,
            1,
            source.as_str(),
            5,
            HunkExpansionDirection::Down,
        ));
        assert_eq!(file.lines[1].new_line, 11);
        assert_eq!(file.lines[5].new_line, 15);
        assert_eq!(file.lines[6].kind, "hunk");
        assert!(hunk_can_expand_down(&file, 6));
    }

    #[test]
    fn hunk_scroll_anchor_uses_the_first_revealed_row_for_down_expansion() {
        let mut original = file("src/lib.rs");
        original.lines = vec![
            PullRequestDiffLine {
                old_line: 10,
                new_line: 10,
                kind: "context".to_string(),
                content: "line 10".to_string(),
            },
            PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: "@@ -40,1 +40,1 @@".to_string(),
            },
            PullRequestDiffLine {
                old_line: 40,
                new_line: 40,
                kind: "context".to_string(),
                content: "line 40".to_string(),
            },
        ];
        let source = (1..=40)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut updated = original.clone();
        assert!(expand_hunk_context(
            &mut updated,
            1,
            source.as_str(),
            5,
            HunkExpansionDirection::Down,
        ));

        assert_eq!(
            hunk_scroll_anchor_source_index(&original, &updated, 1),
            1,
            "the first row revealed downward must replace the clicked control"
        );
    }

    #[test]
    fn hunk_scroll_anchor_does_not_depend_on_wrapped_row_estimates() {
        let mut original = file("src/lib.rs");
        original.lines = vec![
            PullRequestDiffLine {
                old_line: 10,
                new_line: 10,
                kind: "context".to_string(),
                content: "short".to_string(),
            },
            PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: "@@ -40,1 +40,1 @@".to_string(),
            },
            PullRequestDiffLine {
                old_line: 40,
                new_line: 40,
                kind: "context".to_string(),
                content: "short".to_string(),
            },
        ];
        let source = (1..=40)
            .map(|line| format!("{line:02}abcdefghijklmnopqr"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut updated = original.clone();
        assert!(expand_hunk_context(
            &mut updated,
            1,
            source.as_str(),
            2,
            HunkExpansionDirection::Down,
        ));

        assert_eq!(
            hunk_scroll_anchor_source_index(&original, &updated, 1),
            1,
            "visual anchoring must use the stable insertion slot, not wrapped-row estimates"
        );
    }

    #[test]
    fn hunk_scroll_anchor_keeps_each_clicked_slot_during_repeated_down_expansion() {
        let mut file = file("src/lib.rs");
        file.lines = vec![
            PullRequestDiffLine {
                old_line: 10,
                new_line: 10,
                kind: "context".to_string(),
                content: "line 10".to_string(),
            },
            PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: "@@ -80,1 +80,1 @@".to_string(),
            },
            PullRequestDiffLine {
                old_line: 80,
                new_line: 80,
                kind: "context".to_string(),
                content: "line 80".to_string(),
            },
        ];
        let source = (1..=80)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");

        let first_original = file.clone();
        assert!(expand_hunk_context(
            &mut file,
            1,
            source.as_str(),
            5,
            HunkExpansionDirection::Down,
        ));
        assert_eq!(
            hunk_scroll_anchor_source_index(&first_original, &file, 1),
            1
        );
        assert_eq!(file.lines[1].new_line, 11);

        let second_hunk_index = file
            .lines
            .iter()
            .position(|line| line.kind == "hunk")
            .expect("separator should survive the first partial expansion");
        let second_original = file.clone();
        assert!(expand_hunk_context(
            &mut file,
            second_hunk_index,
            source.as_str(),
            5,
            HunkExpansionDirection::Down,
        ));
        assert_eq!(
            hunk_scroll_anchor_source_index(&second_original, &file, second_hunk_index),
            second_hunk_index as i32
        );
        assert_eq!(file.lines[second_hunk_index].new_line, 16);
    }

    #[test]
    fn hunk_scroll_anchor_stays_put_when_the_separator_is_removed() {
        let mut original = file("src/lib.rs");
        original.lines = vec![
            PullRequestDiffLine {
                old_line: 57,
                new_line: 57,
                kind: "context".to_string(),
                content: "line 57".to_string(),
            },
            PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: "@@ -62,1 +62,1 @@".to_string(),
            },
            PullRequestDiffLine {
                old_line: 62,
                new_line: 62,
                kind: "context".to_string(),
                content: "line 62".to_string(),
            },
        ];
        let source = (1..=62)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut updated = original.clone();
        assert!(expand_hunk_context_from_sources(
            &mut updated,
            1,
            source.as_str(),
            source.as_str(),
            i32::MAX as usize,
            HunkExpansionDirection::All,
        ));
        assert!(!updated.lines.iter().any(|line| line.kind == "hunk"));
        assert_eq!(
            updated.lines.get(1).map(|line| line.new_line),
            Some(58),
            "the first revealed row must replace the line-57 hunk control"
        );
        assert_eq!(hunk_scroll_anchor_source_index(&original, &updated, 1), 1);
    }

    #[test]
    fn github_improved_changes_page_reveals_twenty_one_rows_then_expands_the_remainder() {
        let mut file = file("src/lib.rs");
        file.lines = vec![
            PullRequestDiffLine {
                old_line: 17,
                new_line: 17,
                kind: "context".to_string(),
                content: "line 17".to_string(),
            },
            PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: "@@ -51,1 +51,1 @@".to_string(),
            },
            PullRequestDiffLine {
                old_line: 51,
                new_line: 51,
                kind: "context".to_string(),
                content: "line 51".to_string(),
            },
        ];
        let source = (1..=51)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(expand_hunk_context_from_sources(
            &mut file,
            1,
            source.as_str(),
            source.as_str(),
            HUNK_CONTEXT_PAGE_SIZE,
            HunkExpansionDirection::Down,
        ));
        assert!(
            file.lines.iter().any(|line| {
                line.old_line == 38 && line.new_line == 38 && line.kind == "context"
            })
        );
        assert!(
            !file
                .lines
                .iter()
                .any(|line| line.old_line == 39 && line.kind == "context")
        );
        let hunk_index = file
            .lines
            .iter()
            .position(|line| line.kind == "hunk")
            .expect("partially expanded gap keeps its hunk control");
        assert!(hunk_can_expand_all(&file, hunk_index));

        assert!(expand_hunk_context_from_sources(
            &mut file,
            hunk_index,
            source.as_str(),
            source.as_str(),
            i32::MAX as usize,
            HunkExpansionDirection::All,
        ));
        assert!(!file.lines.iter().any(|line| line.kind == "hunk"));
        assert!((17..=51).all(|number| {
            file.lines
                .iter()
                .any(|line| line.old_line == number && line.new_line == number)
        }));
    }

    #[test]
    fn expanding_down_preserves_a_base_only_line_at_the_page_boundary() {
        let mut file = file("src/lib.rs");
        file.lines = vec![
            PullRequestDiffLine {
                old_line: 17,
                new_line: 17,
                kind: "context".to_string(),
                content: "line 17".to_string(),
            },
            PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: "@@ -39,1 +38,1 @@".to_string(),
            },
            PullRequestDiffLine {
                old_line: 39,
                new_line: 38,
                kind: "context".to_string(),
                content: "line 39".to_string(),
            },
        ];
        let base_source = (1..=39)
            .map(|line| {
                if line == 38 {
                    "base-only line 38".to_string()
                } else {
                    format!("line {line}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let head_source = (1..=38)
            .map(|line| {
                if line < 38 {
                    format!("line {line}")
                } else {
                    "line 39".to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(expand_hunk_context_from_sources(
            &mut file,
            1,
            base_source.as_str(),
            head_source.as_str(),
            20,
            HunkExpansionDirection::Down,
        ));
        assert!(file.lines.iter().any(|line| {
            line.old_line == 38
                && line.new_line == 0
                && line.kind == "deletion"
                && line.content == "base-only line 38"
        }));
    }

    #[test]
    fn small_between_hunk_gap_switches_to_one_unfold_action() {
        let mut file = file("src/lib.rs");
        file.lines = vec![
            PullRequestDiffLine {
                old_line: 57,
                new_line: 57,
                kind: "context".to_string(),
                content: "line 57".to_string(),
            },
            PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: "@@ -62,1 +62,1 @@".to_string(),
            },
            PullRequestDiffLine {
                old_line: 62,
                new_line: 62,
                kind: "context".to_string(),
                content: "line 62".to_string(),
            },
        ];

        assert!(hunk_is_expandable(&file, 1));
        assert!(hunk_can_expand_all(&file, 1));
    }

    #[test]
    fn expands_all_hidden_context_in_one_action() {
        let mut file = file("src/lib.rs");
        file.lines = vec![
            PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: "@@ -40,1 +40,1 @@".to_string(),
            },
            PullRequestDiffLine {
                old_line: 40,
                new_line: 40,
                kind: "context".to_string(),
                content: "line 40".to_string(),
            },
        ];
        let source = (1..=40)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");

        assert_eq!(expand_all_hunk_context(&mut file, source.as_str()), 39);
        assert!(!hunk_is_expandable(&file, 0));
        assert_eq!(file.lines[1].new_line, 1);
    }

    #[test]
    fn expand_all_preserves_one_sided_rows_from_both_revisions() {
        let mut file = file("src/lib.rs");
        file.lines = vec![
            PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: "@@ -5,1 +4,1 @@".to_string(),
            },
            PullRequestDiffLine {
                old_line: 5,
                new_line: 4,
                kind: "context".to_string(),
                content: "shared tail".to_string(),
            },
        ];

        assert_eq!(
            expand_all_hunk_context_from_sources(
                &mut file,
                "one\ntwo\nbase only\nthree\nshared tail",
                "one\ntwo\nthree\nshared tail",
            ),
            3
        );
        assert!(
            file.lines.iter().any(|line| {
                line.old_line == 3 && line.new_line == 0 && line.kind == "deletion"
            })
        );
        assert!(!file.lines.iter().any(|line| line.kind == "hunk"));
    }

    #[test]
    fn expand_all_includes_source_after_the_final_diff_hunk() {
        let mut file = file("src/lib.rs");
        file.lines = vec![
            PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: "@@ -2,1 +2,1 @@".to_string(),
            },
            PullRequestDiffLine {
                old_line: 2,
                new_line: 0,
                kind: "deletion".to_string(),
                content: "old".to_string(),
            },
            PullRequestDiffLine {
                old_line: 0,
                new_line: 2,
                kind: "addition".to_string(),
                content: "new".to_string(),
            },
        ];

        assert_eq!(
            expand_all_hunk_context_from_sources(
                &mut file,
                "one\nold\nafter\nlast",
                "one\nnew\nafter\nlast",
            ),
            2
        );
        assert!(!file.lines.iter().any(|line| line.kind == "hunk"));
        assert_eq!(file.lines.last().map(|line| line.old_line), Some(4));
        assert_eq!(file.lines.last().map(|line| line.new_line), Some(4));
        assert_eq!(
            file.lines.last().map(|line| line.content.as_str()),
            Some("last")
        );
    }

    #[test]
    fn collapses_expanded_context_back_to_the_original_diff() {
        let mut original = file("src/lib.rs");
        original.lines = vec![
            PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: "@@ -4,1 +4,1 @@".to_string(),
            },
            PullRequestDiffLine {
                old_line: 4,
                new_line: 4,
                kind: "context".to_string(),
                content: "line 4".to_string(),
            },
        ];
        let mut expanded = original.clone();
        assert!(expand_hunk_context(
            &mut expanded,
            0,
            "line 1\nline 2\nline 3\nline 4",
            20,
            HunkExpansionDirection::Up,
        ));

        assert!(restore_original_diff(&mut expanded, &original));
        assert_eq!(expanded, original);
        assert!(!restore_original_diff(&mut expanded, &original));
    }

    #[test]
    fn builds_a_full_source_model_without_mutating_diff_stats() {
        let file = file("src/lib.rs");
        let full = full_source_file(&file, "first\nsecond");
        assert_eq!(full.lines.len(), 2);
        assert_eq!(full.lines[0].new_line, 1);
        assert_eq!(full.lines[1].content, "second");
        assert_eq!(full.added_lines, file.added_lines);
    }
}
