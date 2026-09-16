//! GitLab repository operations.

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use futures_util::{StreamExt, stream};
use reqwest::header::{ACCEPT, HeaderMap};
use serde::Deserialize;
use serde_json::Value;

use crate::codereview::api::gitlab as endpoints;
use crate::http::{self, RequestOptions};

use super::json::{
    fallback_total, header_u32, nested_string, string_field, value_to_id, value_u32,
};
use super::prefix_cache::PrefixCache;
use super::{
    PageCollector, PullRequestActionState, PullRequestCommit, PullRequestConversationItem,
    PullRequestDiffDetail, PullRequestDiffFile, PullRequestFilters, PullRequestItem,
    PullRequestMergeBlockKind, PullRequestMergeMethod, PullRequestPage, PullRequestReadiness,
    PullRequestReadinessKind, PullRequestRevertResult, PullRequestRevertStrategy,
    PullRequestReviewAction, PullRequestReviewStatus, PullRequestState, RepoLocator, RepoType,
    commit_count_from_detail, configured_api_base_url, configured_token,
    conversation_body_to_plain_text, conversation_commit_batch_items,
    conversation_commit_batch_items_for_actor, conversation_count_from_detail,
    conversation_merged_item, finalize_conversation_timeline, is_configured_enterprise_host,
    is_redundant_commit_system_note, is_redundant_merge_system_note, parse_unified_diff,
    percent_encode_path, unavailable_diff_file,
};

#[derive(Deserialize)]
struct GitLabProject {
    id: serde_json::Value,
}

const PREFIX_CACHE_CAPACITY: usize = 32;
const PREFIX_CACHE_TTL: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PrefixCacheKey {
    api_base_url: String,
    project_id: String,
    state: String,
    query: String,
    reviews: String,
    assignee: String,
    sort: String,
    page_size: u32,
}

#[derive(Clone)]
struct PrefixCacheEntry {
    items: Vec<PullRequestItem>,
    total: u32,
    total_exact: bool,
    exhausted: bool,
}

fn prefix_cache() -> &'static Mutex<PrefixCache<PrefixCacheKey, PrefixCacheEntry>> {
    static CACHE: OnceLock<Mutex<PrefixCache<PrefixCacheKey, PrefixCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(PrefixCache::new(PREFIX_CACHE_CAPACITY, PREFIX_CACHE_TTL)))
}

fn prefix_cache_key(
    locator: &RepoLocator,
    repo_project_id: &str,
    state: PullRequestState,
    filters: &PullRequestFilters,
    page_size: u32,
) -> PrefixCacheKey {
    PrefixCacheKey {
        api_base_url: api_base_url(locator),
        project_id: repo_project_id.trim().to_string(),
        state: state.as_str().to_string(),
        query: filters.query.trim().to_string(),
        reviews: filters.reviews.clone(),
        assignee: filters.assignee.clone(),
        sort: filters.sort.clone(),
        page_size,
    }
}

fn invalidate_prefix_cache(locator: &RepoLocator, repo_project_id: &str) {
    let api_base_url = api_base_url(locator);
    let project_id = repo_project_id.trim();
    prefix_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .invalidate_where(|key| key.api_base_url == api_base_url && key.project_id == project_id);
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

    let response: GitLabProject = http::send_json(
        request,
        RequestOptions::new("Load GitLab project details").retry_transient(3),
    )
    .await
    .map_err(|error| error.to_string())?;
    value_to_id(Some(&response.id), "GitLab")
}

pub(super) async fn get_pull_requests(
    locator: &RepoLocator,
    repo_project_id: &str,
    state: Option<PullRequestState>,
    filters: &PullRequestFilters,
    page: u32,
    per_page: u32,
) -> Result<PullRequestPage, String> {
    if state == Some(PullRequestState::Closed) {
        return get_gitlab_closed_pull_requests(locator, repo_project_id, filters, page, per_page)
            .await;
    }
    get_gitlab_pull_request_server_page(
        locator,
        repo_project_id,
        state,
        filters,
        page,
        per_page,
        true,
    )
    .await
}

async fn get_gitlab_pull_request_server_page(
    locator: &RepoLocator,
    repo_project_id: &str,
    state: Option<PullRequestState>,
    filters: &PullRequestFilters,
    page: u32,
    per_page: u32,
    hydrate_visible: bool,
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
    let (order_by, sort) = gitlab_list_sort(filters.sort.as_str());
    let per_page_query = per_page.to_string();
    let page_query = page.max(1).to_string();
    let mut request = http::get(endpoint)
        .header(ACCEPT, "application/json")
        .query(&[
            ("per_page", per_page_query.as_str()),
            ("page", page_query.as_str()),
            ("order_by", order_by),
            ("sort", sort),
            ("with_merge_status_recheck", "true"),
        ]);
    if let Some(state) = state {
        request = request.query(&[("state", state.gitlab_query())]);
    }
    if !filters.query.trim().is_empty() {
        request = request.query(&[("search", filters.query.trim()), ("in", "title")]);
    }
    request = match filters.assignee.as_str() {
        "none" => request
            .query(&[("scope", "all")])
            .query(&[("assignee_id", "None")]),
        "me" => request.query(&[("scope", "assigned_to_me")]),
        _ => request.query(&[("scope", "all")]),
    };
    if let Some(token) = configured_token(RepoType::GitLab) {
        request = request.header("PRIVATE-TOKEN", token);
    }

    let response = http::send(
        request,
        RequestOptions::new("Load GitLab merge requests").retry_transient(3),
    )
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
            title: string_field(value, "title").unwrap_or_default(),
            state: string_field(value, "state").unwrap_or_else(|| "open".to_string()),
            author: nested_string(value, "author", "name")
                .or_else(|| nested_string(value, "author", "username"))
                .unwrap_or_default(),
            source_branch: string_field(value, "source_branch").unwrap_or_else(|| "—".to_string()),
            target_branch: string_field(value, "target_branch").unwrap_or_else(|| "—".to_string()),
            review_status: gitlab_list_review_status(value),
            conversation_count: conversation_count_from_detail(value),
            created_at: string_field(value, "created_at").unwrap_or_default(),
            updated_at: string_field(value, "updated_at").unwrap_or_default(),
            web_url: string_field(value, "web_url").unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    let items = if per_page == 1 || !hydrate_visible {
        items
    } else {
        hydrate_gitlab_review_statuses(locator, repo_project_id, items).await
    };
    let total = pagination_total(&headers);
    Ok(PullRequestPage {
        total: total.unwrap_or_else(|| fallback_total(page, per_page, items.len())),
        total_exact: total.is_some(),
        page_count: header_u32(&headers, "x-total-pages"),
        items,
    })
}

async fn get_gitlab_closed_pull_requests(
    locator: &RepoLocator,
    repo_project_id: &str,
    filters: &PullRequestFilters,
    page: u32,
    per_page: u32,
) -> Result<PullRequestPage, String> {
    let per_page = per_page.max(1);
    let requested_page = page.max(1);
    let prefix_len = requested_page.saturating_mul(per_page);
    let closed_future = load_gitlab_state_prefix(
        locator,
        repo_project_id,
        PullRequestState::Closed,
        filters,
        prefix_len,
        per_page,
        requested_page == 1,
    );
    let merged_future = load_gitlab_state_prefix(
        locator,
        repo_project_id,
        PullRequestState::Merged,
        filters,
        prefix_len,
        per_page,
        requested_page == 1,
    );
    let (closed_result, merged_result) = tokio::join!(closed_future, merged_future);
    let (mut closed, closed_total, closed_exact) = closed_result?;
    let (merged, merged_total, merged_exact) = merged_result?;
    closed.extend(merged);
    sort_gitlab_items(&mut closed, filters.sort.as_str());

    let total = closed_total.saturating_add(merged_total);
    let start = requested_page.saturating_sub(1).saturating_mul(per_page) as usize;
    let mut items = closed
        .into_iter()
        .skip(start)
        .take(per_page as usize)
        .collect::<Vec<_>>();
    if per_page != 1 {
        items = hydrate_gitlab_review_statuses(locator, repo_project_id, items).await;
    }
    let total_exact = closed_exact && merged_exact;
    let page_count = if total_exact {
        total.div_ceil(per_page).max(1)
    } else if items.len() == per_page as usize {
        requested_page.saturating_add(1)
    } else {
        requested_page.max(1)
    };
    Ok(PullRequestPage {
        items,
        total,
        total_exact,
        page_count: Some(page_count),
    })
}

async fn load_gitlab_state_prefix(
    locator: &RepoLocator,
    repo_project_id: &str,
    state: PullRequestState,
    filters: &PullRequestFilters,
    prefix_len: u32,
    page_size: u32,
    force_refresh: bool,
) -> Result<(Vec<PullRequestItem>, u32, bool), String> {
    let page_size = page_size.clamp(1, 100);
    let key = prefix_cache_key(locator, repo_project_id, state, filters, page_size);
    let cached = {
        let mut cache = prefix_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if force_refresh {
            cache.remove(&key);
            None
        } else {
            cache.get(&key)
        }
    };
    if let Some(entry) = cached.as_ref()
        && (entry.items.len() >= prefix_len as usize || entry.exhausted)
    {
        return Ok((
            entry
                .items
                .iter()
                .take(prefix_len as usize)
                .cloned()
                .collect(),
            entry.total,
            entry.total_exact,
        ));
    }

    let mut entry = cached
        .as_deref()
        .cloned()
        .unwrap_or_else(|| PrefixCacheEntry {
            items: Vec::new(),
            total: 0,
            total_exact: false,
            exhausted: false,
        });
    let first_missing_page = (entry.items.len() as u32)
        .div_ceil(page_size)
        .saturating_add(1);
    let mut page = first_missing_page;
    while entry.items.len() < prefix_len as usize && !entry.exhausted {
        let result = get_gitlab_pull_request_server_page(
            locator,
            repo_project_id,
            Some(state),
            filters,
            page,
            page_size,
            false,
        )
        .await?;
        if result.total_exact || !entry.total_exact {
            entry.total = result.total;
            entry.total_exact = result.total_exact;
        }
        let batch_len = result.items.len();
        merge_gitlab_prefix_items(&mut entry.items, result.items);
        if batch_len < page_size as usize {
            entry.exhausted = true;
        }
        let Some(next_page) = page.checked_add(1) else {
            break;
        };
        page = next_page;
    }
    prefix_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(key, entry.clone());
    let items = entry.items.into_iter().take(prefix_len as usize).collect();
    Ok((items, entry.total, entry.total_exact))
}

fn merge_gitlab_prefix_items(existing: &mut Vec<PullRequestItem>, incoming: Vec<PullRequestItem>) {
    for item in incoming {
        if item.number != "—"
            && let Some(index) = existing
                .iter()
                .position(|candidate| candidate.number == item.number)
        {
            existing[index] = item;
        } else {
            existing.push(item);
        }
    }
}

fn sort_gitlab_items(items: &mut [PullRequestItem], sort: &str) {
    items.sort_by(|left, right| {
        let ordering = match sort {
            "oldest" => left.created_at.cmp(&right.created_at),
            "updated-desc" => right.updated_at.cmp(&left.updated_at),
            "updated-asc" => left.updated_at.cmp(&right.updated_at),
            _ => right.created_at.cmp(&left.created_at),
        };
        ordering.then_with(|| right.number.cmp(&left.number))
    });
}

async fn hydrate_gitlab_review_statuses(
    locator: &RepoLocator,
    repo_project_id: &str,
    mut items: Vec<PullRequestItem>,
) -> Vec<PullRequestItem> {
    let numbers = items
        .iter()
        .enumerate()
        .map(|(index, item)| (index, item.number.clone()))
        .collect::<Vec<_>>();
    // Keep approval hydration to a small number of in-flight requests; large
    // pages otherwise burst-hit the provider and trip abuse/rate limits.
    let approvals_by_index = stream::iter(numbers)
        .map(|(index, number)| async move {
            let endpoint = merge_request_endpoint(
                endpoints::MERGE_REQUEST_APPROVALS_URL,
                locator,
                repo_project_id,
                number.as_str(),
            );
            let approvals = http::send_json::<Value>(
                gitlab_request(endpoint),
                RequestOptions::new("Load GitLab merge request list approvals").retry_transient(3),
            )
            .await
            .ok();
            (index, approvals)
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;
    for (index, approvals) in approvals_by_index {
        if let Some(approvals) = approvals {
            items[index].review_status =
                gitlab_approval_review_status(&approvals, items[index].review_status);
        }
    }
    items
}

fn gitlab_list_review_status(value: &Value) -> PullRequestReviewStatus {
    match string_field(value, "detailed_merge_status")
        .or_else(|| string_field(value, "merge_status"))
        .unwrap_or_default()
        .as_str()
    {
        "not_approved" | "approvals_syncing" => PullRequestReviewStatus::ReviewRequired,
        _ => PullRequestReviewStatus::None,
    }
}

fn gitlab_approval_review_status(
    approvals: &Value,
    fallback: PullRequestReviewStatus,
) -> PullRequestReviewStatus {
    let approvals_left = value_u32(approvals, "approvals_left");
    let approvals_required = value_u32(approvals, "approvals_required");
    let approval_count = approvals
        .get("approved_by")
        .and_then(Value::as_array)
        .map(|approved_by| approved_by.len())
        .unwrap_or(0);
    if approvals_left.is_some_and(|left| left > 0)
        || (approvals_required.is_some_and(|required| required > 0) && approval_count == 0)
    {
        PullRequestReviewStatus::ReviewRequired
    } else if approval_count > 0 {
        PullRequestReviewStatus::Approved
    } else {
        fallback
    }
}

fn gitlab_list_sort(value: &str) -> (&'static str, &'static str) {
    match value {
        "oldest" => ("created_at", "asc"),
        "updated-desc" => ("updated_at", "desc"),
        "updated-asc" => ("updated_at", "asc"),
        _ => ("created_at", "desc"),
    }
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
        RequestOptions::new("Load GitLab merge request details").retry_transient(3),
    )
    .await
    .map_err(|error| error.to_string())?;

    let endpoint = endpoints::MERGE_REQUEST_DIFFS_URL
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace("{project_id}", project_id.as_str())
        .replace("{merge_request_iid}", merge_request_iid.as_str());
    let mut pages = PageCollector::new(100, 30);
    loop {
        let mut request = http::get(endpoint.as_str())
            .header(ACCEPT, "application/json")
            .query(&[
                ("page", pages.page().to_string()),
                ("per_page", "100".to_string()),
            ]);
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
            RequestOptions::new("Load GitLab merge request diffs").retry_transient(3),
        )
        .await
        .map_err(|error| error.to_string())?;
        if !pages.append(batch) {
            break;
        }
    }
    let (values, files_truncated) = pages.into_parts();

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
        author_identity: super::PullRequestAuthorIdentity::from_user(&detail["author"]),
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
        files_truncated,
        files,
    })
}

pub(super) async fn get_commit_range_diff(
    locator: &RepoLocator,
    repo_project_id: &str,
    base_sha: &str,
    head_sha: &str,
    commit_count: u32,
    _ignore_whitespace_change: bool,
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
        RequestOptions::new("Compare GitLab merge request commits").retry_transient(3),
    )
    .await
    .map_err(|error| error.to_string())?;
    let values = payload
        .get("diffs")
        .and_then(Value::as_array)
        .ok_or_else(|| "GitLab commit comparison response has no diff list.".to_string())?;
    let files = values.iter().map(decode_diff_file).collect::<Vec<_>>();
    Ok(PullRequestDiffDetail {
        author_identity: Default::default(),
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
        files_truncated: payload
            .get("overflow")
            .or_else(|| payload.get("compare_timeout"))
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
    // The merge-request detail and the project merge settings are independent
    // reads; fetch them concurrently while keeping each error path intact.
    let detail_future = http::send_json::<Value>(
        request,
        RequestOptions::new("Load GitLab merge request readiness").retry_transient(3),
    );
    let project_future = load_project_settings(locator, repo_project_id);
    let (detail, project) = tokio::join!(detail_future, project_future);
    let detail = detail.map_err(|error| error.to_string())?;
    let project = project?;
    Ok(gitlab_action_state(
        &detail,
        Some(&project),
        configured_token(RepoType::GitLab).is_some(),
    ))
}

pub(super) async fn submit_pull_request_review(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
    action: PullRequestReviewAction,
    body: &str,
) -> Result<(), String> {
    if configured_token(RepoType::GitLab).is_none() {
        return Err("Configure a GitLab Token in Settings before submitting a review.".into());
    }
    if action != PullRequestReviewAction::Approve && body.trim().is_empty() {
        return Err("A review comment is required for this action.".into());
    }
    if action == PullRequestReviewAction::RequestChanges {
        return Err(
            "GitLab does not expose Request changes through its stable REST API; add a comment instead."
                .into(),
        );
    }

    if action == PullRequestReviewAction::Comment {
        return post_merge_request_note(locator, repo_project_id, pull_request_iid, body).await;
    }

    // GitLab's approval endpoint has no review-summary field. Preserve a
    // non-empty summary as a normal MR note before recording the approval.
    let posted_summary = !body.trim().is_empty();
    if posted_summary {
        post_merge_request_note(locator, repo_project_id, pull_request_iid, body).await?;
    }

    let detail = load_merge_request_detail(locator, repo_project_id, pull_request_iid).await?;
    let sha = string_field(&detail, "sha")
        .ok_or_else(|| "GitLab did not return the merge request HEAD SHA.".to_string())?;
    let endpoint = merge_request_endpoint(
        endpoints::MERGE_REQUEST_APPROVE_URL,
        locator,
        repo_project_id,
        pull_request_iid,
    );
    let request = gitlab_authenticated(http::post(endpoint)).json(&serde_json::json!({
        "sha": sha,
    }));
    let result: Result<Value, String> =
        http::send_json(request, RequestOptions::new("Approve GitLab merge request"))
            .await
            .map_err(|error| error.to_string());
    result.map(|_| ()).map_err(|error| {
        if posted_summary {
            format!("GitLab posted the review summary, but approval failed: {error}")
        } else {
            error
        }
    })
}

pub(super) async fn dismiss_pull_request_review(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
    reason: &str,
) -> Result<(), String> {
    if configured_token(RepoType::GitLab).is_none() {
        return Err("Configure a GitLab Token in Settings before dismissing a review.".into());
    }
    if reason.trim().is_empty() {
        return Err("Enter a reason before dismissing the review.".into());
    }

    // GitLab unapproves only the authenticated user. Preserve GitHub's visible
    // dismissal reason as a normal merge-request note before changing state.
    post_merge_request_note(locator, repo_project_id, pull_request_iid, reason).await?;
    let endpoint = merge_request_endpoint(
        endpoints::MERGE_REQUEST_UNAPPROVE_URL,
        locator,
        repo_project_id,
        pull_request_iid,
    );
    let result: Result<Value, String> = http::send_json(
        gitlab_authenticated(http::post(endpoint)),
        RequestOptions::new("Unapprove GitLab merge request"),
    )
    .await
    .map_err(|error| error.to_string());
    result.map(|_| ()).map_err(|error| {
        format!("GitLab posted the dismissal reason, but unapprove failed: {error}")
    })
}

pub(super) async fn merge_pull_request(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
    method: PullRequestMergeMethod,
    remove_source_branch: bool,
) -> Result<(), String> {
    if configured_token(RepoType::GitLab).is_none() {
        return Err("Configure a GitLab Token in Settings before merging.".into());
    }
    if !matches!(
        method,
        PullRequestMergeMethod::Merge | PullRequestMergeMethod::Squash
    ) {
        return Err("GitLab supports merge and squash through the merge request API.".into());
    }

    // Re-read immediately before the write so the SHA guard prevents merging
    // commits that were not represented by the reviewed state.
    let detail = load_merge_request_detail(locator, repo_project_id, pull_request_iid).await?;
    let readiness = gitlab_merge_readiness(&detail);
    if readiness.kind != PullRequestReadinessKind::Ready {
        return Err(readiness.detail);
    }
    if detail
        .get("user")
        .and_then(|user| user.get("can_merge"))
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err("The configured GitLab user cannot merge this merge request.".into());
    }
    let sha = string_field(&detail, "sha")
        .ok_or_else(|| "GitLab did not return the merge request HEAD SHA.".to_string())?;
    let endpoint = merge_request_endpoint(
        endpoints::MERGE_REQUEST_MERGE_URL,
        locator,
        repo_project_id,
        pull_request_iid,
    );
    let request = gitlab_authenticated(http::put(endpoint)).json(&gitlab_merge_payload(
        method,
        remove_source_branch,
        sha.as_str(),
    ));
    let payload: Value =
        http::send_json(request, RequestOptions::new("Merge GitLab merge request"))
            .await
            .map_err(|error| error.to_string())?;
    if string_field(&payload, "state").as_deref() != Some("merged") {
        return Err(string_field(&payload, "merge_error")
            .filter(|message| !message.trim().is_empty())
            .unwrap_or_else(|| "GitLab did not merge this merge request.".to_string()));
    }
    invalidate_prefix_cache(locator, repo_project_id);
    Ok(())
}

pub(super) async fn revert_pull_request(
    _locator: &RepoLocator,
    _repo_project_id: &str,
    _pull_request_iid: &str,
) -> Result<PullRequestRevertResult, String> {
    Err("GitLab does not expose the server-side revert contract used by Usher.".to_string())
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
    let approvals_endpoint = merge_request_endpoint(
        endpoints::MERGE_REQUEST_APPROVALS_URL,
        locator,
        repo_project_id,
        pull_request_iid,
    );
    let current_user_endpoint =
        endpoints::CURRENT_USER_URL.replace("{api_base_url}", api_base_url(locator).as_str());
    let detail_future = async {
        http::send_json(
            gitlab_request(detail_endpoint),
            RequestOptions::new("Load GitLab merge request conversation header").retry_transient(3),
        )
        .await
        .map_err(|error| error.to_string())
    };
    let notes_future = async {
        http::send_json(
            gitlab_request(notes_endpoint).query(&[("per_page", "100")]),
            RequestOptions::new("Load GitLab merge request notes").retry_transient(3),
        )
        .await
        .map_err(|error| error.to_string())
    };
    let commits_future = async {
        http::send_json(
            gitlab_request(commits_endpoint).query(&[("per_page", "100")]),
            RequestOptions::new("Load GitLab merge request conversation commits")
                .retry_transient(3),
        )
        .await
        .map_err(|error| error.to_string())
    };
    let approvals_future = async {
        Ok::<Option<Value>, String>(
            http::send_json(
                gitlab_request(approvals_endpoint),
                RequestOptions::new("Load GitLab merge request approvals").retry_transient(3),
            )
            .await
            .ok(),
        )
    };
    let current_user_future = async {
        Ok::<Option<Value>, String>(
            http::send_json(
                gitlab_request(current_user_endpoint),
                RequestOptions::new("Load GitLab current review user").retry_transient(3),
            )
            .await
            .ok(),
        )
    };
    let (detail, notes, commit_values, approvals, current_user): (
        Value,
        Vec<Value>,
        Vec<Value>,
        Option<Value>,
        Option<Value>,
    ) = tokio::try_join!(
        detail_future,
        notes_future,
        commits_future,
        approvals_future,
        current_user_future
    )?;

    let description = PullRequestConversationItem {
        id: format!("description-{pull_request_iid}"),
        kind: "description".to_string(),
        author: nested_string(&detail, "author", "name")
            .or_else(|| nested_string(&detail, "author", "username"))
            .unwrap_or_default(),
        avatar_url: nested_string(&detail, "author", "avatar_url").unwrap_or_default(),
        author_association: String::new(),
        body: string_field(&detail, "description").unwrap_or_default(),
        created_at: string_field(&detail, "created_at").unwrap_or_default(),
        state: string_field(&detail, "state").unwrap_or_default(),
        review_id: String::new(),
        can_dismiss: false,
    };

    let approval_summaries = approvals
        .as_ref()
        .map(|approvals| gitlab_approval_summaries(approvals, current_user.as_ref(), &notes))
        .unwrap_or_default();
    let has_approval_snapshot = approvals.is_some();
    let mut activity = notes
        .iter()
        .filter_map(|value| {
            let body = string_field(value, "body").unwrap_or_default();
            let is_system = value
                .get("system")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if is_system
                && ((has_approval_snapshot && is_gitlab_review_system_note(body.as_str()))
                    || is_redundant_commit_system_note(body.as_str())
                    || is_redundant_merge_system_note(body.as_str()))
            {
                return None;
            }
            Some(PullRequestConversationItem {
                id: string_field(value, "id").unwrap_or_default(),
                kind: if is_system {
                    "event".to_string()
                } else {
                    "comment".to_string()
                },
                author: nested_string(value, "author", "name")
                    .or_else(|| nested_string(value, "author", "username"))
                    .unwrap_or_default(),
                avatar_url: nested_string(value, "author", "avatar_url").unwrap_or_default(),
                author_association: String::new(),
                body,
                created_at: string_field(value, "created_at").unwrap_or_default(),
                state: String::new(),
                review_id: String::new(),
                can_dismiss: false,
            })
        })
        .collect::<Vec<_>>();
    activity.extend(approval_summaries);

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
                author: string_field(&value, "author_name").unwrap_or_default(),
                avatar_url: string_field(&value, "author_avatar_url")
                    .or_else(|| nested_string(&value, "author", "avatar_url"))
                    .unwrap_or_default(),
                authored_at: string_field(&value, "authored_date")
                    .or_else(|| string_field(&value, "created_at"))
                    .unwrap_or_default(),
                committer: string_field(&value, "committer_name").unwrap_or_default(),
                committer_avatar_url: string_field(&value, "committer_avatar_url")
                    .or_else(|| nested_string(&value, "committer", "avatar_url"))
                    .unwrap_or_default(),
                committed_at: string_field(&value, "committed_date")
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
    activity.extend(gitlab_commit_batch_activity(&notes, &commits));

    if let Some(merged_at) =
        string_field(&detail, "merged_at").filter(|value| !value.trim().is_empty())
    {
        let merge_sha = string_field(&detail, "merge_commit_sha").unwrap_or_default();
        let target_branch = string_field(&detail, "target_branch").unwrap_or_default();
        let author = gitlab_merge_actor(&detail);
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

fn gitlab_commit_batch_activity(
    notes: &[Value],
    commits: &[PullRequestCommit],
) -> Vec<PullRequestConversationItem> {
    let mut commit_notes = notes
        .iter()
        .filter_map(|note| {
            let is_system = note.get("system").and_then(Value::as_bool).unwrap_or(false);
            let body = string_field(note, "body").unwrap_or_default();
            let count = is_system.then(|| commit_count_from_system_note(body.as_str()))??;
            Some((note, body, count))
        })
        .collect::<Vec<_>>();
    commit_notes.sort_by_key(|(note, _, _)| string_field(note, "created_at").unwrap_or_default());

    if commit_notes.is_empty() {
        return conversation_commit_batch_items(commits);
    }

    let mut assigned = vec![false; commits.len()];
    let mut activity = Vec::new();
    for (note, body, count) in commit_notes {
        let references = commit_sha_references(body.as_str());
        let mut batch_indices = commits
            .iter()
            .enumerate()
            .filter(|(index, commit)| {
                !assigned[*index]
                    && references.iter().any(|reference| {
                        commit.sha.starts_with(reference) || reference.starts_with(&commit.sha)
                    })
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();

        // Older/self-managed GitLab versions can return only `added N commits`
        // without the rendered SHA list. Preserve the native batch boundary by
        // consuming the next unassigned commits in chronological order.
        if batch_indices.is_empty() {
            batch_indices.extend(
                assigned
                    .iter()
                    .enumerate()
                    .filter_map(|(index, assigned)| (!*assigned).then_some(index))
                    .take(count),
            );
        }
        for index in &batch_indices {
            assigned[*index] = true;
        }
        let batch = batch_indices
            .into_iter()
            .filter_map(|index| commits.get(index).cloned())
            .collect::<Vec<_>>();
        let actor = nested_string(note, "author", "name")
            .or_else(|| nested_string(note, "author", "username"))
            .unwrap_or_default();
        let avatar_url = nested_string(note, "author", "avatar_url").unwrap_or_default();
        let created_at = string_field(note, "created_at").unwrap_or_default();
        let event_id = string_field(note, "id").unwrap_or_else(|| created_at.clone());
        activity.extend(conversation_commit_batch_items_for_actor(
            &batch,
            actor.as_str(),
            avatar_url.as_str(),
            created_at.as_str(),
            count,
            Some(event_id.as_str()),
        ));
    }

    let remaining = commits
        .iter()
        .zip(assigned)
        .filter_map(|(commit, assigned)| (!assigned).then_some(commit.clone()))
        .collect::<Vec<_>>();
    activity.extend(conversation_commit_batch_items(&remaining));
    activity
}

fn commit_count_from_system_note(body: &str) -> Option<usize> {
    let normalized = conversation_body_to_plain_text(body);
    let lower = normalized.trim().to_ascii_lowercase();
    if !lower.starts_with("added ") || !lower.contains("commit") {
        return None;
    }
    lower.split_whitespace().find_map(|token| {
        token
            .trim_matches(|character: char| !character.is_ascii_digit())
            .parse()
            .ok()
    })
}

fn commit_sha_references(body: &str) -> Vec<String> {
    let mut references = Vec::new();
    let mut candidate = String::new();
    let flush = |candidate: &mut String, references: &mut Vec<String>| {
        if (7..=40).contains(&candidate.len())
            && candidate
                .bytes()
                .any(|byte| matches!(byte.to_ascii_lowercase(), b'a'..=b'f'))
        {
            references.push(candidate.to_ascii_lowercase());
        }
        candidate.clear();
    };
    for character in body.chars() {
        if character.is_ascii_hexdigit() {
            candidate.push(character.to_ascii_lowercase());
        } else {
            flush(&mut candidate, &mut references);
        }
    }
    flush(&mut candidate, &mut references);
    references.sort();
    references.dedup();
    references
}

fn gitlab_merge_actor(detail: &Value) -> String {
    let Some(merged_by) = detail.get("merged_by") else {
        return String::new();
    };
    ["display_name", "displayName", "name"]
        .into_iter()
        .filter_map(|key| string_field(merged_by, key))
        .find(|label| !label.trim().is_empty() && !looks_like_email(label))
        .or_else(|| {
            ["username", "user_name", "login"]
                .into_iter()
                .find_map(|key| string_field(merged_by, key))
                .filter(|label| !label.trim().is_empty())
        })
        .or_else(|| {
            ["name", "email"]
                .into_iter()
                .filter_map(|key| string_field(merged_by, key))
                .find_map(|email| {
                    email
                        .split_once('@')
                        .map(|(account, _)| account.to_string())
                })
        })
        .unwrap_or_default()
}

fn looks_like_email(value: &str) -> bool {
    let Some((local, domain)) = value.trim().split_once('@') else {
        return false;
    };
    !local.is_empty() && domain.contains('.') && !domain.chars().any(char::is_whitespace)
}

fn gitlab_approval_summaries(
    approvals: &Value,
    current_user: Option<&Value>,
    notes: &[Value],
) -> Vec<PullRequestConversationItem> {
    let current_id = current_user.and_then(|user| string_field(user, "id"));
    let current_username = current_user.and_then(|user| string_field(user, "username"));
    approvals
        .get("approved_by")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|entry| entry.get("user").unwrap_or(entry))
        .map(|user| {
            let id = string_field(user, "id").unwrap_or_default();
            let username = string_field(user, "username").unwrap_or_default();
            let author = string_field(user, "name")
                .or_else(|| string_field(user, "username"))
                .unwrap_or_default();
            let created_at = notes
                .iter()
                .filter(|note| {
                    note.get("system").and_then(Value::as_bool) == Some(true)
                        && string_field(note, "body")
                            .is_some_and(|body| is_gitlab_approval_note(body.as_str()))
                        && note.get("author").is_some_and(|note_author| {
                            (!id.is_empty()
                                && string_field(note_author, "id").as_deref() == Some(id.as_str()))
                                || (!username.is_empty()
                                    && string_field(note_author, "username").as_deref()
                                        == Some(username.as_str()))
                        })
                })
                .filter_map(|note| string_field(note, "created_at"))
                .max()
                .unwrap_or_default();
            let can_dismiss = current_id
                .as_ref()
                .zip((!id.is_empty()).then_some(&id))
                .is_some_and(|(current, reviewer)| current == reviewer)
                || current_username
                    .as_ref()
                    .zip((!username.is_empty()).then_some(&username))
                    .is_some_and(|(current, reviewer)| current == reviewer);
            PullRequestConversationItem {
                id: format!("review-summary-{id}"),
                kind: "review_summary".to_string(),
                author,
                avatar_url: string_field(user, "avatar_url").unwrap_or_default(),
                author_association: String::new(),
                body: "approved these changes".to_string(),
                created_at,
                state: "approved".to_string(),
                review_id: id,
                can_dismiss,
            }
        })
        .collect()
}

fn is_gitlab_approval_note(body: &str) -> bool {
    body.trim()
        .to_ascii_lowercase()
        .contains("approved this merge request")
}

fn is_gitlab_review_system_note(body: &str) -> bool {
    let body = body.trim().to_ascii_lowercase();
    body.contains("approved this merge request") || body.contains("unapproved this merge request")
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
        RequestOptions::new("Load GitLab merge request commits").retry_transient(3),
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
                author: string_field(&value, "author_name").unwrap_or_default(),
                avatar_url: string_field(&value, "author_avatar_url")
                    .or_else(|| nested_string(&value, "author", "avatar_url"))
                    .unwrap_or_default(),
                authored_at: string_field(&value, "authored_date")
                    .or_else(|| string_field(&value, "created_at"))
                    .unwrap_or_default(),
                committer: string_field(&value, "committer_name").unwrap_or_default(),
                committer_avatar_url: string_field(&value, "committer_avatar_url")
                    .or_else(|| nested_string(&value, "committer", "avatar_url"))
                    .unwrap_or_default(),
                committed_at: string_field(&value, "committed_date")
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
        RequestOptions::new("Count GitLab merge request commits").retry_transient(3),
    )
    .await
    .ok()?;
    let headers = response.headers().clone();
    pagination_total(&headers)
}

async fn load_merge_request_detail(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
) -> Result<Value, String> {
    let endpoint = merge_request_endpoint(
        endpoints::MERGE_REQUEST_URL,
        locator,
        repo_project_id,
        pull_request_iid,
    );
    http::send_json(
        gitlab_request(endpoint).query(&[("with_merge_status_recheck", "true")]),
        RequestOptions::new("Reload GitLab merge request before remote action"),
    )
    .await
    .map_err(|error| error.to_string())
}

async fn load_project_settings(
    locator: &RepoLocator,
    repo_project_id: &str,
) -> Result<Value, String> {
    let endpoint = endpoints::PROJECT_URL
        .replace("{api_base_url}", api_base_url(locator).as_str())
        .replace(
            "{project_id_or_path}",
            percent_encode_path(repo_project_id.trim()).as_str(),
        );
    http::send_json(
        gitlab_request(endpoint),
        RequestOptions::new("Load GitLab project merge settings").retry_transient(3),
    )
    .await
    .map_err(|error| error.to_string())
}

async fn post_merge_request_note(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
    body: &str,
) -> Result<(), String> {
    let body = body.trim();
    if body.is_empty() {
        return Err("A review comment is required for this action.".into());
    }
    let endpoint = merge_request_endpoint(
        endpoints::MERGE_REQUEST_NOTES_URL,
        locator,
        repo_project_id,
        pull_request_iid,
    );
    let request =
        gitlab_authenticated(http::post(endpoint)).json(&serde_json::json!({ "body": body }));
    let _: Value = http::send_json(
        request,
        RequestOptions::new("Submit GitLab merge request comment"),
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
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
    gitlab_authenticated(http::get(endpoint))
}

fn gitlab_authenticated(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    let mut request = request.header(ACCEPT, "application/json");
    if let Some(token) = configured_token(RepoType::GitLab) {
        request = request.header("PRIVATE-TOKEN", token);
    }
    request
}

fn gitlab_action_state(
    detail: &Value,
    project: Option<&Value>,
    has_token: bool,
) -> PullRequestActionState {
    let readiness = gitlab_merge_readiness(detail);
    let review_state = match string_field(detail, "detailed_merge_status").as_deref() {
        Some("requested_changes") => "rejected",
        Some("not_approved") | Some("approvals_syncing") => "pending",
        _ => "",
    }
    .to_string();
    let terminal = matches!(
        readiness.kind,
        PullRequestReadinessKind::Merged | PullRequestReadinessKind::Closed
    );
    let user_can_merge = detail
        .get("user")
        .and_then(|user| user.get("can_merge"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let can_merge =
        has_token && user_can_merge && readiness.kind == PullRequestReadinessKind::Ready;
    let merge_block_kind = if !has_token {
        PullRequestMergeBlockKind::ConfigureProvider
    } else if readiness.kind != PullRequestReadinessKind::Ready {
        PullRequestMergeBlockKind::Readiness
    } else if !user_can_merge {
        PullRequestMergeBlockKind::Permission
    } else {
        PullRequestMergeBlockKind::None
    };

    PullRequestActionState {
        // GitLab notes remain available on terminal merge requests when the
        // authenticated user is allowed to participate in the discussion.
        can_comment: has_token,
        can_approve: has_token && !terminal,
        // GitLab exposes this review decision in its UI, but not through the
        // stable REST API used by the provider adapter.
        can_request_changes: false,
        can_merge,
        can_revert: false,
        review_state,
        readiness,
        merge_block_kind,
        revert_strategy: PullRequestRevertStrategy::Unsupported,
        allowed_merge_methods: gitlab_merge_methods(project),
    }
}

fn gitlab_merge_methods(project: Option<&Value>) -> Vec<PullRequestMergeMethod> {
    match project
        .and_then(|value| value.get("squash_option"))
        .and_then(Value::as_str)
    {
        Some("never") => vec![PullRequestMergeMethod::Merge],
        Some("always") => vec![PullRequestMergeMethod::Squash],
        _ => vec![
            PullRequestMergeMethod::Merge,
            PullRequestMergeMethod::Squash,
        ],
    }
}

fn gitlab_merge_payload(
    method: PullRequestMergeMethod,
    remove_source_branch: bool,
    sha: &str,
) -> Value {
    serde_json::json!({
        "sha": sha,
        "squash": method == PullRequestMergeMethod::Squash,
        "should_remove_source_branch": remove_source_branch,
    })
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
        crate::git::DiffChangeType::New
    } else if value
        .get("deleted_file")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        crate::git::DiffChangeType::Deleted
    } else if value
        .get("renamed_file")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        crate::git::DiffChangeType::Renamed
    } else {
        crate::git::DiffChangeType::Modified
    };
    let lines = string_field(value, "diff")
        .map(|diff| parse_unified_diff(diff.as_str()))
        .unwrap_or_default();
    let added_lines = lines
        .iter()
        .filter(|line| line.kind == crate::git::DiffLineKind::Addition)
        .count() as u32;
    let removed_lines = lines
        .iter()
        .filter(|line| line.kind == crate::git::DiffLineKind::Deletion)
        .count() as u32;
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
    let response = http::send(
        request,
        RequestOptions::new("Load GitLab repository file")
            .retry_transient(3)
            .response_byte_limit(super::source::MAX_SOURCE_BYTES),
    )
    .await
    .map_err(|error| error.to_string())?;
    response
        .text()
        .await
        .map_err(|error| format!("GitLab repository file decode failed: {error}"))
}

fn api_base_url(locator: &RepoLocator) -> String {
    if is_configured_enterprise_host(RepoType::GitLab, locator.host.as_str())
        && let Some(api_base_url) = configured_api_base_url(RepoType::GitLab)
    {
        return with_default_api_version(api_base_url, endpoints::API_VERSION_PATH);
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use serde_json::json;

    use super::{
        PREFIX_CACHE_CAPACITY, PREFIX_CACHE_TTL, PrefixCacheEntry, PrefixCacheKey,
        commit_sha_references, gitlab_action_state, gitlab_approval_review_status,
        gitlab_approval_summaries, gitlab_commit_batch_activity, gitlab_list_review_status,
        gitlab_list_sort, gitlab_merge_actor, gitlab_merge_methods, gitlab_merge_payload,
        gitlab_merge_readiness, is_gitlab_review_system_note, merge_gitlab_prefix_items,
        sort_gitlab_items,
    };
    use crate::git::prefix_cache::PrefixCache;

    #[test]
    fn commit_sha_references_reject_pure_numeric_ids_and_timestamps() {
        assert_eq!(
            commit_sha_references("added abc1234 and 0123456789 at 20260830010101"),
            vec!["abc1234"]
        );
    }
    use crate::git::{
        PullRequestCommit, PullRequestItem, PullRequestMergeBlockKind, PullRequestMergeMethod,
        PullRequestReadinessKind, PullRequestReviewStatus,
    };

    fn commit(sha: &str, author: &str, authored_at: &str) -> PullRequestCommit {
        PullRequestCommit {
            sha: sha.to_string(),
            title: sha.to_string(),
            message: sha.to_string(),
            author: author.to_string(),
            avatar_url: String::new(),
            authored_at: authored_at.to_string(),
            committer: author.to_string(),
            committer_avatar_url: String::new(),
            committed_at: authored_at.to_string(),
            verified: false,
        }
    }

    fn list_item(number: &str, created_at: &str, updated_at: &str) -> PullRequestItem {
        PullRequestItem {
            number: number.to_string(),
            title: String::new(),
            state: "closed".to_string(),
            author: String::new(),
            source_branch: String::new(),
            target_branch: String::new(),
            review_status: PullRequestReviewStatus::None,
            conversation_count: None,
            created_at: created_at.to_string(),
            updated_at: updated_at.to_string(),
            web_url: String::new(),
        }
    }

    fn prefix_key(project_id: &str) -> PrefixCacheKey {
        PrefixCacheKey {
            api_base_url: "https://gitlab.example/api/v4".to_string(),
            project_id: project_id.to_string(),
            state: "closed".to_string(),
            query: String::new(),
            reviews: "all".to_string(),
            assignee: "any".to_string(),
            sort: "newest".to_string(),
            page_size: 20,
        }
    }

    fn prefix_entry() -> PrefixCacheEntry {
        PrefixCacheEntry {
            items: Vec::new(),
            total: 0,
            total_exact: true,
            exhausted: false,
        }
    }

    #[test]
    fn gitlab_prefix_cache_is_bounded_and_refreshes_existing_keys() {
        let now = Instant::now();
        let mut cache = PrefixCache::new(PREFIX_CACHE_CAPACITY, PREFIX_CACHE_TTL);
        for index in 0..=PREFIX_CACHE_CAPACITY {
            cache.insert_at(
                prefix_key(format!("project-{index}").as_str()),
                prefix_entry(),
                now,
            );
        }
        assert_eq!(cache.len(), PREFIX_CACHE_CAPACITY);
        assert!(!cache.contains_key(&prefix_key("project-0")));

        let newest = prefix_key(format!("project-{PREFIX_CACHE_CAPACITY}").as_str());
        cache.insert_at(newest.clone(), prefix_entry(), now);
        assert_eq!(cache.len(), PREFIX_CACHE_CAPACITY);
        assert_eq!(cache.newest_key(), Some(&newest));
    }

    #[test]
    fn gitlab_prefix_merge_replaces_duplicate_iids_without_reordering() {
        let mut items = vec![
            list_item("3", "2026-01-03", "old"),
            list_item("2", "2026-01-02", "old"),
        ];
        merge_gitlab_prefix_items(
            &mut items,
            vec![
                list_item("2", "2026-01-02", "new"),
                list_item("1", "2026-01-01", "new"),
            ],
        );
        assert_eq!(
            items
                .iter()
                .map(|item| item.number.as_str())
                .collect::<Vec<_>>(),
            ["3", "2", "1"]
        );
        assert_eq!(items[1].updated_at, "new");
    }

    #[test]
    fn deduplicated_prefix_stays_incomplete_until_unique_target_is_reached() {
        let mut items = vec![
            list_item("4", "2026-01-04", "old"),
            list_item("3", "2026-01-03", "old"),
        ];
        merge_gitlab_prefix_items(
            &mut items,
            vec![
                list_item("3", "2026-01-03", "new"),
                list_item("2", "2026-01-02", "new"),
            ],
        );

        assert_eq!(items.len(), 3);
        assert!(items.len() < 4);
    }

    #[test]
    fn gitlab_prefix_cache_expires_and_invalidates_one_project() {
        let now = Instant::now();
        let mut cache = PrefixCache::new(PREFIX_CACHE_CAPACITY, PREFIX_CACHE_TTL);
        let expired = prefix_key("expired");
        cache.insert_at(
            expired.clone(),
            prefix_entry(),
            now - PREFIX_CACHE_TTL - Duration::from_secs(1),
        );
        assert!(cache.get_at(&expired, now).is_none());

        let stable_timestamp = now - PREFIX_CACHE_TTL + Duration::from_secs(1);
        let stable = prefix_key("stable");
        cache.insert_at(stable.clone(), prefix_entry(), stable_timestamp);
        assert!(cache.get_at(&stable, now).is_some());
        assert!(
            cache
                .get_at(&stable, now + Duration::from_secs(2))
                .is_none()
        );

        cache.insert_at(prefix_key("one"), prefix_entry(), now);
        cache.insert_at(prefix_key("two"), prefix_entry(), now);
        cache.invalidate_where(|key| key.project_id == "one");
        assert!(!cache.contains_key(&prefix_key("one")));
        assert!(cache.contains_key(&prefix_key("two")));
    }

    #[test]
    fn builds_gitlab_commit_batches_from_system_note_actors() {
        let commits = vec![
            commit(
                "aaaaaaa111111111",
                "Commit Author A",
                "2026-01-01T01:00:00Z",
            ),
            commit(
                "bbbbbbb222222222",
                "Commit Author B",
                "2026-01-02T01:00:00Z",
            ),
        ];
        let notes = vec![
            json!({
                "id": 2,
                "system": true,
                "body": "added 1 commit\n<ul><li>bbbbbbb2 - second</li></ul>",
                "created_at": "2026-01-02T02:00:00Z",
                "author": { "name": "Branch Updater B", "avatar_url": "b.png" }
            }),
            json!({
                "id": 1,
                "system": true,
                "body": "added 1 commit\n<ul><li>aaaaaaa1 - first</li></ul>",
                "created_at": "2026-01-01T02:00:00Z",
                "author": { "name": "Branch Updater A", "avatar_url": "a.png" }
            }),
        ];

        let activity = gitlab_commit_batch_activity(&notes, &commits);

        assert_eq!(activity.len(), 4);
        assert_eq!(activity[0].kind, "commits");
        assert_eq!(activity[0].author, "Branch Updater A");
        assert_eq!(activity[1].state, "aaaaaaa");
        assert_eq!(activity[2].author, "Branch Updater B");
        assert_eq!(activity[3].state, "bbbbbbb");
    }

    #[test]
    fn maps_gitlab_list_approval_states_to_github_statuses() {
        assert_eq!(
            gitlab_list_review_status(&json!({ "detailed_merge_status": "not_approved" })),
            PullRequestReviewStatus::ReviewRequired
        );
        assert_eq!(
            gitlab_approval_review_status(
                &json!({
                    "approvals_required": 2,
                    "approvals_left": 1,
                    "approved_by": [{ "user": { "id": 1 } }]
                }),
                PullRequestReviewStatus::None,
            ),
            PullRequestReviewStatus::ReviewRequired
        );
        assert_eq!(
            gitlab_approval_review_status(
                &json!({
                    "approvals_required": 1,
                    "approvals_left": 0,
                    "approved_by": [{ "user": { "id": 1 } }]
                }),
                PullRequestReviewStatus::None,
            ),
            PullRequestReviewStatus::Approved
        );
    }

    #[test]
    fn gitlab_merge_actor_prefers_display_name_and_never_uses_pr_author() {
        assert_eq!(
            gitlab_merge_actor(&json!({
                "merged_by": {
                    "display_name": "Merge Owner",
                    "name": "merge-owner@example.com",
                    "username": "merge-owner"
                },
                "author": { "name": "PR Author" }
            })),
            "Merge Owner"
        );
        assert_eq!(
            gitlab_merge_actor(&json!({ "author": { "name": "PR Author" } })),
            ""
        );
    }

    #[test]
    fn merges_gitlab_closed_and_merged_prefixes_in_provider_sort_order() {
        let mut items = vec![
            list_item("1", "2026-08-01T00:00:00Z", "2026-08-04T00:00:00Z"),
            list_item("2", "2026-08-03T00:00:00Z", "2026-08-02T00:00:00Z"),
            list_item("3", "2026-08-02T00:00:00Z", "2026-08-05T00:00:00Z"),
        ];

        sort_gitlab_items(&mut items, "newest");
        assert_eq!(
            items
                .iter()
                .map(|item| item.number.as_str())
                .collect::<Vec<_>>(),
            vec!["2", "3", "1"]
        );
        sort_gitlab_items(&mut items, "updated-desc");
        assert_eq!(
            items
                .iter()
                .map(|item| item.number.as_str())
                .collect::<Vec<_>>(),
            vec!["3", "1", "2"]
        );
    }

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

    #[test]
    fn maps_only_the_current_users_gitlab_approval_to_a_dismissible_review() {
        let notes = vec![json!({
            "system": true,
            "body": "approved this merge request",
            "created_at": "2026-08-10T08:00:00Z",
            "author": { "id": 7, "username": "alice" }
        })];
        let summaries = gitlab_approval_summaries(
            &json!({
                "approved_by": [
                    { "user": { "id": 7, "username": "alice", "name": "Alice" } },
                    { "user": { "id": 8, "username": "bob", "name": "Bob" } }
                ]
            }),
            Some(&json!({ "id": 7, "username": "alice" })),
            &notes,
        );

        assert_eq!(summaries.len(), 2);
        assert!(summaries[0].can_dismiss);
        assert!(!summaries[1].can_dismiss);
        assert_eq!(summaries[0].created_at, "2026-08-10T08:00:00Z");
        assert!(is_gitlab_review_system_note(
            "unapproved this merge request"
        ));
    }

    #[test]
    fn enables_gitlab_comment_approve_and_merge_for_authorized_ready_user() {
        let state = gitlab_action_state(
            &json!({
                "state": "opened",
                "detailed_merge_status": "mergeable",
                "user": { "can_merge": true }
            }),
            Some(&json!({ "squash_option": "default_on" })),
            true,
        );

        assert!(state.can_comment);
        assert!(state.can_approve);
        assert!(!state.can_request_changes);
        assert!(state.can_merge);
        assert_eq!(
            state.allowed_merge_methods,
            vec![
                PullRequestMergeMethod::Merge,
                PullRequestMergeMethod::Squash
            ]
        );
    }

    #[test]
    fn keeps_gitlab_remote_actions_disabled_without_token() {
        let state = gitlab_action_state(
            &json!({
                "state": "opened",
                "detailed_merge_status": "mergeable",
                "user": { "can_merge": true }
            }),
            None,
            false,
        );

        assert!(!state.can_comment);
        assert!(!state.can_approve);
        assert!(!state.can_merge);
        assert_eq!(
            state.merge_block_kind,
            PullRequestMergeBlockKind::ConfigureProvider
        );
    }

    #[test]
    fn respects_gitlab_project_squash_policy() {
        assert_eq!(
            gitlab_merge_methods(Some(&json!({ "squash_option": "never" }))),
            vec![PullRequestMergeMethod::Merge]
        );
        assert_eq!(
            gitlab_merge_methods(Some(&json!({ "squash_option": "always" }))),
            vec![PullRequestMergeMethod::Squash]
        );
    }

    #[test]
    fn builds_gitlab_merge_payload_with_sha_guard_and_user_choices() {
        assert_eq!(
            gitlab_merge_payload(PullRequestMergeMethod::Squash, true, "abc123"),
            json!({
                "sha": "abc123",
                "squash": true,
                "should_remove_source_branch": true
            })
        );
    }

    #[test]
    fn maps_every_pr_list_sort_to_gitlab_parameters() {
        assert_eq!(gitlab_list_sort("newest"), ("created_at", "desc"));
        assert_eq!(gitlab_list_sort("oldest"), ("created_at", "asc"));
        assert_eq!(gitlab_list_sort("updated-desc"), ("updated_at", "desc"));
        assert_eq!(gitlab_list_sort("updated-asc"), ("updated_at", "asc"));
    }
}
