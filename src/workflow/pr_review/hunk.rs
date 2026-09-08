//! Pure diff-context expansion and full-source projection.
//!
//! This module has no UI, network, cache, or controller dependencies. Callers
//! fetch immutable base/head sources and pass them in after stale-response
//! validation.

use crate::git::{PullRequestDiffFile, PullRequestDiffLine};
use crate::workflow::syntax_highlight::expand_tabs;

// Match GitHub's signed-in improved `/changes/{commit}` Files changed view.
pub(super) const HUNK_CONTEXT_PAGE_SIZE: usize = 21;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HunkExpansionDirection {
    Up,
    Down,
    All,
}

/// Plain text shown/copied in the review UI. Tabs are expanded to match the
/// highlighted token stream so selection geometry and ⌘/Ctrl+C agree.
pub(super) fn sanitize_diff_content(content: &str) -> String {
    let stripped: String = content
        .chars()
        .filter(|character| !matches!(character, '\r' | '\n'))
        .collect();
    expand_tabs(stripped.as_str(), 4)
}

pub(super) fn hunk_is_expandable(file: &PullRequestDiffFile, hunk_index: usize) -> bool {
    let Some(line) = file.lines.get(hunk_index) else {
        return false;
    };
    if line.kind != crate::git::DiffLineKind::Hunk {
        return false;
    }
    let use_new_side = file.change_type != crate::git::DiffChangeType::Deleted;
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

pub(super) fn hunk_can_expand_down(file: &PullRequestDiffFile, hunk_index: usize) -> bool {
    if !hunk_is_expandable(file, hunk_index) {
        return false;
    }
    let use_new_side = file.change_type != crate::git::DiffChangeType::Deleted;
    file.lines[..hunk_index]
        .iter()
        .rev()
        .find_map(|line| side_line(line, use_new_side))
        .is_some()
}

pub(super) fn hunk_can_expand_all(file: &PullRequestDiffFile, hunk_index: usize) -> bool {
    let Some((previous, next)) = hunk_hidden_bounds(file, hunk_index) else {
        return false;
    };
    previous > 0 && next.saturating_sub(previous).saturating_sub(1) <= HUNK_CONTEXT_PAGE_SIZE as i32
}

fn hunk_hidden_bounds(file: &PullRequestDiffFile, hunk_index: usize) -> Option<(i32, i32)> {
    let line = file.lines.get(hunk_index)?;
    if line.kind != crate::git::DiffLineKind::Hunk {
        return None;
    }
    let use_new_side = file.change_type != crate::git::DiffChangeType::Deleted;
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

pub(super) fn hidden_hunk_for_line(
    file: &PullRequestDiffFile,
    target_line: i32,
    use_new_side: bool,
) -> Option<usize> {
    (target_line > 0).then_some(())?;
    file.lines.iter().enumerate().find_map(|(index, line)| {
        if line.kind != crate::git::DiffLineKind::Hunk {
            return None;
        }
        let (previous, next) = hunk_side_bounds(file, index, use_new_side)?;
        (target_line > previous && target_line < next).then_some(index)
    })
}

fn centered_source_range<'a>(
    source_lines: &'a [&'a str],
    previous: i32,
    next: i32,
    target: i32,
    limit: usize,
) -> Vec<(i32, &'a str)> {
    let hidden_start = previous.saturating_add(1).max(1);
    let hidden_end = next
        .saturating_sub(1)
        .min(source_lines.len().min(i32::MAX as usize) as i32);
    if limit == 0 || target < hidden_start || target > hidden_end {
        return Vec::new();
    }
    let limit = limit.min(i32::MAX as usize) as i32;
    let count = hidden_end.saturating_sub(hidden_start).saturating_add(1);
    let selected = count.min(limit);
    let mut start = target.saturating_sub(selected / 2).max(hidden_start);
    let mut end = start.saturating_add(selected.saturating_sub(1));
    if end > hidden_end {
        end = hidden_end;
        start = end
            .saturating_sub(selected.saturating_sub(1))
            .max(hidden_start);
    }
    (start..=end)
        .map(|number| (number, source_lines[number as usize - 1]))
        .collect()
}

pub(super) fn expand_hunk_context_around_line_from_sources(
    file: &mut PullRequestDiffFile,
    hunk_index: usize,
    target_line: i32,
    use_new_side: bool,
    base_source: &str,
    head_source: &str,
    limit: usize,
) -> bool {
    let Some(marker) = file
        .lines
        .get(hunk_index)
        .filter(|line| line.kind == crate::git::DiffLineKind::Hunk)
        .cloned()
    else {
        return false;
    };
    let old_bounds = hunk_side_bounds(file, hunk_index, false);
    let new_bounds = hunk_side_bounds(file, hunk_index, true);
    let Some((primary_previous, primary_next)) =
        (if use_new_side { new_bounds } else { old_bounds })
    else {
        return false;
    };
    if target_line <= primary_previous || target_line >= primary_next {
        return false;
    }

    let base_lines = base_source.lines().collect::<Vec<_>>();
    let head_lines = head_source.lines().collect::<Vec<_>>();
    let mapped_target = |bounds: Option<(i32, i32)>| {
        bounds.map(|(_, next)| next.saturating_sub(primary_next.saturating_sub(target_line)))
    };
    let old_target = if use_new_side {
        mapped_target(old_bounds)
    } else {
        Some(target_line)
    };
    let new_target = if use_new_side {
        Some(target_line)
    } else {
        mapped_target(new_bounds)
    };
    let old_range = old_bounds
        .zip(old_target)
        .map(|((previous, next), target)| {
            centered_source_range(&base_lines, previous, next, target, limit)
        })
        .unwrap_or_default();
    let new_range = new_bounds
        .zip(new_target)
        .map(|((previous, next), target)| {
            centered_source_range(&head_lines, previous, next, target, limit)
        })
        .unwrap_or_default();
    let rows = align_source_ranges(old_range.as_slice(), new_range.as_slice());
    let primary_number = |line: &PullRequestDiffLine| {
        if use_new_side {
            line.new_line
        } else {
            line.old_line
        }
    };
    let Some(first) = rows.iter().find_map(|line| {
        let number = primary_number(line);
        (number > 0).then_some(number)
    }) else {
        return false;
    };
    let Some(last) = rows.iter().rev().find_map(|line| {
        let number = primary_number(line);
        (number > 0).then_some(number)
    }) else {
        return false;
    };
    if !rows.iter().any(|line| primary_number(line) == target_line) {
        return false;
    }

    let mut replacement = Vec::with_capacity(rows.len().saturating_add(2));
    if first > primary_previous.saturating_add(1) {
        replacement.push(marker.clone());
    }
    replacement.extend(rows);
    if last < primary_next.saturating_sub(1) {
        replacement.push(marker);
    }
    file.lines.splice(hunk_index..=hunk_index, replacement);
    true
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

fn trailing_source_range<'a>(source_lines: &'a [&'a str], previous: i32) -> Vec<(i32, &'a str)> {
    let start = previous.saturating_add(1).max(1) as usize;
    if start > source_lines.len() {
        return Vec::new();
    }
    (start..=source_lines.len())
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
                kind: crate::git::DiffLineKind::Context,
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
                kind: crate::git::DiffLineKind::Deletion,
                content: sanitize_diff_content(old_lines[old_index].1),
            });
            old_index += 1;
        } else {
            rows.push(PullRequestDiffLine {
                old_line: 0,
                new_line: new_lines[new_index].0,
                kind: crate::git::DiffLineKind::Addition,
                content: sanitize_diff_content(new_lines[new_index].1),
            });
            new_index += 1;
        }
    }
    rows
}

fn align_trailing_source_ranges(
    old_lines: &[(i32, &str)],
    new_lines: &[(i32, &str)],
) -> Vec<PullRequestDiffLine> {
    if old_lines.len() == new_lines.len()
        && old_lines
            .iter()
            .zip(new_lines)
            .all(|(old, new)| old.1 == new.1)
    {
        return old_lines
            .iter()
            .zip(new_lines)
            .map(|(old, new)| PullRequestDiffLine {
                old_line: old.0,
                new_line: new.0,
                kind: crate::git::DiffLineKind::Context,
                content: sanitize_diff_content(old.1),
            })
            .collect();
    }
    align_source_ranges(old_lines, new_lines)
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

pub(super) fn expand_hunk_context_from_sources(
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
    let use_new_side = file.change_type != crate::git::DiffChangeType::Deleted;
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
        .is_some_and(|line| line.kind == crate::git::DiffLineKind::Hunk)
        && !hunk_is_expandable(file, current_hunk_index)
    {
        file.lines.remove(current_hunk_index);
    }
    true
}

#[cfg(test)]
pub(super) fn expand_hunk_context(
    file: &mut PullRequestDiffFile,
    hunk_index: usize,
    source: &str,
    limit: usize,
    direction: HunkExpansionDirection,
) -> bool {
    if limit == 0 || !hunk_is_expandable(file, hunk_index) {
        return false;
    }
    let use_new_side = file.change_type != crate::git::DiffChangeType::Deleted;
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
            kind: crate::git::DiffLineKind::Context,
            content: source_lines[number as usize - 1].to_string(),
        });
    }
    file.lines
        .splice(insertion_index..insertion_index, revealed);
    true
}

#[cfg(test)]
pub(super) fn expand_all_hunk_context(file: &mut PullRequestDiffFile, source: &str) -> usize {
    let initial_len = file.lines.len();
    while let Some(index) = file
        .lines
        .iter()
        .enumerate()
        .find_map(|(index, _)| hunk_is_expandable(file, index).then_some(index))
    {
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

pub(super) fn expand_all_hunk_context_from_sources(
    file: &mut PullRequestDiffFile,
    base_source: &str,
    head_source: &str,
) -> usize {
    let initial_len = file.lines.len();
    let mut changed = false;
    while let Some(index) = file
        .lines
        .iter()
        .enumerate()
        .find_map(|(index, _)| hunk_is_expandable(file, index).then_some(index))
    {
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
        changed = true;
        if hunk_hidden_bounds(file, index) == bounds_before {
            break;
        }
    }

    // Unified diffs have hunk markers before omitted ranges, but no sentinel
    // after the final hunk. Complete the expanded document explicitly so
    // "Expand all lines" includes source through EOF.
    let base_lines = base_source.lines().collect::<Vec<_>>();
    let head_lines = head_source.lines().collect::<Vec<_>>();
    let last_old = file
        .lines
        .iter()
        .rev()
        .find_map(|line| side_line(line, false))
        .unwrap_or(0);
    let last_new = file
        .lines
        .iter()
        .rev()
        .find_map(|line| side_line(line, true))
        .unwrap_or(0);
    let trailing_rows = align_trailing_source_ranges(
        trailing_source_range(base_lines.as_slice(), last_old).as_slice(),
        trailing_source_range(head_lines.as_slice(), last_new).as_slice(),
    );
    changed |= !trailing_rows.is_empty();
    file.lines.extend(trailing_rows);

    let growth = file.lines.len().saturating_sub(initial_len);
    if changed { growth.max(1) } else { 0 }
}

#[cfg(test)]
pub(super) fn restore_original_diff(
    file: &mut PullRequestDiffFile,
    original: &PullRequestDiffFile,
) -> bool {
    if file.lines == original.lines {
        return false;
    }
    *file = original.clone();
    true
}

pub(super) fn full_source_file(file: &PullRequestDiffFile, source: &str) -> PullRequestDiffFile {
    let deleted = file.change_type == crate::git::DiffChangeType::Deleted;
    PullRequestDiffFile {
        path: file.path.clone(),
        old_path: file.old_path.clone(),
        change_type: file.change_type,
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
                    kind: crate::git::DiffLineKind::Context,
                    content: content.to_string(),
                }
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HunkExpansionDirection, expand_all_hunk_context_from_sources,
        expand_hunk_context_around_line_from_sources, expand_hunk_context_from_sources,
        full_source_file, hidden_hunk_for_line, sanitize_diff_content,
    };
    use crate::git::{PullRequestDiffFile, PullRequestDiffLine};

    fn line(old_line: i32, new_line: i32, kind: &str, content: &str) -> PullRequestDiffLine {
        PullRequestDiffLine {
            old_line,
            new_line,
            kind: kind.into(),
            content: content.to_string(),
        }
    }

    fn file(lines: Vec<PullRequestDiffLine>) -> PullRequestDiffFile {
        PullRequestDiffFile {
            path: "src/lib.rs".to_string(),
            old_path: "src/lib.rs".to_string(),
            change_type: crate::git::DiffChangeType::Modified,
            added_lines: 0,
            removed_lines: 0,
            viewed: false,
            lines,
        }
    }

    #[test]
    fn sanitizes_control_newlines_and_expands_tabs_for_rendering() {
        assert_eq!(sanitize_diff_content("\talpha\r\n"), "    alpha");
    }

    #[test]
    fn bounded_expansion_reveals_both_sides_without_reordering() {
        let source = "one\ntwo\nthree\nfour\nfive\nsix";
        let mut diff = file(vec![
            line(1, 1, "context", "one"),
            line(0, 0, "hunk", "@@ hidden @@"),
            line(6, 6, "context", "six"),
        ]);

        assert!(expand_hunk_context_from_sources(
            &mut diff,
            1,
            source,
            source,
            2,
            HunkExpansionDirection::Down,
        ));
        assert!(expand_hunk_context_from_sources(
            &mut diff,
            3,
            source,
            source,
            2,
            HunkExpansionDirection::Up,
        ));

        assert_eq!(
            diff.lines
                .iter()
                .map(|line| (line.old_line, line.new_line, line.content.as_str()))
                .collect::<Vec<_>>(),
            [
                (1, 1, "one"),
                (2, 2, "two"),
                (3, 3, "three"),
                (4, 4, "four"),
                (5, 5, "five"),
                (6, 6, "six"),
            ]
        );
    }

    #[test]
    fn source_alignment_preserves_hidden_deletions_and_additions() {
        let mut diff = file(vec![
            line(1, 1, "context", "one"),
            line(0, 0, "hunk", "@@ hidden @@"),
            line(3, 3, "context", "tail"),
        ]);

        assert!(expand_hunk_context_from_sources(
            &mut diff,
            1,
            "one\nold\ntail",
            "one\nnew\ntail",
            usize::MAX,
            HunkExpansionDirection::All,
        ));

        assert_eq!(
            diff.lines
                .iter()
                .map(|line| (
                    line.old_line,
                    line.new_line,
                    line.kind.as_str(),
                    line.content.as_str()
                ))
                .collect::<Vec<_>>(),
            [
                (1, 1, "context", "one"),
                (2, 0, "deletion", "old"),
                (0, 2, "addition", "new"),
                (3, 3, "context", "tail"),
            ]
        );
    }

    #[test]
    fn target_centered_expansion_keeps_hidden_ranges_on_both_sides() {
        let source = (1..=60)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut diff = file(vec![
            line(1, 1, "context", "line 1"),
            line(0, 0, "hunk", "@@ hidden @@"),
            line(60, 60, "context", "line 60"),
        ]);

        let hunk = hidden_hunk_for_line(&diff, 30, true).expect("hidden hunk");
        assert!(expand_hunk_context_around_line_from_sources(
            &mut diff, hunk, 30, true, &source, &source, 21,
        ));

        assert_eq!(
            diff.lines
                .iter()
                .filter(|line| line.kind == crate::git::DiffLineKind::Hunk)
                .count(),
            2
        );
        assert!(diff.lines.iter().any(|line| line.new_line == 30));
        assert!(!diff.lines.iter().any(|line| line.new_line == 2));
        assert!(!diff.lines.iter().any(|line| line.new_line == 59));
    }

    #[test]
    fn target_centered_expansion_uses_the_requested_old_side() {
        let base = "same\nold two\nold three\ntail";
        let head = "same\nnew two\nnew three\ntail";
        let mut diff = file(vec![
            line(1, 1, "context", "same"),
            line(0, 0, "hunk", "@@ hidden @@"),
            line(4, 4, "context", "tail"),
        ]);

        let hunk = hidden_hunk_for_line(&diff, 2, false).expect("hidden old line");
        assert!(expand_hunk_context_around_line_from_sources(
            &mut diff, hunk, 2, false, base, head, 1,
        ));
        assert!(diff.lines.iter().any(|line| line.old_line == 2));
    }

    #[test]
    fn expand_all_includes_source_after_the_last_diff_hunk() {
        let source = "one\ntwo\nthree\nfour\nfive\nsix";
        let mut diff = file(vec![
            line(1, 1, "context", "one"),
            line(0, 0, "hunk", "@@ hidden @@"),
            line(4, 4, "context", "four"),
        ]);

        assert!(expand_all_hunk_context_from_sources(&mut diff, source, source) > 0);
        assert_eq!(
            diff.lines
                .iter()
                .map(|line| line.new_line)
                .collect::<Vec<_>>(),
            [1, 2, 3, 4, 5, 6]
        );
        assert_eq!(diff.lines.last().unwrap().content, "six");
    }

    #[test]
    fn full_source_projection_uses_the_surviving_side_line_numbers() {
        let original = file(Vec::new());
        let added = full_source_file(&original, "alpha\nbeta");
        assert_eq!((added.lines[1].old_line, added.lines[1].new_line), (0, 2));

        let mut deleted_file = original;
        deleted_file.change_type = crate::git::DiffChangeType::Deleted;
        let deleted = full_source_file(&deleted_file, "alpha\nbeta");
        assert_eq!(
            (deleted.lines[1].old_line, deleted.lines[1].new_line),
            (2, 0)
        );
    }
}
