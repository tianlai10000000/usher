//! Pure row projection for unified and split diff presentation.
//!
//! This module maps immutable provider diff lines onto display rows. It owns no
//! Slint model, controller state, cache, or network lifecycle.

use crate::git::{PullRequestDiffFile, PullRequestDiffLine};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiffRowProjection {
    Unified(usize),
    Split {
        left: Option<usize>,
        right: Option<usize>,
        source_index: usize,
        spanning: bool,
    },
}

pub(super) fn diff_row_projection(
    lines: &[PullRequestDiffLine],
    split: bool,
) -> Vec<DiffRowProjection> {
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

pub(super) fn hunk_scroll_anchor_source_index(
    original: &PullRequestDiffFile,
    updated: &PullRequestDiffFile,
    original_hunk_index: usize,
) -> i32 {
    let Some(original_hunk) = original.lines.get(original_hunk_index) else {
        return -1;
    };
    if original_hunk.kind != "hunk" {
        return -1;
    }
    if original_hunk_index >= updated.lines.len() {
        return -1;
    }

    // Keep the clicked model position stable rather than following the hunk
    // marker itself. "Down" inserts rows before the marker, so the first
    // revealed row takes this position. "Up" inserts after it, so the marker
    // stays here. If the separator disappears, the first revealed row shifts
    // into the same slot. This also keeps the anchor inside the virtual
    // ListView's visible delegate window during repeated expansions.
    original_hunk_index as i32
}

#[cfg(test)]
mod tests {
    use super::{DiffRowProjection, diff_row_projection};
    use crate::git::PullRequestDiffLine;

    fn line(old_line: i32, new_line: i32, kind: &str, content: &str) -> PullRequestDiffLine {
        PullRequestDiffLine {
            old_line,
            new_line,
            kind: kind.to_string(),
            content: content.to_string(),
        }
    }

    #[test]
    fn split_projection_pairs_change_blocks_and_duplicates_context() {
        let lines = vec![
            line(0, 0, "hunk", "@@ -1,3 +1,4 @@"),
            line(1, 1, "context", "same"),
            line(2, 0, "deletion", "old one"),
            line(3, 0, "deletion", "old two"),
            line(0, 2, "addition", "new one"),
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
    fn split_projection_keeps_pure_additions_on_the_right() {
        let lines = vec![
            line(0, 0, "hunk", "@@ -0,0 +1,2 @@"),
            line(0, 1, "addition", "first"),
            line(0, 2, "addition", "second"),
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
}
