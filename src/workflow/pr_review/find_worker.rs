//! Cancellable raw-source scanning with compact hits and lazy UI projection.
use super::diff_model::LazyPrDiffLineModel;
use super::diff_projection::DiffRowProjection;
use super::hunk::sanitize_diff_content;
use crate::git::PullRequestDiffFile;
use crate::workflow::syntax_highlight::character_columns;
use crate::{WorkflowPrDiffFindMatch, WorkflowPrDiffLine};
use slint::{Model, ModelNotify, ModelRc, ModelTracker};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use usher_search::{MatchCase, TextMatch, find_non_overlapping};

#[derive(Clone)]
pub(super) struct FindSource {
    pub file: Arc<PullRequestDiffFile>,
    pub rows: Arc<Vec<DiffRowProjection>>,
}
impl FindSource {
    pub fn capture(lines: &ModelRc<WorkflowPrDiffLine>) -> Option<Self> {
        let model = lines.as_any().downcast_ref::<LazyPrDiffLineModel>()?;
        Some(Self {
            file: model.shared_file(),
            rows: model.shared_projection(),
        })
    }
}
struct Hit {
    row: i32,
    side: u8,
    source: usize,
    range: TextMatch,
    start_column: u32,
    end_column: u32,
}
pub(super) struct FindResults {
    pub query: String,
    source: FindSource,
    hits: Vec<Hit>,
    rows: HashMap<(i32, u8), Range<usize>>,
}
pub(super) fn side_key(side: &str) -> u8 {
    match side {
        "left" => 1,
        "right" => 2,
        _ => 0,
    }
}
fn source_text(raw: &str) -> Cow<'_, str> {
    if raw.contains(['\t', '\r', '\n']) {
        Cow::Owned(sanitize_diff_content(raw))
    } else {
        Cow::Borrowed(raw)
    }
}
pub(super) fn scan(
    source: FindSource,
    query: String,
    split: bool,
    cancelled: impl Fn() -> bool,
) -> Option<FindResults> {
    let mut result = FindResults {
        query: query.trim().into(),
        source,
        hits: Vec::new(),
        rows: HashMap::new(),
    };
    if result.query.is_empty() {
        return Some(result);
    }
    for row in 0..result.source.rows.len() {
        if cancelled() {
            return None;
        }
        let sources = match result.source.rows[row] {
            DiffRowProjection::Unified(index) if !split => [(Some(index), 0), (None, 0)],
            DiffRowProjection::Split {
                left,
                right,
                spanning: false,
                ..
            } if split => [(left, 1), (right, 2)],
            _ => continue,
        };
        for (index, side) in sources {
            let Some(index) = index else {
                continue;
            };
            let line = &result.source.file.lines[index];
            if matches!(line.kind.as_str(), "hunk" | "meta") {
                continue;
            }
            let text = source_text(&line.content);
            let found = find_non_overlapping(&text, &result.query, MatchCase::Insensitive);
            let begin = result.hits.len();
            let mut chars = text.char_indices().peekable();
            let mut column = 0;
            for (ordinal, range) in found.into_iter().enumerate() {
                if ordinal % 256 == 0 && cancelled() {
                    return None;
                }
                while chars
                    .peek()
                    .is_some_and(|(byte, _)| *byte < range.start_byte)
                {
                    column += character_columns(chars.next().unwrap().1);
                }
                let start_column = column;
                while chars.peek().is_some_and(|(byte, _)| *byte < range.end_byte) {
                    column += character_columns(chars.next().unwrap().1);
                }
                result.hits.push(Hit {
                    row: row as i32,
                    side,
                    source: index,
                    range,
                    start_column,
                    end_column: column,
                });
            }
            if result.hits.len() > begin {
                result
                    .rows
                    .insert((row as i32, side), begin..result.hits.len());
            }
        }
    }
    (!cancelled()).then_some(result)
}
impl FindResults {
    pub fn len(&self) -> usize {
        self.hits.len()
    }
    fn row_data(&self, index: usize) -> Option<WorkflowPrDiffFindMatch> {
        let hit = self.hits.get(index)?;
        let text = source_text(&self.source.file.lines[hit.source].content);
        Some(WorkflowPrDiffFindMatch {
            line: hit.row,
            side: match hit.side {
                1 => "left",
                2 => "right",
                _ => "",
            }
            .into(),
            start_column: hit.start_column as i32,
            end_column: hit.end_column as i32,
            start_char: hit.range.start_char as i32,
            end_char: hit.range.end_char as i32,
            prefix: text[..hit.range.start_byte].into(),
            text: text[hit.range.start_byte..hit.range.end_byte].into(),
        })
    }
    pub fn row_model(self: &Arc<Self>, row: i32, side: &str) -> ModelRc<WorkflowPrDiffFindMatch> {
        let range = self
            .rows
            .get(&(row, side_key(side)))
            .cloned()
            .unwrap_or(0..0);
        ModelRc::new(FindSlice {
            result: self.clone(),
            range,
        })
    }
}
struct FindSlice {
    result: Arc<FindResults>,
    range: Range<usize>,
}
impl Model for FindSlice {
    type Data = WorkflowPrDiffFindMatch;
    fn row_count(&self) -> usize {
        self.range.len()
    }
    fn row_data(&self, row: usize) -> Option<Self::Data> {
        (row < self.range.len())
            .then(|| self.result.row_data(self.range.start + row))
            .flatten()
    }
    fn model_tracker(&self) -> &dyn ModelTracker {
        &()
    }
}
pub(super) struct FindModel {
    pub result: RefCell<Option<Arc<FindResults>>>,
    cache: RefCell<HashMap<usize, WorkflowPrDiffFindMatch>>,
    notify: ModelNotify,
}
impl FindModel {
    pub fn new() -> Self {
        Self {
            result: RefCell::new(None),
            cache: RefCell::new(HashMap::new()),
            notify: ModelNotify::default(),
        }
    }
    pub fn replace(&self, result: Option<Arc<FindResults>>) {
        *self.result.borrow_mut() = result;
        self.cache.borrow_mut().clear();
        self.notify.reset();
    }
}
impl Model for FindModel {
    type Data = WorkflowPrDiffFindMatch;
    fn row_count(&self) -> usize {
        self.result
            .borrow()
            .as_ref()
            .map_or(0, |result| result.len())
    }
    fn row_data(&self, row: usize) -> Option<Self::Data> {
        if let Some(hit) = self.cache.borrow().get(&row) {
            return Some(hit.clone());
        }
        let hit = self.result.borrow().as_ref()?.row_data(row)?;
        let mut cache = self.cache.borrow_mut();
        if cache.len() >= 128 {
            cache.clear();
        }
        cache.insert(row, hit.clone());
        Some(hit)
    }
    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::PullRequestDiffLine;
    use crate::workflow::syntax_highlight::HighlightedLine;
    use std::rc::Rc;
    fn model(split: bool) -> ModelRc<WorkflowPrDiffLine> {
        let lines = vec![
            PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".into(),
                content: "@@ alpha".into(),
            },
            PullRequestDiffLine {
                old_line: 1,
                new_line: 1,
                kind: "context".into(),
                content: "İSTANBUL alpha 中文\talpha".into(),
            },
            PullRequestDiffLine {
                old_line: 2,
                new_line: 0,
                kind: "deletion".into(),
                content: "alpha alpha".into(),
            },
            PullRequestDiffLine {
                old_line: 0,
                new_line: 2,
                kind: "addition".into(),
                content: "中文 Alpha".into(),
            },
        ];
        let highlighted = Arc::new(
            lines
                .iter()
                .map(|_| HighlightedLine {
                    columns: 60,
                    tokens: vec![],
                })
                .collect(),
        );
        ModelRc::from(Rc::new(LazyPrDiffLineModel::new(
            Arc::new(PullRequestDiffFile {
                path: "fixture".into(),
                old_path: "fixture".into(),
                change_type: "edit_file".into(),
                added_lines: 1,
                removed_lines: 1,
                viewed: false,
                lines,
            }),
            highlighted,
            split,
        )))
    }
    #[test]
    fn compact_scan_matches_existing_unicode_and_split_contract() {
        for split in [false, true] {
            let lines = model(split);
            for query in ["alpha", "中文", "i̇stanbul", "@@", "none", "a", " alpha "] {
                let old = super::super::find::collect_diff_find_matches(&lines, query, split);
                let result = Arc::new(
                    scan(
                        FindSource::capture(&lines).unwrap(),
                        query.into(),
                        split,
                        || false,
                    )
                    .unwrap(),
                );
                assert_eq!(old.len(), result.len());
                for (index, hit) in old.iter().enumerate() {
                    assert_eq!(hit, &result.row_data(index).unwrap());
                    let row = result.row_model(hit.line, hit.side.as_str());
                    assert!(row.iter().any(|row| &row == hit));
                }
            }
        }
    }
    #[test]
    fn cancellation_discards_partial_results_and_model_cache_is_bounded() {
        let lines = model(false);
        assert!(
            scan(
                FindSource::capture(&lines).unwrap(),
                "alpha".into(),
                false,
                || true
            )
            .is_none()
        );
        let mut source = FindSource::capture(&lines).unwrap();
        let mut file = source.file.as_ref().clone();
        file.lines[1].content = "a ".repeat(10000);
        source.file = Arc::new(file);
        let result = Arc::new(scan(source, "a".into(), false, || false).unwrap());
        let model = FindModel::new();
        model.replace(Some(result));
        for index in (0..model.row_count()).step_by(17) {
            assert!(model.row_data(index).is_some());
            assert!(model.cache.borrow().len() <= 128);
        }
        model.replace(None);
        assert_eq!(model.row_count(), 0);
        assert!(model.cache.borrow().is_empty());
    }
}
