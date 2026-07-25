use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{StreamExt, stream};
use slint::{
    Color, ComponentHandle, Image, Model, ModelRc, ModelTracker, Rgba8Pixel, SharedPixelBuffer,
    VecModel,
};

use crate::git::{
    PullRequestActionState, PullRequestCommit, PullRequestConversationItem, PullRequestDiffDetail,
    PullRequestDiffFile, PullRequestDiffLine, PullRequestMergeMethod, PullRequestReviewAction,
    get_pull_request_action_state, get_pull_request_commit_range_diff, get_pull_request_commits,
    get_pull_request_conversation, get_pull_request_diff, get_repository_file_text,
    merge_pull_request, revert_pull_request, split_commit_message, submit_pull_request_review,
    supports_ignore_whitespace,
};
use crate::http::{self, RequestOptions};
use crate::project::{ProjectRepositoryMetadata, load_repository_metadata};
use crate::{
    MainWindow, Route, WorkflowPrCommit, WorkflowPrConversationItem, WorkflowPrDiffFile,
    WorkflowPrDiffLine, WorkflowPrDiffSelectionHit, WorkflowPrDiffToken,
    WorkflowPrFileExtensionFilter, WorkflowPrTreeItem,
};

use super::bindings::{show_error, show_success};
use super::syntax_highlight::{
    HighlightResources, HighlightedLine, HighlightedToken, character_columns, highlight_diff_lines,
};

const HIGHLIGHT_CACHE_CAPACITY: usize = 4;
const SOURCE_CACHE_CAPACITY: usize = 3;
const RANGE_DIFF_CACHE_CAPACITY: usize = 4;
const VISIBLE_LINE_CACHE_CAPACITY: usize = 64;
const LARGE_DIFF_LINE_THRESHOLD: usize = 400;
const LARGE_DIFF_BYTE_THRESHOLD: usize = 64 * 1024;
// Match GitHub's signed-in improved `/changes/{commit}` Files changed view:
// a directional hunk expansion reveals 21 primary-side source rows. The
// signed-out classic `/commits/{commit}` page uses a different 20-row behavior
// and is intentionally not the UI baseline for this workspace.
const HUNK_CONTEXT_PAGE_SIZE: usize = 21;

struct ConversationAvatarPixels {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[derive(Debug)]
struct BoundedCache<T> {
    capacity: usize,
    entries: HashMap<String, T>,
    order: VecDeque<String>,
}

impl<T> BoundedCache<T> {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    fn remove(&mut self, key: &str) {
        self.entries.remove(key);
        self.order.retain(|candidate| candidate != key);
    }

    fn insert(&mut self, key: String, value: T) {
        self.remove(key.as_str());
        self.entries.insert(key.clone(), value);
        self.order.push_back(key);
        while self.entries.len() > self.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.entries.remove(evicted.as_str());
            }
        }
    }
}

#[derive(Default, Clone, Copy)]
struct OwnedMemoryStats {
    items: usize,
    children: usize,
    text_bytes: usize,
    estimated_heap_bytes: usize,
}

struct LazyPrDiffLineModel {
    file: Arc<PullRequestDiffFile>,
    highlighted: Arc<Vec<HighlightedLine>>,
    rows_projection: Vec<DiffRowProjection>,
    rows: RefCell<LazyLineCache>,
}

impl LazyPrDiffLineModel {
    fn new(
        file: Arc<PullRequestDiffFile>,
        highlighted: Arc<Vec<HighlightedLine>>,
        split: bool,
    ) -> Self {
        let rows_projection = diff_row_projection(file.lines.as_slice(), split);
        Self {
            file,
            highlighted,
            rows_projection,
            rows: RefCell::new(LazyLineCache::new(VISIBLE_LINE_CACHE_CAPACITY)),
        }
    }

    #[cfg(test)]
    fn cached_row_count(&self) -> usize {
        self.rows.borrow().entries.len()
    }
}

impl Model for LazyPrDiffLineModel {
    type Data = WorkflowPrDiffLine;

    fn row_count(&self) -> usize {
        self.rows_projection.len()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        if row >= self.row_count() {
            return None;
        }
        if let Some(data) = self.rows.borrow_mut().get(row) {
            return Some(data);
        }
        let data = match *self.rows_projection.get(row)? {
            DiffRowProjection::Unified(source_index) => to_slint_line(
                &self.file,
                source_index,
                self.file.lines.get(source_index)?,
                self.highlighted.get(source_index)?,
            ),
            DiffRowProjection::Split {
                left,
                right,
                source_index,
                spanning,
            } => to_slint_split_line(
                &self.file,
                left,
                right,
                source_index,
                spanning,
                &self.highlighted,
            ),
        };
        self.rows.borrow_mut().insert(row, data.clone());
        Some(data)
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffRowProjection {
    Unified(usize),
    Split {
        left: Option<usize>,
        right: Option<usize>,
        source_index: usize,
        spanning: bool,
    },
}

fn diff_row_projection(lines: &[PullRequestDiffLine], split: bool) -> Vec<DiffRowProjection> {
    if !split {
        return (0..lines.len()).map(DiffRowProjection::Unified).collect();
    }

    let mut rows = Vec::with_capacity(lines.len());
    let mut index = 0;
    while index < lines.len() {
        let line = &lines[index];
        match line.kind.as_str() {
            "hunk" | "meta" => {
                rows.push(DiffRowProjection::Split {
                    left: Some(index),
                    right: None,
                    source_index: index,
                    spanning: true,
                });
                index += 1;
            }
            "context" => {
                rows.push(DiffRowProjection::Split {
                    left: Some(index),
                    right: Some(index),
                    source_index: index,
                    spanning: false,
                });
                index += 1;
            }
            _ => {
                let block_start = index;
                while index < lines.len()
                    && !matches!(lines[index].kind.as_str(), "hunk" | "meta" | "context")
                {
                    index += 1;
                }
                let deletions = (block_start..index)
                    .filter(|candidate| lines[*candidate].kind == "deletion")
                    .collect::<Vec<_>>();
                let additions = (block_start..index)
                    .filter(|candidate| lines[*candidate].kind == "addition")
                    .collect::<Vec<_>>();
                let row_count = deletions.len().max(additions.len());
                for offset in 0..row_count {
                    let left = deletions.get(offset).copied();
                    let right = additions.get(offset).copied();
                    rows.push(DiffRowProjection::Split {
                        left,
                        right,
                        source_index: left.or(right).unwrap_or(block_start),
                        spanning: false,
                    });
                }
            }
        }
    }
    rows
}

struct LazyLineCache {
    capacity: usize,
    entries: HashMap<usize, WorkflowPrDiffLine>,
    order: VecDeque<usize>,
}

impl LazyLineCache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&mut self, index: usize) -> Option<WorkflowPrDiffLine> {
        let data = self.entries.get(&index)?.clone();
        self.order.retain(|candidate| *candidate != index);
        self.order.push_back(index);
        Some(data)
    }

    fn insert(&mut self, index: usize, data: WorkflowPrDiffLine) {
        self.entries.insert(index, data);
        self.order.retain(|candidate| *candidate != index);
        self.order.push_back(index);
        while self.entries.len() > self.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.entries.remove(&evicted);
            }
        }
    }
}

impl<T: Clone> BoundedCache<T> {
    fn get_cloned(&mut self, key: &str) -> Option<T> {
        let value = self.entries.get(key)?.clone();
        self.order.retain(|candidate| candidate != key);
        self.order.push_back(key.to_string());
        Some(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReviewRequest {
    number: String,
    title: String,
    author: String,
    source_branch: String,
    target_branch: String,
    updated_at: String,
    ignore_whitespace: bool,
}

#[derive(Debug)]
struct ReviewCache {
    project_alias: String,
    request: ReviewRequest,
    metadata: ProjectRepositoryMetadata,
    all_detail: Arc<PullRequestDiffDetail>,
    base_detail: Arc<PullRequestDiffDetail>,
    range_diffs: BoundedCache<Arc<PullRequestDiffDetail>>,
    expanded_files: HashMap<usize, PullRequestDiffFile>,
    loaded_large_diffs: HashSet<usize>,
}

impl ReviewCache {
    fn file(&self, index: usize) -> Option<&PullRequestDiffFile> {
        self.expanded_files
            .get(&index)
            .or_else(|| self.base_detail.files.get(index))
    }

    fn file_mut_cow(&mut self, index: usize) -> Option<&mut PullRequestDiffFile> {
        if !self.expanded_files.contains_key(&index) {
            let original = self.base_detail.files.get(index)?.clone();
            self.expanded_files.insert(index, original);
        }
        self.expanded_files.get_mut(&index)
    }

    fn collapse_file(&mut self, index: usize) -> Option<PullRequestDiffFile> {
        self.expanded_files.remove(&index)?;
        self.base_detail.files.get(index).cloned()
    }
}

#[derive(Clone)]
pub(crate) struct WorkflowPrReviewController {
    project_alias: Arc<Mutex<String>>,
    active_request: Arc<Mutex<Option<ReviewRequest>>>,
    cache: Arc<Mutex<Option<ReviewCache>>>,
    file_filter: Arc<Mutex<String>>,
    selected_extensions: Arc<Mutex<HashSet<String>>>,
    only_manifest_files: Arc<Mutex<bool>>,
    show_viewed_files: Arc<Mutex<bool>>,
    collapsed_folders: Arc<Mutex<HashSet<String>>>,
    highlight_resources: Arc<Mutex<Option<Arc<HighlightResources>>>>,
    highlighted_files: Arc<Mutex<BoundedCache<Arc<Vec<HighlightedLine>>>>>,
    source_files: Arc<Mutex<BoundedCache<String>>>,
    request_sequence: Arc<AtomicU64>,
    highlight_sequence: Arc<AtomicU64>,
    expand_sequence: Arc<AtomicU64>,
    action_sequence: Arc<AtomicU64>,
    conversation_sequence: Arc<AtomicU64>,
    commits_sequence: Arc<AtomicU64>,
    conversation_loaded: Arc<Mutex<bool>>,
    commits_loaded: Arc<Mutex<bool>>,
    commits: Arc<Mutex<Vec<PullRequestCommit>>>,
    pending_open_commit: Arc<Mutex<Option<String>>>,
    commit_detail_return_tab: Arc<Mutex<Option<i32>>>,
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

pub(crate) fn bind_pr_review(ui: &MainWindow) -> WorkflowPrReviewController {
    let controller = WorkflowPrReviewController {
        project_alias: Arc::new(Mutex::new(String::new())),
        active_request: Arc::new(Mutex::new(None)),
        cache: Arc::new(Mutex::new(None)),
        file_filter: Arc::new(Mutex::new(String::new())),
        selected_extensions: Arc::new(Mutex::new(HashSet::new())),
        only_manifest_files: Arc::new(Mutex::new(false)),
        show_viewed_files: Arc::new(Mutex::new(true)),
        collapsed_folders: Arc::new(Mutex::new(HashSet::new())),
        highlight_resources: Arc::new(Mutex::new(None)),
        highlighted_files: Arc::new(Mutex::new(BoundedCache::new(HIGHLIGHT_CACHE_CAPACITY))),
        source_files: Arc::new(Mutex::new(BoundedCache::new(SOURCE_CACHE_CAPACITY))),
        request_sequence: Arc::new(AtomicU64::new(0)),
        highlight_sequence: Arc::new(AtomicU64::new(0)),
        expand_sequence: Arc::new(AtomicU64::new(0)),
        action_sequence: Arc::new(AtomicU64::new(0)),
        conversation_sequence: Arc::new(AtomicU64::new(0)),
        commits_sequence: Arc::new(AtomicU64::new(0)),
        conversation_loaded: Arc::new(Mutex::new(false)),
        commits_loaded: Arc::new(Mutex::new(false)),
        commits: Arc::new(Mutex::new(Vec::new())),
        pending_open_commit: Arc::new(Mutex::new(None)),
        commit_detail_return_tab: Arc::new(Mutex::new(None)),
    };

    let open_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_open(
        move |number, title, author, source_branch, target_branch, updated_at| {
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
            );
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
        if tab != 2 {
            if let Ok(mut return_tab) = tab_controller.commit_detail_return_tab.lock() {
                *return_tab = None;
            }
            ui.set_workflow_pr_review_back_label("Back to pull requests".into());
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
            ui.set_workflow_pr_review_commit_range_label("All commits".into());
            reload_controller.open(&ui, request);
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
                ui.set_workflow_pr_review_commit_range_label("All commits".into());
                diff_settings_controller.open(&ui, request);
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
            return;
        }
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(path)) {
            Ok(()) => {
                if let Some(ui) = ui_weak.upgrade() {
                    show_success(&ui, "Copied file path.");
                }
            }
            Err(error) => {
                log::warn!("[Workflow][PR Review] Failed to copy file path: {error}");
                if let Some(ui) = ui_weak.upgrade() {
                    show_error(&ui, "Failed to copy the file path.");
                }
            }
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_copy_commit_sha(move |sha| {
        let sha = sha.trim();
        if sha.is_empty() {
            return;
        }
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(sha)) {
            Ok(()) => {
                if let Some(ui) = ui_weak.upgrade() {
                    show_success(&ui, "Copied commit SHA.");
                }
            }
            Err(error) => {
                log::warn!("[Workflow][PR Review] Failed to copy commit SHA: {error}");
                if let Some(ui) = ui_weak.upgrade() {
                    show_error(&ui, "Failed to copy the commit SHA.");
                }
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
            return;
        }
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(branch)) {
            Ok(()) => {
                if let Some(ui) = ui_weak.upgrade() {
                    show_success(&ui, "Copied source branch.");
                }
            }
            Err(error) => {
                log::warn!("[Workflow][PR Review] Failed to copy source branch: {error}");
                if let Some(ui) = ui_weak.upgrade() {
                    show_error(&ui, "Failed to copy the source branch.");
                }
            }
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_copy_diff_selection(
        move |start_line, start_col, end_line, end_col, side| {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let Some(text) = diff_selection_text(
                &ui.get_workflow_pr_review_lines(),
                start_line,
                start_col,
                end_line,
                end_col,
                side.as_str(),
            ) else {
                return;
            };
            if text.is_empty() {
                return;
            }
            // Silent like GitHub's in-diff copy — no toast.
            if let Err(error) =
                arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text))
            {
                log::warn!("[Workflow][PR Review] Failed to copy diff selection: {error}");
            }
        },
    );

    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_resolve_diff_selection(
        move |start_line, pointer_x, pointer_y, start_height, code_width, cell_width, side| {
            let Some(ui) = ui_weak.upgrade() else {
                return WorkflowPrDiffSelectionHit::default();
            };
            resolve_diff_selection_hit(
                &ui.get_workflow_pr_review_lines(),
                start_line,
                pointer_x,
                pointer_y,
                start_height,
                code_width,
                cell_width,
                side.as_str(),
            )
        },
    );

    let submit_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_submit_review(move |action, body| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        submit_controller.submit_review(&ui, action.as_str(), body.as_str());
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
    fn navigate_back(&self, ui: &MainWindow) {
        let return_tab = self
            .commit_detail_return_tab
            .lock()
            .ok()
            .and_then(|mut return_tab| return_tab.take());
        if let Some(tab) = return_tab {
            ui.set_workflow_pr_review_tab(tab);
            ui.set_workflow_pr_review_back_label("Back to pull requests".into());
            self.load_tab(ui, tab);
            return;
        }

        self.log_memory_diagnostics("review_close.before_clear");
        self.request_sequence.fetch_add(1, Ordering::SeqCst);
        self.highlight_sequence.fetch_add(1, Ordering::SeqCst);
        self.expand_sequence.fetch_add(1, Ordering::SeqCst);
        self.action_sequence.fetch_add(1, Ordering::SeqCst);
        self.conversation_sequence.fetch_add(1, Ordering::SeqCst);
        self.commits_sequence.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut request) = self.active_request.lock() {
            *request = None;
        }
        if let Ok(mut cache) = self.cache.lock() {
            *cache = None;
        }
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
        reset_review_ui(ui);
        ui.invoke_navigate_back();
        self.log_memory_diagnostics("review_close.after_clear");
    }

    pub(crate) fn select_project(&self, ui: &MainWindow, project_alias: String) {
        self.log_memory_diagnostics("project_switch.before_clear");
        self.request_sequence.fetch_add(1, Ordering::SeqCst);
        self.highlight_sequence.fetch_add(1, Ordering::SeqCst);
        self.expand_sequence.fetch_add(1, Ordering::SeqCst);
        self.action_sequence.fetch_add(1, Ordering::SeqCst);
        self.conversation_sequence.fetch_add(1, Ordering::SeqCst);
        self.commits_sequence.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut alias) = self.project_alias.lock() {
            *alias = project_alias;
        }
        if let Ok(mut request) = self.active_request.lock() {
            *request = None;
        }
        if let Ok(mut cache) = self.cache.lock() {
            *cache = None;
        }
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
        reset_review_ui(ui);
        self.log_memory_diagnostics("project_switch.after_clear");
    }

    fn open(&self, ui: &MainWindow, request: ReviewRequest) {
        let project_alias = self
            .project_alias
            .lock()
            .map(|alias| alias.clone())
            .unwrap_or_default();
        if project_alias.is_empty() {
            ui.set_workflow_pr_review_visible(true);
            ui.set_workflow_pr_review_error(
                "The current project has no repository context.".into(),
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
        if is_new_pull_request {
            self.log_memory_diagnostics("new_review.before_cache_clear");
            if let Ok(mut filter) = self.file_filter.lock() {
                filter.clear();
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
            self.conversation_sequence.fetch_add(1, Ordering::SeqCst);
            self.commits_sequence.fetch_add(1, Ordering::SeqCst);
            if let Ok(mut loaded) = self.conversation_loaded.lock() {
                *loaded = false;
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
            ui.set_workflow_pr_review_tab(2);
            ui.set_workflow_pr_review_conversation_count(0);
            ui.set_workflow_pr_review_commit_count(0);
            ui.set_workflow_pr_review_conversation_items(empty_conversation_model());
            ui.set_workflow_pr_review_conversation_content_height(0);
            ui.set_workflow_pr_review_expanded_commit_body_height(0);
            ui.set_workflow_pr_review_commits(empty_commit_model());
            ui.set_workflow_pr_review_conversation_error("".into());
            ui.set_workflow_pr_review_commits_error("".into());
            ui.set_workflow_pr_review_commit_range_start(-1);
            ui.set_workflow_pr_review_commit_range_end(-1);
            ui.set_workflow_pr_review_commit_range_label("All commits".into());
            ui.set_workflow_pr_review_file_filter("".into());
            reset_action_ui(ui);
            self.log_memory_diagnostics("new_review.after_cache_clear");
        }

        apply_review_header(ui, &request);
        ui.set_workflow_pr_review_visible(true);
        ui.invoke_navigate(Route::PrReview);
        ui.set_workflow_pr_review_error("".into());
        if let Ok(mut active_request) = self.active_request.lock() {
            *active_request = Some(request.clone());
        }
        let cached = self.cache.lock().ok().and_then(|guard| {
            guard
                .as_ref()
                .filter(|cache| cache.project_alias == project_alias && cache.request == request)
                .map(|cache| cache.base_detail.clone())
        });
        if let Some(detail) = cached {
            ui.set_workflow_pr_review_loading(false);
            self.apply_detail(ui, &detail);
            self.refresh_action_state(ui);
            self.log_memory_diagnostics("review_open.cache_hit_applied");
            return;
        }

        self.highlight_sequence.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut highlighted) = self.highlighted_files.lock() {
            highlighted.clear();
        }

        ui.set_workflow_pr_review_loading(true);
        ui.set_workflow_pr_review_code_loading(false);
        ui.set_workflow_pr_review_large_diff_deferred(false);
        ui.set_workflow_pr_review_large_diff_loading(false);
        ui.set_workflow_pr_review_files(empty_file_model());
        ui.set_workflow_pr_review_tree_items(empty_tree_model());
        ui.set_workflow_pr_review_lines(empty_line_model());
        ui.set_workflow_pr_review_selected_file_index(-1);
        ui.set_workflow_pr_review_selected_file_path("".into());
        ui.set_workflow_pr_review_context_expanded(false);

        let sequence = self.request_sequence.fetch_add(1, Ordering::SeqCst) + 1;
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
                    let ignore_whitespace_supported = supports_ignore_whitespace(
                        metadata.repo_type.as_str(),
                        metadata.remote_repo.as_str(),
                    );
                    get_pull_request_diff(
                        metadata.repo_type.as_str(),
                        metadata.repo_project_id.as_str(),
                        metadata.remote_repo.as_str(),
                        request.number.as_str(),
                        request.ignore_whitespace && ignore_whitespace_supported,
                    )
                    .await
                    .map(|detail| (metadata, detail, ignore_whitespace_supported))
                }
                Err(error) => Err(error),
            };

            let _ = slint::invoke_from_event_loop(move || {
                if controller.request_sequence.load(Ordering::SeqCst) != sequence {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_workflow_pr_review_loading(false);
                match result {
                    Ok((metadata, detail, ignore_whitespace_supported)) => {
                        ui.set_workflow_pr_review_ignore_whitespace_supported(
                            ignore_whitespace_supported,
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
                        let detail = Arc::new(detail);
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
                        log::warn!("[Workflow][PR Review] Failed to load diff: {error}");
                        ui.set_workflow_pr_review_error(error.into());
                    }
                }
            });
        });
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
        let sequence = self.conversation_sequence.fetch_add(1, Ordering::SeqCst) + 1;
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
                Err(error) => Err(error),
            };
            let avatars = match &result {
                Ok(items) => {
                    tokio::time::timeout(Duration::from_secs(3), load_conversation_avatars(items))
                        .await
                        .unwrap_or_default()
                }
                Err(_) => HashMap::new(),
            };
            let _ = slint::invoke_from_event_loop(move || {
                if controller.conversation_sequence.load(Ordering::SeqCst) != sequence {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_workflow_pr_review_conversation_loading(false);
                match result {
                    Ok(items) => {
                        ui.set_workflow_pr_review_conversation_content_height(
                            conversation_content_height(&items),
                        );
                        ui.set_workflow_pr_review_expanded_commit_body_height(0);
                        ui.set_workflow_pr_review_conversation_items(conversation_model(
                            &items, &avatars,
                        ));
                        if let Ok(mut loaded) = controller.conversation_loaded.lock() {
                            *loaded = true;
                        }
                    }
                    Err(error) => {
                        log::warn!("[Workflow][PR Review] Failed to load conversation: {error}");
                        ui.set_workflow_pr_review_conversation_error(
                            "The provider could not load this pull request conversation.".into(),
                        );
                    }
                }
            });
        });
    }

    fn refresh_conversation_after_remote_action(&self, ui: &MainWindow) {
        self.conversation_sequence.fetch_add(1, Ordering::SeqCst);
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
        let sequence = self.commits_sequence.fetch_add(1, Ordering::SeqCst) + 1;
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
                Err(error) => Err(error),
            };
            let commits_and_avatars = match result {
                Ok(commits) => {
                    let avatars =
                        tokio::time::timeout(Duration::from_secs(3), load_commit_avatars(&commits))
                            .await
                            .ok()
                            .unwrap_or_default();
                    Ok((commits, avatars))
                }
                Err(error) => Err(error),
            };
            let _ = slint::invoke_from_event_loop(move || {
                if controller.commits_sequence.load(Ordering::SeqCst) != sequence {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_workflow_pr_review_commits_loading(false);
                match commits_and_avatars {
                    Ok((commits, avatars)) => {
                        // Provider summaries normally populate the badge before
                        // activation. Keep the successfully loaded list as a
                        // final compatibility fallback for older enterprise APIs.
                        let current = ui.get_workflow_pr_review_commit_count();
                        ui.set_workflow_pr_review_commit_count(current.max(commits.len() as i32));
                        ui.set_workflow_pr_review_commits(commit_model(&commits, &avatars));
                        if let Ok(mut stored) = controller.commits.lock() {
                            *stored = commits;
                        }
                        if let Ok(mut loaded) = controller.commits_loaded.lock() {
                            *loaded = true;
                        }
                        let pending_sha = controller
                            .pending_open_commit
                            .lock()
                            .ok()
                            .and_then(|mut pending| pending.take());
                        if let Some(sha) = pending_sha {
                            controller.open_commit(&ui, sha.as_str());
                        }
                    }
                    Err(error) => {
                        let was_opening_commit = controller
                            .pending_open_commit
                            .lock()
                            .map(|mut pending| pending.take().is_some())
                            .unwrap_or(false);
                        log::warn!("[Workflow][PR Review] Failed to load commits: {error}");
                        ui.set_workflow_pr_review_commits_error(
                            "The provider could not load this pull request's commits.".into(),
                        );
                        if was_opening_commit {
                            show_error(&ui, "The selected commit details could not be loaded.");
                        }
                    }
                }
            });
        });
    }

    fn open_commit(&self, ui: &MainWindow, sha: &str) {
        let sha = sha.trim();
        if sha.is_empty() {
            return;
        }
        let source_tab = ui.get_workflow_pr_review_tab();
        if let Some(source_tab) = commit_detail_source_tab(source_tab) {
            if let Ok(mut return_tab) = self.commit_detail_return_tab.lock() {
                *return_tab = Some(source_tab);
            }
            ui.set_workflow_pr_review_back_label(
                if source_tab == 0 {
                    "Back to Conversation"
                } else {
                    "Back to Commits"
                }
                .into(),
            );
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
            show_error(ui, "The selected commit is no longer in this pull request.");
            return;
        }
        if let Ok(mut pending) = self.pending_open_commit.lock() {
            *pending = Some(sha.to_string());
        }
        self.load_commits(ui);
    }

    fn apply_commit_range(&self, ui: &MainWindow, start: i32, end: i32) {
        if start < 0 || end < 0 {
            self.request_sequence.fetch_add(1, Ordering::SeqCst);
            let detail = self
                .cache
                .lock()
                .ok()
                .and_then(|cache| cache.as_ref().map(|cache| cache.all_detail.clone()));
            if let Some(detail) = detail {
                self.activate_commit_range(ui, detail);
                ui.set_workflow_pr_review_commit_range_start(-1);
                ui.set_workflow_pr_review_commit_range_end(-1);
                ui.set_workflow_pr_review_commit_range_label("All commits".into());
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
                "Load the pull request commits before selecting a range.",
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
                self.request_sequence.fetch_add(1, Ordering::SeqCst);
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
                "The provider did not return enough commit data for this range.",
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
            self.request_sequence.fetch_add(1, Ordering::SeqCst);
            self.activate_commit_range(ui, detail);
            ui.set_workflow_pr_review_commit_range_start(display_start as i32);
            ui.set_workflow_pr_review_commit_range_end(display_end as i32);
            ui.set_workflow_pr_review_commit_range_label(label.into());
            ui.set_workflow_pr_review_error("".into());
            return;
        }

        let sequence = self.request_sequence.fetch_add(1, Ordering::SeqCst) + 1;
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
                if controller.request_sequence.load(Ordering::SeqCst) != sequence {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_workflow_pr_review_loading(false);
                match result {
                    Ok(detail) => {
                        let detail = Arc::new(detail);
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
                            "The provider could not load the selected commit range.".into(),
                        );
                    }
                }
            });
        });
    }

    fn activate_commit_range(&self, ui: &MainWindow, detail: Arc<PullRequestDiffDetail>) {
        self.highlight_sequence.fetch_add(1, Ordering::SeqCst);
        self.expand_sequence.fetch_add(1, Ordering::SeqCst);
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

    fn action_context(&self) -> Option<(ProjectRepositoryMetadata, String)> {
        self.cache.lock().ok().and_then(|guard| {
            let cache = guard.as_ref()?;
            Some((cache.metadata.clone(), cache.request.number.clone()))
        })
    }

    fn refresh_action_state(&self, ui: &MainWindow) {
        let Some((metadata, pull_request_iid)) = self.action_context() else {
            return;
        };
        let sequence = self.action_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        ui.set_workflow_pr_action_loading(true);
        if ui.get_workflow_pr_merge_readiness_label().trim().is_empty() {
            ui.set_workflow_pr_merge_readiness_kind("checking".into());
            ui.set_workflow_pr_merge_readiness_label("Checking mergeability…".into());
            ui.set_workflow_pr_merge_readiness_detail(
                "Waiting for the repository provider to calculate merge readiness.".into(),
            );
        }
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let result = get_pull_request_action_state(
                metadata.repo_type.as_str(),
                metadata.repo_project_id.as_str(),
                metadata.remote_repo.as_str(),
                pull_request_iid.as_str(),
            )
            .await;
            let _ = slint::invoke_from_event_loop(move || {
                if controller.action_sequence.load(Ordering::SeqCst) != sequence {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_workflow_pr_action_loading(false);
                match result {
                    Ok(state) => apply_action_state(&ui, &state),
                    Err(error) => {
                        log::warn!(
                            "[Workflow][PR Review] Failed to load remote action state: {error}"
                        );
                        reset_action_capabilities(&ui);
                        ui.set_workflow_pr_merge_readiness_kind("unavailable".into());
                        ui.set_workflow_pr_merge_readiness_label("Merge status unavailable".into());
                        ui.set_workflow_pr_merge_readiness_detail(
                            "The repository provider could not return merge readiness.".into(),
                        );
                        ui.set_workflow_pr_merge_block_reason(error.into());
                    }
                }
            });
        });
    }

    fn submit_review(&self, ui: &MainWindow, action: &str, body: &str) {
        let Some(action) = PullRequestReviewAction::parse(action) else {
            show_error(ui, "Choose a valid review action.");
            return;
        };
        let allowed = match action {
            PullRequestReviewAction::Comment => ui.get_workflow_pr_can_comment(),
            PullRequestReviewAction::Approve => ui.get_workflow_pr_can_approve(),
            PullRequestReviewAction::RequestChanges => ui.get_workflow_pr_can_request_changes(),
        };
        if !allowed {
            show_error(
                ui,
                "This repository provider does not support that review action.",
            );
            return;
        }
        if action != PullRequestReviewAction::Approve && body.trim().is_empty() {
            show_error(ui, "Enter a review comment before submitting.");
            return;
        }
        let Some((metadata, pull_request_iid)) = self.action_context() else {
            show_error(ui, "The pull request repository context is unavailable.");
            return;
        };
        let body = body.to_string();
        let sequence = self.action_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        ui.set_workflow_pr_action_loading(true);
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let result = submit_pull_request_review(
                metadata.repo_type.as_str(),
                metadata.repo_project_id.as_str(),
                metadata.remote_repo.as_str(),
                pull_request_iid.as_str(),
                action,
                body.as_str(),
            )
            .await;
            let _ = slint::invoke_from_event_loop(move || {
                if controller.action_sequence.load(Ordering::SeqCst) != sequence {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_workflow_pr_action_loading(false);
                match result {
                    Ok(()) => {
                        ui.set_workflow_pr_review_dialog_open(false);
                        if action == PullRequestReviewAction::Comment {
                            let count = ui.get_workflow_pr_review_conversation_count();
                            ui.set_workflow_pr_review_conversation_count(count.saturating_add(1));
                        }
                        ui.set_workflow_pr_remote_review_state(
                            match action {
                                PullRequestReviewAction::Comment => "commented",
                                PullRequestReviewAction::Approve => "approved",
                                PullRequestReviewAction::RequestChanges => "rejected",
                            }
                            .into(),
                        );
                        show_success(&ui, "Review submitted to the remote repository.");
                        controller.refresh_action_state(&ui);
                        controller.refresh_conversation_after_remote_action(&ui);
                    }
                    Err(error) => {
                        log::warn!("[Workflow][PR Review] Failed to submit review: {error}");
                        show_error(&ui, error.as_str());
                    }
                }
            });
        });
    }

    fn merge(&self, ui: &MainWindow, method: &str, remove_source_branch: bool) {
        if !ui.get_workflow_pr_can_merge() {
            let reason = ui.get_workflow_pr_merge_block_reason();
            show_error(
                ui,
                if reason.trim().is_empty() {
                    "This pull request is not currently mergeable."
                } else {
                    reason.as_str()
                },
            );
            return;
        }
        let Some(method) = PullRequestMergeMethod::parse(method) else {
            show_error(ui, "Choose a valid merge method.");
            return;
        };
        let method_allowed = match method {
            PullRequestMergeMethod::Merge => ui.get_workflow_pr_merge_method_merge(),
            PullRequestMergeMethod::Squash => ui.get_workflow_pr_merge_method_squash(),
            PullRequestMergeMethod::Rebase => ui.get_workflow_pr_merge_method_rebase(),
            PullRequestMergeMethod::FastForward => ui.get_workflow_pr_merge_method_fast_forward(),
        };
        if !method_allowed {
            show_error(ui, "That merge method is disabled for this repository.");
            return;
        }
        let Some((metadata, pull_request_iid)) = self.action_context() else {
            show_error(ui, "The pull request repository context is unavailable.");
            return;
        };
        let sequence = self.action_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        ui.set_workflow_pr_action_loading(true);
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let result = merge_pull_request(
                metadata.repo_type.as_str(),
                metadata.repo_project_id.as_str(),
                metadata.remote_repo.as_str(),
                pull_request_iid.as_str(),
                method,
                remove_source_branch,
            )
            .await;
            let _ = slint::invoke_from_event_loop(move || {
                if controller.action_sequence.load(Ordering::SeqCst) != sequence {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_workflow_pr_action_loading(false);
                match result {
                    Ok(()) => {
                        ui.set_workflow_pr_merge_dialog_open(false);
                        ui.set_workflow_pr_can_merge(false);
                        ui.set_workflow_pr_merge_readiness_kind("merged".into());
                        ui.set_workflow_pr_merge_readiness_label("Merged".into());
                        ui.set_workflow_pr_merge_readiness_detail(
                            "This pull request has been merged successfully.".into(),
                        );
                        ui.set_workflow_pr_merge_block_reason(
                            "This pull request has been merged.".into(),
                        );
                        show_success(&ui, "Pull request merged successfully.");
                        controller.refresh_action_state(&ui);
                        controller.refresh_conversation_after_remote_action(&ui);
                    }
                    Err(error) => {
                        log::warn!("[Workflow][PR Review] Failed to merge pull request: {error}");
                        show_error(&ui, error.as_str());
                        controller.refresh_action_state(&ui);
                    }
                }
            });
        });
    }

    fn revert(&self, ui: &MainWindow) {
        if !ui.get_workflow_pr_can_revert() {
            let reason = ui.get_workflow_pr_revert_block_reason();
            show_error(
                ui,
                if reason.trim().is_empty() {
                    "This merged pull request cannot be reverted automatically."
                } else {
                    reason.as_str()
                },
            );
            return;
        }
        let Some((metadata, pull_request_iid)) = self.action_context() else {
            show_error(ui, "The pull request repository context is unavailable.");
            return;
        };
        let sequence = self.action_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        ui.set_workflow_pr_action_loading(true);
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let result = revert_pull_request(
                metadata.repo_type.as_str(),
                metadata.repo_project_id.as_str(),
                metadata.remote_repo.as_str(),
                pull_request_iid.as_str(),
            )
            .await;
            let _ = slint::invoke_from_event_loop(move || {
                if controller.action_sequence.load(Ordering::SeqCst) != sequence {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_workflow_pr_action_loading(false);
                match result {
                    Ok(revert) => {
                        ui.set_workflow_pr_revert_dialog_open(false);
                        ui.set_workflow_pr_can_revert(false);
                        ui.set_workflow_pr_revert_block_reason(
                            "The revert action already completed in this review session.".into(),
                        );
                        show_success(&ui, revert.message.as_str());
                        log::info!(
                            "[Workflow][PR Review] Completed revert action reference={} title_present={} url_present={}",
                            revert.number,
                            !revert.title.trim().is_empty(),
                            !revert.web_url.trim().is_empty()
                        );
                    }
                    Err(error) => {
                        log::warn!("[Workflow][PR Review] Revert action failed: {error}");
                        show_error(&ui, error.as_str());
                    }
                }
            });
        });
    }

    fn select_file(&self, ui: &MainWindow, index: i32) {
        ui.set_workflow_pr_review_large_diff_loading(false);
        let index = index.max(0) as usize;
        let selected = self.cache.lock().ok().and_then(|guard| {
            let cache = guard.as_ref()?;
            Some((
                Arc::new(cache.file(index)?.clone()),
                cache.expanded_files.contains_key(&index),
                cache.loaded_large_diffs.contains(&index),
            ))
        });
        let Some((file, context_expanded, large_diff_loaded)) = selected else {
            return;
        };
        ui.set_workflow_pr_review_viewing_file(false);
        ui.set_workflow_pr_review_has_expandable_hunks(false);
        ui.set_workflow_pr_review_context_expanded(context_expanded);
        let reset_scroll = ui.get_workflow_pr_review_selected_file_path().as_str() != file.path;
        apply_file_header(ui, index, &file);
        if is_large_diff(&file) && !large_diff_loaded {
            self.highlight_sequence.fetch_add(1, Ordering::SeqCst);
            ui.set_workflow_pr_review_lines(empty_line_model());
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
        let sequence = self.highlight_sequence.fetch_add(1, Ordering::SeqCst) + 1;
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
                if controller.highlight_sequence.load(Ordering::SeqCst) != sequence {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                if ui.get_workflow_pr_review_selected_file_path().as_str() != file.path {
                    return;
                }
                let Ok(highlighted) = highlighted else {
                    ui.set_workflow_pr_review_code_loading(false);
                    ui.set_workflow_pr_review_large_diff_loading(false);
                    ui.set_workflow_pr_review_error(
                        "Failed to render syntax highlighting for this file.".into(),
                    );
                    return;
                };
                let highlighted = Arc::new(highlighted);
                if let Ok(mut cached) = controller.highlighted_files.lock() {
                    cached.insert(file.path.clone(), highlighted.clone());
                }
                apply_highlighted_file(&ui, file, highlighted, reset_scroll);
                ui.set_workflow_pr_review_code_loading(false);
                ui.set_workflow_pr_review_large_diff_loading(false);
                controller.log_memory_diagnostics("file_highlight.generated_and_applied");
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
        if let Ok(mut selected) = self.selected_extensions.lock() {
            if !selected.insert(extension.clone()) {
                selected.remove(extension.as_str());
            }
        }
        self.refresh_tree(ui);
    }

    fn toggle_folder(&self, ui: &MainWindow, path: String) {
        if path.is_empty() {
            return;
        }
        if let Ok(mut folders) = self.collapsed_folders.lock() {
            if !folders.insert(path.clone()) {
                folders.remove(path.as_str());
            }
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
                cache.file(selected_index)?.clone(),
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
                "This diff has no immutable source reference for expanding context.".into(),
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

        let sequence = self.expand_sequence.fetch_add(1, Ordering::SeqCst) + 1;
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
                if controller.expand_sequence.load(Ordering::SeqCst) != sequence {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
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
                cache.file(selected_index)?.clone(),
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
                "This diff has no immutable source reference for expanding context.".into(),
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
        let sequence = self.expand_sequence.fetch_add(1, Ordering::SeqCst) + 1;
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
                if controller.expand_sequence.load(Ordering::SeqCst) != sequence {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
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
                        ui.set_workflow_pr_review_code_loading(false);
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
            let file = cache.file_mut_cow(file_index)?;
            (expand_all_hunk_context_from_sources(file, base_source, head_source) > 0)
                .then(|| file.clone())
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
        self.expand_sequence.fetch_add(1, Ordering::SeqCst);
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
                cache.file(selected_index)?.clone(),
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
            show_error(ui, "This file has no immutable source reference.");
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
        let sequence = self.expand_sequence.fetch_add(1, Ordering::SeqCst) + 1;
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
                if controller.expand_sequence.load(Ordering::SeqCst) != sequence {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
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
                        ui.set_workflow_pr_review_code_loading(false);
                        log::warn!("[Workflow][PR Review] Failed to view file: {error}");
                        show_error(&ui, "Failed to load the complete file.");
                    }
                }
            });
        });
    }

    fn render_full_file(
        &self,
        ui: &MainWindow,
        selected_index: usize,
        file: PullRequestDiffFile,
        source: String,
    ) {
        let full_file = Arc::new(full_source_file(&file, source.as_str()));
        ui.set_workflow_pr_review_viewing_file(true);
        ui.set_workflow_pr_review_has_expandable_hunks(false);
        ui.set_workflow_pr_review_context_expanded(false);
        ui.set_workflow_pr_review_code_loading(true);
        let sequence = self.highlight_sequence.fetch_add(1, Ordering::SeqCst) + 1;
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
                if controller.highlight_sequence.load(Ordering::SeqCst) != sequence {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                if ui.get_workflow_pr_review_selected_file_index() != selected_index as i32
                    || !ui.get_workflow_pr_review_viewing_file()
                {
                    return;
                }
                match highlighted {
                    Ok(highlighted) => {
                        apply_highlighted_file(&ui, full_file, Arc::new(highlighted), true);
                        ui.set_workflow_pr_review_code_loading(false);
                        controller.log_memory_diagnostics("full_file.highlighted_and_applied");
                    }
                    Err(error) => {
                        ui.set_workflow_pr_review_code_loading(false);
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
        let updated_file = self.cache.lock().ok().and_then(|mut cache| {
            let cache = cache.as_mut()?;
            let file = cache.file_mut_cow(file_index)?;
            let limit = if direction == HunkExpansionDirection::All {
                i32::MAX as usize
            } else {
                HUNK_CONTEXT_PAGE_SIZE
            };
            expand_hunk_context_from_sources(
                file,
                line_index,
                base_source,
                head_source,
                limit,
                direction,
            )
            .then(|| file.clone())
        });
        let Some(file) = updated_file else {
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

    fn apply_detail(&self, ui: &MainWindow, detail: &PullRequestDiffDetail) {
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
        let (base_stats, override_stats) = self
            .cache
            .lock()
            .ok()
            .and_then(|cache| {
                cache.as_ref().map(|cache| {
                    (
                        diff_storage_stats(&cache.base_detail.files),
                        diff_override_storage_stats(&cache.expanded_files),
                    )
                })
            })
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
            "[Workflow][PR Review][Memory] reason={} base_files={} base_lines={} base_text_bytes={} base_heap_estimate_bytes={} override_files={} override_lines={} override_text_bytes={} override_heap_estimate_bytes={} highlight_resources_loaded={} highlight_cache_entries={} highlight_lines={} highlight_tokens={} highlight_text_bytes={} highlight_heap_estimate_bytes={} source_cache_entries={} source_text_bytes={} source_heap_estimate_bytes={}",
            reason,
            base_stats.items,
            base_stats.children,
            base_stats.text_bytes,
            base_stats.estimated_heap_bytes,
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

fn diff_storage_stats(files: &[PullRequestDiffFile]) -> OwnedMemoryStats {
    let mut stats = OwnedMemoryStats {
        items: files.len(),
        estimated_heap_bytes: std::mem::size_of_val(files),
        ..OwnedMemoryStats::default()
    };
    for file in files {
        stats.text_bytes = stats
            .text_bytes
            .saturating_add(file.path.len())
            .saturating_add(file.old_path.len())
            .saturating_add(file.change_type.len());
        stats.estimated_heap_bytes = stats
            .estimated_heap_bytes
            .saturating_add(file.path.capacity())
            .saturating_add(file.old_path.capacity())
            .saturating_add(file.change_type.capacity())
            .saturating_add(
                file.lines
                    .capacity()
                    .saturating_mul(std::mem::size_of::<PullRequestDiffLine>()),
            );
        stats.children = stats.children.saturating_add(file.lines.len());
        for line in &file.lines {
            stats.text_bytes = stats
                .text_bytes
                .saturating_add(line.kind.len())
                .saturating_add(line.content.len());
            stats.estimated_heap_bytes = stats
                .estimated_heap_bytes
                .saturating_add(line.kind.capacity())
                .saturating_add(line.content.capacity());
        }
    }
    stats
}

fn diff_override_storage_stats(files: &HashMap<usize, PullRequestDiffFile>) -> OwnedMemoryStats {
    let mut stats = OwnedMemoryStats {
        estimated_heap_bytes: files.capacity().saturating_mul(
            std::mem::size_of::<usize>() + std::mem::size_of::<PullRequestDiffFile>(),
        ),
        ..OwnedMemoryStats::default()
    };
    for file in files.values() {
        let file_stats = diff_storage_stats(std::slice::from_ref(file));
        stats.items = stats.items.saturating_add(file_stats.items);
        stats.children = stats.children.saturating_add(file_stats.children);
        stats.text_bytes = stats.text_bytes.saturating_add(file_stats.text_bytes);
        stats.estimated_heap_bytes = stats
            .estimated_heap_bytes
            .saturating_add(file_stats.estimated_heap_bytes);
    }
    stats
}

fn highlighted_cache_stats(cache: &BoundedCache<Arc<Vec<HighlightedLine>>>) -> OwnedMemoryStats {
    let mut stats = OwnedMemoryStats::default();
    for (key, lines) in &cache.entries {
        stats.estimated_heap_bytes = stats
            .estimated_heap_bytes
            .saturating_add(key.capacity())
            .saturating_add(
                lines
                    .capacity()
                    .saturating_mul(std::mem::size_of::<HighlightedLine>()),
            );
        stats.items = stats.items.saturating_add(lines.len());
        for line in lines.iter() {
            stats.children = stats.children.saturating_add(line.tokens.len());
            stats.estimated_heap_bytes = stats.estimated_heap_bytes.saturating_add(
                line.tokens
                    .capacity()
                    .saturating_mul(std::mem::size_of::<HighlightedToken>()),
            );
            for token in &line.tokens {
                stats.text_bytes = stats.text_bytes.saturating_add(token.text.len());
                stats.estimated_heap_bytes = stats
                    .estimated_heap_bytes
                    .saturating_add(token.text.capacity());
            }
        }
    }
    stats
}

fn source_cache_stats(cache: &BoundedCache<String>) -> OwnedMemoryStats {
    let mut stats = OwnedMemoryStats::default();
    for (key, source) in &cache.entries {
        stats.items = stats.items.saturating_add(1);
        stats.text_bytes = stats.text_bytes.saturating_add(source.len());
        stats.estimated_heap_bytes = stats
            .estimated_heap_bytes
            .saturating_add(key.capacity())
            .saturating_add(source.capacity());
    }
    stats
}

fn apply_review_header(ui: &MainWindow, request: &ReviewRequest) {
    ui.set_workflow_pr_review_number(request.number.clone().into());
    ui.set_workflow_pr_review_title(request.title.clone().into());
    ui.set_workflow_pr_review_author(request.author.clone().into());
    ui.set_workflow_pr_review_source_branch(request.source_branch.clone().into());
    ui.set_workflow_pr_review_target_branch(request.target_branch.clone().into());
    ui.set_workflow_pr_review_updated_at(
        super::pr_list::format_github_relative_time(request.updated_at.as_str()).into(),
    );
    ui.set_workflow_pr_review_updated_at_tooltip(
        super::pr_list::format_github_absolute_tooltip(request.updated_at.as_str()).into(),
    );
    ui.set_workflow_pr_review_ignore_whitespace(request.ignore_whitespace);
}

fn apply_detail_summary(ui: &MainWindow, detail: &PullRequestDiffDetail) {
    let files = detail.files.iter().map(to_slint_file).collect::<Vec<_>>();
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
    ui.set_workflow_pr_review_error("".into());
}

fn clear_selected_file(ui: &MainWindow) {
    ui.set_workflow_pr_review_selected_file_index(-1);
    ui.set_workflow_pr_review_selected_file_path("".into());
    ui.set_workflow_pr_review_selected_file_added_lines(0);
    ui.set_workflow_pr_review_selected_file_removed_lines(0);
    ui.set_workflow_pr_review_max_code_columns(0);
    ui.set_workflow_pr_review_longest_code_line("".into());
    ui.set_workflow_pr_review_lines(empty_line_model());
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
    file: Arc<PullRequestDiffFile>,
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
    ui.set_workflow_pr_review_lines(ModelRc::new(LazyPrDiffLineModel::new(
        file.clone(),
        highlighted,
        ui.get_workflow_pr_review_diff_view().as_str() == "split",
    )));
    ui.set_workflow_pr_review_has_expandable_hunks(
        !ui.get_workflow_pr_review_viewing_file()
            && file
                .lines
                .iter()
                .enumerate()
                .any(|(index, _)| hunk_is_expandable(&file, index)),
    );
    if reset_scroll {
        ui.set_workflow_pr_review_line_model_revision(
            ui.get_workflow_pr_review_line_model_revision()
                .wrapping_add(1),
        );
    }
}

fn to_slint_file(file: &PullRequestDiffFile) -> WorkflowPrDiffFile {
    WorkflowPrDiffFile {
        path: file.path.clone().into(),
        old_path: file.old_path.clone().into(),
        change_type: file.change_type.clone().into(),
        added_lines: file.added_lines as i32,
        removed_lines: file.removed_lines as i32,
        viewed: file.viewed,
        line_count: file.lines.len() as i32,
    }
}

fn to_slint_line(
    file: &PullRequestDiffFile,
    index: usize,
    line: &PullRequestDiffLine,
    highlighted: &HighlightedLine,
) -> WorkflowPrDiffLine {
    let tokens = highlighted
        .tokens
        .iter()
        .map(|token| WorkflowPrDiffToken {
            text: token.text.clone().into(),
            color: Color::from_argb_u8(
                token.color[0],
                token.color[1],
                token.color[2],
                token.color[3],
            ),
            dark_color: Color::from_argb_u8(
                token.dark_color[0],
                token.dark_color[1],
                token.dark_color[2],
                token.dark_color[3],
            ),
            bold: token.bold,
            italic: token.italic,
        })
        .collect::<Vec<_>>();
    WorkflowPrDiffLine {
        old_line: line.old_line,
        new_line: line.new_line,
        kind: line.kind.clone().into(),
        // Keep one sanitized plain-text copy for the live-resize fast path.
        // Normal rendering still uses the token model for syntax colors.
        content: line
            .content
            .chars()
            .filter(|character| !matches!(character, '\r' | '\n'))
            .collect::<String>()
            .into(),
        tokens: ModelRc::new(VecModel::from(tokens)),
        expandable: hunk_is_expandable(file, index),
        expand_all: hunk_can_expand_all(file, index),
        expand_up: hunk_is_expandable(file, index) && !hunk_can_expand_all(file, index),
        expand_down: hunk_can_expand_down(file, index) && !hunk_can_expand_all(file, index),
        source_index: index as i32,
        left_line: 0,
        // Reuse the projection's left column count for Unified wrapping.
        left_columns: highlighted.columns as i32,
        left_kind: "".into(),
        left_content: "".into(),
        left_tokens: empty_token_model(),
        right_line: 0,
        right_columns: 0,
        right_kind: "".into(),
        right_content: "".into(),
        right_tokens: empty_token_model(),
    }
}

fn to_slint_split_line(
    file: &PullRequestDiffFile,
    left: Option<usize>,
    right: Option<usize>,
    source_index: usize,
    spanning: bool,
    highlighted: &[HighlightedLine],
) -> WorkflowPrDiffLine {
    let source = file.lines.get(source_index);
    let (left_line, left_columns, left_kind, left_content, left_tokens) =
        split_side(file, highlighted, left, false);
    let (right_line, right_columns, right_kind, right_content, right_tokens) =
        split_side(file, highlighted, right, true);
    WorkflowPrDiffLine {
        old_line: source.map(|line| line.old_line).unwrap_or(0),
        new_line: source.map(|line| line.new_line).unwrap_or(0),
        kind: if spanning {
            source
                .map(|line| line.kind.clone())
                .unwrap_or_else(|| "meta".to_string())
                .into()
        } else {
            "split".into()
        },
        content: if spanning {
            source
                .map(|line| sanitize_diff_content(line.content.as_str()))
                .unwrap_or_default()
                .into()
        } else {
            "".into()
        },
        tokens: empty_token_model(),
        expandable: spanning && hunk_is_expandable(file, source_index),
        expand_all: spanning && hunk_can_expand_all(file, source_index),
        expand_up: spanning
            && hunk_is_expandable(file, source_index)
            && !hunk_can_expand_all(file, source_index),
        expand_down: spanning
            && hunk_can_expand_down(file, source_index)
            && !hunk_can_expand_all(file, source_index),
        source_index: source_index as i32,
        left_line,
        left_columns,
        left_kind: left_kind.into(),
        left_content: left_content.into(),
        left_tokens,
        right_line,
        right_columns,
        right_kind: right_kind.into(),
        right_content: right_content.into(),
        right_tokens,
    }
}

fn split_side(
    file: &PullRequestDiffFile,
    highlighted: &[HighlightedLine],
    index: Option<usize>,
    right: bool,
) -> (i32, i32, String, String, ModelRc<WorkflowPrDiffToken>) {
    let Some(index) = index else {
        return (
            0,
            0,
            "empty".to_string(),
            String::new(),
            empty_token_model(),
        );
    };
    let Some(line) = file.lines.get(index) else {
        return (
            0,
            0,
            "empty".to_string(),
            String::new(),
            empty_token_model(),
        );
    };
    let line_number = if right { line.new_line } else { line.old_line };
    let tokens = highlighted
        .get(index)
        .map(to_slint_tokens)
        .unwrap_or_else(empty_token_model);
    (
        line_number,
        highlighted
            .get(index)
            .map(|line| line.columns as i32)
            .unwrap_or(0),
        line.kind.clone(),
        sanitize_diff_content(line.content.as_str()),
        tokens,
    )
}

fn to_slint_tokens(highlighted: &HighlightedLine) -> ModelRc<WorkflowPrDiffToken> {
    let tokens = highlighted
        .tokens
        .iter()
        .map(|token| WorkflowPrDiffToken {
            text: token.text.clone().into(),
            color: Color::from_argb_u8(
                token.color[0],
                token.color[1],
                token.color[2],
                token.color[3],
            ),
            dark_color: Color::from_argb_u8(
                token.dark_color[0],
                token.dark_color[1],
                token.dark_color[2],
                token.dark_color[3],
            ),
            bold: token.bold,
            italic: token.italic,
        })
        .collect::<Vec<_>>();
    ModelRc::new(VecModel::from(tokens))
}

fn empty_token_model() -> ModelRc<WorkflowPrDiffToken> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrDiffToken>::new()))
}

/// Resolve a drag position from the grabbed delegate into an exact logical
/// diff row and wrapped display column. Unlike a fixed-height approximation,
/// this accounts for every crossed row's own wrap count.
fn resolve_diff_selection_hit(
    lines: &ModelRc<WorkflowPrDiffLine>,
    start_line: i32,
    pointer_x: f32,
    pointer_y: f32,
    start_height: f32,
    code_width: f32,
    cell_width: f32,
    side: &str,
) -> WorkflowPrDiffSelectionHit {
    const CODE_LINE_HEIGHT: f32 = 22.0;
    const HUNK_LINE_HEIGHT: f32 = 28.0;
    const TWO_WAY_HUNK_HEIGHT: f32 = 52.0;

    let row_count = lines.row_count();
    if row_count == 0 {
        return WorkflowPrDiffSelectionHit { line: 0, column: 0 };
    }

    let start = start_line.clamp(0, row_count.saturating_sub(1) as i32) as usize;
    let safe_code_width = code_width.max(1.0);
    let safe_cell_width = cell_width.max(0.1);
    let row_height = |index: usize| {
        if index == start {
            return start_height.max(1.0);
        }
        let Some(line) = lines.row_data(index) else {
            return CODE_LINE_HEIGHT;
        };
        if line.kind.as_str() == "hunk" {
            return if line.expand_up && line.expand_down {
                TWO_WAY_HUNK_HEIGHT
            } else {
                HUNK_LINE_HEIGHT
            };
        }
        let columns = if side.is_empty() {
            line.left_columns
        } else {
            line.left_columns.max(line.right_columns)
        }
        .max(0) as f32;
        let visual_lines = ((columns * safe_cell_width) / safe_code_width)
            .ceil()
            .max(1.0);
        CODE_LINE_HEIGHT * visual_lines
    };

    let (target, local_y) = if pointer_y >= 0.0 {
        let mut index = start;
        let mut offset = pointer_y;
        loop {
            let height = row_height(index);
            if offset < height || index + 1 >= row_count {
                break (index, offset.clamp(0.0, height));
            }
            offset -= height;
            index += 1;
        }
    } else {
        let mut index = start;
        let mut distance = -pointer_y;
        loop {
            if index == 0 {
                break (0, 0.0);
            }
            index -= 1;
            let height = row_height(index);
            if distance <= height {
                break (index, (height - distance).clamp(0.0, height));
            }
            distance -= height;
        }
    };

    let Some(line) = lines.row_data(target) else {
        return WorkflowPrDiffSelectionHit {
            line: target as i32,
            column: 0,
        };
    };
    if !is_copyable_diff_line(&line) {
        // Hunk/meta rows are not selectable. Snap to the nearest code row in the
        // drag direction so endpoints never land on @@ headers.
        let forward = target > start || (target == start && pointer_y >= start_height * 0.5);
        if forward {
            for index in (target + 1)..row_count {
                if let Some(candidate) = lines.row_data(index) {
                    if is_copyable_diff_line(&candidate) {
                        return WorkflowPrDiffSelectionHit {
                            line: index as i32,
                            column: 0,
                        };
                    }
                }
            }
            for index in (0..target).rev() {
                if let Some(candidate) = lines.row_data(index) {
                    if is_copyable_diff_line(&candidate) {
                        return WorkflowPrDiffSelectionHit {
                            line: index as i32,
                            column: side_display_columns(&candidate, side),
                        };
                    }
                }
            }
        } else {
            for index in (0..target).rev() {
                if let Some(candidate) = lines.row_data(index) {
                    if is_copyable_diff_line(&candidate) {
                        return WorkflowPrDiffSelectionHit {
                            line: index as i32,
                            column: side_display_columns(&candidate, side),
                        };
                    }
                }
            }
            for index in (target + 1)..row_count {
                if let Some(candidate) = lines.row_data(index) {
                    if is_copyable_diff_line(&candidate) {
                        return WorkflowPrDiffSelectionHit {
                            line: index as i32,
                            column: 0,
                        };
                    }
                }
            }
        }
        return WorkflowPrDiffSelectionHit {
            line: target as i32,
            column: 0,
        };
    }
    let columns = side_display_columns(&line, side);
    let visible_columns = (safe_code_width / safe_cell_width).floor().max(1.0) as i32;
    let visual_line = (local_y / CODE_LINE_HEIGHT).floor().max(0.0) as i32;
    // Match SelectableDiffCode::col-at: round to the nearest caret boundary.
    let column_in_line =
        ((pointer_x.max(0.0) + safe_cell_width * 0.5) / safe_cell_width).floor() as i32;

    WorkflowPrDiffSelectionHit {
        line: target as i32,
        column: (visual_line * visible_columns + column_in_line).clamp(0, columns),
    }
}

fn is_copyable_diff_line(line: &WorkflowPrDiffLine) -> bool {
    // Match GitHub: hunk headers and meta separators are never part of a
    // code-column selection or clipboard payload.
    !matches!(line.kind.as_str(), "hunk" | "meta")
}

fn side_display_columns(line: &WorkflowPrDiffLine, side: &str) -> i32 {
    match side {
        "left" => line.left_columns,
        "right" => line.right_columns,
        _ => line.left_columns,
    }
    .max(0)
}

/// Build clipboard text for a GitHub-style code-column selection.
/// `start_col` / `end_col` are rendered monospace columns. This preserves tabs
/// and handles wide glyphs while matching the selection geometry used by Slint.
/// Hunk/meta rows inside the range are omitted, matching GitHub copy.
fn diff_selection_text(
    lines: &ModelRc<WorkflowPrDiffLine>,
    start_line: i32,
    start_col: i32,
    end_line: i32,
    end_col: i32,
    side: &str,
) -> Option<String> {
    if start_line < 0 || end_line < start_line {
        return None;
    }
    let start = start_line as usize;
    let end = end_line as usize;
    if end >= lines.row_count() {
        return None;
    }

    let mut out = String::new();
    let mut emitted = 0_usize;
    for row in start..=end {
        let line = lines.row_data(row)?;
        if !is_copyable_diff_line(&line) {
            continue;
        }
        let content = match side {
            "left" => line.left_content.to_string(),
            "right" => line.right_content.to_string(),
            _ => line.content.to_string(),
        };
        let from = if row == start {
            start_col.max(0) as u32
        } else {
            0
        };
        let to = if row == end {
            end_col.max(0) as u32
        } else {
            u32::MAX
        };
        if emitted > 0 {
            out.push('\n');
        }
        out.push_str(slice_display_columns(content.as_str(), from, to));
        emitted += 1;
    }
    if emitted == 0 {
        return None;
    }
    Some(out)
}

fn slice_display_columns(value: &str, from: u32, to: u32) -> &str {
    if from >= to {
        return "";
    }

    let mut column = 0_u32;
    let mut start_byte = value.len();
    let mut end_byte = value.len();
    let mut found_start = false;

    for (byte, character) in value.char_indices() {
        let width = if character == '\t' {
            4 - column % 4
        } else {
            character_columns(character)
        };
        let next = column + width;

        if !found_start && next > from {
            start_byte = byte;
            found_start = true;
        }
        if column >= to {
            end_byte = byte;
            break;
        }
        column = next;
    }

    if !found_start {
        return "";
    }
    if column < to {
        end_byte = value.len();
    }
    &value[start_byte..end_byte]
}

fn sanitize_diff_content(content: &str) -> String {
    content
        .chars()
        .filter(|character| !matches!(character, '\r' | '\n'))
        .collect()
}

fn hunk_is_expandable(file: &PullRequestDiffFile, hunk_index: usize) -> bool {
    let Some(line) = file.lines.get(hunk_index) else {
        return false;
    };
    if line.kind != "hunk" {
        return false;
    }
    let use_new_side = file.change_type != "deleted_file";
    let previous = file.lines[..hunk_index]
        .iter()
        .rev()
        .find_map(|line| side_line(line, use_new_side))
        .unwrap_or(0);
    let next = file.lines[hunk_index + 1..]
        .iter()
        .find_map(|line| side_line(line, use_new_side));
    next.is_some_and(|next| next > previous.saturating_add(1))
}

fn hunk_can_expand_down(file: &PullRequestDiffFile, hunk_index: usize) -> bool {
    if !hunk_is_expandable(file, hunk_index) {
        return false;
    }
    let use_new_side = file.change_type != "deleted_file";
    file.lines[..hunk_index]
        .iter()
        .rev()
        .find_map(|line| side_line(line, use_new_side))
        .is_some()
}

fn hunk_can_expand_all(file: &PullRequestDiffFile, hunk_index: usize) -> bool {
    let Some((previous, next)) = hunk_hidden_bounds(file, hunk_index) else {
        return false;
    };
    previous > 0 && next.saturating_sub(previous).saturating_sub(1) <= HUNK_CONTEXT_PAGE_SIZE as i32
}

fn hunk_hidden_bounds(file: &PullRequestDiffFile, hunk_index: usize) -> Option<(i32, i32)> {
    let line = file.lines.get(hunk_index)?;
    if line.kind != "hunk" {
        return None;
    }
    let use_new_side = file.change_type != "deleted_file";
    let previous = file.lines[..hunk_index]
        .iter()
        .rev()
        .find_map(|line| side_line(line, use_new_side))
        .unwrap_or(0);
    let next = file.lines[hunk_index + 1..]
        .iter()
        .find_map(|line| side_line(line, use_new_side))?;
    (next > previous.saturating_add(1)).then_some((previous, next))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HunkExpansionDirection {
    Up,
    Down,
    All,
}

fn side_line(line: &PullRequestDiffLine, use_new_side: bool) -> Option<i32> {
    let number = if use_new_side {
        line.new_line
    } else {
        line.old_line
    };
    (number > 0).then_some(number)
}

fn hunk_side_bounds(
    file: &PullRequestDiffFile,
    hunk_index: usize,
    use_new_side: bool,
) -> Option<(i32, i32)> {
    let previous = file.lines[..hunk_index]
        .iter()
        .rev()
        .find_map(|line| side_line(line, use_new_side))
        .unwrap_or(0);
    let next = file.lines[hunk_index + 1..]
        .iter()
        .find_map(|line| side_line(line, use_new_side))?;
    Some((previous, next))
}

fn bounded_source_range<'a>(
    source_lines: &'a [&'a str],
    previous: i32,
    next: i32,
    direction: HunkExpansionDirection,
    limit: usize,
) -> Vec<(i32, &'a str)> {
    let start = previous.saturating_add(1).max(1) as usize;
    let end = next.saturating_sub(1).max(0) as usize;
    if start > end || start > source_lines.len() {
        return Vec::new();
    }
    let end = end.min(source_lines.len());
    let scan_limit = if limit == i32::MAX as usize {
        1_024
    } else {
        limit.saturating_add(128).max(128)
    };
    let (start, end) = match direction {
        HunkExpansionDirection::Down => (start, end.min(start.saturating_add(scan_limit - 1))),
        HunkExpansionDirection::Up => (start.max(end.saturating_sub(scan_limit - 1)), end),
        HunkExpansionDirection::All => (start, end),
    };
    (start..=end)
        .map(|number| (number as i32, source_lines[number - 1]))
        .collect()
}

fn align_source_ranges(
    old_lines: &[(i32, &str)],
    new_lines: &[(i32, &str)],
) -> Vec<PullRequestDiffLine> {
    let old_len = old_lines.len();
    let new_len = new_lines.len();
    let mut matches = vec![vec![0_u16; new_len + 1]; old_len + 1];
    for old_index in (0..old_len).rev() {
        for new_index in (0..new_len).rev() {
            matches[old_index][new_index] = if old_lines[old_index].1 == new_lines[new_index].1 {
                matches[old_index + 1][new_index + 1].saturating_add(1)
            } else {
                matches[old_index + 1][new_index].max(matches[old_index][new_index + 1])
            };
        }
    }

    let mut rows = Vec::with_capacity(old_len.max(new_len));
    let (mut old_index, mut new_index) = (0, 0);
    while old_index < old_len || new_index < new_len {
        if old_index < old_len
            && new_index < new_len
            && old_lines[old_index].1 == new_lines[new_index].1
        {
            rows.push(PullRequestDiffLine {
                old_line: old_lines[old_index].0,
                new_line: new_lines[new_index].0,
                kind: "context".to_string(),
                content: sanitize_diff_content(old_lines[old_index].1),
            });
            old_index += 1;
            new_index += 1;
        } else if new_index >= new_len
            || (old_index < old_len
                && matches[old_index + 1][new_index] >= matches[old_index][new_index + 1])
        {
            rows.push(PullRequestDiffLine {
                old_line: old_lines[old_index].0,
                new_line: 0,
                kind: "deletion".to_string(),
                content: sanitize_diff_content(old_lines[old_index].1),
            });
            old_index += 1;
        } else {
            rows.push(PullRequestDiffLine {
                old_line: 0,
                new_line: new_lines[new_index].0,
                kind: "addition".to_string(),
                content: sanitize_diff_content(new_lines[new_index].1),
            });
            new_index += 1;
        }
    }
    rows
}

fn bounded_aligned_rows(
    rows: Vec<PullRequestDiffLine>,
    use_new_side: bool,
    limit: usize,
    direction: HunkExpansionDirection,
) -> Vec<PullRequestDiffLine> {
    if direction == HunkExpansionDirection::All || limit == i32::MAX as usize {
        return rows;
    }
    let primary_line = |line: &PullRequestDiffLine| {
        if use_new_side {
            line.new_line
        } else {
            line.old_line
        }
    };
    match direction {
        HunkExpansionDirection::Down => {
            let mut count = 0;
            rows.into_iter()
                .take_while(|line| {
                    if primary_line(line) > 0 {
                        if count >= limit {
                            return false;
                        }
                        count += 1;
                    }
                    true
                })
                .collect()
        }
        HunkExpansionDirection::Up => {
            let mut count = 0;
            let mut selected = rows
                .into_iter()
                .rev()
                .take_while(|line| {
                    if primary_line(line) > 0 {
                        if count >= limit {
                            return false;
                        }
                        count += 1;
                    }
                    true
                })
                .collect::<Vec<_>>();
            selected.reverse();
            selected
        }
        HunkExpansionDirection::All => rows,
    }
}

fn expand_hunk_context_from_sources(
    file: &mut PullRequestDiffFile,
    hunk_index: usize,
    base_source: &str,
    head_source: &str,
    limit: usize,
    direction: HunkExpansionDirection,
) -> bool {
    if limit == 0 || !hunk_is_expandable(file, hunk_index) {
        return false;
    }
    let old_bounds = hunk_side_bounds(file, hunk_index, false);
    let new_bounds = hunk_side_bounds(file, hunk_index, true);
    let base_lines = base_source.lines().collect::<Vec<_>>();
    let head_lines = head_source.lines().collect::<Vec<_>>();
    let old_range = old_bounds
        .map(|(previous, next)| bounded_source_range(&base_lines, previous, next, direction, limit))
        .unwrap_or_default();
    let new_range = new_bounds
        .map(|(previous, next)| bounded_source_range(&head_lines, previous, next, direction, limit))
        .unwrap_or_default();
    let use_new_side = file.change_type != "deleted_file";
    let rows = bounded_aligned_rows(
        align_source_ranges(old_range.as_slice(), new_range.as_slice()),
        use_new_side,
        limit,
        direction,
    );
    if rows.is_empty() {
        return false;
    }
    let insertion_index = match direction {
        HunkExpansionDirection::Down => hunk_index,
        HunkExpansionDirection::Up | HunkExpansionDirection::All => hunk_index + 1,
    };
    let inserted_len = rows.len();
    file.lines.splice(insertion_index..insertion_index, rows);
    let current_hunk_index = if direction == HunkExpansionDirection::Down {
        hunk_index.saturating_add(inserted_len)
    } else {
        hunk_index
    };
    if file
        .lines
        .get(current_hunk_index)
        .is_some_and(|line| line.kind == "hunk")
        && !hunk_is_expandable(file, current_hunk_index)
    {
        file.lines.remove(current_hunk_index);
    }
    true
}

#[cfg(test)]
fn expand_hunk_context(
    file: &mut PullRequestDiffFile,
    hunk_index: usize,
    source: &str,
    limit: usize,
    direction: HunkExpansionDirection,
) -> bool {
    if limit == 0 || !hunk_is_expandable(file, hunk_index) {
        return false;
    }
    let use_new_side = file.change_type != "deleted_file";
    let previous = file.lines[..hunk_index]
        .iter()
        .rev()
        .find_map(|line| side_line(line, use_new_side))
        .unwrap_or(0);
    let Some(next) = file.lines[hunk_index + 1..]
        .iter()
        .find_map(|line| side_line(line, use_new_side))
    else {
        return false;
    };
    let hidden_end = next.saturating_sub(1);
    let hidden_start = previous.saturating_add(1);
    let (reveal_start, reveal_end, insertion_index) = match direction {
        HunkExpansionDirection::Up => (
            hidden_start.max(hidden_end.saturating_sub(limit as i32 - 1)),
            hidden_end,
            hunk_index + 1,
        ),
        HunkExpansionDirection::Down => (
            hidden_start,
            hidden_end.min(hidden_start.saturating_add(limit as i32 - 1)),
            hunk_index,
        ),
        HunkExpansionDirection::All => (hidden_start, hidden_end, hunk_index + 1),
    };
    let source_lines = source.lines().collect::<Vec<_>>();
    if reveal_start <= 0 || reveal_start > reveal_end || reveal_end as usize > source_lines.len() {
        return false;
    }

    let next_line = file.lines[hunk_index + 1..]
        .iter()
        .find(|line| side_line(line, use_new_side) == Some(next));
    let counterpart_next = next_line
        .map(|line| {
            if use_new_side {
                line.old_line
            } else {
                line.new_line
            }
        })
        .unwrap_or(0);
    let mut revealed = Vec::new();
    for number in reveal_start..=reveal_end {
        let counterpart = if counterpart_next > 0 {
            counterpart_next.saturating_sub(next.saturating_sub(number))
        } else {
            0
        };
        revealed.push(PullRequestDiffLine {
            old_line: if use_new_side { counterpart } else { number },
            new_line: if use_new_side { number } else { counterpart },
            kind: "context".to_string(),
            content: source_lines[number as usize - 1].to_string(),
        });
    }
    file.lines
        .splice(insertion_index..insertion_index, revealed);
    true
}

#[cfg(test)]
fn expand_all_hunk_context(file: &mut PullRequestDiffFile, source: &str) -> usize {
    let initial_len = file.lines.len();
    loop {
        let Some(index) = file
            .lines
            .iter()
            .enumerate()
            .find_map(|(index, _)| hunk_is_expandable(file, index).then_some(index))
        else {
            break;
        };
        if !expand_hunk_context(
            file,
            index,
            source,
            i32::MAX as usize,
            HunkExpansionDirection::Up,
        ) {
            break;
        }
    }
    file.lines.len().saturating_sub(initial_len)
}

fn expand_all_hunk_context_from_sources(
    file: &mut PullRequestDiffFile,
    base_source: &str,
    head_source: &str,
) -> usize {
    let initial_len = file.lines.len();
    loop {
        let Some(index) = file
            .lines
            .iter()
            .enumerate()
            .find_map(|(index, _)| hunk_is_expandable(file, index).then_some(index))
        else {
            break;
        };
        let bounds_before = hunk_hidden_bounds(file, index);
        if !expand_hunk_context_from_sources(
            file,
            index,
            base_source,
            head_source,
            HUNK_CONTEXT_PAGE_SIZE,
            HunkExpansionDirection::Up,
        ) {
            break;
        }
        if hunk_hidden_bounds(file, index) == bounds_before {
            break;
        }
    }
    file.lines.len().saturating_sub(initial_len)
}

#[cfg(test)]
fn restore_original_diff(file: &mut PullRequestDiffFile, original: &PullRequestDiffFile) -> bool {
    if file.lines == original.lines {
        return false;
    }
    *file = original.clone();
    true
}

fn full_source_file(file: &PullRequestDiffFile, source: &str) -> PullRequestDiffFile {
    let deleted = file.change_type == "deleted_file";
    PullRequestDiffFile {
        path: file.path.clone(),
        old_path: file.old_path.clone(),
        change_type: file.change_type.clone(),
        added_lines: file.added_lines,
        removed_lines: file.removed_lines,
        viewed: file.viewed,
        lines: source
            .lines()
            .enumerate()
            .map(|(index, content)| {
                let number = index.saturating_add(1) as i32;
                PullRequestDiffLine {
                    old_line: if deleted { number } else { 0 },
                    new_line: if deleted { 0 } else { number },
                    kind: "context".to_string(),
                    content: content.to_string(),
                }
            })
            .collect(),
    }
}

#[derive(Default)]
struct FileTreeNode {
    directories: BTreeMap<String, FileTreeNode>,
    files: Vec<usize>,
}

fn build_tree_items(
    files: &[PullRequestDiffFile],
    query: &str,
    collapsed: &HashSet<String>,
    selected_extensions: &HashSet<String>,
    only_manifest_files: bool,
    show_viewed_files: bool,
) -> Vec<WorkflowPrTreeItem> {
    let normalized_query = query.trim().to_ascii_lowercase();
    let mut root = FileTreeNode::default();
    for (index, file) in files.iter().enumerate() {
        if !selected_extensions.contains(file_extension(file.path.as_str()).as_str())
            || (only_manifest_files && !is_manifest_file(file.path.as_str()))
            || (!show_viewed_files && file.viewed)
        {
            continue;
        }
        if !normalized_query.is_empty()
            && !file
                .path
                .to_ascii_lowercase()
                .contains(normalized_query.as_str())
        {
            continue;
        }
        let mut segments = file
            .path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if segments.is_empty() {
            continue;
        }
        segments.pop();
        let mut node = &mut root;
        for segment in segments {
            node = node.directories.entry(segment.to_string()).or_default();
        }
        node.files.push(index);
    }

    let mut items = Vec::new();
    flatten_tree(
        &root,
        "",
        0,
        files,
        collapsed,
        !normalized_query.is_empty(),
        &mut items,
    );
    items
}

fn file_extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    if let Some((stem, extension)) = name.rsplit_once('.')
        && !stem.is_empty()
        && !extension.is_empty()
    {
        return format!(".{}", extension.to_ascii_lowercase());
    }
    "No extension".to_string()
}

fn extension_counts(files: &[PullRequestDiffFile]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for file in files {
        *counts
            .entry(file_extension(file.path.as_str()))
            .or_insert(0) += 1;
    }
    counts
}

fn is_manifest_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "cargo.toml"
            | "cargo.lock"
            | "package.json"
            | "package-lock.json"
            | "npm-shrinkwrap.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "gemfile"
            | "gemfile.lock"
            | "go.mod"
            | "go.sum"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "gradle.lockfile"
            | "requirements.txt"
            | "pipfile"
            | "pipfile.lock"
            | "poetry.lock"
            | "pyproject.toml"
            | "composer.json"
            | "composer.lock"
            | "pubspec.yaml"
            | "pubspec.lock"
            | "podfile"
            | "podfile.lock"
            | "packages.config"
            | "mix.exs"
            | "mix.lock"
            | "package.swift"
            | "package.resolved"
            | "vcpkg.json"
            | "conanfile.py"
    ) || lower.ends_with(".csproj")
        || lower.ends_with(".fsproj")
        || lower.ends_with(".vbproj")
}

fn flatten_tree(
    node: &FileTreeNode,
    parent_path: &str,
    depth: i32,
    files: &[PullRequestDiffFile],
    collapsed: &HashSet<String>,
    force_expanded: bool,
    items: &mut Vec<WorkflowPrTreeItem>,
) {
    for (name, child) in &node.directories {
        let path = if parent_path.is_empty() {
            name.clone()
        } else {
            format!("{parent_path}/{name}")
        };
        let expanded = force_expanded || !collapsed.contains(path.as_str());
        items.push(WorkflowPrTreeItem {
            kind: "folder".into(),
            name: name.clone().into(),
            path: path.clone().into(),
            depth,
            file_index: -1,
            expanded,
            added_lines: 0,
            removed_lines: 0,
            change_type: "".into(),
            file_count: count_files(child) as i32,
        });
        if expanded {
            flatten_tree(
                child,
                path.as_str(),
                depth + 1,
                files,
                collapsed,
                force_expanded,
                items,
            );
        }
    }

    let mut file_indexes = node.files.clone();
    file_indexes.sort_by(|left, right| files[*left].path.cmp(&files[*right].path));
    for index in file_indexes {
        let file = &files[index];
        let name = file.path.rsplit('/').next().unwrap_or(file.path.as_str());
        items.push(WorkflowPrTreeItem {
            kind: "file".into(),
            name: name.into(),
            path: file.path.clone().into(),
            depth,
            file_index: index as i32,
            expanded: false,
            added_lines: file.added_lines as i32,
            removed_lines: file.removed_lines as i32,
            change_type: file.change_type.clone().into(),
            file_count: 0,
        });
    }
}

fn count_files(node: &FileTreeNode) -> usize {
    node.files.len() + node.directories.values().map(count_files).sum::<usize>()
}

fn empty_file_model() -> ModelRc<WorkflowPrDiffFile> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrDiffFile>::new()))
}

fn empty_line_model() -> ModelRc<WorkflowPrDiffLine> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrDiffLine>::new()))
}

fn empty_tree_model() -> ModelRc<WorkflowPrTreeItem> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrTreeItem>::new()))
}

fn empty_extension_filter_model() -> ModelRc<WorkflowPrFileExtensionFilter> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrFileExtensionFilter>::new()))
}

fn empty_conversation_model() -> ModelRc<WorkflowPrConversationItem> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrConversationItem>::new()))
}

fn conversation_row_height(item: &PullRequestConversationItem) -> i32 {
    match item.kind.as_str() {
        "commit" => 36,
        "commits" | "event" | "merged" | "review_summary" => 48,
        _ => conversation_card_row_height(item.body.as_str()),
    }
}

fn conversation_content_height(items: &[PullRequestConversationItem]) -> i32 {
    items.iter().map(conversation_row_height).sum()
}

/// GitHub commit-message body under the title (kebab expansion).
/// Prefer a slight under-estimate: over-estimating leaves a blank gap under the
/// body before the author footer. Slint still sizes the visible Text with
/// `preferred-height` so wrapped text is not clipped when the estimate is low.
fn commit_message_body_height(body: &str) -> i32 {
    const BOTTOM_PADDING: i32 = 4;
    // Matches the 12px monospace preformatted body closely enough for ListView
    // row reservation; the visible body uses preferred-height.
    const LINE_HEIGHT: i32 = 17;
    // Commits body ≈ list(≤980) − 250 ≈ 730px ≈ 100 Latin cols at 12px.
    // Conversation body is similar. Wide columns avoid the blank-gap bug.
    const WRAP_COLUMNS: usize = 100;
    const MAX_LINES: i32 = 20;

    if body.trim().is_empty() {
        return 0;
    }
    let mut lines = 0i32;
    for paragraph in body.lines() {
        if paragraph.is_empty() {
            lines += 1;
            continue;
        }
        // Treat full-width / CJK as ~2 columns so wraps are not under-counted.
        let cols = paragraph
            .chars()
            .map(|ch| if ch_width_hint(ch) { 2usize } else { 1 })
            .sum::<usize>()
            .max(1);
        lines += cols.div_ceil(WRAP_COLUMNS) as i32;
    }
    BOTTOM_PADDING + lines.clamp(1, MAX_LINES) * LINE_HEIGHT
}

fn ch_width_hint(ch: char) -> bool {
    ch > '\u{2E7F}'
}

/// Commits-tab expand layout: title band + body + author band − collapsed row.
fn commit_tab_expand_layout_extra(body_height: i32) -> i32 {
    const TITLE_BAND: i32 = 30;
    const AUTHOR_BAND: i32 = 28;
    const ROW_HEIGHT: i32 = 52;
    body_height + TITLE_BAND + AUTHOR_BAND - ROW_HEIGHT
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

fn refresh_commit_group_expand_extras(commits: &mut [WorkflowPrCommit]) {
    let mut index = 0;
    while index < commits.len() {
        if !commits[index].group_start {
            commits[index].group_expand_extra = 0;
            index += 1;
            continue;
        }
        let group_size = commits[index].group_size.max(0) as usize;
        let end = (index + group_size).min(commits.len());
        let extra = commits[index..end]
            .iter()
            .filter(|commit| commit.message_expanded)
            .map(|commit| commit_tab_expand_layout_extra(commit.message_body_height))
            .sum::<i32>();
        commits[index].group_expand_extra = extra;
        for commit in &mut commits[index + 1..end] {
            commit.group_expand_extra = 0;
        }
        index = end.max(index + 1);
    }
}

/// GitHub Conversation cards hug their text: header + padded body lines, not a
/// fixed tall viewport. Long bodies are capped so ListView row heights stay bounded.
fn conversation_card_row_height(body: &str) -> i32 {
    const OUTER_MARGIN: i32 = 12;
    const HEADER: i32 = 36;
    const EMPTY_BODY: i32 = 40;
    const BODY_PADDING: i32 = 24;
    const LINE_HEIGHT: i32 = 20;
    const WRAP_COLUMNS: usize = 72;
    const MAX_LINES: i32 = 12;

    if body.trim().is_empty() {
        return OUTER_MARGIN + HEADER + EMPTY_BODY;
    }

    let mut lines = 0i32;
    for paragraph in body.lines() {
        let chars = paragraph.chars().count().max(1);
        lines += chars.div_ceil(WRAP_COLUMNS) as i32;
    }
    let lines = lines.clamp(1, MAX_LINES);
    OUTER_MARGIN + HEADER + BODY_PADDING + lines * LINE_HEIGHT
}

async fn load_conversation_avatars(
    items: &[PullRequestConversationItem],
) -> HashMap<String, ConversationAvatarPixels> {
    load_avatar_urls(
        items
            .iter()
            .filter_map(|item| {
                let url = item.avatar_url.trim();
                (!url.is_empty()).then(|| url.to_string())
            })
            .collect::<HashSet<_>>()
            .into_iter(),
    )
    .await
}

async fn load_commit_avatars(
    commits: &[PullRequestCommit],
) -> HashMap<String, ConversationAvatarPixels> {
    load_avatar_urls(
        commits
            .iter()
            .filter_map(|commit| {
                let url = commit.avatar_url.trim();
                (!url.is_empty()).then(|| url.to_string())
            })
            .collect::<HashSet<_>>()
            .into_iter(),
    )
    .await
}

async fn load_avatar_urls(
    urls: impl Iterator<Item = String>,
) -> HashMap<String, ConversationAvatarPixels> {
    let urls = urls.take(32);
    stream::iter(urls)
        .map(|url| async move {
            let image = load_conversation_avatar(url.as_str()).await?;
            Some((url, image))
        })
        .buffer_unordered(6)
        .filter_map(async move |item| item)
        .collect()
        .await
}

async fn load_conversation_avatar(url: &str) -> Option<ConversationAvatarPixels> {
    const MAX_AVATAR_BYTES: usize = 2 * 1024 * 1024;

    let mut response = http::send(
        http::get(url),
        RequestOptions::new("Load pull request conversation avatar"),
    )
    .await
    .ok()?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_AVATAR_BYTES as u64)
    {
        return None;
    }

    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > MAX_AVATAR_BYTES {
            return None;
        }
        bytes.extend_from_slice(chunk.as_ref());
    }

    let decoded = image::load_from_memory(bytes.as_slice()).ok()?;
    let resized = decoded
        .resize_to_fill(80, 80, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    Some(ConversationAvatarPixels {
        width: resized.width(),
        height: resized.height(),
        rgba: resized.into_raw(),
    })
}

fn empty_commit_model() -> ModelRc<WorkflowPrCommit> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrCommit>::new()))
}

fn conversation_model(
    items: &[PullRequestConversationItem],
    avatars: &HashMap<String, ConversationAvatarPixels>,
) -> ModelRc<WorkflowPrConversationItem> {
    ModelRc::new(VecModel::from(
        items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let (body, message_body) = if item.kind == "commit" {
                    let (title, body) = split_commit_message(item.body.as_str());
                    let title = if title.is_empty() {
                        item.body.lines().next().unwrap_or("").to_string()
                    } else {
                        title
                    };
                    (title, body)
                } else {
                    (item.body.clone(), String::new())
                };
                let message_body_height = commit_message_body_height(message_body.as_str());
                WorkflowPrConversationItem {
                    id: item.id.clone().into(),
                    kind: item.kind.clone().into(),
                    author: item.author.clone().into(),
                    avatar: avatars
                        .get(item.avatar_url.as_str())
                        .map(|avatar| {
                            Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                                avatar.rgba.as_slice(),
                                avatar.width,
                                avatar.height,
                            ))
                        })
                        .unwrap_or_default(),
                    avatar_label: item
                        .author
                        .chars()
                        .next()
                        .map(|character| character.to_uppercase().collect::<String>())
                        .unwrap_or_else(|| "?".to_string())
                        .into(),
                    author_association: item.author_association.clone().into(),
                    body: body.into(),
                    message_body: message_body.into(),
                    message_body_height,
                    message_expanded: false,
                    created_at: super::pr_list::format_github_relative_time(
                        item.created_at.as_str(),
                    )
                    .into(),
                    created_at_tooltip: super::pr_list::format_github_absolute_tooltip(
                        item.created_at.as_str(),
                    )
                    .into(),
                    state: item.state.clone().into(),
                    connect_before: conversation_items_connect(
                        index
                            .checked_sub(1)
                            .and_then(|previous| items.get(previous)),
                        Some(item),
                    ),
                    connect_after: conversation_items_connect(Some(item), items.get(index + 1)),
                    row_height: conversation_row_height(item),
                }
            })
            .collect::<Vec<_>>(),
    ))
}

fn conversation_items_connect(
    before: Option<&PullRequestConversationItem>,
    after: Option<&PullRequestConversationItem>,
) -> bool {
    let (Some(before), Some(after)) = (before, after) else {
        return false;
    };
    if before.kind == "merged" {
        return false;
    }
    if after.kind == "review_summary" {
        // Adjacent GitHub reviews are separate summary/card pairs, but their
        // visual timeline remains continuous between the preceding card and
        // the next summary marker.
        return before.kind == "review";
    }
    before.kind == "review_summary" && after.kind == "review" || before.kind != "review_summary"
}

fn commit_model(
    commits: &[PullRequestCommit],
    avatars: &HashMap<String, ConversationAvatarPixels>,
) -> ModelRc<WorkflowPrCommit> {
    // Provider lists stay oldest→newest for range math; the Commits tab and
    // commit-range picker show newest→oldest.
    let display = commits.iter().rev().collect::<Vec<_>>();
    let date_labels = display
        .iter()
        .map(|commit| format_commit_date(commit.authored_at.as_str()))
        .collect::<Vec<_>>();
    ModelRc::new(VecModel::from(
        display
            .iter()
            .enumerate()
            .map(|(index, commit)| {
                let date_label = &date_labels[index];
                let group_start = index == 0 || date_labels[index - 1] != *date_label;
                let group_end =
                    index + 1 == date_labels.len() || date_labels[index + 1] != *date_label;
                let group_size = if group_start {
                    date_labels[index..]
                        .iter()
                        .take_while(|candidate| *candidate == date_label)
                        .count()
                } else {
                    0
                };
                WorkflowPrCommit {
                    sha: commit.sha.clone().into(),
                    short_sha: commit.sha.chars().take(7).collect::<String>().into(),
                    title: commit.title.clone().into(),
                    message: commit.message.clone().into(),
                    message_body: {
                        let (_, body) = split_commit_message(commit.message.as_str());
                        body.into()
                    },
                    message_body_height: {
                        let (_, body) = split_commit_message(commit.message.as_str());
                        commit_message_body_height(body.as_str())
                    },
                    message_expanded: false,
                    group_expand_extra: 0,
                    author: commit.author.clone().into(),
                    avatar: avatars
                        .get(commit.avatar_url.as_str())
                        .map(|avatar| {
                            Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                                avatar.rgba.as_slice(),
                                avatar.width,
                                avatar.height,
                            ))
                        })
                        .unwrap_or_default(),
                    avatar_label: commit
                        .author
                        .chars()
                        .next()
                        .map(|character| character.to_uppercase().collect::<String>())
                        .unwrap_or_else(|| "?".to_string())
                        .into(),
                    // Absolute local datetime for the relative-time tooltip.
                    authored_at: super::pr_list::format_github_absolute_tooltip(
                        commit.authored_at.as_str(),
                    )
                    .into(),
                    date_label: date_label.clone().into(),
                    // GitHub Commits rows: Primer relative-time (P30D → `on Mar 19`).
                    short_date_label: super::pr_list::format_github_relative_time(
                        commit.authored_at.as_str(),
                    )
                    .into(),
                    group_start,
                    group_end,
                    group_size: group_size as i32,
                    verified: commit.verified,
                }
            })
            .collect::<Vec<_>>(),
    ))
}

/// Map a newest-first display index range onto the oldest→newest provider list.
fn display_commit_range_to_chrono(
    display_start: usize,
    display_end: usize,
    len: usize,
) -> Option<(usize, usize)> {
    if len == 0 || display_start >= len || display_end >= len {
        return None;
    }
    let (display_lo, display_hi) = if display_start <= display_end {
        (display_start, display_end)
    } else {
        (display_end, display_start)
    };
    Some((len - 1 - display_hi, len - 1 - display_lo))
}

fn chrono_commit_index_to_display(chrono_index: usize, len: usize) -> Option<usize> {
    if len == 0 || chrono_index >= len {
        return None;
    }
    Some(len - 1 - chrono_index)
}

fn commit_range_label(commits: &[PullRequestCommit], start: usize, end: usize) -> String {
    let short = |sha: &str| sha.chars().take(7).collect::<String>();
    if start == end {
        format!("Commit {}", short(commits[start].sha.as_str()))
    } else {
        format!(
            "{} … {}",
            short(commits[start].sha.as_str()),
            short(commits[end].sha.as_str())
        )
    }
}

fn commit_index_by_sha(commits: &[PullRequestCommit], sha: &str) -> Option<usize> {
    commits.iter().position(|commit| commit.sha == sha)
}

fn range_covers_all_commits(commit_count: usize, start: usize, end: usize) -> bool {
    commit_count > 0 && start == 0 && end == commit_count - 1
}

fn format_commit_date(value: &str) -> String {
    let utc_plus_eight =
        chrono::FixedOffset::east_opt(8 * 60 * 60).expect("UTC+08:00 must be a valid fixed offset");
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|timestamp| {
            timestamp
                .with_timezone(&utc_plus_eight)
                .format("%b %-d, %Y")
                .to_string()
        })
        .unwrap_or_else(|_| value.split('T').next().unwrap_or(value).to_string())
}

fn apply_action_state(ui: &MainWindow, state: &PullRequestActionState) {
    ui.set_workflow_pr_can_comment(state.can_comment);
    ui.set_workflow_pr_can_approve(state.can_approve);
    ui.set_workflow_pr_can_request_changes(state.can_request_changes);
    ui.set_workflow_pr_can_merge(state.can_merge);
    ui.set_workflow_pr_can_revert(state.can_revert);
    if !state.review_state.trim().is_empty() {
        ui.set_workflow_pr_remote_review_state(state.review_state.clone().into());
    }
    ui.set_workflow_pr_merge_readiness_kind(state.readiness.kind.as_str().into());
    ui.set_workflow_pr_merge_readiness_label(state.readiness.kind.label().into());
    ui.set_workflow_pr_merge_readiness_detail(state.readiness.detail.clone().into());
    ui.set_workflow_pr_merge_block_reason(state.merge_block_reason.clone().into());
    ui.set_workflow_pr_revert_block_reason(state.revert_block_reason.clone().into());
    ui.set_workflow_pr_revert_action_label(state.revert_action_label.clone().into());
    ui.set_workflow_pr_revert_confirmation(state.revert_confirmation.clone().into());
    ui.set_workflow_pr_merge_method_merge(
        state
            .allowed_merge_methods
            .contains(&PullRequestMergeMethod::Merge),
    );
    ui.set_workflow_pr_merge_method_squash(
        state
            .allowed_merge_methods
            .contains(&PullRequestMergeMethod::Squash),
    );
    ui.set_workflow_pr_merge_method_rebase(
        state
            .allowed_merge_methods
            .contains(&PullRequestMergeMethod::Rebase),
    );
    ui.set_workflow_pr_merge_method_fast_forward(
        state
            .allowed_merge_methods
            .contains(&PullRequestMergeMethod::FastForward),
    );
}

fn reset_action_capabilities(ui: &MainWindow) {
    ui.set_workflow_pr_can_comment(false);
    ui.set_workflow_pr_can_approve(false);
    ui.set_workflow_pr_can_request_changes(false);
    ui.set_workflow_pr_can_merge(false);
    ui.set_workflow_pr_can_revert(false);
    ui.set_workflow_pr_merge_method_merge(false);
    ui.set_workflow_pr_merge_method_squash(false);
    ui.set_workflow_pr_merge_method_rebase(false);
    ui.set_workflow_pr_merge_method_fast_forward(false);
}

fn reset_action_ui(ui: &MainWindow) {
    ui.set_workflow_pr_review_dialog_open(false);
    ui.set_workflow_pr_merge_dialog_open(false);
    ui.set_workflow_pr_revert_dialog_open(false);
    ui.set_workflow_pr_action_loading(false);
    ui.set_workflow_pr_remote_review_state("".into());
    ui.set_workflow_pr_merge_readiness_kind("".into());
    ui.set_workflow_pr_merge_readiness_label("".into());
    ui.set_workflow_pr_merge_readiness_detail("".into());
    ui.set_workflow_pr_merge_block_reason("".into());
    ui.set_workflow_pr_revert_block_reason("".into());
    ui.set_workflow_pr_revert_action_label("Revert changes".into());
    ui.set_workflow_pr_revert_confirmation("".into());
    reset_action_capabilities(ui);
}

fn reset_review_ui(ui: &MainWindow) {
    ui.set_workflow_pr_review_visible(false);
    ui.set_workflow_pr_review_loading(false);
    ui.set_workflow_pr_review_code_loading(false);
    ui.set_workflow_pr_review_large_diff_deferred(false);
    ui.set_workflow_pr_review_large_diff_loading(false);
    ui.set_workflow_pr_review_error("".into());
    ui.set_workflow_pr_review_files(empty_file_model());
    ui.set_workflow_pr_review_tree_items(empty_tree_model());
    ui.set_workflow_pr_review_extension_filters(empty_extension_filter_model());
    ui.set_workflow_pr_review_only_manifest_files(false);
    ui.set_workflow_pr_review_show_viewed_files(true);
    ui.set_workflow_pr_review_file_filters_active(false);
    ui.set_workflow_pr_review_lines(empty_line_model());
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
    ui.set_workflow_pr_review_tab(2);
    ui.set_workflow_pr_review_back_label("Back to pull requests".into());
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
    ui.set_workflow_pr_review_commit_range_label("All commits".into());
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

    use slint::{Model, ModelRc, VecModel};

    use super::{
        BoundedCache, DiffRowProjection, HUNK_CONTEXT_PAGE_SIZE, HunkExpansionDirection,
        LARGE_DIFF_BYTE_THRESHOLD, LARGE_DIFF_LINE_THRESHOLD, LazyPrDiffLineModel,
        RANGE_DIFF_CACHE_CAPACITY, ReviewCache, ReviewRequest, VISIBLE_LINE_CACHE_CAPACITY,
        build_tree_items, chrono_commit_index_to_display, commit_detail_source_tab,
        commit_index_by_sha, commit_message_body_height, commit_range_label,
        commit_tab_expand_layout_extra, conversation_card_row_height, conversation_content_height,
        conversation_items_connect, diff_row_projection, diff_selection_text,
        display_commit_range_to_chrono, empty_token_model, expand_all_hunk_context,
        expand_all_hunk_context_from_sources, expand_hunk_context,
        expand_hunk_context_from_sources, full_source_file, hunk_can_expand_all,
        hunk_can_expand_down, hunk_is_expandable, is_large_diff, range_covers_all_commits,
        refresh_commit_group_expand_extras, resolve_diff_selection_hit, restore_original_diff,
        slice_display_columns,
    };
    use crate::git::{
        PullRequestCommit, PullRequestConversationItem, PullRequestDiffDetail, PullRequestDiffFile,
        PullRequestDiffLine,
    };
    use crate::project::ProjectRepositoryMetadata;
    use crate::workflow::syntax_highlight::{HighlightedLine, HighlightedToken};
    use crate::{WorkflowPrCommit, WorkflowPrDiffLine};

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

    fn slint_diff_line(content: &str, left: &str, right: &str) -> WorkflowPrDiffLine {
        WorkflowPrDiffLine {
            old_line: 0,
            new_line: 1,
            kind: "context".into(),
            content: content.into(),
            tokens: empty_token_model(),
            expandable: false,
            expand_all: false,
            expand_up: false,
            expand_down: false,
            source_index: 0,
            left_line: 1,
            left_columns: left.chars().count() as i32,
            left_kind: "context".into(),
            left_content: left.into(),
            left_tokens: empty_token_model(),
            right_line: 1,
            right_columns: right.chars().count() as i32,
            right_kind: "context".into(),
            right_content: right.into(),
            right_tokens: empty_token_model(),
        }
    }

    #[test]
    fn copies_unified_and_split_diff_selections_without_gutters() {
        let lines = ModelRc::new(VecModel::from(vec![
            slint_diff_line("alpha", "L-alpha", "R-alpha"),
            slint_diff_line("bravo", "L-bravo", "R-bravo"),
            slint_diff_line("charlie", "L-charlie", "R-charlie"),
        ]));

        assert_eq!(
            diff_selection_text(&lines, 0, 1, 2, 4, "").as_deref(),
            Some("lpha\nbravo\nchar")
        );
        assert_eq!(
            diff_selection_text(&lines, 1, 0, 1, 5, "left").as_deref(),
            Some("L-bra")
        );
        assert_eq!(
            diff_selection_text(&lines, 0, 2, 0, 7, "right").as_deref(),
            Some("alpha")
        );
    }

    #[test]
    fn copies_skip_hunk_and_meta_rows_like_github() {
        let mut hunk = slint_diff_line("@@ -1,2 +1,2 @@", "", "");
        hunk.kind = "hunk".into();
        let mut meta = slint_diff_line("Binary files differ", "", "");
        meta.kind = "meta".into();
        let lines = ModelRc::new(VecModel::from(vec![
            slint_diff_line("alpha", "L-alpha", "R-alpha"),
            hunk,
            meta,
            slint_diff_line("bravo", "L-bravo", "R-bravo"),
        ]));

        assert_eq!(
            diff_selection_text(&lines, 0, 0, 3, 5, "").as_deref(),
            Some("alpha\nbravo")
        );
        assert_eq!(diff_selection_text(&lines, 1, 0, 2, 8, ""), None);
    }

    #[test]
    fn diff_selection_columns_preserve_tabs_and_wide_glyphs() {
        assert_eq!(slice_display_columns("\t中x", 0, 4), "\t");
        assert_eq!(slice_display_columns("\t中x", 4, 6), "中");
        assert_eq!(slice_display_columns("\t中x", 6, 7), "x");
        assert_eq!(slice_display_columns("\t中x", 3, 5), "\t中");
    }

    #[test]
    fn diff_selection_hit_tracks_columns_across_wrapped_rows() {
        let wrapped = "01234567890123456789";
        let lines = ModelRc::new(VecModel::from(vec![
            slint_diff_line(wrapped, wrapped, wrapped),
            slint_diff_line(wrapped, wrapped, wrapped),
        ]));

        let forward = resolve_diff_selection_hit(&lines, 0, 40.0, 74.0, 44.0, 100.0, 10.0, "");
        assert_eq!((forward.line, forward.column), (1, 14));

        let reverse = resolve_diff_selection_hit(&lines, 1, 40.0, -10.0, 44.0, 100.0, 10.0, "");
        assert_eq!((reverse.line, reverse.column), (0, 14));

        // Right-half of a glyph rounds up so the character is included on copy.
        let rounded = resolve_diff_selection_hit(&lines, 0, 45.0, 10.0, 44.0, 100.0, 10.0, "");
        assert_eq!((rounded.line, rounded.column), (0, 5));
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
    fn commit_message_body_height_stays_compact_for_short_paragraphs() {
        let body = "Prevent pointer events on the loading overlay so the review header stays clickable.\n\nAlign the file diff statistics with the Files changed tab badge.";
        let height = commit_message_body_height(body);
        // Two short wrapped paragraphs + blank line should not reserve a tall gap.
        assert!(height > 0);
        assert!(
            height <= 90,
            "short body reserved {height}px; expected a compact GitHub-like band"
        );
    }

    #[test]
    fn commit_group_expand_extras_sum_every_expanded_row() {
        let make =
            |sha: &str, group_start: bool, group_size: i32, expanded: bool, body_height: i32| {
                WorkflowPrCommit {
                    sha: sha.into(),
                    short_sha: sha.into(),
                    title: "title".into(),
                    message: "title".into(),
                    message_body: "body".into(),
                    message_body_height: body_height,
                    message_expanded: expanded,
                    group_expand_extra: 0,
                    author: "alice".into(),
                    avatar: Default::default(),
                    avatar_label: "A".into(),
                    authored_at: "".into(),
                    date_label: "Jul 20, 2026".into(),
                    short_date_label: "2 days ago".into(),
                    group_start,
                    group_end: false,
                    group_size,
                    verified: false,
                }
            };
        let mut commits = vec![
            make("aaa", true, 2, true, 40),
            make("bbb", false, 0, true, 20),
        ];
        refresh_commit_group_expand_extras(&mut commits);
        let expected = commit_tab_expand_layout_extra(40) + commit_tab_expand_layout_extra(20);
        assert_eq!(commits[0].group_expand_extra, expected);
        assert_eq!(commits[1].group_expand_extra, 0);
        assert!(commits[0].message_expanded && commits[1].message_expanded);
    }

    #[test]
    fn measures_conversation_rows_without_viewport_filler_space() {
        let item = |kind: &str, body: &str| PullRequestConversationItem {
            id: kind.to_string(),
            kind: kind.to_string(),
            author: "alice".to_string(),
            avatar_url: String::new(),
            author_association: String::new(),
            body: body.to_string(),
            created_at: String::new(),
            state: String::new(),
        };
        let items = vec![
            item("description", ""),
            item("commits", "added 2 commits"),
            item("commit", "first"),
            item("commit", "second"),
            item("merged", "abcdef0"),
            item("event", "closed this pull request"),
        ];

        assert_eq!(conversation_content_height(&items), 304);
        assert_eq!(conversation_card_row_height("请先解决冲突"), 92);
        assert_eq!(conversation_card_row_height(""), 88);
    }

    #[test]
    fn connects_adjacent_review_pairs_without_crossing_terminal_merge() {
        let item = |kind: &str| PullRequestConversationItem {
            id: kind.to_string(),
            kind: kind.to_string(),
            author: "alice".to_string(),
            avatar_url: String::new(),
            author_association: String::new(),
            body: String::new(),
            created_at: String::new(),
            state: String::new(),
        };
        let summary = item("review_summary");
        let review = item("review");
        let next_summary = item("review_summary");
        let next_review = item("review");
        let merged = item("merged");

        assert!(conversation_items_connect(Some(&summary), Some(&review)));
        assert!(conversation_items_connect(
            Some(&review),
            Some(&next_summary)
        ));
        assert!(conversation_items_connect(
            Some(&next_summary),
            Some(&next_review)
        ));
        assert!(!conversation_items_connect(Some(&merged), Some(&summary)));
    }

    #[test]
    fn labels_single_and_multi_commit_ranges() {
        let commit = |sha: &str| PullRequestCommit {
            sha: sha.to_string(),
            title: String::new(),
            message: String::new(),
            author: String::new(),
            avatar_url: String::new(),
            authored_at: String::new(),
            verified: false,
        };
        let commits = vec![
            commit("1111111aaaaaaaa"),
            commit("2222222bbbbbbbb"),
            commit("3333333cccccccc"),
        ];
        assert_eq!(commit_range_label(&commits, 1, 1), "Commit 2222222");
        assert_eq!(commit_range_label(&commits, 0, 2), "1111111 … 3333333");
    }

    #[test]
    fn commit_detail_back_point_is_only_created_from_content_tabs() {
        assert_eq!(commit_detail_source_tab(0), Some(0));
        assert_eq!(commit_detail_source_tab(1), Some(1));
        assert_eq!(commit_detail_source_tab(2), None);
        assert_eq!(commit_detail_source_tab(99), None);
    }

    #[test]
    fn resolves_commit_detail_selection_by_full_sha() {
        let commit = |sha: &str| PullRequestCommit {
            sha: sha.to_string(),
            title: String::new(),
            message: String::new(),
            author: String::new(),
            avatar_url: String::new(),
            authored_at: String::new(),
            verified: false,
        };
        let commits = vec![commit("1111111aaaa"), commit("2222222bbbb")];
        assert_eq!(commit_index_by_sha(&commits, "2222222bbbb"), Some(1));
        assert_eq!(commit_index_by_sha(&commits, "2222222"), None);
    }

    #[test]
    fn maps_newest_first_display_ranges_onto_oldest_first_provider_lists() {
        assert_eq!(display_commit_range_to_chrono(0, 0, 3), Some((2, 2)));
        assert_eq!(display_commit_range_to_chrono(0, 2, 3), Some((0, 2)));
        assert_eq!(display_commit_range_to_chrono(0, 1, 3), Some((1, 2)));
        assert_eq!(display_commit_range_to_chrono(2, 0, 3), Some((0, 2)));
        assert_eq!(chrono_commit_index_to_display(0, 3), Some(2));
        assert_eq!(chrono_commit_index_to_display(2, 3), Some(0));
        assert_eq!(display_commit_range_to_chrono(0, 0, 0), None);
        assert_eq!(chrono_commit_index_to_display(0, 0), None);
    }

    #[test]
    fn reuses_the_full_diff_when_the_selected_range_covers_every_commit() {
        assert!(range_covers_all_commits(1, 0, 0));
        assert!(range_covers_all_commits(3, 0, 2));
        assert!(!range_covers_all_commits(3, 0, 1));
        assert!(!range_covers_all_commits(3, 1, 2));
        assert!(!range_covers_all_commits(0, 0, 0));
    }

    #[test]
    fn clones_only_the_file_that_receives_expanded_context() {
        let original = file("src/lib.rs");
        let base_detail = Arc::new(PullRequestDiffDetail {
            files: vec![original.clone(), file("src/main.rs")],
            added_lines: 2,
            removed_lines: 0,
            commit_count: 2,
            conversation_count: Some(0),
            base_sha: "base".to_string(),
            head_sha: "head".to_string(),
            start_sha: "start".to_string(),
            too_many_commits: false,
        });
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
        assert_eq!(cache.file(1), base_detail.files.get(1));
        assert_eq!(cache.collapse_file(0), Some(original));
        assert!(cache.expanded_files.is_empty());
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

    #[test]
    fn bounded_cache_evicts_the_least_recently_used_entry() {
        let mut cache = BoundedCache::new(2);
        cache.insert("first".to_string(), 1);
        cache.insert("second".to_string(), 2);
        assert_eq!(cache.get_cloned("first"), Some(1));
        cache.insert("third".to_string(), 3);
        assert_eq!(cache.get_cloned("second"), None);
        assert_eq!(cache.get_cloned("first"), Some(1));
        assert_eq!(cache.get_cloned("third"), Some(3));
    }

    #[test]
    fn split_diff_projection_pairs_change_blocks_and_duplicates_context() {
        let lines = vec![
            PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: "@@ -1,3 +1,4 @@".to_string(),
            },
            PullRequestDiffLine {
                old_line: 1,
                new_line: 1,
                kind: "context".to_string(),
                content: "same".to_string(),
            },
            PullRequestDiffLine {
                old_line: 2,
                new_line: 0,
                kind: "deletion".to_string(),
                content: "old one".to_string(),
            },
            PullRequestDiffLine {
                old_line: 3,
                new_line: 0,
                kind: "deletion".to_string(),
                content: "old two".to_string(),
            },
            PullRequestDiffLine {
                old_line: 0,
                new_line: 2,
                kind: "addition".to_string(),
                content: "new one".to_string(),
            },
        ];

        assert_eq!(
            diff_row_projection(lines.as_slice(), true),
            vec![
                DiffRowProjection::Split {
                    left: Some(0),
                    right: None,
                    source_index: 0,
                    spanning: true,
                },
                DiffRowProjection::Split {
                    left: Some(1),
                    right: Some(1),
                    source_index: 1,
                    spanning: false,
                },
                DiffRowProjection::Split {
                    left: Some(2),
                    right: Some(4),
                    source_index: 2,
                    spanning: false,
                },
                DiffRowProjection::Split {
                    left: Some(3),
                    right: None,
                    source_index: 3,
                    spanning: false,
                },
            ]
        );
    }

    #[test]
    fn split_diff_projection_keeps_pure_additions_on_the_right() {
        let lines = vec![
            PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: "@@ -0,0 +1,2 @@".to_string(),
            },
            PullRequestDiffLine {
                old_line: 0,
                new_line: 1,
                kind: "addition".to_string(),
                content: "first".to_string(),
            },
            PullRequestDiffLine {
                old_line: 0,
                new_line: 2,
                kind: "addition".to_string(),
                content: "second".to_string(),
            },
        ];

        assert_eq!(
            diff_row_projection(lines.as_slice(), true),
            vec![
                DiffRowProjection::Split {
                    left: Some(0),
                    right: None,
                    source_index: 0,
                    spanning: true,
                },
                DiffRowProjection::Split {
                    left: None,
                    right: Some(1),
                    source_index: 1,
                    spanning: false,
                },
                DiffRowProjection::Split {
                    left: None,
                    right: Some(2),
                    source_index: 2,
                    spanning: false,
                },
            ]
        );
    }

    #[test]
    fn lazy_diff_model_materializes_only_a_bounded_row_window() {
        let mut file = file("src/lib.rs");
        file.lines = (0..128)
            .map(|index| PullRequestDiffLine {
                old_line: index + 1,
                new_line: index + 1,
                kind: "context".to_string(),
                content: format!("line {index}"),
            })
            .collect();
        let highlighted = (0..128)
            .map(|index| HighlightedLine {
                tokens: vec![HighlightedToken {
                    text: format!("line {index}"),
                    color: [0xff, 0x24, 0x29, 0x2f],
                    dark_color: [0xff, 0xe6, 0xed, 0xf3],
                    bold: false,
                    italic: false,
                }],
                columns: 8,
            })
            .collect::<Vec<_>>();
        let model = LazyPrDiffLineModel::new(Arc::new(file), Arc::new(highlighted), false);

        assert_eq!(model.row_count(), 128);
        assert_eq!(model.cached_row_count(), 0);
        let row = model.row_data(5).expect("row 5");
        assert_eq!(row.new_line, 6);
        assert_eq!(row.content.as_str(), "line 5");
        assert_eq!(model.cached_row_count(), 1);
        assert!(model.row_data(128).is_none());

        for index in 0..128 {
            assert!(model.row_data(index).is_some());
        }
        assert_eq!(model.cached_row_count(), VISIBLE_LINE_CACHE_CAPACITY);
    }
}
