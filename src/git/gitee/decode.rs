use serde_json::Value;

use super::super::{
    PullRequestDiffDetail, PullRequestDiffFile, PullRequestDiffLine, commit_count_from_detail,
    decode_html_entities,
};
use super::{string_field, value_u32};

pub(super) fn decode_base64_content(payload: &Value) -> Result<String, String> {
    super::super::source::decode_base64_content(payload, "Gitee")
}

pub(super) fn decode_pull_request_diff(payload: &Value) -> Result<PullRequestDiffDetail, String> {
    let diffs = payload
        .get("diffs")
        .and_then(Value::as_array)
        .ok_or_else(|| "Gitee pull request diff response has no diff list.".to_string())?;
    let files = diffs.iter().map(decode_diff_file).collect::<Vec<_>>();
    let refs = payload.get("diff_refs").unwrap_or(&Value::Null);
    Ok(PullRequestDiffDetail {
        author_identity: Default::default(),
        added_lines: value_u32(payload, "added_lines")
            .unwrap_or_else(|| files.iter().map(|file| file.added_lines).sum()),
        removed_lines: value_u32(payload, "removed_lines")
            .unwrap_or_else(|| files.iter().map(|file| file.removed_lines).sum()),
        // Enterprise diffs document `commits_size` (not `commits_count`). Do not
        // use top-level `count`: that is the changed-file total.
        commit_count: commit_count_from_diff_payload(payload).unwrap_or(0),
        // Enterprise diffs omit comment totals; callers overlay MR detail when available.
        conversation_count: None,
        base_sha: string_field(refs, "base_sha").unwrap_or_default(),
        head_sha: string_field(refs, "head_sha").unwrap_or_default(),
        start_sha: string_field(refs, "start_sha").unwrap_or_default(),
        too_many_commits: payload
            .get("too_more_commits")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        files_truncated: value_u32(payload, "count")
            .is_some_and(|count| count as usize > files.len())
            || payload
                .get("diffs_truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        files,
    })
}

/// Enterprise MR diffs expose `commits_size`; some deployments also use
/// `commits_count` / `commit_count`. Never treat payload `count` as commits.
pub(super) fn commit_count_from_diff_payload(payload: &Value) -> Option<u32> {
    value_u32(payload, "commits_size")
        .or_else(|| value_u32(payload, "commits_count"))
        .or_else(|| value_u32(payload, "commit_count"))
        // Numeric `commits` for provider variants that expose a scalar count.
        .or_else(|| value_u32(payload, "commits"))
        // Some enterprise deployments omit every scalar but embed the complete
        // commit list in the diff response. A non-empty, non-truncated array is
        // exact and avoids a second request. Empty arrays remain "unknown".
        .or_else(|| {
            if payload.get("too_more_commits").and_then(Value::as_bool) != Some(false) {
                return None;
            }
            let commits = payload.get("commits")?.as_array()?;
            (!commits.is_empty())
                .then(|| u32::try_from(commits.len()).ok())
                .flatten()
        })
}

pub(super) fn trusted_enterprise_commit_count(detail: Option<&Value>, diff: &Value) -> Option<u32> {
    detail
        .and_then(commit_count_from_detail)
        .filter(|count| *count > 0)
        .or_else(|| commit_count_from_diff_payload(diff).filter(|count| *count > 0))
}

pub(super) fn decode_diff_file(value: &Value) -> PullRequestDiffFile {
    let statistic = value.get("statistic").unwrap_or(&Value::Null);
    let path = string_field(statistic, "new_path")
        .or_else(|| string_field(statistic, "path"))
        .unwrap_or_else(|| "Unknown file".to_string());
    let old_path = string_field(statistic, "old_path").unwrap_or_else(|| path.clone());
    let mut lines = value
        .get("content")
        .and_then(|content| content.get("text"))
        .and_then(Value::as_array)
        .map(|values| values.iter().map(decode_diff_line).collect::<Vec<_>>())
        .unwrap_or_default();
    if lines.is_empty() {
        lines.push(PullRequestDiffLine {
            old_line: 0,
            new_line: 0,
            kind: crate::git::DiffLineKind::Meta,
            content: "Binary, image, or oversized diff content is not available.".to_string(),
        });
    }

    PullRequestDiffFile {
        path,
        old_path,
        change_type: string_field(statistic, "type")
            .map(|value| crate::git::DiffChangeType::from_provider(value.as_str()))
            .unwrap_or_else(|| {
                if value
                    .get("new_file")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    crate::git::DiffChangeType::New
                } else if value
                    .get("deleted_file")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    crate::git::DiffChangeType::Deleted
                } else if value
                    .get("renamed_file")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    crate::git::DiffChangeType::Renamed
                } else {
                    crate::git::DiffChangeType::Modified
                }
            }),
        added_lines: value_u32(value, "added_lines").unwrap_or(0),
        removed_lines: value_u32(value, "removed_lines").unwrap_or(0),
        // Gitee documents statistic.viewed as file read permission, not the
        // current reviewer's viewed/unviewed progress used by GitHub's filter.
        viewed: false,
        lines,
    }
}

pub(super) fn decode_diff_line(value: &Value) -> PullRequestDiffLine {
    let raw_kind = string_field(value, "type").unwrap_or_default();
    PullRequestDiffLine {
        old_line: nested_line_number(value, "old_line"),
        new_line: nested_line_number(value, "new_line"),
        kind: crate::git::DiffLineKind::from_provider(raw_kind.as_str()),
        content: html_diff_line_to_text(
            string_field(value, "line_content")
                .unwrap_or_default()
                .as_str(),
        ),
    }
}

pub(super) fn nested_line_number(value: &Value, key: &str) -> i32 {
    value
        .get(key)
        .and_then(|line| line.get("line_num"))
        .and_then(|number| {
            number
                .as_i64()
                .or_else(|| number.as_str().and_then(|value| value.parse().ok()))
        })
        .and_then(|number| i32::try_from(number).ok())
        .unwrap_or(0)
}

pub(super) fn html_line_to_text(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    let mut inside_tag = false;
    for character in value.chars() {
        match character {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => text.push(character),
            _ => {}
        }
    }
    decode_html_entities(text.as_str())
}

pub(super) fn html_diff_line_to_text(value: &str) -> String {
    // The enterprise diff endpoint sometimes pretty-prints its syntax HTML
    // with literal CR/LF characters between spans. A PullRequestDiffLine is
    // one source line, so retaining those separators makes Slint lay out one
    // fixed-height diff row as multiple text lines. Remove only line-ending
    // controls after decoding; ordinary spaces and tabs remain source data.
    html_line_to_text(value)
        .chars()
        .filter(|character| !matches!(character, '\r' | '\n'))
        .collect()
}
