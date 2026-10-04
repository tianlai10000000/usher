//! Provider-neutral Git repository operations and response models.

pub(crate) mod conflicts;
mod diff;
mod error;
pub(crate) mod gitee;
mod github;
mod gitlab;
mod json;
mod locator;
mod model;
mod overview;
mod prefix_cache;
mod provider;
mod response_budget;
#[cfg(test)]
mod response_budget_tests;
mod source;

use diff::{parse_unified_diff, unavailable_diff_file};
pub(crate) use error::{GitError, GitResult};
use locator::*;
pub(crate) use model::*;
pub(crate) use overview::EnvironmentPullRequestCounts;
pub(crate) use provider::ProviderCapabilities;
use provider::{ProjectContext, RepoContext};

/// Query the configured provider for the immutable numeric/string project id.
///
/// The remote URL is the source of truth for both provider selection and the
/// namespace/repository used by provider APIs.
pub(crate) async fn get_repo_detail(remote_repo: &str) -> GitResult<RepoDetail> {
    let context = RepoContext::discover(remote_repo)?;
    let repo_project_id = context
        .provider()
        .repo_detail(&context)
        .await
        .map_err(|error| GitError::provider("load repository details", error))?;

    Ok(RepoDetail {
        repo_project_id,
        repo_type: context.repo_type(),
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
) -> GitResult<PullRequestPage> {
    let context = ProjectContext::from_saved(repo_type, repo_project_id, remote_repo)?;
    context
        .provider()
        .pull_requests(&context, state, filters, page, per_page)
        .await
        .map_err(|error| GitError::provider("load pull requests", error))
}

/// Count open pull requests targeting the standard delivery environments.
///
/// Provider selection, persisted-id validation, authentication, endpoint
/// selection, and response normalization stay inside the repository service.
pub(crate) async fn get_standard_environment_pull_request_counts(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
) -> GitResult<EnvironmentPullRequestCounts> {
    let context = ProjectContext::from_saved(repo_type, repo_project_id, remote_repo)?;
    context
        .provider()
        .standard_environment_counts(&context)
        .await
        .map_err(|error| GitError::provider("load pull request counts", error))
}

/// Load the review diff for one pull request. Provider-specific response
/// shapes remain inside the provider modules under `src/git/` and are normalized here.
pub(crate) async fn get_pull_request_diff(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
    pull_request_iid: &str,
    ignore_whitespace_change: bool,
) -> GitResult<PullRequestDiffDetail> {
    let context = ProjectContext::from_saved(repo_type, repo_project_id, remote_repo)?;
    if pull_request_iid.trim().is_empty() {
        return Err(GitError::configuration("Pull request number is empty."));
    }
    context
        .provider()
        .pull_request_diff(&context, pull_request_iid, ignore_whitespace_change)
        .await
        .map_err(|error| GitError::provider("load pull request diff", error))
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
) -> GitResult<PullRequestDiffDetail> {
    let context = ProjectContext::from_saved(repo_type, repo_project_id, remote_repo)?;
    if base_sha.trim().is_empty() || head_sha.trim().is_empty() {
        return Err(GitError::configuration("Commit range is incomplete."));
    }

    context
        .provider()
        .commit_range_diff(
            &context,
            base_sha,
            head_sha,
            commit_count,
            ignore_whitespace_change,
        )
        .await
        .map_err(|error| GitError::provider("load commit range diff", error))
}

/// Whether the configured provider endpoint can filter whitespace-only diff
/// changes. Currently this is part of the Gitee-compatible enterprise diff
/// contract only; public Gitee, GitHub, and GitLab adapters do not support it.
pub(crate) fn provider_capabilities(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
) -> GitResult<ProviderCapabilities> {
    Ok(ProjectContext::from_saved(repo_type, repo_project_id, remote_repo)?.capabilities())
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
) -> GitResult<String> {
    let context = ProjectContext::from_saved(repo_type, repo_project_id, remote_repo)?;
    if path.trim().is_empty() || reference.trim().is_empty() {
        return Err(GitError::configuration(
            "Repository file path or commit reference is empty.",
        ));
    }

    context
        .provider()
        .repository_file_text(&context, path, reference)
        .await
        .map_err(|error| GitError::provider("load repository file", error))
}

pub(crate) async fn get_pull_request_action_state(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
    pull_request_iid: &str,
) -> GitResult<PullRequestActionState> {
    let context = ProjectContext::from_saved(repo_type, repo_project_id, remote_repo)?;
    if pull_request_iid.trim().is_empty() {
        return Err(GitError::configuration("Pull request number is empty."));
    }
    context
        .provider()
        .pull_request_action_state(&context, pull_request_iid)
        .await
        .map_err(|error| GitError::provider("load pull request action state", error))
}

pub(crate) async fn get_pull_request_conversation(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
    pull_request_iid: &str,
) -> GitResult<Vec<PullRequestConversationItem>> {
    let context = ProjectContext::from_saved(repo_type, repo_project_id, remote_repo)?;
    if pull_request_iid.trim().is_empty() {
        return Err(GitError::configuration("Pull request number is empty."));
    }
    context
        .provider()
        .pull_request_conversation(&context, pull_request_iid)
        .await
        .map_err(|error| GitError::provider("load pull request conversation", error))
}

pub(crate) async fn get_pull_request_commits(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
    pull_request_iid: &str,
) -> GitResult<Vec<PullRequestCommit>> {
    let context = ProjectContext::from_saved(repo_type, repo_project_id, remote_repo)?;
    if pull_request_iid.trim().is_empty() {
        return Err(GitError::configuration("Pull request number is empty."));
    }
    context
        .provider()
        .pull_request_commits(&context, pull_request_iid)
        .await
        .map_err(|error| GitError::provider("load pull request commits", error))
}

pub(crate) async fn submit_pull_request_review(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
    pull_request_iid: &str,
    action: PullRequestReviewAction,
    body: &str,
) -> GitResult<()> {
    let context = ProjectContext::from_saved(repo_type, repo_project_id, remote_repo)?;
    if pull_request_iid.trim().is_empty() {
        return Err(GitError::configuration("Pull request number is empty."));
    }
    context
        .provider()
        .submit_pull_request_review(&context, pull_request_iid, action, body)
        .await
        .map_err(|error| GitError::provider("submit pull request review", error))
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
) -> GitResult<()> {
    let context = ProjectContext::from_saved(repo_type, repo_project_id, remote_repo)?;
    if pull_request_iid.trim().is_empty() {
        return Err(GitError::configuration("Pull request number is empty."));
    }
    context
        .provider()
        .dismiss_pull_request_review(&context, pull_request_iid, review_id, reason)
        .await
        .map_err(|error| GitError::provider("dismiss pull request review", error))
}

pub(crate) async fn merge_pull_request(
    repo_type: &str,
    repo_project_id: &str,
    remote_repo: &str,
    pull_request_iid: &str,
    method: PullRequestMergeMethod,
    remove_source_branch: bool,
) -> GitResult<()> {
    let context = ProjectContext::from_saved(repo_type, repo_project_id, remote_repo)?;
    if pull_request_iid.trim().is_empty() {
        return Err(GitError::configuration("Pull request number is empty."));
    }
    if remove_source_branch && !context.capabilities().merge_delete_source_branch {
        return Err(GitError::unsupported(
            "merge pull request",
            "The configured provider cannot delete the source branch through its merge endpoint.",
        ));
    }
    context
        .provider()
        .merge_pull_request(&context, pull_request_iid, method, remove_source_branch)
        .await
        .map_err(|error| GitError::provider("merge pull request", error))
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
) -> GitResult<PullRequestRevertResult> {
    let context = ProjectContext::from_saved(repo_type, repo_project_id, remote_repo)?;
    if pull_request_iid.trim().is_empty() {
        return Err(GitError::configuration("Pull request number is empty."));
    }
    if !context.capabilities().server_side_revert {
        return Err(GitError::unsupported(
            "revert pull request",
            format!(
                "Automated merge-request revert is not implemented for {}.",
                context.repo_type()
            ),
        ));
    }
    context
        .provider()
        .revert_pull_request(&context, pull_request_iid)
        .await
        .map_err(|error| GitError::provider("revert pull request", error))
}

#[cfg(test)]
mod tests {
    use super::{
        PageCollector, PullRequestCommit, PullRequestState, RepoType, commit_count_from_detail,
        conversation_body_to_plain_text, conversation_commit_batch_items,
        conversation_count_from_detail, conversation_merged_item, finalize_conversation_timeline,
        is_redundant_commit_system_note, is_redundant_merge_system_note, parse_repo_locator,
        parse_unified_diff, percent_encode_path, provider_capabilities, split_commit_message,
    };

    #[test]
    fn page_collector_marks_a_full_final_page_as_truncated() {
        let mut collector = PageCollector::new(2, 2);
        assert!(collector.append(vec![1, 2]));
        assert_eq!(collector.page(), 2);
        assert!(!collector.append(vec![3, 4]));
        let (values, truncated) = collector.into_parts();
        assert_eq!(values, vec![1, 2, 3, 4]);
        assert!(truncated);

        let mut complete = PageCollector::new(2, 2);
        assert!(complete.append(vec![1, 2]));
        assert!(!complete.append(vec![3]));
        assert!(!complete.into_parts().1);
    }

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
    fn sorts_conversation_timestamps_by_instant_and_keeps_unknown_times_last() {
        let item = |id: &str, kind: &str, created_at: &str| super::PullRequestConversationItem {
            id: id.into(),
            kind: kind.into(),
            author: "alice".into(),
            avatar_url: String::new(),
            author_association: String::new(),
            body: id.into(),
            created_at: created_at.into(),
            state: String::new(),
            review_id: String::new(),
            can_dismiss: false,
        };
        let timeline = finalize_conversation_timeline(
            item("description", "description", "2026-08-10T00:00:00Z"),
            vec![
                item("later", "comment", "2026-08-10T08:30:00+08:00"),
                item("same-a", "comment", "2026-08-10T00:00:00Z"),
                item("same-b", "comment", "2026-08-10T08:00:00+08:00"),
                item("invalid", "comment", "not-a-timestamp"),
                item("empty", "comment", ""),
            ],
        );

        assert_eq!(
            timeline
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "description",
                "same-a",
                "same-b",
                "later",
                "invalid",
                "empty"
            ]
        );
    }

    #[test]
    fn sorts_review_block_by_review_time_when_summary_time_is_missing() {
        let item = |id: &str, kind: &str, created_at: &str| super::PullRequestConversationItem {
            id: id.into(),
            kind: kind.into(),
            author: "alice".into(),
            avatar_url: String::new(),
            author_association: String::new(),
            body: id.into(),
            created_at: created_at.into(),
            state: String::new(),
            review_id: String::new(),
            can_dismiss: false,
        };
        let timeline = finalize_conversation_timeline(
            item("description", "description", "2026-08-10T00:00:00Z"),
            vec![
                item("middle", "comment", "2026-08-10T00:02:00Z"),
                item("summary", "review_summary", ""),
                item("review", "review", "2026-08-10T00:01:00Z"),
            ],
        );

        assert_eq!(
            timeline
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["description", "summary", "review", "middle"]
        );
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
                committer: "alice".into(),
                committer_avatar_url: "https://example.test/alice.png".into(),
                committed_at: "2026-01-01T00:00:00Z".into(),
                verified: false,
            },
            PullRequestCommit {
                sha: "1234567890abcdef".into(),
                title: "second".into(),
                message: "second".into(),
                author: "bob".into(),
                avatar_url: "https://example.test/bob.png".into(),
                authored_at: "2026-01-02T00:00:00Z".into(),
                committer: "bob".into(),
                committer_avatar_url: "https://example.test/bob.png".into(),
                committed_at: "2026-01-02T00:00:00Z".into(),
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
            committer: "alice".into(),
            committer_avatar_url: String::new(),
            committed_at: "2026-01-01T00:00:00Z".into(),
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
        let github =
            provider_capabilities("github", "42", "https://github.com/acme/widget.git").unwrap();
        assert!(!github.pull_request_ignore_whitespace);
        assert!(!github.merge_delete_source_branch);
        assert!(github.dismiss_review_by_id);

        assert!(
            provider_capabilities("gitee", "42", "https://github.com/acme/widget.git").is_err()
        );
    }

    #[test]
    fn parses_unified_diff_hunks_for_provider_normalization() {
        let lines =
            parse_unified_diff("@@ -4,2 +4,3 @@ fn example() {\n context\n-old\n+new\n+extra");
        assert_eq!(lines[0].kind, crate::git::DiffLineKind::Hunk);
        assert_eq!((lines[1].old_line, lines[1].new_line), (4, 4));
        assert_eq!((lines[2].old_line, lines[2].new_line), (5, 0));
        assert_eq!((lines[3].old_line, lines[3].new_line), (0, 5));
        assert_eq!((lines[4].old_line, lines[4].new_line), (0, 6));
    }

    #[test]
    fn diff_domain_aliases_normalize_at_the_provider_boundary() {
        assert_eq!(
            crate::git::DiffLineKind::from_provider("new"),
            crate::git::DiffLineKind::Addition
        );
        assert_eq!(
            crate::git::DiffLineKind::from_provider("old"),
            crate::git::DiffLineKind::Deletion
        );
        assert_eq!(
            crate::git::DiffLineKind::from_provider("match"),
            crate::git::DiffLineKind::Hunk
        );
        assert_eq!(
            crate::git::DiffLineKind::from_provider("unknown"),
            crate::git::DiffLineKind::Context
        );
        assert_eq!(
            crate::git::DiffChangeType::from_provider("added"),
            crate::git::DiffChangeType::New
        );
        assert_eq!(
            crate::git::DiffChangeType::from_provider("removed"),
            crate::git::DiffChangeType::Deleted
        );
        assert_eq!(
            crate::git::DiffChangeType::from_provider("renamed"),
            crate::git::DiffChangeType::Renamed
        );
        assert_eq!(
            crate::git::DiffChangeType::from_provider("modified"),
            crate::git::DiffChangeType::Modified
        );
    }
}
