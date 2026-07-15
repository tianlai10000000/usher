//! Remote repository detail lookups used when a project is saved.

pub(crate) mod gitee;
mod github;
mod gitlab;

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepoType {
    Gitee,
    GitLab,
    GitHub,
}

impl RepoType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Gitee => "gitee",
            Self::GitLab => "gitlab",
            Self::GitHub => "github",
        }
    }
}

impl fmt::Display for RepoType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepoDetail {
    pub(crate) repo_project_id: String,
    pub(crate) repo_type: RepoType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestItem {
    pub(crate) number: String,
    pub(crate) title: String,
    pub(crate) author: String,
    pub(crate) source_branch: String,
    pub(crate) target_branch: String,
    pub(crate) updated_at: String,
    pub(crate) web_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestPage {
    pub(crate) items: Vec<PullRequestItem>,
    pub(crate) total: u32,
    pub(crate) total_exact: bool,
    /// Provider-supplied page count when it is available separately from the
    /// total-item header (common on GitLab-shaped internal APIs).
    pub(crate) page_count: Option<u32>,
}

/// Provider-neutral status filter. `None` means the caller intentionally
/// requests the provider default and no state query parameter is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PullRequestState {
    Open,
    Closed,
    Merged,
    All,
}

impl PullRequestState {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "open" | "opened" => Some(Self::Open),
            "closed" => Some(Self::Closed),
            "merged" => Some(Self::Merged),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Merged => "merged",
            Self::All => "all",
        }
    }

    pub(super) const fn gitee_query(self) -> &'static str {
        match self {
            Self::Open => "opened",
            Self::Closed => "closed",
            Self::Merged => "merged",
            Self::All => "all",
        }
    }

    pub(super) const fn gitee_public_query(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed | Self::Merged => "closed",
            Self::All => "all",
        }
    }

    pub(super) const fn gitlab_query(self) -> &'static str {
        match self {
            Self::Open => "opened",
            Self::Closed => "closed",
            Self::Merged => "merged",
            Self::All => "all",
        }
    }

    pub(super) const fn github_query(self) -> &'static str {
        match self {
            Self::Open => "is:open",
            Self::Closed => "is:closed",
            Self::Merged => "is:merged",
            Self::All => "",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RepoLocator {
    pub(super) remote_repo: String,
    pub(super) scheme: String,
    pub(super) authority: String,
    pub(super) host: String,
    pub(super) namespace: String,
    pub(super) repo: String,
    pub(super) repo_type: RepoType,
}

/// Query the configured provider for the immutable numeric/string project id.
///
/// The remote URL is the source of truth for both provider selection and the
/// namespace/repository used by provider APIs.
pub(crate) async fn get_repo_detail(remote_repo: &str) -> Result<RepoDetail, String> {
    let locator = parse_repo_locator(remote_repo)?;
    let repo_project_id = match locator.repo_type {
        RepoType::Gitee => gitee::get_repo_detail(&locator).await?,
        RepoType::GitLab => gitlab::get_repo_detail(&locator).await?,
        RepoType::GitHub => github::get_repo_detail(&locator).await?,
    };

    Ok(RepoDetail {
        repo_project_id,
        repo_type: locator.repo_type,
    })
}

/// Load one page of currently open pull requests for an already-configured project.
pub(crate) async fn get_pull_requests(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
    state: Option<PullRequestState>,
    page: u32,
    per_page: u32,
) -> Result<PullRequestPage, String> {
    let locator = parse_repo_locator(remote_repo)?;
    let expected_type = locator.repo_type.as_str();
    if !repo_type.trim().eq_ignore_ascii_case(expected_type) {
        return Err(
            "Saved repository provider does not match the remote repository URL.".to_string(),
        );
    }
    if repo_project_id.trim().is_empty() {
        return Err("This project has no remote project id. Save it again to synchronize repository metadata.".to_string());
    }

    match locator.repo_type {
        RepoType::Gitee => {
            gitee::get_pull_requests(&locator, repo_project_id, state, page, per_page).await
        }
        RepoType::GitLab => {
            gitlab::get_pull_requests(&locator, repo_project_id, state, page, per_page).await
        }
        RepoType::GitHub => github::get_pull_requests(&locator, state, page, per_page).await,
    }
}

pub(super) fn parse_repo_locator(remote_repo: &str) -> Result<RepoLocator, String> {
    let trimmed = remote_repo
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git");
    if trimmed.is_empty() {
        return Err("Remote repository URL is empty.".to_string());
    }

    let (scheme, authority, host, path) = if let Ok(url) = reqwest::Url::parse(trimmed) {
        let host = url
            .host_str()
            .ok_or_else(|| "Remote repository URL has no host.".to_string())?
            .to_string();
        let authority = match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.clone(),
        };
        (
            url.scheme().to_string(),
            authority,
            host,
            url.path().to_string(),
        )
    } else if let Some((host, path)) = parse_ssh_remote(trimmed) {
        let scheme = configured_api_scheme(RepoType::Gitee, host.as_str())
            .unwrap_or("https")
            .to_string();
        (scheme, host.clone(), host, path)
    } else {
        return Err("Unsupported remote repository URL. Use HTTPS or SSH Git syntax.".to_string());
    };

    let segments = path
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.len() < 2 {
        return Err(
            "Remote repository URL must include an owner/group and repository name.".to_string(),
        );
    }

    let repo = segments[segments.len() - 1].to_string();
    let namespace = segments[..segments.len() - 1].join("/");
    let repo_type = detect_repo_type(host.as_str()).ok_or_else(|| {
        format!(
            "Unsupported repository host '{host}'. Supported providers are Gitee, GitLab, and GitHub."
        )
    })?;

    Ok(RepoLocator {
        remote_repo: trimmed.to_string(),
        scheme,
        authority,
        host,
        namespace,
        repo,
        repo_type,
    })
}

fn parse_ssh_remote(remote: &str) -> Option<(String, String)> {
    if let Some(rest) = remote.strip_prefix("ssh://") {
        let after_user = rest
            .rsplit_once('@')
            .map(|(_, value)| value)
            .unwrap_or(rest);
        let (authority, path) = after_user.split_once('/')?;
        return Some((authority.to_string(), path.to_string()));
    }

    let (user_and_host, path) = remote.split_once(':')?;
    let host = user_and_host
        .rsplit_once('@')
        .map(|(_, value)| value)
        .unwrap_or(user_and_host);
    Some((host.to_string(), path.to_string()))
}

fn detect_repo_type(host: &str) -> Option<RepoType> {
    let host = host.to_ascii_lowercase();
    if is_configured_enterprise_host(RepoType::GitHub, host.as_str()) || host.contains("github") {
        Some(RepoType::GitHub)
    } else if is_configured_enterprise_host(RepoType::Gitee, host.as_str())
        || host.contains("gitee")
    {
        Some(RepoType::Gitee)
    } else if is_configured_enterprise_host(RepoType::GitLab, host.as_str())
        || host.contains("gitlab")
    {
        Some(RepoType::GitLab)
    } else {
        None
    }
}

pub(super) fn configured_api_base_url(repo_type: RepoType) -> Option<String> {
    let value = crate::settings::load_git_hosted_repository_config(repo_type.as_str()).api_base_url;
    (!value.trim().is_empty()).then_some(value)
}

pub(super) fn configured_enterprise(repo_type: RepoType) -> Option<String> {
    let value = crate::settings::load_git_hosted_repository_config(repo_type.as_str()).enterprise;
    (!value.trim().is_empty()).then_some(value)
}

pub(super) fn is_configured_enterprise_host(repo_type: RepoType, host: &str) -> bool {
    let Some(api_base_url) = configured_api_base_url(repo_type) else {
        return false;
    };
    let Ok(url) = reqwest::Url::parse(api_base_url.as_str()) else {
        return false;
    };
    let configured_host = match url.port() {
        Some(port) => format!("{}:{port}", url.host_str().unwrap_or_default()),
        None => url.host_str().unwrap_or_default().to_string(),
    };
    host.trim_end_matches('/')
        .eq_ignore_ascii_case(configured_host.as_str())
        || host
            .trim_end_matches('/')
            .eq_ignore_ascii_case(url.host_str().unwrap_or_default())
}

pub(super) fn configured_api_scheme(repo_type: RepoType, host: &str) -> Option<&'static str> {
    if !is_configured_enterprise_host(repo_type, host) {
        return None;
    }
    let api_base_url = configured_api_base_url(repo_type)?;
    match reqwest::Url::parse(api_base_url.as_str()).ok()?.scheme() {
        "http" => Some("http"),
        "https" => Some("https"),
        _ => None,
    }
}

pub(super) fn percent_encode_path(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

pub(super) fn configured_token(repo_type: RepoType) -> Option<String> {
    let tokens = crate::settings::load_git_platform_tokens();
    let token = match repo_type {
        RepoType::Gitee => tokens.gitee,
        RepoType::GitLab => tokens.gitlab,
        RepoType::GitHub => tokens.github,
    };
    (!token.trim().is_empty()).then(|| token.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::{PullRequestState, RepoType, parse_repo_locator, percent_encode_path};

    #[test]
    fn parses_https_and_ssh_remotes() {
        let github = parse_repo_locator("git@github.com:acme/widget.git").unwrap();
        assert_eq!(github.repo_type, RepoType::GitHub);
        assert_eq!(github.namespace, "acme");
        assert_eq!(github.repo, "widget");

        let gitlab = parse_repo_locator("https://gitlab.example.com/team/mobile/app.git").unwrap();
        assert_eq!(gitlab.repo_type, RepoType::GitLab);
        assert_eq!(gitlab.namespace, "team/mobile");
        assert_eq!(gitlab.scheme, "https");

        let gitee = parse_repo_locator("git@gitee.example.com:platform/usher.git").unwrap();
        assert_eq!(gitee.repo_type, RepoType::Gitee);
        assert_eq!(gitee.scheme, "https");
    }

    #[test]
    fn rejects_unknown_hosts_and_invalid_paths() {
        assert!(parse_repo_locator("https://example.com/acme/widget.git").is_err());
        assert!(parse_repo_locator("https://github.com/widget.git").is_err());
    }

    #[test]
    fn encodes_gitlab_project_paths() {
        assert_eq!(
            percent_encode_path("team/mobile app"),
            "team%2Fmobile%20app"
        );
    }

    #[test]
    fn maps_explicit_pull_request_state_filters() {
        assert_eq!(
            PullRequestState::parse("open"),
            Some(PullRequestState::Open)
        );
        assert_eq!(PullRequestState::parse("all"), Some(PullRequestState::All));
        assert_eq!(PullRequestState::Merged.gitlab_query(), "merged");
        assert_eq!(PullRequestState::Open.gitee_query(), "opened");
        assert_eq!(PullRequestState::All.github_query(), "");
    }
}
