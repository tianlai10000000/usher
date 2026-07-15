//! Gitee-compatible project APIs.
//!
//! The internal endpoint documented here is GitLab-shaped, but is used by the
//! configured Gitee-compatible enterprise service:
//!
//! - `GET ${api_base_url}/projects?search=${project_name}`
//! - `GET ${api_base_url}/projects/${project_id}/merge_requests?...`
//!
//! `project_name` is always the repository name extracted from `remote_repo`
//! (the final path segment before `.git`), never the editable project alias.

use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap};
use serde_json::Value;

use crate::http::{self, RequestOptions};

use super::{
    PullRequestItem, PullRequestPage, PullRequestState, RepoLocator, RepoType,
    configured_api_base_url, configured_enterprise, configured_token,
    is_configured_enterprise_host, percent_encode_path,
};

/// Fetch the stable provider project id for a remote repository.
pub(super) async fn get_repo_detail(locator: &RepoLocator) -> Result<String, String> {
    if is_configured_enterprise_host(RepoType::Gitee, locator.host.as_str()) {
        return get_enterprise_repo_detail(locator).await;
    }
    get_public_gitee_repo_detail(locator).await
}

async fn get_enterprise_repo_detail(locator: &RepoLocator) -> Result<String, String> {
    let api_base_url = enterprise_api_base_url()?;
    let endpoint = format!("{api_base_url}/projects");
    let mut request = http::get(endpoint)
        .header("OAUTH", "enabled")
        .header(ACCEPT, "application/json")
        // The search term comes from remote_repo's final path component.
        .query(&[("search", locator.repo.as_str())]);
    if let Some(enterprise) = configured_enterprise(RepoType::Gitee) {
        request = request.header("enterprise", enterprise);
    }
    if let Some(token) = configured_token(RepoType::Gitee) {
        request = request.header("PRIVATE-TOKEN", token);
    }

    let payload: Value =
        http::send_json(request, RequestOptions::new("Load Gitee project details"))
            .await
            .map_err(|error| error.to_string())?;
    select_enterprise_project_id(&payload, locator)
}

async fn get_public_gitee_repo_detail(locator: &RepoLocator) -> Result<String, String> {
    let endpoint = format!(
        "{}://{}/api/v5/repos/{}/{}",
        locator.scheme,
        locator.authority,
        percent_encode_path(locator.namespace.as_str()),
        percent_encode_path(locator.repo.as_str())
    );
    let mut request = http::get(endpoint).header(ACCEPT, "application/json");
    if let Some(token) = configured_token(RepoType::Gitee) {
        request = request.header("Authorization", format!("token {token}"));
    }

    let payload: Value =
        http::send_json(request, RequestOptions::new("Load Gitee project details"))
            .await
            .map_err(|error| error.to_string())?;
    value_to_id(payload.get("id"), "Gitee")
}

fn select_enterprise_project_id(payload: &Value, locator: &RepoLocator) -> Result<String, String> {
    let projects = payload
        .as_array()
        .or_else(|| payload.get("data").and_then(Value::as_array))
        .ok_or_else(|| "Gitee project search returned an unexpected response.".to_string())?;

    let expected_path = format!("{}/{}", locator.namespace, locator.repo);
    let project = projects
        .iter()
        .find(|project| project_exactly_matches_remote(project, expected_path.as_str(), locator))
        .or_else(|| (projects.len() == 1).then(|| &projects[0]))
        .ok_or_else(|| {
            format!(
                "Gitee project search did not find an exact match for '{}'.",
                locator.remote_repo
            )
        })?;
    value_to_id(project.get("id"), "Gitee")
}

/// Query one page of pull requests. The internal service follows the
/// documented `/api/v8/projects/${project_id}/merge_requests` contract.
pub(super) async fn get_pull_requests(
    locator: &RepoLocator,
    repo_project_id: &str,
    state: Option<PullRequestState>,
    page: u32,
    per_page: u32,
) -> Result<PullRequestPage, String> {
    if is_configured_enterprise_host(RepoType::Gitee, locator.host.as_str()) {
        log::debug!(
            "[CodeReview][Gitee] Load merge requests: source=enterprise remote_host={} api_base_configured=true enterprise_configured={} token_configured={} state={} page={} per_page={}",
            locator.host,
            configured_enterprise(RepoType::Gitee).is_some(),
            configured_token(RepoType::Gitee).is_some(),
            state
                .map(PullRequestState::as_str)
                .unwrap_or("provider-default"),
            page.max(1),
            per_page.max(1),
        );
        return get_enterprise_pull_requests(repo_project_id, state, page, per_page).await;
    }
    log::debug!(
        "[CodeReview][Gitee] Load merge requests: source=public remote_host={} api_base_configured={} state={} page={} per_page={}",
        locator.host,
        configured_api_base_url(RepoType::Gitee).is_some(),
        state
            .map(PullRequestState::as_str)
            .unwrap_or("provider-default"),
        page.max(1),
        per_page.max(1),
    );
    get_public_pull_requests(locator, state, page, per_page).await
}

async fn get_enterprise_pull_requests(
    repo_project_id: &str,
    state: Option<PullRequestState>,
    page: u32,
    per_page: u32,
) -> Result<PullRequestPage, String> {
    let repo_project_id = repo_project_id.trim();
    if repo_project_id.is_empty() {
        return Err("Gitee project id is empty.".to_string());
    }
    let api_base_url = enterprise_api_base_url()?;
    let endpoint = format!(
        "{api_base_url}/projects/{}/merge_requests",
        percent_encode_path(repo_project_id)
    );
    let per_page = per_page.max(1);
    let mut request = http::get(endpoint)
        .header("OAUTH", "enabled")
        .header(ACCEPT, "application/json")
        .query(&[
            ("per_page", &per_page.to_string()),
            ("page", &page.max(1).to_string()),
        ]);
    if let Some(enterprise) = configured_enterprise(RepoType::Gitee) {
        request = request.header("enterprise", enterprise);
    }
    if let Some(state) = state {
        request = request.query(&[("state", state.gitee_query())]);
    }
    if let Some(token) = configured_token(RepoType::Gitee) {
        request = request.header("PRIVATE-TOKEN", token);
    }
    decode_pull_request_page(request, page, per_page).await
}

fn enterprise_api_base_url() -> Result<String, String> {
    let configured = configured_api_base_url(RepoType::Gitee).ok_or_else(|| {
        "Configure the Gitee API Base URL in Settings before using this enterprise repository."
            .to_string()
    })?;
    let parsed = reqwest::Url::parse(configured.as_str())
        .map_err(|_| "The configured Gitee API Base URL is invalid.".to_string())?;
    if parsed.path().trim_end_matches('/').ends_with("/api/v8") {
        Ok(configured)
    } else {
        Ok(format!("{configured}/api/v8"))
    }
}

async fn get_public_pull_requests(
    locator: &RepoLocator,
    state: Option<PullRequestState>,
    page: u32,
    per_page: u32,
) -> Result<PullRequestPage, String> {
    let per_page = per_page.max(1);
    let endpoint = format!(
        "{}://{}/api/v5/repos/{}/{}/pulls",
        locator.scheme,
        locator.authority,
        percent_encode_path(locator.namespace.as_str()),
        percent_encode_path(locator.repo.as_str())
    );
    let mut request = http::get(endpoint)
        .header(ACCEPT, "application/json")
        .query(&[
            ("per_page", &per_page.to_string()),
            ("page", &page.max(1).to_string()),
        ]);
    if let Some(state) = state {
        request = request.query(&[("state", state.gitee_public_query())]);
    }
    if let Some(token) = configured_token(RepoType::Gitee) {
        request = request.header("Authorization", format!("token {token}"));
    }
    decode_pull_request_page(request, page, per_page).await
}

async fn decode_pull_request_page(
    request: reqwest::RequestBuilder,
    page: u32,
    per_page: u32,
) -> Result<PullRequestPage, String> {
    let response = http::send(request, RequestOptions::new("Load Gitee merge requests"))
        .await
        .map_err(|error| error.to_string())?;
    let headers = response.headers().clone();
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing")
        .to_string();
    let body = response
        .text()
        .await
        .map_err(|error| format!("Gitee merge request response read failed: {error}"))?;
    let body_len = body.len();
    let payload: Value = serde_json::from_str(body.as_str()).map_err(|error| {
        log::warn!(
            "[CodeReview][Gitee] Merge-request response was not JSON: content_type={} body_bytes={} parse_error={}",
            content_type,
            body_len,
            error,
        );
        "Gitee merge request response was not valid JSON.".to_string()
    })?;
    let list = payload
        .as_array()
        .or_else(|| payload.get("data").and_then(Value::as_array))
        .ok_or_else(|| "Gitee merge request response is not a list.".to_string())?;
    let items = list.iter().map(to_pull_request_item).collect::<Vec<_>>();
    let total = pagination_total(&headers, &payload);
    Ok(PullRequestPage {
        total: total.unwrap_or_else(|| fallback_total(page, per_page, items.len())),
        total_exact: total.is_some(),
        page_count: pagination_page_count(&headers, &payload),
        items,
    })
}

/// Different Gitee-compatible deployments expose the total either as an HTTP
/// header or alongside the list payload. Prefer exact totals over page length.
fn pagination_total(headers: &HeaderMap, payload: &Value) -> Option<u32> {
    ["x-total", "x-total-count", "total-count"]
        .into_iter()
        .find_map(|header| header_u32(headers, header))
        .or_else(|| value_u32(payload, "total"))
        .or_else(|| value_u32(payload, "total_count"))
        .or_else(|| value_u32(payload, "count"))
        .or_else(|| {
            payload
                .get("meta")
                .and_then(|meta| value_u32(meta, "total"))
        })
        .or_else(|| {
            payload
                .get("meta")
                .and_then(|meta| value_u32(meta, "total_count"))
        })
        .or_else(|| {
            payload
                .get("pagination")
                .and_then(|pagination| value_u32(pagination, "total"))
        })
}

fn pagination_page_count(headers: &HeaderMap, payload: &Value) -> Option<u32> {
    ["x-total-pages", "x-page-count"]
        .into_iter()
        .find_map(|header| header_u32(headers, header))
        .or_else(|| value_u32(payload, "total_pages"))
        .or_else(|| value_u32(payload, "totalPages"))
        .or_else(|| value_u32(payload, "pages"))
        .or_else(|| {
            payload
                .get("meta")
                .and_then(|meta| value_u32(meta, "total_pages"))
        })
        .or_else(|| {
            payload
                .get("pagination")
                .and_then(|pagination| value_u32(pagination, "total_pages"))
        })
}

fn header_u32(headers: &HeaderMap, name: &str) -> Option<u32> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u32>().ok())
}

fn value_u32(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| {
            value
                .get(key)
                .and_then(Value::as_str)
                .and_then(|value| value.parse::<u32>().ok())
        })
}

fn to_pull_request_item(value: &Value) -> PullRequestItem {
    PullRequestItem {
        number: string_field(value, "iid")
            .or_else(|| string_field(value, "number"))
            .unwrap_or_else(|| "—".to_string()),
        title: string_field(value, "title").unwrap_or_else(|| "Untitled pull request".to_string()),
        author: nested_string(value, "author", "name")
            .or_else(|| nested_string(value, "author", "username"))
            .or_else(|| nested_string(value, "user", "login"))
            .unwrap_or_else(|| "Unknown author".to_string()),
        source_branch: string_field(value, "source_branch")
            .or_else(|| nested_string(value, "head", "label"))
            .unwrap_or_else(|| "—".to_string()),
        target_branch: string_field(value, "target_branch")
            .or_else(|| nested_string(value, "base", "label"))
            .unwrap_or_else(|| "—".to_string()),
        updated_at: string_field(value, "updated_at")
            .unwrap_or_else(|| "Unknown update time".to_string()),
        web_url: string_field(value, "web_url")
            .or_else(|| string_field(value, "html_url"))
            .unwrap_or_default(),
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

fn project_exactly_matches_remote(
    project: &Value,
    expected_path: &str,
    locator: &RepoLocator,
) -> bool {
    let matches_value = |key: &str, expected: &str| {
        project
            .get(key)
            .and_then(Value::as_str)
            .map(|value| value.trim_end_matches(".git") == expected)
            .unwrap_or(false)
    };

    matches_value("path_with_namespace", expected_path)
        || matches_value("ssh_url_to_repo", locator.remote_repo.as_str())
        || matches_value("http_url_to_repo", locator.remote_repo.as_str())
}

fn value_to_id(value: Option<&Value>, provider: &str) -> Result<String, String> {
    match value {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
        Some(Value::Number(value)) => Ok(value.to_string()),
        _ => Err(format!("{provider} project details did not include an id.")),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::select_enterprise_project_id;
    use crate::codereview::api::parse_repo_locator;

    #[test]
    fn chooses_the_exact_remote_match_from_a_search_result() {
        let locator = parse_repo_locator("git@gitee.example.com:platform/usher.git").unwrap();
        let payload = json!([
            { "id": 17, "name": "usher", "path_with_namespace": "other/usher" },
            { "id": 42, "name": "usher", "path_with_namespace": "platform/usher" }
        ]);

        assert_eq!(
            select_enterprise_project_id(&payload, &locator).unwrap(),
            "42"
        );
    }
}
