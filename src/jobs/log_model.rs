//! Send-safe incremental parsing and bounded, lazy Slint line materialization.
use crate::JobLogLineItem;
use slint::{Model, ModelNotify, ModelTracker};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParsedText {
    pub kind: String,
    pub prefix: String,
    pub text: String,
}
#[derive(Debug)]
struct Line {
    raw: String,
    parsed: ParsedText,
}
#[derive(Debug, Clone, Default)]
pub(crate) struct LogSnapshot(Arc<Vec<Arc<Line>>>);
impl LogSnapshot {
    pub(crate) fn update(&self, raw: &str) -> Self {
        let mut known: HashMap<&str, Arc<Line>> = self
            .0
            .iter()
            .map(|line| (line.raw.as_str(), line.clone()))
            .collect();
        Self(Arc::new(
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
        ))
    }
}

pub(super) struct LogModel {
    snapshot: RefCell<LogSnapshot>,
    cache: RefCell<HashMap<usize, JobLogLineItem>>,
    notify: ModelNotify,
}
impl LogModel {
    pub(super) fn new(snapshot: LogSnapshot) -> Self {
        Self {
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
                let mut intermediate = old.0.as_ref().clone();
                intermediate.drain(prefix..prefix + removed);
                self.snapshot.replace(LogSnapshot(Arc::new(intermediate)));
                self.notify.row_removed(prefix, removed);
            }
            self.snapshot.replace(next);
            if added != 0 {
                self.notify.row_added(prefix, added);
            }
        }
    }
}
impl Model for LogModel {
    type Data = JobLogLineItem;
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
