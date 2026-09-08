use super::{DiffChangeType, DiffLineKind, PullRequestDiffFile, PullRequestDiffLine};

pub(super) fn parse_unified_diff(diff: &str) -> Vec<PullRequestDiffLine> {
    let mut lines = Vec::new();
    let mut old_line = 0i32;
    let mut new_line = 0i32;
    for raw in diff.lines() {
        if raw.starts_with("@@") {
            if let Some((old_start, new_start)) = parse_hunk_starts(raw) {
                old_line = old_start;
                new_line = new_start;
            }
            lines.push(PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: DiffLineKind::Hunk,
                content: raw.to_string(),
            });
            continue;
        }
        if raw.starts_with("\\ No newline at end of file") {
            lines.push(PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: DiffLineKind::Meta,
                content: raw.to_string(),
            });
            continue;
        }
        let (kind, old_number, new_number, content) = if let Some(content) = raw.strip_prefix('+') {
            let current = new_line;
            new_line = new_line.saturating_add(1);
            (DiffLineKind::Addition, 0, current, content)
        } else if let Some(content) = raw.strip_prefix('-') {
            let current = old_line;
            old_line = old_line.saturating_add(1);
            (DiffLineKind::Deletion, current, 0, content)
        } else {
            let content = raw.strip_prefix(' ').unwrap_or(raw);
            let current_old = old_line;
            let current_new = new_line;
            old_line = old_line.saturating_add(1);
            new_line = new_line.saturating_add(1);
            (DiffLineKind::Context, current_old, current_new, content)
        };
        lines.push(PullRequestDiffLine {
            old_line: old_number,
            new_line: new_number,
            kind,
            content: content.to_string(),
        });
    }
    lines
}

fn parse_hunk_starts(header: &str) -> Option<(i32, i32)> {
    let mut fields = header.split_whitespace();
    (fields.next()? == "@@").then_some(())?;
    let old = parse_hunk_range(fields.next()?, '-')?;
    let new = parse_hunk_range(fields.next()?, '+')?;
    Some((old, new))
}

fn parse_hunk_range(value: &str, prefix: char) -> Option<i32> {
    value
        .strip_prefix(prefix)?
        .split(',')
        .next()?
        .parse::<i32>()
        .ok()
}

pub(super) fn unavailable_diff_file(
    path: String,
    old_path: String,
    change_type: DiffChangeType,
    added_lines: u32,
    removed_lines: u32,
    message: &str,
) -> PullRequestDiffFile {
    PullRequestDiffFile {
        path,
        old_path,
        change_type,
        added_lines,
        removed_lines,
        viewed: false,
        lines: vec![PullRequestDiffLine {
            old_line: 0,
            new_line: 0,
            kind: DiffLineKind::Meta,
            content: message.to_string(),
        }],
    }
}
