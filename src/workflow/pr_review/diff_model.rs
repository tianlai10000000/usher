//! Slint adapter for normalized pull-request diff lines.
//!
//! The adapter owns Unified/Split row materialization and a bounded converted
//! row cache. It does not own controller state, request lifecycle, or UI
//! callbacks.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use slint::{Color, Model, ModelRc, ModelTracker, VecModel};

use super::diff_projection::{DiffRowProjection, diff_row_projection};
use super::hunk::{
    hunk_can_expand_all, hunk_can_expand_down, hunk_is_expandable, sanitize_diff_content,
};
use crate::git::{PullRequestDiffFile, PullRequestDiffLine};
use crate::workflow::syntax_highlight::HighlightedLine;
use crate::{WorkflowPrDiffFile, WorkflowPrDiffLine, WorkflowPrDiffToken};

const VISIBLE_LINE_CACHE_CAPACITY: usize = 64;

pub(super) struct LazyPrDiffLineModel {
    file: Arc<PullRequestDiffFile>,
    highlighted: Arc<Vec<HighlightedLine>>,
    rows_projection: Vec<DiffRowProjection>,
    rows: RefCell<LazyLineCache>,
}

impl LazyPrDiffLineModel {
    pub(super) fn new(
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

pub(super) fn to_slint_file(file: &PullRequestDiffFile) -> WorkflowPrDiffFile {
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use slint::Model;

    use super::{LazyPrDiffLineModel, VISIBLE_LINE_CACHE_CAPACITY};
    use crate::git::{PullRequestDiffFile, PullRequestDiffLine};
    use crate::workflow::syntax_highlight::{HighlightedLine, HighlightedToken};

    #[test]
    fn materializes_only_a_bounded_row_window() {
        let file = PullRequestDiffFile {
            path: "src/lib.rs".to_string(),
            old_path: "src/lib.rs".to_string(),
            change_type: "edit_file".to_string(),
            added_lines: 1,
            removed_lines: 0,
            viewed: false,
            lines: (0..128)
                .map(|index| PullRequestDiffLine {
                    old_line: index + 1,
                    new_line: index + 1,
                    kind: "context".to_string(),
                    content: format!("line {index}"),
                })
                .collect(),
        };
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
