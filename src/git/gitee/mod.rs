//! Gitee-compatible repository operations.
//!
//! `project_name` is always the repository name extracted from `remote_repo`
//! (the final path segment before `.git`), never the editable project alias.

use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap};
use serde_json::Value;

use crate::codereview::api::gitee as endpoints;
use crate::http::{self, RequestOptions};

use super::{
    GiteeApiProfile, PullRequestActionState, PullRequestCommit, PullRequestConversationItem,
    PullRequestDiffDetail, PullRequestDiffFile, PullRequestFilters, PullRequestItem,
    PullRequestMergeMethod, PullRequestPage, PullRequestReadiness, PullRequestReadinessKind,
    PullRequestRevertResult, PullRequestReviewAction, PullRequestState, RepoLocator, RepoType,
    commit_count_from_detail, configured_api_base_url, configured_enterprise,
    configured_gitee_token, conversation_commit_batch_items, conversation_count_from_detail,
    conversation_merged_item, finalize_conversation_timeline, gitee_api_profile,
    is_redundant_commit_system_note, is_redundant_merge_system_note, percent_encode_path,
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
    enterprise_authenticated_builder, enterprise_pull_request_endpoint,
    enterprise_repository_commit_endpoint, parse_gitee_merge_method, public_authenticated_builder,
    public_authenticated_request, public_repo_endpoint,
};

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

    let payload: Value =
        http::send_json(request, RequestOptions::new("Load Gitee project details"))
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

    let payload: Value =
        http::send_json(request, RequestOptions::new("Load Gitee project details"))
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
        request = request.query(&[("state", state.gitee_query())]);
    }
    if let Some(token) = configured_gitee_token(GiteeApiProfile::EnterpriseV8) {
        request = request.header("PRIVATE-TOKEN", token);
    }
    decode_pull_request_page(request, page, per_page).await
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
    let detail_endpoint = enterprise_pull_request_endpoint(
        endpoints::ENTERPRISE_MERGE_REQUEST_URL,
        repo_project_id,
        pull_request_iid,
    )?;
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
            RequestOptions::new("Load Gitee pull request diff"),
        )
        .await
        .map_err(|error| error.to_string())
    };
    // MR detail supplies Conversation/Commits badge summaries without waiting for
    // the lazy Conversation tab payload.
    let detail_future = async {
        http::send_json(
            enterprise_authenticated_builder(http::get(detail_endpoint)),
            RequestOptions::new("Load Gitee enterprise pull request detail summary"),
        )
        .await
        .map_err(|error| error.to_string())
    };
    let (payload, detail) = tokio::join!(diffs_future, detail_future);
    let payload = payload?;
    if let Err(error) = &detail {
        log::debug!(
            "[CodeReview][Gitee] Enterprise badge detail summary unavailable; using bounded probes: {error}"
        );
    }
    let mut decoded = decode_pull_request_diff(&payload)?;
    // Enterprise Conversation badge: MR detail `user_notes_count` / `notes_count`.
    // Fallback is a bounded notes page probe, not the full Conversation timeline.
    decoded.conversation_count = Some(
        detail
            .as_ref()
            .ok()
            .and_then(conversation_count_from_detail)
            .or(count_pull_request_comments(locator, pull_request_iid, Some(repo_project_id)).await)
            .unwrap_or(0),
    );
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
        RequestOptions::new("Load Gitee pull request details"),
    )
    .await
    .map_err(|error| error.to_string())?;
    let files_endpoint = public_repo_endpoint(
        endpoints::PUBLIC_PULL_REQUEST_FILES_URL,
        locator,
        Some(pull_request_iid),
        None,
    );
    let values: Vec<Value> = http::send_json(
        public_authenticated_request(files_endpoint).query(&[("per_page", "100")]),
        RequestOptions::new("Load Gitee pull request files"),
    )
    .await
    .map_err(|error| error.to_string())?;
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
    Ok(PullRequestDiffDetail {
        added_lines: files.iter().map(|file| file.added_lines).sum(),
        removed_lines: files.iter().map(|file| file.removed_lines).sum(),
        // Public v5 PR detail often omits commit totals; probe a bounded page.
        commit_count: commit_count_from_detail(&detail)
            .or(count_pull_request_commits(locator, pull_request_iid, None).await)
            .unwrap_or(0),
        conversation_count: Some(
            conversation_count_from_detail(&detail)
                .or(count_pull_request_comments(locator, pull_request_iid, None).await)
                .unwrap_or(0),
        ),
        base_sha: nested_string(&detail, "base", "sha").unwrap_or_default(),
        head_sha: nested_string(&detail, "head", "sha").unwrap_or_default(),
        start_sha: nested_string(&detail, "base", "sha").unwrap_or_default(),
        too_many_commits: false,
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
                RequestOptions::new("Compare Gitee pull request commits"),
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
                RequestOptions::new("Compare Gitee Enterprise pull request commits"),
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
    let payload: Value =
        http::send_json(request, RequestOptions::new("Load Gitee repository file"))
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
    let current: Value = http::send_json(
        enterprise_authenticated_builder(http::get(current_endpoint)),
        RequestOptions::new("Load Gitee enterprise merge state"),
    )
    .await
    .map_err(|error| error.to_string())?;

    let detail_endpoint = enterprise_pull_request_endpoint(
        endpoints::ENTERPRISE_MERGE_REQUEST_URL,
        repo_project_id,
        pull_request_iid,
    )?;
    let detail: Value = http::send_json(
        enterprise_authenticated_builder(http::get(detail_endpoint)),
        RequestOptions::new("Load Gitee enterprise pull request details"),
    )
    .await
    .map_err(|error| error.to_string())?;

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
        RequestOptions::new("Load Gitee public pull request merge state"),
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
    let (detail, notes, reviews) = match gitee_api_profile(locator) {
        GiteeApiProfile::EnterpriseV8 => {
            let detail_endpoint = enterprise_pull_request_endpoint(
                endpoints::ENTERPRISE_MERGE_REQUEST_URL,
                repo_project_id,
                pull_request_iid,
            )?;
            let notes_endpoint = enterprise_pull_request_endpoint(
                endpoints::ENTERPRISE_MERGE_REQUEST_NOTES_URL,
                repo_project_id,
                pull_request_iid,
            )?;
            let reviews_endpoint = enterprise_pull_request_endpoint(
                endpoints::ENTERPRISE_MERGE_REQUEST_REVIEWS_URL,
                repo_project_id,
                pull_request_iid,
            )?;
            let detail_future = async {
                http::send_json(
                    enterprise_authenticated_builder(http::get(detail_endpoint)),
                    RequestOptions::new("Load Gitee enterprise merge request conversation header"),
                )
                .await
                .map_err(|error| error.to_string())
            };
            let notes_future = async {
                http::send_json(
                    enterprise_authenticated_builder(http::get(notes_endpoint))
                        .query(&[("per_page", "100")]),
                    RequestOptions::new("Load Gitee enterprise merge request notes"),
                )
                .await
                .map_err(|error| error.to_string())
            };
            // Reviews are best-effort: notes still render if this endpoint is
            // unavailable or returns an unexpected wrapper.
            let reviews_future = async {
                http::send_json(
                    enterprise_authenticated_builder(http::get(reviews_endpoint))
                        .query(&[("per_page", "100")]),
                    RequestOptions::new("Load Gitee enterprise merge request reviews"),
                )
                .await
                .ok()
            };
            let (detail, notes, reviews) =
                tokio::join!(detail_future, notes_future, reviews_future);
            (detail?, notes?, reviews)
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
            let detail = http::send_json(
                public_authenticated_request(detail_endpoint),
                RequestOptions::new("Load Gitee public pull request conversation header"),
            )
            .await
            .map_err(|error| error.to_string())?;
            let notes = http::send_json(
                public_authenticated_request(comments_endpoint)
                    .query(&[("page", "1"), ("per_page", "100")]),
                RequestOptions::new("Load Gitee public pull request comments"),
            )
            .await
            .map_err(|error| error.to_string())?;
            (detail, notes, None)
        }
    };

    let mut items = vec![PullRequestConversationItem {
        id: format!("description-{pull_request_iid}"),
        kind: "description".to_string(),
        author: nested_string(&detail, "author", "name")
            .or_else(|| nested_string(&detail, "author", "username"))
            .or_else(|| nested_string(&detail, "user", "login"))
            .or_else(|| nested_string(&detail, "user", "name"))
            .unwrap_or_else(|| "Unknown author".to_string()),
        avatar_url: nested_string(&detail, "author", "avatar_url")
            .or_else(|| nested_string(&detail, "author", "avatar"))
            .or_else(|| nested_string(&detail, "user", "avatar_url"))
            .or_else(|| nested_string(&detail, "user", "avatar"))
            .unwrap_or_default(),
        author_association: String::new(),
        body: string_field(&detail, "description")
            .or_else(|| string_field(&detail, "body"))
            .unwrap_or_default(),
        created_at: string_field(&detail, "created_at").unwrap_or_default(),
        state: string_field(&detail, "state").unwrap_or_default(),
    }];
    if let Some(values) = pull_request_values(&notes) {
        match gitee_api_profile(locator) {
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

    if let Some(reviews_payload) = reviews.as_ref() {
        if let Some(values) = pull_request_values(reviews_payload) {
            let note_bodies = items
                .iter()
                .filter(|item| item.kind == "comment" || item.kind == "review")
                .map(|item| (item.author.clone(), item.body.trim().to_string()))
                .filter(|(_, body)| !body.is_empty())
                .collect::<std::collections::HashSet<_>>();
            items.extend(values.iter().filter_map(|value| {
                let review = decode_conversation_review(value)?;
                // Prefer the richer note card when the same text already exists.
                if !review.body.trim().is_empty()
                    && note_bodies
                        .contains(&(review.author.clone(), review.body.trim().to_string()))
                {
                    return None;
                }
                Some(review)
            }));
        }
    }

    // Structured commit/merge rows mirror GitHub Conversation activity for both
    // Gitee profiles. Failures stay silent so notes still render.
    if let Ok(commits) = get_pull_request_commits(locator, repo_project_id, pull_request_iid).await
    {
        items.extend(conversation_commit_batch_items(&commits));
    }
    if let Some(merged_at) =
        string_field(&detail, "merged_at").filter(|value| !value.trim().is_empty())
    {
        let merge_sha = string_field(&detail, "merge_commit_sha")
            .or_else(|| nested_string(&detail, "merge_commit", "sha"))
            .unwrap_or_default();
        let target_branch = string_field(&detail, "target_branch")
            .or_else(|| nested_string(&detail, "base", "ref"))
            .unwrap_or_default();
        let author = nested_string(&detail, "merged_by", "name")
            .or_else(|| nested_string(&detail, "merged_by", "username"))
            .or_else(|| nested_string(&detail, "merged_by", "login"))
            .or_else(|| nested_string(&detail, "user", "login"))
            .or_else(|| nested_string(&detail, "author", "name"))
            .unwrap_or_else(|| "Unknown author".to_string());
        if !merge_sha.trim().is_empty() {
            items.push(conversation_merged_item(
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

    let description = items.remove(0);
    let activity = items;
    Ok(finalize_conversation_timeline(description, activity))
}

pub(super) async fn get_pull_request_commits(
    locator: &RepoLocator,
    repo_project_id: &str,
    pull_request_iid: &str,
) -> Result<Vec<PullRequestCommit>, String> {
    let endpoint = match gitee_api_profile(locator) {
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
    let request = match gitee_api_profile(locator) {
        GiteeApiProfile::EnterpriseV8 => {
            enterprise_authenticated_builder(http::get(endpoint)).query(&[("per_page", "100")])
        }
        GiteeApiProfile::PublicV5 => {
            public_authenticated_request(endpoint).query(&[("page", "1"), ("per_page", "100")])
        }
    };
    let payload: Value = http::send_json(
        request,
        RequestOptions::new("Load Gitee pull request commits"),
    )
    .await
    .map_err(|error| error.to_string())?;
    let values = pull_request_values(&payload)
        .ok_or_else(|| "Gitee pull request commit response is not a list.".to_string())?;
    let mut commits = values.iter().map(decode_commit).collect::<Vec<_>>();
    if gitee_api_profile(locator) == GiteeApiProfile::EnterpriseV8 {
        normalize_enterprise_commit_order(&mut commits);
    }
    Ok(commits)
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
    let endpoint = match gitee_api_profile(locator) {
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
    let (request, per_page) = match gitee_api_profile(locator) {
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
        RequestOptions::new("Count Gitee pull request commits"),
    )
    .await
    .ok()?;
    let headers = response.headers().clone();
    let payload: Value = response.json().await.ok()?;
    match gitee_api_profile(locator) {
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
                RequestOptions::new("Count Gitee enterprise pull request commits fallback"),
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
    let endpoint = match gitee_api_profile(locator) {
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
    let (request, per_page) = match gitee_api_profile(locator) {
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
        RequestOptions::new("Count Gitee pull request comments"),
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
                enterprise_authenticated_builder(http::get(detail_endpoint)),
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
    let response = http::send(request, RequestOptions::new("Load Gitee merge requests"))
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

    const COLLECTION_KEYS: [&str; 8] = [
        "items",
        "list",
        "content",
        "commits",
        "notes",
        "merge_requests",
        "pull_requests",
        "records",
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
        updated_at: string_field(value, "updated_at")
            .unwrap_or_else(|| "Unknown update time".to_string()),
        web_url: string_field(value, "web_url")
            .or_else(|| string_field(value, "html_url"))
            .unwrap_or_default(),
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
        bounded_list_total, commit_count_from_diff_payload, complete_collection_total,
        decode_commit, decode_conversation_comment, decode_conversation_review,
        decode_pull_request_diff, enterprise_commit_probe_total, enterprise_commit_refs_can_revert,
        enterprise_compare_diff_payload, enterprise_data_object, enterprise_note_payload,
        enterprise_review_payload, expand_enterprise_conversation_notes,
        gitee_enterprise_list_sort, gitee_enterprise_merge_readiness, gitee_public_list_sort,
        gitee_public_merge_readiness, normalize_enterprise_commit_order, pull_request_values,
        select_enterprise_project_id, trusted_enterprise_commit_count,
    };
    use crate::codereview::api::gitee as endpoints;
    use crate::git::{PullRequestReadinessKind, parse_repo_locator};
    use reqwest::header::HeaderMap;

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
    fn enterprise_approve_omits_an_empty_comment() {
        assert_eq!(
            enterprise_review_payload("approved", "  "),
            json!({ "state": "approved" })
        );
        assert_eq!(
            enterprise_review_payload("approved", " Looks good. "),
            json!({ "state": "approved", "comment": "Looks good." })
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
            "commit": {
                "message": "Public title\nbody",
                "author": { "name": "Public Author", "date": "2026-07-20T01:00:00Z" }
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
