//! GitHub repository operations.

use base64::Engine as _;
use futures_util::future::join_all;
use reqwest::header::{ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::Deserialize;
use serde_json::Value;

use crate::codereview::api::github as endpoints;
use crate::http::{self, RequestOptions};

use super::{
    PullRequestActionState, PullRequestDiffDetail, PullRequestDiffFile, PullRequestItem,
    PullRequestMergeMethod, PullRequestPage, PullRequestReadiness, PullRequestReadinessKind,
    PullRequestReviewAction, PullRequestState, RepoLocator, RepoType, configured_api_base_url,
    configured_token, is_configured_enterprise_host, parse_unified_diff, percent_encode_path,
    unavailable_diff_file,
};

#[derive(Deserialize)]
struct GitHubRepository {
    id: serde_json::Value,
}

pub(super) async fn get_repo_detail(locator: &RepoLocator) -> Result<String, String> {
    let endpoint = endpoints::REPOSITORY_URL
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace(
            "{namespace}",
            percent_encode_path(locator.namespace.as_str()).as_str(),
        )
        .replace(
            "{repository}",
            percent_encode_path(locator.repo.as_str()).as_str(),
        );
    let mut request = http::get(endpoint)
        .header(ACCEPT, "application/vnd.github+json")
        .header(USER_AGENT, "usher-project-lookup");
    if let Some(token) = configured_token(RepoType::GitHub) {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }

    let response: GitHubRepository =
        http::send_json(request, RequestOptions::new("Load GitHub project details"))
            .await
            .map_err(|error| error.to_string())?;
    value_to_id(&response.id, "GitHub")
}

pub(super) async fn get_pull_requests(
    locator: &RepoLocator,
    state: Option<PullRequestState>,
    page: u32,
    per_page: u32,
) -> Result<PullRequestPage, String> {
    let per_page = per_page.max(1);
    let mut query = format!("repo:{}/{} is:pr", locator.namespace, locator.repo);
    if let Some(state) = state {
        let state_query = state.github_query();
        if !state_query.is_empty() {
            query.push(' ');
            query.push_str(state_query);
        }
    }
    let endpoint = endpoints::SEARCH_PULL_REQUESTS_URL
        .replace("{api_base_url}", api_base_url(locator).as_str());
    let mut request = http::get(endpoint)
        .header(ACCEPT, "application/vnd.github+json")
        .header(USER_AGENT, "usher-project-detail")
        .query(&[
            ("q", query.as_str()),
            ("per_page", &per_page.to_string()),
            ("page", &page.max(1).to_string()),
        ]);
    if let Some(token) = configured_token(RepoType::GitHub) {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }

    let payload: Value = http::send_json(request, RequestOptions::new("Load GitHub pull requests"))
        .await
        .map_err(|error| error.to_string())?;
    let total = payload
        .get("total_count")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let items = payload
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| "GitHub pull request response is not a list.".to_string())?
        .iter()
        .map(|value| PullRequestItem {
            number: string_field(value, "number").unwrap_or_else(|| "—".to_string()),
            title: string_field(value, "title")
                .unwrap_or_else(|| "Untitled pull request".to_string()),
            author: nested_string(value, "user", "login")
                .unwrap_or_else(|| "Unknown author".to_string()),
            source_branch: "—".to_string(),
            target_branch: "—".to_string(),
            updated_at: string_field(value, "updated_at")
                .unwrap_or_else(|| "Unknown update time".to_string()),
            web_url: string_field(value, "html_url").unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    let items = hydrate_pull_request_branches(locator, items).await;
    Ok(PullRequestPage {
        items,
        total,
        total_exact: true,
        page_count: Some(total.div_ceil(per_page)),
    })
}

/// GitHub's issue-search response provides the exact PR count, but not the
/// head/base refs. Fetch details for the visible rows concurrently so the list
/// keeps its accurate paginator while still showing the branch pair.
async fn hydrate_pull_request_branches(
    locator: &RepoLocator,
    mut items: Vec<PullRequestItem>,
) -> Vec<PullRequestItem> {
    let requests = items.iter().enumerate().map(|(index, item)| async move {
        let branches = load_pull_request_branches(locator, item.number.as_str()).await;
        (index, branches)
    });

    for (index, branches) in join_all(requests).await {
        if let Some((source_branch, target_branch)) = branches {
            items[index].source_branch = source_branch;
            items[index].target_branch = target_branch;
        }
    }
    items
}

async fn load_pull_request_branches(
    locator: &RepoLocator,
    pull_request_number: &str,
) -> Option<(String, String)> {
    let endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let detail: Value = http::send_json(
        authenticated_request(locator, endpoint),
        RequestOptions::new("Load GitHub pull request branches"),
    )
    .await
    .ok()?;
    pull_request_branches(&detail)
}

pub(super) async fn get_pull_request_diff(
    locator: &RepoLocator,
    pull_request_number: &str,
    ignore_whitespace_change: bool,
) -> Result<PullRequestDiffDetail, String> {
    let detail_endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let detail: Value = http::send_json(
        authenticated_request(locator, detail_endpoint),
        RequestOptions::new("Load GitHub pull request details"),
    )
    .await
    .map_err(|error| error.to_string())?;

    if ignore_whitespace_change {
        log::debug!(
            "[CodeReview][GitHub] Ignore-whitespace is not supported by the pull files endpoint"
        );
    }
    let files_endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_FILES_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let mut page = 1u32;
    let mut values = Vec::new();
    loop {
        let request = authenticated_request(locator, files_endpoint.clone())
            .query(&[("page", page.to_string()), ("per_page", "100".to_string())]);
        let batch: Vec<Value> = http::send_json(
            request,
            RequestOptions::new("Load GitHub pull request files"),
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
    Ok(PullRequestDiffDetail {
        added_lines: files.iter().map(|file| file.added_lines).sum(),
        removed_lines: files.iter().map(|file| file.removed_lines).sum(),
        base_sha: nested_string(&detail, "base", "sha").unwrap_or_default(),
        head_sha: nested_string(&detail, "head", "sha").unwrap_or_default(),
        start_sha: nested_string(&detail, "base", "sha").unwrap_or_default(),
        too_many_commits: values.len() >= 3000,
        files,
    })
}

fn decode_diff_file(value: &Value) -> PullRequestDiffFile {
    let path = string_field(value, "filename").unwrap_or_else(|| "Unknown file".to_string());
    let old_path = string_field(value, "previous_filename").unwrap_or_else(|| path.clone());
    let change_type = match string_field(value, "status").as_deref() {
        Some("added") => "new_file",
        Some("removed") => "deleted_file",
        Some("renamed") => "renamed_file",
        _ => "edit_file",
    }
    .to_string();
    let added_lines = value_u32(value, "additions").unwrap_or(0);
    let removed_lines = value_u32(value, "deletions").unwrap_or(0);
    let lines = string_field(value, "patch")
        .map(|patch| parse_unified_diff(patch.as_str()))
        .unwrap_or_default();
    if lines.is_empty() {
        return unavailable_diff_file(
            path,
            old_path,
            change_type,
            added_lines,
            removed_lines,
            "GitHub omitted this binary or oversized patch.",
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
    path: &str,
    reference: &str,
) -> Result<String, String> {
    let endpoint =
        repository_endpoint(endpoints::REPOSITORY_CONTENT_URL, locator, None, Some(path));
    let payload: Value = http::send_json(
        authenticated_request(locator, endpoint).query(&[("ref", reference)]),
        RequestOptions::new("Load GitHub repository file"),
    )
    .await
    .map_err(|error| error.to_string())?;
    decode_base64_content(&payload, "GitHub")
}

pub(super) async fn get_pull_request_action_state(
    locator: &RepoLocator,
    pull_request_number: &str,
) -> Result<PullRequestActionState, String> {
    let detail_endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let detail: Value = http::send_json(
        authenticated_request(locator, detail_endpoint),
        RequestOptions::new("Load GitHub pull request merge state"),
    )
    .await
    .map_err(|error| error.to_string())?;

    let repository_url = repository_endpoint(endpoints::REPOSITORY_URL, locator, None, None);
    let repository: Value = http::send_json(
        authenticated_request(locator, repository_url),
        RequestOptions::new("Load GitHub repository merge settings"),
    )
    .await
    .map_err(|error| error.to_string())?;

    let reviews_endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_REVIEWS_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let reviews = http::send_json::<Vec<Value>>(
        authenticated_request(locator, reviews_endpoint).query(&[("per_page", "100")]),
        RequestOptions::new("Load GitHub pull request reviews"),
    )
    .await
    .unwrap_or_default();

    let has_token = configured_token(RepoType::GitHub).is_some();
    let review_state = aggregate_review_state(reviews.as_slice());
    let readiness = github_merge_readiness(&detail, review_state.as_str());
    let can_merge = has_token && readiness.kind == PullRequestReadinessKind::Ready;
    let merge_block_reason = if !has_token {
        "Configure a GitHub Token in Settings to review or merge this pull request.".to_string()
    } else if readiness.kind != PullRequestReadinessKind::Ready {
        readiness.detail.clone()
    } else {
        String::new()
    };

    let mut allowed_merge_methods = Vec::new();
    if repository
        .get("allow_merge_commit")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        allowed_merge_methods.push(PullRequestMergeMethod::Merge);
    }
    if repository
        .get("allow_squash_merge")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        allowed_merge_methods.push(PullRequestMergeMethod::Squash);
    }
    if repository
        .get("allow_rebase_merge")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        allowed_merge_methods.push(PullRequestMergeMethod::Rebase);
    }

    Ok(PullRequestActionState {
        can_comment: has_token,
        can_approve: has_token,
        can_request_changes: has_token,
        can_merge,
        review_state,
        readiness,
        merge_block_reason,
        allowed_merge_methods,
    })
}

pub(super) async fn submit_pull_request_review(
    locator: &RepoLocator,
    pull_request_number: &str,
    action: PullRequestReviewAction,
    body: &str,
) -> Result<(), String> {
    if configured_token(RepoType::GitHub).is_none() {
        return Err("Configure a GitHub Token in Settings before submitting a review.".into());
    }
    if action != PullRequestReviewAction::Approve && body.trim().is_empty() {
        return Err("A review comment is required for this action.".into());
    }
    let endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_REVIEWS_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let event = match action {
        PullRequestReviewAction::Comment => "COMMENT",
        PullRequestReviewAction::Approve => "APPROVE",
        PullRequestReviewAction::RequestChanges => "REQUEST_CHANGES",
    };
    let request = authenticated_builder(
        http::post(endpoint),
        configured_token(RepoType::GitHub).as_deref(),
    )
    .json(&serde_json::json!({ "body": body.trim(), "event": event }));
    let _: Value = http::send_json(
        request,
        RequestOptions::new("Submit GitHub pull request review"),
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) async fn merge_pull_request(
    locator: &RepoLocator,
    pull_request_number: &str,
    method: PullRequestMergeMethod,
) -> Result<(), String> {
    if configured_token(RepoType::GitHub).is_none() {
        return Err("Configure a GitHub Token in Settings before merging.".into());
    }
    if method == PullRequestMergeMethod::FastForward {
        return Err("GitHub does not expose fast-forward as a pull request merge method.".into());
    }
    let endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_MERGE_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let request = authenticated_builder(
        http::put(endpoint),
        configured_token(RepoType::GitHub).as_deref(),
    )
    .json(&serde_json::json!({ "merge_method": method.as_str() }));
    let payload: Value = http::send_json(request, RequestOptions::new("Merge GitHub pull request"))
        .await
        .map_err(|error| error.to_string())?;
    if payload.get("merged").and_then(Value::as_bool) == Some(false) {
        return Err(string_field(&payload, "message")
            .unwrap_or_else(|| "GitHub did not merge this pull request.".to_string()));
    }
    Ok(())
}

fn authenticated_request(_locator: &RepoLocator, endpoint: String) -> reqwest::RequestBuilder {
    authenticated_builder(
        http::get(endpoint),
        configured_token(RepoType::GitHub).as_deref(),
    )
}

fn authenticated_builder(
    request: reqwest::RequestBuilder,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut request = request
        .header(ACCEPT, "application/vnd.github+json")
        .header(USER_AGENT, "usher-code-review")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = token {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    request
}

fn repository_endpoint(
    template: &str,
    locator: &RepoLocator,
    pull_request_number: Option<&str>,
    file_path: Option<&str>,
) -> String {
    let mut endpoint = template
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace(
            "{namespace}",
            percent_encode_path(locator.namespace.as_str()).as_str(),
        )
        .replace(
            "{repository}",
            percent_encode_path(locator.repo.as_str()).as_str(),
        );
    if let Some(number) = pull_request_number {
        endpoint = endpoint.replace(
            "{pull_request_number}",
            percent_encode_path(number.trim()).as_str(),
        );
    }
    if let Some(path) = file_path {
        endpoint = endpoint.replace("{file_path}", percent_encode_path(path).as_str());
    }
    endpoint
}

fn decode_base64_content(payload: &Value, provider: &str) -> Result<String, String> {
    let content = payload
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{provider} repository file response has no content."))?
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content.as_bytes())
        .map_err(|error| format!("{provider} repository file base64 decode failed: {error}"))?;
    String::from_utf8(bytes)
        .map_err(|error| format!("{provider} repository file is not UTF-8 text: {error}"))
}

fn value_u32(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn api_base_url(locator: &RepoLocator) -> String {
    if is_configured_enterprise_host(RepoType::GitHub, locator.host.as_str()) {
        if let Some(api_base_url) = configured_api_base_url(RepoType::GitHub) {
            return with_default_api_version(api_base_url, endpoints::ENTERPRISE_API_VERSION_PATH);
        }
    }
    endpoints::PUBLIC_API_BASE_URL.to_string()
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

fn pull_request_branches(value: &Value) -> Option<(String, String)> {
    let source_branch = nested_string(value, "head", "ref")?;
    let target_branch = nested_string(value, "base", "ref")?;
    (!source_branch.trim().is_empty() && !target_branch.trim().is_empty())
        .then_some((source_branch, target_branch))
}

fn aggregate_review_state(reviews: &[Value]) -> String {
    let mut latest_by_user = std::collections::BTreeMap::new();
    for review in reviews {
        let Some(user) = nested_string(review, "user", "login") else {
            continue;
        };
        let state = string_field(review, "state")
            .unwrap_or_default()
            .to_ascii_uppercase();
        if matches!(
            state.as_str(),
            "APPROVED" | "CHANGES_REQUESTED" | "DISMISSED"
        ) {
            latest_by_user.insert(user, state);
        }
    }
    if latest_by_user
        .values()
        .any(|state| state == "CHANGES_REQUESTED")
    {
        "rejected".to_string()
    } else if latest_by_user.values().any(|state| state == "APPROVED") {
        "approved".to_string()
    } else {
        String::new()
    }
}

/// GitHub owns this mapping. Do not reuse it for Gitee or GitLab: GitHub's
/// `mergeable` and `mergeable_state` are provider-specific and `null` means
/// that GitHub is still computing a synthetic test merge.
fn github_merge_readiness(detail: &Value, review_state: &str) -> PullRequestReadiness {
    let state = string_field(detail, "state").unwrap_or_default();
    let merged = detail
        .get("merged")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || detail
            .get("merged_at")
            .is_some_and(|value| !value.is_null());
    if merged {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Merged,
            "GitHub reports that this pull request has been merged.",
        );
    }
    if state == "closed" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Closed,
            "GitHub reports that this pull request was closed without merging.",
        );
    }
    if state != "open" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Unavailable,
            "GitHub did not return a recognized open, closed, or merged state.",
        );
    }
    if detail
        .get("draft")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Draft,
            "GitHub draft pull requests cannot be merged until they are marked ready for review.",
        );
    }

    let mergeable = detail.get("mergeable").and_then(Value::as_bool);
    let mergeable_state = string_field(detail, "mergeable_state")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if mergeable.is_none() || mergeable_state == "unknown" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Checking,
            "GitHub is calculating a test merge for this pull request.",
        );
    }
    if mergeable == Some(false) || mergeable_state == "dirty" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Conflicts,
            "GitHub found conflicts between the head and base branches.",
        );
    }
    if review_state == "rejected" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::ChangesRequested,
            "A GitHub review is requesting changes before this pull request can be merged.",
        );
    }

    match mergeable_state.as_str() {
        "blocked" => PullRequestReadiness::new(
            PullRequestReadinessKind::Blocked,
            "GitHub branch protection, required reviews, checks, or another repository rule is blocking this merge.",
        ),
        "behind" => PullRequestReadiness::new(
            PullRequestReadinessKind::BranchOutOfDate,
            "GitHub requires the head branch to be updated with the base branch before merging.",
        ),
        "unstable" => PullRequestReadiness::new(
            PullRequestReadinessKind::ChecksPending,
            "GitHub reports non-passing commit status checks. They may still be pending or non-required.",
        ),
        "clean" | "has_hooks" | "" if mergeable == Some(true) => PullRequestReadiness::new(
            PullRequestReadinessKind::Ready,
            "GitHub reports a clean merge with no unmet merge requirement in the pull request state.",
        ),
        _ if mergeable == Some(true) => PullRequestReadiness::new(
            PullRequestReadinessKind::Ready,
            format!(
                "GitHub reports this pull request as mergeable (provider state: {}).",
                if mergeable_state.is_empty() {
                    "mergeable"
                } else {
                    mergeable_state.as_str()
                }
            ),
        ),
        _ => PullRequestReadiness::new(
            PullRequestReadinessKind::Unavailable,
            "GitHub did not return enough information to determine merge readiness.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{aggregate_review_state, github_merge_readiness, pull_request_branches};
    use crate::git::PullRequestReadinessKind;

    #[test]
    fn reads_head_and_base_refs_from_a_pull_request_detail() {
        let payload = json!({
            "head": { "ref": "feature/pr-branches" },
            "base": { "ref": "main" }
        });

        assert_eq!(
            pull_request_branches(&payload),
            Some(("feature/pr-branches".to_string(), "main".to_string()))
        );
    }

    #[test]
    fn rejects_missing_or_empty_branch_refs() {
        assert_eq!(
            pull_request_branches(&json!({ "head": { "ref": "feature" } })),
            None
        );
        assert_eq!(
            pull_request_branches(&json!({
                "head": { "ref": "" },
                "base": { "ref": "main" }
            })),
            None
        );
    }

    #[test]
    fn aggregates_latest_github_review_state_per_user() {
        let reviews = vec![
            json!({ "user": { "login": "alice" }, "state": "APPROVED" }),
            json!({ "user": { "login": "bob" }, "state": "CHANGES_REQUESTED" }),
            json!({ "user": { "login": "bob" }, "state": "DISMISSED" }),
        ];
        assert_eq!(aggregate_review_state(&reviews), "approved");
    }

    #[test]
    fn github_null_mergeable_is_checking_not_conflicting() {
        let readiness = github_merge_readiness(
            &json!({
                "state": "open",
                "draft": false,
                "mergeable": null,
                "mergeable_state": "unknown"
            }),
            "",
        );
        assert_eq!(readiness.kind, PullRequestReadinessKind::Checking);
    }

    #[test]
    fn github_dirty_state_is_conflicting() {
        let readiness = github_merge_readiness(
            &json!({
                "state": "open",
                "draft": false,
                "mergeable": false,
                "mergeable_state": "dirty"
            }),
            "",
        );
        assert_eq!(readiness.kind, PullRequestReadinessKind::Conflicts);
    }

    #[test]
    fn github_clean_state_is_ready() {
        let readiness = github_merge_readiness(
            &json!({
                "state": "open",
                "draft": false,
                "mergeable": true,
                "mergeable_state": "clean"
            }),
            "approved",
        );
        assert_eq!(readiness.kind, PullRequestReadinessKind::Ready);
    }
}
