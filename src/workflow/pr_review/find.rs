//! In-diff find matches for the Files changed toolbar search bar.
//!
//! GitHub's PR Files changed surface has no first-party find chrome (browser
//! find-in-page is used there). Usher docks a compact CodeMirror/editor-style
//! find bar in the selected-file toolbar and resolves matches against the
//! current projected line model.

use slint::{Model, ModelRc, SharedString, VecModel};
use usher_search::{MatchCase, find_non_overlapping};

use crate::workflow::syntax_highlight::character_columns;
use crate::{WorkflowPrDiffFindMatch, WorkflowPrDiffLine};

use super::selection::soft_wrap_visual_line_index;

/// Case-insensitive substring matches over copyable code columns.
/// Split mode searches both panes and tags each hit with `left` / `right`.
pub(super) fn collect_diff_find_matches(
    lines: &ModelRc<WorkflowPrDiffLine>,
    query: &str,
    split: bool,
) -> ModelRc<WorkflowPrDiffFindMatch> {
    let needle = query.trim();
    if needle.is_empty() {
        return ModelRc::new(VecModel::from(Vec::<WorkflowPrDiffFindMatch>::new()));
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

    ModelRc::new(VecModel::from(matches))
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
    use slint::{Model, ModelRc, VecModel};

    use super::{
        collect_diff_find_matches, collect_diff_find_row_matches, diff_find_intra_item_offset,
    };
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
        assert_eq!(matches.row_count(), 2);
        let first = matches.row_data(0).expect("first match");
        assert_eq!(first.line, 0);
        assert_eq!(first.start_column, 0);
        assert_eq!(first.end_column, 3);
        assert_eq!(first.start_char, 0);
        assert_eq!(first.end_char, 3);
        assert_eq!(first.prefix.as_str(), "");
        assert_eq!(first.text.as_str(), "Alp");
        assert_eq!(first.side.as_str(), "");
        assert_eq!(matches.row_data(1).map(|item| item.line), Some(2));
    }

    #[test]
    fn cjk_match_keeps_prefix_and_display_columns() {
        let content = "确保开发者和 coding agent";
        let lines = ModelRc::new(VecModel::from(vec![line(content, content, content)]));
        let matches = collect_diff_find_matches(&lines, "保开发者", false);
        assert_eq!(matches.row_count(), 1);
        let hit = matches.row_data(0).expect("cjk match");
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
        assert_eq!(matches.row_count(), 2);
        assert_eq!(
            matches
                .row_data(0)
                .map(|item| item.side.to_string())
                .as_deref(),
            Some("left")
        );
        assert_eq!(
            matches
                .row_data(1)
                .map(|item| item.side.to_string())
                .as_deref(),
            Some("right")
        );
    }

    #[test]
    fn same_line_occurrences_are_distinct_non_overlapping_matches() {
        let lines = ModelRc::new(VecModel::from(vec![line(
            "find find find",
            "find find find",
            "find find find",
        )]));
        let matches = collect_diff_find_matches(&lines, "find", false);
        assert_eq!(matches.row_count(), 3);
        assert_eq!(matches.row_data(0).map(|hit| hit.start_char), Some(0));
        assert_eq!(matches.row_data(1).map(|hit| hit.start_char), Some(5));
        assert_eq!(matches.row_data(2).map(|hit| hit.start_char), Some(10));
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
}
