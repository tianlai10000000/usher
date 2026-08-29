//! In-diff find matches for the Files changed toolbar search bar.
//!
//! GitHub's PR Files changed surface has no first-party find chrome (browser
//! find-in-page is used there). Usher docks a compact CodeMirror/editor-style
//! find bar in the selected-file toolbar and resolves matches against the
//! current projected line model.

use std::collections::HashMap;

use slint::{Model, ModelRc, SharedString, VecModel};
use usher_search::{MatchCase, find_non_overlapping};

use crate::workflow::syntax_highlight::character_columns;
use crate::{WorkflowPrDiffFindMatch, WorkflowPrDiffLine};

use super::diff_model::LazyPrDiffLineModel;
use super::diff_projection::DiffRowProjection;
use super::hunk::sanitize_diff_content;
use super::selection::soft_wrap_visual_line_index;

/// Case-insensitive substring matches over copyable code columns.
/// Split mode searches both panes and tags each hit with `left` / `right`.
///
/// Lazy diff models are scanned through their raw provider lines so a
/// full-file search never materializes converted rows (tokens, shared strings)
/// or churns the bounded visible-row cache.
pub(super) fn collect_diff_find_matches(
    lines: &ModelRc<WorkflowPrDiffLine>,
    query: &str,
    split: bool,
) -> Vec<WorkflowPrDiffFindMatch> {
    let needle = query.trim();
    if needle.is_empty() {
        return Vec::new();
    }
    if let Some(model) = lines.as_any().downcast_ref::<LazyPrDiffLineModel>() {
        return collect_source_find_matches(model, needle, split);
    }
    let mut matches = Vec::new();

    for index in 0..lines.row_count() {
        let Some(line) = lines.row_data(index) else {
            continue;
        };
        if !is_copyable_diff_line(&line) {
            continue;
        }
        if split {
            push_content_matches(
                &mut matches,
                index as i32,
                line.left_content.as_str(),
                needle,
                "left",
            );
            push_content_matches(
                &mut matches,
                index as i32,
                line.right_content.as_str(),
                needle,
                "right",
            );
        } else {
            push_content_matches(
                &mut matches,
                index as i32,
                line.content.as_str(),
                needle,
                "",
            );
        }
    }

    matches
}

/// Source-side scan mirroring the materialized-row semantics exactly: unified
/// queries only see unified projections (split rows expose empty `content`),
/// split queries only see split pane content, spanning rows surface `hunk` /
/// `meta` kinds and are never copyable.
fn collect_source_find_matches(
    model: &LazyPrDiffLineModel,
    needle: &str,
    split: bool,
) -> Vec<WorkflowPrDiffFindMatch> {
    let file = model.source_file();
    let mut matches = Vec::new();
    for (row, projection) in model.source_projection().iter().enumerate() {
        match *projection {
            DiffRowProjection::Unified(source_index) => {
                if split {
                    continue;
                }
                let Some(line) = file.lines.get(source_index) else {
                    continue;
                };
                if matches!(line.kind.as_str(), "hunk" | "meta") {
                    continue;
                }
                push_source_matches(&mut matches, row as i32, line.content.as_str(), needle, "");
            }
            DiffRowProjection::Split {
                left,
                right,
                spanning,
                ..
            } => {
                if !split || spanning {
                    continue;
                }
                if let Some(line) = left.and_then(|index| file.lines.get(index)) {
                    push_source_matches(
                        &mut matches,
                        row as i32,
                        line.content.as_str(),
                        needle,
                        "left",
                    );
                }
                if let Some(line) = right.and_then(|index| file.lines.get(index)) {
                    push_source_matches(
                        &mut matches,
                        row as i32,
                        line.content.as_str(),
                        needle,
                        "right",
                    );
                }
            }
        }
    }
    matches
}

/// Materialized rows carry sanitized content (tabs expanded, newlines
/// stripped); sanitize raw provider text only when it would differ.
fn push_source_matches(
    out: &mut Vec<WorkflowPrDiffFindMatch>,
    line: i32,
    raw_content: &str,
    needle: &str,
    side: &str,
) {
    if raw_content.contains(['\t', '\r', '\n']) {
        let sanitized = sanitize_diff_content(raw_content);
        push_content_matches(out, line, sanitized.as_str(), needle, side);
    } else {
        push_content_matches(out, line, raw_content, needle, side);
    }
}

/// O(1) per-row lookup over the most recent full find refresh. Rows without
/// matches share one empty model, so mounted-row bindings allocate nothing.
pub(super) struct DiffFindRowTable {
    query: String,
    rows: HashMap<(i32, u8), ModelRc<WorkflowPrDiffFindMatch>>,
    empty: ModelRc<WorkflowPrDiffFindMatch>,
}

impl DiffFindRowTable {
    pub(super) fn new() -> Self {
        Self {
            query: String::new(),
            rows: HashMap::new(),
            empty: ModelRc::new(VecModel::from(Vec::<WorkflowPrDiffFindMatch>::new())),
        }
    }

    pub(super) fn rebuild(&mut self, query: &str, matches: &[WorkflowPrDiffFindMatch]) {
        self.query = query.trim().to_string();
        let mut grouped: HashMap<(i32, u8), Vec<WorkflowPrDiffFindMatch>> = HashMap::new();
        for hit in matches {
            grouped
                .entry((hit.line, side_key(hit.side.as_str())))
                .or_default()
                .push(hit.clone());
        }
        self.rows = grouped
            .into_iter()
            .map(|(key, row_matches)| (key, ModelRc::new(VecModel::from(row_matches))))
            .collect();
    }

    /// `None` when the table was built for a different query; callers fall back
    /// to a single-row scan so paint stays correct during transient states.
    pub(super) fn lookup(
        &self,
        line: i32,
        side: &str,
        query: &str,
    ) -> Option<ModelRc<WorkflowPrDiffFindMatch>> {
        if query.trim() != self.query {
            return None;
        }
        if self.query.is_empty() {
            return Some(self.empty.clone());
        }
        Some(
            self.rows
                .get(&(line, side_key(side)))
                .cloned()
                .unwrap_or_else(|| self.empty.clone()),
        )
    }
}

fn side_key(side: &str) -> u8 {
    match side {
        "left" => 1,
        "right" => 2,
        _ => 0,
    }
}

/// Matches for one mounted row and pane, derived solely from this invocation's
/// query. This keeps paint projection independent from full-result refresh order.
pub(super) fn collect_diff_find_row_matches(
    lines: &ModelRc<WorkflowPrDiffLine>,
    line_index: i32,
    side: &str,
    query: &str,
) -> ModelRc<WorkflowPrDiffFindMatch> {
    let needle = query.trim();
    if needle.is_empty() || line_index < 0 {
        return ModelRc::new(VecModel::from(Vec::<WorkflowPrDiffFindMatch>::new()));
    }
    let Some(line) = lines.row_data(line_index as usize) else {
        return ModelRc::new(VecModel::from(Vec::<WorkflowPrDiffFindMatch>::new()));
    };
    if !is_copyable_diff_line(&line) {
        return ModelRc::new(VecModel::from(Vec::<WorkflowPrDiffFindMatch>::new()));
    }
    let content = match side {
        "left" => line.left_content.as_str(),
        "right" => line.right_content.as_str(),
        _ => line.content.as_str(),
    };
    let mut matches = Vec::new();
    push_content_matches(&mut matches, line_index, content, needle, side);
    ModelRc::new(VecModel::from(matches))
}

/// Resolves a match to its visual offset inside one exact-height virtual row.
pub(super) fn diff_find_intra_item_offset(
    lines: &ModelRc<WorkflowPrDiffLine>,
    target_line: i32,
    start_char: i32,
    code_width: f32,
    cell_width: f32,
    side: &str,
) -> f32 {
    let row_count = lines.row_count();
    if row_count == 0 {
        return 0.0;
    }
    let target_index = target_line.clamp(0, row_count.saturating_sub(1) as i32) as usize;

    estimate_diff_match_intra_row_y(
        lines,
        target_index,
        start_char,
        code_width,
        cell_width,
        side,
    )
}

fn estimate_diff_match_intra_row_y(
    lines: &ModelRc<WorkflowPrDiffLine>,
    target_line: usize,
    start_char: i32,
    code_width: f32,
    cell_width: f32,
    side: &str,
) -> f32 {
    const CODE_LINE_HEIGHT: f32 = 22.0;
    if target_line >= lines.row_count() {
        return 0.0;
    }
    let Some(line) = lines.row_data(target_line) else {
        return 0.0;
    };
    let (content, columns) = match side {
        "left" => (line.left_content.as_str(), line.left_columns),
        "right" => (line.right_content.as_str(), line.right_columns),
        _ => (line.content.as_str(), line.left_columns),
    };
    let safe_code_width = code_width.max(1.0);
    let safe_cell_width = cell_width.max(0.1);
    if columns.max(0) as f32 * safe_cell_width <= safe_code_width + 0.5 {
        return 0.0;
    }
    let max_cells = (safe_code_width / safe_cell_width).floor().max(1.0) as i32;
    soft_wrap_visual_line_index(content, start_char, max_cells) as f32 * CODE_LINE_HEIGHT
}

fn is_copyable_diff_line(line: &WorkflowPrDiffLine) -> bool {
    !matches!(line.kind.as_str(), "hunk" | "meta")
}

fn push_content_matches(
    out: &mut Vec<WorkflowPrDiffFindMatch>,
    line: i32,
    content: &str,
    needle_lower: &str,
    side: &str,
) {
    if content.is_empty() || needle_lower.is_empty() {
        return;
    }
    for hit in find_non_overlapping(content, needle_lower, MatchCase::Insensitive) {
        let start_byte = hit.start_byte;
        let end_byte = hit.end_byte;
        let prefix = &content[..start_byte];
        let text = &content[start_byte..end_byte];
        let start_column = display_column_at_byte(content, start_byte);
        let end_column = display_column_at_byte(content, end_byte);
        out.push(WorkflowPrDiffFindMatch {
            line,
            start_column: start_column as i32,
            end_column: end_column as i32,
            start_char: hit.start_char as i32,
            end_char: hit.end_char as i32,
            side: side.into(),
            prefix: SharedString::from(prefix),
            text: SharedString::from(text),
        });
    }
}

fn display_column_at_byte(value: &str, byte: usize) -> u32 {
    let mut column = 0_u32;
    for (offset, character) in value.char_indices() {
        if offset >= byte {
            break;
        }
        column += if character == '\t' {
            4 - column % 4
        } else {
            character_columns(character)
        };
    }
    column
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use std::sync::Arc;

    use slint::{Model, ModelRc, VecModel};

    use super::super::diff_model::LazyPrDiffLineModel;
    use super::{
        DiffFindRowTable, collect_diff_find_matches, collect_diff_find_row_matches,
        diff_find_intra_item_offset,
    };
    use crate::git::{PullRequestDiffFile, PullRequestDiffLine};
    use crate::workflow::syntax_highlight::HighlightedLine;
    use crate::{WorkflowPrDiffLine, WorkflowPrDiffToken};

    fn line(content: &str, left: &str, right: &str) -> WorkflowPrDiffLine {
        let empty_tokens = || ModelRc::new(VecModel::from(Vec::<WorkflowPrDiffToken>::new()));
        WorkflowPrDiffLine {
            old_line: 0,
            new_line: 1,
            kind: "context".into(),
            content: content.into(),
            tokens: empty_tokens(),
            expandable: false,
            expand_all: false,
            expand_up: false,
            expand_down: false,
            source_index: 0,
            left_line: 1,
            left_columns: left.chars().count() as i32,
            left_kind: "context".into(),
            left_content: left.into(),
            left_tokens: empty_tokens(),
            right_line: 1,
            right_columns: right.chars().count() as i32,
            right_kind: "context".into(),
            right_content: right.into(),
            right_tokens: empty_tokens(),
        }
    }

    #[test]
    fn finds_case_insensitive_substrings_and_skips_hunks() {
        let mut hunk = line("@@ -1 +1 @@", "", "");
        hunk.kind = "hunk".into();
        let lines = ModelRc::new(VecModel::from(vec![
            line("Alpha beta", "L-Alpha", "R-beta"),
            hunk,
            line("alpha again", "L-alpha", "R-again"),
        ]));

        let matches = collect_diff_find_matches(&lines, "ALP", false);
        assert_eq!(matches.len(), 2);
        let first = &matches[0];
        assert_eq!(first.line, 0);
        assert_eq!(first.start_column, 0);
        assert_eq!(first.end_column, 3);
        assert_eq!(first.start_char, 0);
        assert_eq!(first.end_char, 3);
        assert_eq!(first.prefix.as_str(), "");
        assert_eq!(first.text.as_str(), "Alp");
        assert_eq!(first.side.as_str(), "");
        assert_eq!(matches[1].line, 2);
    }

    #[test]
    fn cjk_match_keeps_prefix_and_display_columns() {
        let content = "确保开发者和 coding agent";
        let lines = ModelRc::new(VecModel::from(vec![line(content, content, content)]));
        let matches = collect_diff_find_matches(&lines, "保开发者", false);
        assert_eq!(matches.len(), 1);
        let hit = &matches[0];
        assert_eq!(hit.prefix.as_str(), "确");
        assert_eq!(hit.text.as_str(), "保开发者");
        assert_eq!(hit.start_char, 1);
        assert_eq!(hit.end_char, 5);
        // East-Asian width columns: 确(2) then 保开发者(8).
        assert_eq!(hit.start_column, 2);
        assert_eq!(hit.end_column, 10);
    }

    #[test]
    fn split_search_tags_sides() {
        let lines = ModelRc::new(VecModel::from(vec![line(
            "ignored",
            "left-needle",
            "right-needle-extra",
        )]));
        let matches = collect_diff_find_matches(&lines, "needle", true);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].side.as_str(), "left");
        assert_eq!(matches[1].side.as_str(), "right");
    }

    #[test]
    fn same_line_occurrences_are_distinct_non_overlapping_matches() {
        let lines = ModelRc::new(VecModel::from(vec![line(
            "find find find",
            "find find find",
            "find find find",
        )]));
        let matches = collect_diff_find_matches(&lines, "find", false);
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].start_char, 0);
        assert_eq!(matches[1].start_char, 5);
        assert_eq!(matches[2].start_char, 10);
    }

    #[test]
    fn row_paint_matches_follow_each_query_without_shared_refresh_state() {
        let content = "确保开发者 保持 Usher";
        let lines = ModelRc::new(VecModel::from(vec![line(content, content, content)]));

        let longer = collect_diff_find_row_matches(&lines, 0, "", "保持");
        assert_eq!(longer.row_count(), 1);
        assert_eq!(
            longer.row_data(0).map(|hit| hit.end_char - hit.start_char),
            Some(2)
        );

        let shorter = collect_diff_find_row_matches(&lines, 0, "", "保");
        assert_eq!(shorter.row_count(), 2);
        assert!(shorter.iter().all(|hit| hit.end_char - hit.start_char == 1));

        let cleared = collect_diff_find_row_matches(&lines, 0, "", "");
        assert_eq!(cleared.row_count(), 0);
    }

    #[test]
    fn wrapped_matches_use_their_visual_subline() {
        let content = "alpha beta gamma delta epsilon";
        let lines = ModelRc::new(VecModel::from(vec![line(content, content, content)]));
        let viewport_y = diff_find_intra_item_offset(&lines, 0, 23, 72.0, 7.2, "");
        assert!(viewport_y > 0.0);
    }

    fn source_line(old_line: i32, new_line: i32, kind: &str, content: &str) -> PullRequestDiffLine {
        PullRequestDiffLine {
            old_line,
            new_line,
            kind: kind.to_string(),
            content: content.to_string(),
        }
    }

    fn lazy_model(split: bool) -> ModelRc<WorkflowPrDiffLine> {
        let lines = vec![
            source_line(0, 0, "hunk", "@@ -1,3 +1,4 @@"),
            source_line(1, 1, "context", "shared Alpha context"),
            source_line(2, 0, "deletion", "\told alpha value"),
            source_line(3, 0, "deletion", "old two"),
            source_line(0, 2, "addition", "new ALPHA 确保开发者 value"),
        ];
        let highlighted = lines
            .iter()
            .map(|line| HighlightedLine {
                tokens: Vec::new(),
                columns: line.content.chars().count() as u32,
            })
            .collect::<Vec<_>>();
        let file = PullRequestDiffFile {
            path: "src/lib.rs".to_string(),
            old_path: "src/lib.rs".to_string(),
            change_type: "edit_file".to_string(),
            added_lines: 1,
            removed_lines: 2,
            viewed: false,
            lines,
        };
        ModelRc::from(Rc::new(LazyPrDiffLineModel::new(
            Arc::new(file),
            Arc::new(highlighted),
            split,
        )))
    }

    fn materialized_copy(lines: &ModelRc<WorkflowPrDiffLine>) -> ModelRc<WorkflowPrDiffLine> {
        ModelRc::new(VecModel::from(
            (0..lines.row_count())
                .filter_map(|index| lines.row_data(index))
                .collect::<Vec<_>>(),
        ))
    }

    #[track_caller]
    fn assert_same_matches(lines: &ModelRc<WorkflowPrDiffLine>, query: &str, split: bool) {
        let flatten = |matches: Vec<crate::WorkflowPrDiffFindMatch>| {
            matches
                .into_iter()
                .map(|hit| {
                    (
                        hit.line,
                        hit.start_column,
                        hit.end_column,
                        hit.start_char,
                        hit.end_char,
                        hit.side.to_string(),
                        hit.prefix.to_string(),
                        hit.text.to_string(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let source_scan = flatten(collect_diff_find_matches(lines, query, split));
        let row_scan = flatten(collect_diff_find_matches(
            &materialized_copy(lines),
            query,
            split,
        ));
        assert_eq!(source_scan, row_scan, "query {query:?} split {split}");
        assert!(
            !source_scan.is_empty(),
            "query {query:?} split {split} should produce matches"
        );
    }

    #[test]
    fn source_scan_matches_materialized_rows_in_unified_mode() {
        let lines = lazy_model(false);
        assert_same_matches(&lines, "alpha", false);
        assert_same_matches(&lines, "确保", false);
        // Tab-expanded content keeps materialized display columns.
        assert_same_matches(&lines, "old alpha", false);
        // Hunk rows never match even when the query hits the marker text.
        assert!(collect_diff_find_matches(&lines, "@@ -1,3", false).is_empty());
        // A unified model exposes empty split panes.
        assert!(collect_diff_find_matches(&lines, "alpha", true).is_empty());
    }

    #[test]
    fn source_scan_matches_materialized_rows_in_split_mode() {
        let lines = lazy_model(true);
        assert_same_matches(&lines, "alpha", true);
        assert_same_matches(&lines, "value", true);
        // A split model exposes empty unified content.
        assert!(collect_diff_find_matches(&lines, "alpha", false).is_empty());
    }

    #[test]
    fn row_table_shares_one_empty_model_and_falls_back_on_stale_queries() {
        let lines = lazy_model(false);
        let matches = collect_diff_find_matches(&lines, "alpha", false);
        let mut table = DiffFindRowTable::new();
        table.rebuild("alpha", &matches);

        let hit_row = table.lookup(1, "", "alpha").expect("current query");
        assert_eq!(hit_row.row_count(), 1);
        let empty_first = table.lookup(0, "", "alpha").expect("hunk row is empty");
        let empty_second = table.lookup(3, "", "alpha").expect("no-match row");
        assert_eq!(empty_first.row_count(), 0);
        assert!(std::ptr::eq(
            empty_first.as_any() as *const _ as *const u8,
            empty_second.as_any() as *const _ as *const u8,
        ));

        assert!(table.lookup(1, "", "beta").is_none());
        // Whitespace-only differences hit the same trimmed query.
        assert!(table.lookup(1, "", " alpha ").is_some());
    }
}
