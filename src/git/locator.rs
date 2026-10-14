use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, OnceLock};

use super::{GiteeApiProfile, RepoType};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct RepoLocator {
    pub(super) remote_repo: String,
    pub(super) scheme: String,
    pub(super) authority: String,
    pub(super) host: String,
    pub(super) namespace: String,
    pub(super) repo: String,
    pub(super) repo_type: RepoType,
}

impl fmt::Debug for RepoLocator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepoLocator")
            .field("remote_repo", &"<redacted>")
            .field("scheme", &self.scheme)
            .field("authority", &self.authority)
            .field("host", &self.host)
            .field("namespace", &self.namespace)
            .field("repo", &self.repo)
            .field("repo_type", &self.repo_type)
            .finish()
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
    if host.contains("github") {
        Some(RepoType::GitHub)
    } else if host.contains("gitee") {
        Some(RepoType::Gitee)
    } else if host.contains("gitlab") {
        Some(RepoType::GitLab)
    } else if is_configured_enterprise_host(RepoType::GitHub, host.as_str()) {
        Some(RepoType::GitHub)
    } else if is_configured_enterprise_host(RepoType::Gitee, host.as_str()) {
        Some(RepoType::Gitee)
    } else if is_configured_enterprise_host(RepoType::GitLab, host.as_str()) {
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

/// Host and scheme facts extracted from one `api_base_url` value.
#[derive(Debug, Clone)]
struct ApiBaseFacts {
    scheme: String,
    host: String,
    /// `host[:port]`, matching Git remote authorities.
    authority: String,
}

/// Memoized `Url::parse` of configured API base URLs, keyed by the exact
/// input string: when the setting changes, the new value simply misses the
/// cache, so no invalidation hook is needed. Failed parses are cached too.
fn api_base_facts(api_base_url: &str) -> Option<ApiBaseFacts> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<ApiBaseFacts>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(entries) = cache.lock()
        && let Some(hit) = entries.get(api_base_url)
    {
        return hit.clone();
    }
    let parsed = reqwest::Url::parse(api_base_url).ok().map(|url| {
        let host = url.host_str().unwrap_or_default().to_string();
        ApiBaseFacts {
            scheme: url.scheme().to_string(),
            authority: match url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.clone(),
            },
            host,
        }
    });
    if let Ok(mut entries) = cache.lock() {
        // Only a handful of distinct base URLs ever exist per run; guard
        // against pathological growth instead of implementing eviction.
        if entries.len() >= 32 {
            entries.clear();
        }
        entries.insert(api_base_url.to_string(), parsed.clone());
    }
    parsed
}

pub(super) fn is_configured_enterprise_host(repo_type: RepoType, host: &str) -> bool {
    let Some(api_base_url) = configured_api_base_url(repo_type) else {
        return false;
    };
    let Some(facts) = api_base_facts(api_base_url.as_str()) else {
        return false;
    };
    let host = host.trim_end_matches('/');
    host.eq_ignore_ascii_case(facts.authority.as_str())
        || host.eq_ignore_ascii_case(facts.host.as_str())
}

pub(super) fn configured_api_scheme(repo_type: RepoType, host: &str) -> Option<&'static str> {
    if !is_configured_enterprise_host(repo_type, host) {
        return None;
    }
    let api_base_url = configured_api_base_url(repo_type)?;
    match api_base_facts(api_base_url.as_str())?.scheme.as_str() {
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
    crate::settings::get_settings_manager().with_git(|git| {
        let token = match repo_type {
            RepoType::Gitee => git.platform.gitee.token.as_str(),
            RepoType::GitLab => git.platform.gitlab.token.as_str(),
            RepoType::GitHub => git.platform.github.token.as_str(),
        };
        let token = token.trim();
        (!token.is_empty()).then(|| token.to_string())
    })
}

/// Provider-name detection is not authority to disclose a configured token.
/// Check the actual API origin, using the token and enterprise origin from one
/// settings snapshot so a concurrent settings edit cannot mix their owners.
pub(super) fn configured_token_for_api_origin(
    repo_type: RepoType,
    api_url: &str,
) -> Option<String> {
    crate::settings::get_settings_manager().with_git(|git| {
        let settings = match repo_type {
            RepoType::GitHub => &git.platform.github,
            RepoType::GitLab => &git.platform.gitlab,
            RepoType::Gitee => &git.platform.gitee,
        };
        token_for_api_origin(repo_type, api_url, settings)
    })
}

fn token_for_api_origin(
    repo_type: RepoType,
    api_url: &str,
    settings: &crate::settings::GitTokenSettings,
) -> Option<String> {
    let endpoint = reqwest::Url::parse(api_url).ok()?;
    if !matches!(endpoint.scheme(), "http" | "https") || endpoint.host_str().is_none() {
        return None;
    }
    let public_origin = match repo_type {
        RepoType::GitHub => "https://api.github.com",
        RepoType::GitLab => "https://gitlab.com",
        RepoType::Gitee => "https://gitee.com",
    };
    let allowed = [public_origin, settings.api_base_url.trim()]
        .into_iter()
        .filter_map(|value| reqwest::Url::parse(value).ok())
        .any(|trusted| endpoint.origin() == trusted.origin());
    let token = settings.token.trim();
    (allowed && !token.is_empty()).then(|| token.to_string())
}

pub(super) fn gitee_api_profile(locator: &RepoLocator) -> GiteeApiProfile {
    if is_configured_enterprise_host(RepoType::Gitee, locator.host.as_str()) {
        GiteeApiProfile::EnterpriseV8
    } else {
        GiteeApiProfile::PublicV5
    }
}

pub(super) fn configured_gitee_token(profile: GiteeApiProfile) -> Option<String> {
    crate::settings::get_settings_manager().with_git(|git| {
        let token = match profile {
            GiteeApiProfile::PublicV5 => git.platform.gitee.token.as_str(),
            GiteeApiProfile::EnterpriseV8 => git.platform.gitee.enterprise_token.as_str(),
        };
        let token = token.trim();
        (!token.is_empty()).then(|| token.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::{RepoType, parse_repo_locator, token_for_api_origin};
    use crate::settings::GitTokenSettings;

    #[test]
    fn gitlab_token_requires_the_actual_trusted_api_origin() {
        let settings = GitTokenSettings {
            token: "fixture-token".into(),
            ..Default::default()
        };
        for endpoint in [
            "https://gitlab.com/api/v4/projects/1",
            "https://GITLAB.com:443/api/v4/projects/1",
        ] {
            assert_eq!(
                token_for_api_origin(RepoType::GitLab, endpoint, &settings).as_deref(),
                Some("fixture-token"),
                "trusted endpoint: {endpoint}"
            );
        }
        for endpoint in [
            "https://gitlab.attacker.invalid/api/v4/projects/1",
            "https://gitlab.com.attacker.invalid/api/v4/projects/1",
            "https://gitlab.com:444/api/v4/projects/1",
            "http://gitlab.com/api/v4/projects/1",
            "http://gitlab.com:443/api/v4/projects/1",
            "ssh://gitlab.com/api/v4/projects/1",
            "invalid",
        ] {
            assert!(
                token_for_api_origin(RepoType::GitLab, endpoint, &settings).is_none(),
                "untrusted endpoint: {endpoint}"
            );
        }
        // Legacy provider recognition remains compatible; it cannot grant
        // credential access for an unconfigured self-hosted service.
        let detected = parse_repo_locator("https://gitlab.attacker.invalid/team/repo").unwrap();
        assert_eq!(detected.repo_type, RepoType::GitLab);
    }

    #[test]
    fn gitlab_token_preserves_explicit_self_hosted_origins() {
        for configured in [
            "https://code.example.invalid:8443/api/v4",
            "http://127.0.0.1:9876/api/v4",
        ] {
            let settings = GitTokenSettings {
                token: " fixture-token ".into(),
                api_base_url: configured.into(),
                ..Default::default()
            };
            let endpoint = format!("{configured}/projects/1");
            assert_eq!(
                token_for_api_origin(RepoType::GitLab, &endpoint, &settings).as_deref(),
                Some("fixture-token")
            );
            let mut wrong_origin = reqwest::Url::parse(&endpoint).unwrap();
            wrong_origin.set_port(Some(9877)).unwrap();
            assert!(
                token_for_api_origin(RepoType::GitLab, wrong_origin.as_str(), &settings).is_none()
            );
            wrong_origin = reqwest::Url::parse(&endpoint).unwrap();
            wrong_origin
                .set_host(Some("gitlab.attacker.invalid"))
                .unwrap();
            assert!(
                token_for_api_origin(RepoType::GitLab, wrong_origin.as_str(), &settings).is_none()
            );
        }
    }

    #[test]
    fn locator_debug_never_exposes_remote_url_credentials() {
        let locator = parse_repo_locator("https://user:secret@github.com/acme/app.git").unwrap();
        let debug = format!("{locator:?}");

        assert!(!debug.contains("secret"));
        assert!(!debug.contains("user:"));
        assert!(debug.contains("<redacted>"));
        assert!(debug.contains("github.com"));
    }
}
