use serde_json::Value;

use super::super::{PullRequestReadiness, PullRequestReadinessKind};
use super::string_field;

pub(super) fn gitee_enterprise_merge_readiness(
    current: &Value,
    detail: &Value,
    review_state: &str,
) -> PullRequestReadiness {
    let state = string_field(current, "state")
        .or_else(|| string_field(detail, "state"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if state == "merged" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Merged,
            "Gitee Enterprise reports that this pull request has been merged.",
        );
    }
    if state == "closed" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Closed,
            "Gitee Enterprise reports that this pull request is closed.",
        );
    }
    if state == "drafted"
        || detail.get("draft").and_then(Value::as_bool) == Some(true)
        || detail.get("is_draft").and_then(Value::as_bool) == Some(true)
    {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Draft,
            "Mark the Gitee Enterprise pull request as ready before merging.",
        );
    }

    // The enterprise v8 document defines conflict_result=true as “no
    // conflict”; false is the conflicting state.
    if current.get("conflict_result").and_then(Value::as_bool) == Some(false) {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Conflicts,
            "Gitee Enterprise reports merge conflicts between the source and target branches.",
        );
    }
    if review_state == "review_rejected" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::ChangesRequested,
            "A Gitee Enterprise reviewer requested changes.",
        );
    }
    if current.get("scan_quality_state").and_then(Value::as_bool) == Some(false) {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::ChecksFailed,
            "The Gitee Enterprise quality gate has not passed.",
        );
    }
    let note = current.get("note").unwrap_or(&Value::Null);
    let unresolved_notes = note
        .get("resolve_note_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && note.get("resolve").and_then(Value::as_u64).unwrap_or(0)
            < note.get("total").and_then(Value::as_u64).unwrap_or(0);
    if unresolved_notes {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::DiscussionsUnresolved,
            "Resolve all required Gitee Enterprise review comments before merging.",
        );
    }
    let review_requirement_unmet = ["primary_review", "general_review"].iter().any(|key| {
        current
            .get(key)
            .and_then(|review| review.get("meet_merger_conditions"))
            .and_then(Value::as_bool)
            == Some(false)
    });
    if review_state == "review_pending" || review_requirement_unmet {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::ReviewRequired,
            "Required Gitee Enterprise reviews have not been completed.",
        );
    }
    if current.get("strategy_result").and_then(Value::as_bool) == Some(false) {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Blocked,
            "A Gitee Enterprise merge strategy or repository rule is blocking this merge.",
        );
    }
    if current
        .get("can_merge")
        .and_then(Value::as_bool)
        .or_else(|| current.get("can_be_merge").and_then(Value::as_bool))
        == Some(true)
    {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Ready,
            "All merge conditions reported by Gitee Enterprise are satisfied.",
        );
    }
    PullRequestReadiness::new(
        PullRequestReadinessKind::Blocked,
        "Gitee Enterprise reports that this pull request does not currently meet the merge conditions.",
    )
}

pub(super) fn gitee_public_merge_readiness(detail: &Value) -> PullRequestReadiness {
    let state = string_field(detail, "state")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if state == "merged" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Merged,
            "Gitee public v5 reports that this pull request has been merged.",
        );
    }
    if state == "closed" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Closed,
            "Gitee public v5 reports that this pull request is closed.",
        );
    }
    if detail.get("draft").and_then(Value::as_bool) == Some(true)
        || detail.get("is_draft").and_then(Value::as_bool) == Some(true)
    {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Draft,
            "Mark the Gitee pull request as ready before merging.",
        );
    }
    match detail
        .get("mergeable")
        .and_then(Value::as_bool)
        .or_else(|| detail.get("can_merge").and_then(Value::as_bool))
    {
        Some(true) if state == "open" || state == "opened" => PullRequestReadiness::new(
            PullRequestReadinessKind::Ready,
            "Gitee public v5 reports that this pull request can be merged.",
        ),
        Some(false) => PullRequestReadiness::new(
            PullRequestReadinessKind::Blocked,
            "Gitee public v5 reports that this pull request cannot currently be merged.",
        ),
        _ => PullRequestReadiness::new(
            PullRequestReadinessKind::Unavailable,
            "Gitee public v5 did not return a mergeable state for this pull request.",
        ),
    }
}
