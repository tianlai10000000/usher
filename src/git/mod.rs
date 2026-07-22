//! Provider-neutral Git repository operations and response models.

pub(crate) mod gitee;
mod github;
mod gitlab;

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepoType {
    Gitee,
    GitLab,
    GitHub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GiteeApiProfile {
    PublicV5,
    EnterpriseV8,
}

impl RepoType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Gitee => "gitee",
            Self::GitLab => "gitlab",
            Self::GitHub => "github",
        }
    }
}

impl fmt::Display for RepoType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepoDetail {
    pub(crate) repo_project_id: String,
    pub(crate) repo_type: RepoType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestItem {
    pub(crate) number: String,
    pub(crate) title: String,
    pub(crate) author: String,
    pub(crate) source_branch: String,
    pub(crate) target_branch: String,
    pub(crate) updated_at: String,
    pub(crate) web_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestPage {
    pub(crate) items: Vec<PullRequestItem>,
    pub(crate) total: u32,
    pub(crate) total_exact: bool,
    /// Provider-supplied page count when it is available separately from the
    /// total-item header (common on GitLab-shaped internal APIs).
    pub(crate) page_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestDiffLine {
    pub(crate) old_line: i32,
    pub(crate) new_line: i32,
    pub(crate) kind: String,
    pub(crate) content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestDiffFile {
    pub(crate) path: String,
    pub(crate) old_path: String,
    pub(crate) change_type: String,
    pub(crate) added_lines: u32,
    pub(crate) removed_lines: u32,
    pub(crate) viewed: bool,
    pub(crate) lines: Vec<PullRequestDiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestDiffDetail {
    pub(crate) files: Vec<PullRequestDiffFile>,
    pub(crate) added_lines: u32,
    pub(crate) removed_lines: u32,
    pub(crate) commit_count: u32,
    /// Provider summary for the Conversation tab badge. `None` on commit-range
    /// compares so applying a range cannot wipe the PR-level badge.
    pub(crate) conversation_count: Option<u32>,
    pub(crate) base_sha: String,
    pub(crate) head_sha: String,
    pub(crate) start_sha: String,
    pub(crate) too_many_commits: bool,
}

/// Read the Conversation-tab badge summary from a provider PR/MR detail payload.
/// Returns `None` when the provider omitted every known count field so callers can
/// fall back to a lightweight probe without treating "missing" as zero.
pub(crate) fn conversation_count_from_detail(detail: &serde_json::Value) -> Option<u32> {
    let root = detail;
    let detail = unwrap_detail_object(root);
    let direct = [
        "comments",
        "comments_count",
        "commentsCount",
        "user_notes_count",
        "userNotesCount",
        "notes_count",
        "notesCount",
        "comment_count",
        "commentCount",
    ]
    .into_iter()
    .find_map(|key| detail.get(key).and_then(json_u32));

    direct.or_else(|| {
        // Gitee Enterprise notes responses carry the exact total beside the
        // paged list rather than on each MR detail object.
        root.get("note_statistics")
            .or_else(|| {
                root.get("data")
                    .and_then(|data| data.get("note_statistics"))
            })
            .and_then(|statistics| {
                ["all_note_count", "allNoteCount", "note_count", "noteCount"]
                    .into_iter()
                    .find_map(|key| statistics.get(key).and_then(json_u32))
            })
    })
}

/// Read the Commits-tab badge summary from a provider PR/MR detail payload.
/// Returns `None` when the provider omitted every known count field.
pub(crate) fn commit_count_from_detail(detail: &serde_json::Value) -> Option<u32> {
    let detail = unwrap_detail_object(detail);
    [
        "commits",
        "commits_count",
        "commitsCount",
        "commit_count",
        "commitCount",
    ]
    .into_iter()
    .find_map(|key| detail.get(key).and_then(json_u32))
}

fn unwrap_detail_object(detail: &serde_json::Value) -> &serde_json::Value {
    detail
        .get("data")
        .filter(|value| value.is_object())
        .unwrap_or(detail)
}

fn json_u32(value: &serde_json::Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| value.as_i64().and_then(|value| u32::try_from(value).ok()))
        .or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<u32>().ok())
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestConversationItem {
    pub(crate) id: String,
    /// Portable timeline kinds: `description`, `comment`, `review`, `event`,
    /// `commits` (batch header), `commit` (row under a batch), `merged`.
    pub(crate) kind: String,
    pub(crate) author: String,
    pub(crate) avatar_url: String,
    /// Provider author association (for example GitHub `AUTHOR`).
    pub(crate) author_association: String,
    pub(crate) body: String,
    pub(crate) created_at: String,
    /// Extra display payload: review state, short SHA, target branch, or commit count.
    pub(crate) state: String,
}

pub(crate) fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

/// Build a GitHub-style "added N commits" batch plus one row per commit.
pub(crate) fn conversation_commit_batch_items(
    commits: &[PullRequestCommit],
) -> Vec<PullRequestConversationItem> {
    if commits.is_empty() {
        return Vec::new();
    }
    let count = commits.len();
    let author = commits
        .first()
        .map(|commit| commit.author.clone())
        .unwrap_or_else(|| "Unknown author".to_string());
    let avatar_url = commits
        .first()
        .map(|commit| commit.avatar_url.clone())
        .unwrap_or_default();
    let created_at = commits
        .iter()
        .rev()
        .map(|commit| commit.authored_at.as_str())
        .find(|value| !value.trim().is_empty())
        .unwrap_or("")
        .to_string();
    let mut items = Vec::with_capacity(count + 1);
    items.push(PullRequestConversationItem {
        id: format!(
            "commits-{}",
            commits
                .first()
                .map(|commit| commit.sha.as_str())
                .unwrap_or("batch")
        ),
        kind: "commits".to_string(),
        author,
        avatar_url,
        author_association: String::new(),
        body: if count == 1 {
            "added 1 commit".to_string()
        } else {
            format!("added {count} commits")
        },
        created_at,
        state: count.to_string(),
    });
    for commit in commits {
        items.push(PullRequestConversationItem {
            id: commit.sha.clone(),
            kind: "commit".to_string(),
            author: commit.author.clone(),
            avatar_url: commit.avatar_url.clone(),
            author_association: String::new(),
            body: commit.title.clone(),
            created_at: commit.authored_at.clone(),
            state: short_sha(commit.sha.as_str()),
        });
    }
    items
}

pub(crate) fn conversation_merged_item(
    author: &str,
    merge_sha: &str,
    target_branch: &str,
    created_at: &str,
) -> PullRequestConversationItem {
    PullRequestConversationItem {
        id: format!("merged-{}", short_sha(merge_sha)),
        kind: "merged".to_string(),
        author: if author.trim().is_empty() {
            "Unknown author".to_string()
        } else {
            author.to_string()
        },
        avatar_url: String::new(),
        author_association: String::new(),
        body: short_sha(merge_sha),
        created_at: created_at.to_string(),
        state: target_branch.to_string(),
    }
}

pub(crate) fn conversation_system_event_item(
    id: impl Into<String>,
    author: &str,
    body: impl Into<String>,
    created_at: &str,
) -> PullRequestConversationItem {
    PullRequestConversationItem {
        id: id.into(),
        kind: "event".to_string(),
        author: if author.trim().is_empty() {
            "Unknown author".to_string()
        } else {
            author.to_string()
        },
        avatar_url: String::new(),
        author_association: String::new(),
        body: body.into(),
        created_at: created_at.to_string(),
        state: String::new(),
    }
}

/// Keep the description card first, then append activity sorted by `created_at`.
pub(crate) fn finalize_conversation_timeline(
    mut description: PullRequestConversationItem,
    activity: Vec<PullRequestConversationItem>,
) -> Vec<PullRequestConversationItem> {
    description.kind = "description".to_string();

    // Treat a GitHub-style `commits` header and its following `commit` rows as
    // one atomic timeline block. Sorting every row independently separates the
    // "added N commits" record from commits whose authored dates differ. This
    // block normalization is shared by GitHub, GitLab, and both Gitee profiles.
    let mut blocks = Vec::<(String, Vec<PullRequestConversationItem>)>::new();
    let mut activity = activity.into_iter().peekable();
    while let Some(item) = activity.next() {
        let sort_at = item.created_at.clone();
        let mut block = vec![item];
        if block[0].kind == "commits" {
            while activity.peek().is_some_and(|next| next.kind == "commit") {
                if let Some(commit) = activity.next() {
                    block.push(commit);
                }
            }
        } else if block[0].kind == "review_summary"
            && activity.peek().is_some_and(|next| next.kind == "review")
        {
            if let Some(review) = activity.next() {
                block.push(review);
            }
        }
        blocks.push((sort_at, block));
    }
    blocks.sort_by(|left, right| left.0.cmp(&right.0));

    let mut items = Vec::new();
    items.push(description);
    items.extend(blocks.into_iter().flat_map(|(_, block)| block.into_iter()));
    items
}

/// True when a provider system note is already covered by structured commit rows.
pub(crate) fn is_redundant_commit_system_note(body: &str) -> bool {
    let normalized = body.trim().to_ascii_lowercase();
    normalized.starts_with("added ") && normalized.contains("commit")
}

/// True when a provider system note is already covered by a structured merge row.
pub(crate) fn is_redundant_merge_system_note(body: &str) -> bool {
    let normalized = body.trim().to_ascii_lowercase();
    normalized == "merged"
        || normalized.starts_with("merged commit")
        || normalized.starts_with("merged this")
        || normalized.starts_with("accepted this")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestCommit {
    pub(crate) sha: String,
    pub(crate) title: String,
    pub(crate) message: String,
    pub(crate) author: String,
    pub(crate) avatar_url: String,
    pub(crate) authored_at: String,
    pub(crate) verified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PullRequestReviewAction {
    Comment,
    Approve,
    RequestChanges,
}

impl PullRequestReviewAction {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "comment" => Some(Self::Comment),
            "approve" => Some(Self::Approve),
            "request_changes" | "request-changes" => Some(Self::RequestChanges),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PullRequestMergeMethod {
    Merge,
    Squash,
    Rebase,
    FastForward,
}

impl PullRequestMergeMethod {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "merge" => Some(Self::Merge),
            "squash" => Some(Self::Squash),
            "rebase" => Some(Self::Rebase),
            "fast_forward" | "fast-forward" => Some(Self::FastForward),
            _ => None,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
            Self::FastForward => "fast_forward",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PullRequestReadinessKind {
    Checking,
    Ready,
    Conflicts,
    Draft,
    ReviewRequired,
    ChangesRequested,
    ChecksPending,
    ChecksFailed,
    BranchOutOfDate,
    DiscussionsUnresolved,
    Blocked,
    Merged,
    Closed,
    Unavailable,
}

impl PullRequestReadinessKind {
    pub(crate) const fn as_str(&self) -> &'static str {
        match self {
            Self::Checking => "checking",
            Self::Ready => "ready",
            Self::Conflicts => "conflicts",
            Self::Draft => "draft",
            Self::ReviewRequired => "review_required",
            Self::ChangesRequested => "changes_requested",
            Self::ChecksPending => "checks_pending",
            Self::ChecksFailed => "checks_failed",
            Self::BranchOutOfDate => "branch_out_of_date",
            Self::DiscussionsUnresolved => "discussions_unresolved",
            Self::Blocked => "blocked",
            Self::Merged => "merged",
            Self::Closed => "closed",
            Self::Unavailable => "unavailable",
        }
    }

    pub(crate) const fn label(&self) -> &'static str {
        match self {
            Self::Checking => "Checking mergeability…",
            Self::Ready => "Ready to merge",
            Self::Conflicts => "Conflicts",
            Self::Draft => "Draft",
            Self::ReviewRequired => "Review required",
            Self::ChangesRequested => "Changes requested",
            Self::ChecksPending => "Checks pending",
            Self::ChecksFailed => "Checks failed",
            Self::BranchOutOfDate => "Branch out of date",
            Self::DiscussionsUnresolved => "Resolve conversations",
            Self::Blocked => "Merge blocked",
            Self::Merged => "Merged",
            Self::Closed => "Closed",
            Self::Unavailable => "Merge status unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestReadiness {
    pub(crate) kind: PullRequestReadinessKind,
    pub(crate) detail: String,
}

impl PullRequestReadiness {
    pub(crate) fn new(kind: PullRequestReadinessKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestActionState {
    pub(crate) can_comment: bool,
    pub(crate) can_approve: bool,
    pub(crate) can_request_changes: bool,
    pub(crate) can_merge: bool,
    pub(crate) review_state: String,
    pub(crate) readiness: PullRequestReadiness,
    pub(crate) merge_block_reason: String,
    pub(crate) allowed_merge_methods: Vec<PullRequestMergeMethod>,
}

/// Provider-neutral status filter. `None` means the caller intentionally
/// requests the provider default and no state query parameter is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PullRequestState {
    Open,
    Closed,
    Merged,
    All,
}

impl PullRequestState {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "open" | "opened" => Some(Self::Open),
            "closed" => Some(Self::Closed),
            "merged" => Some(Self::Merged),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Merged => "merged",
            Self::All => "all",
        }
    }

    pub(super) const fn gitee_query(self) -> &'static str {
        match self {
            Self::Open => "opened",
            Self::Closed => "closed",
            Self::Merged => "merged",
            Self::All => "all",
        }
    }

    pub(super) const fn gitee_public_query(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed | Self::Merged => "closed",
            Self::All => "all",
        }
    }

    pub(super) const fn gitlab_query(self) -> &'static str {
        match self {
            Self::Open => "opened",
            Self::Closed => "closed",
            Self::Merged => "merged",
            Self::All => "all",
        }
    }

    pub(super) const fn github_query(self) -> &'static str {
        match self {
            Self::Open => "is:open",
            Self::Closed => "is:closed",
            Self::Merged => "is:merged",
            Self::All => "",
        }
    }
}

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
            gitee::get_pull_requests(&locator, repo_project_id, state, page, per_page).await
        }
        RepoType::GitLab => {
            gitlab::get_pull_requests(&locator, repo_project_id, state, page, per_page).await
        }
        RepoType::GitHub => github::get_pull_requests(&locator, state, page, per_page).await,
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
            Err("Remote review actions are not implemented for GitLab yet.".to_string())
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
            Err("Remote merge actions are not implemented for GitLab yet.".to_string())
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

pub(super) fn parse_unified_diff(diff: &str) -> Vec<PullRequestDiffLine> {
    let mut lines = Vec::new();
    let mut old_line = 0i32;
    let mut new_line = 0i32;
    for raw in diff.lines() {
        if raw.starts_with("@@") {
            if let Some((old_start, new_start)) = parse_hunk_starts(raw) {
                old_line = old_start;
                new_line = new_start;
            }
            lines.push(PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: raw.to_string(),
            });
            continue;
        }
        if raw.starts_with("\\ No newline at end of file") {
            lines.push(PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "meta".to_string(),
                content: raw.to_string(),
            });
            continue;
        }
        let (kind, old_number, new_number, content) = if let Some(content) = raw.strip_prefix('+') {
            let current = new_line;
            new_line = new_line.saturating_add(1);
            ("addition", 0, current, content)
        } else if let Some(content) = raw.strip_prefix('-') {
            let current = old_line;
            old_line = old_line.saturating_add(1);
            ("deletion", current, 0, content)
        } else {
            let content = raw.strip_prefix(' ').unwrap_or(raw);
            let current_old = old_line;
            let current_new = new_line;
            old_line = old_line.saturating_add(1);
            new_line = new_line.saturating_add(1);
            ("context", current_old, current_new, content)
        };
        lines.push(PullRequestDiffLine {
            old_line: old_number,
            new_line: new_number,
            kind: kind.to_string(),
            content: content.to_string(),
        });
    }
    lines
}

fn parse_hunk_starts(header: &str) -> Option<(i32, i32)> {
    let mut fields = header.split_whitespace();
    (fields.next()? == "@@").then_some(())?;
    let old = parse_hunk_range(fields.next()?, '-')?;
    let new = parse_hunk_range(fields.next()?, '+')?;
    Some((old, new))
}

fn parse_hunk_range(value: &str, prefix: char) -> Option<i32> {
    value
        .strip_prefix(prefix)?
        .split(',')
        .next()?
        .parse::<i32>()
        .ok()
}

pub(super) fn unavailable_diff_file(
    path: String,
    old_path: String,
    change_type: String,
    added_lines: u32,
    removed_lines: u32,
    message: &str,
) -> PullRequestDiffFile {
    PullRequestDiffFile {
        path,
        old_path,
        change_type,
        added_lines,
        removed_lines,
        viewed: false,
        lines: vec![PullRequestDiffLine {
            old_line: 0,
            new_line: 0,
            kind: "meta".to_string(),
            content: message.to_string(),
        }],
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

#[cfg(test)]
mod tests {
    use super::{
        PullRequestCommit, PullRequestState, RepoType, commit_count_from_detail,
        conversation_commit_batch_items, conversation_count_from_detail, conversation_merged_item,
        finalize_conversation_timeline, is_redundant_commit_system_note,
        is_redundant_merge_system_note, parse_repo_locator, parse_unified_diff,
        percent_encode_path, supports_ignore_whitespace,
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
        assert_eq!(batch[1].state, "abcdef0");
        assert_eq!(batch[1].avatar_url, "https://example.test/alice.png");
        assert_eq!(batch[2].state, "1234567");
        assert_eq!(batch[0].created_at, "2026-01-02T00:00:00Z");
        assert_eq!(batch[1].created_at, "2026-01-01T00:00:00Z");
        assert_eq!(batch[2].created_at, batch[0].created_at);

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
