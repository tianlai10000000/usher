//! Provider-neutral Git repository operations and response models.

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestDiffLine {
    pub(crate) old_line: i32,
    pub(crate) new_line: i32,
    pub(crate) kind: String,
    pub(crate) content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestDiffFile {
    pub(crate) path: String,
    pub(crate) old_path: String,
    pub(crate) change_type: String,
    pub(crate) added_lines: u32,
    pub(crate) removed_lines: u32,
    pub(crate) viewed: bool,
    pub(crate) lines: Vec<PullRequestDiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestDiffDetail {
    pub(crate) files: Vec<PullRequestDiffFile>,
    pub(crate) added_lines: u32,
    pub(crate) removed_lines: u32,
    pub(crate) base_sha: String,
    pub(crate) head_sha: String,
    pub(crate) start_sha: String,
    pub(crate) too_many_commits: bool,
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

/// Load the review diff for one pull request. Provider-specific response
/// shapes remain inside the provider modules under `src/git/` and are normalized here.
pub(crate) async fn get_pull_request_diff(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
    pull_request_iid: &str,
    ignore_whitespace_change: bool,
) -> Result<PullRequestDiffDetail, String> {
    let locator = parse_repo_locator(remote_repo)?;
    if !repo_type
        .trim()
        .eq_ignore_ascii_case(locator.repo_type.as_str())
    {
        return Err(
            "Saved repository provider does not match the remote repository URL.".to_string(),
        );
    }
    if repo_project_id.trim().is_empty() {
        return Err("This project has no remote project id.".to_string());
    }
    if pull_request_iid.trim().is_empty() {
        return Err("Pull request number is empty.".to_string());
    }

    match locator.repo_type {
        RepoType::Gitee => {
            gitee::get_pull_request_diff(
                &locator,
                repo_project_id,
                pull_request_iid,
                ignore_whitespace_change,
            )
            .await
        }
        RepoType::GitLab => {
            gitlab::get_pull_request_diff(
                &locator,
                repo_project_id,
                pull_request_iid,
                ignore_whitespace_change,
            )
            .await
        }
        RepoType::GitHub => {
            github::get_pull_request_diff(&locator, pull_request_iid, ignore_whitespace_change)
                .await
        }
    }
}

/// Whether the configured provider endpoint can filter whitespace-only diff
/// changes. Currently this is part of the Gitee-compatible enterprise diff
/// contract only; public Gitee, GitHub, and GitLab adapters do not support it.
pub(crate) fn supports_ignore_whitespace(repo_type: &str, remote_repo: &str) -> bool {
    let Ok(locator) = parse_repo_locator(remote_repo) else {
        return false;
    };
    repo_type
        .trim()
        .eq_ignore_ascii_case(locator.repo_type.as_str())
        && locator.repo_type == RepoType::Gitee
        && is_configured_enterprise_host(RepoType::Gitee, locator.host.as_str())
}

/// Load repository source for a single diff file at an immutable commit ref.
/// The provider modules own authentication and response decoding; callers only
/// receive normalized UTF-8 text for local hunk expansion.
pub(crate) async fn get_repository_file_text(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
    path: &str,
    reference: &str,
) -> Result<String, String> {
    let locator = validated_repository_context(repo_type, repo_project_id, remote_repo)?;
    if path.trim().is_empty() || reference.trim().is_empty() {
        return Err("Repository file path or commit reference is empty.".to_string());
    }

    match locator.repo_type {
        RepoType::Gitee => {
            gitee::get_repository_file_text(&locator, repo_project_id, path, reference).await
        }
        RepoType::GitLab => {
            gitlab::get_repository_file_text(&locator, repo_project_id, path, reference).await
        }
        RepoType::GitHub => github::get_repository_file_text(&locator, path, reference).await,
    }
}

fn validated_repository_context(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
) -> Result<RepoLocator, String> {
    let locator = parse_repo_locator(remote_repo)?;
    if !repo_type
        .trim()
        .eq_ignore_ascii_case(locator.repo_type.as_str())
    {
        return Err(
            "Saved repository provider does not match the remote repository URL.".to_string(),
        );
    }
    if repo_project_id.trim().is_empty() {
        return Err("This project has no remote project id.".to_string());
    }
    Ok(locator)
}

pub(super) fn parse_unified_diff(diff: &str) -> Vec<PullRequestDiffLine> {
    let mut lines = Vec::new();
    let mut old_line = 0i32;
    let mut new_line = 0i32;
    for raw in diff.lines() {
        if raw.starts_with("@@") {
            if let Some((old_start, new_start)) = parse_hunk_starts(raw) {
                old_line = old_start;
                new_line = new_start;
            }
            lines.push(PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: raw.to_string(),
            });
            continue;
        }
        if raw.starts_with("\\ No newline at end of file") {
            lines.push(PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "meta".to_string(),
                content: raw.to_string(),
            });
            continue;
        }
        let (kind, old_number, new_number, content) = if let Some(content) = raw.strip_prefix('+') {
            let current = new_line;
            new_line = new_line.saturating_add(1);
            ("addition", 0, current, content)
        } else if let Some(content) = raw.strip_prefix('-') {
            let current = old_line;
            old_line = old_line.saturating_add(1);
            ("deletion", current, 0, content)
        } else {
            let content = raw.strip_prefix(' ').unwrap_or(raw);
            let current_old = old_line;
            let current_new = new_line;
            old_line = old_line.saturating_add(1);
            new_line = new_line.saturating_add(1);
            ("context", current_old, current_new, content)
        };
        lines.push(PullRequestDiffLine {
            old_line: old_number,
            new_line: new_number,
            kind: kind.to_string(),
            content: content.to_string(),
        });
    }
    lines
}

fn parse_hunk_starts(header: &str) -> Option<(i32, i32)> {
    let mut fields = header.split_whitespace();
    (fields.next()? == "@@").then_some(())?;
    let old = parse_hunk_range(fields.next()?, '-')?;
    let new = parse_hunk_range(fields.next()?, '+')?;
    Some((old, new))
}

fn parse_hunk_range(value: &str, prefix: char) -> Option<i32> {
    value
        .strip_prefix(prefix)?
        .split(',')
        .next()?
        .parse::<i32>()
        .ok()
}

pub(super) fn unavailable_diff_file(
    path: String,
    old_path: String,
    change_type: String,
    added_lines: u32,
    removed_lines: u32,
    message: &str,
) -> PullRequestDiffFile {
    PullRequestDiffFile {
        path,
        old_path,
        change_type,
        added_lines,
        removed_lines,
        viewed: false,
        lines: vec![PullRequestDiffLine {
            old_line: 0,
            new_line: 0,
            kind: "meta".to_string(),
            content: message.to_string(),
        }],
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
    use super::{
        PullRequestState, RepoType, parse_repo_locator, parse_unified_diff, percent_encode_path,
        supports_ignore_whitespace,
    };

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

    #[test]
    fn does_not_advertise_whitespace_filtering_for_github() {
        assert!(!supports_ignore_whitespace(
            "github",
            "https://github.com/acme/widget.git"
        ));
        assert!(!supports_ignore_whitespace(
            "gitee",
            "https://github.com/acme/widget.git"
        ));
    }

    #[test]
    fn parses_unified_diff_hunks_for_provider_normalization() {
        let lines =
            parse_unified_diff("@@ -4,2 +4,3 @@ fn example() {\n context\n-old\n+new\n+extra");
        assert_eq!(lines[0].kind, "hunk");
        assert_eq!((lines[1].old_line, lines[1].new_line), (4, 4));
        assert_eq!((lines[2].old_line, lines[2].new_line), (5, 0));
        assert_eq!((lines[3].old_line, lines[3].new_line), (0, 5));
        assert_eq!((lines[4].old_line, lines[4].new_line), (0, 6));
    }
}
