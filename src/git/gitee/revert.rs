use serde_json::Value;

use super::string_field;

pub(super) fn enterprise_commit_refs_can_revert(refs: &Value, pull_request_iid: &str) -> bool {
    let pull_request_matches = refs
        .get("pull_request")
        .filter(|pull_request| pull_request.is_object())
        .and_then(|pull_request| string_field(pull_request, "iid"))
        .map(|value| value == pull_request_iid)
        // Some enterprise versions document the association but omit the PR
        // object. The two explicit capability flags remain authoritative.
        .unwrap_or(true);
    pull_request_matches
        && refs
            .get("merge_commit_by_pr")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        && refs
            .get("can_revert")
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

pub(super) fn enterprise_data_object(payload: &Value) -> &Value {
    payload
        .get("data")
        .filter(|value| value.is_object())
        .unwrap_or(payload)
}
