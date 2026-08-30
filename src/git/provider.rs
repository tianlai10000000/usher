use async_trait::async_trait;

use super::{
    EnvironmentPullRequestCounts, GiteeApiProfile, PullRequestActionState, PullRequestCommit,
    PullRequestConversationItem, PullRequestDiffDetail, PullRequestFilters, PullRequestMergeMethod,
    PullRequestPage, PullRequestRevertResult, PullRequestReviewAction, PullRequestState,
    RepoLocator, RepoType, gitee, gitee_api_profile, github, gitlab, overview, parse_repo_locator,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ProviderCapabilities {
    pub(super) ignore_whitespace: bool,
    pub(super) server_side_revert: bool,
}

#[derive(Debug, Clone)]
pub(super) struct RepoContext {
    locator: RepoLocator,
    project_id: String,
}

impl RepoContext {
    pub(super) fn discover(remote_repo: &str) -> Result<Self, String> {
        Ok(Self {
            locator: parse_repo_locator(remote_repo)?,
            project_id: String::new(),
        })
    }

    pub(super) fn from_saved(
        repo_type: &str,
        repo_project_id: &str,
        remote_repo: &str,
    ) -> Result<Self, String> {
        let context = Self::discover(remote_repo)?;
        if !repo_type
            .trim()
            .eq_ignore_ascii_case(context.locator.repo_type.as_str())
        {
            return Err(
                "Saved repository provider does not match the remote repository URL.".to_string(),
            );
        }
        let project_id = repo_project_id.trim();
        if project_id.is_empty() {
            return Err("This project has no remote project id.".to_string());
        }
        Ok(Self {
            locator: context.locator,
            project_id: project_id.to_string(),
        })
    }

    pub(super) fn locator(&self) -> &RepoLocator {
        &self.locator
    }

    pub(super) fn project_id(&self) -> &str {
        self.project_id.as_str()
    }

    pub(super) fn repo_type(&self) -> RepoType {
        self.locator.repo_type
    }

    pub(super) fn provider(&self) -> &'static dyn GitProvider {
        provider(self.repo_type())
    }

    pub(super) fn capabilities(&self) -> ProviderCapabilities {
        self.provider().capabilities(self)
    }
}

#[async_trait]
pub(super) trait GitProvider: Sync {
    fn capabilities(&self, context: &RepoContext) -> ProviderCapabilities;

    async fn repo_detail(&self, context: &RepoContext) -> Result<String, String>;

    async fn pull_requests(
        &self,
        context: &RepoContext,
        state: Option<PullRequestState>,
        filters: &PullRequestFilters,
        page: u32,
        per_page: u32,
    ) -> Result<PullRequestPage, String>;

    async fn standard_environment_counts(
        &self,
        context: &RepoContext,
    ) -> Result<EnvironmentPullRequestCounts, String>;

    async fn pull_request_diff(
        &self,
        context: &RepoContext,
        pull_request_iid: &str,
        ignore_whitespace_change: bool,
    ) -> Result<PullRequestDiffDetail, String>;

    async fn commit_range_diff(
        &self,
        context: &RepoContext,
        base_sha: &str,
        head_sha: &str,
        commit_count: u32,
        ignore_whitespace_change: bool,
    ) -> Result<PullRequestDiffDetail, String>;

    async fn repository_file_text(
        &self,
        context: &RepoContext,
        path: &str,
        reference: &str,
    ) -> Result<String, String>;

    async fn pull_request_action_state(
        &self,
        context: &RepoContext,
        pull_request_iid: &str,
    ) -> Result<PullRequestActionState, String>;

    async fn pull_request_conversation(
        &self,
        context: &RepoContext,
        pull_request_iid: &str,
    ) -> Result<Vec<PullRequestConversationItem>, String>;

    async fn pull_request_commits(
        &self,
        context: &RepoContext,
        pull_request_iid: &str,
    ) -> Result<Vec<PullRequestCommit>, String>;

    async fn submit_pull_request_review(
        &self,
        context: &RepoContext,
        pull_request_iid: &str,
        action: PullRequestReviewAction,
        body: &str,
    ) -> Result<(), String>;

    async fn dismiss_pull_request_review(
        &self,
        context: &RepoContext,
        pull_request_iid: &str,
        review_id: &str,
        reason: &str,
    ) -> Result<(), String>;

    async fn merge_pull_request(
        &self,
        context: &RepoContext,
        pull_request_iid: &str,
        method: PullRequestMergeMethod,
        remove_source_branch: bool,
    ) -> Result<(), String>;

    async fn revert_pull_request(
        &self,
        context: &RepoContext,
        pull_request_iid: &str,
    ) -> Result<PullRequestRevertResult, String> {
        if context.repo_type() == RepoType::Gitee {
            gitee::revert_pull_request(context.locator(), context.project_id(), pull_request_iid)
                .await
        } else {
            Err(
                "Automated merge-request revert is not implemented for this provider yet."
                    .to_string(),
            )
        }
    }
}

struct GitHubProvider;
struct GitLabProvider;
struct GiteeProvider;

static GITHUB: GitHubProvider = GitHubProvider;
static GITLAB: GitLabProvider = GitLabProvider;
static GITEE: GiteeProvider = GiteeProvider;

fn provider(repo_type: RepoType) -> &'static dyn GitProvider {
    match repo_type {
        RepoType::GitHub => &GITHUB,
        RepoType::GitLab => &GITLAB,
        RepoType::Gitee => &GITEE,
    }
}

#[async_trait]
impl GitProvider for GitHubProvider {
    fn capabilities(&self, _context: &RepoContext) -> ProviderCapabilities {
        ProviderCapabilities {
            ignore_whitespace: false,
            server_side_revert: true,
        }
    }

    async fn repo_detail(&self, context: &RepoContext) -> Result<String, String> {
        github::get_repo_detail(context.locator()).await
    }

    async fn pull_requests(
        &self,
        context: &RepoContext,
        state: Option<PullRequestState>,
        filters: &PullRequestFilters,
        page: u32,
        per_page: u32,
    ) -> Result<PullRequestPage, String> {
        github::get_pull_requests(context.locator(), state, filters, page, per_page).await
    }

    async fn standard_environment_counts(
        &self,
        context: &RepoContext,
    ) -> Result<EnvironmentPullRequestCounts, String> {
        let (fat, uat, prod) = tokio::try_join!(
            github::get_open_pull_request_count(context.locator(), "fat"),
            github::get_open_pull_request_count(context.locator(), "uat"),
            github::get_open_pull_request_count(context.locator(), "prod"),
        )?;
        Ok(EnvironmentPullRequestCounts { fat, uat, prod })
    }

    async fn pull_request_diff(
        &self,
        context: &RepoContext,
        pull_request_iid: &str,
        ignore_whitespace_change: bool,
    ) -> Result<PullRequestDiffDetail, String> {
        github::get_pull_request_diff(
            context.locator(),
            pull_request_iid,
            ignore_whitespace_change,
        )
        .await
    }

    async fn commit_range_diff(
        &self,
        context: &RepoContext,
        base_sha: &str,
        head_sha: &str,
        commit_count: u32,
        _ignore_whitespace_change: bool,
    ) -> Result<PullRequestDiffDetail, String> {
        github::get_commit_range_diff(context.locator(), base_sha, head_sha, commit_count).await
    }

    async fn repository_file_text(
        &self,
        context: &RepoContext,
        path: &str,
        reference: &str,
    ) -> Result<String, String> {
        github::get_repository_file_text(context.locator(), path, reference).await
    }

    async fn pull_request_action_state(
        &self,
        context: &RepoContext,
        pull_request_iid: &str,
    ) -> Result<PullRequestActionState, String> {
        github::get_pull_request_action_state(context.locator(), pull_request_iid).await
    }

    async fn pull_request_conversation(
        &self,
        context: &RepoContext,
        pull_request_iid: &str,
    ) -> Result<Vec<PullRequestConversationItem>, String> {
        github::get_pull_request_conversation(context.locator(), pull_request_iid).await
    }

    async fn pull_request_commits(
        &self,
        context: &RepoContext,
        pull_request_iid: &str,
    ) -> Result<Vec<PullRequestCommit>, String> {
        github::get_pull_request_commits(context.locator(), pull_request_iid).await
    }

    async fn submit_pull_request_review(
        &self,
        context: &RepoContext,
        pull_request_iid: &str,
        action: PullRequestReviewAction,
        body: &str,
    ) -> Result<(), String> {
        github::submit_pull_request_review(context.locator(), pull_request_iid, action, body).await
    }

    async fn dismiss_pull_request_review(
        &self,
        context: &RepoContext,
        pull_request_iid: &str,
        review_id: &str,
        reason: &str,
    ) -> Result<(), String> {
        github::dismiss_pull_request_review(context.locator(), pull_request_iid, review_id, reason)
            .await
    }

    async fn merge_pull_request(
        &self,
        context: &RepoContext,
        pull_request_iid: &str,
        method: PullRequestMergeMethod,
        remove_source_branch: bool,
    ) -> Result<(), String> {
        if remove_source_branch {
            log::debug!(
                "[CodeReview][GitHub] Source-branch deletion is not part of the merge endpoint"
            );
        }
        github::merge_pull_request(context.locator(), pull_request_iid, method).await
    }

    async fn revert_pull_request(
        &self,
        context: &RepoContext,
        pull_request_iid: &str,
    ) -> Result<PullRequestRevertResult, String> {
        github::revert_pull_request(context.locator(), pull_request_iid).await
    }
}

macro_rules! impl_project_provider {
    ($provider:ty, $module:ident, $capabilities:expr) => {
        #[async_trait]
        impl GitProvider for $provider {
            fn capabilities(&self, context: &RepoContext) -> ProviderCapabilities {
                ($capabilities)(context)
            }

            async fn repo_detail(&self, context: &RepoContext) -> Result<String, String> {
                $module::get_repo_detail(context.locator()).await
            }

            async fn pull_requests(
                &self,
                context: &RepoContext,
                state: Option<PullRequestState>,
                filters: &PullRequestFilters,
                page: u32,
                per_page: u32,
            ) -> Result<PullRequestPage, String> {
                $module::get_pull_requests(
                    context.locator(),
                    context.project_id(),
                    state,
                    filters,
                    page,
                    per_page,
                )
                .await
            }

            async fn standard_environment_counts(
                &self,
                context: &RepoContext,
            ) -> Result<EnvironmentPullRequestCounts, String> {
                let filters = PullRequestFilters::default();
                overview::count_standard_environment_pages(|page, per_page| {
                    $module::get_pull_requests(
                        context.locator(),
                        context.project_id(),
                        Some(PullRequestState::Open),
                        &filters,
                        page,
                        per_page,
                    )
                })
                .await
            }

            async fn pull_request_diff(
                &self,
                context: &RepoContext,
                pull_request_iid: &str,
                ignore_whitespace_change: bool,
            ) -> Result<PullRequestDiffDetail, String> {
                $module::get_pull_request_diff(
                    context.locator(),
                    context.project_id(),
                    pull_request_iid,
                    ignore_whitespace_change,
                )
                .await
            }

            async fn commit_range_diff(
                &self,
                context: &RepoContext,
                base_sha: &str,
                head_sha: &str,
                commit_count: u32,
                ignore_whitespace_change: bool,
            ) -> Result<PullRequestDiffDetail, String> {
                $module::get_commit_range_diff(
                    context.locator(),
                    context.project_id(),
                    base_sha,
                    head_sha,
                    commit_count,
                    ignore_whitespace_change,
                )
                .await
            }

            async fn repository_file_text(
                &self,
                context: &RepoContext,
                path: &str,
                reference: &str,
            ) -> Result<String, String> {
                $module::get_repository_file_text(
                    context.locator(),
                    context.project_id(),
                    path,
                    reference,
                )
                .await
            }

            async fn pull_request_action_state(
                &self,
                context: &RepoContext,
                pull_request_iid: &str,
            ) -> Result<PullRequestActionState, String> {
                $module::get_pull_request_action_state(
                    context.locator(),
                    context.project_id(),
                    pull_request_iid,
                )
                .await
            }

            async fn pull_request_conversation(
                &self,
                context: &RepoContext,
                pull_request_iid: &str,
            ) -> Result<Vec<PullRequestConversationItem>, String> {
                $module::get_pull_request_conversation(
                    context.locator(),
                    context.project_id(),
                    pull_request_iid,
                )
                .await
            }

            async fn pull_request_commits(
                &self,
                context: &RepoContext,
                pull_request_iid: &str,
            ) -> Result<Vec<PullRequestCommit>, String> {
                $module::get_pull_request_commits(
                    context.locator(),
                    context.project_id(),
                    pull_request_iid,
                )
                .await
            }

            async fn submit_pull_request_review(
                &self,
                context: &RepoContext,
                pull_request_iid: &str,
                action: PullRequestReviewAction,
                body: &str,
            ) -> Result<(), String> {
                $module::submit_pull_request_review(
                    context.locator(),
                    context.project_id(),
                    pull_request_iid,
                    action,
                    body,
                )
                .await
            }

            async fn dismiss_pull_request_review(
                &self,
                context: &RepoContext,
                pull_request_iid: &str,
                _review_id: &str,
                reason: &str,
            ) -> Result<(), String> {
                $module::dismiss_pull_request_review(
                    context.locator(),
                    context.project_id(),
                    pull_request_iid,
                    reason,
                )
                .await
            }

            async fn merge_pull_request(
                &self,
                context: &RepoContext,
                pull_request_iid: &str,
                method: PullRequestMergeMethod,
                remove_source_branch: bool,
            ) -> Result<(), String> {
                $module::merge_pull_request(
                    context.locator(),
                    context.project_id(),
                    pull_request_iid,
                    method,
                    remove_source_branch,
                )
                .await
            }
        }
    };
}

impl_project_provider!(GitLabProvider, gitlab, |_context: &RepoContext| {
    ProviderCapabilities::default()
});

impl_project_provider!(GiteeProvider, gitee, |context: &RepoContext| {
    let enterprise = gitee_api_profile(context.locator()) == GiteeApiProfile::EnterpriseV8;
    ProviderCapabilities {
        ignore_whitespace: enterprise,
        server_side_revert: enterprise,
    }
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_context_rejects_provider_mismatch_and_missing_id() {
        let mismatch = RepoContext::from_saved("gitlab", "42", "https://github.com/acme/app");
        assert!(mismatch.unwrap_err().contains("does not match"));

        let missing = RepoContext::from_saved("github", "", "https://github.com/acme/app");
        assert!(missing.unwrap_err().contains("no remote project id"));
    }

    #[test]
    fn github_capabilities_are_selected_from_the_validated_context() {
        let context =
            RepoContext::from_saved("github", "42", "https://github.com/acme/app").unwrap();
        assert_eq!(context.repo_type(), RepoType::GitHub);
        assert_eq!(
            context.capabilities(),
            ProviderCapabilities {
                ignore_whitespace: false,
                server_side_revert: true,
            }
        );
    }
}
