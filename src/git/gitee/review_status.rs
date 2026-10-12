use serde_json::Value;

use super::super::PullRequestReviewStatus;
use super::{enterprise_data_object, normalize_enterprise_review_state, string_field};

fn explicit_review_status(value: &Value) -> Option<PullRequestReviewStatus> {
    ["review_state", "reviewState"]
        .into_iter()
        .find_map(|field| {
            let state = string_field(value, field)?;
            match state.trim().to_ascii_lowercase().as_str() {
                "review_approved" | "approved" => Some(PullRequestReviewStatus::Approved),
                "review_rejected" | "rejected" | "changes_requested" => {
                    Some(PullRequestReviewStatus::ChangesRequested)
                }
                "review_pending" | "pending" => Some(PullRequestReviewStatus::ReviewRequired),
                _ => None,
            }
        })
}

fn reviewer_status(value: &Value) -> PullRequestReviewStatus {
    let mut approved = false;
    let mut pending = false;
    for reviewer in ["general_reviewers", "primary_reviewers"]
        .into_iter()
        .filter_map(|field| value.get(field).and_then(Value::as_array))
        .flatten()
    {
        let Some(state) = string_field(reviewer, "approve_status")
            .or_else(|| string_field(reviewer, "approveStatus"))
        else {
            continue;
        };
        match normalize_enterprise_review_state(state.as_str()).as_str() {
            "rejected" => return PullRequestReviewStatus::ChangesRequested,
            "approved" => approved = true,
            "initialized" | "pending" | "review_pending" => pending = true,
            _ => {}
        }
    }
    // An active approval is a review decision. It does not assert that all
    // required reviewers have approved or that the request is mergeable.
    if approved {
        PullRequestReviewStatus::Approved
    } else if pending {
        PullRequestReviewStatus::ReviewRequired
    } else {
        PullRequestReviewStatus::None
    }
}

pub(super) fn gitee_list_review_status(value: &Value) -> PullRequestReviewStatus {
    let value = enterprise_data_object(value);
    display_review_status([
        explicit_review_status(value).unwrap_or(PullRequestReviewStatus::None),
        reviewer_status(value),
    ])
}

pub(super) fn enterprise_review_state(detail: &Value, current: &Value) -> String {
    let detail = enterprise_data_object(detail);
    let current = enterprise_data_object(current);
    let status = display_review_status([
        explicit_review_status(detail).unwrap_or(PullRequestReviewStatus::None),
        explicit_review_status(current).unwrap_or(PullRequestReviewStatus::None),
        reviewer_status(detail),
        reviewer_status(current),
    ]);
    canonical_review_state(status)
}

fn display_review_status(
    statuses: impl IntoIterator<Item = PullRequestReviewStatus>,
) -> PullRequestReviewStatus {
    let mut approved = false;
    let mut pending = false;
    for status in statuses {
        match status {
            PullRequestReviewStatus::ChangesRequested => {
                return PullRequestReviewStatus::ChangesRequested;
            }
            PullRequestReviewStatus::Approved => approved = true,
            PullRequestReviewStatus::ReviewRequired => pending = true,
            PullRequestReviewStatus::None => {}
        }
    }
    // Pending requirements belong to merge readiness. They do not erase an
    // active approval displayed independently in the list and review header.
    if approved {
        PullRequestReviewStatus::Approved
    } else if pending {
        PullRequestReviewStatus::ReviewRequired
    } else {
        PullRequestReviewStatus::None
    }
}

pub(super) fn enterprise_native_review_state(detail: &Value, current: &Value) -> String {
    let status = explicit_review_status(enterprise_data_object(detail))
        .or_else(|| explicit_review_status(enterprise_data_object(current)))
        .unwrap_or(PullRequestReviewStatus::None);
    canonical_review_state(status)
}

fn canonical_review_state(status: PullRequestReviewStatus) -> String {
    match status {
        PullRequestReviewStatus::Approved => "review_approved",
        PullRequestReviewStatus::ChangesRequested => "review_rejected",
        PullRequestReviewStatus::ReviewRequired => "review_pending",
        PullRequestReviewStatus::None => "",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        enterprise_native_review_state, enterprise_review_state, gitee_list_review_status,
    };
    use crate::git::{PullRequestReadinessKind, PullRequestReviewStatus};
    use serde_json::json;

    #[test]
    fn enterprise_reviewer_decisions_agree_between_list_and_action_state() {
        for (native, status, state) in [
            (
                "approvedAndCommented",
                PullRequestReviewStatus::Approved,
                "review_approved",
            ),
            (
                "rejectedAndCommented",
                PullRequestReviewStatus::ChangesRequested,
                "review_rejected",
            ),
            (
                "initialized",
                PullRequestReviewStatus::ReviewRequired,
                "review_pending",
            ),
        ] {
            let detail = json!({
                "data": {
                    "review_state": "",
                    "general_reviewers": [{ "id": 1, "approve_status": native }],
                    "primary_reviewers": []
                }
            });
            assert_eq!(gitee_list_review_status(&detail), status);
            assert_eq!(enterprise_review_state(&detail, &json!({})), state);
        }
    }

    #[test]
    fn enterprise_rejections_override_approvals_without_inventing_merge_readiness() {
        let detail = json!({
            "general_reviewers": [{ "id": 1, "approve_status": "approved" }],
            "primary_reviewers": [{ "id": 2, "approve_status": "rejected" }]
        });
        assert_eq!(
            gitee_list_review_status(&detail),
            PullRequestReviewStatus::ChangesRequested
        );
        assert_eq!(
            enterprise_review_state(&detail, &json!({})),
            "review_rejected"
        );

        let detail = json!({
            "general_reviewers": [{ "id": 1, "approve_status": "approved" }],
            "primary_reviewers": [{ "id": 2, "approve_status": "initialized" }]
        });
        let current = json!({
            "state": "opened",
            "can_merge": false,
            "primary_review": { "meet_merger_conditions": false }
        });
        let review_state = enterprise_review_state(&detail, &current);
        assert_eq!(
            gitee_list_review_status(&detail),
            PullRequestReviewStatus::Approved
        );
        assert_eq!(review_state, "review_approved");
        assert_eq!(
            super::super::gitee_enterprise_merge_readiness(
                &current,
                &detail,
                &enterprise_native_review_state(&detail, &current),
            )
            .kind,
            PullRequestReadinessKind::ReviewRequired,
        );
    }

    #[test]
    fn optional_pending_reviewers_do_not_override_native_merge_conditions() {
        let detail = json!({
            "general_reviewers": [{ "id": 1, "approve_status": "initialized" }]
        });
        let current = json!({
            "state": "opened",
            "can_merge": true,
            "general_review": { "meet_merger_conditions": true }
        });
        assert_eq!(enterprise_review_state(&detail, &current), "review_pending");
        assert_eq!(
            super::super::gitee_enterprise_merge_readiness(
                &current,
                &detail,
                &enterprise_native_review_state(&detail, &current),
            )
            .kind,
            PullRequestReadinessKind::Ready,
        );
    }

    #[test]
    fn pending_requirements_do_not_hide_active_approval() {
        let detail = json!({
            "reviewState": "review_pending",
            "general_reviewers": [{ "approve_status": "approved" }]
        });
        assert_eq!(
            gitee_list_review_status(&detail),
            PullRequestReviewStatus::Approved
        );
        assert_eq!(
            enterprise_review_state(&detail, &json!({})),
            "review_approved"
        );

        let current = json!({ "state": "opened", "can_merge": false });
        assert_eq!(
            super::super::gitee_enterprise_merge_readiness(
                &current,
                &detail,
                &enterprise_native_review_state(&detail, &current),
            )
            .kind,
            PullRequestReadinessKind::ReviewRequired,
        );
        let detail = json!({ "review_state": "review_pending" });
        assert_eq!(
            gitee_list_review_status(&detail),
            PullRequestReviewStatus::ReviewRequired
        );
        assert_eq!(enterprise_review_state(&detail, &current), "review_pending");
    }

    #[test]
    fn native_rejections_override_active_or_aggregate_approvals() {
        let current = json!({ "data": { "reviewState": "review_rejected" } });
        let detail = json!({
            "general_reviewers": [{ "approve_status": "approved" }]
        });
        assert_eq!(
            enterprise_review_state(&detail, &current),
            "review_rejected"
        );
        let detail = json!({
            "review_state": "review_approved",
            "general_reviewers": [{ "approve_status": "rejected" }]
        });
        assert_eq!(
            gitee_list_review_status(&detail),
            PullRequestReviewStatus::ChangesRequested
        );
        assert_eq!(
            enterprise_review_state(&detail, &json!({})),
            "review_rejected"
        );
    }

    #[test]
    fn mergeability_or_unknown_reviewer_states_do_not_imply_approval() {
        for detail in [
            json!({ "can_merge": true, "conflict_result": true }),
            json!({ "general_reviewers": [{ "id": 1 }] }),
            json!({ "general_reviewers": [{ "approve_status": "unknown" }] }),
            json!({ "primary_reviewers": [{ "approve_status": "commented" }] }),
            json!({ "general_reviewers": [], "primary_reviewers": [] }),
        ] {
            assert_eq!(
                gitee_list_review_status(&detail),
                PullRequestReviewStatus::None
            );
            assert_eq!(enterprise_review_state(&detail, &json!({})), "");
        }
    }

    #[test]
    fn withdrawn_approval_becomes_review_required() {
        let mut detail = json!({
            "general_reviewers": [{ "id": 1, "approve_status": "approved" }]
        });
        assert_eq!(
            enterprise_review_state(&detail, &json!({})),
            "review_approved"
        );
        detail["general_reviewers"][0]["approve_status"] = json!("initialized");
        assert_eq!(
            enterprise_review_state(&detail, &json!({})),
            "review_pending"
        );
        assert_eq!(
            gitee_list_review_status(&detail),
            PullRequestReviewStatus::ReviewRequired
        );
    }
}
