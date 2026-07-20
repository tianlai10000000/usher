//! GitLab repository operations.

use reqwest::header::{ACCEPT, HeaderMap};
use serde::Deserialize;
use serde_json::Value;

use crate::codereview::api::gitlab as endpoints;
use crate::http::{self, RequestOptions};

use super::{
    PullRequestActionState, PullRequestDiffDetail, PullRequestDiffFile, PullRequestItem,
    PullRequestPage, PullRequestReadiness, PullRequestReadinessKind, PullRequestState, RepoLocator,
    RepoType, configured_api_base_url, configured_token, is_configured_enterprise_host,
    parse_unified_diff, percent_encode_path, unavailable_diff_file,
};

#[derive(Deserialize)]
struct GitLabProject {
    id: serde_json::Value,
}

pub(super) async fn get_repo_detail(locator: &RepoLocator) -> Result<String, String> {
    let full_path = format!("{}/{}", locator.namespace, locator.repo);
    let endpoint = endpoints::PROJECT_URL
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace(
            "{project_id_or_path}",
            percent_encode_path(full_path.as_str()).as_str(),
        );
    let mut request = http::get(endpoint).header(ACCEPT, "application/json");
    if let Some(token) = configured_token(RepoType::GitLab) {
        request = request.header("PRIVATE-TOKEN", token);
    }

    let response: GitLabProject =
        http::send_json(request, RequestOptions::new("Load GitLab project details"))
            .await
            .map_err(|error| error.to_string())?;
    value_to_id(&response.id, "GitLab")
}

pub(super) async fn get_pull_requests(
    locator: &RepoLocator,
    repo_project_id: &str,
    state: Option<PullRequestState>,
    page: u32,
    per_page: u32,
) -> Result<PullRequestPage, String> {
    let repo_project_id = repo_project_id.trim();
    if repo_project_id.is_empty() {
        return Err("GitLab project id is empty.".to_string());
    }
    let per_page = per_page.max(1);
    let endpoint = endpoints::MERGE_REQUESTS_URL
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace(
            "{project_id}",
            percent_encode_path(repo_project_id).as_str(),
        );
    let mut request = http::get(endpoint)
        .header(ACCEPT, "application/json")
        .query(&[
            ("per_page", &per_page.to_string()),
            ("page", &page.max(1).to_string()),
        ]);
    if let Some(state) = state {
        request = request.query(&[("state", state.gitlab_query())]);
    }
    if let Some(token) = configured_token(RepoType::GitLab) {
        request = request.header("PRIVATE-TOKEN", token);
    }

    let response = http::send(request, RequestOptions::new("Load GitLab merge requests"))
        .await
        .map_err(|error| error.to_string())?;
    let headers = response.headers().clone();
    let payload: Vec<Value> = response
        .json()
        .await
        .map_err(|error| format!("GitLab merge request response decode failed: {error}"))?;
    let items = payload
        .iter()
        .map(|value| PullRequestItem {
            number: string_field(value, "iid").unwrap_or_else(|| "—".to_string()),
            title: string_field(value, "title")
                .unwrap_or_else(|| "Untitled merge request".to_string()),
            author: nested_string(value, "author", "name")
                .or_else(|| nested_string(value, "author", "username"))
                .unwrap_or_else(|| "Unknown author".to_string()),
            source_branch: string_field(value, "source_branch").unwrap_or_else(|| "—".to_string()),
            target_branch: string_field(value, "target_branch").unwrap_or_else(|| "—".to_string()),
            updated_at: string_field(value, "updated_at")
                .unwrap_or_else(|| "Unknown update time".to_string()),
            web_url: string_field(value, "web_url").unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    let total = pagination_total(&headers);
    Ok(PullRequestPage {
        total: total.unwrap_or_else(|| fallback_total(page, per_page, items.len())),
        total_exact: total.is_some(),
        page_count: header_u32(&headers, "x-total-pages"),
        items,
    })
}

pub(super) async fn get_pull_request_diff(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
    ignore_whitespace_change: bool,
) -> Result<PullRequestDiffDetail, String> {
    let project_id = percent_encode_path(repo_project_id.trim());
    let merge_request_iid = percent_encode_path(pull_request_iid.trim());
    let detail_endpoint = endpoints::MERGE_REQUEST_URL
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace("{project_id}", project_id.as_str())
        .replace("{merge_request_iid}", merge_request_iid.as_str());
    let mut detail_request = http::get(detail_endpoint).header(ACCEPT, "application/json");
    if let Some(token) = configured_token(RepoType::GitLab) {
        detail_request = detail_request.header("PRIVATE-TOKEN", token);
    }
    let detail: Value = http::send_json(
        detail_request,
        RequestOptions::new("Load GitLab merge request details"),
    )
    .await
    .map_err(|error| error.to_string())?;

    let endpoint = endpoints::MERGE_REQUEST_DIFFS_URL
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace("{project_id}", project_id.as_str())
        .replace("{merge_request_iid}", merge_request_iid.as_str());
    let mut page = 1u32;
    let mut values = Vec::new();
    loop {
        let mut request = http::get(endpoint.as_str())
            .header(ACCEPT, "application/json")
            .query(&[("page", page.to_string()), ("per_page", "100".to_string())]);
        if ignore_whitespace_change {
            // Standard GitLab does not currently expose a whitespace query on
            // this endpoint. Keep the option provider-neutral without sending
            // an unsupported parameter.
            log::debug!(
                "[CodeReview][GitLab] Ignore-whitespace is not supported by the diff endpoint"
            );
        }
        if let Some(token) = configured_token(RepoType::GitLab) {
            request = request.header("PRIVATE-TOKEN", token);
        }
        let batch: Vec<Value> = http::send_json(
            request,
            RequestOptions::new("Load GitLab merge request diffs"),
        )
        .await
        .map_err(|error| error.to_string())?;
        let batch_len = batch.len();
        values.extend(batch);
        if batch_len < 100 {
            break;
        }
        page = page.saturating_add(1);
        if page > 30 {
            break;
        }
    }

    let files = values.iter().map(decode_diff_file).collect::<Vec<_>>();
    let refs = detail.get("diff_refs").unwrap_or(&Value::Null);
    Ok(PullRequestDiffDetail {
        added_lines: files.iter().map(|file| file.added_lines).sum(),
        removed_lines: files.iter().map(|file| file.removed_lines).sum(),
        base_sha: string_field(refs, "base_sha").unwrap_or_default(),
        head_sha: string_field(refs, "head_sha").unwrap_or_default(),
        start_sha: string_field(refs, "start_sha").unwrap_or_default(),
        too_many_commits: values.iter().any(|value| {
            value
                .get("too_large")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || value
                    .get("collapsed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        }),
        files,
    })
}

pub(super) async fn get_pull_request_action_state(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
) -> Result<PullRequestActionState, String> {
    let endpoint = endpoints::MERGE_REQUEST_URL
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace(
            "{project_id}",
            percent_encode_path(repo_project_id.trim()).as_str(),
        )
        .replace(
            "{merge_request_iid}",
            percent_encode_path(pull_request_iid.trim()).as_str(),
        );
    let mut request = http::get(endpoint)
        .header(ACCEPT, "application/json")
        .query(&[("with_merge_status_recheck", "true")]);
    if let Some(token) = configured_token(RepoType::GitLab) {
        request = request.header("PRIVATE-TOKEN", token);
    }
    let detail: Value = http::send_json(
        request,
        RequestOptions::new("Load GitLab merge request readiness"),
    )
    .await
    .map_err(|error| error.to_string())?;
    let readiness = gitlab_merge_readiness(&detail);
    let review_state = match string_field(&detail, "detailed_merge_status").as_deref() {
        Some("requested_changes") => "rejected",
        Some("not_approved") | Some("approvals_syncing") => "pending",
        _ => "",
    }
    .to_string();
    let merge_block_reason = if readiness.kind == PullRequestReadinessKind::Ready {
        "GitLab reports this merge request as ready, but remote review and merge actions are not implemented in Usher yet."
            .to_string()
    } else {
        readiness.detail.clone()
    };
    Ok(PullRequestActionState {
        can_comment: false,
        can_approve: false,
        can_request_changes: false,
        can_merge: false,
        review_state,
        readiness,
        merge_block_reason,
        allowed_merge_methods: Vec::new(),
    })
}

fn gitlab_merge_readiness(detail: &Value) -> PullRequestReadiness {
    let state = string_field(detail, "state")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if state == "merged" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Merged,
            "GitLab reports that this merge request has been merged.",
        );
    }
    if state == "closed" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Closed,
            "GitLab reports that this merge request is closed.",
        );
    }
    if detail.get("draft").and_then(Value::as_bool) == Some(true)
        || detail.get("work_in_progress").and_then(Value::as_bool) == Some(true)
    {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Draft,
            "Mark the GitLab merge request as ready before merging.",
        );
    }
    if detail.get("has_conflicts").and_then(Value::as_bool) == Some(true) {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Conflicts,
            "GitLab reports merge conflicts between the source and target branches.",
        );
    }

    let status = string_field(detail, "detailed_merge_status")
        .unwrap_or_else(|| string_field(detail, "merge_status").unwrap_or_default());
    match status.as_str() {
        "mergeable" | "can_be_merged" => PullRequestReadiness::new(
            PullRequestReadinessKind::Ready,
            "GitLab reports that all current merge requirements are satisfied.",
        ),
        "unchecked" | "checking" | "preparing" | "approvals_syncing" => PullRequestReadiness::new(
            PullRequestReadinessKind::Checking,
            "GitLab is calculating the mergeability of this merge request.",
        ),
        "conflict" | "cannot_be_merged" => PullRequestReadiness::new(
            PullRequestReadinessKind::Conflicts,
            "GitLab reports merge conflicts between the source and target branches.",
        ),
        "not_approved" => PullRequestReadiness::new(
            PullRequestReadinessKind::ReviewRequired,
            "Required GitLab approvals have not been completed.",
        ),
        "requested_changes" => PullRequestReadiness::new(
            PullRequestReadinessKind::ChangesRequested,
            "A GitLab reviewer requested changes.",
        ),
        "discussions_not_resolved" => PullRequestReadiness::new(
            PullRequestReadinessKind::DiscussionsUnresolved,
            "Resolve all required GitLab discussions before merging.",
        ),
        "draft_status" => PullRequestReadiness::new(
            PullRequestReadinessKind::Draft,
            "Mark the GitLab merge request as ready before merging.",
        ),
        "need_rebase" => PullRequestReadiness::new(
            PullRequestReadinessKind::BranchOutOfDate,
            "GitLab requires the source branch to be rebased before merging.",
        ),
        "ci_still_running" => PullRequestReadiness::new(
            PullRequestReadinessKind::ChecksPending,
            "The required GitLab CI pipeline is still running.",
        ),
        "ci_must_pass"
        | "commits_status"
        | "security_policy_pipeline_check"
        | "security_policy_violations"
        | "status_checks_must_pass" => PullRequestReadiness::new(
            PullRequestReadinessKind::ChecksFailed,
            "A required GitLab pipeline, status check, or security policy has not passed.",
        ),
        "not_open" => PullRequestReadiness::new(
            PullRequestReadinessKind::Closed,
            "GitLab reports that this merge request is not open.",
        ),
        "merge_request_blocked"
        | "merge_time"
        | "jira_association_missing"
        | "locked_paths"
        | "locked_lfs_files"
        | "title_regex" => PullRequestReadiness::new(
            PullRequestReadinessKind::Blocked,
            "A GitLab repository rule or merge condition is blocking this merge request.",
        ),
        _ => PullRequestReadiness::new(
            PullRequestReadinessKind::Unavailable,
            "GitLab did not return enough information to determine merge readiness.",
        ),
    }
}

fn decode_diff_file(value: &Value) -> PullRequestDiffFile {
    let path = string_field(value, "new_path").unwrap_or_else(|| "Unknown file".to_string());
    let old_path = string_field(value, "old_path").unwrap_or_else(|| path.clone());
    let change_type = if value
        .get("new_file")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "new_file"
    } else if value
        .get("deleted_file")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "deleted_file"
    } else if value
        .get("renamed_file")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "renamed_file"
    } else {
        "edit_file"
    }
    .to_string();
    let lines = string_field(value, "diff")
        .map(|diff| parse_unified_diff(diff.as_str()))
        .unwrap_or_default();
    let added_lines = lines.iter().filter(|line| line.kind == "addition").count() as u32;
    let removed_lines = lines.iter().filter(|line| line.kind == "deletion").count() as u32;
    if lines.is_empty() {
        return unavailable_diff_file(
            path,
            old_path,
            change_type,
            added_lines,
            removed_lines,
            "GitLab omitted this binary, collapsed, or oversized diff.",
        );
    }
    PullRequestDiffFile {
        path,
        old_path,
        change_type,
        added_lines,
        removed_lines,
        viewed: false,
        lines,
    }
}

pub(super) async fn get_repository_file_text(
    locator: &RepoLocator,
    repo_project_id: &str,
    path: &str,
    reference: &str,
) -> Result<String, String> {
    let endpoint = endpoints::REPOSITORY_FILE_RAW_URL
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace(
            "{project_id}",
            percent_encode_path(repo_project_id.trim()).as_str(),
        )
        .replace("{file_path}", percent_encode_path(path).as_str());
    let mut request = http::get(endpoint)
        .header(ACCEPT, "text/plain")
        .query(&[("ref", reference)]);
    if let Some(token) = configured_token(RepoType::GitLab) {
        request = request.header("PRIVATE-TOKEN", token);
    }
    let response = http::send(request, RequestOptions::new("Load GitLab repository file"))
        .await
        .map_err(|error| error.to_string())?;
    response
        .text()
        .await
        .map_err(|error| format!("GitLab repository file decode failed: {error}"))
}

fn api_base_url(locator: &RepoLocator) -> String {
    if is_configured_enterprise_host(RepoType::GitLab, locator.host.as_str()) {
        if let Some(api_base_url) = configured_api_base_url(RepoType::GitLab) {
            return with_default_api_version(api_base_url, endpoints::API_VERSION_PATH);
        }
    }
    format!(
        "{}://{}{}",
        locator.scheme,
        locator.authority,
        endpoints::API_VERSION_PATH
    )
}

fn with_default_api_version(api_base_url: String, required_path: &str) -> String {
    let has_required_path = reqwest::Url::parse(api_base_url.as_str())
        .ok()
        .map(|url| url.path().trim_end_matches('/').ends_with(required_path))
        .unwrap_or(false);
    if has_required_path {
        api_base_url
    } else {
        format!("{api_base_url}{required_path}")
    }
}

fn pagination_total(headers: &HeaderMap) -> Option<u32> {
    ["x-total", "x-total-count", "total-count"]
        .into_iter()
        .find_map(|header| header_u32(headers, header))
}

fn header_u32(headers: &HeaderMap, name: &str) -> Option<u32> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u32>().ok())
}

fn value_to_id(value: &serde_json::Value, provider: &str) -> Result<String, String> {
    match value {
        serde_json::Value::String(value) if !value.trim().is_empty() => Ok(value.clone()),
        serde_json::Value::Number(value) => Ok(value.to_string()),
        _ => Err(format!("{provider} project details did not include an id.")),
    }
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            value
                .get(key)
                .and_then(Value::as_i64)
                .map(|value| value.to_string())
        })
}

fn nested_string(value: &Value, parent: &str, key: &str) -> Option<String> {
    value.get(parent)?.get(key)?.as_str().map(str::to_string)
}

fn fallback_total(page: u32, per_page: u32, item_count: usize) -> u32 {
    page.saturating_sub(1)
        .saturating_mul(per_page)
        .saturating_add(item_count as u32)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::gitlab_merge_readiness;
    use crate::git::PullRequestReadinessKind;

    #[test]
    fn maps_gitlab_mergeable_status_to_ready() {
        let readiness = gitlab_merge_readiness(&json!({
            "state": "opened",
            "detailed_merge_status": "mergeable",
            "has_conflicts": false
        }));

        assert_eq!(readiness.kind, PullRequestReadinessKind::Ready);
    }

    #[test]
    fn maps_gitlab_async_calculation_to_checking() {
        let readiness = gitlab_merge_readiness(&json!({
            "state": "opened",
            "detailed_merge_status": "checking"
        }));

        assert_eq!(readiness.kind, PullRequestReadinessKind::Checking);
    }

    #[test]
    fn maps_gitlab_conflict_independently_of_other_providers() {
        let readiness = gitlab_merge_readiness(&json!({
            "state": "opened",
            "detailed_merge_status": "conflict",
            "has_conflicts": true
        }));

        assert_eq!(readiness.kind, PullRequestReadinessKind::Conflicts);
    }

    #[test]
    fn maps_gitlab_missing_approval_to_review_required() {
        let readiness = gitlab_merge_readiness(&json!({
            "state": "opened",
            "detailed_merge_status": "not_approved"
        }));

        assert_eq!(readiness.kind, PullRequestReadinessKind::ReviewRequired);
    }
}
