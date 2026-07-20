use crate::http::{self, RequestOptions};
use reqwest::blocking::Response;
use reqwest::header::{ACCEPT, AUTHORIZATION, USER_AGENT};
use serde_json::Value;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum PullRequestStateFilter {
    Open,
    Closed,
    All,
}

#[derive(Debug, Clone)]
pub(crate) struct PullRequestCountQuery {
    pub(crate) remote_repo: String,
    pub(crate) branch_name: String,
    pub(crate) state: PullRequestStateFilter,
    pub(crate) reviewer: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PullRequestCountResult {
    pub(crate) branch_name: String,
    pub(crate) count: u32,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EnvironmentPullRequestCounts {
    pub(crate) fat: u32,
    pub(crate) uat: u32,
    pub(crate) prod: u32,
}

pub(crate) fn fetch_pull_request_counts(
    queries: &[PullRequestCountQuery],
) -> Result<Vec<PullRequestCountResult>, String> {
    if queries.is_empty() {
        return Ok(Vec::new());
    }

    queries
        .iter()
        .map(|query| {
            fetch_pull_request_count(query).map(|count| PullRequestCountResult {
                branch_name: query.branch_name.clone(),
                count,
            })
        })
        .collect()
}

pub(crate) fn fetch_standard_environment_pr_counts(
    remote_repo: &str,
    state: PullRequestStateFilter,
    reviewer: Option<&str>,
) -> Result<EnvironmentPullRequestCounts, String> {
    let queries = ["fat", "uat", "prod"]
        .into_iter()
        .map(|branch_name| PullRequestCountQuery {
            remote_repo: remote_repo.to_string(),
            branch_name: branch_name.to_string(),
            state: state.clone(),
            reviewer: reviewer.map(str::to_string),
        })
        .collect::<Vec<_>>();

    let mut counts = EnvironmentPullRequestCounts::default();
    for result in fetch_pull_request_counts(&queries)? {
        match result.branch_name.as_str() {
            "fat" => counts.fat = result.count,
            "uat" => counts.uat = result.count,
            "prod" => counts.prod = result.count,
            _ => {}
        }
    }
    Ok(counts)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepoProvider {
    GitHub,
    GitLab,
    GiteeEnterprise,
}

#[derive(Debug, Clone)]
struct RepoLocator {
    provider: RepoProvider,
    host: String,
    owner: String,
    repo: String,
    enterprise: Option<String>,
}

fn fetch_pull_request_count(query: &PullRequestCountQuery) -> Result<u32, String> {
    let locator = parse_repo_locator(&query.remote_repo)?;
    log::info!(
        "[PRStats] request provider={} host={} repo={}/{} branch={} state={} reviewer={} auth={}",
        provider_name(locator.provider),
        locator.host,
        locator.owner,
        locator.repo,
        query.branch_name,
        map_state(query.state.clone()),
        query.reviewer.as_deref().unwrap_or("-"),
        auth_source_for_provider(&locator)
    );
    match locator.provider {
        RepoProvider::GitHub => fetch_github_pull_count(&locator, query),
        RepoProvider::GitLab => fetch_gitlab_merge_request_count(&locator, query),
        RepoProvider::GiteeEnterprise => fetch_gitee_enterprise_pull_count(&locator, query),
    }
}

fn parse_repo_locator(remote_repo: &str) -> Result<RepoLocator, String> {
    let trimmed = remote_repo.trim().trim_end_matches(".git");
    if trimmed.is_empty() {
        return Err("Missing remote repository for PR statistics request.".to_string());
    }

    // 支持 https://host/owner/repo(.git)
    if let Ok(url) = reqwest::Url::parse(trimmed) {
        let host = url
            .host_str()
            .ok_or_else(|| "Remote repo URL missing host.".to_string())?
            .to_string();
        let path_parts = url
            .path()
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if path_parts.len() < 2 {
            return Err(format!(
                "Remote repo path is invalid: '{}'. Expected at least owner/repo.",
                remote_repo
            ));
        }
        let repo = path_parts[path_parts.len() - 1].to_string();
        let owner = path_parts[..path_parts.len() - 1].join("/");
        let provider = detect_provider(&host);
        let enterprise = parse_gitee_enterprise_from_path(provider, &path_parts);
        return Ok(RepoLocator {
            provider,
            host,
            owner,
            repo,
            enterprise,
        });
    }

    // 支持 git@host:owner/repo(.git) / ssh://git@host/owner/repo(.git)
    if let Some((host, path)) = parse_ssh_like_remote(trimmed) {
        let path_parts = path
            .split('/')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if path_parts.len() < 2 {
            return Err(format!(
                "Remote repo path is invalid: '{}'. Expected at least owner/repo.",
                remote_repo
            ));
        }
        let repo = path_parts[path_parts.len() - 1].to_string();
        let owner = path_parts[..path_parts.len() - 1].join("/");
        let provider = detect_provider(&host);
        let enterprise = parse_gitee_enterprise_from_path(provider, &path_parts);
        return Ok(RepoLocator {
            provider,
            host,
            owner,
            repo,
            enterprise,
        });
    }

    Err(format!(
        "Unsupported remote repository format: '{}'.",
        remote_repo
    ))
}

fn parse_ssh_like_remote(remote: &str) -> Option<(String, String)> {
    if let Some(rest) = remote.strip_prefix("ssh://") {
        let after_at = rest
            .rsplit_once('@')
            .map(|(_, right)| right)
            .unwrap_or(rest);
        let (host, path) = after_at.split_once('/')?;
        return Some((host.to_string(), path.to_string()));
    }
    let (left, path) = remote.split_once(':')?;
    let host = left
        .rsplit_once('@')
        .map(|(_, right)| right)
        .unwrap_or(left);
    Some((host.to_string(), path.to_string()))
}

fn detect_provider(host: &str) -> RepoProvider {
    let lower = host.to_ascii_lowercase();
    if lower.contains("github") {
        RepoProvider::GitHub
    } else if lower.contains("gitlab") {
        RepoProvider::GitLab
    } else if lower.contains("gitee") {
        RepoProvider::GiteeEnterprise
    } else {
        // 默认按 GitLab 自建兼容处理
        RepoProvider::GitLab
    }
}

fn parse_gitee_enterprise_from_path(provider: RepoProvider, path_parts: &[&str]) -> Option<String> {
    if provider != RepoProvider::GiteeEnterprise {
        return None;
    }
    // 企业版常见路径: /enterprises/{enterprise}/projects/{group}/{repo}
    let lower = path_parts
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect::<Vec<_>>();
    for index in 0..lower.len().saturating_sub(1) {
        if lower[index] == "enterprises" {
            return Some(path_parts[index + 1].to_string());
        }
    }
    None
}

fn fetch_github_pull_count(
    locator: &RepoLocator,
    query: &PullRequestCountQuery,
) -> Result<u32, String> {
    let mut page = 1u32;
    let mut count = 0u32;
    loop {
        let endpoint = format!(
            "https://api.github.com/repos/{}/{}/pulls",
            locator.owner, locator.repo
        );
        let mut request = http::blocking_get(endpoint)
            .query(&[
                ("state", map_state(query.state.clone())),
                ("base", query.branch_name.as_str()),
                ("per_page", "100"),
                ("page", page.to_string().as_str()),
            ])
            .header(USER_AGENT, "usher-pr-stats")
            .header(ACCEPT, "application/vnd.github+json");

        if let Some(token) = read_github_token() {
            request = request.header(AUTHORIZATION, format!("Bearer {}", token));
        }

        let response =
            http::send_blocking(request, RequestOptions::new("Load GitHub pull requests"))
                .map_err(|error| error.to_string())?;
        let payload = ensure_success(response, "GitHub")?;
        let list = payload
            .as_array()
            .ok_or_else(|| "GitHub API response is not an array.".to_string())?;
        let filtered = if let Some(reviewer) = query.reviewer.as_ref() {
            list.iter()
                .filter(|item| github_matches_reviewer(item, reviewer))
                .count() as u32
        } else {
            list.len() as u32
        };
        count = count.saturating_add(filtered);
        if list.len() < 100 {
            break;
        }
        page = page.saturating_add(1);
    }
    Ok(count)
}

fn fetch_gitlab_merge_request_count(
    locator: &RepoLocator,
    query: &PullRequestCountQuery,
) -> Result<u32, String> {
    let project_id = url_encode_path(&(locator.owner.clone() + "/" + &locator.repo));
    let endpoint = format!(
        "https://{}/api/v4/projects/{}/merge_requests",
        locator.host, project_id
    );
    let mut page = 1u32;
    let mut count = 0u32;
    loop {
        let mut request = http::blocking_get(endpoint.as_str())
            .query(&[
                ("state", map_state(query.state.clone())),
                ("target_branch", query.branch_name.as_str()),
                ("per_page", "100"),
                ("page", page.to_string().as_str()),
            ])
            .header(ACCEPT, "application/json");

        if let Some(reviewer) = query.reviewer.as_ref() {
            request = request.query(&[("reviewer_username", reviewer.as_str())]);
        }
        if let Some(token) = read_gitlab_token() {
            request = request.header("PRIVATE-TOKEN", token);
        }

        let response =
            http::send_blocking(request, RequestOptions::new("Load GitLab merge requests"))
                .map_err(|error| error.to_string())?;
        let payload = ensure_success(response, "GitLab")?;
        let list = payload
            .as_array()
            .ok_or_else(|| "GitLab API response is not an array.".to_string())?;
        let filtered = if let Some(reviewer) = query.reviewer.as_ref() {
            list.iter()
                .filter(|item| gitlab_matches_reviewer(item, reviewer))
                .count() as u32
        } else {
            list.len() as u32
        };
        count = count.saturating_add(filtered);
        if list.len() < 100 {
            break;
        }
        page = page.saturating_add(1);
    }
    Ok(count)
}

fn fetch_gitee_enterprise_pull_count(
    locator: &RepoLocator,
    query: &PullRequestCountQuery,
) -> Result<u32, String> {
    // 企业版优先使用 enterprise 级接口，失败后回退到仓库 pulls 接口。
    let mut first_error = None::<String>;
    if let Some(enterprise) = locator.enterprise.as_ref() {
        match fetch_gitee_enterprise_level_pull_count(locator, enterprise, query) {
            Ok(count) => return Ok(count),
            Err(error) => first_error = Some(error),
        }
    }

    match fetch_gitee_repo_pull_count(locator, query) {
        Ok(count) => Ok(count),
        Err(error) => {
            let prefix = first_error
                .map(|e| format!("Enterprise endpoint failed: {}. ", e))
                .unwrap_or_default();
            Err(format!(
                "{}Gitee repository endpoint failed: {}",
                prefix, error
            ))
        }
    }
}

fn fetch_gitee_enterprise_level_pull_count(
    locator: &RepoLocator,
    enterprise: &str,
    query: &PullRequestCountQuery,
) -> Result<u32, String> {
    let endpoint = format!(
        "https://{}/api/v5/enterprises/{}/pull_requests",
        locator.host, enterprise
    );
    let mut page = 1u32;
    let mut count = 0u32;
    loop {
        let mut request = http::blocking_get(endpoint.as_str()).query(&[
            ("state", map_state(query.state.clone())),
            ("base", query.branch_name.as_str()),
            (
                "project_path",
                format!("{}/{}", locator.owner, locator.repo).as_str(),
            ),
            ("per_page", "100"),
            ("page", page.to_string().as_str()),
        ]);
        request = attach_gitee_auth(request, locator.host.as_str());
        let response = http::send_blocking(
            request,
            RequestOptions::new("Load Gitee enterprise pull requests"),
        )
        .map_err(|error| error.to_string())?;
        let payload = ensure_success(response, "Gitee enterprise")?;
        let list = payload
            .as_array()
            .ok_or_else(|| "Gitee enterprise API response is not an array.".to_string())?;
        let filtered = if let Some(reviewer) = query.reviewer.as_ref() {
            list.iter()
                .filter(|item| gitee_matches_reviewer(item, reviewer))
                .count() as u32
        } else {
            list.len() as u32
        };
        count = count.saturating_add(filtered);
        if list.len() < 100 {
            break;
        }
        page = page.saturating_add(1);
    }
    Ok(count)
}

fn fetch_gitee_repo_pull_count(
    locator: &RepoLocator,
    query: &PullRequestCountQuery,
) -> Result<u32, String> {
    let endpoint = format!(
        "https://{}/api/v5/repos/{}/{}/pulls",
        locator.host, locator.owner, locator.repo
    );
    let mut page = 1u32;
    let mut count = 0u32;
    loop {
        let mut request = http::blocking_get(endpoint.as_str()).query(&[
            ("state", map_state(query.state.clone())),
            ("base", query.branch_name.as_str()),
            ("per_page", "100"),
            ("page", page.to_string().as_str()),
        ]);
        request = attach_gitee_auth(request, locator.host.as_str());
        let response = http::send_blocking(
            request,
            RequestOptions::new("Load Gitee repository pull requests"),
        )
        .map_err(|error| error.to_string())?;
        let payload = ensure_success(response, "Gitee repository")?;
        let list = payload
            .as_array()
            .ok_or_else(|| "Gitee repository API response is not an array.".to_string())?;
        let filtered = if let Some(reviewer) = query.reviewer.as_ref() {
            list.iter()
                .filter(|item| gitee_matches_reviewer(item, reviewer))
                .count() as u32
        } else {
            list.len() as u32
        };
        count = count.saturating_add(filtered);
        if list.len() < 100 {
            break;
        }
        page = page.saturating_add(1);
    }
    Ok(count)
}

fn attach_gitee_auth(
    request: reqwest::blocking::RequestBuilder,
    host: &str,
) -> reqwest::blocking::RequestBuilder {
    if let Some(token) = read_gitee_token(host) {
        // 兼容两种方式：Bearer 头 + access_token query
        return request
            .header(AUTHORIZATION, format!("token {}", token))
            .query(&[("access_token", token.as_str())]);
    }
    request
}

fn ensure_success(response: Response, provider: &str) -> Result<Value, String> {
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| format!("{} response read failed: {}", provider, error))?;
    if !status.is_success() {
        return Err(format!(
            "{} API returned {}: {}",
            provider,
            status.as_u16(),
            body
        ));
    }
    serde_json::from_str(&body).map_err(|error| {
        let body_excerpt = body.chars().take(500).collect::<String>();
        format!(
            "{} response parse failed: {}. body={}",
            provider, error, body_excerpt
        )
    })
}

fn map_state(state: PullRequestStateFilter) -> &'static str {
    match state {
        PullRequestStateFilter::Open => "open",
        PullRequestStateFilter::Closed => "closed",
        PullRequestStateFilter::All => "all",
    }
}

fn github_matches_reviewer(item: &Value, reviewer: &str) -> bool {
    let reviewer_lower = reviewer.to_ascii_lowercase();
    item.get("requested_reviewers")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter().any(|u| {
                u.get("login")
                    .and_then(Value::as_str)
                    .map(|name| name.eq_ignore_ascii_case(&reviewer_lower))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn gitlab_matches_reviewer(item: &Value, reviewer: &str) -> bool {
    let reviewer_lower = reviewer.to_ascii_lowercase();
    item.get("reviewers")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter().any(|u| {
                u.get("username")
                    .and_then(Value::as_str)
                    .map(|name| name.eq_ignore_ascii_case(&reviewer_lower))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn gitee_matches_reviewer(item: &Value, reviewer: &str) -> bool {
    let reviewer_lower = reviewer.to_ascii_lowercase();
    // 企业版/标准版字段不完全一致，做多字段兼容。
    if item
        .get("assignees")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter().any(|u| {
                u.get("login")
                    .and_then(Value::as_str)
                    .map(|name| name.eq_ignore_ascii_case(&reviewer_lower))
                    .unwrap_or(false)
                    || u.get("name")
                        .and_then(Value::as_str)
                        .map(|name| name.eq_ignore_ascii_case(&reviewer_lower))
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false)
    {
        return true;
    }
    item.get("reviewers")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter().any(|u| {
                u.get("login")
                    .and_then(Value::as_str)
                    .map(|name| name.eq_ignore_ascii_case(&reviewer_lower))
                    .unwrap_or(false)
                    || u.get("name")
                        .and_then(Value::as_str)
                        .map(|name| name.eq_ignore_ascii_case(&reviewer_lower))
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn url_encode_path(path: &str) -> String {
    path.replace('%', "%25")
        .replace('/', "%2F")
        .replace(' ', "%20")
}

fn read_non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn provider_name(provider: RepoProvider) -> &'static str {
    match provider {
        RepoProvider::GitHub => "github",
        RepoProvider::GitLab => "gitlab",
        RepoProvider::GiteeEnterprise => "gitee-enterprise",
    }
}

fn auth_source_for_provider(locator: &RepoLocator) -> &'static str {
    match locator.provider {
        RepoProvider::GitHub => {
            if read_non_empty_settings_token("github").is_some() {
                "settings:git.platform.github.token"
            } else if read_non_empty_env("GITHUB_TOKEN").is_some() {
                "env:GITHUB_TOKEN"
            } else {
                "none"
            }
        }
        RepoProvider::GitLab => {
            if read_non_empty_settings_token("gitlab").is_some() {
                "settings:git.platform.gitlab.token"
            } else if read_non_empty_env("GITLAB_TOKEN").is_some() {
                "env:GITLAB_TOKEN"
            } else {
                "none"
            }
        }
        RepoProvider::GiteeEnterprise => {
            if gitee_uses_enterprise_profile(locator.host.as_str()) {
                if read_non_empty_settings_token("gitee-enterprise").is_some() {
                    "settings:git.platform.gitee.enterprise_token"
                } else if read_non_empty_env("GITEE_ENTERPRISE_TOKEN").is_some() {
                    "env:GITEE_ENTERPRISE_TOKEN"
                } else {
                    "none"
                }
            } else if read_non_empty_settings_token("gitee").is_some() {
                "settings:git.platform.gitee.token"
            } else if read_non_empty_env("GITEE_TOKEN").is_some() {
                "env:GITEE_TOKEN"
            } else {
                "none"
            }
        }
    }
}

fn read_github_token() -> Option<String> {
    read_non_empty_settings_token("github").or_else(|| read_non_empty_env("GITHUB_TOKEN"))
}

fn read_gitlab_token() -> Option<String> {
    read_non_empty_settings_token("gitlab").or_else(|| read_non_empty_env("GITLAB_TOKEN"))
}

fn read_gitee_token(host: &str) -> Option<String> {
    if gitee_uses_enterprise_profile(host) {
        read_non_empty_settings_token("gitee-enterprise")
            .or_else(|| read_non_empty_env("GITEE_ENTERPRISE_TOKEN"))
    } else {
        read_non_empty_settings_token("gitee").or_else(|| read_non_empty_env("GITEE_TOKEN"))
    }
}

fn read_non_empty_settings_token(platform: &str) -> Option<String> {
    let tokens = crate::settings::load_git_platform_tokens();
    let value = match platform {
        "github" => tokens.github,
        "gitlab" => tokens.gitlab,
        "gitee" => tokens.gitee,
        "gitee-enterprise" => tokens.gitee_enterprise,
        _ => String::new(),
    };
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn gitee_uses_enterprise_profile(host: &str) -> bool {
    let config = crate::settings::load_git_hosted_repository_config("gitee");
    let Ok(url) = reqwest::Url::parse(config.api_base_url.as_str()) else {
        return false;
    };
    url.host_str()
        .map(|configured| configured.eq_ignore_ascii_case(host))
        .unwrap_or(false)
}
