//! Immutable inputs prepared for Review and Ask work.
//!
//! This leaf owns provider-diff normalization, bounded Ask context, configuration
//! validation and local rule snapshots. It has no controller, scheduler or Slint
//! dependency. Call rule-file I/O only from Review work off the UI event loop.

use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use usher_ai_review::{
    AiModelRef, MAX_RULE_BYTES, ProjectReviewKey, RepositoryContext, ReviewInput, ReviewRule,
    ReviewTarget, RuleSnapshot,
};

use crate::settings::AiAgentSettings;

const MAX_CHAT_DIFF_BYTES: usize = 48 * 1024;

#[derive(Debug, Clone)]
pub(super) struct PendingReviewRequest {
    pub(super) project_alias: String,
    pub(super) project_key: ProjectReviewKey,
    pub(super) project_name: String,
    pub(super) local_repo: String,
    pub(super) remote_repo: String,
    pub(super) repo_type: String,
    pub(super) repo_project_id: String,
    pub(super) number: String,
    pub(super) title: String,
    pub(super) author: String,
    pub(super) source_branch: String,
    pub(super) target_branch: String,
    pub(super) updated_at: String,
    pub(super) model_name: String,
    pub(super) extra_requirements: String,
}

pub(super) fn chat_input(
    request: &PendingReviewRequest,
    settings: &AiAgentSettings,
    target: ReviewTarget,
    unified_diff: String,
) -> ReviewInput {
    let unified_diff_truncated = unified_diff.len() > MAX_CHAT_DIFF_BYTES;
    let unified_diff = bounded_utf8_prefix(unified_diff.as_str(), MAX_CHAT_DIFF_BYTES).to_string();
    ReviewInput {
        target,
        model: AiModelRef {
            provider: settings.provider.clone(),
            base_url: settings.base_url.clone(),
            model: settings.model.clone(),
        },
        unified_diff,
        unified_diff_truncated,
        repository_context: RepositoryContext {
            local_repo: (!request.local_repo.trim().is_empty())
                .then(|| PathBuf::from(request.local_repo.clone())),
            remote_repo: (!request.remote_repo.trim().is_empty())
                .then(|| request.remote_repo.clone()),
        },
        base_rule: None,
        additional_rules: Vec::new(),
        extra_requirements: String::new(),
    }
}

pub(super) fn provisional_chat_input(
    request: &PendingReviewRequest,
    settings: &AiAgentSettings,
    target: &ReviewTarget,
) -> ReviewInput {
    ReviewInput {
        target: target.clone(),
        model: AiModelRef {
            provider: settings.provider.clone(),
            base_url: settings.base_url.clone(),
            model: settings.model.clone(),
        },
        unified_diff: String::new(),
        unified_diff_truncated: false,
        repository_context: RepositoryContext {
            local_repo: (!request.local_repo.trim().is_empty())
                .then(|| PathBuf::from(request.local_repo.clone())),
            remote_repo: (!request.remote_repo.trim().is_empty())
                .then(|| request.remote_repo.clone()),
        },
        base_rule: None,
        additional_rules: Vec::new(),
        extra_requirements: String::new(),
    }
}

pub(super) fn bounded_utf8_prefix(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

pub(super) fn target_from_detail(
    request: &PendingReviewRequest,
    detail: &crate::git::PullRequestDiffDetail,
) -> ReviewTarget {
    ReviewTarget {
        author_name: detail.author_identity.name.clone(),
        author_email: detail.author_identity.email.clone(),
        project_key: request.project_key.clone(),
        project_name: request.project_name.clone(),
        pull_request_number: request.number.clone(),
        pull_request_title: request.title.clone(),
        base_sha: detail.base_sha.clone(),
        head_sha: detail.head_sha.clone(),
        author: request.author.clone(),
        source_branch: request.source_branch.clone(),
        target_branch: request.target_branch.clone(),
        updated_at: request.updated_at.clone(),
    }
}

pub(super) fn unified_diff_from_detail(detail: &crate::git::PullRequestDiffDetail) -> String {
    let mut output = String::new();
    for file in &detail.files {
        let old_path = if file.old_path.trim().is_empty() {
            file.path.as_str()
        } else {
            file.old_path.as_str()
        };
        output.push_str("diff --git a/");
        output.push_str(old_path);
        output.push_str(" b/");
        output.push_str(file.path.as_str());
        output.push('\n');
        output.push_str("--- a/");
        output.push_str(old_path);
        output.push('\n');
        output.push_str("+++ b/");
        output.push_str(file.path.as_str());
        output.push('\n');
        for line in &file.lines {
            // Only the provider-reported kind identifies hunk headers. Body
            // lines whose content merely starts with "@@" must keep their
            // +/-/space prefix, or the AI receives shifted line numbers.
            if line.kind == crate::git::DiffLineKind::Hunk {
                output.push_str(line.content.as_str());
            } else {
                output.push(match line.kind {
                    crate::git::DiffLineKind::Addition => '+',
                    crate::git::DiffLineKind::Deletion => '-',
                    _ => ' ',
                });
                output.push_str(line.content.as_str());
            }
            output.push('\n');
        }
    }
    output
}

pub(super) fn read_rule_snapshot(rule: ReviewRule) -> Result<RuleSnapshot, String> {
    let metadata = fs::metadata(&rule.markdown_path).map_err(|error| {
        log::warn!(
            "[AI Review][Rule] Cannot read rule '{}': {error}",
            rule.name
        );
        "ai_review.failure.rule".to_string()
    })?;
    if !metadata.is_file() {
        log::warn!("[AI Review][Rule] Rule '{}' is not a file", rule.name);
        return Err("ai_review.failure.rule".to_string());
    }
    if metadata.len() > MAX_RULE_BYTES {
        log::warn!(
            "[AI Review][Rule] Rule '{}' exceeds the {} KiB limit",
            rule.name,
            MAX_RULE_BYTES / 1024
        );
        return Err("ai_review.failure.rule".to_string());
    }
    let is_markdown = rule
        .markdown_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown")
        });
    if !is_markdown {
        log::warn!("[AI Review][Rule] Rule '{}' is not Markdown", rule.name);
        return Err("ai_review.failure.rule".to_string());
    }
    let content = fs::read_to_string(&rule.markdown_path).map_err(|error| {
        log::warn!(
            "[AI Review][Rule] Cannot read rule '{}': {error}",
            rule.name
        );
        "ai_review.failure.rule".to_string()
    })?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    Ok(RuleSnapshot {
        id: rule.id,
        name: rule.name,
        markdown_path: rule.markdown_path,
        content,
        content_hash: format!("{:016x}", hasher.finish()),
    })
}

pub(super) fn validate_model_settings(settings: &AiAgentSettings) -> Result<(), String> {
    if settings.base_url.trim().is_empty() {
        return Err("ai_review.failure.configure_base_url".to_string());
    }
    if settings.api_key.trim().is_empty() {
        return Err("ai_review.failure.configure_api_key".to_string());
    }
    if settings.model.trim().is_empty() {
        return Err("ai_review.failure.configure_model".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_CHAT_DIFF_BYTES, PendingReviewRequest, chat_input, provisional_chat_input,
        validate_model_settings,
    };
    use crate::settings::AiAgentSettings;
    use usher_ai_review::{ProjectReviewKey, ReviewTarget};

    #[test]
    fn ask_context_preserves_bound_target_and_bounds_unicode_diff() {
        let request = PendingReviewRequest {
            project_alias: "project".into(),
            project_key: ProjectReviewKey("project-id".into()),
            project_name: "Project".into(),
            local_repo: "/synthetic/project".into(),
            remote_repo: "https://example.invalid/project".into(),
            repo_type: "github".into(),
            repo_project_id: "owner/project".into(),
            number: "42".into(),
            title: "Review context".into(),
            author: "author".into(),
            source_branch: "topic".into(),
            target_branch: "main".into(),
            updated_at: "synthetic-time".into(),
            model_name: "selected-model".into(),
            extra_requirements: "Review-only requirements".into(),
        };
        let target = ReviewTarget {
            project_key: request.project_key.clone(),
            project_name: request.project_name.clone(),
            pull_request_number: request.number.clone(),
            pull_request_title: request.title.clone(),
            base_sha: "base".into(),
            head_sha: "head".into(),
            author: request.author.clone(),
            author_name: Some("Author Name".into()),
            author_email: None,
            source_branch: request.source_branch.clone(),
            target_branch: request.target_branch.clone(),
            updated_at: request.updated_at.clone(),
        };
        let settings = AiAgentSettings {
            base_url: "https://example.invalid/v1".into(),
            api_key: "synthetic-key".into(),
            model: "selected-model".into(),
            ..Default::default()
        };
        let provisional = provisional_chat_input(&request, &settings, &target);
        let prefix = "a".repeat(MAX_CHAT_DIFF_BYTES - 1);
        let complete = chat_input(&request, &settings, target.clone(), format!("{prefix}中文"));

        assert_eq!(complete.target, target);
        assert_eq!(complete.model, provisional.model);
        assert_eq!(complete.repository_context, provisional.repository_context);
        assert_eq!(complete.unified_diff, prefix);
        assert!(complete.unified_diff_truncated);
        assert!(provisional.unified_diff.is_empty());
        assert!(!provisional.unified_diff_truncated);
        assert!(complete.base_rule.is_none());
        assert!(complete.additional_rules.is_empty());
        assert!(complete.extra_requirements.is_empty());

        let exact = "a".repeat(MAX_CHAT_DIFF_BYTES);
        let complete = chat_input(&request, &settings, target, exact.clone());
        assert_eq!(complete.unified_diff, exact);
        assert!(!complete.unified_diff_truncated);
    }

    #[test]
    fn missing_model_configuration_returns_semantic_error_keys() {
        let mut settings = AiAgentSettings::default();
        assert_eq!(
            validate_model_settings(&settings).unwrap_err(),
            "ai_review.failure.configure_base_url"
        );
        settings.base_url = "https://example.invalid/v1".into();
        assert_eq!(
            validate_model_settings(&settings).unwrap_err(),
            "ai_review.failure.configure_api_key"
        );
        settings.api_key = "synthetic-key".into();
        assert_eq!(
            validate_model_settings(&settings).unwrap_err(),
            "ai_review.failure.configure_model"
        );
        settings.model = "selected-model".into();
        assert!(validate_model_settings(&settings).is_ok());
    }
}
