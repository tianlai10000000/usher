//! GitHub repository operations.

use base64::Engine as _;
use futures_util::future::join_all;
use reqwest::header::{ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::Deserialize;
use serde_json::Value;

use crate::codereview::api::github as endpoints;
use crate::http::{self, RequestOptions};

use super::{
    PullRequestActionState, PullRequestCommit, PullRequestConversationItem, PullRequestDiffDetail,
    PullRequestDiffFile, PullRequestFilters, PullRequestItem, PullRequestMergeMethod,
    PullRequestPage, PullRequestReadiness, PullRequestReadinessKind, PullRequestRevertResult,
    PullRequestReviewAction, PullRequestState, RepoLocator, RepoType, commit_count_from_detail,
    configured_api_base_url, configured_token, conversation_commit_batch_items,
    conversation_count_from_detail, conversation_merged_item, conversation_system_event_item,
    finalize_conversation_timeline, is_configured_enterprise_host, parse_unified_diff,
    percent_encode_path, unavailable_diff_file,
};

#[derive(Deserialize)]
struct GitHubRepository {
    id: serde_json::Value,
}

pub(super) async fn get_repo_detail(locator: &RepoLocator) -> Result<String, String> {
    let endpoint = endpoints::REPOSITORY_URL
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace(
            "{namespace}",
            percent_encode_path(locator.namespace.as_str()).as_str(),
        )
        .replace(
            "{repository}",
            percent_encode_path(locator.repo.as_str()).as_str(),
        );
    let mut request = http::get(endpoint)
        .header(ACCEPT, "application/vnd.github+json")
        .header(USER_AGENT, "usher-project-lookup");
    if let Some(token) = configured_token(RepoType::GitHub) {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }

    let response: GitHubRepository =
        http::send_json(request, RequestOptions::new("Load GitHub project details"))
            .await
            .map_err(|error| error.to_string())?;
    value_to_id(&response.id, "GitHub")
}

pub(super) async fn get_pull_requests(
    locator: &RepoLocator,
    state: Option<PullRequestState>,
    filters: &PullRequestFilters,
    page: u32,
    per_page: u32,
) -> Result<PullRequestPage, String> {
    let per_page = per_page.max(1);
    let mut query = format!("repo:{}/{} is:pr", locator.namespace, locator.repo);
    if let Some(state) = state {
        let state_query = state.github_query();
        if !state_query.is_empty() {
            query.push(' ');
            query.push_str(state_query);
        }
    }
    match filters.reviews.as_str() {
        "required" => query.push_str(" review:required"),
        "approved" => query.push_str(" review:approved"),
        "changes-requested" => query.push_str(" review:changes_requested"),
        _ => {}
    }
    match filters.assignee.as_str() {
        "none" => query.push_str(" no:assignee"),
        "me" => query.push_str(" assignee:@me"),
        _ => {}
    }
    let (sort, order) = match filters.sort.as_str() {
        "oldest" => ("created", "asc"),
        "updated-desc" => ("updated", "desc"),
        "updated-asc" => ("updated", "asc"),
        _ => ("created", "desc"),
    };
    let endpoint = endpoints::SEARCH_PULL_REQUESTS_URL
        .replace("{api_base_url}", api_base_url(locator).as_str());
    let mut request = http::get(endpoint)
        .header(ACCEPT, "application/vnd.github+json")
        .header(USER_AGENT, "usher-project-detail")
        .query(&[
            ("q", query.as_str()),
            ("per_page", &per_page.to_string()),
            ("page", &page.max(1).to_string()),
            ("sort", sort),
            ("order", order),
        ]);
    if let Some(token) = configured_token(RepoType::GitHub) {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }

    let payload: Value = http::send_json(request, RequestOptions::new("Load GitHub pull requests"))
        .await
        .map_err(|error| error.to_string())?;
    let total = payload
        .get("total_count")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let items = payload
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| "GitHub pull request response is not a list.".to_string())?
        .iter()
        .map(|value| PullRequestItem {
            number: string_field(value, "number").unwrap_or_else(|| "—".to_string()),
            title: string_field(value, "title")
                .unwrap_or_else(|| "Untitled pull request".to_string()),
            state: if value
                .pointer("/pull_request/merged_at")
                .is_some_and(|merged_at| !merged_at.is_null())
            {
                "merged".to_string()
            } else {
                string_field(value, "state").unwrap_or_else(|| "open".to_string())
            },
            author: nested_string(value, "user", "login")
                .unwrap_or_else(|| "Unknown author".to_string()),
            source_branch: "—".to_string(),
            target_branch: "—".to_string(),
            conversation_count: conversation_count_from_detail(value),
            updated_at: string_field(value, "updated_at")
                .unwrap_or_else(|| "Unknown update time".to_string()),
            web_url: string_field(value, "html_url").unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    // Count probes request one row only and never render it. Avoid an extra PR
    // detail request when only Search's exact `total_count` is needed.
    let items = if per_page == 1 {
        items
    } else {
        hydrate_pull_request_branches(locator, items).await
    };
    Ok(PullRequestPage {
        items,
        total,
        total_exact: true,
        page_count: Some(total.div_ceil(per_page)),
    })
}

pub(super) async fn get_open_pull_request_count(
    locator: &RepoLocator,
    target_branch: &str,
) -> Result<u32, String> {
    let query = open_pull_request_count_query(locator, target_branch);
    let endpoint = endpoints::SEARCH_PULL_REQUESTS_URL
        .replace("{api_base_url}", api_base_url(locator).as_str());
    let mut request = http::get(endpoint)
        .header(ACCEPT, "application/vnd.github+json")
        .header(USER_AGENT, "usher-project-overview")
        .query(&[("q", query.as_str()), ("per_page", "1"), ("page", "1")]);
    if let Some(token) = configured_token(RepoType::GitHub) {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }

    let payload: Value = http::send_json(
        request,
        RequestOptions::new("Load GitHub pull request overview"),
    )
    .await
    .map_err(|error| error.to_string())?;
    payload
        .get("total_count")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| "GitHub pull request overview has no valid total count.".to_string())
}

fn open_pull_request_count_query(locator: &RepoLocator, target_branch: &str) -> String {
    format!(
        "repo:{}/{} is:pr is:open base:{}",
        locator.namespace,
        locator.repo,
        target_branch.trim()
    )
}

/// GitHub's issue-search response provides the exact PR count, but not the
/// head/base refs. Fetch details for the visible rows concurrently so the list
/// keeps its accurate paginator while still showing the branch pair.
async fn hydrate_pull_request_branches(
    locator: &RepoLocator,
    mut items: Vec<PullRequestItem>,
) -> Vec<PullRequestItem> {
    let requests = items.iter().enumerate().map(|(index, item)| async move {
        let summary = load_pull_request_summary(locator, item.number.as_str()).await;
        (index, summary)
    });

    for (index, summary) in join_all(requests).await {
        if let Some((source_branch, target_branch, conversation_count)) = summary {
            items[index].source_branch = source_branch;
            items[index].target_branch = target_branch;
            items[index].conversation_count =
                conversation_count.or(items[index].conversation_count);
        }
    }
    items
}

async fn load_pull_request_summary(
    locator: &RepoLocator,
    pull_request_number: &str,
) -> Option<(String, String, Option<u32>)> {
    let endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let detail: Value = http::send_json(
        authenticated_request(locator, endpoint),
        RequestOptions::new("Load GitHub pull request branches"),
    )
    .await
    .ok()?;
    pull_request_branches(&detail).map(|(source_branch, target_branch)| {
        (
            source_branch,
            target_branch,
            conversation_count_from_detail(&detail),
        )
    })
}

pub(super) async fn get_pull_request_diff(
    locator: &RepoLocator,
    pull_request_number: &str,
    ignore_whitespace_change: bool,
) -> Result<PullRequestDiffDetail, String> {
    let detail_endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let detail: Value = http::send_json(
        authenticated_request(locator, detail_endpoint),
        RequestOptions::new("Load GitHub pull request details"),
    )
    .await
    .map_err(|error| error.to_string())?;

    if ignore_whitespace_change {
        log::debug!(
            "[CodeReview][GitHub] Ignore-whitespace is not supported by the pull files endpoint"
        );
    }
    let files_endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_FILES_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let mut page = 1u32;
    let mut values = Vec::new();
    loop {
        let request = authenticated_request(locator, files_endpoint.clone())
            .query(&[("page", page.to_string()), ("per_page", "100".to_string())]);
        let batch: Vec<Value> = http::send_json(
            request,
            RequestOptions::new("Load GitHub pull request files"),
        )
        .await
        .map_err(|error| error.to_string())?;
        let batch_len = batch.len();
        values.extend(batch);
        if batch_len < 100 {
            break;
        }
        page = page.saturating_add(1);
        if page > 30 {
            break;
        }
    }
    let files = values.iter().map(decode_diff_file).collect::<Vec<_>>();
    let commit_count = match commit_count_from_detail(&detail) {
        Some(count) => count,
        None => count_pull_request_commits(locator, pull_request_number)
            .await
            .unwrap_or(0),
    };
    // GitHub's PR `comments` field counts issue comments but excludes reviews
    // submitted through `/pulls/{number}/reviews` (including COMMENT reviews).
    // Count that collection with one item per page so both kinds contribute to
    // the Conversation badge without loading the full timeline.
    let conversation_count = conversation_count_from_detail(&detail)
        .unwrap_or(0)
        .saturating_add(
            count_pull_request_reviews(locator, pull_request_number)
                .await
                .unwrap_or(0),
        );
    Ok(PullRequestDiffDetail {
        added_lines: files.iter().map(|file| file.added_lines).sum(),
        removed_lines: files.iter().map(|file| file.removed_lines).sum(),
        commit_count,
        conversation_count: Some(conversation_count),
        base_sha: nested_string(&detail, "base", "sha").unwrap_or_default(),
        head_sha: nested_string(&detail, "head", "sha").unwrap_or_default(),
        start_sha: nested_string(&detail, "base", "sha").unwrap_or_default(),
        too_many_commits: values.len() >= 3000,
        files,
    })
}

pub(super) async fn get_commit_range_diff(
    locator: &RepoLocator,
    base_sha: &str,
    head_sha: &str,
    commit_count: u32,
) -> Result<PullRequestDiffDetail, String> {
    let endpoint = repository_endpoint(endpoints::REPOSITORY_COMPARE_URL, locator, None, None)
        .replace("{base}", percent_encode_path(base_sha.trim()).as_str())
        .replace("{head}", percent_encode_path(head_sha.trim()).as_str());
    let payload: Value = http::send_json(
        authenticated_request(locator, endpoint),
        RequestOptions::new("Compare GitHub pull request commits"),
    )
    .await
    .map_err(|error| error.to_string())?;
    let values = payload
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| "GitHub commit comparison response has no file list.".to_string())?;
    let files = values.iter().map(decode_diff_file).collect::<Vec<_>>();
    Ok(PullRequestDiffDetail {
        added_lines: files.iter().map(|file| file.added_lines).sum(),
        removed_lines: files.iter().map(|file| file.removed_lines).sum(),
        commit_count,
        conversation_count: None,
        base_sha: base_sha.to_string(),
        head_sha: head_sha.to_string(),
        start_sha: base_sha.to_string(),
        too_many_commits: values.len() >= 300,
        files,
    })
}

fn decode_diff_file(value: &Value) -> PullRequestDiffFile {
    let path = string_field(value, "filename").unwrap_or_else(|| "Unknown file".to_string());
    let old_path = string_field(value, "previous_filename").unwrap_or_else(|| path.clone());
    let change_type = match string_field(value, "status").as_deref() {
        Some("added") => "new_file",
        Some("removed") => "deleted_file",
        Some("renamed") => "renamed_file",
        _ => "edit_file",
    }
    .to_string();
    let added_lines = value_u32(value, "additions").unwrap_or(0);
    let removed_lines = value_u32(value, "deletions").unwrap_or(0);
    let lines = string_field(value, "patch")
        .map(|patch| parse_unified_diff(patch.as_str()))
        .unwrap_or_default();
    if lines.is_empty() {
        return unavailable_diff_file(
            path,
            old_path,
            change_type,
            added_lines,
            removed_lines,
            "GitHub omitted this binary or oversized patch.",
        );
    }
    PullRequestDiffFile {
        path,
        old_path,
        change_type,
        added_lines,
        removed_lines,
        viewed: false,
        lines,
    }
}

pub(super) async fn get_repository_file_text(
    locator: &RepoLocator,
    path: &str,
    reference: &str,
) -> Result<String, String> {
    let endpoint =
        repository_endpoint(endpoints::REPOSITORY_CONTENT_URL, locator, None, Some(path));
    let payload: Value = http::send_json(
        authenticated_request(locator, endpoint).query(&[("ref", reference)]),
        RequestOptions::new("Load GitHub repository file"),
    )
    .await
    .map_err(|error| error.to_string())?;
    decode_base64_content(&payload, "GitHub")
}

pub(super) async fn get_pull_request_action_state(
    locator: &RepoLocator,
    pull_request_number: &str,
) -> Result<PullRequestActionState, String> {
    let detail_endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let detail: Value = http::send_json(
        authenticated_request(locator, detail_endpoint),
        RequestOptions::new("Load GitHub pull request merge state"),
    )
    .await
    .map_err(|error| error.to_string())?;

    let repository_url = repository_endpoint(endpoints::REPOSITORY_URL, locator, None, None);
    let repository: Value = http::send_json(
        authenticated_request(locator, repository_url),
        RequestOptions::new("Load GitHub repository merge settings"),
    )
    .await
    .map_err(|error| error.to_string())?;

    let reviews_endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_REVIEWS_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let reviews = http::send_json::<Vec<Value>>(
        authenticated_request(locator, reviews_endpoint).query(&[("per_page", "100")]),
        RequestOptions::new("Load GitHub pull request reviews"),
    )
    .await
    .unwrap_or_default();

    let has_token = configured_token(RepoType::GitHub).is_some();
    let review_state = aggregate_review_state(reviews.as_slice());
    let readiness = github_merge_readiness(&detail, review_state.as_str());
    let terminal = matches!(
        readiness.kind,
        PullRequestReadinessKind::Merged | PullRequestReadinessKind::Closed
    );
    let pull_request_node_id = string_field(&detail, "node_id").unwrap_or_default();
    let can_merge = has_token && readiness.kind == PullRequestReadinessKind::Ready;
    let can_revert = has_token
        && readiness.kind == PullRequestReadinessKind::Merged
        && !pull_request_node_id.is_empty();
    let merge_block_reason = if !has_token {
        "Configure a GitHub Token in Settings to review or merge this pull request.".to_string()
    } else if readiness.kind != PullRequestReadinessKind::Ready {
        readiness.detail.clone()
    } else {
        String::new()
    };
    let revert_block_reason = if !has_token {
        "Configure a GitHub Token in Settings to create a revert pull request.".to_string()
    } else if readiness.kind != PullRequestReadinessKind::Merged {
        "Only a merged pull request can be reverted.".to_string()
    } else if pull_request_node_id.is_empty() {
        "GitHub did not return the pull request node identifier required for a revert.".to_string()
    } else {
        String::new()
    };

    let mut allowed_merge_methods = Vec::new();
    if repository
        .get("allow_merge_commit")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        allowed_merge_methods.push(PullRequestMergeMethod::Merge);
    }
    if repository
        .get("allow_squash_merge")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        allowed_merge_methods.push(PullRequestMergeMethod::Squash);
    }
    if repository
        .get("allow_rebase_merge")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        allowed_merge_methods.push(PullRequestMergeMethod::Rebase);
    }

    Ok(PullRequestActionState {
        // GitHub keeps the Conversation composer writable on closed/merged PRs
        // (issue comments). Approve / request-changes stay open-PR only.
        can_comment: has_token,
        can_approve: has_token && !terminal,
        can_request_changes: has_token && !terminal,
        can_merge,
        can_revert,
        review_state,
        readiness,
        merge_block_reason,
        revert_block_reason,
        revert_action_label: "Create revert PR".to_string(),
        revert_confirmation:
            "GitHub will create a separate pull request that reverses the merged changes. The original pull request remains merged."
                .to_string(),
        allowed_merge_methods,
    })
}

pub(super) async fn get_pull_request_conversation(
    locator: &RepoLocator,
    pull_request_number: &str,
) -> Result<Vec<PullRequestConversationItem>, String> {
    let detail_endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let comments_endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_ISSUE_COMMENTS_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let reviews_endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_REVIEWS_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let timeline_endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_ISSUE_TIMELINE_URL,
        locator,
        Some(pull_request_number),
        None,
    );

    let detail_future = async {
        http::send_json(
            authenticated_request(locator, detail_endpoint),
            RequestOptions::new("Load GitHub pull request conversation header"),
        )
        .await
        .map_err(|error| error.to_string())
    };
    let comments_future = load_github_pages(
        locator,
        comments_endpoint,
        "Load GitHub pull request comments",
    );
    let reviews_future = load_github_pages(
        locator,
        reviews_endpoint,
        "Load GitHub pull request reviews",
    );
    let timeline_future = load_github_pages(
        locator,
        timeline_endpoint,
        "Load GitHub pull request timeline",
    );
    let (detail, comments, reviews, timeline) = tokio::try_join!(
        detail_future,
        comments_future,
        reviews_future,
        timeline_future
    )?;

    let description = PullRequestConversationItem {
        id: format!("description-{pull_request_number}"),
        kind: "description".to_string(),
        author: nested_string(&detail, "user", "login")
            .unwrap_or_else(|| "Unknown author".to_string()),
        avatar_url: nested_string(&detail, "user", "avatar_url").unwrap_or_default(),
        author_association: string_field(&detail, "author_association").unwrap_or_default(),
        body: string_field(&detail, "body").unwrap_or_default(),
        created_at: string_field(&detail, "created_at").unwrap_or_default(),
        state: string_field(&detail, "state").unwrap_or_default(),
    };
    let target_branch = nested_string(&detail, "base", "ref").unwrap_or_default();

    let mut activity = Vec::new();
    activity.extend(
        comments
            .into_iter()
            .map(|value| PullRequestConversationItem {
                id: string_field(&value, "id").unwrap_or_default(),
                kind: "comment".to_string(),
                author: nested_string(&value, "user", "login")
                    .unwrap_or_else(|| "Unknown author".to_string()),
                avatar_url: nested_string(&value, "user", "avatar_url").unwrap_or_default(),
                author_association: string_field(&value, "author_association").unwrap_or_default(),
                body: string_field(&value, "body").unwrap_or_default(),
                created_at: string_field(&value, "created_at").unwrap_or_default(),
                state: String::new(),
            }),
    );
    activity.extend(reviews.into_iter().flat_map(|value| {
        let body = string_field(&value, "body").unwrap_or_default();
        let state = string_field(&value, "state")
            .unwrap_or_default()
            .to_ascii_lowercase();
        if body.trim().is_empty() && state.trim().is_empty() {
            return Vec::new();
        }
        let id = string_field(&value, "id").unwrap_or_default();
        let author = nested_string(&value, "user", "login")
            .unwrap_or_else(|| "Unknown reviewer".to_string());
        let avatar_url = nested_string(&value, "user", "avatar_url").unwrap_or_default();
        let author_association = string_field(&value, "author_association").unwrap_or_default();
        let created_at = string_field(&value, "submitted_at").unwrap_or_default();
        let mut items = vec![PullRequestConversationItem {
            id: format!("review-summary-{id}"),
            kind: "review_summary".to_string(),
            author: author.clone(),
            avatar_url: avatar_url.clone(),
            author_association: author_association.clone(),
            body: github_review_action(state.as_str()).to_string(),
            created_at: created_at.clone(),
            state: state.clone(),
        }];
        if !body.trim().is_empty() {
            items.push(PullRequestConversationItem {
                id,
                kind: "review".to_string(),
                author,
                avatar_url,
                author_association,
                body,
                created_at,
                state,
            });
        }
        items
    }));
    activity.extend(decode_github_timeline_activity(
        &timeline,
        target_branch.as_str(),
    ));
    Ok(finalize_conversation_timeline(description, activity))
}

fn decode_github_timeline_activity(
    timeline: &[Value],
    target_branch: &str,
) -> Vec<PullRequestConversationItem> {
    let mut items = Vec::new();
    let mut pending_commits = Vec::<PullRequestCommit>::new();
    let flush_commits = |pending: &mut Vec<PullRequestCommit>,
                         out: &mut Vec<PullRequestConversationItem>| {
        if pending.is_empty() {
            return;
        }
        out.extend(conversation_commit_batch_items(pending));
        pending.clear();
    };

    let has_merged_event = timeline
        .iter()
        .any(|event| string_field(event, "event").as_deref() == Some("merged"));
    for event in timeline {
        let event_kind = string_field(event, "event").unwrap_or_default();
        match event_kind.as_str() {
            "committed" => {
                let message = string_field(event, "message").unwrap_or_default();
                pending_commits.push(PullRequestCommit {
                    sha: string_field(event, "sha").unwrap_or_default(),
                    title: message.lines().next().unwrap_or_default().to_string(),
                    message,
                    author: nested_string(event, "author", "name")
                        .or_else(|| nested_string(event, "committer", "name"))
                        .unwrap_or_else(|| "Unknown author".to_string()),
                    avatar_url: nested_string(event, "author", "avatar_url")
                        .or_else(|| nested_string(event, "committer", "avatar_url"))
                        .unwrap_or_default(),
                    authored_at: nested_string(event, "author", "date")
                        .or_else(|| nested_string(event, "committer", "date"))
                        .unwrap_or_default(),
                    verified: event
                        .get("verification")
                        .and_then(|verification| verification.get("verified"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                });
            }
            "merged" => {
                flush_commits(&mut pending_commits, &mut items);
                let author = nested_string(event, "actor", "login")
                    .unwrap_or_else(|| "Unknown author".to_string());
                let sha = string_field(event, "commit_id").unwrap_or_default();
                let created_at = string_field(event, "created_at").unwrap_or_default();
                if !sha.trim().is_empty() {
                    items.push(conversation_merged_item(
                        author.as_str(),
                        sha.as_str(),
                        if target_branch.trim().is_empty() {
                            "main"
                        } else {
                            target_branch
                        },
                        created_at.as_str(),
                    ));
                }
            }
            "closed"
            | "reopened"
            | "head_ref_force_pushed"
            | "ready_for_review"
            | "convert_to_draft" => {
                // GitHub emits both `merged` and `closed` for a merged PR but
                // renders only the semantically richer merge event.
                if event_kind == "closed" && has_merged_event {
                    continue;
                }
                flush_commits(&mut pending_commits, &mut items);
                let author = nested_string(event, "actor", "login")
                    .unwrap_or_else(|| "Unknown author".to_string());
                let created_at = string_field(event, "created_at").unwrap_or_default();
                let body = match event_kind.as_str() {
                    "closed" => "closed this pull request",
                    "reopened" => "reopened this pull request",
                    "head_ref_force_pushed" => "force-pushed the branch",
                    "ready_for_review" => "marked this pull request as ready for review",
                    "convert_to_draft" => "converted this pull request to draft",
                    _ => "updated this pull request",
                };
                items.push(conversation_system_event_item(
                    format!(
                        "event-{}-{}",
                        event_kind,
                        string_field(event, "id").unwrap_or_else(|| created_at.clone())
                    ),
                    author.as_str(),
                    body,
                    created_at.as_str(),
                ));
            }
            // Comments/reviews are loaded from dedicated endpoints to keep bodies complete.
            _ => flush_commits(&mut pending_commits, &mut items),
        }
    }
    flush_commits(&mut pending_commits, &mut items);
    items
}

fn github_review_action(state: &str) -> &'static str {
    match state {
        "approved" => "approved these changes",
        "changes_requested" => "requested changes",
        "dismissed" => "had a review dismissed",
        _ => "commented",
    }
}

pub(super) async fn get_pull_request_commits(
    locator: &RepoLocator,
    pull_request_number: &str,
) -> Result<Vec<PullRequestCommit>, String> {
    let endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_COMMITS_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let values = load_github_pages(locator, endpoint, "Load GitHub pull request commits").await?;
    Ok(values.iter().map(decode_commit).collect())
}

/// One-page commit probe for the Commits tab badge. Never loads the full list.
async fn count_pull_request_commits(
    locator: &RepoLocator,
    pull_request_number: &str,
) -> Option<u32> {
    let endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_COMMITS_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let response = http::send(
        authenticated_request(locator, endpoint).query(&[("page", "1"), ("per_page", "1")]),
        RequestOptions::new("Count GitHub pull request commits"),
    )
    .await
    .ok()?;
    let headers = response.headers().clone();
    let values: Vec<Value> = response.json().await.ok()?;
    single_item_page_total(&headers, values.len())
}

/// One-page review probe for the Conversation tab badge. GitHub PR detail does
/// not include reviews in its `comments` total.
async fn count_pull_request_reviews(
    locator: &RepoLocator,
    pull_request_number: &str,
) -> Option<u32> {
    let endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_REVIEWS_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let response = http::send(
        authenticated_request(locator, endpoint).query(&[("page", "1"), ("per_page", "1")]),
        RequestOptions::new("Count GitHub pull request reviews"),
    )
    .await
    .ok()?;
    let headers = response.headers().clone();
    let values: Vec<Value> = response.json().await.ok()?;
    single_item_page_total(&headers, values.len())
}

fn single_item_page_total(headers: &reqwest::header::HeaderMap, item_count: usize) -> Option<u32> {
    if let Some(total) = link_rel_last_page(headers) {
        return Some(total);
    }
    match item_count {
        0 => Some(0),
        // No `rel=last` means a single page; with per_page=1 this is exactly one item.
        1 => Some(1),
        _ => None,
    }
}

fn link_rel_last_page(headers: &reqwest::header::HeaderMap) -> Option<u32> {
    let link = headers.get(reqwest::header::LINK)?.to_str().ok()?;
    for part in link.split(',') {
        let part = part.trim();
        if !(part.contains("rel=\"last\"") || part.contains("rel='last'")) {
            continue;
        }
        let start = part.find("page=")? + "page=".len();
        let end = part[start..]
            .find(|ch: char| !ch.is_ascii_digit())
            .map(|offset| start + offset)
            .unwrap_or(part.len());
        return part[start..end].parse().ok();
    }
    None
}

fn decode_commit(value: &Value) -> PullRequestCommit {
    let message = value
        .get("commit")
        .and_then(|commit| string_field(commit, "message"))
        .unwrap_or_default();
    PullRequestCommit {
        sha: string_field(value, "sha").unwrap_or_default(),
        title: message.lines().next().unwrap_or_default().to_string(),
        message,
        author: nested_string(value, "author", "login")
            .or_else(|| {
                value
                    .get("commit")
                    .and_then(|commit| nested_string(commit, "author", "name"))
            })
            .unwrap_or_else(|| "Unknown author".to_string()),
        avatar_url: nested_string(value, "author", "avatar_url")
            .or_else(|| nested_string(value, "committer", "avatar_url"))
            .unwrap_or_default(),
        authored_at: value
            .get("commit")
            .and_then(|commit| nested_string(commit, "author", "date"))
            .unwrap_or_default(),
        verified: value
            .get("commit")
            .and_then(|commit| commit.get("verification"))
            .and_then(|verification| verification.get("verified"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

async fn load_github_pages(
    locator: &RepoLocator,
    endpoint: String,
    operation: &'static str,
) -> Result<Vec<Value>, String> {
    let mut values = Vec::new();
    for page in 1..=30u32 {
        let batch: Vec<Value> = http::send_json(
            authenticated_request(locator, endpoint.clone())
                .query(&[("page", page.to_string()), ("per_page", "100".to_string())]),
            RequestOptions::new(operation),
        )
        .await
        .map_err(|error| error.to_string())?;
        let complete = batch.len() < 100;
        values.extend(batch);
        if complete {
            break;
        }
    }
    Ok(values)
}

pub(super) async fn submit_pull_request_review(
    locator: &RepoLocator,
    pull_request_number: &str,
    action: PullRequestReviewAction,
    body: &str,
) -> Result<(), String> {
    if configured_token(RepoType::GitHub).is_none() {
        return Err("Configure a GitHub Token in Settings before submitting a review.".into());
    }
    if action != PullRequestReviewAction::Approve && body.trim().is_empty() {
        return Err("A review comment is required for this action.".into());
    }

    // Conversation / Comment matches GitHub's timeline composer: an issue
    // comment that works on open, closed, and merged pull requests. Approve
    // and request-changes still use the reviews endpoint.
    if action == PullRequestReviewAction::Comment {
        let endpoint = repository_endpoint(
            endpoints::PULL_REQUEST_ISSUE_COMMENTS_URL,
            locator,
            Some(pull_request_number),
            None,
        );
        let request = authenticated_builder(
            http::post(endpoint),
            configured_token(RepoType::GitHub).as_deref(),
        )
        .json(&serde_json::json!({ "body": body.trim() }));
        let _: Value = http::send_json(
            request,
            RequestOptions::new("Submit GitHub pull request comment"),
        )
        .await
        .map_err(|error| error.to_string())?;
        return Ok(());
    }

    let endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_REVIEWS_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let event = match action {
        PullRequestReviewAction::Approve => "APPROVE",
        PullRequestReviewAction::RequestChanges => "REQUEST_CHANGES",
        // Handled above via issue comments; keep the arm exhaustive.
        PullRequestReviewAction::Comment => return Ok(()),
    };
    let request = authenticated_builder(
        http::post(endpoint),
        configured_token(RepoType::GitHub).as_deref(),
    )
    .json(&serde_json::json!({ "body": body.trim(), "event": event }));
    let _: Value = http::send_json(
        request,
        RequestOptions::new("Submit GitHub pull request review"),
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) async fn merge_pull_request(
    locator: &RepoLocator,
    pull_request_number: &str,
    method: PullRequestMergeMethod,
) -> Result<(), String> {
    if configured_token(RepoType::GitHub).is_none() {
        return Err("Configure a GitHub Token in Settings before merging.".into());
    }
    if method == PullRequestMergeMethod::FastForward {
        return Err("GitHub does not expose fast-forward as a pull request merge method.".into());
    }
    let endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_MERGE_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let request = authenticated_builder(
        http::put(endpoint),
        configured_token(RepoType::GitHub).as_deref(),
    )
    .json(&serde_json::json!({ "merge_method": method.as_str() }));
    let payload: Value = http::send_json(request, RequestOptions::new("Merge GitHub pull request"))
        .await
        .map_err(|error| error.to_string())?;
    if payload.get("merged").and_then(Value::as_bool) == Some(false) {
        return Err(string_field(&payload, "message")
            .unwrap_or_else(|| "GitHub did not merge this pull request.".to_string()));
    }
    Ok(())
}

pub(super) async fn revert_pull_request(
    locator: &RepoLocator,
    pull_request_number: &str,
) -> Result<PullRequestRevertResult, String> {
    let Some(token) = configured_token(RepoType::GitHub) else {
        return Err(
            "Configure a GitHub Token in Settings before creating a revert pull request.".into(),
        );
    };
    let detail_endpoint = repository_endpoint(
        endpoints::PULL_REQUEST_URL,
        locator,
        Some(pull_request_number),
        None,
    );
    let detail: Value = http::send_json(
        authenticated_request(locator, detail_endpoint),
        RequestOptions::new("Load GitHub pull request before revert"),
    )
    .await
    .map_err(|error| error.to_string())?;
    let readiness = github_merge_readiness(&detail, "");
    if readiness.kind != PullRequestReadinessKind::Merged {
        return Err("Only a merged GitHub pull request can be reverted.".to_string());
    }
    let pull_request_node_id = string_field(&detail, "node_id").ok_or_else(|| {
        "GitHub did not return the pull request node identifier required for a revert.".to_string()
    })?;
    let request = authenticated_builder(http::post(graphql_url(locator)), Some(token.as_str()))
        .json(&revert_pull_request_payload(pull_request_node_id.as_str()));
    let payload: Value = http::send_json(
        request,
        RequestOptions::new("Create GitHub revert pull request"),
    )
    .await
    .map_err(|error| error.to_string())?;
    decode_revert_pull_request_result(&payload)
}

fn revert_pull_request_payload(pull_request_node_id: &str) -> Value {
    serde_json::json!({
        "query": "mutation RevertPullRequest($input: RevertPullRequestInput!) { revertPullRequest(input: $input) { revertPullRequest { number title url } } }",
        "variables": {
            "input": {
                "pullRequestId": pull_request_node_id
            }
        }
    })
}

fn decode_revert_pull_request_result(payload: &Value) -> Result<PullRequestRevertResult, String> {
    if let Some(message) = payload
        .get("errors")
        .and_then(Value::as_array)
        .and_then(|errors| errors.first())
        .and_then(|error| string_field(error, "message"))
    {
        return Err(message);
    }
    let pull_request = payload
        .get("data")
        .and_then(|data| data.get("revertPullRequest"))
        .and_then(|result| result.get("revertPullRequest"))
        .ok_or_else(|| {
            "GitHub did not return the newly created revert pull request.".to_string()
        })?;
    let number = pull_request
        .get("number")
        .and_then(Value::as_u64)
        .map(|number| number.to_string())
        .or_else(|| string_field(pull_request, "number"))
        .ok_or_else(|| {
            "GitHub revert response did not include a pull request number.".to_string()
        })?;
    Ok(PullRequestRevertResult {
        message: format!("Created revert pull request #{number}."),
        number,
        title: string_field(pull_request, "title").unwrap_or_default(),
        web_url: string_field(pull_request, "url").unwrap_or_default(),
    })
}

fn graphql_url(locator: &RepoLocator) -> String {
    graphql_url_from_rest_base(api_base_url(locator).as_str())
}

fn graphql_url_from_rest_base(rest_base: &str) -> String {
    if rest_base == endpoints::PUBLIC_API_BASE_URL {
        return endpoints::PUBLIC_GRAPHQL_URL.to_string();
    }
    rest_base
        .strip_suffix(endpoints::ENTERPRISE_API_VERSION_PATH)
        .map(|base| format!("{base}/api/graphql"))
        .unwrap_or_else(|| format!("{}/graphql", rest_base.trim_end_matches('/')))
}

fn authenticated_request(_locator: &RepoLocator, endpoint: String) -> reqwest::RequestBuilder {
    authenticated_builder(
        http::get(endpoint),
        configured_token(RepoType::GitHub).as_deref(),
    )
}

fn authenticated_builder(
    request: reqwest::RequestBuilder,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut request = request
        .header(ACCEPT, "application/vnd.github+json")
        .header(USER_AGENT, "usher-code-review")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = token {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    request
}

fn repository_endpoint(
    template: &str,
    locator: &RepoLocator,
    pull_request_number: Option<&str>,
    file_path: Option<&str>,
) -> String {
    let mut endpoint = template
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace(
            "{namespace}",
            percent_encode_path(locator.namespace.as_str()).as_str(),
        )
        .replace(
            "{repository}",
            percent_encode_path(locator.repo.as_str()).as_str(),
        );
    if let Some(number) = pull_request_number {
        endpoint = endpoint.replace(
            "{pull_request_number}",
            percent_encode_path(number.trim()).as_str(),
        );
    }
    if let Some(path) = file_path {
        endpoint = endpoint.replace("{file_path}", percent_encode_path(path).as_str());
    }
    endpoint
}

fn decode_base64_content(payload: &Value, provider: &str) -> Result<String, String> {
    let content = payload
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{provider} repository file response has no content."))?
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content.as_bytes())
        .map_err(|error| format!("{provider} repository file base64 decode failed: {error}"))?;
    String::from_utf8(bytes)
        .map_err(|error| format!("{provider} repository file is not UTF-8 text: {error}"))
}

fn value_u32(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn api_base_url(locator: &RepoLocator) -> String {
    if is_configured_enterprise_host(RepoType::GitHub, locator.host.as_str()) {
        if let Some(api_base_url) = configured_api_base_url(RepoType::GitHub) {
            return with_default_api_version(api_base_url, endpoints::ENTERPRISE_API_VERSION_PATH);
        }
    }
    endpoints::PUBLIC_API_BASE_URL.to_string()
}

fn with_default_api_version(api_base_url: String, required_path: &str) -> String {
    let has_required_path = reqwest::Url::parse(api_base_url.as_str())
        .ok()
        .map(|url| url.path().trim_end_matches('/').ends_with(required_path))
        .unwrap_or(false);
    if has_required_path {
        api_base_url
    } else {
        format!("{api_base_url}{required_path}")
    }
}

fn value_to_id(value: &serde_json::Value, provider: &str) -> Result<String, String> {
    match value {
        serde_json::Value::String(value) if !value.trim().is_empty() => Ok(value.clone()),
        serde_json::Value::Number(value) => Ok(value.to_string()),
        _ => Err(format!("{provider} project details did not include an id.")),
    }
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            value
                .get(key)
                .and_then(Value::as_i64)
                .map(|value| value.to_string())
        })
}

fn nested_string(value: &Value, parent: &str, key: &str) -> Option<String> {
    value.get(parent)?.get(key)?.as_str().map(str::to_string)
}

fn pull_request_branches(value: &Value) -> Option<(String, String)> {
    let source_branch = nested_string(value, "head", "ref")?;
    let target_branch = nested_string(value, "base", "ref")?;
    (!source_branch.trim().is_empty() && !target_branch.trim().is_empty())
        .then_some((source_branch, target_branch))
}

fn aggregate_review_state(reviews: &[Value]) -> String {
    let mut latest_by_user = std::collections::BTreeMap::new();
    for review in reviews {
        let Some(user) = nested_string(review, "user", "login") else {
            continue;
        };
        let state = string_field(review, "state")
            .unwrap_or_default()
            .to_ascii_uppercase();
        if matches!(
            state.as_str(),
            "APPROVED" | "CHANGES_REQUESTED" | "DISMISSED"
        ) {
            latest_by_user.insert(user, state);
        }
    }
    if latest_by_user
        .values()
        .any(|state| state == "CHANGES_REQUESTED")
    {
        "rejected".to_string()
    } else if latest_by_user.values().any(|state| state == "APPROVED") {
        "approved".to_string()
    } else {
        String::new()
    }
}

/// GitHub owns this mapping. Do not reuse it for Gitee or GitLab: GitHub's
/// `mergeable` and `mergeable_state` are provider-specific and `null` means
/// that GitHub is still computing a synthetic test merge.
fn github_merge_readiness(detail: &Value, review_state: &str) -> PullRequestReadiness {
    let state = string_field(detail, "state").unwrap_or_default();
    let merged = detail
        .get("merged")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || detail
            .get("merged_at")
            .is_some_and(|value| !value.is_null());
    if merged {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Merged,
            "GitHub reports that this pull request has been merged.",
        );
    }
    if state == "closed" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Closed,
            "GitHub reports that this pull request was closed without merging.",
        );
    }
    if state != "open" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Unavailable,
            "GitHub did not return a recognized open, closed, or merged state.",
        );
    }
    if detail
        .get("draft")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Draft,
            "GitHub draft pull requests cannot be merged until they are marked ready for review.",
        );
    }

    let mergeable = detail.get("mergeable").and_then(Value::as_bool);
    let mergeable_state = string_field(detail, "mergeable_state")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if mergeable.is_none() || mergeable_state == "unknown" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Checking,
            "GitHub is calculating a test merge for this pull request.",
        );
    }
    if mergeable == Some(false) || mergeable_state == "dirty" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Conflicts,
            "GitHub found conflicts between the head and base branches.",
        );
    }
    if review_state == "rejected" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::ChangesRequested,
            "A GitHub review is requesting changes before this pull request can be merged.",
        );
    }

    match mergeable_state.as_str() {
        "blocked" => PullRequestReadiness::new(
            PullRequestReadinessKind::Blocked,
            "GitHub branch protection, required reviews, checks, or another repository rule is blocking this merge.",
        ),
        "behind" => PullRequestReadiness::new(
            PullRequestReadinessKind::BranchOutOfDate,
            "GitHub requires the head branch to be updated with the base branch before merging.",
        ),
        "unstable" => PullRequestReadiness::new(
            PullRequestReadinessKind::ChecksPending,
            "GitHub reports non-passing commit status checks. They may still be pending or non-required.",
        ),
        "clean" | "has_hooks" | "" if mergeable == Some(true) => PullRequestReadiness::new(
            PullRequestReadinessKind::Ready,
            "GitHub reports a clean merge with no unmet merge requirement in the pull request state.",
        ),
        _ if mergeable == Some(true) => PullRequestReadiness::new(
            PullRequestReadinessKind::Ready,
            format!(
                "GitHub reports this pull request as mergeable (provider state: {}).",
                if mergeable_state.is_empty() {
                    "mergeable"
                } else {
                    mergeable_state.as_str()
                }
            ),
        ),
        _ => PullRequestReadiness::new(
            PullRequestReadinessKind::Unavailable,
            "GitHub did not return enough information to determine merge readiness.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        aggregate_review_state, decode_commit, decode_github_timeline_activity,
        decode_revert_pull_request_result, github_merge_readiness, github_review_action,
        graphql_url_from_rest_base, link_rel_last_page, open_pull_request_count_query,
        pull_request_branches, revert_pull_request_payload, single_item_page_total,
    };
    use crate::git::{PullRequestReadinessKind, parse_repo_locator};

    #[test]
    fn scopes_overview_count_to_repository_state_and_target_branch() {
        let locator = parse_repo_locator("https://github.com/acme/widget.git").unwrap();
        assert_eq!(
            open_pull_request_count_query(&locator, "fat"),
            "repo:acme/widget is:pr is:open base:fat"
        );
    }

    #[test]
    fn parses_github_link_rel_last_page_for_commit_badge_counts() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::LINK,
            reqwest::header::HeaderValue::from_static(
                r#"<https://api.github.com/repos/o/r/pulls/1/commits?page=2>; rel="next", <https://api.github.com/repos/o/r/pulls/1/commits?page=14>; rel="last""#,
            ),
        );
        assert_eq!(link_rel_last_page(&headers), Some(14));
        assert_eq!(single_item_page_total(&headers, 1), Some(14));
        assert_eq!(single_item_page_total(&Default::default(), 0), Some(0));
        assert_eq!(single_item_page_total(&Default::default(), 1), Some(1));
    }

    #[test]
    fn builds_github_revert_mutation_and_decodes_the_created_pull_request() {
        let request = revert_pull_request_payload("PR_node_123");
        assert_eq!(
            request["variables"]["input"]["pullRequestId"],
            "PR_node_123"
        );
        assert!(
            request["query"]
                .as_str()
                .unwrap()
                .contains("revertPullRequest")
        );

        let result = decode_revert_pull_request_result(&json!({
            "data": {
                "revertPullRequest": {
                    "revertPullRequest": {
                        "number": 42,
                        "title": "Revert \"Feature\"",
                        "url": "https://github.example/o/r/pull/42"
                    }
                }
            }
        }))
        .unwrap();
        assert_eq!(result.number, "42");
        assert_eq!(result.title, "Revert \"Feature\"");
        assert_eq!(result.message, "Created revert pull request #42.");
    }

    #[test]
    fn surfaces_github_revert_graphql_errors() {
        let error = decode_revert_pull_request_result(&json!({
            "errors": [{ "message": "Revert conflict" }]
        }))
        .unwrap_err();
        assert_eq!(error, "Revert conflict");
    }

    #[test]
    fn derives_public_and_enterprise_github_graphql_urls() {
        assert_eq!(
            graphql_url_from_rest_base("https://api.github.com"),
            "https://api.github.com/graphql"
        );
        assert_eq!(
            graphql_url_from_rest_base("https://github.example/api/v3"),
            "https://github.example/api/graphql"
        );
    }

    #[test]
    fn reads_head_and_base_refs_from_a_pull_request_detail() {
        let payload = json!({
            "head": { "ref": "feature/pr-branches" },
            "base": { "ref": "main" }
        });

        assert_eq!(
            pull_request_branches(&payload),
            Some(("feature/pr-branches".to_string(), "main".to_string()))
        );
    }

    #[test]
    fn decodes_github_pull_request_commit() {
        let commit = decode_commit(&json!({
            "sha": "1234567890abcdef",
            "author": {
                "login": "octocat",
                "avatar_url": "https://avatars.example.test/octocat"
            },
            "commit": {
                "message": "Improve PR navigation\n\nKeep list state.",
                "author": { "name": "Octo Cat", "date": "2026-07-20T10:00:00Z" },
                "verification": { "verified": true }
            }
        }));
        assert_eq!(commit.sha, "1234567890abcdef");
        assert_eq!(commit.title, "Improve PR navigation");
        assert_eq!(commit.author, "octocat");
        assert_eq!(commit.avatar_url, "https://avatars.example.test/octocat");
        assert!(commit.verified);
    }

    #[test]
    fn rejects_missing_or_empty_branch_refs() {
        assert_eq!(
            pull_request_branches(&json!({ "head": { "ref": "feature" } })),
            None
        );
        assert_eq!(
            pull_request_branches(&json!({
                "head": { "ref": "" },
                "base": { "ref": "main" }
            })),
            None
        );
    }

    #[test]
    fn aggregates_latest_github_review_state_per_user() {
        let reviews = vec![
            json!({ "user": { "login": "alice" }, "state": "APPROVED" }),
            json!({ "user": { "login": "bob" }, "state": "CHANGES_REQUESTED" }),
            json!({ "user": { "login": "bob" }, "state": "DISMISSED" }),
        ];
        assert_eq!(aggregate_review_state(&reviews), "approved");
    }

    #[test]
    fn maps_github_review_states_to_timeline_actions() {
        assert_eq!(github_review_action("commented"), "commented");
        assert_eq!(github_review_action("approved"), "approved these changes");
        assert_eq!(
            github_review_action("changes_requested"),
            "requested changes"
        );
    }

    #[test]
    fn merged_timeline_suppresses_the_redundant_closed_event() {
        let timeline = vec![
            json!({
                "id": 1,
                "event": "merged",
                "actor": { "login": "alice" },
                "commit_id": "1234567890abcdef",
                "created_at": "2026-07-21T00:00:00Z"
            }),
            json!({
                "id": 2,
                "event": "closed",
                "actor": { "login": "alice" },
                "created_at": "2026-07-21T00:00:01Z"
            }),
        ];
        let items = decode_github_timeline_activity(&timeline, "main");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "merged");
    }

    #[test]
    fn github_null_mergeable_is_checking_not_conflicting() {
        let readiness = github_merge_readiness(
            &json!({
                "state": "open",
                "draft": false,
                "mergeable": null,
                "mergeable_state": "unknown"
            }),
            "",
        );
        assert_eq!(readiness.kind, PullRequestReadinessKind::Checking);
    }

    #[test]
    fn github_dirty_state_is_conflicting() {
        let readiness = github_merge_readiness(
            &json!({
                "state": "open",
                "draft": false,
                "mergeable": false,
                "mergeable_state": "dirty"
            }),
            "",
        );
        assert_eq!(readiness.kind, PullRequestReadinessKind::Conflicts);
    }

    #[test]
    fn github_clean_state_is_ready() {
        let readiness = github_merge_readiness(
            &json!({
                "state": "open",
                "draft": false,
                "mergeable": true,
                "mergeable_state": "clean"
            }),
            "approved",
        );
        assert_eq!(readiness.kind, PullRequestReadinessKind::Ready);
    }
}
