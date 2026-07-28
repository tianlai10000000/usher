//! GitLab repository operations.

use reqwest::header::{ACCEPT, HeaderMap};
use serde::Deserialize;
use serde_json::Value;

use crate::codereview::api::gitlab as endpoints;
use crate::http::{self, RequestOptions};

use super::{
    PullRequestActionState, PullRequestCommit, PullRequestConversationItem, PullRequestDiffDetail,
    PullRequestDiffFile, PullRequestItem, PullRequestPage, PullRequestReadiness,
    PullRequestReadinessKind, PullRequestState, RepoLocator, RepoType, commit_count_from_detail,
    configured_api_base_url, configured_token, conversation_commit_batch_items,
    conversation_count_from_detail, conversation_merged_item, finalize_conversation_timeline,
    is_configured_enterprise_host, is_redundant_commit_system_note, is_redundant_merge_system_note,
    parse_unified_diff, percent_encode_path, unavailable_diff_file,
};

#[derive(Deserialize)]
struct GitLabProject {
    id: serde_json::Value,
}

pub(super) async fn get_repo_detail(locator: &RepoLocator) -> Result<String, String> {
    let full_path = format!("{}/{}", locator.namespace, locator.repo);
    let endpoint = endpoints::PROJECT_URL
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace(
            "{project_id_or_path}",
            percent_encode_path(full_path.as_str()).as_str(),
        );
    let mut request = http::get(endpoint).header(ACCEPT, "application/json");
    if let Some(token) = configured_token(RepoType::GitLab) {
        request = request.header("PRIVATE-TOKEN", token);
    }

    let response: GitLabProject =
        http::send_json(request, RequestOptions::new("Load GitLab project details"))
            .await
            .map_err(|error| error.to_string())?;
    value_to_id(&response.id, "GitLab")
}

pub(super) async fn get_pull_requests(
    locator: &RepoLocator,
    repo_project_id: &str,
    state: Option<PullRequestState>,
    page: u32,
    per_page: u32,
) -> Result<PullRequestPage, String> {
    let repo_project_id = repo_project_id.trim();
    if repo_project_id.is_empty() {
        return Err("GitLab project id is empty.".to_string());
    }
    let per_page = per_page.max(1);
    let endpoint = endpoints::MERGE_REQUESTS_URL
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace(
            "{project_id}",
            percent_encode_path(repo_project_id).as_str(),
        );
    let mut request = http::get(endpoint)
        .header(ACCEPT, "application/json")
        .query(&[
            ("per_page", &per_page.to_string()),
            ("page", &page.max(1).to_string()),
        ]);
    if let Some(state) = state {
        request = request.query(&[("state", state.gitlab_query())]);
    }
    if let Some(token) = configured_token(RepoType::GitLab) {
        request = request.header("PRIVATE-TOKEN", token);
    }

    let response = http::send(request, RequestOptions::new("Load GitLab merge requests"))
        .await
        .map_err(|error| error.to_string())?;
    let headers = response.headers().clone();
    let payload: Vec<Value> = response
        .json()
        .await
        .map_err(|error| format!("GitLab merge request response decode failed: {error}"))?;
    let items = payload
        .iter()
        .map(|value| PullRequestItem {
            number: string_field(value, "iid").unwrap_or_else(|| "—".to_string()),
            title: string_field(value, "title")
                .unwrap_or_else(|| "Untitled merge request".to_string()),
            state: string_field(value, "state").unwrap_or_else(|| "open".to_string()),
            author: nested_string(value, "author", "name")
                .or_else(|| nested_string(value, "author", "username"))
                .unwrap_or_else(|| "Unknown author".to_string()),
            source_branch: string_field(value, "source_branch").unwrap_or_else(|| "—".to_string()),
            target_branch: string_field(value, "target_branch").unwrap_or_else(|| "—".to_string()),
            updated_at: string_field(value, "updated_at")
                .unwrap_or_else(|| "Unknown update time".to_string()),
            web_url: string_field(value, "web_url").unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    let total = pagination_total(&headers);
    Ok(PullRequestPage {
        total: total.unwrap_or_else(|| fallback_total(page, per_page, items.len())),
        total_exact: total.is_some(),
        page_count: header_u32(&headers, "x-total-pages"),
        items,
    })
}

pub(super) async fn get_pull_request_diff(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
    ignore_whitespace_change: bool,
) -> Result<PullRequestDiffDetail, String> {
    let project_id = percent_encode_path(repo_project_id.trim());
    let merge_request_iid = percent_encode_path(pull_request_iid.trim());
    let detail_endpoint = endpoints::MERGE_REQUEST_URL
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace("{project_id}", project_id.as_str())
        .replace("{merge_request_iid}", merge_request_iid.as_str());
    let mut detail_request = http::get(detail_endpoint).header(ACCEPT, "application/json");
    if let Some(token) = configured_token(RepoType::GitLab) {
        detail_request = detail_request.header("PRIVATE-TOKEN", token);
    }
    let detail: Value = http::send_json(
        detail_request,
        RequestOptions::new("Load GitLab merge request details"),
    )
    .await
    .map_err(|error| error.to_string())?;

    let endpoint = endpoints::MERGE_REQUEST_DIFFS_URL
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace("{project_id}", project_id.as_str())
        .replace("{merge_request_iid}", merge_request_iid.as_str());
    let mut page = 1u32;
    let mut values = Vec::new();
    loop {
        let mut request = http::get(endpoint.as_str())
            .header(ACCEPT, "application/json")
            .query(&[("page", page.to_string()), ("per_page", "100".to_string())]);
        if ignore_whitespace_change {
            // Standard GitLab does not currently expose a whitespace query on
            // this endpoint. Keep the option provider-neutral without sending
            // an unsupported parameter.
            log::debug!(
                "[CodeReview][GitLab] Ignore-whitespace is not supported by the diff endpoint"
            );
        }
        if let Some(token) = configured_token(RepoType::GitLab) {
            request = request.header("PRIVATE-TOKEN", token);
        }
        let batch: Vec<Value> = http::send_json(
            request,
            RequestOptions::new("Load GitLab merge request diffs"),
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
    let refs = detail.get("diff_refs").unwrap_or(&Value::Null);
    let commit_count = match commit_count_from_detail(&detail) {
        Some(count) => count,
        None => {
            // Stock GitLab often omits commits_count on MR detail; one lightweight
            // page with X-Total is enough for the Commits tab badge.
            count_merge_request_commits(locator, repo_project_id, pull_request_iid)
                .await
                .unwrap_or(0)
        }
    };
    Ok(PullRequestDiffDetail {
        added_lines: files.iter().map(|file| file.added_lines).sum(),
        removed_lines: files.iter().map(|file| file.removed_lines).sum(),
        commit_count,
        conversation_count: Some(conversation_count_from_detail(&detail).unwrap_or(0)),
        base_sha: string_field(refs, "base_sha").unwrap_or_default(),
        head_sha: string_field(refs, "head_sha").unwrap_or_default(),
        start_sha: string_field(refs, "start_sha").unwrap_or_default(),
        too_many_commits: values.iter().any(|value| {
            value
                .get("too_large")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || value
                    .get("collapsed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        }),
        files,
    })
}

pub(super) async fn get_commit_range_diff(
    locator: &RepoLocator,
    repo_project_id: &str,
    base_sha: &str,
    head_sha: &str,
    commit_count: u32,
) -> Result<PullRequestDiffDetail, String> {
    let endpoint = endpoints::REPOSITORY_COMPARE_URL
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace(
            "{project_id}",
            percent_encode_path(repo_project_id.trim()).as_str(),
        );
    let payload: Value = http::send_json(
        gitlab_request(endpoint).query(&[
            ("from", base_sha.trim()),
            ("to", head_sha.trim()),
            ("straight", "true"),
            ("unidiff", "true"),
        ]),
        RequestOptions::new("Compare GitLab merge request commits"),
    )
    .await
    .map_err(|error| error.to_string())?;
    let values = payload
        .get("diffs")
        .and_then(Value::as_array)
        .ok_or_else(|| "GitLab commit comparison response has no diff list.".to_string())?;
    let files = values.iter().map(decode_diff_file).collect::<Vec<_>>();
    Ok(PullRequestDiffDetail {
        added_lines: files.iter().map(|file| file.added_lines).sum(),
        removed_lines: files.iter().map(|file| file.removed_lines).sum(),
        commit_count,
        conversation_count: None,
        base_sha: base_sha.to_string(),
        head_sha: head_sha.to_string(),
        start_sha: base_sha.to_string(),
        too_many_commits: payload
            .get("compare_timeout")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        files,
    })
}

pub(super) async fn get_pull_request_action_state(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
) -> Result<PullRequestActionState, String> {
    let endpoint = endpoints::MERGE_REQUEST_URL
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace(
            "{project_id}",
            percent_encode_path(repo_project_id.trim()).as_str(),
        )
        .replace(
            "{merge_request_iid}",
            percent_encode_path(pull_request_iid.trim()).as_str(),
        );
    let mut request = http::get(endpoint)
        .header(ACCEPT, "application/json")
        .query(&[("with_merge_status_recheck", "true")]);
    if let Some(token) = configured_token(RepoType::GitLab) {
        request = request.header("PRIVATE-TOKEN", token);
    }
    let detail: Value = http::send_json(
        request,
        RequestOptions::new("Load GitLab merge request readiness"),
    )
    .await
    .map_err(|error| error.to_string())?;
    let readiness = gitlab_merge_readiness(&detail);
    let review_state = match string_field(&detail, "detailed_merge_status").as_deref() {
        Some("requested_changes") => "rejected",
        Some("not_approved") | Some("approvals_syncing") => "pending",
        _ => "",
    }
    .to_string();
    let merge_block_reason = if readiness.kind == PullRequestReadinessKind::Ready {
        "GitLab reports this merge request as ready, but remote review and merge actions are not implemented in Usher yet."
            .to_string()
    } else {
        readiness.detail.clone()
    };
    Ok(PullRequestActionState {
        can_comment: false,
        can_approve: false,
        can_request_changes: false,
        can_merge: false,
        can_revert: false,
        review_state,
        readiness,
        merge_block_reason,
        revert_block_reason: "Automated merge-request revert is not implemented for GitLab yet."
            .to_string(),
        revert_action_label: "Revert changes".to_string(),
        revert_confirmation:
            "This provider does not currently expose a supported automatic revert operation."
                .to_string(),
        allowed_merge_methods: Vec::new(),
    })
}

pub(super) async fn get_pull_request_conversation(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
) -> Result<Vec<PullRequestConversationItem>, String> {
    let detail_endpoint = merge_request_endpoint(
        endpoints::MERGE_REQUEST_URL,
        locator,
        repo_project_id,
        pull_request_iid,
    );
    let notes_endpoint = merge_request_endpoint(
        endpoints::MERGE_REQUEST_NOTES_URL,
        locator,
        repo_project_id,
        pull_request_iid,
    );
    let commits_endpoint = merge_request_endpoint(
        endpoints::MERGE_REQUEST_COMMITS_URL,
        locator,
        repo_project_id,
        pull_request_iid,
    );
    let detail_future = async {
        http::send_json(
            gitlab_request(detail_endpoint),
            RequestOptions::new("Load GitLab merge request conversation header"),
        )
        .await
        .map_err(|error| error.to_string())
    };
    let notes_future = async {
        http::send_json(
            gitlab_request(notes_endpoint).query(&[("per_page", "100")]),
            RequestOptions::new("Load GitLab merge request notes"),
        )
        .await
        .map_err(|error| error.to_string())
    };
    let commits_future = async {
        http::send_json(
            gitlab_request(commits_endpoint).query(&[("per_page", "100")]),
            RequestOptions::new("Load GitLab merge request conversation commits"),
        )
        .await
        .map_err(|error| error.to_string())
    };
    let (detail, notes, commit_values): (Value, Vec<Value>, Vec<Value>) =
        tokio::try_join!(detail_future, notes_future, commits_future)?;

    let description = PullRequestConversationItem {
        id: format!("description-{pull_request_iid}"),
        kind: "description".to_string(),
        author: nested_string(&detail, "author", "name")
            .or_else(|| nested_string(&detail, "author", "username"))
            .unwrap_or_else(|| "Unknown author".to_string()),
        avatar_url: nested_string(&detail, "author", "avatar_url").unwrap_or_default(),
        author_association: String::new(),
        body: string_field(&detail, "description").unwrap_or_default(),
        created_at: string_field(&detail, "created_at").unwrap_or_default(),
        state: string_field(&detail, "state").unwrap_or_default(),
    };

    let mut activity = notes
        .into_iter()
        .filter_map(|value| {
            let body = string_field(&value, "body").unwrap_or_default();
            let is_system = value
                .get("system")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if is_system
                && (is_redundant_commit_system_note(body.as_str())
                    || is_redundant_merge_system_note(body.as_str()))
            {
                return None;
            }
            Some(PullRequestConversationItem {
                id: string_field(&value, "id").unwrap_or_default(),
                kind: if is_system {
                    "event".to_string()
                } else {
                    "comment".to_string()
                },
                author: nested_string(&value, "author", "name")
                    .or_else(|| nested_string(&value, "author", "username"))
                    .unwrap_or_else(|| "Unknown author".to_string()),
                avatar_url: nested_string(&value, "author", "avatar_url").unwrap_or_default(),
                author_association: String::new(),
                body,
                created_at: string_field(&value, "created_at").unwrap_or_default(),
                state: String::new(),
            })
        })
        .collect::<Vec<_>>();

    let commits = commit_values
        .into_iter()
        .map(|value| {
            let message = string_field(&value, "message").unwrap_or_default();
            PullRequestCommit {
                sha: string_field(&value, "id")
                    .or_else(|| string_field(&value, "sha"))
                    .unwrap_or_default(),
                title: string_field(&value, "title")
                    .unwrap_or_else(|| message.lines().next().unwrap_or_default().to_string()),
                message,
                author: string_field(&value, "author_name")
                    .unwrap_or_else(|| "Unknown author".to_string()),
                avatar_url: string_field(&value, "author_avatar_url")
                    .or_else(|| nested_string(&value, "author", "avatar_url"))
                    .unwrap_or_default(),
                authored_at: string_field(&value, "authored_date")
                    .or_else(|| string_field(&value, "created_at"))
                    .unwrap_or_default(),
                verified: false,
            }
        })
        .collect::<Vec<_>>();
    // GitLab returns newest-first; normalize to oldest→newest before the shared
    // Conversation batch builder (which renders newest-first for display).
    let commits = {
        let mut commits = commits;
        commits.reverse();
        commits
    };
    activity.extend(conversation_commit_batch_items(&commits));

    if let Some(merged_at) =
        string_field(&detail, "merged_at").filter(|value| !value.trim().is_empty())
    {
        let merge_sha = string_field(&detail, "merge_commit_sha").unwrap_or_default();
        let target_branch = string_field(&detail, "target_branch").unwrap_or_default();
        let author = nested_string(&detail, "merged_by", "name")
            .or_else(|| nested_string(&detail, "merged_by", "username"))
            .or_else(|| nested_string(&detail, "author", "name"))
            .unwrap_or_else(|| "Unknown author".to_string());
        if !merge_sha.trim().is_empty() {
            activity.push(conversation_merged_item(
                author.as_str(),
                merge_sha.as_str(),
                if target_branch.trim().is_empty() {
                    "main"
                } else {
                    target_branch.as_str()
                },
                merged_at.as_str(),
            ));
        }
    }

    Ok(finalize_conversation_timeline(description, activity))
}

pub(super) async fn get_pull_request_commits(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
) -> Result<Vec<PullRequestCommit>, String> {
    let endpoint = merge_request_endpoint(
        endpoints::MERGE_REQUEST_COMMITS_URL,
        locator,
        repo_project_id,
        pull_request_iid,
    );
    let values: Vec<Value> = http::send_json(
        gitlab_request(endpoint).query(&[("per_page", "100")]),
        RequestOptions::new("Load GitLab merge request commits"),
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(values
        .into_iter()
        .map(|value| {
            let message = string_field(&value, "message").unwrap_or_default();
            PullRequestCommit {
                sha: string_field(&value, "id")
                    .or_else(|| string_field(&value, "sha"))
                    .unwrap_or_default(),
                title: string_field(&value, "title")
                    .unwrap_or_else(|| message.lines().next().unwrap_or_default().to_string()),
                message,
                author: string_field(&value, "author_name")
                    .unwrap_or_else(|| "Unknown author".to_string()),
                avatar_url: string_field(&value, "author_avatar_url")
                    .or_else(|| nested_string(&value, "author", "avatar_url"))
                    .unwrap_or_default(),
                authored_at: string_field(&value, "authored_date")
                    .or_else(|| string_field(&value, "created_at"))
                    .unwrap_or_default(),
                verified: false,
            }
        })
        .collect())
}

async fn count_merge_request_commits(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
) -> Option<u32> {
    let endpoint = merge_request_endpoint(
        endpoints::MERGE_REQUEST_COMMITS_URL,
        locator,
        repo_project_id,
        pull_request_iid,
    );
    let response = http::send(
        gitlab_request(endpoint).query(&[("page", "1"), ("per_page", "1")]),
        RequestOptions::new("Count GitLab merge request commits"),
    )
    .await
    .ok()?;
    let headers = response.headers().clone();
    pagination_total(&headers)
}

fn merge_request_endpoint(
    template: &str,
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
) -> String {
    template
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace(
            "{project_id}",
            percent_encode_path(repo_project_id.trim()).as_str(),
        )
        .replace(
            "{merge_request_iid}",
            percent_encode_path(pull_request_iid.trim()).as_str(),
        )
}

fn gitlab_request(endpoint: String) -> reqwest::RequestBuilder {
    let mut request = http::get(endpoint).header(ACCEPT, "application/json");
    if let Some(token) = configured_token(RepoType::GitLab) {
        request = request.header("PRIVATE-TOKEN", token);
    }
    request
}

fn gitlab_merge_readiness(detail: &Value) -> PullRequestReadiness {
    let state = string_field(detail, "state")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if state == "merged" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Merged,
            "GitLab reports that this merge request has been merged.",
        );
    }
    if state == "closed" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Closed,
            "GitLab reports that this merge request is closed.",
        );
    }
    if detail.get("draft").and_then(Value::as_bool) == Some(true)
        || detail.get("work_in_progress").and_then(Value::as_bool) == Some(true)
    {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Draft,
            "Mark the GitLab merge request as ready before merging.",
        );
    }
    if detail.get("has_conflicts").and_then(Value::as_bool) == Some(true) {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Conflicts,
            "GitLab reports merge conflicts between the source and target branches.",
        );
    }

    let status = string_field(detail, "detailed_merge_status")
        .unwrap_or_else(|| string_field(detail, "merge_status").unwrap_or_default());
    match status.as_str() {
        "mergeable" | "can_be_merged" => PullRequestReadiness::new(
            PullRequestReadinessKind::Ready,
            "GitLab reports that all current merge requirements are satisfied.",
        ),
        "unchecked" | "checking" | "preparing" | "approvals_syncing" => PullRequestReadiness::new(
            PullRequestReadinessKind::Checking,
            "GitLab is calculating the mergeability of this merge request.",
        ),
        "conflict" | "cannot_be_merged" => PullRequestReadiness::new(
            PullRequestReadinessKind::Conflicts,
            "GitLab reports merge conflicts between the source and target branches.",
        ),
        "not_approved" => PullRequestReadiness::new(
            PullRequestReadinessKind::ReviewRequired,
            "Required GitLab approvals have not been completed.",
        ),
        "requested_changes" => PullRequestReadiness::new(
            PullRequestReadinessKind::ChangesRequested,
            "A GitLab reviewer requested changes.",
        ),
        "discussions_not_resolved" => PullRequestReadiness::new(
            PullRequestReadinessKind::DiscussionsUnresolved,
            "Resolve all required GitLab discussions before merging.",
        ),
        "draft_status" => PullRequestReadiness::new(
            PullRequestReadinessKind::Draft,
            "Mark the GitLab merge request as ready before merging.",
        ),
        "need_rebase" => PullRequestReadiness::new(
            PullRequestReadinessKind::BranchOutOfDate,
            "GitLab requires the source branch to be rebased before merging.",
        ),
        "ci_still_running" => PullRequestReadiness::new(
            PullRequestReadinessKind::ChecksPending,
            "The required GitLab CI pipeline is still running.",
        ),
        "ci_must_pass"
        | "commits_status"
        | "security_policy_pipeline_check"
        | "security_policy_violations"
        | "status_checks_must_pass" => PullRequestReadiness::new(
            PullRequestReadinessKind::ChecksFailed,
            "A required GitLab pipeline, status check, or security policy has not passed.",
        ),
        "not_open" => PullRequestReadiness::new(
            PullRequestReadinessKind::Closed,
            "GitLab reports that this merge request is not open.",
        ),
        "merge_request_blocked"
        | "merge_time"
        | "jira_association_missing"
        | "locked_paths"
        | "locked_lfs_files"
        | "title_regex" => PullRequestReadiness::new(
            PullRequestReadinessKind::Blocked,
            "A GitLab repository rule or merge condition is blocking this merge request.",
        ),
        _ => PullRequestReadiness::new(
            PullRequestReadinessKind::Unavailable,
            "GitLab did not return enough information to determine merge readiness.",
        ),
    }
}

fn decode_diff_file(value: &Value) -> PullRequestDiffFile {
    let path = string_field(value, "new_path").unwrap_or_else(|| "Unknown file".to_string());
    let old_path = string_field(value, "old_path").unwrap_or_else(|| path.clone());
    let change_type = if value
        .get("new_file")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "new_file"
    } else if value
        .get("deleted_file")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "deleted_file"
    } else if value
        .get("renamed_file")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "renamed_file"
    } else {
        "edit_file"
    }
    .to_string();
    let lines = string_field(value, "diff")
        .map(|diff| parse_unified_diff(diff.as_str()))
        .unwrap_or_default();
    let added_lines = lines.iter().filter(|line| line.kind == "addition").count() as u32;
    let removed_lines = lines.iter().filter(|line| line.kind == "deletion").count() as u32;
    if lines.is_empty() {
        return unavailable_diff_file(
            path,
            old_path,
            change_type,
            added_lines,
            removed_lines,
            "GitLab omitted this binary, collapsed, or oversized diff.",
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
    repo_project_id: &str,
    path: &str,
    reference: &str,
) -> Result<String, String> {
    let endpoint = endpoints::REPOSITORY_FILE_RAW_URL
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace(
            "{project_id}",
            percent_encode_path(repo_project_id.trim()).as_str(),
        )
        .replace("{file_path}", percent_encode_path(path).as_str());
    let mut request = http::get(endpoint)
        .header(ACCEPT, "text/plain")
        .query(&[("ref", reference)]);
    if let Some(token) = configured_token(RepoType::GitLab) {
        request = request.header("PRIVATE-TOKEN", token);
    }
    let response = http::send(request, RequestOptions::new("Load GitLab repository file"))
        .await
        .map_err(|error| error.to_string())?;
    response
        .text()
        .await
        .map_err(|error| format!("GitLab repository file decode failed: {error}"))
}

fn api_base_url(locator: &RepoLocator) -> String {
    if is_configured_enterprise_host(RepoType::GitLab, locator.host.as_str()) {
        if let Some(api_base_url) = configured_api_base_url(RepoType::GitLab) {
            return with_default_api_version(api_base_url, endpoints::API_VERSION_PATH);
        }
    }
    format!(
        "{}://{}{}",
        locator.scheme,
        locator.authority,
        endpoints::API_VERSION_PATH
    )
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

fn pagination_total(headers: &HeaderMap) -> Option<u32> {
    ["x-total", "x-total-count", "total-count"]
        .into_iter()
        .find_map(|header| header_u32(headers, header))
}

fn header_u32(headers: &HeaderMap, name: &str) -> Option<u32> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u32>().ok())
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

fn fallback_total(page: u32, per_page: u32, item_count: usize) -> u32 {
    page.saturating_sub(1)
        .saturating_mul(per_page)
        .saturating_add(item_count as u32)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::gitlab_merge_readiness;
    use crate::git::PullRequestReadinessKind;

    #[test]
    fn maps_gitlab_mergeable_status_to_ready() {
        let readiness = gitlab_merge_readiness(&json!({
            "state": "opened",
            "detailed_merge_status": "mergeable",
            "has_conflicts": false
        }));

        assert_eq!(readiness.kind, PullRequestReadinessKind::Ready);
    }

    #[test]
    fn maps_gitlab_async_calculation_to_checking() {
        let readiness = gitlab_merge_readiness(&json!({
            "state": "opened",
            "detailed_merge_status": "checking"
        }));

        assert_eq!(readiness.kind, PullRequestReadinessKind::Checking);
    }

    #[test]
    fn maps_gitlab_conflict_independently_of_other_providers() {
        let readiness = gitlab_merge_readiness(&json!({
            "state": "opened",
            "detailed_merge_status": "conflict",
            "has_conflicts": true
        }));

        assert_eq!(readiness.kind, PullRequestReadinessKind::Conflicts);
    }

    #[test]
    fn maps_gitlab_missing_approval_to_review_required() {
        let readiness = gitlab_merge_readiness(&json!({
            "state": "opened",
            "detailed_merge_status": "not_approved"
        }));

        assert_eq!(readiness.kind, PullRequestReadinessKind::ReviewRequired);
    }
}
