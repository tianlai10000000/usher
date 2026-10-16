//! Structured local Git initialization diagnostics.
#[cfg(test)]
use super::command_output::clamp_step_log;
use crate::i18n::LocalizedText;
use std::{fmt, io};
pub(super) const LOCAL_TAG_ERROR_DETAIL_CHARS: usize = 320;

#[cfg(test)]
pub(super) fn format_command_log(rendered: &str, stdout: &str, stderr: &str) -> String {
    let mut lines = vec![format!("Run {rendered}")];
    lines.extend(stdout.lines().map(str::to_owned));
    lines.extend(stderr.lines().map(str::to_owned));
    clamp_step_log(lines.join("\n"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LocalTagReadError {
    RepositoryMissing,
    GitNotFound,
    GitPermissionDenied,
    XcodeLicenseNotAccepted,
    DeveloperToolsUnavailable,
    NotRepository,
    UnsafeRepository,
    GitStartFailed { detail: String },
    GitFailed { status: String, detail: String },
    GitFailedWithoutDetail { status: String },
}

impl LocalTagReadError {
    pub(super) fn from_spawn(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::GitNotFound,
            io::ErrorKind::PermissionDenied => Self::GitPermissionDenied,
            _ => Self::GitStartFailed {
                detail: bounded_diagnostic_detail(error.to_string().as_str()),
            },
        }
    }

    pub(super) fn classify_failed_output(status: String, combined: &str) -> Self {
        let normalized = combined.to_ascii_lowercase();

        if normalized.contains("xcode and apple sdks license")
            || normalized.contains("xcode license agreement")
        {
            Self::XcodeLicenseNotAccepted
        } else if normalized.contains("invalid active developer path")
            || normalized.contains("requires the command line developer tools")
            || normalized.contains("unable to find utility \"git\"")
        {
            Self::DeveloperToolsUnavailable
        } else if normalized.contains("not a git repository") {
            Self::NotRepository
        } else if normalized.contains("detected dubious ownership") {
            Self::UnsafeRepository
        } else if normalized.contains("permission denied")
            || normalized.contains("operation not permitted")
        {
            Self::GitPermissionDenied
        } else {
            let detail = bounded_diagnostic_detail(combined);
            if detail.is_empty() {
                Self::GitFailedWithoutDetail { status }
            } else {
                Self::GitFailed { status, detail }
            }
        }
    }

    pub(super) fn localized(&self) -> LocalizedText {
        match self {
            Self::RepositoryMissing => LocalizedText::message("tag.load_local_repo_missing"),
            Self::GitNotFound => LocalizedText::message("tag.load_local_git_not_found"),
            Self::GitPermissionDenied => LocalizedText::message("tag.load_git_permission_denied"),
            Self::XcodeLicenseNotAccepted => LocalizedText::message("tag.load_local_xcode_license"),
            Self::DeveloperToolsUnavailable => {
                LocalizedText::message("tag.load_local_developer_tools")
            }
            Self::NotRepository => LocalizedText::message("tag.load_local_not_repository"),
            Self::UnsafeRepository => LocalizedText::message("tag.load_local_unsafe_repository"),
            Self::GitStartFailed { detail } => LocalizedText::message_with(
                "tag.load_local_git_start_failed",
                [("detail", detail.clone())],
            ),
            Self::GitFailed { status, detail } => LocalizedText::message_with(
                "tag.load_remote_git_failed",
                [("status", status.clone()), ("detail", detail.clone())],
            ),
            Self::GitFailedWithoutDetail { status } => LocalizedText::message_with(
                "tag.load_remote_git_failed_without_detail",
                [("status", status.clone())],
            ),
        }
    }
}

impl fmt::Display for LocalTagReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.localized().source_text())
    }
}

pub(super) fn bounded_diagnostic_detail(detail: &str) -> String {
    let normalized = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = normalized.chars();
    let bounded = chars
        .by_ref()
        .take(LOCAL_TAG_ERROR_DETAIL_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        format!("{bounded}...")
    } else {
        bounded
    }
}
