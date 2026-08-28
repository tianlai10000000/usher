//! Diff selection semantics, wrapped-row hit routing, and clipboard projection.
//!
//! Selection endpoints are Unicode character indices into the displayed
//! (tab-expanded) line text. Unwrapped shaped-text hit-testing is injected by
//! the controller from `text_geometry`; this module owns document-row routing,
//! soft-wrap behavior, click expansion, and copied text.

use slint::{Model, ModelRc, SharedString};

use crate::workflow::syntax_highlight::character_columns;
use crate::{
    WorkflowPrDiffLine, WorkflowPrDiffSelectionHit, WorkflowPrDiffSelectionRange,
    WorkflowPrDiffTextSlice,
};

/// Resolve a drag position from the grabbed delegate into an exact logical
/// diff row and character index. Unlike a fixed-height approximation,
/// this accounts for every crossed row's own wrap count.
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_diff_selection_hit<F, G>(
    lines: &ModelRc<WorkflowPrDiffLine>,
    start_line: i32,
    pointer_x: f32,
    pointer_y: f32,
    start_height: f32,
    code_width: f32,
    cell_width: f32,
    line_pitch: f32,
    side: &str,
    mut resolve_unwrapped: F,
    mut resolve_wrapped: G,
) -> WorkflowPrDiffSelectionHit
where
    F: FnMut(&WorkflowPrDiffLine, &str, f32) -> i32,
    G: FnMut(&WorkflowPrDiffLine, &str, f32, f32, f32, f32) -> i32,
{
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
    // Match SelectableDiffCode wrap-line-pitch when painting soft-wrapped rows.
    let wrap_pitch = if line_pitch > 0.5 {
        line_pitch
    } else {
        CODE_LINE_HEIGHT
    };
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
        // Soft-wrapped rows use font line pitch; single-line rows stay on the
        // nominal 22px code line height used by unwrapped delegates.
        if visual_lines > 1.0 {
            wrap_pitch * visual_lines
        } else {
            CODE_LINE_HEIGHT
        }
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
                if let Some(candidate) = lines.row_data(index)
                    && is_copyable_diff_line(&candidate)
                {
                    return WorkflowPrDiffSelectionHit {
                        line: index as i32,
                        column: 0,
                    };
                }
            }
            for index in (0..target).rev() {
                if let Some(candidate) = lines.row_data(index)
                    && is_copyable_diff_line(&candidate)
                {
                    return WorkflowPrDiffSelectionHit {
                        line: index as i32,
                        column: side_char_count(&candidate, side),
                    };
                }
            }
        } else {
            for index in (0..target).rev() {
                if let Some(candidate) = lines.row_data(index)
                    && is_copyable_diff_line(&candidate)
                {
                    return WorkflowPrDiffSelectionHit {
                        line: index as i32,
                        column: side_char_count(&candidate, side),
                    };
                }
            }
            for index in (target + 1)..row_count {
                if let Some(candidate) = lines.row_data(index)
                    && is_copyable_diff_line(&candidate)
                {
                    return WorkflowPrDiffSelectionHit {
                        line: index as i32,
                        column: 0,
                    };
                }
            }
        }
        return WorkflowPrDiffSelectionHit {
            line: target as i32,
            column: 0,
        };
    }

    let content = side_content(&line, side);
    let char_count = content.chars().count() as i32;
    let content_columns = side_columns(&line, side).max(0) as f32;
    let wraps = content_columns * safe_cell_width > safe_code_width + 0.5;
    let local_x = pointer_x.max(0.0);
    let column = if wraps {
        resolve_wrapped(&line, side, local_x, local_y, safe_code_width, wrap_pitch)
    } else {
        resolve_unwrapped(&line, side, local_x)
    };

    WorkflowPrDiffSelectionHit {
        line: target as i32,
        column: column.clamp(0, char_count),
    }
}

fn is_copyable_diff_line(line: &WorkflowPrDiffLine) -> bool {
    // Match GitHub: hunk headers and meta separators are never part of a
    // code-column selection or clipboard payload.
    !matches!(line.kind.as_str(), "hunk" | "meta")
}

fn side_char_count(line: &WorkflowPrDiffLine, side: &str) -> i32 {
    side_content(line, side).chars().count() as i32
}

fn side_columns(line: &WorkflowPrDiffLine, side: &str) -> i32 {
    match side {
        "right" => line.right_columns,
        _ => line.left_columns,
    }
}

fn side_content<'a>(line: &'a WorkflowPrDiffLine, side: &str) -> &'a str {
    match side {
        "left" => line.left_content.as_str(),
        "right" => line.right_content.as_str(),
        _ => line.content.as_str(),
    }
}

/// Character index where the soft-wrapped visual line containing `caret` begins.
///
/// Uses a greedy display-column fill with breaks at the last whitespace when
/// possible — close to Skia's word-wrap fallback for `Text.char-wrap`, so
/// selection X on continuation lines lines up with the auto-wrapped Text.
pub(super) fn soft_wrap_line_start(content: &str, caret: i32, max_cells: i32) -> i32 {
    soft_wrap_line_range_at_caret(content, caret, max_cells).0
}

/// Zero-based visual line containing `caret`, using the same word-wrap
/// boundaries as selection and find-highlight geometry.
pub(super) fn soft_wrap_visual_line_index(content: &str, caret: i32, max_cells: i32) -> i32 {
    let chars: Vec<char> = content.chars().collect();
    if chars.is_empty() {
        return 0;
    }
    let caret = caret.clamp(0, chars.len() as i32) as usize;
    let max_cells = max_cells.max(1) as u32;
    let mut line_start = 0usize;
    let mut visual_line = 0i32;
    while line_start < chars.len() {
        let line_end = soft_wrap_next_line_end(&chars, line_start, max_cells);
        if caret < line_end {
            return visual_line;
        }
        line_start = line_end;
        visual_line += 1;
    }
    visual_line.saturating_sub(1)
}

/// Inclusive-start / exclusive-end character range of the soft-wrapped visual
/// line that contains `caret`.
fn soft_wrap_line_range_at_caret(content: &str, caret: i32, max_cells: i32) -> (i32, i32) {
    let chars: Vec<char> = content.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let caret = caret.clamp(0, chars.len() as i32) as usize;
    let max_cells = max_cells.max(1) as u32;

    let mut line_start = 0usize;
    while line_start < chars.len() {
        let line_end = soft_wrap_next_line_end(&chars, line_start, max_cells);
        if caret < line_end {
            return (line_start as i32, line_end as i32);
        }
        line_start = line_end;
    }
    let len = chars.len() as i32;
    (len, len)
}

fn soft_wrap_next_line_end(chars: &[char], line_start: usize, max_cells: u32) -> usize {
    let mut cells = 0u32;
    let mut last_ws_break = line_start;
    let mut saw_break = false;
    let mut index = line_start;
    while index < chars.len() {
        let weight = character_columns(chars[index]).max(1);
        if cells + weight > max_cells && cells > 0 {
            break;
        }
        cells += weight;
        index += 1;
        if chars[index - 1].is_whitespace() {
            last_ws_break = index;
            saw_break = true;
        }
    }
    if index >= chars.len() {
        chars.len()
    } else if saw_break && last_ws_break > line_start {
        last_ws_break
    } else {
        index.max(line_start + 1)
    }
}

/// Resolve a GitHub-style double-click (word) or triple-click (line) range.
/// `mode` is `1` for word and `2` for line; other values return a collapsed
/// caret at `column` so the caller can fall back to drag selection.
/// `column` is a character index into the displayed line text.
pub(super) fn resolve_diff_click_range(
    lines: &ModelRc<WorkflowPrDiffLine>,
    line: i32,
    column: i32,
    side: &str,
    mode: i32,
) -> WorkflowPrDiffSelectionRange {
    let row_count = lines.row_count();
    if row_count == 0 || line < 0 || line as usize >= row_count {
        return WorkflowPrDiffSelectionRange {
            start_line: 0,
            start_column: 0,
            end_line: 0,
            end_column: 0,
        };
    }
    let Some(row) = lines.row_data(line as usize) else {
        return WorkflowPrDiffSelectionRange {
            start_line: line,
            start_column: column.max(0),
            end_line: line,
            end_column: column.max(0),
        };
    };
    if !is_copyable_diff_line(&row) {
        return WorkflowPrDiffSelectionRange {
            start_line: line,
            start_column: 0,
            end_line: line,
            end_column: 0,
        };
    }

    let content = side_content(&row, side);
    let char_count = content.chars().count() as i32;
    let caret = column.clamp(0, char_count);

    match mode {
        1 => {
            let (start_column, end_column) = word_range_character_indices(content, caret as u32);
            WorkflowPrDiffSelectionRange {
                start_line: line,
                start_column: start_column as i32,
                end_line: line,
                end_column: end_column as i32,
            }
        }
        2 => WorkflowPrDiffSelectionRange {
            start_line: line,
            start_column: 0,
            end_line: line,
            // Empty lines still need a one-cell highlight so the gesture is
            // visible and survives end-diff-selection's zero-width clear.
            end_column: char_count.max(1),
        },
        _ => WorkflowPrDiffSelectionRange {
            start_line: line,
            start_column: caret,
            end_line: line,
            end_column: caret,
        },
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Word,
    Whitespace,
    Other,
}

fn char_class(character: char) -> CharClass {
    if character.is_alphanumeric() || character == '_' {
        CharClass::Word
    } else if character.is_whitespace() {
        CharClass::Whitespace
    } else {
        CharClass::Other
    }
}

/// Expand the caret to the contiguous same-class run (code-editor word select).
fn word_range_character_indices(value: &str, caret: u32) -> (u32, u32) {
    if value.is_empty() {
        return (0, 0);
    }

    let chars: Vec<char> = value.chars().collect();
    let total = chars.len() as u32;
    let probe = caret.min(total.saturating_sub(1)) as usize;
    let class = char_class(chars[probe]);
    let mut left = probe;
    while left > 0 && char_class(chars[left - 1]) == class {
        left -= 1;
    }
    let mut right = probe;
    while right + 1 < chars.len() && char_class(chars[right + 1]) == class {
        right += 1;
    }
    (left as u32, (right + 1) as u32)
}

/// Build clipboard text for a GitHub-style code-column selection.
/// `start_col` / `end_col` are Unicode character indices into the displayed
/// (tab-expanded) line text, matching Slint hit-testing and highlights.
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
        out.push_str(&slice_character_range(content.as_str(), from, to));
        emitted += 1;
    }
    if emitted == 0 {
        return None;
    }
    Some(out)
}

/// Prefix + selected slice for preferred-width selection painting.
pub(super) fn diff_selection_slice(
    lines: &ModelRc<WorkflowPrDiffLine>,
    line: i32,
    side: &str,
    start_col: i32,
    end_col: i32,
) -> WorkflowPrDiffTextSlice {
    let empty = WorkflowPrDiffTextSlice {
        prefix: SharedString::new(),
        text: SharedString::new(),
        prefix_bytes: 0,
        text_bytes: 0,
    };
    if line < 0 || line as usize >= lines.row_count() {
        return empty;
    }
    let Some(row) = lines.row_data(line as usize) else {
        return empty;
    };
    if !is_copyable_diff_line(&row) {
        return empty;
    }
    let content = side_content(&row, side);
    let char_count = content.chars().count() as i32;
    let from = start_col.clamp(0, char_count) as u32;
    let to = end_col.clamp(from as i32, char_count) as u32;
    let prefix = slice_character_range(content, 0, from);
    let text = slice_character_range(content, from, to);
    WorkflowPrDiffTextSlice {
        prefix_bytes: prefix.len() as i32,
        text_bytes: text.len() as i32,
        prefix: SharedString::from(prefix),
        text: SharedString::from(text),
    }
}

/// Soft-wrap visual-line start for a caret, used by wrap selection X geometry.
pub(super) fn diff_wrap_line_start(
    lines: &ModelRc<WorkflowPrDiffLine>,
    line: i32,
    side: &str,
    caret: i32,
    max_cells: i32,
) -> i32 {
    if line < 0 || line as usize >= lines.row_count() {
        return 0;
    }
    let Some(row) = lines.row_data(line as usize) else {
        return 0;
    };
    if !is_copyable_diff_line(&row) {
        return 0;
    }
    soft_wrap_line_start(side_content(&row, side), caret, max_cells)
}

fn slice_character_range(value: &str, from: u32, to: u32) -> String {
    if from >= to {
        return String::new();
    }
    value
        .chars()
        .skip(from as usize)
        .take(to.saturating_sub(from) as usize)
        .collect()
}

/// Legacy display-column slicer kept for find/wrap helpers and unit coverage.
#[cfg(test)]
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

    use super::{
        diff_selection_text, resolve_diff_click_range, resolve_diff_selection_hit,
        slice_character_range, slice_display_columns, soft_wrap_line_start,
        word_range_character_indices,
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

    fn wrapped_test_hit(
        line: &WorkflowPrDiffLine,
        side: &str,
        pointer_x: f32,
        pointer_y: f32,
        width: f32,
        pitch: f32,
    ) -> i32 {
        let content = match side {
            "left" => line.left_content.as_str(),
            "right" => line.right_content.as_str(),
            _ => line.content.as_str(),
        };
        let columns = (width / 10.0).floor().max(1.0) as i32;
        let visual_line = (pointer_y / pitch).floor().max(0.0) as i32;
        let local_column = ((pointer_x + 5.0) / 10.0).floor().max(0.0) as i32;
        (visual_line * columns + local_column).clamp(0, content.chars().count() as i32)
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
    fn copy_matches_cjk_character_selection_not_display_columns() {
        // Regression: selecting "安全" used to copy "t-lo" because hit-testing
        // produced monospaced cell indices that slice_display_columns treated
        // as East-Asian display columns.
        let content = "保持 Usher 的桌面响应速度、清晰模块边界、event-loop 安全 UI 更新、项目级状态隔离，以及统一的横切基础设施。";
        let lines = ModelRc::new(VecModel::from(vec![line(content, content, content)]));
        let start = content.find("安全").expect("fixture") as u32;
        let chars_before = content[..content.find("安全").unwrap()].chars().count() as i32;
        let _ = start;
        assert_eq!(
            diff_selection_text(&lines, 0, chars_before, 0, chars_before + 2, "").as_deref(),
            Some("安全")
        );
        assert_ne!(
            slice_display_columns(content, chars_before as u32, (chars_before + 2) as u32),
            "安全"
        );
    }

    #[test]
    fn copy_matches_tab_expanded_display_text() {
        let expanded = "    let answer = 1;";
        let lines = ModelRc::new(VecModel::from(vec![line(expanded, expanded, expanded)]));

        assert_eq!(
            diff_selection_text(&lines, 0, 0, 0, 4, "").as_deref(),
            Some("    ")
        );
        assert_eq!(
            diff_selection_text(&lines, 0, 2, 0, 7, "").as_deref(),
            Some("  let")
        );
        assert_eq!(
            diff_selection_text(&lines, 0, 4, 0, 7, "").as_deref(),
            Some("let")
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
        assert_eq!(slice_character_range("安全UI", 0, 2), "安全");
    }

    #[test]
    fn soft_wrap_breaks_at_whitespace_before_overflow() {
        let text = "This file applies to the entire repository. It is intentionally bilingual";
        // Narrow width forces a wrap around the first sentence.
        let start = soft_wrap_line_start(text, 50, 40);
        assert!(start > 0);
        assert!(start <= 50);
        assert_eq!(soft_wrap_line_start(text, 0, 40), 0);
        assert_eq!(soft_wrap_line_start(text, start, 40), start);
    }

    #[test]
    fn hit_tracks_columns_across_wrapped_rows() {
        let wrapped = "01234567890123456789";
        let lines = ModelRc::new(VecModel::from(vec![
            line(wrapped, wrapped, wrapped),
            line(wrapped, wrapped, wrapped),
        ]));

        let forward = resolve_diff_selection_hit(
            &lines,
            0,
            40.0,
            74.0,
            44.0,
            100.0,
            10.0,
            22.0,
            "",
            |_, _, _| unreachable!("wrapped rows use soft-wrap geometry"),
            wrapped_test_hit,
        );
        assert_eq!((forward.line, forward.column), (1, 14));

        let reverse = resolve_diff_selection_hit(
            &lines,
            1,
            40.0,
            -10.0,
            44.0,
            100.0,
            10.0,
            22.0,
            "",
            |_, _, _| unreachable!("wrapped rows use soft-wrap geometry"),
            wrapped_test_hit,
        );
        assert_eq!((reverse.line, reverse.column), (0, 14));

        let rounded = resolve_diff_selection_hit(
            &lines,
            0,
            45.0,
            10.0,
            44.0,
            100.0,
            10.0,
            22.0,
            "",
            |_, _, _| unreachable!("wrapped rows use soft-wrap geometry"),
            wrapped_test_hit,
        );
        assert_eq!((rounded.line, rounded.column), (0, 5));
    }

    #[test]
    fn unwrapped_cross_row_hit_is_delegated_to_shaped_geometry() {
        let lines = ModelRc::new(VecModel::from(vec![
            line("alpha", "alpha", "alpha"),
            line("标点/event-loop", "标点/event-loop", "标点/event-loop"),
        ]));
        let hit = resolve_diff_selection_hit(
            &lines,
            0,
            37.0,
            30.0,
            22.0,
            400.0,
            10.0,
            22.0,
            "",
            |line, side, pointer_x| {
                assert_eq!(line.content.as_str(), "标点/event-loop");
                assert_eq!(side, "");
                assert!((pointer_x - 37.0).abs() < f32::EPSILON);
                4
            },
            |_, _, _, _, _, _| unreachable!("unwrapped rows use shaped-run geometry"),
        );
        assert_eq!((hit.line, hit.column), (1, 4));
    }

    #[test]
    fn word_range_expands_same_character_class() {
        assert_eq!(word_range_character_indices("foo_bar.baz", 2), (0, 7));
        assert_eq!(word_range_character_indices("foo_bar.baz", 7), (7, 8));
        assert_eq!(word_range_character_indices("foo_bar.baz", 9), (8, 11));
        assert_eq!(word_range_character_indices("a  b", 1), (1, 3));
        assert_eq!(word_range_character_indices("==>", 1), (0, 3));
        assert_eq!(
            word_range_character_indices("event-loop 安全 UI", 12),
            (11, 13)
        );
    }

    #[test]
    fn click_range_selects_word_or_line() {
        let lines = ModelRc::new(VecModel::from(vec![line(
            "alpha beta",
            "L-alpha beta",
            "R-alpha beta",
        )]));

        let word = resolve_diff_click_range(&lines, 0, 2, "", 1);
        assert_eq!(
            (
                word.start_line,
                word.start_column,
                word.end_line,
                word.end_column
            ),
            (0, 0, 0, 5)
        );
        assert_eq!(
            diff_selection_text(
                &lines,
                word.start_line,
                word.start_column,
                word.end_line,
                word.end_column,
                ""
            )
            .as_deref(),
            Some("alpha")
        );

        let full = resolve_diff_click_range(&lines, 0, 2, "left", 2);
        assert_eq!(
            (
                full.start_line,
                full.start_column,
                full.end_line,
                full.end_column
            ),
            (0, 0, 0, 12)
        );
        assert_eq!(
            diff_selection_text(
                &lines,
                full.start_line,
                full.start_column,
                full.end_line,
                full.end_column,
                "left"
            )
            .as_deref(),
            Some("L-alpha beta")
        );
    }
}
