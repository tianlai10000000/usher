//! Diff selection geometry and clipboard projection.
//!
//! The controller supplies the current display model and pointer coordinates;
//! this module deterministically resolves logical endpoints and copied text.

use slint::{Model, ModelRc};

use crate::workflow::syntax_highlight::character_columns;
use crate::{WorkflowPrDiffLine, WorkflowPrDiffSelectionHit};

/// Resolve a drag position from the grabbed delegate into an exact logical
/// diff row and wrapped display column. Unlike a fixed-height approximation,
/// this accounts for every crossed row's own wrap count.
pub(super) fn resolve_diff_selection_hit(
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
pub(super) fn diff_selection_text(
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

pub(super) fn slice_display_columns(value: &str, from: u32, to: u32) -> &str {
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

#[cfg(test)]
mod tests {
    use slint::{ModelRc, VecModel};

    use super::{diff_selection_text, resolve_diff_selection_hit, slice_display_columns};
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
    fn copies_unified_and_split_diff_selections_without_gutters() {
        let lines = ModelRc::new(VecModel::from(vec![
            line("alpha", "L-alpha", "R-alpha"),
            line("bravo", "L-bravo", "R-bravo"),
            line("charlie", "L-charlie", "R-charlie"),
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
    fn copy_skips_hunk_and_meta_rows() {
        let mut hunk = line("@@ -1,2 +1,2 @@", "", "");
        hunk.kind = "hunk".into();
        let mut meta = line("Binary files differ", "", "");
        meta.kind = "meta".into();
        let lines = ModelRc::new(VecModel::from(vec![
            line("alpha", "L-alpha", "R-alpha"),
            hunk,
            meta,
            line("bravo", "L-bravo", "R-bravo"),
        ]));

        assert_eq!(
            diff_selection_text(&lines, 0, 0, 3, 5, "").as_deref(),
            Some("alpha\nbravo")
        );
        assert_eq!(diff_selection_text(&lines, 1, 0, 2, 8, ""), None);
    }

    #[test]
    fn display_columns_preserve_tabs_and_wide_glyphs() {
        assert_eq!(slice_display_columns("\t中x", 0, 4), "\t");
        assert_eq!(slice_display_columns("\t中x", 4, 6), "中");
        assert_eq!(slice_display_columns("\t中x", 6, 7), "x");
        assert_eq!(slice_display_columns("\t中x", 3, 5), "\t中");
    }

    #[test]
    fn hit_tracks_columns_across_wrapped_rows() {
        let wrapped = "01234567890123456789";
        let lines = ModelRc::new(VecModel::from(vec![
            line(wrapped, wrapped, wrapped),
            line(wrapped, wrapped, wrapped),
        ]));

        let forward = resolve_diff_selection_hit(&lines, 0, 40.0, 74.0, 44.0, 100.0, 10.0, "");
        assert_eq!((forward.line, forward.column), (1, 14));

        let reverse = resolve_diff_selection_hit(&lines, 1, 40.0, -10.0, 44.0, 100.0, 10.0, "");
        assert_eq!((reverse.line, reverse.column), (0, 14));

        let rounded = resolve_diff_selection_hit(&lines, 0, 45.0, 10.0, 44.0, 100.0, 10.0, "");
        assert_eq!((rounded.line, rounded.column), (0, 5));
    }
}
