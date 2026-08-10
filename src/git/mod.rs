//! Provider-neutral Git repository operations and response models.

mod diff;
pub(crate) mod gitee;
mod github;
mod gitlab;
mod locator;
mod model;
mod overview;

use diff::{parse_unified_diff, unavailable_diff_file};
use locator::*;
pub(crate) use model::*;
pub(crate) use overview::EnvironmentPullRequestCounts;

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
    filters: &PullRequestFilters,
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
            gitee::get_pull_requests(&locator, repo_project_id, state, filters, page, per_page)
                .await
        }
        RepoType::GitLab => {
            gitlab::get_pull_requests(&locator, repo_project_id, state, filters, page, per_page)
                .await
        }
        RepoType::GitHub => {
            github::get_pull_requests(&locator, state, filters, page, per_page).await
        }
    }
}

/// Count open pull requests targeting the standard delivery environments.
///
/// Provider selection, persisted-id validation, authentication, endpoint
/// selection, and response normalization stay inside the repository service.
pub(crate) async fn get_standard_environment_pull_request_counts(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
) -> Result<EnvironmentPullRequestCounts, String> {
    let locator = validated_repository_context(repo_type, repo_project_id, remote_repo)?;

    match locator.repo_type {
        RepoType::GitHub => {
            let (fat, uat, prod) = tokio::try_join!(
                github::get_open_pull_request_count(&locator, "fat"),
                github::get_open_pull_request_count(&locator, "uat"),
                github::get_open_pull_request_count(&locator, "prod"),
            )?;
            Ok(EnvironmentPullRequestCounts { fat, uat, prod })
        }
        RepoType::GitLab => {
            let filters = PullRequestFilters::default();
            overview::count_standard_environment_pages(|page, per_page| {
                gitlab::get_pull_requests(
                    &locator,
                    repo_project_id,
                    Some(PullRequestState::Open),
                    &filters,
                    page,
                    per_page,
                )
            })
            .await
        }
        RepoType::Gitee => {
            let filters = PullRequestFilters::default();
            overview::count_standard_environment_pages(|page, per_page| {
                gitee::get_pull_requests(
                    &locator,
                    repo_project_id,
                    Some(PullRequestState::Open),
                    &filters,
                    page,
                    per_page,
                )
            })
            .await
        }
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

/// Load a diff for one contiguous commit range within a pull request.
pub(crate) async fn get_pull_request_commit_range_diff(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
    base_sha: &str,
    head_sha: &str,
    commit_count: u32,
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
    if base_sha.trim().is_empty() || head_sha.trim().is_empty() {
        return Err("Commit range is incomplete.".to_string());
    }

    match locator.repo_type {
        RepoType::Gitee => {
            gitee::get_commit_range_diff(
                &locator,
                repo_project_id,
                base_sha,
                head_sha,
                commit_count,
                ignore_whitespace_change,
            )
            .await
        }
        RepoType::GitLab => {
            gitlab::get_commit_range_diff(
                &locator,
                repo_project_id,
                base_sha,
                head_sha,
                commit_count,
            )
            .await
        }
        RepoType::GitHub => {
            github::get_commit_range_diff(&locator, base_sha, head_sha, commit_count).await
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
        && gitee_api_profile(&locator) == GiteeApiProfile::EnterpriseV8
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

pub(crate) async fn get_pull_request_action_state(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
    pull_request_iid: &str,
) -> Result<PullRequestActionState, String> {
    let locator = validated_repository_context(repo_type, repo_project_id, remote_repo)?;
    if pull_request_iid.trim().is_empty() {
        return Err("Pull request number is empty.".to_string());
    }
    match locator.repo_type {
        RepoType::Gitee => {
            gitee::get_pull_request_action_state(&locator, repo_project_id, pull_request_iid).await
        }
        RepoType::GitHub => github::get_pull_request_action_state(&locator, pull_request_iid).await,
        RepoType::GitLab => {
            gitlab::get_pull_request_action_state(&locator, repo_project_id, pull_request_iid).await
        }
    }
}

pub(crate) async fn get_pull_request_conversation(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
    pull_request_iid: &str,
) -> Result<Vec<PullRequestConversationItem>, String> {
    let locator = validated_repository_context(repo_type, repo_project_id, remote_repo)?;
    if pull_request_iid.trim().is_empty() {
        return Err("Pull request number is empty.".to_string());
    }
    match locator.repo_type {
        RepoType::Gitee => {
            gitee::get_pull_request_conversation(&locator, repo_project_id, pull_request_iid).await
        }
        RepoType::GitHub => github::get_pull_request_conversation(&locator, pull_request_iid).await,
        RepoType::GitLab => {
            gitlab::get_pull_request_conversation(&locator, repo_project_id, pull_request_iid).await
        }
    }
}

pub(crate) async fn get_pull_request_commits(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
    pull_request_iid: &str,
) -> Result<Vec<PullRequestCommit>, String> {
    let locator = validated_repository_context(repo_type, repo_project_id, remote_repo)?;
    if pull_request_iid.trim().is_empty() {
        return Err("Pull request number is empty.".to_string());
    }
    match locator.repo_type {
        RepoType::Gitee => {
            gitee::get_pull_request_commits(&locator, repo_project_id, pull_request_iid).await
        }
        RepoType::GitHub => github::get_pull_request_commits(&locator, pull_request_iid).await,
        RepoType::GitLab => {
            gitlab::get_pull_request_commits(&locator, repo_project_id, pull_request_iid).await
        }
    }
}

pub(crate) async fn submit_pull_request_review(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
    pull_request_iid: &str,
    action: PullRequestReviewAction,
    body: &str,
) -> Result<(), String> {
    let locator = validated_repository_context(repo_type, repo_project_id, remote_repo)?;
    if pull_request_iid.trim().is_empty() {
        return Err("Pull request number is empty.".to_string());
    }
    match locator.repo_type {
        RepoType::Gitee => {
            gitee::submit_pull_request_review(
                &locator,
                repo_project_id,
                pull_request_iid,
                action,
                body,
            )
            .await
        }
        RepoType::GitHub => {
            github::submit_pull_request_review(&locator, pull_request_iid, action, body).await
        }
        RepoType::GitLab => {
            gitlab::submit_pull_request_review(
                &locator,
                repo_project_id,
                pull_request_iid,
                action,
                body,
            )
            .await
        }
    }
}

/// Dismiss one visible review through the configured provider's native
/// operation. GitHub dismisses the selected review id; GitLab and Gitee
/// Enterprise withdraw the authenticated user's review state.
pub(crate) async fn dismiss_pull_request_review(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
    pull_request_iid: &str,
    review_id: &str,
    reason: &str,
) -> Result<(), String> {
    let locator = validated_repository_context(repo_type, repo_project_id, remote_repo)?;
    if pull_request_iid.trim().is_empty() {
        return Err("Pull request number is empty.".to_string());
    }
    match locator.repo_type {
        RepoType::Gitee => {
            gitee::dismiss_pull_request_review(&locator, repo_project_id, pull_request_iid, reason)
                .await
        }
        RepoType::GitHub => {
            github::dismiss_pull_request_review(&locator, pull_request_iid, review_id, reason).await
        }
        RepoType::GitLab => {
            gitlab::dismiss_pull_request_review(&locator, repo_project_id, pull_request_iid, reason)
                .await
        }
    }
}

pub(crate) async fn merge_pull_request(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
    pull_request_iid: &str,
    method: PullRequestMergeMethod,
    remove_source_branch: bool,
) -> Result<(), String> {
    let locator = validated_repository_context(repo_type, repo_project_id, remote_repo)?;
    if pull_request_iid.trim().is_empty() {
        return Err("Pull request number is empty.".to_string());
    }
    match locator.repo_type {
        RepoType::Gitee => {
            gitee::merge_pull_request(
                &locator,
                repo_project_id,
                pull_request_iid,
                method,
                remove_source_branch,
            )
            .await
        }
        RepoType::GitHub => {
            if remove_source_branch {
                log::debug!(
                    "[CodeReview][GitHub] Source-branch deletion is not part of the merge endpoint"
                );
            }
            github::merge_pull_request(&locator, pull_request_iid, method).await
        }
        RepoType::GitLab => {
            gitlab::merge_pull_request(
                &locator,
                repo_project_id,
                pull_request_iid,
                method,
                remove_source_branch,
            )
            .await
        }
    }
}

/// Create a provider-native reverse change for one merged pull request.
///
/// The portable contract is intentionally capability-gated: providers that do
/// not expose a safe server-side operation return an explicit unsupported
/// error instead of reconstructing commit ranges in the client.
pub(crate) async fn revert_pull_request(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
    pull_request_iid: &str,
) -> Result<PullRequestRevertResult, String> {
    let locator = validated_repository_context(repo_type, repo_project_id, remote_repo)?;
    if pull_request_iid.trim().is_empty() {
        return Err("Pull request number is empty.".to_string());
    }
    match locator.repo_type {
        RepoType::GitHub => github::revert_pull_request(&locator, pull_request_iid).await,
        RepoType::Gitee => {
            gitee::revert_pull_request(&locator, repo_project_id, pull_request_iid).await
        }
        RepoType::GitLab => {
            Err("Automated merge-request revert is not implemented for GitLab yet.".to_string())
        }
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

#[cfg(test)]
mod tests {
    use super::{
        PullRequestCommit, PullRequestState, RepoType, commit_count_from_detail,
        conversation_body_to_plain_text, conversation_commit_batch_items,
        conversation_count_from_detail, conversation_merged_item, finalize_conversation_timeline,
        is_redundant_commit_system_note, is_redundant_merge_system_note, parse_repo_locator,
        parse_unified_diff, percent_encode_path, split_commit_message, supports_ignore_whitespace,
    };

    #[test]
    fn reads_badge_counts_for_github_gitlab_and_gitee_detail_shapes() {
        // GitHub PR detail
        assert_eq!(
            commit_count_from_detail(&serde_json::json!({ "commits": 14, "comments": 3 })),
            Some(14)
        );
        assert_eq!(
            conversation_count_from_detail(&serde_json::json!({ "commits": 14, "comments": 3 })),
            Some(3)
        );
        // GitLab MR detail
        assert_eq!(
            commit_count_from_detail(&serde_json::json!({ "commits_count": 2 })),
            Some(2)
        );
        assert_eq!(
            conversation_count_from_detail(&serde_json::json!({ "user_notes_count": 5 })),
            Some(5)
        );
        // Gitee public / enterprise snake_case and camelCase / data-wrapped shapes
        assert_eq!(
            commit_count_from_detail(&serde_json::json!({ "commits_count": 6 })),
            Some(6)
        );
        assert_eq!(
            conversation_count_from_detail(&serde_json::json!({ "comments_count": 1 })),
            Some(1)
        );
        assert_eq!(
            commit_count_from_detail(&serde_json::json!({ "data": { "commitsCount": 9 } })),
            Some(9)
        );
        assert_eq!(
            conversation_count_from_detail(&serde_json::json!({ "data": { "notesCount": 4 } })),
            Some(4)
        );
        assert_eq!(
            conversation_count_from_detail(&serde_json::json!({
                "data": [{ "id": 1 }],
                "note_statistics": { "note_count": 1, "all_note_count": 7 }
            })),
            Some(7)
        );
        // Public Gitee often omits totals entirely — callers must probe, not assume 0.
        assert_eq!(commit_count_from_detail(&serde_json::json!({})), None);
        assert_eq!(conversation_count_from_detail(&serde_json::json!({})), None);
        assert_eq!(
            conversation_count_from_detail(&serde_json::json!({ "comments": 0 })),
            Some(0)
        );
    }

    #[test]
    fn normalizes_provider_markup_for_the_shared_conversation_cards() {
        assert_eq!(
            conversation_body_to_plain_text(
                r#"<p>feat: customer page (<a target="_blank" href="https://example.test/p11">p11_202604100012</a>)</p>"#,
            ),
            "feat: customer page (p11_202604100012)"
        );
        assert_eq!(
            conversation_body_to_plain_text("**Ready**\n\n- first\n- second &amp; final"),
            "Ready\n• first\n• second & final"
        );

        let timeline = finalize_conversation_timeline(
            super::PullRequestConversationItem {
                id: "description-1".into(),
                kind: "provider-description".into(),
                author: "alice".into(),
                avatar_url: String::new(),
                author_association: String::new(),
                body: "<p>Shared <strong>description</strong></p>".into(),
                created_at: "2026-08-10T00:00:00Z".into(),
                state: "open".into(),
                review_id: String::new(),
                can_dismiss: false,
            },
            vec![super::PullRequestConversationItem {
                id: "comment-1".into(),
                kind: "comment".into(),
                author: "bob".into(),
                avatar_url: String::new(),
                author_association: String::new(),
                body: "<p>Shared <em>comment</em></p>".into(),
                created_at: "2026-08-10T00:01:00Z".into(),
                state: String::new(),
                review_id: String::new(),
                can_dismiss: false,
            }],
        );
        assert_eq!(timeline[0].kind, "description");
        assert_eq!(timeline[0].body, "Shared description");
        assert_eq!(timeline[1].body, "Shared comment");
    }

    #[test]
    fn reads_conversation_badge_counts_from_provider_detail_fields() {
        assert_eq!(
            conversation_count_from_detail(&serde_json::json!({ "comments": 2 })),
            Some(2)
        );
        assert_eq!(
            conversation_count_from_detail(&serde_json::json!({ "user_notes_count": "4" })),
            Some(4)
        );
        assert_eq!(
            conversation_count_from_detail(&serde_json::json!({ "notes_count": 1 })),
            Some(1)
        );
        assert_eq!(
            conversation_count_from_detail(&serde_json::json!({
                "data": { "commentsCount": 3 }
            })),
            Some(3)
        );
        assert_eq!(
            conversation_count_from_detail(&serde_json::json!({
                "data": { "note_statistics": { "allNoteCount": "8" } }
            })),
            Some(8)
        );
        assert_eq!(conversation_count_from_detail(&serde_json::json!({})), None);
    }

    #[test]
    fn reads_commit_badge_counts_from_provider_detail_fields() {
        assert_eq!(
            commit_count_from_detail(&serde_json::json!({ "commits": 14 })),
            Some(14)
        );
        assert_eq!(
            commit_count_from_detail(&serde_json::json!({ "commitsCount": "6" })),
            Some(6)
        );
        assert_eq!(
            commit_count_from_detail(&serde_json::json!({
                "data": { "commits_count": 2 }
            })),
            Some(2)
        );
        assert_eq!(commit_count_from_detail(&serde_json::json!({})), None);
    }

    #[test]
    fn builds_github_style_commit_and_merge_conversation_rows() {
        let commits = vec![
            PullRequestCommit {
                sha: "abcdef0123456789".into(),
                title: "first".into(),
                message: "first".into(),
                author: "alice".into(),
                avatar_url: "https://example.test/alice.png".into(),
                authored_at: "2026-01-01T00:00:00Z".into(),
                verified: false,
            },
            PullRequestCommit {
                sha: "1234567890abcdef".into(),
                title: "second".into(),
                message: "second".into(),
                author: "bob".into(),
                avatar_url: "https://example.test/bob.png".into(),
                authored_at: "2026-01-02T00:00:00Z".into(),
                verified: false,
            },
        ];
        let batch = conversation_commit_batch_items(&commits);
        assert_eq!(batch.len(), 3);
        assert_eq!(batch[0].kind, "commits");
        assert_eq!(batch[0].body, "added 2 commits");
        assert_eq!(batch[1].kind, "commit");
        // Display order is newest→oldest while input stays oldest→newest.
        assert_eq!(batch[1].body, "second");
        assert_eq!(batch[1].state, "1234567");
        assert_eq!(batch[1].avatar_url, "https://example.test/bob.png");
        assert_eq!(batch[2].body, "first");
        assert_eq!(batch[2].state, "abcdef0");
        assert_eq!(batch[0].created_at, "2026-01-02T00:00:00Z");
        assert_eq!(batch[1].created_at, batch[0].created_at);
        assert_eq!(batch[2].created_at, "2026-01-01T00:00:00Z");

        let with_body = conversation_commit_batch_items(&[PullRequestCommit {
            sha: "deadbeef".into(),
            title: "subject".into(),
            message: "subject\n\n- detail one\n- detail two".into(),
            author: "alice".into(),
            avatar_url: String::new(),
            authored_at: "2026-01-01T00:00:00Z".into(),
            verified: false,
        }]);
        assert_eq!(with_body[1].body, "subject\n\n- detail one\n- detail two");
        assert_eq!(
            split_commit_message(with_body[1].body.as_str()),
            ("subject".into(), "- detail one\n- detail two".into())
        );

        let merged =
            conversation_merged_item("carol", "54037a9deadbeef", "main", "2026-01-03T00:00:00Z");
        assert_eq!(merged.kind, "merged");
        assert_eq!(merged.body, "54037a9");
        assert_eq!(merged.state, "main");

        assert!(is_redundant_commit_system_note(
            "added 2 commits\n<ul><li>x</li></ul>"
        ));
        assert!(is_redundant_merge_system_note("merged"));
        assert!(!is_redundant_merge_system_note("changed the description"));

        let timeline = finalize_conversation_timeline(
            super::PullRequestConversationItem {
                id: "description-1".into(),
                kind: "description".into(),
                author: "alice".into(),
                avatar_url: String::new(),
                author_association: String::new(),
                body: "hello".into(),
                created_at: "2025-12-31T00:00:00Z".into(),
                state: "open".into(),
                review_id: String::new(),
                can_dismiss: false,
            },
            {
                let mut activity = batch;
                activity.push(merged);
                activity
            },
        );
        assert_eq!(timeline[0].kind, "description");
        assert_eq!(
            timeline
                .iter()
                .map(|item| item.kind.as_str())
                .collect::<Vec<_>>(),
            vec!["description", "commits", "commit", "commit", "merged"]
        );
    }

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
