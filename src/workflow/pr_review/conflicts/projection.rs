use super::super::diff_model::LazyPrDiffLineModel;
use crate::workflow::syntax_highlight::{
    HighlightResources, HighlightedLine, highlight_diff_lines,
};
use crate::{
    PrConflictRow,
    git::{
        DiffChangeType, DiffLineKind, PullRequestDiffFile, PullRequestDiffLine,
        conflicts::ConflictFile,
    },
};
use slint::{Model, ModelTracker};
use std::sync::Arc;

pub(super) struct Rows {
    lines: LazyPrDiffLineModel,
    file: ConflictFile,
}
pub(super) struct Prepared {
    source: Arc<PullRequestDiffFile>,
    highlighted: Arc<Vec<HighlightedLine>>,
    file: ConflictFile,
}
impl Prepared {
    pub(super) fn first_conflict(&self) -> i32 {
        self.file
            .blocks
            .first()
            .map_or(0, |block| block.start as i32)
    }
    pub(super) fn max_columns(&self) -> i32 {
        self.highlighted
            .iter()
            .map(|line| line.columns as i32)
            .max()
            .unwrap_or(0)
    }
    pub(super) fn into_model(self) -> Rows {
        Rows {
            lines: LazyPrDiffLineModel::new(self.source, self.highlighted, false),
            file: self.file,
        }
    }
}
pub(super) fn prepare(file: ConflictFile) -> Prepared {
    let source = Arc::new(PullRequestDiffFile {
        path: file.path.clone(),
        old_path: file.target_path.clone(),
        change_type: DiffChangeType::Modified,
        added_lines: 0,
        removed_lines: 0,
        viewed: false,
        lines: file
            .content
            .as_deref()
            .unwrap_or("")
            .lines()
            .enumerate()
            .map(|(i, s)| PullRequestDiffLine {
                old_line: (i + 1) as i32,
                new_line: (i + 1) as i32,
                kind: DiffLineKind::Context,
                content: s.into(),
            })
            .collect(),
    });
    let highlighted = Arc::new(highlight_diff_lines(
        &HighlightResources::load(),
        &source.path,
        &source.lines,
    ));
    Prepared {
        source,
        highlighted,
        file,
    }
}
impl Model for Rows {
    type Data = PrConflictRow;
    fn row_count(&self) -> usize {
        self.lines.row_count()
    }
    fn row_data(&self, index: usize) -> Option<PrConflictRow> {
        let line = self.lines.row_data(index)?;
        let block = self
            .file
            .blocks
            .iter()
            .find(|b| index >= b.start && index <= b.end);
        Some(PrConflictRow {
            number: (index + 1) as i32,
            text: line.content,
            tokens: line.tokens,
            conflict: block.is_some(),
            start: block.is_some_and(|b| index == b.start),
            separator: block.is_some_and(|b| index == b.separator),
            end: block.is_some_and(|b| index == b.end),
        })
    }
    fn model_tracker(&self) -> &dyn ModelTracker {
        self.lines.model_tracker()
    }
}
