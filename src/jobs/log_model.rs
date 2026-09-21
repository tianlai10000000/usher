//! Send-safe incremental parsing and bounded, lazy Slint line materialization.
use crate::JobLogLineItem;
use slint::{Model, ModelNotify, ModelTracker};
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParsedText {
    pub kind: String,
    pub prefix: String,
    pub text: String,
}
impl ParsedText {
    pub(super) fn display_text(&self) -> Cow<'_, str> {
        if self.prefix.is_empty() {
            Cow::Borrowed(&self.text)
        } else {
            Cow::Owned(format!("{}{}", self.prefix, self.text))
        }
    }

    pub(super) fn display_range(&self, range: std::ops::Range<usize>) -> Cow<'_, str> {
        let boundary = self.prefix.len();
        if range.end <= boundary {
            Cow::Borrowed(&self.prefix[range])
        } else if range.start >= boundary {
            Cow::Borrowed(&self.text[range.start - boundary..range.end - boundary])
        } else {
            Cow::Owned(format!(
                "{}{}",
                &self.prefix[range.start..],
                &self.text[..range.end - boundary]
            ))
        }
    }
}
#[derive(Debug)]
struct Line {
    raw: String,
    parsed: ParsedText,
}

impl Line {
    fn retained_text_bytes(&self) -> usize {
        self.raw
            .capacity()
            .saturating_add(self.parsed.kind.capacity())
            .saturating_add(self.parsed.prefix.capacity())
            .saturating_add(self.parsed.text.capacity())
    }
}

#[derive(Debug, Default)]
struct SnapshotData {
    lines: Vec<Arc<Line>>,
    retained_text_bytes: usize,
}

impl SnapshotData {
    fn new(lines: Vec<Arc<Line>>) -> Self {
        let retained_text_bytes = lines
            .iter()
            .map(|line| line.retained_text_bytes())
            .fold(0usize, usize::saturating_add);
        Self {
            lines,
            retained_text_bytes,
        }
    }
}

impl Deref for SnapshotData {
    type Target = [Arc<Line>];

    fn deref(&self) -> &Self::Target {
        &self.lines
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LogSnapshot(Arc<SnapshotData>);
impl LogSnapshot {
    fn from_lines(lines: Vec<Arc<Line>>) -> Self {
        Self(Arc::new(SnapshotData::new(lines)))
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &ParsedText> {
        self.0.iter().map(|line| &line.parsed)
    }

    pub(super) fn len(&self) -> usize {
        self.0.len()
    }

    pub(super) fn line(&self, index: usize) -> &ParsedText {
        &self.0[index].parsed
    }

    pub(super) fn from_parsed(lines: impl Iterator<Item = ParsedText>) -> Self {
        Self::from_lines(
            lines
                .map(|parsed| {
                    Arc::new(Line {
                        raw: String::new(),
                        parsed,
                    })
                })
                .collect(),
        )
    }

    pub(super) fn retained_text_bytes(&self) -> usize {
        self.0.retained_text_bytes
    }

    #[cfg(test)]
    pub(crate) fn shared_rows(&self, other: &Self) -> usize {
        self.0
            .iter()
            .filter(|line| other.0.iter().any(|candidate| Arc::ptr_eq(line, candidate)))
            .count()
    }
    pub(crate) fn update(&self, raw: &str) -> Self {
        let mut known: HashMap<&str, Arc<Line>> = self
            .0
            .iter()
            .map(|line| (line.raw.as_str(), line.clone()))
            .collect();
        Self::from_lines(
            raw.lines()
                .map(|raw| {
                    known
                        .entry(raw)
                        .or_insert_with(|| {
                            Arc::new(Line {
                                raw: raw.into(),
                                parsed: parse_log_line(raw),
                            })
                        })
                        .clone()
                })
                .collect(),
        )
    }
}

pub(super) struct LogModel {
    pub(super) revision: Cell<i32>,
    snapshot: RefCell<LogSnapshot>,
    cache: RefCell<HashMap<usize, JobLogLineItem>>,
    notify: ModelNotify,
}
impl LogModel {
    pub(super) fn retained_text_bytes(&self) -> usize {
        self.snapshot.borrow().retained_text_bytes()
    }
    pub(super) fn shares_snapshot(&self, snapshot: &LogSnapshot) -> bool {
        Arc::ptr_eq(&self.snapshot.borrow().0, &snapshot.0)
    }
    pub(super) fn new(snapshot: LogSnapshot) -> Self {
        Self {
            revision: Cell::new(0),
            snapshot: RefCell::new(snapshot),
            cache: RefCell::new(HashMap::new()),
            notify: ModelNotify::default(),
        }
    }
    pub(super) fn update(&self, next: LogSnapshot) {
        let old = self.snapshot.borrow().clone();
        if Arc::ptr_eq(&old.0, &next.0) {
            return;
        }
        self.revision.set(self.revision.get().wrapping_add(1));
        let prefix = old
            .0
            .iter()
            .zip(next.0.iter())
            .take_while(|(a, b)| Arc::ptr_eq(a, b))
            .count();
        let suffix = old.0[prefix..]
            .iter()
            .rev()
            .zip(next.0[prefix..].iter().rev())
            .take_while(|(a, b)| Arc::ptr_eq(a, b))
            .count();
        let removed = old.0.len() - prefix - suffix;
        let added = next.0.len() - prefix - suffix;
        self.cache.borrow_mut().clear();
        if removed == added && removed <= 128 {
            self.snapshot.replace(next);
            for row in prefix..prefix + added {
                self.notify.row_changed(row);
            }
        } else {
            if removed != 0 {
                let mut intermediate = old.0.lines.clone();
                intermediate.drain(prefix..prefix + removed);
                self.snapshot.replace(LogSnapshot::from_lines(intermediate));
                self.notify.row_removed(prefix, removed);
            }
            self.snapshot.replace(next);
            if added != 0 {
                self.notify.row_added(prefix, added);
            }
        }
    }
    pub(super) fn measurement_lines(&self) -> LogSnapshot {
        self.snapshot.borrow().clone()
    }
}
impl Model for LogModel {
    type Data = JobLogLineItem;
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn row_count(&self) -> usize {
        self.snapshot.borrow().0.len()
    }
    fn row_data(&self, row: usize) -> Option<Self::Data> {
        if let Some(item) = self.cache.borrow().get(&row) {
            return Some(item.clone());
        }
        let snapshot = self.snapshot.borrow();
        let line = &snapshot.0.get(row)?.parsed;
        let item = JobLogLineItem {
            kind: line.kind.as_str().into(),
            prefix: line.prefix.as_str().into(),
            text: line.text.as_str().into(),
        };
        let mut cache = self.cache.borrow_mut();
        if cache.len() >= 128 {
            cache.clear();
        }
        cache.insert(row, item.clone());
        Some(item)
    }
    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}

fn decode_workflow_command(value: &str) -> String {
    value
        .replace("%0D", "\r")
        .replace("%0A", "\n")
        .replace("%3A", ":")
        .replace("%2C", ",")
        .replace("%25", "%")
}

fn semantic_line(kind: &'static str, prefix: &'static str, text: impl Into<String>) -> ParsedText {
    ParsedText {
        kind: kind.into(),
        prefix: prefix.into(),
        text: text.into(),
    }
}

fn parse_workflow_annotation(line: &str) -> Option<ParsedText> {
    let rest = line.strip_prefix("::")?;
    let separator = rest.find("::")?;
    let (header, message_with_separator) = rest.split_at(separator);
    let command = header.split([' ', ',']).next()?.to_ascii_lowercase();
    let message = decode_workflow_command(&message_with_separator[2..]);
    match command.as_str() {
        "notice" => Some(semantic_line("notice", "Notice: ", message)),
        "warning" => Some(semantic_line("warning", "Warning: ", message)),
        "error" => Some(semantic_line("error", "Error: ", message)),
        "debug" => Some(semantic_line("debug", "Debug: ", message)),
        _ => None,
    }
}

fn split_prefixed_line<'a>(line: &'a str, lowercase: &str, prefix: &str) -> Option<&'a str> {
    lowercase
        .strip_prefix(prefix)
        .map(|suffix| &line[line.len() - suffix.len()..])
}

pub(super) fn parse_log_line(line: &str) -> ParsedText {
    if let Some(annotation) = parse_workflow_annotation(line) {
        return annotation;
    }

    let trimmed = line.trim_start();
    let lowercase = trimmed.to_ascii_lowercase();
    if let Some(message) = split_prefixed_line(trimmed, &lowercase, "warning:") {
        return semantic_line("warning", "Warning:", message);
    }
    if let Some(message) = split_prefixed_line(trimmed, &lowercase, "error:") {
        return semantic_line("error", "Error:", message);
    }
    if let Some(message) = split_prefixed_line(trimmed, &lowercase, "fatal:") {
        return semantic_line("error", "Fatal:", message);
    }
    if let Some(message) = split_prefixed_line(trimmed, &lowercase, "notice:") {
        return semantic_line("notice", "Notice:", message);
    }
    if let Some(message) = split_prefixed_line(trimmed, &lowercase, "debug:") {
        return semantic_line("debug", "Debug:", message);
    }

    let embedded_kind = if lowercase.contains(": warning:") {
        "warning"
    } else if lowercase.contains(": error:") || lowercase.contains(": fatal:") {
        "error"
    } else {
        "plain"
    };
    semantic_line(embedded_kind, "", line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_caches_retained_text_bytes_when_it_is_produced() {
        let first = LogSnapshot::default().update("first\nwarning: retained\npartial");
        let expected_first = first
            .0
            .lines
            .iter()
            .map(|line| line.retained_text_bytes())
            .sum::<usize>();
        assert_eq!(first.retained_text_bytes(), expected_first);

        let next = first.update("warning: retained\npartial completed\nlast");
        let expected_next = next
            .0
            .lines
            .iter()
            .map(|line| line.retained_text_bytes())
            .sum::<usize>();
        assert_eq!(next.retained_text_bytes(), expected_next);
        assert!(next.retained_text_bytes() > 0);
    }

    #[test]
    fn measurement_downcast_shares_snapshot_without_materializing_ui_rows() {
        let first = LogSnapshot::default().update("first\nwarning: retained\npartial");
        let model = std::rc::Rc::new(LogModel::new(first.clone()));
        let erased: slint::ModelRc<JobLogLineItem> = model.clone().into();
        let actual = erased
            .as_any()
            .downcast_ref::<LogModel>()
            .expect("production downcast");
        let measurement = actual.measurement_lines();
        assert!(Arc::ptr_eq(&measurement.0, &first.0));
        assert!(model.cache.borrow().is_empty());

        let next = first.update("warning: retained\npartial completed\nlast");
        model.update(next.clone());
        assert_eq!(measurement.line(0).text, "first");
        assert_eq!(measurement.line(2).text, "partial");
        assert!(model.shares_snapshot(&next));
        assert!(!model.shares_snapshot(&measurement));
        assert!(Arc::ptr_eq(&measurement.0[1], &next.0[0]));
        assert!(model.cache.borrow().is_empty());
    }

    #[test]
    fn display_ranges_match_joined_semantic_text_at_all_unicode_boundaries() {
        for raw in [
            "",
            "plain \u{6d88}\u{606f}",
            "::warning::\u{6d88}\u{606f}%0A%0Anext",
            "error: path",
        ] {
            let parsed = parse_log_line(raw);
            let joined = format!("{}{}", parsed.prefix, parsed.text);
            let boundaries: Vec<_> = joined
                .char_indices()
                .map(|(index, _)| index)
                .chain(std::iter::once(joined.len()))
                .collect();
            for &start in &boundaries {
                for &end in boundaries.iter().filter(|&&end| end >= start) {
                    assert_eq!(parsed.display_range(start..end), joined[start..end]);
                }
            }
        }
    }

    #[test]
    fn append_and_tail_eviction_reuse_parsed_lines_and_bound_ui_cache() {
        let first = LogSnapshot::default().update("first\nwarning: retained\npartial");
        let second = first.update("warning: retained\npartial completed\nlast");
        assert!(Arc::ptr_eq(&first.0[1], &second.0[0]));
        assert_eq!(second.0[0].parsed.kind, "warning");
        let model = LogModel::new(second);
        model.update(LogSnapshot::default().update(&"x\n".repeat(16000)));
        for row in 0..model.row_count() {
            assert!(model.row_data(row).is_some());
        }
        assert!(model.cache.borrow().len() <= 128);
        assert!(model.row_data(16000).is_none());
    }
}
