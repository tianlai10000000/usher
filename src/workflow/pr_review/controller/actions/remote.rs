//! Provider operations, shared PR record publication and confirmed-mutation Hooks.
//!
//! Inputs are owned snapshots. This worker leaf has no controller or Slint
//! dependency; the controller alone schedules work and guards UI completion.
use std::future::Future;

use crate::git::{
    GitError, GitResult, PullRequestActionState, PullRequestAuthorIdentity, PullRequestMergeMethod,
    PullRequestRevertResult, PullRequestReviewAction, dismiss_pull_request_review,
    get_pull_request_action_state, merge_pull_request, revert_pull_request,
    submit_pull_request_review,
};
use crate::project::ProjectRepositoryMetadata;
use crate::workflow::pr_data::{PrKey, shared as shared_pr_data};

#[derive(Clone)]
pub(super) struct RemotePullRequest {
    pub(super) metadata: ProjectRepositoryMetadata,
    pub(super) number: String,
}

pub(super) struct PrHookContext {
    pub(super) target: RemotePullRequest,
    pub(super) title: String,
    pub(super) author: String,
    pub(super) source_branch: String,
    pub(super) target_branch: String,
    pub(super) author_identity: PullRequestAuthorIdentity,
    pub(super) project: String,
    pub(super) cwd: String,
}

/// A rejected publication is a stale read, not an error for the current page.
pub(super) async fn load_action_state(
    context: Option<RemotePullRequest>,
    alias: String,
    number: String,
    ticket: u64,
) -> Option<GitResult<PullRequestActionState>> {
    let context = match context {
        Some(context) => Ok(context),
        None => {
            tokio::task::spawn_blocking(move || crate::project::load_repository_metadata(&alias))
                .await
                .map_err(|_| GitError::configuration("Repository metadata worker stopped"))
                .and_then(|result| result.map_err(GitError::configuration))
                .map(|metadata| RemotePullRequest { metadata, number })
        }
    };
    let target = match context {
        Ok(target) => target,
        Err(error) => return Some(Err(error)),
    };
    let key = target.key();
    let result = get_pull_request_action_state(
        &target.metadata.repo_type,
        &target.metadata.repo_project_id,
        &target.metadata.remote_repo,
        &target.number,
    )
    .await;
    let accepted = match &result {
        Ok(state) => shared_pr_data().publish_action(ticket, &key, state.clone()),
        Err(_) => shared_pr_data().publish_action_unavailable(ticket, &key),
    };
    accepted.then_some(result)
}

impl RemotePullRequest {
    fn key(&self) -> PrKey {
        PrKey::new(&self.metadata, &self.number)
    }

    pub(super) fn dismiss(
        self,
        review_id: String,
        reason: String,
    ) -> impl Future<Output = GitResult<()>> + Send {
        // Capture credential scope when the command is accepted, before the
        // worker runs or settings can change while the provider is responding.
        let key = self.key();
        async move {
            let result = dismiss_pull_request_review(
                &self.metadata.repo_type,
                &self.metadata.repo_project_id,
                &self.metadata.remote_repo,
                &self.number,
                &review_id,
                &reason,
            )
            .await;
            if result.is_ok() {
                let data = shared_pr_data();
                data.publish_dismissal(data.ticket(), &key);
            }
            result
        }
    }

    pub(super) async fn revert(self) -> GitResult<PullRequestRevertResult> {
        revert_pull_request(
            &self.metadata.repo_type,
            &self.metadata.repo_project_id,
            &self.metadata.remote_repo,
            &self.number,
        )
        .await
    }
}

impl PrHookContext {
    pub(super) fn submit_review(
        self,
        action: PullRequestReviewAction,
        body: String,
    ) -> impl Future<Output = GitResult<()>> + Send {
        let hook_event = review_hook_event(&self, action);
        let target = self.target;
        let key = target.key();
        async move {
            let result = submit_pull_request_review(
                &target.metadata.repo_type,
                &target.metadata.repo_project_id,
                &target.metadata.remote_repo,
                &target.number,
                action,
                &body,
            )
            .await;
            if result.is_ok() {
                let data = shared_pr_data();
                data.publish_review(data.ticket(), &key, action);
                if let Some(hook_event) = hook_event {
                    if let Err(error) = crate::hooks::emit(hook_event).await {
                        log::warn!("[Hooks] PR approval dispatch failed: {error}");
                    }
                }
            }
            result
        }
    }

    pub(super) fn merge(
        self,
        method: PullRequestMergeMethod,
        remove_source_branch: bool,
    ) -> impl Future<Output = GitResult<()>> + Send {
        let hook_event = merged_hook_event(&self, method, remove_source_branch);
        let target = self.target;
        let key = target.key();
        async move {
            let result = merge_pull_request(
                &target.metadata.repo_type,
                &target.metadata.repo_project_id,
                &target.metadata.remote_repo,
                &target.number,
                method,
                remove_source_branch,
            )
            .await;
            if result.is_ok() {
                let data = shared_pr_data();
                data.publish_merge(data.ticket(), &key);
                if let Err(error) = crate::hooks::emit(hook_event).await {
                    log::warn!("[Hooks] PR merge dispatch failed: {error}");
                }
            }
            result
        }
    }
}

fn pull_request_hook_event(
    context: &PrHookContext,
    name: &str,
    source: &str,
    action: &str,
) -> crate::hooks::HookTrigger {
    let mut event = crate::hooks::HookTrigger::new(
        name,
        source,
        serde_json::json!({
            "number": context.target.number,
            "provider": context.target.metadata.repo_type,
            "repository": context.target.metadata.remote_repo,
            "action": action,
            "title": context.title,
            "source_branch": context.source_branch,
            "target_branch": context.target_branch,
            "author": context.author,
            "author_name": context.author_identity.name,
            "author_email": context.author_identity.email,
        }),
    );
    event.project = Some(context.project.clone());
    event.cwd = (!context.cwd.is_empty()).then(|| context.cwd.clone().into());
    event
}

fn review_hook_event(
    context: &PrHookContext,
    action: PullRequestReviewAction,
) -> Option<crate::hooks::HookTrigger> {
    (action == PullRequestReviewAction::Approve).then(|| {
        let mut event = pull_request_hook_event(
            context,
            crate::hooks::PR_REVIEW_APPROVED,
            "pr.review",
            "approved",
        );
        event.outcome = "approved".into();
        event
    })
}

fn merged_hook_event(
    context: &PrHookContext,
    method: PullRequestMergeMethod,
    remove_source_branch: bool,
) -> crate::hooks::HookTrigger {
    let mut event = pull_request_hook_event(context, crate::hooks::PR_MERGED, "pr.merge", "merged");
    let payload = event
        .payload
        .as_object_mut()
        .expect("PR Hook payload must be an object");
    payload.insert("merge_method".into(), method.as_str().into());
    payload.insert("remove_source_branch".into(), remove_source_branch.into());
    event
}

#[cfg(test)]
mod tests {
    use super::{PrHookContext, RemotePullRequest, merged_hook_event, review_hook_event};
    use crate::git::{PullRequestMergeMethod, PullRequestReviewAction};
    use crate::project::ProjectRepositoryMetadata;

    fn context() -> PrHookContext {
        PrHookContext {
            target: RemotePullRequest {
                metadata: ProjectRepositoryMetadata {
                    repo_project_id: "repo-1".into(),
                    repo_type: "github".into(),
                    remote_repo: "https://github.com/example/repo".into(),
                },
                number: "42".into(),
            },
            title: "Separate approval and merge".into(),
            author: "author".into(),
            source_branch: "feature/hooks".into(),
            target_branch: "main".into(),
            author_identity: crate::git::PullRequestAuthorIdentity {
                name: Some("Example Author".into()),
                email: Some("author@example.com".into()),
            },
            project: "project-1".into(),
            cwd: "/tmp/project-1".into(),
        }
    }

    #[test]
    fn only_approve_builds_a_review_hook_event() {
        let context = context();
        assert!(review_hook_event(&context, PullRequestReviewAction::Comment).is_none());
        assert!(review_hook_event(&context, PullRequestReviewAction::RequestChanges).is_none());
        let event = review_hook_event(&context, PullRequestReviewAction::Approve).unwrap();
        assert_eq!(event.name, crate::hooks::PR_REVIEW_APPROVED);
        assert_eq!(event.source_module, "pr.review");
        assert_eq!(event.outcome, "approved");
        assert_eq!(event.payload["action"], "approved");
        assert_eq!(event.project.as_deref(), Some("project-1"));
        assert_eq!(
            event.cwd.as_deref(),
            Some(std::path::Path::new("/tmp/project-1"))
        );
        assert_eq!(event.payload["number"], "42");
        assert_eq!(event.payload["provider"], "github");
        assert_eq!(
            event.payload["repository"],
            "https://github.com/example/repo"
        );
        assert_eq!(event.payload["title"], "Separate approval and merge");
        assert_eq!(event.payload["author"], "author");
        assert_eq!(event.payload["author_name"], "Example Author");
        assert_eq!(event.payload["author_email"], "author@example.com");
        assert_eq!(event.payload["source_branch"], "feature/hooks");
        assert_eq!(event.payload["target_branch"], "main");
    }

    #[test]
    fn every_successful_merge_method_uses_the_merged_hook_event() {
        let context = context();
        for method in [
            PullRequestMergeMethod::Merge,
            PullRequestMergeMethod::Squash,
            PullRequestMergeMethod::Rebase,
            PullRequestMergeMethod::FastForward,
        ] {
            let event = merged_hook_event(&context, method, true);
            assert_eq!(event.name, crate::hooks::PR_MERGED);
            assert_eq!(event.source_module, "pr.merge");
            assert_eq!(event.outcome, "success");
            assert_eq!(event.payload["action"], "merged");
            assert_eq!(event.payload["merge_method"], method.as_str());
            assert_eq!(event.payload["remove_source_branch"], true);
        }
    }
}
