use reqwest::header::ACCEPT;

use crate::http;

use super::super::{
    GiteeApiProfile, PullRequestMergeMethod, RepoLocator, RepoType, configured_enterprise,
    configured_gitee_token, percent_encode_path,
};
use super::enterprise_api_base_url;

pub(super) fn enterprise_pull_request_endpoint(
    template: &str,
    repo_project_id: &str,
    pull_request_iid: &str,
) -> Result<String, String> {
    if repo_project_id.trim().is_empty() || pull_request_iid.trim().is_empty() {
        return Err("Gitee project id or pull request number is empty.".to_string());
    }
    Ok(template
        .replace("{api_base_url}", enterprise_api_base_url()?.as_str())
        .replace(
            "{project_id}",
            percent_encode_path(repo_project_id.trim()).as_str(),
        )
        .replace(
            "{merge_request_iid}",
            percent_encode_path(pull_request_iid.trim()).as_str(),
        ))
}

pub(super) fn enterprise_web_pull_request_review_endpoint(
    locator: &RepoLocator,
    pull_request_iid: &str,
) -> Result<String, String> {
    if pull_request_iid.trim().is_empty() {
        return Err("Gitee pull request number is empty.".to_string());
    }
    let full_path = locator
        .namespace
        .split('/')
        .chain(std::iter::once(locator.repo.as_str()))
        .map(percent_encode_path)
        .collect::<Vec<_>>()
        .join("/");
    Ok(super::endpoints::ENTERPRISE_WEB_PULL_REQUEST_REVIEWS_URL
        .replace("{scheme}", locator.scheme.as_str())
        .replace("{authority}", locator.authority.as_str())
        .replace("{full_path}", full_path.as_str())
        .replace(
            "{pull_request_iid}",
            percent_encode_path(pull_request_iid.trim()).as_str(),
        ))
}

pub(super) fn enterprise_authenticated_builder(
    request: reqwest::RequestBuilder,
) -> reqwest::RequestBuilder {
    let mut request = request
        .header("OAUTH", "enabled")
        .header(ACCEPT, "application/json");
    if let Some(enterprise) = configured_enterprise(RepoType::Gitee) {
        request = request.header("enterprise", enterprise);
    }
    if let Some(token) = configured_gitee_token(GiteeApiProfile::EnterpriseV8) {
        request = request.header("PRIVATE-TOKEN", token);
    }
    request
}

pub(super) fn public_authenticated_builder(
    request: reqwest::RequestBuilder,
) -> reqwest::RequestBuilder {
    let mut request = request.header(ACCEPT, "application/json");
    if let Some(token) = configured_gitee_token(GiteeApiProfile::PublicV5) {
        request = request.header("Authorization", format!("token {token}"));
    }
    request
}

pub(super) fn parse_gitee_merge_method(value: &str) -> Option<PullRequestMergeMethod> {
    PullRequestMergeMethod::parse(value)
}

pub(super) fn public_authenticated_request(endpoint: String) -> reqwest::RequestBuilder {
    public_authenticated_builder(http::get(endpoint))
}

pub(super) fn public_repo_endpoint(
    template: &str,
    locator: &RepoLocator,
    pull_request_iid: Option<&str>,
    file_path: Option<&str>,
) -> String {
    let mut endpoint = template
        .replace("{scheme}", locator.scheme.as_str())
        .replace("{authority}", locator.authority.as_str())
        .replace(
            "{namespace}",
            percent_encode_path(locator.namespace.as_str()).as_str(),
        )
        .replace(
            "{repository}",
            percent_encode_path(locator.repo.as_str()).as_str(),
        );
    if let Some(iid) = pull_request_iid {
        endpoint = endpoint.replace(
            "{pull_request_number}",
            percent_encode_path(iid.trim()).as_str(),
        );
    }
    if let Some(path) = file_path {
        endpoint = endpoint.replace("{file_path}", percent_encode_path(path).as_str());
    }
    endpoint
}

pub(super) fn enterprise_repository_commit_endpoint(
    template: &str,
    repo_project_id: &str,
    commit_sha: &str,
) -> Result<String, String> {
    if repo_project_id.trim().is_empty() || commit_sha.trim().is_empty() {
        return Err("Gitee project id or commit SHA is empty.".to_string());
    }
    Ok(template
        .replace("{api_base_url}", enterprise_api_base_url()?.as_str())
        .replace(
            "{project_id}",
            percent_encode_path(repo_project_id.trim()).as_str(),
        )
        .replace(
            "{commit_sha}",
            percent_encode_path(commit_sha.trim()).as_str(),
        ))
}
