use std::fmt;

pub(crate) type GitResult<T> = Result<T, GitError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GitErrorKind {
    Configuration,
    Authentication,
    Permission,
    NotFound,
    Conflict,
    RateLimited,
    Network,
    Timeout,
    InvalidResponse,
    Unsupported,
    Unknown,
}

impl GitErrorKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::Authentication => "authentication",
            Self::Permission => "permission",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::RateLimited => "rate_limited",
            Self::Network => "network",
            Self::Timeout => "timeout",
            Self::InvalidResponse => "invalid_response",
            Self::Unsupported => "unsupported",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitError {
    kind: GitErrorKind,
    operation: &'static str,
    detail: String,
}

impl GitError {
    pub(crate) fn configuration(detail: impl Into<String>) -> Self {
        Self::new(
            GitErrorKind::Configuration,
            "repository configuration",
            detail,
        )
    }

    pub(crate) fn unsupported(operation: &'static str, detail: impl Into<String>) -> Self {
        Self::new(GitErrorKind::Unsupported, operation, detail)
    }

    pub(crate) fn provider(operation: &'static str, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self::new(classify_provider_error(&detail), operation, detail)
    }

    fn new(kind: GitErrorKind, operation: &'static str, detail: impl Into<String>) -> Self {
        Self {
            kind,
            operation,
            detail: detail.into(),
        }
    }

    #[cfg(test)]
    pub(crate) const fn kind(&self) -> GitErrorKind {
        self.kind
    }

    pub(crate) const fn error_kind(&self) -> &'static str {
        self.kind.as_str()
    }

    #[cfg(test)]
    pub(crate) const fn user_message(&self) -> &'static str {
        match self.kind {
            GitErrorKind::Configuration => {
                "Repository settings are incomplete or do not match the remote URL."
            }
            GitErrorKind::Authentication => {
                "Authentication failed. Check the provider token in Settings."
            }
            GitErrorKind::Permission => {
                "The provider denied this operation for the current account."
            }
            GitErrorKind::NotFound => "The repository or pull request could not be found.",
            GitErrorKind::Conflict => {
                "The repository changed and this operation can no longer be completed."
            }
            GitErrorKind::RateLimited => {
                "The provider rate limit was reached. Wait a moment and try again."
            }
            GitErrorKind::Network => {
                "The provider could not be reached. Check the network and try again."
            }
            GitErrorKind::Timeout => "The provider request timed out. Try again.",
            GitErrorKind::InvalidResponse => {
                "The provider returned a response that Usher could not read."
            }
            GitErrorKind::Unsupported => {
                "This operation is not supported by the configured provider."
            }
            GitErrorKind::Unknown => "The provider operation failed. Try again.",
        }
    }

    pub(crate) const fn user_message_key(&self) -> &'static str {
        match self.kind {
            GitErrorKind::Configuration => "git_error.configuration",
            GitErrorKind::Authentication => "git_error.authentication",
            GitErrorKind::Permission => "git_error.permission",
            GitErrorKind::NotFound => "git_error.not_found",
            GitErrorKind::Conflict => "git_error.conflict",
            GitErrorKind::RateLimited => "git_error.rate_limited",
            GitErrorKind::Network => "git_error.network",
            GitErrorKind::Timeout => "git_error.timeout",
            GitErrorKind::InvalidResponse => "git_error.invalid_response",
            GitErrorKind::Unsupported => "git_error.unsupported",
            GitErrorKind::Unknown => "git_error.unknown",
        }
    }
}

impl fmt::Display for GitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} failed ({}): {}",
            self.operation,
            self.kind.as_str(),
            self.detail
        )
    }
}

impl std::error::Error for GitError {}

fn classify_provider_error(detail: &str) -> GitErrorKind {
    let normalized = detail.to_ascii_lowercase();
    if contains_any(&normalized, &["timed out", "timeout", "deadline exceeded"]) {
        GitErrorKind::Timeout
    } else if contains_any(
        &normalized,
        &[
            "401",
            "unauthorized",
            "authentication",
            "invalid token",
            "bad credentials",
        ],
    ) {
        GitErrorKind::Authentication
    } else if contains_any(&normalized, &["403", "forbidden", "permission denied"]) {
        GitErrorKind::Permission
    } else if contains_any(&normalized, &["429", "rate limit", "too many requests"]) {
        GitErrorKind::RateLimited
    } else if contains_any(&normalized, &["404", "not found", "does not exist"]) {
        GitErrorKind::NotFound
    } else if contains_any(&normalized, &["409", "conflict", "already merged", "stale"]) {
        GitErrorKind::Conflict
    } else if contains_any(
        &normalized,
        &[
            "connection refused",
            "connection reset",
            "dns",
            "network",
            "error sending request",
            "failed to connect",
        ],
    ) {
        GitErrorKind::Network
    } else if contains_any(
        &normalized,
        &[
            "decode",
            "deserial",
            "invalid json",
            "invalid response",
            "unexpected response",
        ],
    ) {
        GitErrorKind::InvalidResponse
    } else if contains_any(
        &normalized,
        &["not supported", "not implemented", "unsupported"],
    ) {
        GitErrorKind::Unsupported
    } else {
        GitErrorKind::Unknown
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_provider_failures_without_exposing_details_to_users() {
        let auth = GitError::provider("load pull requests", "GitHub HTTP 401: bad credentials");
        assert_eq!(auth.kind(), GitErrorKind::Authentication);
        assert_eq!(auth.error_kind(), "authentication");
        assert!(auth.user_message().contains("Settings"));
        assert!(!auth.user_message().contains("bad credentials"));

        let timeout = GitError::provider("load diff", "request timed out after 10 seconds");
        assert_eq!(timeout.kind(), GitErrorKind::Timeout);
    }

    #[test]
    fn display_retains_developer_detail_for_logs() {
        let error = GitError::provider("merge pull request", "HTTP 409 conflict");
        assert_eq!(error.kind(), GitErrorKind::Conflict);
        assert!(error.to_string().contains("HTTP 409 conflict"));
    }
}
