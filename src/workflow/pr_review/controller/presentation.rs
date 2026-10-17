//! Event-loop projections for the feature-owned PR UI root.
//! Models, row identity and layout revisions are committed here; provider I/O,
//! request generations and operation capture stay in the controller.
use super::*;

pub(super) fn apply_review_header(ui: &MainWindow, request: &ReviewRequest) {
    let page = ui.pr_review();
    let pr_state = page.global::<crate::PrReviewBridge>();
    let pr_review_view = ui.pr_review();
    let bridge = pr_review_view.global::<crate::PrConflictsBridge>();
    bridge.set_number(request.number.clone().into());
    bridge.set_repository(bridge.get_list_repository());
    let rows = ui.get_workflow_pr_list_items();
    bridge.set_status(
        (0..rows.row_count())
            .filter_map(|i| rows.row_data(i))
            .find(|row| row.number.as_str() == request.number)
            .map(|row| row.conflict_status)
            .unwrap_or_else(|| "unknown".into()),
    );
    pr_state.set_workflow_pr_review_number(request.number.clone().into());
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
    pr_state.set_workflow_pr_review_title(title.into());
    pr_state.set_workflow_pr_review_author(author.into());
    pr_state.set_workflow_pr_review_source_branch(request.source_branch.clone().into());
    pr_state.set_workflow_pr_review_target_branch(request.target_branch.clone().into());
    pr_state.set_workflow_pr_review_updated_at(
        crate::workflow::pr_list::format_github_relative_time(request.updated_at.as_str()).into(),
    );
    pr_state.set_workflow_pr_review_updated_at_tooltip(
        crate::workflow::pr_list::format_github_absolute_tooltip(request.updated_at.as_str())
            .into(),
    );
    pr_state.set_workflow_pr_review_ignore_whitespace(request.ignore_whitespace);
}

pub(super) fn apply_detail_summary(ui: &MainWindow, detail: &SharedDiffDetail) {
    let page = ui.pr_review();
    let pr_state = page.global::<crate::PrReviewBridge>();
    let files = detail
        .files
        .iter()
        .map(|file| to_slint_file(file))
        .collect::<Vec<_>>();
    pr_state.set_workflow_pr_review_files(ModelRc::new(VecModel::from(files)));
    pr_state.set_workflow_pr_review_added_lines(detail.added_lines as i32);
    pr_state.set_workflow_pr_review_removed_lines(detail.removed_lines as i32);
    // Prefer the larger of the summary field and any already-visible badge so a
    // later range compare with a missing total cannot wipe the Commits counter.
    let current_commits = pr_state.get_workflow_pr_review_commit_count();
    pr_state.set_workflow_pr_review_commit_count(current_commits.max(detail.commit_count as i32));
    if let Some(conversation_count) = detail.conversation_count {
        let current = pr_state.get_workflow_pr_review_conversation_count();
        pr_state.set_workflow_pr_review_conversation_count(current.max(conversation_count as i32));
    }
    pr_state.set_workflow_pr_review_base_sha(detail.base_sha.clone().into());
    pr_state.set_workflow_pr_review_head_sha(detail.head_sha.clone().into());
    pr_state.set_workflow_pr_review_too_many_commits(detail.too_many_commits);
    pr_state.set_workflow_pr_review_files_truncated(detail.files_truncated);
    pr_state.set_workflow_pr_review_error("".into());
    pr_state.set_workflow_pr_review_error_kind("".into());
}

pub(super) fn clear_selected_file(ui: &MainWindow) {
    let page = ui.pr_review();
    let pr_state = page.global::<crate::PrReviewBridge>();
    pr_state.set_workflow_pr_review_selected_file_index(-1);
    pr_state.set_workflow_pr_review_selected_file_path("".into());
    pr_state.set_workflow_pr_review_selected_file_added_lines(0);
    pr_state.set_workflow_pr_review_selected_file_removed_lines(0);
    pr_state.set_workflow_pr_review_max_code_columns(0);
    pr_state.set_workflow_pr_review_longest_code_line("".into());
    set_empty_line_model(ui);
    pr_state.set_workflow_pr_review_code_loading(false);
    pr_state.set_workflow_pr_review_large_diff_deferred(false);
    pr_state.set_workflow_pr_review_large_diff_loading(false);
    pr_state.set_workflow_pr_review_context_expanded(false);
}

pub(super) fn apply_file_header(ui: &MainWindow, index: usize, file: &PullRequestDiffFile) {
    let page = ui.pr_review();
    let pr_state = page.global::<crate::PrReviewBridge>();
    pr_state.set_workflow_pr_review_selected_file_index(index as i32);
    pr_state.set_workflow_pr_review_selected_file_path(file.path.clone().into());
    pr_state.set_workflow_pr_review_selected_file_added_lines(file.added_lines as i32);
    pr_state.set_workflow_pr_review_selected_file_removed_lines(file.removed_lines as i32);
}

pub(super) fn apply_highlighted_file(
    ui: &MainWindow,
    file: SharedDiffFile,
    highlighted: Arc<Vec<HighlightedLine>>,
    reset_scroll: bool,
) {
    let page = ui.pr_review();
    let pr_state = page.global::<crate::PrReviewBridge>();
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
    pr_state.set_workflow_pr_review_max_code_columns(max_columns as i32);
    pr_state.set_workflow_pr_review_longest_code_line(longest_code_line.into());
    let line_model = Rc::new(LazyPrDiffLineModel::new(
        file.clone(),
        highlighted,
        pr_state.get_workflow_pr_review_diff_view().as_str() == "split",
    ));
    let metrics = ModelRc::new(VecModel::from(line_model.row_metrics()));
    set_line_model(ui, ModelRc::from(line_model), metrics);
    pr_state.set_workflow_pr_review_has_expandable_hunks(
        !pr_state.get_workflow_pr_review_viewing_file()
            && file
                .lines
                .iter()
                .enumerate()
                .any(|(index, _)| hunk_is_expandable(&file, index)),
    );
    if pr_state.get_workflow_pr_review_hunk_scroll_restore_pending() {
        pr_state.set_workflow_pr_review_hunk_scroll_restore_pending(false);
        pr_state.set_workflow_pr_review_hunk_scroll_restore_revision(
            pr_state
                .get_workflow_pr_review_hunk_scroll_restore_revision()
                .wrapping_add(1),
        );
    }
    if reset_scroll {
        pr_state.set_workflow_pr_review_line_model_revision(
            pr_state
                .get_workflow_pr_review_line_model_revision()
                .wrapping_add(1),
        );
    }
}

pub(super) fn set_line_model(
    ui: &MainWindow,
    lines: ModelRc<WorkflowPrDiffLine>,
    metrics: ModelRc<WorkflowPrDiffRowMetric>,
) {
    let page = ui.pr_review();
    let pr_state = page.global::<crate::PrReviewBridge>();
    crate::workflow::pr_review::find_bridge::cancel();
    pr_state.set_workflow_pr_review_lines(lines);
    pr_state.set_workflow_pr_review_line_metrics(metrics);
    pr_state.set_workflow_pr_review_line_content_revision(
        pr_state
            .get_workflow_pr_review_line_content_revision()
            .wrapping_add(1),
    );
}

pub(super) fn set_empty_line_model(ui: &MainWindow) {
    set_line_model(ui, empty_line_model(), empty_line_metric_model());
}

pub(super) fn empty_file_model() -> ModelRc<WorkflowPrDiffFile> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrDiffFile>::new()))
}

pub(super) fn empty_line_model() -> ModelRc<WorkflowPrDiffLine> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrDiffLine>::new()))
}

pub(super) fn empty_line_metric_model() -> ModelRc<WorkflowPrDiffRowMetric> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrDiffRowMetric>::new()))
}

pub(super) fn empty_tree_model() -> ModelRc<WorkflowPrTreeItem> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrTreeItem>::new()))
}

pub(super) fn empty_extension_filter_model() -> ModelRc<WorkflowPrFileExtensionFilter> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrFileExtensionFilter>::new()))
}

pub(super) fn toggle_commit_message_expansion(ui: &MainWindow, sha: &str) {
    let page = ui.pr_review();
    let pr_state = page.global::<crate::PrReviewBridge>();
    let sha = sha.trim();
    if sha.is_empty() {
        return;
    }

    let conversation = pr_state.get_workflow_pr_review_conversation_items();
    let mut conversation_items = (0..conversation.row_count())
        .filter_map(|index| conversation.row_data(index))
        .collect::<Vec<_>>();
    let commits = pr_state.get_workflow_pr_review_commits();
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
    pr_state.set_workflow_pr_review_expanded_commit_body_height(expanded_body_height);

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

pub(super) fn reset_review_ui(ui: &MainWindow) {
    let page = ui.pr_review();
    let pr_state = page.global::<crate::PrReviewBridge>();
    pr_state.set_workflow_pr_review_visible(false);
    pr_state.set_workflow_pr_review_loading(false);
    pr_state.set_workflow_pr_review_code_loading(false);
    pr_state.set_workflow_pr_review_large_diff_deferred(false);
    pr_state.set_workflow_pr_review_large_diff_loading(false);
    pr_state.set_workflow_pr_review_location_row_index(-1);
    pr_state.set_workflow_pr_review_location_side("".into());
    pr_state.set_workflow_pr_review_error("".into());
    pr_state.set_workflow_pr_review_files(empty_file_model());
    pr_state.set_workflow_pr_review_tree_items(empty_tree_model());
    pr_state.set_workflow_pr_review_extension_filters(empty_extension_filter_model());
    pr_state.set_workflow_pr_review_only_manifest_files(false);
    pr_state.set_workflow_pr_review_show_viewed_files(true);
    pr_state.set_workflow_pr_review_file_filters_active(false);
    set_empty_line_model(ui);
    pr_state.set_workflow_pr_review_selected_file_index(-1);
    pr_state.set_workflow_pr_review_selected_file_path("".into());
    pr_state.set_workflow_pr_review_selected_file_added_lines(0);
    pr_state.set_workflow_pr_review_selected_file_removed_lines(0);
    pr_state.set_workflow_pr_review_max_code_columns(0);
    pr_state.set_workflow_pr_review_longest_code_line("".into());
    pr_state.set_workflow_pr_review_added_lines(0);
    pr_state.set_workflow_pr_review_removed_lines(0);
    pr_state.set_workflow_pr_review_commit_count(0);
    pr_state.set_workflow_pr_review_conversation_count(0);
    pr_state.set_workflow_pr_review_base_sha("".into());
    pr_state.set_workflow_pr_review_head_sha("".into());
    pr_state.set_workflow_pr_review_too_many_commits(false);
    pr_state.set_workflow_pr_review_files_truncated(false);
    pr_state.set_workflow_pr_review_tab(2);
    pr_state.set_workflow_pr_review_conversation_items(empty_conversation_model());
    pr_state.set_workflow_pr_review_conversation_content_height(0);
    pr_state.set_workflow_pr_review_expanded_commit_body_height(0);
    pr_state.set_workflow_pr_review_commits(empty_commit_model());
    pr_state.set_workflow_pr_review_conversation_loading(false);
    pr_state.set_workflow_pr_review_commits_loading(false);
    pr_state.set_workflow_pr_review_conversation_error("".into());
    pr_state.set_workflow_pr_review_commits_error("".into());
    pr_state.set_workflow_pr_review_commit_range_start(-1);
    pr_state.set_workflow_pr_review_commit_range_end(-1);
    pr_state.set_workflow_pr_review_commit_range_label(
        crate::i18n::tr("pr_review.all_commits").into_owned().into(),
    );
    pr_state.set_workflow_pr_review_has_expandable_hunks(false);
    pr_state.set_workflow_pr_review_context_expanded(false);
    pr_state.set_workflow_pr_review_viewing_file(false);
    pr_state.set_workflow_pr_review_ignore_whitespace_supported(false);
    pr_state.set_workflow_pr_review_ignore_whitespace(false);
    pr_state.set_workflow_pr_review_file_filter("".into());
    reset_action_ui(ui);
}

impl WorkflowPrReviewController {
    pub(crate) fn sync_language(&self, ui: &MainWindow) {
        let page = ui.pr_review();
        let pr_state = page.global::<crate::PrReviewBridge>();
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
            pr_state.set_workflow_pr_review_conversation_content_height(
                conversation_content_height(items.as_slice()),
            );
            pr_state.set_workflow_pr_review_expanded_commit_body_height(0);
            pr_state.set_workflow_pr_review_conversation_items(conversation_model(
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
            pr_state
                .set_workflow_pr_review_commits(commit_model(commits.as_slice(), &HashMap::new()));
        }
        if !pr_state
            .get_workflow_pr_review_conversation_error()
            .is_empty()
        {
            pr_state.set_workflow_pr_review_conversation_error(
                crate::i18n::tr("pr_review.conversation_load_failed")
                    .into_owned()
                    .into(),
            );
        }
        if !pr_state.get_workflow_pr_review_commits_error().is_empty() {
            pr_state.set_workflow_pr_review_commits_error(
                crate::i18n::tr("pr_review.commits_load_failed")
                    .into_owned()
                    .into(),
            );
        }
        if pr_state.get_workflow_pr_review_commit_range_start() < 0
            && pr_state.get_workflow_pr_review_commit_range_end() < 0
        {
            pr_state.set_workflow_pr_review_commit_range_label(
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
            retranslate_action_state(ui, &state);
        } else if !pr_state.get_workflow_pr_merge_readiness_kind().is_empty() {
            let label_key =
                readiness_label_key(pr_state.get_workflow_pr_merge_readiness_kind().as_str());
            pr_state.set_workflow_pr_merge_readiness_label(
                crate::i18n::tr(label_key).into_owned().into(),
            );
        }
    }

    pub(super) fn apply_detail(&self, ui: &MainWindow, detail: &SharedDiffDetail) {
        let page = ui.pr_review();
        let pr_state = page.global::<crate::PrReviewBridge>();
        apply_detail_summary(ui, detail);
        // Tab-row statistics describe the entire PR even while comparing a commit range.
        if let Some(all_detail) = self
            .cache
            .lock()
            .ok()
            .and_then(|cache| cache.as_ref().map(|cache| cache.all_detail.clone()))
        {
            pr_state.set_workflow_pr_review_added_lines(all_detail.added_lines as i32);
            pr_state.set_workflow_pr_review_removed_lines(all_detail.removed_lines as i32);
        }
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

    pub(super) fn refresh_tree(&self, ui: &MainWindow) {
        let page = ui.pr_review();
        let pr_state = page.global::<crate::PrReviewBridge>();
        let detail = self
            .cache
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|cache| cache.base_detail.clone()));
        let Some(detail) = detail else {
            pr_state.set_workflow_pr_review_tree_items(empty_tree_model());
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
        pr_state.set_workflow_pr_review_extension_filters(ModelRc::new(VecModel::from(
            extension_filters,
        )));
        pr_state.set_workflow_pr_review_only_manifest_files(only_manifest_files);
        pr_state.set_workflow_pr_review_show_viewed_files(show_viewed_files);
        pr_state.set_workflow_pr_review_file_filters_active(
            only_manifest_files || !show_viewed_files || selected_extensions.len() != counts.len(),
        );
        pr_state.set_workflow_pr_review_tree_items(ModelRc::new(VecModel::from(build_tree_items(
            detail.files.as_slice(),
            query.as_str(),
            &collapsed,
            &selected_extensions,
            only_manifest_files,
            show_viewed_files,
        ))));
    }
}
