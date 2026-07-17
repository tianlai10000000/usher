use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use slint::{Color, ComponentHandle, ModelRc, VecModel};

use crate::git::{
    PullRequestDiffDetail, PullRequestDiffFile, PullRequestDiffLine, get_pull_request_diff,
    get_repository_file_text, supports_ignore_whitespace,
};
use crate::project::{ProjectRepositoryMetadata, load_repository_metadata};
use crate::{
    MainWindow, WorkflowPrDiffFile, WorkflowPrDiffLine, WorkflowPrDiffToken,
    WorkflowPrFileExtensionFilter, WorkflowPrTreeItem,
};

use super::bindings::{show_error, show_success};
use super::syntax_highlight::{HighlightedLine, HighlightedToken, highlight_diff_lines};

const HIGHLIGHT_CACHE_CAPACITY: usize = 4;
const SOURCE_CACHE_CAPACITY: usize = 3;

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

#[derive(Debug, Clone)]
struct ReviewCache {
    project_alias: String,
    request: ReviewRequest,
    metadata: ProjectRepositoryMetadata,
    detail: PullRequestDiffDetail,
    original_files: Vec<PullRequestDiffFile>,
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
    highlighted_files: Arc<Mutex<BoundedCache<Vec<HighlightedLine>>>>,
    source_files: Arc<Mutex<BoundedCache<String>>>,
    request_sequence: Arc<AtomicU64>,
    highlight_sequence: Arc<AtomicU64>,
    expand_sequence: Arc<AtomicU64>,
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
        highlighted_files: Arc::new(Mutex::new(BoundedCache::new(HIGHLIGHT_CACHE_CAPACITY))),
        source_files: Arc::new(Mutex::new(BoundedCache::new(SOURCE_CACHE_CAPACITY))),
        request_sequence: Arc::new(AtomicU64::new(0)),
        highlight_sequence: Arc::new(AtomicU64::new(0)),
        expand_sequence: Arc::new(AtomicU64::new(0)),
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

    let close_controller = controller.clone();
    let ui_weak = ui.as_weak();
    ui.on_workflow_pr_review_close(move || {
        close_controller.log_memory_diagnostics("review_close.before_hide");
        close_controller
            .request_sequence
            .fetch_add(1, Ordering::SeqCst);
        close_controller
            .highlight_sequence
            .fetch_add(1, Ordering::SeqCst);
        close_controller
            .expand_sequence
            .fetch_add(1, Ordering::SeqCst);
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_workflow_pr_review_visible(false);
            ui.set_workflow_pr_review_loading(false);
            ui.set_workflow_pr_review_code_loading(false);
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
            reload_controller.open(&ui, request);
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
    ui.on_workflow_pr_review_expand_hunk(move |line_index| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        expand_controller.expand_hunk(&ui, line_index);
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

    controller
}

impl WorkflowPrReviewController {
    pub(crate) fn select_project(&self, ui: &MainWindow, project_alias: String) {
        self.log_memory_diagnostics("project_switch.before_clear");
        self.request_sequence.fetch_add(1, Ordering::SeqCst);
        self.highlight_sequence.fetch_add(1, Ordering::SeqCst);
        self.expand_sequence.fetch_add(1, Ordering::SeqCst);
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
        if let Ok(mut highlighted) = self.highlighted_files.lock() {
            highlighted.clear();
        }
        if let Ok(mut sources) = self.source_files.lock() {
            sources.clear();
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
            if let Ok(mut highlighted) = self.highlighted_files.lock() {
                highlighted.clear();
            }
            if let Ok(mut sources) = self.source_files.lock() {
                sources.clear();
            }
            ui.set_workflow_pr_review_file_filter("".into());
            self.log_memory_diagnostics("new_review.after_cache_clear");
        }

        apply_review_header(ui, &request);
        ui.set_workflow_pr_review_visible(true);
        ui.set_workflow_pr_review_error("".into());
        if let Ok(mut active_request) = self.active_request.lock() {
            *active_request = Some(request.clone());
        }

        let cached = self.cache.lock().ok().and_then(|guard| guard.clone());
        if let Some(cache) =
            cached.filter(|cache| cache.project_alias == project_alias && cache.request == request)
        {
            ui.set_workflow_pr_review_loading(false);
            self.apply_detail(ui, &cache.detail);
            self.log_memory_diagnostics("review_open.cache_hit_applied");
            return;
        }

        self.highlight_sequence.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut highlighted) = self.highlighted_files.lock() {
            highlighted.clear();
        }

        ui.set_workflow_pr_review_loading(true);
        ui.set_workflow_pr_review_code_loading(false);
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
                        if let Ok(mut cache) = controller.cache.lock() {
                            *cache = Some(ReviewCache {
                                project_alias,
                                request,
                                metadata,
                                detail: detail.clone(),
                                original_files: detail.files.clone(),
                            });
                        }
                        controller.apply_detail(&ui, &detail);
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

    fn select_file(&self, ui: &MainWindow, index: i32) {
        let cache = self.cache.lock().ok().and_then(|guard| guard.clone());
        let Some(cache) = cache else {
            return;
        };
        let index = index.max(0) as usize;
        let Some(file) = cache.detail.files.get(index) else {
            return;
        };
        ui.set_workflow_pr_review_viewing_file(false);
        ui.set_workflow_pr_review_has_expandable_hunks(false);
        ui.set_workflow_pr_review_context_expanded(
            cache
                .original_files
                .get(index)
                .map(|original| original.lines != file.lines)
                .unwrap_or(false),
        );
        let reset_scroll = ui.get_workflow_pr_review_selected_file_path().as_str() != file.path;
        apply_file_header(ui, index, file);
        let sequence = self.highlight_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let cached = self
            .highlighted_files
            .lock()
            .ok()
            .and_then(|mut highlighted| highlighted.get_cloned(file.path.as_str()));
        if let Some(highlighted) = cached {
            apply_highlighted_file(ui, file, highlighted.as_slice(), reset_scroll);
            ui.set_workflow_pr_review_code_loading(false);
            self.log_memory_diagnostics("file_highlight.cache_hit_applied");
            return;
        }

        // Keep the previous file/model mounted while syntax highlighting runs.
        // Clearing it here causes a full white flash on file switches and hunk
        // expansion. The completed model is applied atomically on the UI loop.
        ui.set_workflow_pr_review_code_loading(true);
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        let file = file.clone();
        crate::app_runtime::shared_runtime().spawn(async move {
            let file_for_highlight = file.clone();
            let highlighted = tokio::task::spawn_blocking(move || {
                highlight_diff_lines(
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
                    ui.set_workflow_pr_review_error(
                        "Failed to render syntax highlighting for this file.".into(),
                    );
                    return;
                };
                if let Ok(mut cached) = controller.highlighted_files.lock() {
                    cached.insert(file.path.clone(), highlighted.clone());
                }
                apply_highlighted_file(&ui, &file, highlighted.as_slice(), reset_scroll);
                ui.set_workflow_pr_review_code_loading(false);
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

    fn expand_hunk(&self, ui: &MainWindow, line_index: i32) {
        let selected_index = ui.get_workflow_pr_review_selected_file_index().max(0) as usize;
        log::debug!(
            "[Workflow][PR Review] Expand hunk requested: file_index={} line_index={}",
            selected_index,
            line_index
        );
        let cached = self.cache.lock().ok().and_then(|cache| cache.clone());
        let Some(cache) = cached else {
            log::warn!("[Workflow][PR Review] Ignored hunk expansion without review cache");
            return;
        };
        let Some(file) = cache.detail.files.get(selected_index) else {
            log::warn!(
                "[Workflow][PR Review] Ignored hunk expansion for invalid file index: {}",
                selected_index
            );
            return;
        };
        let line_index = line_index.max(0) as usize;
        if !hunk_is_expandable(file, line_index) {
            log::debug!(
                "[Workflow][PR Review] Hunk no longer has hidden context: file={} line_index={}",
                file.path,
                line_index
            );
            return;
        }

        let use_new_side = file.change_type != "deleted_file";
        let path = if use_new_side {
            file.path.clone()
        } else {
            file.old_path.clone()
        };
        let reference = if use_new_side {
            cache.detail.head_sha.clone()
        } else {
            cache.detail.base_sha.clone()
        };
        if path.is_empty() || reference.is_empty() {
            ui.set_workflow_pr_review_error(
                "This diff has no immutable source reference for expanding context.".into(),
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
            self.apply_hunk_expansion(ui, selected_index, line_index, source.as_str());
            return;
        }

        let sequence = self.expand_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        let metadata = cache.metadata;
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
                        controller.apply_hunk_expansion(
                            &ui,
                            selected_index,
                            line_index,
                            source.as_str(),
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
        let cached = self.cache.lock().ok().and_then(|cache| cache.clone());
        let Some(cache) = cached else {
            return;
        };
        let Some(file) = cache.detail.files.get(selected_index) else {
            return;
        };
        if !file
            .lines
            .iter()
            .enumerate()
            .any(|(index, _)| hunk_is_expandable(file, index))
        {
            return;
        }

        let use_new_side = file.change_type != "deleted_file";
        let path = if use_new_side {
            file.path.clone()
        } else {
            file.old_path.clone()
        };
        let reference = if use_new_side {
            cache.detail.head_sha.clone()
        } else {
            cache.detail.base_sha.clone()
        };
        if path.is_empty() || reference.is_empty() {
            ui.set_workflow_pr_review_error(
                "This diff has no immutable source reference for expanding context.".into(),
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
            self.apply_all_hunk_expansion(ui, selected_index, source.as_str());
            return;
        }

        ui.set_workflow_pr_review_code_loading(true);
        let sequence = self.expand_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        let metadata = cache.metadata;
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
                        controller.apply_all_hunk_expansion(&ui, selected_index, source.as_str());
                    }
                    Err(error) => {
                        ui.set_workflow_pr_review_code_loading(false);
                        log::warn!("[Workflow][PR Review] Failed to expand all hunks: {error}");
                    }
                }
            });
        });
    }

    fn apply_all_hunk_expansion(&self, ui: &MainWindow, file_index: usize, source: &str) {
        let updated = self.cache.lock().ok().and_then(|mut cache| {
            let cache = cache.as_mut()?;
            let file = cache.detail.files.get_mut(file_index)?;
            (expand_all_hunk_context(file, source) > 0).then(|| file.clone())
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
            let original = cache.original_files.get(selected_index)?.clone();
            let file = cache.detail.files.get_mut(selected_index)?;
            if !restore_original_diff(file, &original) {
                return None;
            }
            Some(original)
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

        let cached = self.cache.lock().ok().and_then(|cache| cache.clone());
        let Some(cache) = cached else {
            return;
        };
        let Some(file) = cache.detail.files.get(selected_index).cloned() else {
            return;
        };
        let use_new_side = file.change_type != "deleted_file";
        let path = if use_new_side {
            file.path.clone()
        } else {
            file.old_path.clone()
        };
        let reference = if use_new_side {
            cache.detail.head_sha.clone()
        } else {
            cache.detail.base_sha.clone()
        };
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
        let metadata = cache.metadata;
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
        let full_file = full_source_file(&file, source.as_str());
        ui.set_workflow_pr_review_viewing_file(true);
        ui.set_workflow_pr_review_has_expandable_hunks(false);
        ui.set_workflow_pr_review_context_expanded(false);
        ui.set_workflow_pr_review_code_loading(true);
        let sequence = self.highlight_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let controller = self.clone();
        let ui_weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            let highlight_file = full_file.clone();
            let highlighted = tokio::task::spawn_blocking(move || {
                highlight_diff_lines(
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
                        apply_highlighted_file(&ui, &full_file, highlighted.as_slice(), true);
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
        source: &str,
    ) {
        let updated_file = self.cache.lock().ok().and_then(|mut cache| {
            let cache = cache.as_mut()?;
            let file = cache.detail.files.get_mut(file_index)?;
            expand_hunk_context(file, line_index, source, 20).then(|| file.clone())
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
            "[Workflow][PR Review] Expanded hunk context: file={} line_index={} limit=20",
            file.path,
            line_index
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
            .and_then(|guard| guard.as_ref().map(|cache| cache.detail.clone()));
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
        let (detail_stats, original_stats) = self
            .cache
            .lock()
            .ok()
            .and_then(|cache| {
                cache.as_ref().map(|cache| {
                    (
                        diff_storage_stats(&cache.detail.files),
                        diff_storage_stats(&cache.original_files),
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
        log::info!(
            "[Workflow][PR Review][Memory] reason={} detail_files={} detail_lines={} detail_text_bytes={} detail_heap_estimate_bytes={} original_files={} original_lines={} original_text_bytes={} original_heap_estimate_bytes={} highlight_cache_entries={} highlight_lines={} highlight_tokens={} highlight_text_bytes={} highlight_heap_estimate_bytes={} source_cache_entries={} source_text_bytes={} source_heap_estimate_bytes={}",
            reason,
            detail_stats.items,
            detail_stats.children,
            detail_stats.text_bytes,
            detail_stats.estimated_heap_bytes,
            original_stats.items,
            original_stats.children,
            original_stats.text_bytes,
            original_stats.estimated_heap_bytes,
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

fn highlighted_cache_stats(cache: &BoundedCache<Vec<HighlightedLine>>) -> OwnedMemoryStats {
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
        for line in lines {
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
    ui.set_workflow_pr_review_updated_at(request.updated_at.clone().into());
    ui.set_workflow_pr_review_ignore_whitespace(request.ignore_whitespace);
}

fn apply_detail_summary(ui: &MainWindow, detail: &PullRequestDiffDetail) {
    let files = detail.files.iter().map(to_slint_file).collect::<Vec<_>>();
    ui.set_workflow_pr_review_files(ModelRc::new(VecModel::from(files)));
    ui.set_workflow_pr_review_added_lines(detail.added_lines as i32);
    ui.set_workflow_pr_review_removed_lines(detail.removed_lines as i32);
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
    ui.set_workflow_pr_review_lines(empty_line_model());
    ui.set_workflow_pr_review_code_loading(false);
    ui.set_workflow_pr_review_context_expanded(false);
}

fn apply_file_header(ui: &MainWindow, index: usize, file: &PullRequestDiffFile) {
    ui.set_workflow_pr_review_selected_file_index(index as i32);
    ui.set_workflow_pr_review_selected_file_path(file.path.clone().into());
    ui.set_workflow_pr_review_selected_file_added_lines(file.added_lines as i32);
    ui.set_workflow_pr_review_selected_file_removed_lines(file.removed_lines as i32);
}

fn apply_highlighted_file(
    ui: &MainWindow,
    file: &PullRequestDiffFile,
    highlighted: &[HighlightedLine],
    reset_scroll: bool,
) {
    let max_columns = highlighted
        .iter()
        .map(|line| line.columns)
        .max()
        .unwrap_or(0);
    let lines = file
        .lines
        .iter()
        .enumerate()
        .zip(highlighted.iter())
        .map(|((index, line), highlighted)| to_slint_line(file, index, line, highlighted))
        .collect::<Vec<_>>();
    ui.set_workflow_pr_review_max_code_columns(max_columns as i32);
    ui.set_workflow_pr_review_lines(ModelRc::new(VecModel::from(lines)));
    ui.set_workflow_pr_review_has_expandable_hunks(
        !ui.get_workflow_pr_review_viewing_file()
            && file
                .lines
                .iter()
                .enumerate()
                .any(|(index, _)| hunk_is_expandable(file, index)),
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
            columns: token.columns as i32,
            indentation: token.indentation,
            bold: token.bold,
            italic: token.italic,
        })
        .collect::<Vec<_>>();
    WorkflowPrDiffLine {
        old_line: line.old_line,
        new_line: line.new_line,
        kind: line.kind.clone().into(),
        content: line.content.clone().into(),
        tokens: ModelRc::new(VecModel::from(tokens)),
        display_columns: highlighted.columns as i32,
        expandable: hunk_is_expandable(file, index),
    }
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

fn side_line(line: &PullRequestDiffLine, use_new_side: bool) -> Option<i32> {
    let number = if use_new_side {
        line.new_line
    } else {
        line.old_line
    };
    (number > 0).then_some(number)
}

fn expand_hunk_context(
    file: &mut PullRequestDiffFile,
    hunk_index: usize,
    source: &str,
    limit: usize,
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
    let reveal_start = hidden_start.max(hidden_end.saturating_sub(limit as i32 - 1));
    let source_lines = source.lines().collect::<Vec<_>>();
    if reveal_start <= 0 || reveal_start > hidden_end || hidden_end as usize > source_lines.len() {
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
    for number in reveal_start..=hidden_end {
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
    file.lines.splice(hunk_index + 1..hunk_index + 1, revealed);
    true
}

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
        if !expand_hunk_context(file, index, source, i32::MAX as usize) {
            break;
        }
    }
    file.lines.len().saturating_sub(initial_len)
}

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

fn reset_review_ui(ui: &MainWindow) {
    ui.set_workflow_pr_review_visible(false);
    ui.set_workflow_pr_review_loading(false);
    ui.set_workflow_pr_review_code_loading(false);
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
    ui.set_workflow_pr_review_added_lines(0);
    ui.set_workflow_pr_review_removed_lines(0);
    ui.set_workflow_pr_review_base_sha("".into());
    ui.set_workflow_pr_review_head_sha("".into());
    ui.set_workflow_pr_review_too_many_commits(false);
    ui.set_workflow_pr_review_has_expandable_hunks(false);
    ui.set_workflow_pr_review_context_expanded(false);
    ui.set_workflow_pr_review_viewing_file(false);
    ui.set_workflow_pr_review_ignore_whitespace_supported(false);
    ui.set_workflow_pr_review_ignore_whitespace(false);
    ui.set_workflow_pr_review_file_filter("".into());
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        BoundedCache, build_tree_items, expand_all_hunk_context, expand_hunk_context,
        full_source_file, hunk_is_expandable, restore_original_diff,
    };
    use crate::git::{PullRequestDiffFile, PullRequestDiffLine};

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
    fn expands_hidden_context_in_twenty_line_pages() {
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
        assert!(expand_hunk_context(&mut file, 0, source.as_str(), 20));
        assert_eq!(file.lines[1].new_line, 10);
        assert_eq!(file.lines[20].new_line, 29);
        assert!(hunk_is_expandable(&file, 0));

        assert!(expand_hunk_context(&mut file, 0, source.as_str(), 20));
        assert_eq!(file.lines[1].new_line, 1);
        assert!(!hunk_is_expandable(&file, 0));
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
}
