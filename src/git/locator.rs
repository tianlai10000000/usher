use super::{GiteeApiProfile, RepoType};

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

pub(super) fn gitee_api_profile(locator: &RepoLocator) -> GiteeApiProfile {
    if is_configured_enterprise_host(RepoType::Gitee, locator.host.as_str()) {
        GiteeApiProfile::EnterpriseV8
    } else {
        GiteeApiProfile::PublicV5
    }
}

pub(super) fn configured_gitee_token(profile: GiteeApiProfile) -> Option<String> {
    let tokens = crate::settings::load_git_platform_tokens();
    let token = match profile {
        GiteeApiProfile::PublicV5 => tokens.gitee,
        GiteeApiProfile::EnterpriseV8 => tokens.gitee_enterprise,
    };
    (!token.trim().is_empty()).then(|| token.trim().to_string())
}
