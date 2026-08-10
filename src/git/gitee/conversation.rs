use serde_json::Value;

use super::super::{PullRequestCommit, PullRequestConversationItem};
use super::{nested_string, string_field};

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
        author: string_field(value, "name")
            .or_else(|| string_field(value, "username"))
            .or_else(|| string_field(value, "user_name"))
            .or_else(|| string_field(value, "login"))
            .or_else(|| nested_string(value, "author", "name"))
            .or_else(|| nested_string(value, "author", "username"))
            .or_else(|| nested_string(value, "user", "name"))
            .or_else(|| nested_string(value, "user", "login"))
            .or_else(|| nested_string(value, "reviewer", "name"))
            .or_else(|| nested_string(value, "reviewer", "username"))
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
        "approved" | "approvedandcommented" => "approved",
        "rejected" | "rejectedandcommented" => "rejected",
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
