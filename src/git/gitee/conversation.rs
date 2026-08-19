use serde_json::Value;

use super::super::{
    PullRequestCommit, PullRequestConversationItem, conversation_body_to_plain_text,
};
use super::{nested_string, string_field};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GiteeMergeEvent {
    pub(super) author: String,
    pub(super) avatar_url: String,
    pub(super) created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GiteeUserLabel {
    text: String,
    quality: u8,
}

pub(super) fn decode_gitee_operate_log_activity(
    values: &[Value],
) -> Vec<PullRequestConversationItem> {
    values
        .iter()
        .filter_map(|value| {
            let action = gitee_operate_log_action(value);
            let content = gitee_operate_log_content(value);
            let created_at = string_field(value, "created_at")
                .or_else(|| string_field(value, "createdAt"))
                .unwrap_or_default();
            let author =
                gitee_operate_log_user_name(value).unwrap_or_else(|| "Unknown author".to_string());
            let id = string_field(value, "id").unwrap_or_else(|| created_at.clone());

            if is_gitee_merge_log(action.as_str(), content.as_str()) {
                return None;
            }
            if is_gitee_reviewer_assignment_log(action.as_str(), content.as_str()) {
                let reviewer = gitee_operate_log_reviewer(value, content.as_str());
                return Some(PullRequestConversationItem {
                    id: format!("gitee-review-requested-{id}"),
                    kind: "review_requested".to_string(),
                    author,
                    avatar_url: gitee_operate_log_user_avatar(value).unwrap_or_default(),
                    author_association: String::new(),
                    body: if reviewer.is_some() {
                        "requested a review from".to_string()
                    } else {
                        "requested a review".to_string()
                    },
                    created_at,
                    state: reviewer.unwrap_or_default(),
                    review_id: String::new(),
                    can_dismiss: false,
                });
            }
            let state = gitee_operate_log_review_state(value, action.as_str(), content.as_str())?;
            Some(PullRequestConversationItem {
                id: format!("gitee-review-summary-{id}"),
                kind: "review_summary".to_string(),
                author,
                avatar_url: gitee_operate_log_user_avatar(value).unwrap_or_default(),
                author_association: String::new(),
                body: super::gitee_review_action(state).to_string(),
                created_at,
                state: state.to_string(),
                review_id: id,
                can_dismiss: false,
            })
        })
        .collect()
}

pub(super) fn merge_gitee_review_summaries(
    activity: &mut Vec<PullRequestConversationItem>,
    fallback: Vec<PullRequestConversationItem>,
) {
    for summary in fallback {
        let matching_log = activity.iter_mut().rfind(|item| {
            item.kind == "review_summary"
                && item.author.eq_ignore_ascii_case(summary.author.as_str())
                && item.state == summary.state
        });
        if let Some(log) = matching_log {
            log.can_dismiss = summary.can_dismiss;
            log.review_id = summary.review_id;
        } else {
            activity.push(summary);
        }
    }
}

#[cfg(test)]
pub(super) fn gitee_merge_event(
    detail: &Value,
    operate_logs: Option<&Vec<Value>>,
    merge_commit: Option<&Value>,
) -> GiteeMergeEvent {
    gitee_merge_event_with_user(detail, operate_logs, merge_commit, None)
}

pub(super) fn gitee_merge_event_with_user(
    detail: &Value,
    operate_logs: Option<&Vec<Value>>,
    merge_commit: Option<&Value>,
    resolved_user: Option<&Value>,
) -> GiteeMergeEvent {
    if let Some(event) = latest_gitee_merge_log(operate_logs) {
        let merger = gitee_detail_merger(detail);
        let actor = best_gitee_user_label(
            resolved_user
                .into_iter()
                .chain(gitee_operate_log_users(event))
                .chain(merger.into_iter()),
        );
        return GiteeMergeEvent {
            author: best_gitee_labels(
                actor
                    .into_iter()
                    .chain(merge_commit.and_then(gitee_commit_committer_label)),
            )
            .map(|label| label.text)
            .unwrap_or_else(|| "Unknown merger".to_string()),
            avatar_url: resolved_user
                .and_then(gitee_user_avatar)
                .or_else(|| gitee_operate_log_user_avatar(event))
                .or_else(|| merger.and_then(gitee_user_avatar))
                .or_else(|| merge_commit.and_then(gitee_commit_committer_avatar))
                .unwrap_or_default(),
            created_at: string_field(event, "created_at")
                .or_else(|| string_field(event, "createdAt"))
                .unwrap_or_default(),
        };
    }

    let merger = gitee_detail_merger(detail);
    GiteeMergeEvent {
        author: best_gitee_labels(
            resolved_user
                .and_then(gitee_user_label)
                .into_iter()
                .chain(merger.and_then(gitee_user_label).into_iter())
                .chain(merge_commit.and_then(gitee_commit_committer_label)),
        )
        .map(|label| label.text)
        .unwrap_or_else(|| "Unknown merger".to_string()),
        avatar_url: resolved_user
            .and_then(gitee_user_avatar)
            .or_else(|| merger.and_then(gitee_user_avatar))
            .or_else(|| merge_commit.and_then(gitee_commit_committer_avatar))
            .unwrap_or_default(),
        created_at: String::new(),
    }
}

pub(super) fn gitee_merge_actor_account(
    detail: &Value,
    operate_logs: Option<&Vec<Value>>,
) -> Option<String> {
    latest_gitee_merge_log(operate_logs)
        .into_iter()
        .flat_map(gitee_operate_log_users)
        .find_map(gitee_user_account)
        .or_else(|| gitee_detail_merger(detail).and_then(gitee_user_account))
}

fn latest_gitee_merge_log(operate_logs: Option<&Vec<Value>>) -> Option<&Value> {
    operate_logs?
        .iter()
        .filter(|value| {
            let action = gitee_operate_log_action(value);
            let content = gitee_operate_log_content(value);
            is_gitee_merge_log(action.as_str(), content.as_str())
        })
        .max_by_key(|value| {
            string_field(value, "created_at")
                .or_else(|| string_field(value, "createdAt"))
                .unwrap_or_default()
        })
}

fn gitee_operate_log_content(value: &Value) -> String {
    let content = [
        "content",
        "title",
        "message",
        "description",
        "desc",
        "body",
        "note",
    ]
    .into_iter()
    .find_map(|key| string_field(value, key))
    .unwrap_or_default();
    conversation_body_to_plain_text(content.as_str())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn gitee_operate_log_action(value: &Value) -> String {
    [
        "action_type",
        "actionType",
        "action",
        "operate_type",
        "operateType",
        "operating",
    ]
    .into_iter()
    .find_map(|key| string_field(value, key))
    .unwrap_or_default()
    .to_ascii_lowercase()
}

fn gitee_operate_log_user_name(value: &Value) -> Option<String> {
    best_gitee_user_label(gitee_operate_log_users(value)).map(|label| label.text)
}

fn gitee_operate_log_target_name(value: &Value) -> Option<String> {
    value.get("target").and_then(gitee_user_name)
}

fn gitee_operate_log_reviewer(value: &Value, content: &str) -> Option<String> {
    gitee_operate_log_target_name(value).or_else(|| {
        let assigned = content
            .strip_prefix("指派了")
            .or_else(|| content.strip_prefix("指派"))?
            .trim();
        assigned
            .strip_suffix("参与评审")
            .map(str::trim)
            .filter(|reviewer| !reviewer.is_empty())
            .map(str::to_string)
    })
}

fn gitee_user_name(value: &Value) -> Option<String> {
    gitee_user_label(value).map(|label| label.text)
}

fn gitee_operate_log_user_avatar(value: &Value) -> Option<String> {
    gitee_operate_log_users(value).find_map(gitee_user_avatar)
}

fn gitee_operate_log_users(value: &Value) -> impl Iterator<Item = &Value> {
    [
        "user",
        "operator",
        "operator_user",
        "operatorUser",
        "author",
        "creator",
        "merge_user",
        "merged_by",
    ]
    .into_iter()
    .filter_map(|key| value.get(key))
    .chain(std::iter::once(value))
}

fn gitee_detail_merger(detail: &Value) -> Option<&Value> {
    ["merge_user", "merged_by", "merger", "merge_by"]
        .into_iter()
        .find_map(|key| detail.get(key))
}

fn best_gitee_user_label<'a>(users: impl Iterator<Item = &'a Value>) -> Option<GiteeUserLabel> {
    best_gitee_labels(users.filter_map(gitee_user_label))
}

fn best_gitee_labels(labels: impl Iterator<Item = GiteeUserLabel>) -> Option<GiteeUserLabel> {
    labels.reduce(|best, candidate| {
        if candidate.quality > best.quality {
            candidate
        } else {
            best
        }
    })
}

fn gitee_commit_committer_label(value: &Value) -> Option<GiteeUserLabel> {
    let nested = ["committer", "committed_by", "committedBy"]
        .into_iter()
        .filter_map(|key| value.get(key))
        .filter_map(gitee_user_label);
    let names = ["committer_name", "committerName"]
        .into_iter()
        .filter_map(|key| string_field(value, key))
        .filter_map(|name| normalized_user_label(name.as_str(), 3));
    let emails = ["committer_email", "committerEmail"]
        .into_iter()
        .filter_map(|key| string_field(value, key))
        .filter_map(|email| normalized_user_label(email.as_str(), 1));
    best_gitee_labels(nested.chain(names).chain(emails))
}

fn gitee_commit_committer_avatar(value: &Value) -> Option<String> {
    ["committer", "committed_by", "committedBy"]
        .into_iter()
        .filter_map(|key| value.get(key))
        .find_map(gitee_user_avatar)
        .or_else(|| string_field(value, "committer_avatar_url"))
        .or_else(|| string_field(value, "committerAvatarUrl"))
}

fn gitee_user_label(value: &Value) -> Option<GiteeUserLabel> {
    if let Some(value) = value.as_str() {
        return normalized_user_label(value, 2);
    }

    // Enterprise deployments often use `name` for the SSO email while keeping
    // the human-facing label in remark/display-name fields. Choose by display
    // quality instead of accepting the first populated field.
    let display_fields = [
        "display_name",
        "displayName",
        "nickname",
        "nick_name",
        "real_name",
        "realName",
        "full_name",
        "fullName",
        "remark",
        "name",
    ];
    let login_fields = ["username", "user_name", "login"];
    display_fields
        .into_iter()
        .filter_map(|field| string_field(value, field))
        .filter_map(|value| normalized_user_label(value.as_str(), 3))
        .chain(
            login_fields
                .into_iter()
                .filter_map(|field| string_field(value, field))
                .filter_map(|value| normalized_user_label(value.as_str(), 2)),
        )
        .reduce(|best, candidate| {
            if candidate.quality > best.quality {
                candidate
            } else {
                best
            }
        })
}

fn gitee_user_account(value: &Value) -> Option<String> {
    if let Some(value) = value.as_str() {
        return normalized_account(value);
    }

    [
        "username",
        "user_name",
        "login",
        "author_username",
        "operator_username",
    ]
    .into_iter()
    .find_map(|field| string_field(value, field))
    .and_then(|value| normalized_account(value.as_str()))
    .or_else(|| string_field(value, "email").and_then(|value| normalized_account(value.as_str())))
    .or_else(|| {
        ["name", "display_name", "displayName"]
            .into_iter()
            .find_map(|field| string_field(value, field))
            .and_then(|value| normalized_account(value.as_str()))
    })
}

fn normalized_account(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let account = value
        .split_once('@')
        .map_or(value, |(account, _)| account)
        .trim();
    (!account.is_empty()).then(|| account.to_string())
}

fn normalized_user_label(value: &str, quality: u8) -> Option<GiteeUserLabel> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if looks_like_email(value) {
        let account = value.split_once('@')?.0.trim();
        return (!account.is_empty()).then(|| GiteeUserLabel {
            text: account.to_string(),
            quality: 1,
        });
    }
    Some(GiteeUserLabel {
        text: value.to_string(),
        quality,
    })
}

fn looks_like_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.trim().is_empty() && domain.contains('.') && !domain.chars().any(char::is_whitespace)
}

fn gitee_user_avatar(value: &Value) -> Option<String> {
    string_field(value, "avatar_url")
        .or_else(|| string_field(value, "avatar"))
        .or_else(|| string_field(value, "image_path"))
        .or_else(|| string_field(value, "imagePath"))
        .filter(|avatar| !avatar.trim().is_empty())
}

fn is_gitee_merge_log(action: &str, content: &str) -> bool {
    let approval_action = action.contains("allow")
        || action.contains("approv")
        || action.contains("accept")
        || action.contains("review");
    let compact_content = content.split_whitespace().collect::<String>();
    let lower_content = content.to_ascii_lowercase();
    let approval_content = compact_content.contains("允许合并")
        || compact_content.contains("同意合并")
        || compact_content.contains("不允许合并")
        || lower_content.contains("allow merge")
        || lower_content.contains("approved");
    matches!(action, "merged_pr" | "merge_pr" | "merged_pull_request")
        || (!approval_action
            && action.contains("merge")
            && (action.contains("pr") || action.contains("pull")))
        || (!approval_content
            && (compact_content.contains("合并了PullRequest")
                || compact_content.contains("合并PullRequest")
                || lower_content.contains("merged pull request")))
}

fn is_gitee_reviewer_assignment_log(action: &str, content: &str) -> bool {
    matches!(
        action,
        "set_assignee" | "set_reviewer" | "add_reviewer" | "review_requested"
    ) || ((action.contains("assign") || action.contains("reviewer"))
        && (action.contains("set") || action.contains("add") || action.contains("request")))
        || content.contains("参与评审")
        || (content.contains("指派") && content.contains("评审"))
        || content
            .to_ascii_lowercase()
            .contains("requested a review from")
}

fn gitee_operate_log_review_state(
    value: &Value,
    action: &str,
    content: &str,
) -> Option<&'static str> {
    let lower_content = content.to_ascii_lowercase();
    let data = value.get("data");
    let structured_state = std::iter::once(Some(value))
        .chain([
            data,
            value.get("review"),
            data.and_then(|data| data.get("review")),
            data.and_then(|data| data.get("review_detail")),
        ])
        .flatten()
        .find_map(|candidate| {
            [
                "review_state",
                "reviewState",
                "review_status",
                "reviewStatus",
                "approve_status",
                "approveStatus",
                "approve_state",
                "approveState",
                "result",
                "status",
                "state",
            ]
            .into_iter()
            .find_map(|key| string_field(candidate, key))
        })
        .unwrap_or_default()
        .to_ascii_lowercase();
    if action.contains("reject")
        || action.contains("disallow")
        || action.contains("request_changes")
        || structured_state.contains("reject")
        || structured_state.contains("request_changes")
        || structured_state.contains("not_allow")
        || content.contains("审查不通过")
        || content.contains("审核不通过")
        || content.contains("评审不通过")
        || content.contains("不允许合并")
        || content.contains("拒绝合并")
        || content.contains("拒绝")
        || lower_content.contains("requested changes")
        || lower_content.contains("rejected")
    {
        return Some("rejected");
    }
    if action.contains("approv")
        || action.contains("allow_merge")
        || (action.contains("accept") && (action.contains("pr") || action.contains("review")))
        || structured_state.contains("approv")
        || structured_state == "accepted"
        || structured_state.contains("allow_merge")
        || content.contains("审查通过")
        || content.contains("审核通过")
        || content.contains("评审通过")
        || content.contains("允许合并")
        || content.contains("同意合并")
        || lower_content.contains("approved")
        || lower_content.contains("accepted the pull request")
    {
        return Some("approved");
    }
    None
}

pub(super) fn expand_enterprise_conversation_notes(
    values: &[Value],
) -> Vec<PullRequestConversationItem> {
    let mut items = Vec::new();
    for value in values {
        let mut nodes = Vec::new();
        collect_enterprise_note_nodes(value, &mut nodes);
        items.extend(nodes.into_iter().map(decode_conversation_comment));
    }
    items
}

pub(super) fn collect_enterprise_note_nodes<'a>(value: &'a Value, out: &mut Vec<&'a Value>) {
    if let Some(notes) = value.get("notes").and_then(Value::as_array) {
        if !notes.is_empty() {
            for note in notes {
                out.push(note);
                // Enterprise comments are two levels: root + children replies.
                if let Some(children) = note.get("children").and_then(Value::as_array) {
                    out.extend(children.iter());
                }
            }
            return;
        }
    }
    // Theme / ordinary comments may appear as a flat `note` on the data item.
    // Skip thread shells that only carry diff-context `content` and no body.
    if conversation_note_body(value).is_empty() {
        return;
    }
    out.push(value);
}

pub(super) fn decode_conversation_comment(value: &Value) -> PullRequestConversationItem {
    PullRequestConversationItem {
        id: string_field(value, "id").unwrap_or_default(),
        kind: if value
            .get("system")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            "event".to_string()
        } else {
            "comment".to_string()
        },
        author: nested_string(value, "author", "name")
            .or_else(|| nested_string(value, "author", "username"))
            .or_else(|| nested_string(value, "user", "name"))
            .or_else(|| nested_string(value, "user", "login"))
            .unwrap_or_else(|| "Unknown author".to_string()),
        avatar_url: nested_string(value, "author", "avatar_url")
            .or_else(|| nested_string(value, "author", "avatar"))
            .or_else(|| nested_string(value, "user", "avatar_url"))
            .or_else(|| nested_string(value, "user", "avatar"))
            .unwrap_or_default(),
        author_association: String::new(),
        // Nested enterprise discussion threads use `note`. Official enterprise
        // notes use markdown string `content` (objects are diff context only).
        // Public v5 uses `body`; review payloads may use `comment`.
        body: conversation_note_body(value),
        created_at: string_field(value, "created_at")
            .or_else(|| string_field(value, "createdAt"))
            .unwrap_or_default(),
        state: string_field(value, "state").unwrap_or_default(),
        review_id: String::new(),
        can_dismiss: false,
    }
}

pub(super) fn decode_conversation_review(value: &Value) -> Option<PullRequestConversationItem> {
    let state = string_field(value, "state")
        .or_else(|| string_field(value, "result"))
        .or_else(|| string_field(value, "approve_status"))
        .or_else(|| string_field(value, "approveStatus"))
        .or_else(|| string_field(value, "review_state"))
        .or_else(|| string_field(value, "reviewState"))
        .or_else(|| string_field(value, "review_status"))
        .or_else(|| string_field(value, "reviewStatus"))
        .map(|state| normalize_enterprise_review_state(state.as_str()))
        .unwrap_or_default();
    let body = conversation_note_body(value);
    if body.trim().is_empty() && state.trim().is_empty() {
        return None;
    }
    Some(PullRequestConversationItem {
        id: string_field(value, "id")
            .map(|id| format!("review-{id}"))
            .unwrap_or_else(|| format!("review-{}", body.len())),
        kind: "review".to_string(),
        author: best_gitee_user_label(
            std::iter::once(value).chain(
                ["author", "user", "reviewer"]
                    .into_iter()
                    .filter_map(|key| value.get(key)),
            ),
        )
        .map(|label| label.text)
        .unwrap_or_else(|| "Unknown reviewer".to_string()),
        avatar_url: string_field(value, "avatar_url")
            .or_else(|| string_field(value, "avatar"))
            .or_else(|| nested_string(value, "author", "avatar_url"))
            .or_else(|| nested_string(value, "author", "avatar"))
            .or_else(|| nested_string(value, "user", "avatar_url"))
            .or_else(|| nested_string(value, "user", "avatar"))
            .or_else(|| nested_string(value, "reviewer", "avatar_url"))
            .or_else(|| nested_string(value, "reviewer", "avatar"))
            .unwrap_or_default(),
        author_association: String::new(),
        body,
        created_at: string_field(value, "created_at")
            .or_else(|| string_field(value, "createdAt"))
            .or_else(|| string_field(value, "updated_at"))
            .or_else(|| string_field(value, "updatedAt"))
            .unwrap_or_default(),
        state,
        review_id: string_field(value, "id").unwrap_or_default(),
        can_dismiss: false,
    })
}

pub(super) fn normalize_enterprise_review_state(state: &str) -> String {
    match state.trim().to_ascii_lowercase().as_str() {
        "approved"
        | "approvedandcommented"
        | "review_approved"
        | "accepted"
        | "allow_merge"
        | "allowed_to_merge" => "approved",
        "rejected"
        | "rejectedandcommented"
        | "review_rejected"
        | "request_changes"
        | "changes_requested"
        | "disallow_merge" => "rejected",
        "commented" => "commented",
        "initialized" => "initialized",
        other => other,
    }
    .to_string()
}

pub(super) fn conversation_note_body(value: &Value) -> String {
    // Gitee-only body resolution (GitHub/GitLab keep their own adapters).
    // Internal enterprise notes docs: top-level / notes[].note is the markdown
    // body; top-level content is diff context and must stay ignored as an object.
    // Review submit responses nest the same text at note.note. Public v5 uses body.
    string_field(value, "note")
        .filter(|text| !text.trim().is_empty())
        .or_else(|| nested_string(value, "note", "note").filter(|text| !text.trim().is_empty()))
        .or_else(|| string_field(value, "body").filter(|text| !text.trim().is_empty()))
        .or_else(|| string_field(value, "comment").filter(|text| !text.trim().is_empty()))
        .or_else(|| string_field(value, "content").filter(|text| !text.trim().is_empty()))
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

pub(super) fn normalize_enterprise_commit_order(commits: &mut [PullRequestCommit]) {
    let parse_time = |commit: &PullRequestCommit| {
        chrono::DateTime::parse_from_rfc3339(commit.authored_at.trim()).ok()
    };
    let Some(first_time) = commits.iter().find_map(parse_time) else {
        return;
    };
    let Some(last_time) = commits.iter().rev().find_map(parse_time) else {
        return;
    };
    if first_time > last_time {
        commits.reverse();
    }
}

/// Bounded commit probe for the Commits tab badge. Never loads/caches the full list.
///
/// Enterprise v8 defaults `with_page=false` and returns every commit as a bare

pub(super) fn decode_commit(value: &Value) -> PullRequestCommit {
    let message = string_field(value, "message")
        .or_else(|| {
            value
                .get("commit")
                .and_then(|commit| string_field(commit, "message"))
        })
        .unwrap_or_default();
    PullRequestCommit {
        sha: string_field(value, "sha")
            .or_else(|| string_field(value, "id"))
            .unwrap_or_default(),
        title: string_field(value, "title")
            .unwrap_or_else(|| message.lines().next().unwrap_or_default().to_string()),
        message,
        author: string_field(value, "author_name")
            .or_else(|| nested_string(value, "author", "name"))
            .or_else(|| nested_string(value, "author", "login"))
            .or_else(|| {
                value
                    .get("commit")
                    .and_then(|commit| nested_string(commit, "author", "name"))
            })
            .unwrap_or_else(|| "Unknown author".to_string()),
        avatar_url: nested_string(value, "author", "avatar_url")
            .or_else(|| nested_string(value, "author", "avatar"))
            .or_else(|| nested_string(value, "committer", "avatar_url"))
            .or_else(|| nested_string(value, "committer", "avatar"))
            .unwrap_or_default(),
        // Internal enterprise MR commits document `created_at`; repository
        // commits still use authored_date/committed_date. Keep both profiles.
        authored_at: string_field(value, "authored_date")
            .or_else(|| string_field(value, "authoredDate"))
            .or_else(|| string_field(value, "committed_date"))
            .or_else(|| string_field(value, "committedDate"))
            .or_else(|| string_field(value, "created_at"))
            .or_else(|| string_field(value, "createdAt"))
            .or_else(|| {
                value
                    .get("commit")
                    .and_then(|commit| nested_string(commit, "author", "date"))
            })
            .unwrap_or_default(),
        verified: value
            .get("verification")
            .and_then(|verification| verification.get("verified"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}
