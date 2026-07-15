use reqwest::header::{ACCEPT, HeaderMap};
use serde::Deserialize;
use serde_json::Value;

use crate::http::{self, RequestOptions};

use super::{
    PullRequestItem, PullRequestPage, PullRequestState, RepoLocator, RepoType,
    configured_api_base_url, configured_token, is_configured_enterprise_host, percent_encode_path,
};

#[derive(Deserialize)]
struct GitLabProject {
    id: serde_json::Value,
}

pub(super) async fn get_repo_detail(locator: &RepoLocator) -> Result<String, String> {
    let full_path = format!("{}/{}", locator.namespace, locator.repo);
    let endpoint = format!(
        "{}/projects/{}",
        api_base_url(locator),
        percent_encode_path(full_path.as_str())
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
    let endpoint = format!(
        "{}/projects/{}/merge_requests",
        api_base_url(locator),
        percent_encode_path(repo_project_id)
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

fn api_base_url(locator: &RepoLocator) -> String {
    if is_configured_enterprise_host(RepoType::GitLab, locator.host.as_str()) {
        if let Some(api_base_url) = configured_api_base_url(RepoType::GitLab) {
            return with_default_api_version(api_base_url, "/api/v4");
        }
    }
    format!("{}://{}/api/v4", locator.scheme, locator.authority)
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
