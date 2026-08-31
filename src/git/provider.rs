use async_trait::async_trait;

use super::{
    EnvironmentPullRequestCounts, GitError, GitResult, GiteeApiProfile, PullRequestActionState,
    PullRequestCommit, PullRequestConversationItem, PullRequestDiffDetail, PullRequestFilters,
    PullRequestMergeMethod, PullRequestPage, PullRequestRevertResult, PullRequestReviewAction,
    PullRequestState, RepoLocator, RepoType, gitee, gitee_api_profile, github, gitlab, overview,
    parse_repo_locator,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ProviderCapabilities {
    pub(crate) pull_request_ignore_whitespace: bool,
    pub(crate) commit_range_ignore_whitespace: bool,
    pub(crate) merge_delete_source_branch: bool,
    pub(crate) dismiss_review_by_id: bool,
    pub(crate) dismiss_own_review: bool,
    pub(crate) server_side_revert: bool,
}

#[derive(Debug, Clone)]
pub(super) struct RepoContext {
    locator: RepoLocator,
}

impl RepoContext {
    pub(super) fn discover(remote_repo: &str) -> GitResult<Self> {
        Ok(Self {
            locator: parse_repo_locator(remote_repo).map_err(GitError::configuration)?,
        })
    }

    pub(super) fn locator(&self) -> &RepoLocator {
        &self.locator
    }

    pub(super) fn repo_type(&self) -> RepoType {
        self.locator.repo_type
    }

    pub(super) fn provider(&self) -> &'static dyn GitProvider {
        provider(self.repo_type())
    }
}

#[derive(Debug, Clone)]
pub(super) struct ProjectContext {
    locator: RepoLocator,
    project_id: String,
    capabilities: ProviderCapabilities,
}

impl ProjectContext {
    pub(super) fn from_saved(
        repo_type: &str,
        repo_project_id: &str,
        remote_repo: &str,
    ) -> GitResult<Self> {
        let context = RepoContext::discover(remote_repo)?;
        if !repo_type
            .trim()
            .eq_ignore_ascii_case(context.locator.repo_type.as_str())
        {
            return Err(GitError::configuration(
                "Saved repository provider does not match the remote repository URL.",
            ));
        }
        let project_id = repo_project_id.trim();
        if project_id.is_empty() {
            return Err(GitError::configuration(
                "This project has no remote project id.",
            ));
        }
        let capabilities = provider(context.repo_type()).capabilities(context.locator());
        Ok(Self {
            locator: context.locator,
            project_id: project_id.to_string(),
            capabilities,
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
        self.capabilities
    }
}

#[async_trait]
pub(super) trait GitProvider: Sync {
    fn capabilities(&self, locator: &RepoLocator) -> ProviderCapabilities;

    async fn repo_detail(&self, context: &RepoContext) -> Result<String, String>;

    async fn pull_requests(
        &self,
        context: &ProjectContext,
        state: Option<PullRequestState>,
        filters: &PullRequestFilters,
        page: u32,
        per_page: u32,
    ) -> Result<PullRequestPage, String>;

    async fn standard_environment_counts(
        &self,
        context: &ProjectContext,
    ) -> Result<EnvironmentPullRequestCounts, String>;

    async fn pull_request_diff(
        &self,
        context: &ProjectContext,
        pull_request_iid: &str,
        ignore_whitespace_change: bool,
    ) -> Result<PullRequestDiffDetail, String>;

    async fn commit_range_diff(
        &self,
        context: &ProjectContext,
        base_sha: &str,
        head_sha: &str,
        commit_count: u32,
        ignore_whitespace_change: bool,
    ) -> Result<PullRequestDiffDetail, String>;

    async fn repository_file_text(
        &self,
        context: &ProjectContext,
        path: &str,
        reference: &str,
    ) -> Result<String, String>;

    async fn pull_request_action_state(
        &self,
        context: &ProjectContext,
        pull_request_iid: &str,
    ) -> Result<PullRequestActionState, String>;

    async fn pull_request_conversation(
        &self,
        context: &ProjectContext,
        pull_request_iid: &str,
    ) -> Result<Vec<PullRequestConversationItem>, String>;

    async fn pull_request_commits(
        &self,
        context: &ProjectContext,
        pull_request_iid: &str,
    ) -> Result<Vec<PullRequestCommit>, String>;

    async fn submit_pull_request_review(
        &self,
        context: &ProjectContext,
        pull_request_iid: &str,
        action: PullRequestReviewAction,
        body: &str,
    ) -> Result<(), String>;

    async fn dismiss_pull_request_review(
        &self,
        context: &ProjectContext,
        pull_request_iid: &str,
        review_id: &str,
        reason: &str,
    ) -> Result<(), String>;

    async fn merge_pull_request(
        &self,
        context: &ProjectContext,
        pull_request_iid: &str,
        method: PullRequestMergeMethod,
        remove_source_branch: bool,
    ) -> Result<(), String>;

    async fn revert_pull_request(
        &self,
        context: &ProjectContext,
        pull_request_iid: &str,
    ) -> Result<PullRequestRevertResult, String>;
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
    fn capabilities(&self, _locator: &RepoLocator) -> ProviderCapabilities {
        ProviderCapabilities {
            dismiss_review_by_id: true,
            server_side_revert: true,
            ..ProviderCapabilities::default()
        }
    }

    async fn repo_detail(&self, context: &RepoContext) -> Result<String, String> {
        github::get_repo_detail(context.locator()).await
    }

    async fn pull_requests(
        &self,
        context: &ProjectContext,
        state: Option<PullRequestState>,
        filters: &PullRequestFilters,
        page: u32,
        per_page: u32,
    ) -> Result<PullRequestPage, String> {
        github::get_pull_requests(context.locator(), state, filters, page, per_page).await
    }

    async fn standard_environment_counts(
        &self,
        context: &ProjectContext,
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
        context: &ProjectContext,
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
        context: &ProjectContext,
        base_sha: &str,
        head_sha: &str,
        commit_count: u32,
        _ignore_whitespace_change: bool,
    ) -> Result<PullRequestDiffDetail, String> {
        github::get_commit_range_diff(context.locator(), base_sha, head_sha, commit_count).await
    }

    async fn repository_file_text(
        &self,
        context: &ProjectContext,
        path: &str,
        reference: &str,
    ) -> Result<String, String> {
        github::get_repository_file_text(context.locator(), path, reference).await
    }

    async fn pull_request_action_state(
        &self,
        context: &ProjectContext,
        pull_request_iid: &str,
    ) -> Result<PullRequestActionState, String> {
        github::get_pull_request_action_state(context.locator(), pull_request_iid).await
    }

    async fn pull_request_conversation(
        &self,
        context: &ProjectContext,
        pull_request_iid: &str,
    ) -> Result<Vec<PullRequestConversationItem>, String> {
        github::get_pull_request_conversation(context.locator(), pull_request_iid).await
    }

    async fn pull_request_commits(
        &self,
        context: &ProjectContext,
        pull_request_iid: &str,
    ) -> Result<Vec<PullRequestCommit>, String> {
        github::get_pull_request_commits(context.locator(), pull_request_iid).await
    }

    async fn submit_pull_request_review(
        &self,
        context: &ProjectContext,
        pull_request_iid: &str,
        action: PullRequestReviewAction,
        body: &str,
    ) -> Result<(), String> {
        github::submit_pull_request_review(context.locator(), pull_request_iid, action, body).await
    }

    async fn dismiss_pull_request_review(
        &self,
        context: &ProjectContext,
        pull_request_iid: &str,
        review_id: &str,
        reason: &str,
    ) -> Result<(), String> {
        github::dismiss_pull_request_review(context.locator(), pull_request_iid, review_id, reason)
            .await
    }

    async fn merge_pull_request(
        &self,
        context: &ProjectContext,
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
        context: &ProjectContext,
        pull_request_iid: &str,
    ) -> Result<PullRequestRevertResult, String> {
        github::revert_pull_request(context.locator(), pull_request_iid).await
    }
}

macro_rules! impl_project_provider {
    ($provider:ty, $module:ident, $capabilities:expr) => {
        #[async_trait]
        impl GitProvider for $provider {
            fn capabilities(&self, locator: &RepoLocator) -> ProviderCapabilities {
                ($capabilities)(locator)
            }

            async fn repo_detail(&self, context: &RepoContext) -> Result<String, String> {
                $module::get_repo_detail(context.locator()).await
            }

            async fn pull_requests(
                &self,
                context: &ProjectContext,
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
                context: &ProjectContext,
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
                context: &ProjectContext,
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
                context: &ProjectContext,
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
                context: &ProjectContext,
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
                context: &ProjectContext,
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
                context: &ProjectContext,
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
                context: &ProjectContext,
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
                context: &ProjectContext,
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
                context: &ProjectContext,
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
                context: &ProjectContext,
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

            async fn revert_pull_request(
                &self,
                context: &ProjectContext,
                pull_request_iid: &str,
            ) -> Result<PullRequestRevertResult, String> {
                $module::revert_pull_request(
                    context.locator(),
                    context.project_id(),
                    pull_request_iid,
                )
                .await
            }
        }
    };
}

impl_project_provider!(GitLabProvider, gitlab, |_locator: &RepoLocator| {
    ProviderCapabilities {
        merge_delete_source_branch: true,
        dismiss_own_review: true,
        ..ProviderCapabilities::default()
    }
});

impl_project_provider!(GiteeProvider, gitee, |locator: &RepoLocator| {
    let enterprise = gitee_api_profile(locator) == GiteeApiProfile::EnterpriseV8;
    ProviderCapabilities {
        pull_request_ignore_whitespace: enterprise,
        commit_range_ignore_whitespace: enterprise,
        merge_delete_source_branch: true,
        dismiss_own_review: enterprise,
        server_side_revert: enterprise,
        ..ProviderCapabilities::default()
    }
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::error::GitErrorKind;

    #[test]
    fn saved_context_rejects_provider_mismatch_and_missing_id() {
        let mismatch = ProjectContext::from_saved("gitlab", "42", "https://github.com/acme/app");
        assert_eq!(mismatch.unwrap_err().kind(), GitErrorKind::Configuration);

        let missing = ProjectContext::from_saved("github", "", "https://github.com/acme/app");
        assert_eq!(missing.unwrap_err().kind(), GitErrorKind::Configuration);
    }

    #[test]
    fn github_capabilities_are_selected_from_the_validated_context() {
        let context =
            ProjectContext::from_saved("github", "42", "https://github.com/acme/app").unwrap();
        assert_eq!(context.repo_type(), RepoType::GitHub);
        assert_eq!(
            context.capabilities(),
            ProviderCapabilities {
                dismiss_review_by_id: true,
                server_side_revert: true,
                ..ProviderCapabilities::default()
            }
        );
    }
}
