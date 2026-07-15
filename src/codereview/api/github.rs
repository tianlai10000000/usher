use reqwest::header::{ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::Deserialize;
use serde_json::Value;

use crate::http::{self, RequestOptions};

use super::{
    PullRequestItem, PullRequestPage, PullRequestState, RepoLocator, RepoType,
    configured_api_base_url, configured_token, is_configured_enterprise_host, percent_encode_path,
};

#[derive(Deserialize)]
struct GitHubRepository {
    id: serde_json::Value,
}

pub(super) async fn get_repo_detail(locator: &RepoLocator) -> Result<String, String> {
    let endpoint = format!(
        "{}/repos/{}/{}",
        api_base_url(locator),
        percent_encode_path(locator.namespace.as_str()),
        percent_encode_path(locator.repo.as_str())
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
    let endpoint = format!("{}/search/issues", api_base_url(locator));
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
        .collect();
    Ok(PullRequestPage {
        items,
        total,
        total_exact: true,
        page_count: Some(total.div_ceil(per_page)),
    })
}

fn api_base_url(locator: &RepoLocator) -> String {
    if is_configured_enterprise_host(RepoType::GitHub, locator.host.as_str()) {
        if let Some(api_base_url) = configured_api_base_url(RepoType::GitHub) {
            return with_default_api_version(api_base_url, "/api/v3");
        }
    }
    "https://api.github.com".to_string()
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
