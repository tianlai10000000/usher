//! In-diff find matches for the Files changed toolbar search bar.
//!
//! GitHub's PR Files changed surface has no first-party find chrome (browser
//! find-in-page is used there). Usher docks a compact CodeMirror/editor-style
//! find bar in the selected-file toolbar and resolves matches against the
//! current projected line model.

use slint::{Model, ModelRc, SharedString, VecModel};

use crate::workflow::syntax_highlight::character_columns;
use crate::{WorkflowPrDiffFindMatch, WorkflowPrDiffLine};

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
    let needle_lower = needle.to_lowercase();
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
                needle_lower.as_str(),
                "left",
            );
            push_content_matches(
                &mut matches,
                index as i32,
                line.right_content.as_str(),
                needle_lower.as_str(),
                "right",
            );
        } else {
            push_content_matches(
                &mut matches,
                index as i32,
                line.content.as_str(),
                needle_lower.as_str(),
                "",
            );
        }
    }

    ModelRc::new(VecModel::from(matches))
}

/// Content Y of the top of `target_line`, matching selection hit-test heights.
pub(super) fn estimate_diff_line_offset_y(
    lines: &ModelRc<WorkflowPrDiffLine>,
    target_line: i32,
    code_width: f32,
    cell_width: f32,
    side: &str,
) -> f32 {
    const CODE_LINE_HEIGHT: f32 = 22.0;
    const HUNK_LINE_HEIGHT: f32 = 28.0;
    const TWO_WAY_HUNK_HEIGHT: f32 = 52.0;

    let row_count = lines.row_count();
    if row_count == 0 || target_line <= 0 {
        return 0.0;
    }
    let target = (target_line as usize).min(row_count.saturating_sub(1));
    let safe_code_width = code_width.max(1.0);
    let safe_cell_width = cell_width.max(0.1);

    let mut offset = 0.0_f32;
    for index in 0..target {
        let Some(line) = lines.row_data(index) else {
            offset += CODE_LINE_HEIGHT;
            continue;
        };
        if line.kind.as_str() == "hunk" {
            offset += if line.expand_up && line.expand_down {
                TWO_WAY_HUNK_HEIGHT
            } else {
                HUNK_LINE_HEIGHT
            };
            continue;
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
        offset += CODE_LINE_HEIGHT * visual_lines;
    }
    offset
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
    let haystack_lower = content.to_lowercase();
    let mut search_from = 0_usize;
    while let Some(rel) = haystack_lower[search_from..].find(needle_lower) {
        let start_byte = search_from + rel;
        let end_byte = start_byte + needle_lower.len();
        // Needle length is measured on the lowercased haystack; for UTF-8 this
        // matches the original slice length because lowercasing BMP letters
        // keeps byte length for the scripts we care about in source diffs.
        if !content.is_char_boundary(start_byte) || !content.is_char_boundary(end_byte) {
            search_from = start_byte.saturating_add(1);
            continue;
        }
        let prefix = &content[..start_byte];
        let text = &content[start_byte..end_byte];
        let start_column = display_column_at_byte(content, start_byte);
        let end_column = display_column_at_byte(content, end_byte);
        let start_char = prefix.chars().count();
        let end_char = start_char + text.chars().count();
        out.push(WorkflowPrDiffFindMatch {
            line,
            start_column: start_column as i32,
            end_column: end_column as i32,
            start_char: start_char as i32,
            end_char: end_char as i32,
            side: side.into(),
            prefix: SharedString::from(prefix),
            text: SharedString::from(text),
        });
        search_from = if end_byte > start_byte {
            end_byte
        } else {
            start_byte.saturating_add(1)
        };
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

    use super::{collect_diff_find_matches, estimate_diff_line_offset_y};
    use crate::{WorkflowPrDiffFindMatch, WorkflowPrDiffLine, WorkflowPrDiffToken};

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
    fn line_offset_accounts_for_wrapped_rows() {
        let wrapped = "01234567890123456789";
        let lines = ModelRc::new(VecModel::from(vec![
            line(wrapped, wrapped, wrapped),
            line("short", "short", "short"),
        ]));
        // 20 cols / 10 px cells in 100 px width => 2 visual rows => 44 px.
        let offset = estimate_diff_line_offset_y(&lines, 1, 100.0, 10.0, "");
        assert_eq!(offset, 44.0);
    }
}
