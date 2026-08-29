//! Gitee-compatible repository operations.
//!
//! `project_name` is always the repository name extracted from `remote_repo`
//! (the final path segment before `.git`), never the editable project alias.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

use futures_util::{StreamExt, stream};
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap};
use serde_json::Value;

use crate::codereview::api::gitee as endpoints;
use crate::http::{self, RequestOptions};

use super::{
    GiteeApiProfile, PageCollector, PullRequestActionState, PullRequestCommit,
    PullRequestConversationItem, PullRequestDiffDetail, PullRequestDiffFile, PullRequestFilters,
    PullRequestItem, PullRequestMergeMethod, PullRequestPage, PullRequestReadiness,
    PullRequestReadinessKind, PullRequestRevertResult, PullRequestReviewAction,
    PullRequestReviewStatus, PullRequestState, RepoLocator, RepoType, commit_count_from_detail,
    configured_api_base_url, configured_enterprise, configured_gitee_token,
    conversation_commit_batch_items, conversation_count_from_detail, conversation_merged_item,
    finalize_conversation_timeline, gitee_api_profile, is_redundant_commit_system_note,
    is_redundant_merge_system_note, percent_encode_path,
};

mod conversation;
mod decode;
mod readiness;
mod revert;
mod transport;

use conversation::*;
use decode::*;
use readiness::{gitee_enterprise_merge_readiness, gitee_public_merge_readiness};
use revert::{enterprise_commit_refs_can_revert, enterprise_data_object};
use transport::{
    enterprise_authenticated_builder, enterprise_project_endpoint,
    enterprise_pull_request_endpoint, enterprise_pull_request_get,
    enterprise_repository_commit_endpoint, enterprise_web_pull_request_review_endpoint,
    parse_gitee_merge_method, public_authenticated_builder, public_authenticated_request,
    public_repo_endpoint,
};

const IDENTITY_AVATAR_CACHE_CAPACITY: usize = 256;

#[derive(Default)]
struct IdentityAvatarCache {
    entries: HashMap<String, Option<String>>,
    order: VecDeque<String>,
}

impl IdentityAvatarCache {
    fn get(&mut self, key: &str) -> Option<Option<String>> {
        let value = self.entries.get(key)?.clone();
        self.order.retain(|candidate| candidate != key);
        self.order.push_back(key.to_string());
        Some(value)
    }

    fn insert(&mut self, key: String, value: Option<String>) {
        self.entries.insert(key.clone(), value);
        self.order.retain(|candidate| candidate != &key);
        self.order.push_back(key);
        while self.entries.len() > IDENTITY_AVATAR_CACHE_CAPACITY {
            if let Some(evicted) = self.order.pop_front() {
                self.entries.remove(evicted.as_str());
            }
        }
    }
}

fn identity_avatar_cache() -> &'static Mutex<IdentityAvatarCache> {
    static CACHE: OnceLock<Mutex<IdentityAvatarCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(IdentityAvatarCache::default()))
}

/// Fetch the stable provider project id for a remote repository.
pub(super) async fn get_repo_detail(locator: &RepoLocator) -> Result<String, String> {
    if gitee_api_profile(locator) == GiteeApiProfile::EnterpriseV8 {
        return get_enterprise_repo_detail(locator).await;
    }
    get_public_gitee_repo_detail(locator).await
}

async fn get_enterprise_repo_detail(locator: &RepoLocator) -> Result<String, String> {
    let api_base_url = enterprise_api_base_url()?;
    let endpoint = endpoints::ENTERPRISE_PROJECTS_URL.replace("{api_base_url}", &api_base_url);
    let mut request = http::get(endpoint)
        .header("OAUTH", "enabled")
        .header(ACCEPT, "application/json")
        // The search term comes from remote_repo's final path component.
        .query(&[("search", locator.repo.as_str())]);
    if let Some(enterprise) = configured_enterprise(RepoType::Gitee) {
        request = request.header("enterprise", enterprise);
    }
    if let Some(token) = configured_gitee_token(GiteeApiProfile::EnterpriseV8) {
        request = request.header("PRIVATE-TOKEN", token);
    }

    let payload: Value = http::send_json(
        request,
        RequestOptions::new("Load Gitee project details").retry_transient(3),
    )
    .await
    .map_err(|error| error.to_string())?;
    select_enterprise_project_id(&payload, locator)
}

async fn get_public_gitee_repo_detail(locator: &RepoLocator) -> Result<String, String> {
    let endpoint = endpoints::PUBLIC_REPOSITORY_URL
        .replace("{scheme}", locator.scheme.as_str())
        .replace("{authority}", locator.authority.as_str())
        .replace(
            "{namespace}",
            percent_encode_path(locator.namespace.as_str()).as_str(),
        )
        .replace(
            "{repository}",
            percent_encode_path(locator.repo.as_str()).as_str(),
        );
    let mut request = http::get(endpoint).header(ACCEPT, "application/json");
    if let Some(token) = configured_gitee_token(GiteeApiProfile::PublicV5) {
        request = request.header("Authorization", format!("token {token}"));
    }

    let payload: Value = http::send_json(
        request,
        RequestOptions::new("Load Gitee project details").retry_transient(3),
    )
    .await
    .map_err(|error| error.to_string())?;
    value_to_id(payload.get("id"), "Gitee")
}

fn select_enterprise_project_id(payload: &Value, locator: &RepoLocator) -> Result<String, String> {
    let projects = payload
        .as_array()
        .or_else(|| payload.get("data").and_then(Value::as_array))
        .ok_or_else(|| "Gitee project search returned an unexpected response.".to_string())?;

    let expected_path = format!("{}/{}", locator.namespace, locator.repo);
    let project = projects
        .iter()
        .find(|project| project_exactly_matches_remote(project, expected_path.as_str(), locator))
        .or_else(|| (projects.len() == 1).then(|| &projects[0]))
        .ok_or_else(|| {
            format!(
                "Gitee project search did not find an exact match for '{}'.",
                locator.remote_repo
            )
        })?;
    value_to_id(project.get("id"), "Gitee")
}

/// Query one page of pull requests using the configured provider contract.
pub(super) async fn get_pull_requests(
    locator: &RepoLocator,
    repo_project_id: &str,
    state: Option<PullRequestState>,
    filters: &PullRequestFilters,
    page: u32,
    per_page: u32,
) -> Result<PullRequestPage, String> {
    if gitee_api_profile(locator) == GiteeApiProfile::EnterpriseV8 {
        log::debug!(
            "[CodeReview][Gitee] Load merge requests: source=enterprise remote_host={} api_base_configured=true enterprise_configured={} token_configured={} state={} page={} per_page={}",
            locator.host,
            configured_enterprise(RepoType::Gitee).is_some(),
            configured_gitee_token(GiteeApiProfile::EnterpriseV8).is_some(),
            state
                .map(PullRequestState::as_str)
                .unwrap_or("provider-default"),
            page.max(1),
            per_page.max(1),
        );
        return get_enterprise_pull_requests(repo_project_id, state, filters, page, per_page).await;
    }
    log::debug!(
        "[CodeReview][Gitee] Load merge requests: source=public remote_host={} api_base_configured={} state={} page={} per_page={}",
        locator.host,
        configured_api_base_url(RepoType::Gitee).is_some(),
        state
            .map(PullRequestState::as_str)
            .unwrap_or("provider-default"),
        page.max(1),
        per_page.max(1),
    );
    get_public_pull_requests(locator, state, filters, page, per_page).await
}

async fn get_enterprise_pull_requests(
    repo_project_id: &str,
    state: Option<PullRequestState>,
    filters: &PullRequestFilters,
    page: u32,
    per_page: u32,
) -> Result<PullRequestPage, String> {
    let repo_project_id = repo_project_id.trim();
    if repo_project_id.is_empty() {
        return Err("Gitee project id is empty.".to_string());
    }
    let api_base_url = enterprise_api_base_url()?;
    let endpoint = endpoints::ENTERPRISE_MERGE_REQUESTS_URL
        .replace("{api_base_url}", &api_base_url)
        .replace(
            "{project_id}",
            percent_encode_path(repo_project_id).as_str(),
        );
    let per_page = per_page.max(1);
    let (order_by, sort) = gitee_enterprise_list_sort(filters.sort.as_str());
    let per_page_query = per_page.to_string();
    let page_query = page.max(1).to_string();
    let mut request = http::get(endpoint)
        .header("OAUTH", "enabled")
        .header(ACCEPT, "application/json")
        .query(&[
            ("per_page", per_page_query.as_str()),
            ("page", page_query.as_str()),
            ("order_by", order_by),
            ("sort", sort),
        ]);
    if let Some(enterprise) = configured_enterprise(RepoType::Gitee) {
        request = request.header("enterprise", enterprise);
    }
    if let Some(state) = state {
        request = request.query(&gitee_enterprise_state_query(state));
    }
    if !filters.query.trim().is_empty() {
        request = request.query(&[("search", filters.query.trim())]);
    }
    if let Some(token) = configured_gitee_token(GiteeApiProfile::EnterpriseV8) {
        request = request.header("PRIVATE-TOKEN", token);
    }
    decode_pull_request_page(request, page, per_page).await
}

fn gitee_enterprise_state_query(state: PullRequestState) -> Vec<(&'static str, &'static str)> {
    if state == PullRequestState::Closed {
        // GitHub's Closed tab includes both merged and closed-without-merge.
        // Gitee Enterprise exposes a multi-state query for the same union.
        vec![("states[]", "closed"), ("states[]", "merged")]
    } else {
        vec![("state", state.gitee_query())]
    }
}

fn enterprise_api_base_url() -> Result<String, String> {
    let configured = configured_api_base_url(RepoType::Gitee).ok_or_else(|| {
        "Configure the Gitee API Base URL in Settings before using this enterprise repository."
            .to_string()
    })?;
    let parsed = reqwest::Url::parse(configured.as_str())
        .map_err(|_| "The configured Gitee API Base URL is invalid.".to_string())?;
    if parsed
        .path()
        .trim_end_matches('/')
        .ends_with(endpoints::ENTERPRISE_API_VERSION_PATH)
    {
        Ok(configured)
    } else {
        Ok(format!(
            "{configured}{}",
            endpoints::ENTERPRISE_API_VERSION_PATH
        ))
    }
}

pub(super) async fn get_pull_request_diff(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
    ignore_whitespace_change: bool,
) -> Result<PullRequestDiffDetail, String> {
    if gitee_api_profile(locator) == GiteeApiProfile::PublicV5 {
        return get_public_pull_request_diff(locator, pull_request_iid).await;
    }

    let diffs_endpoint = endpoints::ENTERPRISE_MERGE_REQUEST_DIFFS_URL
        .replace("{api_base_url}", enterprise_api_base_url()?.as_str())
        .replace(
            "{project_id}",
            percent_encode_path(repo_project_id.trim()).as_str(),
        )
        .replace(
            "{merge_request_iid}",
            percent_encode_path(pull_request_iid.trim()).as_str(),
        );
    let mut diffs_request = http::post(diffs_endpoint)
        .header("OAUTH", "enabled")
        .header(ACCEPT, "application/json")
        .json(&serde_json::json!({
            "paths": [],
            "view_mode": "",
            "ignore_whitespace_change": ignore_whitespace_change,
        }));
    if let Some(enterprise) = configured_enterprise(RepoType::Gitee) {
        diffs_request = diffs_request.header("enterprise", enterprise);
    }
    if let Some(token) = configured_gitee_token(GiteeApiProfile::EnterpriseV8) {
        diffs_request = diffs_request.header("PRIVATE-TOKEN", token);
    }

    log::debug!(
        "[CodeReview][Gitee] Load pull-request diff: remote_host={} project_id_set=true iid={} ignore_whitespace={} enterprise_configured={} token_configured={}",
        locator.host,
        pull_request_iid,
        ignore_whitespace_change,
        configured_enterprise(RepoType::Gitee).is_some(),
        configured_gitee_token(GiteeApiProfile::EnterpriseV8).is_some(),
    );
    let diffs_future = async {
        http::send_json(
            diffs_request,
            RequestOptions::new("Load Gitee pull request diff").retry_transient(3),
        )
        .await
        .map_err(|error| error.to_string())
    };
    // MR detail supplies Conversation/Commits badge summaries without waiting for
    // the lazy Conversation tab payload.
    let detail_future = async {
        let detail_endpoint = enterprise_pull_request_endpoint(
            endpoints::ENTERPRISE_MERGE_REQUEST_URL,
            repo_project_id,
            pull_request_iid,
        )?;
        let detail: Value = http::send_json(
            enterprise_pull_request_get(detail_endpoint),
            RequestOptions::new("Load Gitee enterprise pull request detail summary")
                .retry_transient(3),
        )
        .await
        .map_err(|error| error.to_string())?;
        log_enterprise_pull_request_detail_shape("diff-summary", &detail);
        Ok::<Value, String>(detail)
    };
    let (payload, detail) = tokio::join!(diffs_future, detail_future);
    let payload = payload?;
    if let Err(error) = &detail {
        log::debug!(
            "[CodeReview][Gitee] Enterprise badge detail summary unavailable; using bounded probes: {error}"
        );
    }
    let mut decoded = decode_pull_request_diff(&payload)?;
    // Enterprise Conversation includes ordinary notes plus one current-state
    // summary for every active approved/rejected reviewer in MR detail.
    let active_review_count = detail
        .as_ref()
        .ok()
        .map(|detail| {
            enterprise_review_summaries(enterprise_data_object(detail), None).len() as u32
        })
        .unwrap_or(0);
    // `Option::or` would evaluate (and await) the probe unconditionally, so the
    // fallback request must stay behind an explicit miss check.
    let note_count = match detail
        .as_ref()
        .ok()
        .and_then(conversation_count_from_detail)
    {
        Some(count) => count,
        None => count_pull_request_comments(locator, pull_request_iid, Some(repo_project_id))
            .await
            .unwrap_or(0),
    };
    decoded.conversation_count = Some(note_count.saturating_add(active_review_count));
    // Some Enterprise deployments advertise zero in MR detail/diffs until the
    // commits collection is requested. Only a positive summary is authoritative;
    // zero or a missing field needs the lightweight commits probe.
    let advertised_commit_count = trusted_enterprise_commit_count(detail.as_ref().ok(), &payload);
    decoded.commit_count = match advertised_commit_count {
        Some(count) if count > 0 => count,
        _ => count_pull_request_commits(locator, pull_request_iid, Some(repo_project_id))
            .await
            .unwrap_or(0),
    };
    Ok(decoded)
}

async fn get_public_pull_request_diff(
    locator: &RepoLocator,
    pull_request_iid: &str,
) -> Result<PullRequestDiffDetail, String> {
    let detail_endpoint = public_repo_endpoint(
        endpoints::PUBLIC_PULL_REQUEST_URL,
        locator,
        Some(pull_request_iid),
        None,
    );
    let detail: Value = http::send_json(
        public_authenticated_request(detail_endpoint),
        RequestOptions::new("Load Gitee pull request details").retry_transient(3),
    )
    .await
    .map_err(|error| error.to_string())?;
    let files_endpoint = public_repo_endpoint(
        endpoints::PUBLIC_PULL_REQUEST_FILES_URL,
        locator,
        Some(pull_request_iid),
        None,
    );
    let mut pages = PageCollector::new(100, 30);
    loop {
        let batch: Vec<Value> = http::send_json(
            public_authenticated_request(files_endpoint.clone()).query(&[
                ("page", pages.page().to_string()),
                ("per_page", "100".to_string()),
            ]),
            RequestOptions::new("Load Gitee pull request files").retry_transient(3),
        )
        .await
        .map_err(|error| error.to_string())?;
        if !pages.append(batch) {
            break;
        }
    }
    let (values, files_truncated) = pages.into_parts();
    let files = values
        .iter()
        .map(|value| {
            let path = string_field(value, "filename")
                .or_else(|| string_field(value, "new_path"))
                .unwrap_or_else(|| "Unknown file".to_string());
            let old_path = string_field(value, "previous_filename")
                .or_else(|| string_field(value, "old_path"))
                .unwrap_or_else(|| path.clone());
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
                .or_else(|| string_field(value, "diff"))
                .map(|diff| super::parse_unified_diff(diff.as_str()))
                .unwrap_or_default();
            if lines.is_empty() {
                return super::unavailable_diff_file(
                    path,
                    old_path,
                    change_type,
                    added_lines,
                    removed_lines,
                    "Gitee omitted this binary or oversized patch.",
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
        })
        .collect::<Vec<_>>();
    // Public v5 PR detail often omits commit totals; probe a bounded page only
    // when the detail payload misses the count. `Option::or` would await the
    // probe unconditionally, so both fallbacks stay behind explicit miss checks.
    let commit_count = match commit_count_from_detail(&detail) {
        Some(count) => count,
        None => count_pull_request_commits(locator, pull_request_iid, None)
            .await
            .unwrap_or(0),
    };
    let conversation_count = match conversation_count_from_detail(&detail) {
        Some(count) => count,
        None => count_pull_request_comments(locator, pull_request_iid, None)
            .await
            .unwrap_or(0),
    };
    Ok(PullRequestDiffDetail {
        added_lines: files.iter().map(|file| file.added_lines).sum(),
        removed_lines: files.iter().map(|file| file.removed_lines).sum(),
        commit_count,
        conversation_count: Some(conversation_count),
        base_sha: nested_string(&detail, "base", "sha").unwrap_or_default(),
        head_sha: nested_string(&detail, "head", "sha").unwrap_or_default(),
        start_sha: nested_string(&detail, "base", "sha").unwrap_or_default(),
        too_many_commits: false,
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
    ignore_whitespace_change: bool,
) -> Result<PullRequestDiffDetail, String> {
    let payload: Value = match gitee_api_profile(locator) {
        GiteeApiProfile::PublicV5 => {
            let endpoint = public_repo_endpoint(
                endpoints::PUBLIC_REPOSITORY_COMPARE_URL,
                locator,
                None,
                None,
            )
            .replace("{base}", percent_encode_path(base_sha.trim()).as_str())
            .replace("{head}", percent_encode_path(head_sha.trim()).as_str());
            http::send_json(
                public_authenticated_request(endpoint),
                RequestOptions::new("Compare Gitee pull request commits").retry_transient(3),
            )
            .await
            .map_err(|error| error.to_string())?
        }
        GiteeApiProfile::EnterpriseV8 => {
            let endpoint = endpoints::ENTERPRISE_REPOSITORY_COMPARE_DIFFS_URL
                .replace("{api_base_url}", enterprise_api_base_url()?.as_str())
                .replace(
                    "{project_id}",
                    percent_encode_path(repo_project_id.trim()).as_str(),
                );
            let request = enterprise_authenticated_builder(http::post(endpoint)).json(
                &enterprise_compare_diff_payload(base_sha, head_sha, ignore_whitespace_change),
            );
            http::send_json(
                request,
                RequestOptions::new("Compare Gitee Enterprise pull request commits")
                    .retry_transient(3),
            )
            .await
            .map_err(|error| error.to_string())?
        }
    };

    let values = payload
        .get("files")
        .or_else(|| payload.get("diffs"))
        .and_then(Value::as_array)
        .ok_or_else(|| "Gitee commit comparison response has no diff list.".to_string())?;
    let files = values
        .iter()
        .map(decode_compare_diff_file)
        .collect::<Vec<_>>();
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
        files_truncated: payload
            .get("compare_timeout")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        files,
    })
}

fn enterprise_compare_diff_payload(
    base_sha: &str,
    head_sha: &str,
    ignore_whitespace_change: bool,
) -> Value {
    serde_json::json!({
        // Enterprise names the newer/source ref `from` and the older/target
        // ref `to`, opposite to conventional base..head notation.
        "from": head_sha.trim(),
        "to": base_sha.trim(),
        "paths": [],
        "view_mode": "",
        "ignore_whitespace_change": ignore_whitespace_change,
    })
}

fn decode_compare_diff_file(value: &Value) -> PullRequestDiffFile {
    if value.get("statistic").is_some() {
        return decode_diff_file(value);
    }
    let path = string_field(value, "filename")
        .or_else(|| string_field(value, "new_path"))
        .unwrap_or_else(|| "Unknown file".to_string());
    let old_path = string_field(value, "previous_filename")
        .or_else(|| string_field(value, "old_path"))
        .unwrap_or_else(|| path.clone());
    let change_type = match string_field(value, "status").as_deref() {
        Some("added") => "new_file",
        Some("removed") => "deleted_file",
        Some("renamed") => "renamed_file",
        _ if value
            .get("new_file")
            .and_then(Value::as_bool)
            .unwrap_or(false) =>
        {
            "new_file"
        }
        _ if value
            .get("deleted_file")
            .and_then(Value::as_bool)
            .unwrap_or(false) =>
        {
            "deleted_file"
        }
        _ if value
            .get("renamed_file")
            .and_then(Value::as_bool)
            .unwrap_or(false) =>
        {
            "renamed_file"
        }
        _ => "edit_file",
    }
    .to_string();
    let lines = string_field(value, "patch")
        .or_else(|| string_field(value, "diff"))
        .map(|diff| super::parse_unified_diff(diff.as_str()))
        .unwrap_or_default();
    let added_lines = value_u32(value, "additions")
        .unwrap_or_else(|| lines.iter().filter(|line| line.kind == "addition").count() as u32);
    let removed_lines = value_u32(value, "deletions")
        .unwrap_or_else(|| lines.iter().filter(|line| line.kind == "deletion").count() as u32);
    if lines.is_empty() {
        return super::unavailable_diff_file(
            path,
            old_path,
            change_type,
            added_lines,
            removed_lines,
            "Gitee omitted this binary, collapsed, or oversized comparison.",
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
    let request = if gitee_api_profile(locator) == GiteeApiProfile::EnterpriseV8 {
        let endpoint = endpoints::ENTERPRISE_REPOSITORY_FILE_URL
            .replace("{api_base_url}", enterprise_api_base_url()?.as_str())
            .replace(
                "{project_id}",
                percent_encode_path(repo_project_id.trim()).as_str(),
            )
            .replace("{file_path}", percent_encode_path(path).as_str());
        let mut request = http::get(endpoint)
            .header("OAUTH", "enabled")
            .header(ACCEPT, "application/json")
            .query(&[("ref", reference)]);
        if let Some(enterprise) = configured_enterprise(RepoType::Gitee) {
            request = request.header("enterprise", enterprise);
        }
        if let Some(token) = configured_gitee_token(GiteeApiProfile::EnterpriseV8) {
            request = request.header("PRIVATE-TOKEN", token);
        }
        request
    } else {
        let endpoint = public_repo_endpoint(
            endpoints::PUBLIC_REPOSITORY_FILE_URL,
            locator,
            None,
            Some(path),
        );
        public_authenticated_request(endpoint).query(&[("ref", reference)])
    };
    let payload: Value = http::send_json(
        request,
        RequestOptions::new("Load Gitee repository file").retry_transient(3),
    )
    .await
    .map_err(|error| error.to_string())?;
    decode_base64_content(&payload)
}

pub(super) async fn get_pull_request_action_state(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
) -> Result<PullRequestActionState, String> {
    match gitee_api_profile(locator) {
        GiteeApiProfile::EnterpriseV8 => {
            get_enterprise_pull_request_action_state(repo_project_id, pull_request_iid).await
        }
        GiteeApiProfile::PublicV5 => {
            get_public_pull_request_action_state(locator, pull_request_iid).await
        }
    }
}

async fn get_enterprise_pull_request_action_state(
    repo_project_id: &str,
    pull_request_iid: &str,
) -> Result<PullRequestActionState, String> {
    if configured_gitee_token(GiteeApiProfile::EnterpriseV8).is_none() {
        return Ok(PullRequestActionState {
            can_comment: false,
            can_approve: false,
            can_request_changes: false,
            can_merge: false,
            can_revert: false,
            review_state: String::new(),
            readiness: PullRequestReadiness::new(
                PullRequestReadinessKind::Unavailable,
                "Configure a Gitee Enterprise Token to load the enterprise merge conditions.",
            ),
            merge_block_reason:
                "Configure a Gitee Enterprise Token in Settings to review or merge this pull request."
                    .to_string(),
            revert_block_reason:
                "Configure a Gitee Enterprise Token in Settings to inspect and revert a supported merged pull request."
                    .to_string(),
            revert_action_label: "Revert pull request".to_string(),
            revert_confirmation:
                "Gitee Enterprise will add a revert commit directly to the target branch. The original pull request remains merged."
                    .to_string(),
            allowed_merge_methods: Vec::new(),
        });
    }
    let current_endpoint = enterprise_pull_request_endpoint(
        endpoints::ENTERPRISE_MERGE_REQUEST_CURRENT_STATE_URL,
        repo_project_id,
        pull_request_iid,
    )?;
    let detail_endpoint = enterprise_pull_request_endpoint(
        endpoints::ENTERPRISE_MERGE_REQUEST_URL,
        repo_project_id,
        pull_request_iid,
    )?;
    // The current merge state and the pull request detail are independent
    // reads; fetch them concurrently and keep each error path intact.
    let current_future = http::send_json::<Value>(
        enterprise_pull_request_get(current_endpoint),
        RequestOptions::new("Load Gitee enterprise merge state").retry_transient(3),
    );
    let detail_future = http::send_json::<Value>(
        enterprise_pull_request_get(detail_endpoint),
        RequestOptions::new("Load Gitee enterprise pull request details").retry_transient(3),
    );
    let (current, detail) = tokio::join!(current_future, detail_future);
    let current = current.map_err(|error| error.to_string())?;
    let detail = detail.map_err(|error| error.to_string())?;
    log_enterprise_pull_request_detail_shape("action-state", &detail);

    let has_token = configured_gitee_token(GiteeApiProfile::EnterpriseV8).is_some();
    let current_object = enterprise_data_object(&current);
    let detail_object = enterprise_data_object(&detail);
    let server_can_merge = current_object
        .get("can_merge")
        .and_then(Value::as_bool)
        .or_else(|| current_object.get("can_be_merge").and_then(Value::as_bool))
        .unwrap_or(false);
    let review_state = string_field(detail_object, "review_state")
        .or_else(|| string_field(current_object, "review_state"))
        .unwrap_or_default();
    let readiness =
        gitee_enterprise_merge_readiness(current_object, detail_object, review_state.as_str());
    let terminal = matches!(
        readiness.kind,
        PullRequestReadinessKind::Merged | PullRequestReadinessKind::Closed
    );
    let can_merge =
        has_token && server_can_merge && readiness.kind == PullRequestReadinessKind::Ready;
    let mut allowed_merge_methods = detail_object
        .get("restrict_merge_type")
        .and_then(Value::as_array)
        .map(|methods| {
            methods
                .iter()
                .filter_map(Value::as_str)
                .filter_map(parse_gitee_merge_method)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if allowed_merge_methods.is_empty() {
        allowed_merge_methods = vec![
            PullRequestMergeMethod::Merge,
            PullRequestMergeMethod::Squash,
            PullRequestMergeMethod::Rebase,
            PullRequestMergeMethod::FastForward,
        ];
    }
    let merge_block_reason = if !has_token {
        "Configure a Gitee Enterprise Token in Settings to review or merge this pull request."
            .to_string()
    } else if can_merge {
        String::new()
    } else {
        readiness.detail.clone()
    };
    let (can_revert, revert_block_reason) = if readiness.kind == PullRequestReadinessKind::Merged {
        match enterprise_revert_context_from_detail(
            repo_project_id,
            pull_request_iid,
            detail_object,
        )
        .await
        {
            Ok(_) => (true, String::new()),
            Err(reason) => (false, reason),
        }
    } else {
        (
            false,
            "Only a merged pull request can be reverted.".to_string(),
        )
    };

    Ok(PullRequestActionState {
        // Ordinary Conversation notes remain available after merge/close, matching
        // GitHub's timeline composer. Review decisions stay open-MR only.
        can_comment: has_token,
        can_approve: has_token && !terminal,
        can_request_changes: has_token && !terminal,
        can_merge,
        can_revert,
        review_state,
        readiness,
        merge_block_reason,
        revert_block_reason,
        revert_action_label: "Revert pull request".to_string(),
        revert_confirmation:
            "Gitee Enterprise will add a revert commit directly to the target branch. This changes the branch immediately; it does not create a new pull request, and the original pull request remains merged."
                .to_string(),
        allowed_merge_methods,
    })
}

async fn get_public_pull_request_action_state(
    locator: &RepoLocator,
    pull_request_iid: &str,
) -> Result<PullRequestActionState, String> {
    let endpoint = public_repo_endpoint(
        endpoints::PUBLIC_PULL_REQUEST_URL,
        locator,
        Some(pull_request_iid),
        None,
    );
    let detail: Value = http::send_json(
        public_authenticated_request(endpoint),
        RequestOptions::new("Load Gitee public pull request merge state").retry_transient(3),
    )
    .await
    .map_err(|error| error.to_string())?;
    let has_token = configured_gitee_token(GiteeApiProfile::PublicV5).is_some();
    let readiness = gitee_public_merge_readiness(&detail);
    let can_merge = has_token && readiness.kind == PullRequestReadinessKind::Ready;
    let merge_block_reason = if !has_token {
        "Configure a Gitee Public Token in Settings to comment or merge this pull request."
            .to_string()
    } else {
        readiness.detail.clone()
    };
    Ok(PullRequestActionState {
        can_comment: has_token,
        // Gitee public v5 exposes PR comments but has no approve/request-change review endpoint.
        can_approve: false,
        can_request_changes: false,
        can_merge,
        can_revert: false,
        review_state: String::new(),
        readiness,
        merge_block_reason,
        revert_block_reason:
            "Automated revert is not supported by the configured Gitee Public provider.".to_string(),
        revert_action_label: "Revert changes".to_string(),
        revert_confirmation:
            "This provider does not currently expose a supported automatic revert operation."
                .to_string(),
        allowed_merge_methods: vec![
            PullRequestMergeMethod::Merge,
            PullRequestMergeMethod::Squash,
        ],
    })
}

pub(super) async fn get_pull_request_conversation(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
) -> Result<Vec<PullRequestConversationItem>, String> {
    let profile = gitee_api_profile(locator);
    let (detail_payload, notes, current_user, historical_activity, push_events) = match profile {
        GiteeApiProfile::EnterpriseV8 => {
            let notes_endpoint = enterprise_pull_request_endpoint(
                endpoints::ENTERPRISE_MERGE_REQUEST_NOTES_URL,
                repo_project_id,
                pull_request_iid,
            )?;
            let api_base_url = enterprise_api_base_url()?;
            let current_user_endpoint = endpoints::ENTERPRISE_CURRENT_USER_URL
                .replace("{api_base_url}", api_base_url.as_str());
            let detail_future = async {
                let detail_endpoint = enterprise_pull_request_endpoint(
                    endpoints::ENTERPRISE_MERGE_REQUEST_URL,
                    repo_project_id,
                    pull_request_iid,
                )?;
                let detail: Value = http::send_json(
                    enterprise_pull_request_get(detail_endpoint),
                    RequestOptions::new("Load Gitee enterprise merge request conversation header")
                        .retry_transient(3),
                )
                .await
                .map_err(|error| error.to_string())?;
                log_enterprise_pull_request_detail_shape("conversation", &detail);
                Ok::<Value, String>(detail)
            };
            let notes_future = async {
                http::send_json(
                    enterprise_authenticated_builder(http::get(notes_endpoint))
                        .query(&[("per_page", "100")]),
                    RequestOptions::new("Load Gitee enterprise merge request notes")
                        .retry_transient(3),
                )
                .await
                .map_err(|error| error.to_string())
            };
            let current_user_future = async {
                http::send_json(
                    enterprise_authenticated_builder(http::get(current_user_endpoint)),
                    RequestOptions::new("Load Gitee enterprise current review user")
                        .retry_transient(3),
                )
                .await
                .ok()
            };
            let push_events_future = async {
                let endpoint = enterprise_project_endpoint(
                    endpoints::ENTERPRISE_PROJECT_EVENTS_URL,
                    repo_project_id,
                )
                .ok()?;
                http::send_json(
                    enterprise_authenticated_builder(http::get(endpoint)).query(&[
                        ("type", "push"),
                        // Read the newest project events so a recently pushed PR
                        // branch is not displaced by the project's older history.
                        // The pure normalizer restores chronological display order.
                        ("sort", "desc"),
                        ("page", "1"),
                        ("per_page", "100"),
                    ]),
                    RequestOptions::new("Load Gitee enterprise source branch push events")
                        .retry_transient(3),
                )
                .await
                .ok()
            };
            let (detail, notes, current_user, push_events) = tokio::join!(
                detail_future,
                notes_future,
                current_user_future,
                push_events_future
            );
            (detail?, notes?, current_user, None, push_events)
        }
        GiteeApiProfile::PublicV5 => {
            let detail_endpoint = public_repo_endpoint(
                endpoints::PUBLIC_PULL_REQUEST_URL,
                locator,
                Some(pull_request_iid),
                None,
            );
            let comments_endpoint = public_repo_endpoint(
                endpoints::PUBLIC_PULL_REQUEST_COMMENTS_URL,
                locator,
                Some(pull_request_iid),
                None,
            );
            let operate_logs_endpoint = public_repo_endpoint(
                endpoints::PUBLIC_PULL_REQUEST_OPERATE_LOGS_URL,
                locator,
                Some(pull_request_iid),
                None,
            );
            let detail_future = async {
                http::send_json(
                    public_authenticated_request(detail_endpoint),
                    RequestOptions::new("Load Gitee public pull request conversation header")
                        .retry_transient(3),
                )
                .await
                .map_err(|error| error.to_string())
            };
            let notes_future = async {
                http::send_json(
                    public_authenticated_request(comments_endpoint)
                        .query(&[("page", "1"), ("per_page", "100")]),
                    RequestOptions::new("Load Gitee public pull request comments")
                        .retry_transient(3),
                )
                .await
                .map_err(|error| error.to_string())
            };
            let operate_logs_future = async {
                http::send_json(
                    public_authenticated_request(operate_logs_endpoint).query(&[
                        ("sort", "asc"),
                        ("page", "1"),
                        ("per_page", "100"),
                    ]),
                    RequestOptions::new("Load Gitee public pull request operation logs")
                        .retry_transient(3),
                )
                .await
                .ok()
            };
            let (detail, notes, operate_logs) =
                tokio::join!(detail_future, notes_future, operate_logs_future);
            (detail?, notes?, None, operate_logs, None)
        }
    };
    let detail = if profile == GiteeApiProfile::EnterpriseV8 {
        enterprise_data_object(&detail_payload)
    } else {
        &detail_payload
    };

    let mut items = vec![PullRequestConversationItem {
        id: format!("description-{pull_request_iid}"),
        kind: "description".to_string(),
        author: nested_string(detail, "author", "name")
            .or_else(|| nested_string(detail, "author", "username"))
            .or_else(|| nested_string(detail, "user", "login"))
            .or_else(|| nested_string(detail, "user", "name"))
            .unwrap_or_else(|| "Unknown author".to_string()),
        avatar_url: nested_string(detail, "author", "avatar_url")
            .or_else(|| nested_string(detail, "author", "avatar"))
            .or_else(|| nested_string(detail, "user", "avatar_url"))
            .or_else(|| nested_string(detail, "user", "avatar"))
            .unwrap_or_default(),
        author_association: String::new(),
        body: string_field(detail, "description")
            .or_else(|| string_field(detail, "body"))
            .unwrap_or_default(),
        created_at: string_field(detail, "created_at").unwrap_or_default(),
        state: string_field(detail, "state").unwrap_or_default(),
        review_id: String::new(),
        can_dismiss: false,
    }];
    if let Some(values) = pull_request_values(&notes) {
        match profile {
            // Enterprise GET /notes returns discussion threads in `data`, with
            // comment text nested at `notes[].note` (+ optional `children`).
            // Top-level `content` is diff context and must not be treated as body.
            GiteeApiProfile::EnterpriseV8 => {
                items.extend(
                    expand_enterprise_conversation_notes(values)
                        .into_iter()
                        .filter(|item| {
                            !(item.kind == "event"
                                && (is_redundant_commit_system_note(item.body.as_str())
                                    || is_redundant_merge_system_note(item.body.as_str())))
                        }),
                );
            }
            // Public v5 keeps a flat comment list with `body`.
            GiteeApiProfile::PublicV5 => {
                items.extend(
                    values
                        .iter()
                        .map(decode_conversation_comment)
                        .filter(|item| {
                            !(item.kind == "event"
                                && (is_redundant_commit_system_note(item.body.as_str())
                                    || is_redundant_merge_system_note(item.body.as_str())))
                        }),
                );
            }
        }
    }

    let historical_activity_values = historical_activity.as_ref().and_then(pull_request_values);
    let mut normalized_activity = historical_activity_values
        .map(|values| decode_gitee_operate_log_activity(values))
        .unwrap_or_default();
    if profile == GiteeApiProfile::EnterpriseV8 {
        merge_gitee_review_summaries(
            &mut normalized_activity,
            enterprise_review_summaries(detail, current_user.as_ref()),
        );
    }
    items.extend(normalized_activity);

    let merged_at = string_field(detail, "merged_at").filter(|value| !value.trim().is_empty());
    let merge_sha = string_field(detail, "merge_commit_sha")
        .or_else(|| nested_string(detail, "merge_commit", "sha"))
        .filter(|value| !value.trim().is_empty());
    let merge_commit_future = async {
        if profile == GiteeApiProfile::EnterpriseV8 && merged_at.is_some() && merge_sha.is_some() {
            load_enterprise_merge_commit(
                repo_project_id,
                pull_request_iid,
                merge_sha.as_deref().unwrap_or_default(),
            )
            .await
        } else {
            None
        }
    };
    let merge_actor_account = (profile == GiteeApiProfile::EnterpriseV8 && merged_at.is_some())
        .then(|| gitee_merge_actor_account(detail, historical_activity_values))
        .flatten();
    let merge_actor_user_future = async {
        if let Some(account) = merge_actor_account.as_deref() {
            load_enterprise_merge_actor_user(repo_project_id, account).await
        } else {
            None
        }
    };
    let (commits, merge_commit, merge_actor_user) = tokio::join!(
        load_pull_request_commits(locator, repo_project_id, pull_request_iid, false),
        merge_commit_future,
        merge_actor_user_future,
    );

    // Structured commit/merge rows mirror GitHub Conversation activity for both
    // Gitee profiles. Failures stay silent so notes still render.
    if let Ok(commits) = commits {
        let commit_activity = match profile {
            GiteeApiProfile::EnterpriseV8 => push_events
                .as_ref()
                .and_then(pull_request_values)
                .map(|events| {
                    let source_branch = string_field(detail, "source_branch")
                        .or_else(|| nested_string(detail, "head", "ref"))
                        .unwrap_or_default();
                    let created_at = string_field(detail, "created_at").unwrap_or_default();
                    gitee_push_event_commit_activity(
                        events,
                        source_branch.as_str(),
                        created_at.as_str(),
                        &commits,
                    )
                })
                .unwrap_or_default(),
            GiteeApiProfile::PublicV5 => historical_activity_values
                .map(|events| gitee_operate_log_commit_activity(events, &commits))
                .unwrap_or_default(),
        };
        if commit_activity.is_empty() {
            items.extend(conversation_commit_batch_items(&commits));
        } else {
            items.extend(commit_activity);
        }
    }
    if let (Some(merged_at), Some(merge_sha)) = (merged_at, merge_sha) {
        let target_branch = string_field(detail, "target_branch")
            .or_else(|| nested_string(detail, "base", "ref"))
            .unwrap_or_default();
        let merge_event = gitee_merge_event_with_user(
            detail,
            historical_activity_values,
            merge_commit.as_ref(),
            merge_actor_user.as_ref(),
        );
        let mut merged_item = conversation_merged_item(
            merge_event.author.as_str(),
            merge_sha.as_str(),
            if target_branch.trim().is_empty() {
                "main"
            } else {
                target_branch.as_str()
            },
            if merge_event.created_at.trim().is_empty() {
                merged_at.as_str()
            } else {
                merge_event.created_at.as_str()
            },
        );
        merged_item.avatar_url = merge_event.avatar_url;
        items.push(merged_item);
    }

    let description = items.remove(0);
    let activity = items;
    Ok(finalize_conversation_timeline(description, activity))
}

async fn load_enterprise_merge_actor_user(repo_project_id: &str, account: &str) -> Option<Value> {
    let members_endpoint =
        enterprise_project_endpoint(endpoints::ENTERPRISE_PROJECT_MEMBERS_URL, repo_project_id)
            .ok()?;
    let members: Option<Value> = http::send_json(
        enterprise_authenticated_builder(http::get(members_endpoint)).query(&[
            ("is_all", "true"),
            ("with_page", "true"),
            ("page", "1"),
            ("per_page", "100"),
            ("query", account),
        ]),
        RequestOptions::new("Resolve Gitee enterprise merge actor from project members"),
    )
    .await
    .ok();
    if let Some(user) = members
        .as_ref()
        .and_then(|payload| select_enterprise_user(payload, account))
        .filter(|user| enterprise_user_has_distinct_display_name(user, account))
    {
        return Some(user.clone());
    }

    let users_endpoint = endpoints::ENTERPRISE_USERS_URL
        .replace("{api_base_url}", enterprise_api_base_url().ok()?.as_str());
    let users: Value = http::send_json(
        enterprise_authenticated_builder(http::get(users_endpoint)).query(&[("search", account)]),
        RequestOptions::new("Resolve Gitee enterprise merge actor from user directory"),
    )
    .await
    .ok()?;
    select_enterprise_user(&users, account)
        .filter(|user| enterprise_user_has_distinct_display_name(user, account))
        .cloned()
}

fn select_enterprise_user<'a>(payload: &'a Value, account: &str) -> Option<&'a Value> {
    let account = account.trim();
    let users = enterprise_user_values(payload)?;
    users
        .iter()
        .find(|user| {
            enterprise_user_login(user).is_some_and(|value| value.eq_ignore_ascii_case(account))
        })
        .or_else(|| {
            users.iter().find(|user| {
                string_field(user, "email")
                    .and_then(|email| email.split_once('@').map(|(local, _)| local.to_string()))
                    .is_some_and(|local| local.eq_ignore_ascii_case(account))
            })
        })
        .or_else(|| {
            users.iter().find(|user| {
                string_field(user, "name").is_some_and(|name| name.eq_ignore_ascii_case(account))
            })
        })
}

fn enterprise_user_values(payload: &Value) -> Option<&Vec<Value>> {
    payload
        .as_array()
        .or_else(|| payload.get("data").and_then(Value::as_array))
        .or_else(|| payload.get("members").and_then(Value::as_array))
        .or_else(|| payload.get("users").and_then(Value::as_array))
        .or_else(|| {
            let data = payload.get("data")?;
            data.get("members")
                .and_then(Value::as_array)
                .or_else(|| data.get("users").and_then(Value::as_array))
        })
}

fn enterprise_user_has_distinct_display_name(user: &Value, account: &str) -> bool {
    [
        "display_name",
        "displayName",
        "nickname",
        "nick_name",
        "real_name",
        "realName",
        "full_name",
        "fullName",
        "remark",
        "name",
    ]
    .into_iter()
    .filter_map(|field| string_field(user, field))
    .any(|name| {
        let name = name.trim();
        !name.is_empty()
            && !name.eq_ignore_ascii_case(account)
            && !name
                .split_once('@')
                .is_some_and(|(local, _)| local.eq_ignore_ascii_case(account))
    })
}

async fn load_enterprise_merge_commit(
    repo_project_id: &str,
    _pull_request_iid: &str,
    merge_sha: &str,
) -> Option<Value> {
    let endpoint = enterprise_repository_commit_endpoint(
        endpoints::ENTERPRISE_REPOSITORY_COMMIT_URL,
        repo_project_id,
        merge_sha,
    )
    .ok()?;
    let payload: Value = http::send_json(
        enterprise_authenticated_builder(http::get(endpoint)),
        RequestOptions::new("Load Gitee enterprise merge commit actor").retry_transient(3),
    )
    .await
    .ok()?;
    Some(enterprise_data_object(&payload).clone())
}

fn enterprise_review_summaries(
    detail: &Value,
    current_user: Option<&Value>,
) -> Vec<PullRequestConversationItem> {
    let current_user = current_user.map(enterprise_data_object);
    let mut seen = std::collections::HashSet::new();
    ["general_reviewers", "primary_reviewers"]
        .into_iter()
        .filter_map(|key| detail.get(key).and_then(Value::as_array))
        .flatten()
        .filter_map(|reviewer| {
            let mut review = decode_conversation_review(reviewer)?;
            if !matches!(review.state.as_str(), "approved" | "rejected") {
                return None;
            }
            let identity = enterprise_user_identity(reviewer);
            if !seen.insert(identity.clone()) {
                return None;
            }
            review.review_id = identity.clone();
            let can_dismiss = current_user.is_some_and(|current_user| {
                enterprise_review_belongs_to_current_user(reviewer, current_user)
            });
            Some(PullRequestConversationItem {
                id: format!("review-summary-{identity}"),
                kind: "review_summary".to_string(),
                author: review.author,
                avatar_url: review.avatar_url,
                author_association: review.author_association,
                body: gitee_review_action(review.state.as_str()).to_string(),
                // The MR detail exposes reviewer state but no review timestamp.
                created_at: String::new(),
                state: review.state,
                review_id: review.review_id,
                can_dismiss,
            })
        })
        .collect()
}

fn log_enterprise_pull_request_detail_shape(context: &str, payload: &Value) {
    let detail = enterprise_data_object(payload);
    let general = detail
        .get("general_reviewers")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let primary = detail
        .get("primary_reviewers")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut approved = 0usize;
    let mut rejected = 0usize;
    let mut commented = 0usize;
    let mut initialized = 0usize;
    let mut empty = 0usize;
    let mut other = 0usize;
    for reviewer in general.iter().chain(primary.iter()) {
        match string_field(reviewer, "approve_status")
            .map(|state| normalize_enterprise_review_state(state.as_str()))
            .unwrap_or_default()
            .as_str()
        {
            "approved" => approved += 1,
            "rejected" => rejected += 1,
            "commented" => commented += 1,
            "initialized" => initialized += 1,
            "" => empty += 1,
            _ => other += 1,
        }
    }
    log::debug!(
        "[CodeReview][Gitee] Enterprise MR detail shape: context={} id={} iid={} general_reviewers={} primary_reviewers={} approve_status_counts=approved:{},rejected:{},commented:{},initialized:{},empty:{},other:{}",
        context,
        string_field(detail, "id").unwrap_or_else(|| "missing".to_string()),
        string_field(detail, "iid").unwrap_or_else(|| "missing".to_string()),
        general.len(),
        primary.len(),
        approved,
        rejected,
        commented,
        initialized,
        empty,
        other,
    );
}

fn enterprise_review_belongs_to_current_user(review: &Value, current_user: &Value) -> bool {
    let current_user = enterprise_data_object(current_user);
    ["author", "user", "reviewer"]
        .into_iter()
        .filter_map(|key| review.get(key))
        .chain(std::iter::once(review))
        .any(|candidate| enterprise_users_match(candidate, current_user))
}

fn enterprise_users_match(candidate: &Value, current_user: &Value) -> bool {
    ["id", "uuid"].into_iter().any(|field| {
        string_field(candidate, field)
            .zip(string_field(current_user, field))
            .is_some_and(|(candidate, current)| candidate == current)
    }) || enterprise_user_login(candidate)
        .zip(enterprise_user_login(current_user))
        .is_some_and(|(candidate, current)| candidate == current)
}

fn enterprise_user_login(user: &Value) -> Option<String> {
    ["username", "user_name", "login"]
        .into_iter()
        .find_map(|field| string_field(user, field))
        .filter(|value| !value.trim().is_empty())
}

fn enterprise_user_identity(user: &Value) -> String {
    string_field(user, "id")
        .or_else(|| string_field(user, "uuid"))
        .or_else(|| enterprise_user_login(user))
        .unwrap_or_else(|| "unknown-reviewer".to_string())
}

fn gitee_review_action(state: &str) -> &'static str {
    match state {
        "approved" => "approved these changes",
        "rejected" => "requested changes",
        "initialized" => "withdrew a review",
        _ => "reviewed these changes",
    }
}

pub(super) async fn get_pull_request_commits(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
) -> Result<Vec<PullRequestCommit>, String> {
    load_pull_request_commits(locator, repo_project_id, pull_request_iid, true).await
}

async fn load_pull_request_commits(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
    enrich_identities: bool,
) -> Result<Vec<PullRequestCommit>, String> {
    let profile = gitee_api_profile(locator);
    let endpoint = match profile {
        GiteeApiProfile::EnterpriseV8 => enterprise_pull_request_endpoint(
            endpoints::ENTERPRISE_MERGE_REQUEST_COMMITS_URL,
            repo_project_id,
            pull_request_iid,
        )?,
        GiteeApiProfile::PublicV5 => public_repo_endpoint(
            endpoints::PUBLIC_PULL_REQUEST_COMMITS_URL,
            locator,
            Some(pull_request_iid),
            None,
        ),
    };
    let request = match profile {
        GiteeApiProfile::EnterpriseV8 => {
            enterprise_authenticated_builder(http::get(endpoint)).query(&[("per_page", "100")])
        }
        GiteeApiProfile::PublicV5 => {
            public_authenticated_request(endpoint).query(&[("page", "1"), ("per_page", "100")])
        }
    };
    let payload: Value = http::send_json(
        request,
        RequestOptions::new("Load Gitee pull request commits").retry_transient(3),
    )
    .await
    .map_err(|error| error.to_string())?;
    let values = pull_request_values(&payload)
        .ok_or_else(|| "Gitee pull request commit response is not a list.".to_string())?;
    let mut commits = values.iter().map(decode_commit).collect::<Vec<_>>();
    if profile == GiteeApiProfile::EnterpriseV8 {
        normalize_enterprise_commit_order(&mut commits);
        if enrich_identities {
            commits = enrich_enterprise_commit_identities(repo_project_id, commits).await;
        }
    }
    Ok(commits)
}

async fn enrich_enterprise_commit_identities(
    repo_project_id: &str,
    commits: Vec<PullRequestCommit>,
) -> Vec<PullRequestCommit> {
    let mut enriched = stream::iter(commits.into_iter().enumerate())
        .map(|(index, mut commit)| async move {
            let Some(endpoint) = enterprise_repository_commit_endpoint(
                endpoints::ENTERPRISE_REPOSITORY_COMMIT_URL,
                repo_project_id,
                commit.sha.as_str(),
            )
            .ok() else {
                return (index, commit, String::new(), String::new());
            };
            let detail = http::send_json::<Value>(
                enterprise_authenticated_builder(http::get(endpoint)),
                RequestOptions::new("Load Gitee enterprise commit author and committer")
                    .retry_transient(3),
            )
            .await
            .ok();
            let mut author_email = String::new();
            let mut committer_email = String::new();
            if let Some(detail) = detail.as_ref().map(enterprise_data_object) {
                commit.author = string_field(detail, "author_name")
                    .or_else(|| string_field(detail, "authorName"))
                    .unwrap_or(commit.author);
                commit.avatar_url = nested_string(detail, "author", "avatar_url")
                    .or_else(|| nested_string(detail, "author", "avatar"))
                    .unwrap_or(commit.avatar_url);
                commit.authored_at = string_field(detail, "authored_date")
                    .or_else(|| string_field(detail, "authoredDate"))
                    .unwrap_or(commit.authored_at);
                author_email = string_field(detail, "author_email")
                    .or_else(|| string_field(detail, "authorEmail"))
                    .unwrap_or_default();
                commit.committer = string_field(detail, "committer_name")
                    .or_else(|| string_field(detail, "committerName"))
                    .or_else(|| nested_string(detail, "committer", "name"))
                    .unwrap_or(commit.committer);
                commit.committer_avatar_url = nested_string(detail, "committer", "avatar_url")
                    .or_else(|| nested_string(detail, "committer", "avatar"))
                    .unwrap_or(commit.committer_avatar_url);
                commit.committed_at = string_field(detail, "committed_date")
                    .or_else(|| string_field(detail, "committedDate"))
                    .unwrap_or(commit.committed_at);
                committer_email = string_field(detail, "committer_email")
                    .or_else(|| string_field(detail, "committerEmail"))
                    .unwrap_or_default();
            }
            (index, commit, author_email, committer_email)
        })
        .buffer_unordered(8)
        .collect::<Vec<_>>()
        .await;
    enriched.sort_by_key(|(index, _, _, _)| *index);

    let mut identities = std::collections::HashMap::<String, (String, String)>::new();
    for (_, commit, author_email, committer_email) in &enriched {
        for (name, email) in [
            (commit.author.as_str(), author_email.as_str()),
            (commit.committer.as_str(), committer_email.as_str()),
        ] {
            if !name.trim().is_empty() && !email.trim().is_empty() {
                identities
                    .entry(enterprise_commit_identity_key(name, email))
                    .or_insert_with(|| (name.to_string(), email.to_string()));
            }
        }
    }
    let avatars = load_enterprise_commit_identity_avatars(identities).await;
    enriched
        .into_iter()
        .map(|(_, mut commit, author_email, committer_email)| {
            if commit.avatar_url.trim().is_empty() {
                commit.avatar_url = avatars
                    .get(&enterprise_commit_identity_key(
                        commit.author.as_str(),
                        author_email.as_str(),
                    ))
                    .cloned()
                    .unwrap_or_default();
            }
            if commit.committer_avatar_url.trim().is_empty() {
                commit.committer_avatar_url = avatars
                    .get(&enterprise_commit_identity_key(
                        commit.committer.as_str(),
                        committer_email.as_str(),
                    ))
                    .cloned()
                    .unwrap_or_default();
            }
            commit
        })
        .collect()
}

fn enterprise_commit_identity_key(name: &str, email: &str) -> String {
    format!(
        "{}\n{}",
        name.trim().to_ascii_lowercase(),
        email.trim().to_ascii_lowercase()
    )
}

async fn load_enterprise_commit_identity_avatars(
    identities: std::collections::HashMap<String, (String, String)>,
) -> std::collections::HashMap<String, String> {
    let Some(api_base_url) = enterprise_api_base_url().ok() else {
        return std::collections::HashMap::new();
    };
    let endpoint = endpoints::ENTERPRISE_USERS_URL.replace("{api_base_url}", &api_base_url);
    let mut resolved = HashMap::new();
    let mut pending = Vec::new();
    {
        let mut cache = identity_avatar_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (key, (name, email)) in identities {
            let cache_key = format!("{api_base_url}\n{key}");
            match cache.get(cache_key.as_str()) {
                Some(Some(avatar)) => {
                    resolved.insert(key, avatar);
                }
                Some(None) => {}
                None => pending.push((key, cache_key, name, email)),
            }
        }
    }

    let downloaded = stream::iter(pending)
        .map(|(key, cache_key, name, email)| {
            let endpoint = endpoint.clone();
            async move {
                let payload = http::send_json::<Value>(
                    enterprise_authenticated_builder(http::get(endpoint))
                        .query(&[("search", name.as_str())]),
                    RequestOptions::new("Resolve Gitee enterprise commit identity avatar")
                        .retry_transient(3),
                )
                .await
                .ok();
                let avatar = payload.as_ref().and_then(|payload| {
                    let users = enterprise_user_values(payload)?;
                    let mut exact = users.iter().filter(|user| {
                        string_field(user, "email")
                            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(email.as_str()))
                    });
                    let user = exact.next()?;
                    if exact.next().is_some() {
                        return None;
                    }
                    string_field(user, "avatar_url")
                        .or_else(|| string_field(user, "avatar"))
                        .or_else(|| string_field(user, "image_path"))
                        .or_else(|| string_field(user, "imagePath"))
                        .filter(|avatar| !avatar.trim().is_empty())
                });
                (key, cache_key, avatar)
            }
        })
        .buffer_unordered(6)
        .collect::<Vec<_>>()
        .await;
    let mut cache = identity_avatar_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for (key, cache_key, avatar) in downloaded {
        cache.insert(cache_key, avatar.clone());
        if let Some(avatar) = avatar {
            resolved.insert(key, avatar);
        }
    }
    resolved
}

/// Commit range selection expects the provider-neutral list to run from the
/// oldest PR commit to the newest. Enterprise deployments commonly return the
/// MR collection newest-first, while other Gitee profiles return oldest-first.
/// array; badge probes must force `with_page=true` and read `meta.total_count`.
async fn count_pull_request_commits(
    locator: &RepoLocator,
    pull_request_iid: &str,
    repo_project_id: Option<&str>,
) -> Option<u32> {
    const PUBLIC_PROBE_PER_PAGE: u32 = 100;
    let profile = gitee_api_profile(locator);
    let endpoint = match profile {
        GiteeApiProfile::EnterpriseV8 => enterprise_pull_request_endpoint(
            endpoints::ENTERPRISE_MERGE_REQUEST_COMMITS_URL,
            repo_project_id?,
            pull_request_iid,
        )
        .ok()?,
        GiteeApiProfile::PublicV5 => public_repo_endpoint(
            endpoints::PUBLIC_PULL_REQUEST_COMMITS_URL,
            locator,
            Some(pull_request_iid),
            None,
        ),
    };
    let (request, per_page) = match profile {
        GiteeApiProfile::EnterpriseV8 => (
            enterprise_authenticated_builder(http::get(endpoint)).query(&[
                ("with_page", "true"),
                ("page", "1"),
                ("per_page", "1"),
            ]),
            1_u32,
        ),
        GiteeApiProfile::PublicV5 => (
            public_authenticated_request(endpoint).query(&[("page", "1"), ("per_page", "100")]),
            PUBLIC_PROBE_PER_PAGE,
        ),
    };
    let response = http::send(
        request,
        RequestOptions::new("Count Gitee pull request commits").retry_transient(3),
    )
    .await
    .ok()?;
    let headers = response.headers().clone();
    let payload: Value = response.json().await.ok()?;
    match profile {
        GiteeApiProfile::EnterpriseV8 => {
            if let Some(total) = enterprise_commit_probe_total(&headers, &payload, per_page) {
                return Some(total);
            }

            // A few Enterprise v8 deployments accept `with_page=true` but omit
            // every pagination total. The documented non-paginated response is
            // a complete commit collection, so use it only as a compatibility
            // fallback for the badge. The Commits tab model remains lazy.
            let endpoint = enterprise_pull_request_endpoint(
                endpoints::ENTERPRISE_MERGE_REQUEST_COMMITS_URL,
                repo_project_id?,
                pull_request_iid,
            )
            .ok()?;
            let payload: Value = http::send_json(
                enterprise_authenticated_builder(http::get(endpoint))
                    .query(&[("with_page", "false")]),
                RequestOptions::new("Count Gitee enterprise pull request commits fallback")
                    .retry_transient(3),
            )
            .await
            .ok()?;
            complete_collection_total(&payload)
        }
        GiteeApiProfile::PublicV5 => bounded_list_total(&headers, &payload, per_page),
    }
}

/// Bounded comments/notes probe for the Conversation tab badge.
async fn count_pull_request_comments(
    locator: &RepoLocator,
    pull_request_iid: &str,
    repo_project_id: Option<&str>,
) -> Option<u32> {
    const PUBLIC_PROBE_PER_PAGE: u32 = 100;
    let profile = gitee_api_profile(locator);
    let endpoint = match profile {
        GiteeApiProfile::EnterpriseV8 => enterprise_pull_request_endpoint(
            endpoints::ENTERPRISE_MERGE_REQUEST_NOTES_URL,
            repo_project_id?,
            pull_request_iid,
        )
        .ok()?,
        GiteeApiProfile::PublicV5 => public_repo_endpoint(
            endpoints::PUBLIC_PULL_REQUEST_COMMENTS_URL,
            locator,
            Some(pull_request_iid),
            None,
        ),
    };
    let (request, per_page) = match profile {
        GiteeApiProfile::EnterpriseV8 => (
            enterprise_authenticated_builder(http::get(endpoint)).query(&[
                ("with_page", "true"),
                ("page", "1"),
                ("per_page", "1"),
            ]),
            1_u32,
        ),
        GiteeApiProfile::PublicV5 => (
            public_authenticated_request(endpoint).query(&[("page", "1"), ("per_page", "100")]),
            PUBLIC_PROBE_PER_PAGE,
        ),
    };
    let response = http::send(
        request,
        RequestOptions::new("Count Gitee pull request comments").retry_transient(3),
    )
    .await
    .ok()?;
    let headers = response.headers().clone();
    let payload: Value = response.json().await.ok()?;
    conversation_count_from_detail(&payload)
        .or_else(|| bounded_list_total(&headers, &payload, per_page))
}

pub(super) async fn submit_pull_request_review(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
    action: PullRequestReviewAction,
    body: &str,
) -> Result<(), String> {
    match gitee_api_profile(locator) {
        GiteeApiProfile::EnterpriseV8 => {
            if configured_gitee_token(GiteeApiProfile::EnterpriseV8).is_none() {
                return Err(
                    "Configure a Gitee Enterprise Token in Settings before submitting a review."
                        .to_string(),
                );
            }
            let endpoint = enterprise_pull_request_endpoint(
                if action == PullRequestReviewAction::Comment {
                    endpoints::ENTERPRISE_MERGE_REQUEST_NOTES_URL
                } else {
                    endpoints::ENTERPRISE_MERGE_REQUEST_REVIEWS_URL
                },
                repo_project_id,
                pull_request_iid,
            )?;
            if action != PullRequestReviewAction::Approve && body.trim().is_empty() {
                return Err("A review comment is required for this action.".into());
            }
            let payload = match action {
                // A Conversation comment is an MR note. `/reviews` represents
                // review decisions and returns 500 on some Enterprise servers
                // when it receives a plain `commented` review.
                PullRequestReviewAction::Comment => enterprise_note_payload(body),
                PullRequestReviewAction::Approve => enterprise_review_payload("approved", body),
                PullRequestReviewAction::RequestChanges => {
                    enterprise_review_payload("rejected", body)
                }
            };
            let request = enterprise_authenticated_builder(http::post(endpoint)).json(&payload);
            let operation = if action == PullRequestReviewAction::Comment {
                "Submit Gitee enterprise comment"
            } else {
                "Submit Gitee enterprise review"
            };
            http::send(request, RequestOptions::new(operation))
                .await
                .map_err(|error| error.to_string())?;
            Ok(())
        }
        GiteeApiProfile::PublicV5 => {
            if configured_gitee_token(GiteeApiProfile::PublicV5).is_none() {
                return Err(
                    "Configure a Gitee Public Token in Settings before submitting a comment."
                        .to_string(),
                );
            }
            if action != PullRequestReviewAction::Comment {
                return Err(
                    "Gitee public v5 supports pull request comments, but not approve or request-changes reviews."
                        .to_string(),
                );
            }
            if body.trim().is_empty() {
                return Err("A comment is required.".into());
            }
            let endpoint = public_repo_endpoint(
                endpoints::PUBLIC_PULL_REQUEST_COMMENTS_URL,
                locator,
                Some(pull_request_iid),
                None,
            );
            let request = public_authenticated_builder(http::post(endpoint))
                .json(&serde_json::json!({ "body": body.trim() }));
            http::send(
                request,
                RequestOptions::new("Submit Gitee public pull request comment"),
            )
            .await
            .map_err(|error| error.to_string())?;
            Ok(())
        }
    }
}

pub(super) async fn dismiss_pull_request_review(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
    reason: &str,
) -> Result<(), String> {
    if gitee_api_profile(locator) != GiteeApiProfile::EnterpriseV8 {
        return Err("Gitee public v5 does not support dismissing pull request reviews.".into());
    }
    if configured_gitee_token(GiteeApiProfile::EnterpriseV8).is_none() {
        return Err(
            "Configure a Gitee Enterprise Token in Settings before dismissing a review.".into(),
        );
    }
    if reason.trim().is_empty() {
        return Err("Enter a reason before dismissing the review.".into());
    }
    let payload = enterprise_review_dismissal_payload();
    let web_endpoint = enterprise_web_pull_request_review_endpoint(locator, pull_request_iid)?;
    let web_result = http::send(
        enterprise_authenticated_builder(http::post(web_endpoint)).json(&payload),
        RequestOptions::new("Dismiss Gitee enterprise pull request review"),
    )
    .await;

    if let Err(web_error) = web_result {
        log::warn!(
            "[CodeReview][Gitee] Web review dismissal failed; trying documented v8 endpoint: {web_error}"
        );
        let api_endpoint = enterprise_pull_request_endpoint(
            endpoints::ENTERPRISE_MERGE_REQUEST_REVIEWS_URL,
            repo_project_id,
            pull_request_iid,
        )?;
        http::send(
            enterprise_authenticated_builder(http::post(api_endpoint)).json(&payload),
            RequestOptions::new("Dismiss Gitee enterprise pull request review through v8"),
        )
        .await
        .map_err(|error| error.to_string())?;
    }

    // The working withdrawal request has no reason field. Preserve GitHub's
    // required reason as a normal Conversation note after the state change,
    // without making a successful withdrawal appear retryable if this
    // secondary write fails.
    let note_endpoint = enterprise_pull_request_endpoint(
        endpoints::ENTERPRISE_MERGE_REQUEST_NOTES_URL,
        repo_project_id,
        pull_request_iid,
    )?;
    if let Err(error) = http::send(
        enterprise_authenticated_builder(http::post(note_endpoint))
            .json(&enterprise_note_payload(reason)),
        RequestOptions::new("Add Gitee enterprise review dismissal reason"),
    )
    .await
    {
        log::warn!("[CodeReview][Gitee] Review dismissed but reason note failed: {error}");
    }
    Ok(())
}

fn enterprise_review_dismissal_payload() -> Value {
    serde_json::json!({ "state": "initialized" })
}

fn enterprise_review_payload(state: &str, body: &str) -> Value {
    let mut payload =
        serde_json::Map::from_iter([("state".to_string(), Value::String(state.to_string()))]);
    let body = body.trim();
    if !body.is_empty() {
        payload.insert("comment".to_string(), Value::String(body.to_string()));
    }
    Value::Object(payload)
}

fn enterprise_note_payload(body: &str) -> Value {
    // Enterprise v8 requires `noteable_type` even for a plain MR comment.
    // Keep the optional relation fields aligned with the documented ordinary
    // comment shape so the request cannot be mistaken for a code-line reply.
    serde_json::json!({
        "note": body.trim(),
        "noteable_type": "PullRequest",
        "parent_id": null,
        "reply_user_id": null,
        "reply_note_id": null,
        "commit_id": "",
    })
}

pub(super) async fn merge_pull_request(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
    method: PullRequestMergeMethod,
    remove_source_branch: bool,
) -> Result<(), String> {
    match gitee_api_profile(locator) {
        GiteeApiProfile::EnterpriseV8 => {
            if configured_gitee_token(GiteeApiProfile::EnterpriseV8).is_none() {
                return Err(
                    "Configure a Gitee Enterprise Token in Settings before merging.".to_string(),
                );
            }
            let endpoint = enterprise_pull_request_endpoint(
                endpoints::ENTERPRISE_MERGE_REQUEST_MERGE_URL,
                repo_project_id,
                pull_request_iid,
            )?;
            let request =
                enterprise_authenticated_builder(http::put(endpoint)).json(&serde_json::json!({
                    "merge_type": method.as_str(),
                    "should_remove_source_branch": remove_source_branch,
                }));
            http::send(
                request,
                RequestOptions::new("Merge Gitee enterprise pull request"),
            )
            .await
            .map_err(|error| error.to_string())?;
            Ok(())
        }
        GiteeApiProfile::PublicV5 => {
            if configured_gitee_token(GiteeApiProfile::PublicV5).is_none() {
                return Err(
                    "Configure a Gitee Public Token in Settings before merging.".to_string()
                );
            }
            if !matches!(
                method,
                PullRequestMergeMethod::Merge | PullRequestMergeMethod::Squash
            ) {
                return Err("Gitee public v5 supports merge and squash methods only.".to_string());
            }
            let endpoint = public_repo_endpoint(
                endpoints::PUBLIC_PULL_REQUEST_MERGE_URL,
                locator,
                Some(pull_request_iid),
                None,
            );
            let request =
                public_authenticated_builder(http::put(endpoint)).json(&serde_json::json!({
                    "merge_method": method.as_str(),
                    "prune_source_branch": remove_source_branch,
                }));
            http::send(
                request,
                RequestOptions::new("Merge Gitee public pull request"),
            )
            .await
            .map_err(|error| error.to_string())?;
            Ok(())
        }
    }
}

pub(super) async fn revert_pull_request(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
) -> Result<PullRequestRevertResult, String> {
    match gitee_api_profile(locator) {
        GiteeApiProfile::EnterpriseV8 => {
            if configured_gitee_token(GiteeApiProfile::EnterpriseV8).is_none() {
                return Err(
                    "Configure a Gitee Enterprise Token in Settings before reverting.".to_string(),
                );
            }
            let detail_endpoint = enterprise_pull_request_endpoint(
                endpoints::ENTERPRISE_MERGE_REQUEST_URL,
                repo_project_id,
                pull_request_iid,
            )?;
            let detail: Value = http::send_json(
                enterprise_pull_request_get(detail_endpoint),
                RequestOptions::new("Reload Gitee enterprise pull request before revert"),
            )
            .await
            .map_err(|error| error.to_string())?;
            let (commit_sha, target_branch) = enterprise_revert_context_from_detail(
                repo_project_id,
                pull_request_iid,
                enterprise_data_object(&detail),
            )
            .await?;
            let endpoint = enterprise_repository_commit_endpoint(
                endpoints::ENTERPRISE_REPOSITORY_COMMIT_REVERT_URL,
                repo_project_id,
                commit_sha.as_str(),
            )?;
            let response: Value = http::send_json(
                enterprise_authenticated_builder(http::post(endpoint))
                    .json(&serde_json::json!({ "branch_name": target_branch })),
                RequestOptions::new("Revert Gitee enterprise pull request merge commit"),
            )
            .await
            .map_err(|error| error.to_string())?;
            if response.get("code").and_then(Value::as_i64).unwrap_or(0) != 0 {
                return Err(string_field(&response, "message").unwrap_or_else(|| {
                    "Gitee Enterprise rejected the revert operation.".to_string()
                }));
            }
            Ok(PullRequestRevertResult {
                number: String::new(),
                title: format!("Revert {}", short_sha(commit_sha.as_str())),
                web_url: String::new(),
                message: format!(
                    "Reverted the merged changes on target branch {}.",
                    target_branch
                ),
            })
        }
        GiteeApiProfile::PublicV5 => {
            Err("Automated revert is not supported by Gitee Public v5.".to_string())
        }
    }
}

async fn enterprise_revert_context_from_detail(
    repo_project_id: &str,
    pull_request_iid: &str,
    detail: &Value,
) -> Result<(String, String), String> {
    if string_field(detail, "state").as_deref() != Some("merged") {
        return Err("Only a merged pull request can be reverted.".to_string());
    }
    let commit_sha = string_field(detail, "merge_commit_sha")
        .or_else(|| string_field(detail, "squash_commit_sha"))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "This merge has no dedicated merge or squash commit. Rebase and fast-forward merges cannot be safely reverted automatically."
                .to_string()
        })?;
    let target_branch = string_field(detail, "target_branch")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "Gitee Enterprise did not return the pull request target branch.".to_string()
        })?;
    let endpoint = enterprise_repository_commit_endpoint(
        endpoints::ENTERPRISE_REPOSITORY_COMMIT_REFS_URL,
        repo_project_id,
        commit_sha.as_str(),
    )?;
    let refs: Value = http::send_json(
        enterprise_authenticated_builder(http::get(endpoint)),
        RequestOptions::new("Inspect Gitee enterprise commit references for revert capability"),
    )
    .await
    .map_err(|error| error.to_string())?;
    if !enterprise_commit_refs_can_revert(enterprise_data_object(&refs), pull_request_iid) {
        return Err(
            "Gitee Enterprise did not mark this pull-request commit as safely revertible. Rebase and fast-forward merges remain unsupported."
                .to_string(),
        );
    }
    Ok((commit_sha, target_branch))
}

fn short_sha(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

async fn get_public_pull_requests(
    locator: &RepoLocator,
    state: Option<PullRequestState>,
    filters: &PullRequestFilters,
    page: u32,
    per_page: u32,
) -> Result<PullRequestPage, String> {
    if matches!(
        state,
        Some(PullRequestState::Merged | PullRequestState::ClosedUnmerged)
    ) || !filters.query.trim().is_empty()
    {
        return get_filtered_public_pull_requests(locator, state, filters, page, per_page).await;
    }
    get_public_pull_request_server_page(locator, state, filters, page, per_page).await
}

async fn get_filtered_public_pull_requests(
    locator: &RepoLocator,
    state: Option<PullRequestState>,
    filters: &PullRequestFilters,
    page: u32,
    per_page: u32,
) -> Result<PullRequestPage, String> {
    const SCAN_PAGE_SIZE: u32 = 100;
    const MAX_SCAN_PAGES: u32 = 30;

    let server_state = if matches!(
        state,
        Some(PullRequestState::Merged | PullRequestState::ClosedUnmerged)
    ) {
        Some(PullRequestState::Closed)
    } else {
        state
    };
    let mut matches = Vec::new();
    let mut exhausted = false;

    for server_page in 1..=MAX_SCAN_PAGES {
        let result = get_public_pull_request_server_page(
            locator,
            server_state,
            filters,
            server_page,
            SCAN_PAGE_SIZE,
        )
        .await?;
        let batch_len = result.items.len();
        matches.extend(result.items.into_iter().filter(|item| {
            (state != Some(PullRequestState::Merged) || item.state == "merged")
                && (state != Some(PullRequestState::ClosedUnmerged) || item.state == "closed")
                && matches_public_query(item, filters.query.as_str())
        }));

        let reached_provider_end = result
            .page_count
            .is_some_and(|page_count| server_page >= page_count)
            || batch_len < SCAN_PAGE_SIZE as usize;
        if reached_provider_end {
            exhausted = true;
            break;
        }
    }

    let per_page = per_page.max(1);
    let requested_page = page.max(1);
    let total = matches.len() as u32;
    let start = requested_page.saturating_sub(1).saturating_mul(per_page) as usize;
    let items = matches
        .into_iter()
        .skip(start)
        .take(per_page as usize)
        .collect::<Vec<_>>();
    let page_count = if exhausted {
        total.div_ceil(per_page).max(1)
    } else if items.len() == per_page as usize {
        requested_page.saturating_add(1)
    } else {
        requested_page.max(1)
    };
    Ok(PullRequestPage {
        items,
        total,
        total_exact: exhausted,
        page_count: Some(page_count),
    })
}

fn matches_public_query(item: &PullRequestItem, query: &str) -> bool {
    let haystack = format!(
        "{} {} {} {}",
        item.title, item.author, item.source_branch, item.target_branch
    )
    .to_ascii_lowercase();
    query
        .split_whitespace()
        .map(|term| term.trim_matches(['\'', '"']).to_ascii_lowercase())
        .filter(|term| !term.is_empty())
        .all(|term| haystack.contains(term.as_str()))
}

async fn get_public_pull_request_server_page(
    locator: &RepoLocator,
    state: Option<PullRequestState>,
    filters: &PullRequestFilters,
    page: u32,
    per_page: u32,
) -> Result<PullRequestPage, String> {
    let per_page = per_page.max(1);
    let endpoint = endpoints::PUBLIC_PULL_REQUESTS_URL
        .replace("{scheme}", locator.scheme.as_str())
        .replace("{authority}", locator.authority.as_str())
        .replace(
            "{namespace}",
            percent_encode_path(locator.namespace.as_str()).as_str(),
        )
        .replace(
            "{repository}",
            percent_encode_path(locator.repo.as_str()).as_str(),
        );
    let (sort, direction) = gitee_public_list_sort(filters.sort.as_str());
    let per_page_query = per_page.to_string();
    let page_query = page.max(1).to_string();
    let mut request = http::get(endpoint)
        .header(ACCEPT, "application/json")
        .query(&[
            ("per_page", per_page_query.as_str()),
            ("page", page_query.as_str()),
            ("sort", sort),
            ("direction", direction),
        ]);
    if let Some(state) = state {
        request = request.query(&[("state", state.gitee_public_query())]);
    }
    if let Some(token) = configured_gitee_token(GiteeApiProfile::PublicV5) {
        request = request.header("Authorization", format!("token {token}"));
    }
    decode_pull_request_page(request, page, per_page).await
}

fn gitee_enterprise_list_sort(value: &str) -> (&'static str, &'static str) {
    match value {
        "oldest" => ("created_at", "asc"),
        "updated-desc" => ("updated_at", "desc"),
        "updated-asc" => ("updated_at", "asc"),
        _ => ("created_at", "desc"),
    }
}

fn gitee_public_list_sort(value: &str) -> (&'static str, &'static str) {
    match value {
        "oldest" => ("created", "asc"),
        "updated-desc" => ("updated", "desc"),
        "updated-asc" => ("updated", "asc"),
        _ => ("created", "desc"),
    }
}

async fn decode_pull_request_page(
    request: reqwest::RequestBuilder,
    page: u32,
    per_page: u32,
) -> Result<PullRequestPage, String> {
    let response = http::send(
        request,
        RequestOptions::new("Load Gitee merge requests").retry_transient(3),
    )
    .await
    .map_err(|error| error.to_string())?;
    let headers = response.headers().clone();
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing")
        .to_string();
    let body = response
        .text()
        .await
        .map_err(|error| format!("Gitee merge request response read failed: {error}"))?;
    let body_len = body.len();
    let payload: Value = serde_json::from_str(body.as_str()).map_err(|error| {
        log::warn!(
            "[CodeReview][Gitee] Merge-request response was not JSON: content_type={} body_bytes={} parse_error={}",
            content_type,
            body_len,
            error,
        );
        "Gitee merge request response was not valid JSON.".to_string()
    })?;
    let total = pagination_total(&headers, &payload);
    let items = match pull_request_values(&payload) {
        Some(list) => list.iter().map(to_pull_request_item).collect::<Vec<_>>(),
        // Some Gitee-compatible deployments encode an empty page as a null or
        // omitted collection alongside an explicit zero total. Treat only that
        // unambiguous contract as empty; unexpected objects remain errors.
        None if total == Some(0) => Vec::new(),
        None => return Err("Gitee merge request response is not a list.".to_string()),
    };
    Ok(PullRequestPage {
        total: total.unwrap_or_else(|| fallback_total(page, per_page, items.len())),
        total_exact: total.is_some(),
        page_count: pagination_page_count(&headers, &payload),
        items,
    })
}

fn pull_request_values(payload: &Value) -> Option<&Vec<Value>> {
    if let Some(values) = payload.as_array() {
        return Some(values);
    }

    const COLLECTION_KEYS: [&str; 11] = [
        "items",
        "list",
        "content",
        "commits",
        "notes",
        "operate_logs",
        "operation_logs",
        "merge_requests",
        "pull_requests",
        "records",
        "events",
    ];
    if let Some(values) = payload.get("data").and_then(Value::as_array) {
        return Some(values);
    }
    COLLECTION_KEYS
        .into_iter()
        .find_map(|key| payload.get(key).and_then(Value::as_array))
        .or_else(|| {
            let data = payload.get("data")?;
            COLLECTION_KEYS
                .into_iter()
                .find_map(|key| data.get(key).and_then(Value::as_array))
        })
}

/// Different Gitee-compatible deployments expose the total either as an HTTP
/// header or alongside the list payload. Prefer exact totals over page length.
fn pagination_total(headers: &HeaderMap, payload: &Value) -> Option<u32> {
    ["x-total", "x-total-count", "total-count"]
        .into_iter()
        .find_map(|header| header_u32(headers, header))
        .or_else(|| value_u32(payload, "total"))
        .or_else(|| value_u32(payload, "total_count"))
        .or_else(|| value_u32(payload, "count"))
        .or_else(|| {
            payload
                .get("meta")
                .and_then(|meta| value_u32(meta, "total"))
        })
        .or_else(|| {
            payload
                .get("meta")
                .and_then(|meta| value_u32(meta, "total_count"))
        })
        .or_else(|| {
            payload
                .get("pagination")
                .and_then(|pagination| value_u32(pagination, "total"))
        })
        .or_else(|| {
            let data = payload.get("data")?;
            value_u32(data, "total")
                .or_else(|| value_u32(data, "total_count"))
                .or_else(|| value_u32(data, "count"))
                .or_else(|| data.get("meta").and_then(|meta| value_u32(meta, "total")))
                .or_else(|| {
                    data.get("meta")
                        .and_then(|meta| value_u32(meta, "total_count"))
                })
        })
}

/// Enterprise v8 may return a bare array even when pagination is enabled. A
/// page whose length exactly equals `per_page` is ambiguous without an explicit
/// total and must fall back to the documented non-paginated collection. A bare
/// array larger than the requested page proves pagination was ignored and is
/// therefore the complete collection; a short page is exact as usual.
fn enterprise_commit_probe_total(
    headers: &HeaderMap,
    payload: &Value,
    per_page: u32,
) -> Option<u32> {
    pagination_total(headers, payload)
        .or_else(|| {
            let len = u32::try_from(payload.as_array()?.len()).ok()?;
            (len != per_page).then_some(len)
        })
        .or_else(|| bounded_list_total(headers, payload, per_page))
}

/// Count a response that is contractually a complete, non-paginated collection.
fn complete_collection_total(payload: &Value) -> Option<u32> {
    let values = pull_request_values(payload)?;
    u32::try_from(values.len()).ok()
}

/// Prefer provider totals; otherwise a short page (`len < per_page`) is exact.
/// A full page without a total is ambiguous and returns `None`.
fn bounded_list_total(headers: &HeaderMap, payload: &Value, per_page: u32) -> Option<u32> {
    pagination_total(headers, payload).or_else(|| {
        let values = pull_request_values(payload)?;
        let len = u32::try_from(values.len()).ok()?;
        (len < per_page).then_some(len)
    })
}

fn pagination_page_count(headers: &HeaderMap, payload: &Value) -> Option<u32> {
    ["x-total-pages", "x-page-count"]
        .into_iter()
        .find_map(|header| header_u32(headers, header))
        .or_else(|| value_u32(payload, "total_pages"))
        .or_else(|| value_u32(payload, "totalPages"))
        .or_else(|| value_u32(payload, "pages"))
        .or_else(|| {
            payload
                .get("meta")
                .and_then(|meta| value_u32(meta, "total_pages"))
        })
        .or_else(|| {
            payload
                .get("pagination")
                .and_then(|pagination| value_u32(pagination, "total_pages"))
        })
}

fn header_u32(headers: &HeaderMap, name: &str) -> Option<u32> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u32>().ok())
}

fn value_u32(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| {
            value
                .get(key)
                .and_then(Value::as_str)
                .and_then(|value| value.parse::<u32>().ok())
        })
}

fn to_pull_request_item(value: &Value) -> PullRequestItem {
    PullRequestItem {
        number: string_field(value, "iid")
            .or_else(|| string_field(value, "number"))
            .unwrap_or_else(|| "—".to_string()),
        title: string_field(value, "title").unwrap_or_else(|| "Untitled pull request".to_string()),
        state: string_field(value, "state").unwrap_or_else(|| "open".to_string()),
        author: nested_string(value, "author", "name")
            .or_else(|| nested_string(value, "author", "username"))
            .or_else(|| nested_string(value, "user", "login"))
            .unwrap_or_else(|| "Unknown author".to_string()),
        source_branch: string_field(value, "source_branch")
            .or_else(|| nested_string(value, "head", "label"))
            .unwrap_or_else(|| "—".to_string()),
        target_branch: string_field(value, "target_branch")
            .or_else(|| nested_string(value, "base", "label"))
            .unwrap_or_else(|| "—".to_string()),
        review_status: gitee_list_review_status(value),
        conversation_count: conversation_count_from_detail(value),
        created_at: string_field(value, "created_at").unwrap_or_default(),
        updated_at: string_field(value, "updated_at")
            .unwrap_or_else(|| "Unknown update time".to_string()),
        web_url: string_field(value, "web_url")
            .or_else(|| string_field(value, "html_url"))
            .unwrap_or_default(),
    }
}

fn gitee_list_review_status(value: &Value) -> PullRequestReviewStatus {
    match string_field(value, "review_state")
        .or_else(|| string_field(value, "reviewState"))
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "review_approved" | "approved" => PullRequestReviewStatus::Approved,
        "review_rejected" | "rejected" => PullRequestReviewStatus::ChangesRequested,
        "review_pending" | "pending" => PullRequestReviewStatus::ReviewRequired,
        _ => PullRequestReviewStatus::None,
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

fn project_exactly_matches_remote(
    project: &Value,
    expected_path: &str,
    locator: &RepoLocator,
) -> bool {
    let matches_value = |key: &str, expected: &str| {
        project
            .get(key)
            .and_then(Value::as_str)
            .map(|value| value.trim_end_matches(".git") == expected)
            .unwrap_or(false)
    };

    matches_value("path_with_namespace", expected_path)
        || matches_value("ssh_url_to_repo", locator.remote_repo.as_str())
        || matches_value("http_url_to_repo", locator.remote_repo.as_str())
}

fn value_to_id(value: Option<&Value>, provider: &str) -> Result<String, String> {
    match value {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
        Some(Value::Number(value)) => Ok(value.to_string()),
        _ => Err(format!("{provider} project details did not include an id.")),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        IDENTITY_AVATAR_CACHE_CAPACITY, IdentityAvatarCache, bounded_list_total,
        commit_count_from_diff_payload, complete_collection_total, decode_commit,
        decode_conversation_comment, decode_conversation_review, decode_gitee_operate_log_activity,
        decode_pull_request_diff, enterprise_commit_probe_total, enterprise_commit_refs_can_revert,
        enterprise_compare_diff_payload, enterprise_data_object, enterprise_note_payload,
        enterprise_pull_request_get, enterprise_review_belongs_to_current_user,
        enterprise_review_dismissal_payload, enterprise_review_payload,
        enterprise_review_summaries, enterprise_user_has_distinct_display_name,
        enterprise_web_pull_request_review_endpoint, expand_enterprise_conversation_notes,
        gitee_enterprise_list_sort, gitee_enterprise_merge_readiness, gitee_enterprise_state_query,
        gitee_list_review_status, gitee_merge_actor_account, gitee_merge_event,
        gitee_merge_event_with_user, gitee_operate_log_commit_activity, gitee_public_list_sort,
        gitee_public_merge_readiness, gitee_push_event_commit_activity,
        merge_gitee_review_summaries, normalize_enterprise_commit_order, pull_request_values,
        select_enterprise_project_id, select_enterprise_user, trusted_enterprise_commit_count,
    };
    use crate::codereview::api::gitee as endpoints;
    use crate::git::{
        PullRequestReadinessKind, PullRequestReviewStatus, PullRequestState, parse_repo_locator,
    };
    use reqwest::header::HeaderMap;

    #[test]
    fn maps_gitee_list_review_states_to_github_statuses() {
        assert_eq!(
            gitee_list_review_status(&json!({ "review_state": "review_pending" })),
            PullRequestReviewStatus::ReviewRequired
        );
        assert_eq!(
            gitee_list_review_status(&json!({ "review_state": "review_approved" })),
            PullRequestReviewStatus::Approved
        );
        assert_eq!(
            gitee_list_review_status(&json!({ "review_state": "review_rejected" })),
            PullRequestReviewStatus::ChangesRequested
        );
    }

    #[test]
    fn identity_avatar_cache_is_bounded_and_remembers_negative_results() {
        let mut cache = IdentityAvatarCache::default();
        cache.insert("missing".to_string(), None);
        assert_eq!(cache.get("missing"), Some(None));
        for index in 0..=IDENTITY_AVATAR_CACHE_CAPACITY {
            cache.insert(format!("identity-{index}"), Some(format!("avatar-{index}")));
        }
        assert!(cache.entries.len() <= IDENTITY_AVATAR_CACHE_CAPACITY);
        assert_eq!(cache.get("identity-0"), None);
        assert_eq!(
            cache.get(format!("identity-{IDENTITY_AVATAR_CACHE_CAPACITY}").as_str()),
            Some(Some(format!("avatar-{IDENTITY_AVATAR_CACHE_CAPACITY}")))
        );
    }

    #[test]
    fn builds_enterprise_commit_batch_from_matching_source_branch_push_event() {
        let commits = vec![
            decode_commit(&json!({
                "id": "aaaaaaa111111111",
                "message": "initial",
                "author_name": "Initial Author",
                "created_at": "2026-01-01T01:00:00Z"
            })),
            decode_commit(&json!({
                "id": "bbbbbbb222222222",
                "message": "update",
                "author_name": "Commit Author",
                "created_at": "2026-01-02T01:00:00Z"
            })),
        ];
        let events = vec![json!({
            "id": 8,
            "created_at": "2026-01-02T02:00:00Z",
            "author": {
                "name": "Branch Updater",
                "avatar_url": "https://example.test/updater.png"
            },
            "push_data": {
                "ref": "refs/heads/feature/pr-1",
                "commit_count": 1,
                "commit_from": "aaaaaaa111111111",
                "commit_to": "bbbbbbb222222222"
            }
        })];

        let activity = gitee_push_event_commit_activity(
            &events,
            "feature/pr-1",
            "2026-01-01T00:00:00Z",
            &commits,
        );

        assert_eq!(activity.len(), 4);
        assert_eq!(activity[0].author, "Branch Updater");
        assert_eq!(activity[1].state, "bbbbbbb");
        assert_eq!(activity[2].author, "Initial Author");
        assert_eq!(activity[3].state, "aaaaaaa");
    }

    #[test]
    fn builds_public_commit_batch_from_chinese_push_log() {
        let commits = vec![decode_commit(&json!({
            "sha": "bbbbbbb222222222",
            "message": "update",
            "author": { "name": "Commit Author" },
            "created_at": "2026-01-02T01:00:00Z"
        }))];
        let logs = vec![json!({
            "id": 9,
            "created_at": "2026-01-02T02:00:00Z",
            "content": "推送了 1 个提交",
            "user": {
                "name": "Branch Updater",
                "avatar_url": "https://example.test/updater.png"
            }
        })];

        let activity = gitee_operate_log_commit_activity(&logs, &commits);

        assert_eq!(activity.len(), 2);
        assert_eq!(activity[0].author, "Branch Updater");
        assert_eq!(activity[0].body, "added 1 commit");
        assert_eq!(activity[1].state, "bbbbbbb");
    }

    #[test]
    fn enterprise_pull_request_reads_use_the_path_iid_without_a_global_pr_id() {
        let request = enterprise_pull_request_get(
            "https://gitee.example.com/api/v8/projects/3/merge_requests/11".to_string(),
        )
        .build()
        .expect("request");

        assert_eq!(
            request.url().as_str(),
            "https://gitee.example.com/api/v8/projects/3/merge_requests/11"
        );
    }

    #[test]
    fn maps_github_closed_semantics_to_gitee_enterprise_multi_state_query() {
        assert_eq!(
            gitee_enterprise_state_query(PullRequestState::Closed),
            vec![("states[]", "closed"), ("states[]", "merged")]
        );
        assert_eq!(
            gitee_enterprise_state_query(PullRequestState::ClosedUnmerged),
            vec![("state", "closed")]
        );
        assert_eq!(
            gitee_enterprise_state_query(PullRequestState::Merged),
            vec![("state", "merged")]
        );
    }

    #[test]
    fn chooses_the_exact_remote_match_from_a_search_result() {
        let locator = parse_repo_locator("git@gitee.example.com:platform/usher.git").unwrap();
        let payload = json!([
            { "id": 17, "name": "usher", "path_with_namespace": "other/usher" },
            { "id": 42, "name": "usher", "path_with_namespace": "platform/usher" }
        ]);

        assert_eq!(
            select_enterprise_project_id(&payload, &locator).unwrap(),
            "42"
        );
    }

    #[test]
    fn maps_every_pr_list_sort_to_each_gitee_api_profile() {
        assert_eq!(gitee_enterprise_list_sort("newest"), ("created_at", "desc"));
        assert_eq!(gitee_enterprise_list_sort("oldest"), ("created_at", "asc"));
        assert_eq!(
            gitee_enterprise_list_sort("updated-desc"),
            ("updated_at", "desc")
        );
        assert_eq!(
            gitee_enterprise_list_sort("updated-asc"),
            ("updated_at", "asc")
        );
        assert_eq!(gitee_public_list_sort("newest"), ("created", "desc"));
        assert_eq!(gitee_public_list_sort("oldest"), ("created", "asc"));
        assert_eq!(gitee_public_list_sort("updated-desc"), ("updated", "desc"));
        assert_eq!(gitee_public_list_sort("updated-asc"), ("updated", "asc"));
    }

    #[test]
    fn enterprise_commit_ranges_use_the_documented_compare_diffs_contract() {
        assert_eq!(
            endpoints::ENTERPRISE_REPOSITORY_COMPARE_DIFFS_URL,
            "{api_base_url}/projects/{project_id}/repository/compare/diffs"
        );
        assert_eq!(
            enterprise_compare_diff_payload(" base-sha ", " head-sha ", true),
            json!({
                "from": "head-sha",
                "to": "base-sha",
                "paths": [],
                "view_mode": "",
                "ignore_whitespace_change": true,
            })
        );
    }

    #[test]
    fn normalizes_newest_first_enterprise_commits_for_range_selection() {
        let mut commits = vec![
            decode_commit(&json!({
                "id": "newest",
                "created_at": "2026-07-23T10:00:00+08:00"
            })),
            decode_commit(&json!({
                "id": "middle",
                "created_at": "2026-07-23T09:00:00+08:00"
            })),
            decode_commit(&json!({
                "id": "oldest",
                "created_at": "2026-07-23T08:00:00+08:00"
            })),
        ];

        normalize_enterprise_commit_order(&mut commits);

        assert_eq!(
            commits
                .iter()
                .map(|commit| commit.sha.as_str())
                .collect::<Vec<_>>(),
            vec!["oldest", "middle", "newest"]
        );
    }

    #[test]
    fn trusts_only_server_marked_pull_request_refs_for_enterprise_revert() {
        assert!(enterprise_commit_refs_can_revert(
            &json!({
                "merge_commit_by_pr": true,
                "can_revert": true,
                "pull_request": { "iid": 21 }
            }),
            "21"
        ));
        assert!(!enterprise_commit_refs_can_revert(
            &json!({
                "merge_commit_by_pr": true,
                "can_revert": true,
                "pull_request": { "iid": 22 }
            }),
            "21"
        ));
        assert!(!enterprise_commit_refs_can_revert(
            &json!({
                "merge_commit_by_pr": false,
                "can_revert": true
            }),
            "21"
        ));
        assert!(!enterprise_commit_refs_can_revert(
            &json!({
                "merge_commit_by_pr": true,
                "can_revert": false
            }),
            "21"
        ));
        assert!(enterprise_commit_refs_can_revert(
            &json!({
                "merge_commit_by_pr": true,
                "can_revert": true
            }),
            "21"
        ));
    }

    #[test]
    fn unwraps_enterprise_data_objects_before_capability_checks() {
        let payload = json!({
            "code": 0,
            "data": {
                "state": "merged",
                "merge_commit_sha": "abcdef123456"
            }
        });
        let detail = enterprise_data_object(&payload);
        assert_eq!(detail["state"], "merged");
        assert_eq!(detail["merge_commit_sha"], "abcdef123456");
    }

    #[test]
    fn enterprise_review_payloads_match_the_documented_contract() {
        assert_eq!(
            enterprise_review_payload("approved", "  "),
            json!({ "state": "approved" })
        );
        assert_eq!(
            enterprise_review_payload("approved", " Looks good. "),
            json!({ "state": "approved", "comment": "Looks good." })
        );
        assert_eq!(
            enterprise_review_dismissal_payload(),
            json!({ "state": "initialized" })
        );
    }

    #[test]
    fn enterprise_review_dismissal_is_scoped_to_the_token_user() {
        let detail = json!({
            "general_reviewers": [
                { "id": 15, "username": "alice", "name": "Alice", "approve_status": "approvedAndCommented" },
                { "id": 16, "username": "bob", "name": "Bob", "approve_status": "initialized" }
            ],
            "primary_reviewers": [
                { "id": 15, "username": "alice", "name": "Alice", "approve_status": "approved" },
                { "id": 17, "user_name": "carol", "name": "Carol", "approve_status": "rejected" }
            ]
        });
        let current_user = json!({
            "data": { "id": 15, "username": "alice" }
        });
        let summaries = enterprise_review_summaries(&detail, Some(&current_user));

        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].author, "Alice");
        assert_eq!(summaries[0].state, "approved");
        assert_eq!(summaries[0].review_id, "15");
        assert!(summaries[0].can_dismiss);
        assert_eq!(summaries[1].author, "Carol");
        assert_eq!(summaries[1].state, "rejected");
        assert!(!summaries[1].can_dismiss);
        assert!(enterprise_review_belongs_to_current_user(
            &detail["general_reviewers"][0],
            &current_user
        ));
        assert!(!enterprise_review_belongs_to_current_user(
            &detail["general_reviewers"][0],
            &json!({ "id": 16, "username": "bob" })
        ));
    }

    #[test]
    fn enterprise_documented_reviewer_shape_uses_name_and_allow_merge_state() {
        let detail = json!({
            "general_reviewers": [
                {
                    "id": 15,
                    "uuid": "review-owner-uuid",
                    "name": "Review Owner",
                    "user_name": "review-owner",
                    "email": "review-owner@example.com",
                    "avatar_url": "https://gitee.example/review-owner.png",
                    "approve_status": "approved"
                }
            ],
            "primary_reviewers": []
        });

        let summaries = enterprise_review_summaries(&detail, None);

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].kind, "review_summary");
        assert_eq!(summaries[0].author, "Review Owner");
        assert_eq!(summaries[0].state, "approved");
        assert_eq!(summaries[0].body, "approved these changes");
        assert!(!summaries[0].author.contains('@'));
    }

    #[test]
    fn decodes_gitee_reviewer_assignment_and_review_operation_logs() {
        let logs = vec![
            json!({
                "id": 101,
                "user": {
                    "name": "PR author",
                    "avatar_url": "https://gitee.example/author.png"
                },
                "target": { "name": "Alice Reviewer" },
                "content": "指派了 <a>Alice Reviewer</a> 参与评审",
                "created_at": "2026-07-27T10:31:57+08:00",
                "action_type": "set_assignee"
            }),
            json!({
                "id": 102,
                "user": {
                    "name": "Alice Reviewer",
                    "avatar_url": "https://gitee.example/alice.png"
                },
                "content": "审查通过",
                "created_at": "2026-07-27T10:35:00+08:00",
                "action_type": "accepted_pr"
            }),
            json!({
                "id": 103,
                "user": { "name": "Bob Reviewer" },
                "content": "requested changes",
                "createdAt": "2026-07-27T10:36:00+08:00",
                "actionType": "review_rejected"
            }),
        ];

        let activity = decode_gitee_operate_log_activity(&logs);

        assert_eq!(activity.len(), 3);
        assert_eq!(activity[0].kind, "review_requested");
        assert_eq!(activity[0].author, "PR author");
        assert_eq!(activity[0].body, "requested a review from");
        assert_eq!(activity[0].state, "Alice Reviewer");
        assert_eq!(activity[1].kind, "review_summary");
        assert_eq!(activity[1].author, "Alice Reviewer");
        assert_eq!(activity[1].state, "approved");
        assert_eq!(activity[1].body, "approved these changes");
        assert_eq!(activity[1].avatar_url, "https://gitee.example/alice.png");
        assert_eq!(activity[2].state, "rejected");
        assert_eq!(activity[2].body, "requested changes");
    }

    #[test]
    fn decodes_gitee_allow_merge_log_as_a_github_style_approval() {
        let activity = decode_gitee_operate_log_activity(&[json!({
            "id": 104,
            "user": {
                "name": "reviewer@example.com",
                "remark": "Review Owner"
            },
            "title": "允许合并 Pull Request",
            "created_at": "2026-07-27T10:37:00+08:00",
            "operating": "allow_merge_pr"
        })]);

        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0].kind, "review_summary");
        assert_eq!(activity[0].author, "Review Owner");
        assert_eq!(activity[0].state, "approved");
        assert_eq!(activity[0].body, "approved these changes");
    }

    #[test]
    fn decodes_structured_enterprise_review_activity_as_a_github_style_approval() {
        let payload = json!({
            "data": [{
                "id": 1867793,
                "action": "review_pr",
                "desc": "新增了评审意见",
                "created_at": "2026-08-17T14:49:59.779767+08:00",
                "data": { "state": "approved" },
                "user": {
                    "name": "时津津",
                    "avatar_url": "https://gitee.example/shijinjin.png"
                }
            }]
        });

        let activity = decode_gitee_operate_log_activity(
            pull_request_values(&payload).expect("enterprise activity collection"),
        );

        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0].kind, "review_summary");
        assert_eq!(activity[0].author, "时津津");
        assert_eq!(activity[0].state, "approved");
        assert_eq!(activity[0].body, "approved these changes");
        assert_eq!(activity[0].created_at, "2026-08-17T14:49:59.779767+08:00");
    }

    #[test]
    fn unwraps_nested_gitee_operation_log_collections() {
        let payload = json!({
            "data": {
                "operate_logs": [
                    { "id": 104, "content": "允许合并 Pull Request" }
                ]
            }
        });

        let values = pull_request_values(&payload).expect("operation logs");

        assert_eq!(values.len(), 1);
        assert_eq!(values[0]["id"], 104);
    }

    #[test]
    fn decodes_structured_gitee_approval_when_log_text_is_generic() {
        let activity = decode_gitee_operate_log_activity(&[json!({
            "id": 105,
            "operator": { "display_name": "Review Owner" },
            "content": "更新了 Pull Request",
            "created_at": "2026-07-27T10:38:00+08:00",
            "action_type": "update_pull_request",
            "review_state": "review_approved"
        })]);

        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0].author, "Review Owner");
        assert_eq!(activity[0].state, "approved");
    }

    #[test]
    fn gitee_merge_actor_prefers_operation_log_then_dedicated_detail_identity() {
        let detail = json!({
            "user": { "name": "PR author" },
            "merged_by": { "name": "Detail merger" }
        });
        let logs = vec![json!({
            "id": 201,
            "user": {
                "name": "Actual merger",
                "avatar_url": "https://gitee.example/merger.png"
            },
            "content": "合并了Pull Request",
            "created_at": "2026-07-27T10:40:00+08:00",
            "action_type": "merged_pr"
        })];

        let from_log = gitee_merge_event(&detail, Some(&logs), None);
        assert_eq!(from_log.author, "Actual merger");
        assert_eq!(from_log.avatar_url, "https://gitee.example/merger.png");
        assert_eq!(from_log.created_at, "2026-07-27T10:40:00+08:00");

        let from_detail = gitee_merge_event(
            &json!({
                "user": { "name": "PR author" },
                "merge_user": { "username": "dedicated-merger" }
            }),
            None,
            None,
        );
        assert_eq!(from_detail.author, "dedicated-merger");
    }

    #[test]
    fn gitee_merge_actor_reads_documented_string_merged_by() {
        let event = gitee_merge_event(
            &json!({
                "author": { "name": "PR author" },
                "merged_by": "Merge Owner"
            }),
            None,
            None,
        );

        assert_eq!(event.author, "Merge Owner");
    }

    #[test]
    fn gitee_merge_actor_uses_detail_display_name_instead_of_log_email() {
        let detail = json!({
            "merged_by": {
                "name": "Merge Owner",
                "username": "merge-owner@example.com"
            }
        });
        let logs = vec![json!({
            "id": 202,
            "user": { "name": "merge-owner@example.com" },
            "content": "合并了Pull Request",
            "created_at": "2026-07-27T10:40:00+08:00",
            "action_type": "merged_pr"
        })];

        let event = gitee_merge_event(&detail, Some(&logs), None);

        assert_eq!(event.author, "Merge Owner");
        assert_eq!(event.created_at, "2026-07-27T10:40:00+08:00");
    }

    #[test]
    fn gitee_merge_actor_never_displays_a_full_email_address() {
        let logs = vec![json!({
            "id": 203,
            "user": { "name": "merge-owner@example.com" },
            "content": "合并了Pull Request",
            "created_at": "2026-07-27T10:40:00+08:00",
            "action_type": "merged_pr"
        })];

        let event = gitee_merge_event(&json!({}), Some(&logs), None);

        assert_eq!(event.author, "merge-owner");
    }

    #[test]
    fn gitee_merge_actor_never_falls_back_to_pull_request_author() {
        let event = gitee_merge_event(
            &json!({
                "user": { "name": "PR author" },
                "author": { "name": "PR author" }
            }),
            None,
            None,
        );

        assert_eq!(event.author, "Unknown merger");
    }

    #[test]
    fn gitee_merge_actor_uses_commit_name_when_detail_only_has_an_email_account() {
        let event = gitee_merge_event(
            &json!({ "merged_by": "chenxuegen@example.com" }),
            None,
            Some(&json!({
                "committer_name": "陈雪根",
                "committer_email": "chenxuegen@example.com"
            })),
        );

        assert_eq!(event.author, "陈雪根");
    }

    #[test]
    fn gitee_merge_actor_account_uses_the_recorded_merger_instead_of_the_pr_author() {
        assert_eq!(
            gitee_merge_actor_account(
                &json!({
                    "author": { "username": "pr-author" },
                    "merged_by": "chenxuegen@example.com"
                }),
                None,
            )
            .as_deref(),
            Some("chenxuegen")
        );
    }

    #[test]
    fn enterprise_member_directory_resolves_a_merger_account_to_its_display_name() {
        let payload = json!({
            "data": [
                {
                    "username": "chenxuegen-old",
                    "name": "Wrong User"
                },
                {
                    "username": "chenxuegen",
                    "name": "陈雪根",
                    "avatar_url": "https://gitee.example/chenxuegen.png"
                }
            ]
        });
        let user = select_enterprise_user(&payload, "chenxuegen").expect("exact member");
        let event = gitee_merge_event_with_user(
            &json!({ "merged_by": "chenxuegen" }),
            None,
            Some(&json!({ "committer_name": "chenxuegen" })),
            Some(user),
        );

        assert_eq!(event.author, "陈雪根");
        assert_eq!(event.avatar_url, "https://gitee.example/chenxuegen.png");
    }

    #[test]
    fn enterprise_member_directory_never_accepts_a_fuzzy_only_match() {
        let payload = json!([
            { "username": "chenxuegen-old", "name": "Wrong User" },
            { "username": "other", "name": "chenxuegen-old" }
        ]);

        assert!(select_enterprise_user(&payload, "chenxuegen").is_none());
    }

    #[test]
    fn enterprise_member_account_label_does_not_override_a_better_commit_name() {
        let account_only = json!({
            "username": "chenxuegen",
            "name": "chenxuegen",
            "avatar_url": "https://gitee.example/account.png"
        });
        let display_name = json!({
            "username": "chenxuegen",
            "name": "陈雪根"
        });

        assert!(!enterprise_user_has_distinct_display_name(
            &account_only,
            "chenxuegen"
        ));
        assert!(enterprise_user_has_distinct_display_name(
            &display_name,
            "chenxuegen"
        ));
    }

    #[test]
    fn gitee_merge_actor_keeps_dedicated_display_name_over_commit_fallback() {
        let event = gitee_merge_event(
            &json!({ "merged_by": { "name": "Merge Owner" } }),
            None,
            Some(&json!({ "committer_name": "Committer Name" })),
        );

        assert_eq!(event.author, "Merge Owner");
    }

    #[test]
    fn operation_log_review_keeps_active_enterprise_dismissal_capability() {
        let mut activity = decode_gitee_operate_log_activity(&[json!({
            "id": 301,
            "user": { "name": "Alice", "username": "alice" },
            "content": "审查通过",
            "created_at": "2026-07-27T10:35:00+08:00",
            "action_type": "approved_pr"
        })]);
        let detail = json!({
            "general_reviewers": [
                { "id": 15, "username": "alice", "name": "Alice", "approve_status": "approved" }
            ]
        });
        let current_user = json!({ "data": { "id": 15, "username": "alice" } });

        merge_gitee_review_summaries(
            &mut activity,
            enterprise_review_summaries(&detail, Some(&current_user)),
        );

        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0].created_at, "2026-07-27T10:35:00+08:00");
        assert_eq!(activity[0].review_id, "15");
        assert!(activity[0].can_dismiss);
    }

    #[test]
    fn enterprise_web_review_endpoint_preserves_nested_repository_path() {
        let locator = parse_repo_locator(
            "git@gitee.example.com:picc/picc__wechat/enterprise-insurance-management.git",
        )
        .unwrap();
        assert_eq!(
            enterprise_web_pull_request_review_endpoint(&locator, "1746").unwrap(),
            "https://gitee.example.com/api/code/api/picc/picc__wechat/enterprise-insurance-management/-/pull_requests/1746/reviews"
        );
    }

    #[test]
    fn enterprise_conversation_comments_use_note_payloads() {
        assert_eq!(
            enterprise_note_payload("  hello  "),
            json!({
                "note": "hello",
                "noteable_type": "PullRequest",
                "parent_id": null,
                "reply_user_id": null,
                "reply_note_id": null,
                "commit_id": "",
            })
        );
    }

    #[test]
    fn accepts_common_gitee_list_wrappers() {
        let direct = json!([{ "number": 1 }]);
        let data = json!({ "data": [{ "number": 2 }] });
        let nested = json!({ "data": { "list": [{ "number": 3 }] } });
        let records = json!({ "records": [] });

        assert_eq!(pull_request_values(&direct).unwrap().len(), 1);
        assert_eq!(pull_request_values(&data).unwrap().len(), 1);
        assert_eq!(pull_request_values(&nested).unwrap().len(), 1);
        assert!(pull_request_values(&records).unwrap().is_empty());
        assert!(pull_request_values(&json!({ "message": "forbidden" })).is_none());
    }

    #[test]
    fn decodes_public_and_enterprise_commit_shapes() {
        let public = decode_commit(&json!({
            "sha": "public-sha",
            "author": {
                "login": "public-author",
                "avatar_url": "https://gitee.example.test/public.png"
            },
            "committer": {
                "login": "public-committer",
                "avatar_url": "https://gitee.example.test/committer.png"
            },
            "commit": {
                "message": "Public title\nbody",
                "author": { "name": "Public Author", "date": "2026-07-20T01:00:00Z" },
                "committer": { "name": "Public Committer", "date": "2026-07-20T01:01:00Z" }
            }
        }));
        let enterprise = decode_commit(&json!({
            "id": "enterprise-sha",
            "title": "Enterprise title",
            "message": "Enterprise title\nbody",
            "author_name": "Enterprise Author",
            "author": {
                "name": "Enterprise Author",
                "avatar": "https://gitee.example.test/enterprise.png"
            },
            "authored_date": "2026-07-20T02:00:00Z"
        }));
        assert_eq!(public.title, "Public title");
        assert_eq!(public.author, "public-author");
        assert_eq!(public.avatar_url, "https://gitee.example.test/public.png");
        assert_eq!(public.committer, "public-committer");
        assert_eq!(
            public.committer_avatar_url,
            "https://gitee.example.test/committer.png"
        );
        assert_eq!(enterprise.sha, "enterprise-sha");
        assert_eq!(enterprise.author, "Enterprise Author");
        assert_eq!(
            enterprise.avatar_url,
            "https://gitee.example.test/enterprise.png"
        );
        assert_eq!(enterprise.authored_at, "2026-07-20T02:00:00Z");

        // Internal enterprise MR commits document created_at instead of authored_date.
        let mr_commit = decode_commit(&json!({
            "id": "ed899a2f4b50b4370feeea94676502b42383c746",
            "short_id": "ed899a2f4b5",
            "title": "Replace sanitize with escape once",
            "author_name": "Example User",
            "author_email": "user@example.com",
            "created_at": "2012-09-20T11:50:22+03:00",
            "message": "Replace sanitize with escape once"
        }));
        assert_eq!(mr_commit.authored_at, "2012-09-20T11:50:22+03:00");
    }

    #[test]
    fn expands_enterprise_nested_notes_without_using_content_object() {
        // Shape from internal enterprise GET /notes: thread under data with
        // nested notes[].note; top-level content is diff context only.
        let expanded = expand_enterprise_conversation_notes(&[json!({
            "id": 1956,
            "line_code": "abc_21_21",
            "author": { "name": "Thread Author", "username": "thread" },
            "content": {
                "code": 0,
                "text": [{ "line_content": "<div>diff</div>", "type": "match" }]
            },
            "notes": [
                {
                    "id": 1956,
                    "author": { "name": "Reviewer A", "username": "reviewer-a" },
                    "note": "请先解决冲突",
                    "created_at": "2026-07-20T03:00:00+08:00",
                    "children": [
                        {
                            "id": 1957,
                            "author": { "name": "Reviewer B", "username": "reviewer-b" },
                            "note": "pr有冲突",
                            "created_at": "2026-07-20T03:05:00+08:00"
                        }
                    ]
                }
            ]
        })]);
        assert_eq!(expanded.len(), 2);
        assert_eq!(expanded[0].body, "请先解决冲突");
        assert_eq!(expanded[0].author, "Reviewer A");
        assert_eq!(expanded[1].body, "pr有冲突");
        assert_eq!(expanded[1].author, "Reviewer B");

        let theme = decode_conversation_comment(&json!({
            "id": 11,
            "author": { "name": "Theme Author", "username": "theme" },
            "note": "主题评论",
            "created_at": "2026-07-20T03:06:00+08:00"
        }));
        assert_eq!(theme.kind, "comment");
        assert_eq!(theme.body, "主题评论");

        // Official enterprise ordinary notes store markdown in string `content`.
        let ordinary = decode_conversation_comment(&json!({
            "id": 12,
            "author": { "name": "Ordinary Author", "username": "ordinary" },
            "content": "请先解决冲突",
            "createdAt": "2026-07-20T03:07:00+08:00"
        }));
        assert_eq!(ordinary.kind, "comment");
        assert_eq!(ordinary.body, "请先解决冲突");
        assert_eq!(ordinary.created_at, "2026-07-20T03:07:00+08:00");

        // Diff-context `content` objects must never become comment bodies.
        let diff_context = decode_conversation_comment(&json!({
            "id": 14,
            "author": { "name": "Diff Author", "username": "diff" },
            "content": {
                "code": 0,
                "text": [{ "line_content": "<div>diff</div>", "type": "match" }]
            },
            "created_at": "2026-07-20T03:07:30+08:00"
        }));
        assert_eq!(diff_context.body, "");

        // Public v5 flat comments keep body decoding.
        let public = decode_conversation_comment(&json!({
            "id": 13,
            "user": { "login": "public-user" },
            "body": "public comment body",
            "created_at": "2026-07-20T03:08:00+08:00"
        }));
        assert_eq!(public.kind, "comment");
        assert_eq!(public.body, "public comment body");
        assert_eq!(public.author, "public-user");

        let review = decode_conversation_review(&json!({
            "id": 99,
            "user": { "name": "Reviewer", "username": "reviewer" },
            "note": { "note": "pr有冲突" },
            "state": "rejected",
            "created_at": "2026-07-20T03:09:00+08:00"
        }))
        .expect("review comment");
        assert_eq!(review.kind, "review");
        assert_eq!(review.body, "pr有冲突");
        assert_eq!(review.state, "rejected");

        // Theme comments expose the body on the data item itself.
        let theme_flat = decode_conversation_comment(&json!({
            "id": 15,
            "author": { "name": "Theme Author", "username": "theme" },
            "note": "主题评论正文",
            "created_at": "2026-07-20T03:10:00+08:00"
        }));
        assert_eq!(theme_flat.body, "主题评论正文");
    }

    #[test]
    fn decodes_enterprise_diff_lines_and_strips_html() {
        let payload = json!({
            "added_lines": 1,
            "removed_lines": 1,
            "commits_size": 1,
            "count": 3,
            "commits": [{ "id": "abc" }],
            "diff_refs": { "base_sha": "base", "head_sha": "head", "start_sha": "start" },
            "diffs": [{
                "added_lines": 1,
                "removed_lines": 1,
                "statistic": {
                    "new_path": "src/lib.rs",
                    "old_path": "src/lib.rs",
                    "type": "edit_file",
                    "viewed": false
                },
                "content": { "text": [
                    {
                        "line_content": "<div><span>let old = true;</span></div>",
                        "old_line": { "line_num": 7 },
                        "new_line": { "line_num": 0 },
                        "type": "old"
                    },
                    {
                        "line_content": "<div><span>let new = true &amp;&amp; ready;</span></div>",
                        "old_line": { "line_num": 0 },
                        "new_line": { "line_num": "7" },
                        "type": "new"
                    }
                ] }
            }]
        });

        let detail = decode_pull_request_diff(&payload).unwrap();
        assert_eq!(detail.files.len(), 1);
        assert_eq!(detail.files[0].lines[0].kind, "deletion");
        assert_eq!(detail.files[0].lines[1].new_line, 7);
        assert_eq!(detail.files[0].lines[1].content, "let new = true && ready;");
        assert_eq!(detail.head_sha, "head");
        // Prefer commits_size; ignore file-total `count` and commits array.
        assert_eq!(detail.commit_count, 1);
        assert_eq!(commit_count_from_diff_payload(&payload), Some(1));
        assert_eq!(
            commit_count_from_diff_payload(&json!({
                "too_more_commits": false,
                "commits": [{ "id": "one" }, { "id": "two" }]
            })),
            Some(2)
        );
        assert_eq!(
            commit_count_from_diff_payload(&json!({ "count": 9, "commits": [] })),
            None
        );
        assert_eq!(
            commit_count_from_diff_payload(&json!({
                "too_more_commits": true,
                "commits": [{ "id": "preview-only" }]
            })),
            None
        );
    }

    #[test]
    fn bounded_list_total_prefers_meta_and_short_pages() {
        let headers = HeaderMap::new();
        let meta = json!({
            "data": [{ "id": 1 }],
            "meta": { "current_page": 1, "total_pages": 3, "total_count": 3 }
        });
        assert_eq!(bounded_list_total(&headers, &meta, 1), Some(3));

        let short = json!([{ "id": 1 }]);
        assert_eq!(bounded_list_total(&headers, &short, 100), Some(1));
        assert_eq!(bounded_list_total(&headers, &json!([]), 100), Some(0));

        let full_without_total = json!([{ "id": 1 }]);
        assert_eq!(bounded_list_total(&headers, &full_without_total, 1), None);
    }

    #[test]
    fn enterprise_commit_probe_distinguishes_full_pages_from_complete_arrays() {
        let headers = HeaderMap::new();
        // `with_page=true&per_page=1` may still return a bare one-item page.
        // Without a total, this must trigger the non-paginated fallback.
        assert_eq!(
            enterprise_commit_probe_total(&headers, &json!([{ "id": "one" }]), 1),
            None
        );
        assert_eq!(
            enterprise_commit_probe_total(
                &headers,
                &json!([{ "id": "one" }, { "id": "two" }, { "id": "three" }]),
                1,
            ),
            Some(3)
        );
        assert_eq!(
            enterprise_commit_probe_total(&headers, &json!([]), 1),
            Some(0)
        );
        assert_eq!(
            enterprise_commit_probe_total(
                &headers,
                &json!({
                    "data": {
                        "list": [{ "id": "one" }],
                        "meta": { "total_count": 4 }
                    }
                }),
                1,
            ),
            Some(4)
        );
    }

    #[test]
    fn enterprise_zero_commit_summaries_require_a_probe() {
        let detail = json!({ "commits_count": 0 });
        let diff = json!({ "commits_size": 0 });
        assert_eq!(trusted_enterprise_commit_count(Some(&detail), &diff), None);

        let diff = json!({ "commits_size": 1 });
        assert_eq!(
            trusted_enterprise_commit_count(Some(&detail), &diff),
            Some(1)
        );

        let detail = json!({ "commits_count": 2 });
        assert_eq!(
            trusted_enterprise_commit_count(Some(&detail), &diff),
            Some(2)
        );
    }

    #[test]
    fn complete_enterprise_commit_fallback_accepts_wrapped_collections() {
        assert_eq!(
            complete_collection_total(&json!({
                "data": { "commits": [{ "id": "one" }, { "id": "two" }] }
            })),
            Some(2)
        );
    }

    #[test]
    fn decodes_named_decimal_and_hex_html_entities_in_source_lines() {
        assert_eq!(
            super::html_line_to_text(
                "<span>&lt;div class=&#34;card&#34; data-label=&#x27;A&amp;B&#x27;&gt;</span>"
            ),
            "<div class=\"card\" data-label='A&B'>"
        );
    }

    #[test]
    fn normalizes_pretty_printed_diff_html_to_one_visual_line() {
        let line = super::html_diff_line_to_text(
            "<span>const value = </span>\r\n<span>&#34;中文&#34;;</span>\n",
        );

        assert_eq!(line, "const value = \"中文\";");
        assert!(!line.contains(['\r', '\n']));
    }

    #[test]
    fn enterprise_true_conflict_result_means_no_conflict() {
        let current = json!({
            "state": "opened",
            "conflict_result": true,
            "scan_quality_state": true,
            "strategy_result": true,
            "can_merge": true
        });
        let readiness = gitee_enterprise_merge_readiness(&current, &json!({}), "review_approved");

        assert_eq!(readiness.kind, PullRequestReadinessKind::Ready);
    }

    #[test]
    fn enterprise_false_conflict_result_means_conflicting() {
        let current = json!({
            "state": "opened",
            "conflict_result": false,
            "can_merge": false
        });
        let readiness = gitee_enterprise_merge_readiness(&current, &json!({}), "review_approved");

        assert_eq!(readiness.kind, PullRequestReadinessKind::Conflicts);
    }

    #[test]
    fn enterprise_unresolved_required_notes_block_merge() {
        let current = json!({
            "state": "opened",
            "conflict_result": true,
            "note": {
                "resolve_note_enabled": true,
                "resolve": 1,
                "total": 2
            },
            "can_merge": false
        });
        let readiness = gitee_enterprise_merge_readiness(&current, &json!({}), "review_approved");

        assert_eq!(
            readiness.kind,
            PullRequestReadinessKind::DiscussionsUnresolved
        );
    }

    #[test]
    fn public_non_mergeable_state_is_blocked_not_conflicting() {
        let readiness = gitee_public_merge_readiness(&json!({
            "state": "open",
            "mergeable": false
        }));

        assert_eq!(readiness.kind, PullRequestReadinessKind::Blocked);
    }
}
