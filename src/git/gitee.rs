//! Gitee-compatible repository operations.
//!
//! `project_name` is always the repository name extracted from `remote_repo`
//! (the final path segment before `.git`), never the editable project alias.

use base64::Engine as _;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap};
use serde_json::Value;

use crate::codereview::api::gitee as endpoints;
use crate::http::{self, RequestOptions};

use super::{
    GiteeApiProfile, PullRequestActionState, PullRequestDiffDetail, PullRequestDiffFile,
    PullRequestDiffLine, PullRequestItem, PullRequestMergeMethod, PullRequestPage,
    PullRequestReadiness, PullRequestReadinessKind, PullRequestReviewAction, PullRequestState,
    RepoLocator, RepoType, configured_api_base_url, configured_enterprise, configured_gitee_token,
    gitee_api_profile, percent_encode_path,
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
        return get_enterprise_pull_requests(repo_project_id, state, page, per_page).await;
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
    get_public_pull_requests(locator, state, page, per_page).await
}

async fn get_enterprise_pull_requests(
    repo_project_id: &str,
    state: Option<PullRequestState>,
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
    let mut request = http::get(endpoint)
        .header("OAUTH", "enabled")
        .header(ACCEPT, "application/json")
        .query(&[
            ("per_page", &per_page.to_string()),
            ("page", &page.max(1).to_string()),
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

    let endpoint = endpoints::ENTERPRISE_MERGE_REQUEST_DIFFS_URL
        .replace("{api_base_url}", enterprise_api_base_url()?.as_str())
        .replace(
            "{project_id}",
            percent_encode_path(repo_project_id.trim()).as_str(),
        )
        .replace(
            "{merge_request_iid}",
            percent_encode_path(pull_request_iid.trim()).as_str(),
        );
    let mut request = http::post(endpoint)
        .header("OAUTH", "enabled")
        .header(ACCEPT, "application/json")
        .json(&serde_json::json!({
            "paths": [],
            "view_mode": "",
            "ignore_whitespace_change": ignore_whitespace_change,
        }));
    if let Some(enterprise) = configured_enterprise(RepoType::Gitee) {
        request = request.header("enterprise", enterprise);
    }
    if let Some(token) = configured_gitee_token(GiteeApiProfile::EnterpriseV8) {
        request = request.header("PRIVATE-TOKEN", token);
    }

    log::debug!(
        "[CodeReview][Gitee] Load pull-request diff: remote_host={} project_id_set=true iid={} ignore_whitespace={} enterprise_configured={} token_configured={}",
        locator.host,
        pull_request_iid,
        ignore_whitespace_change,
        configured_enterprise(RepoType::Gitee).is_some(),
        configured_gitee_token(GiteeApiProfile::EnterpriseV8).is_some(),
    );
    let payload: Value =
        http::send_json(request, RequestOptions::new("Load Gitee pull request diff"))
            .await
            .map_err(|error| error.to_string())?;
    decode_pull_request_diff(&payload)
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
        base_sha: nested_string(&detail, "base", "sha").unwrap_or_default(),
        head_sha: nested_string(&detail, "head", "sha").unwrap_or_default(),
        start_sha: nested_string(&detail, "base", "sha").unwrap_or_default(),
        too_many_commits: false,
        files,
    })
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
            review_state: String::new(),
            readiness: PullRequestReadiness::new(
                PullRequestReadinessKind::Unavailable,
                "Configure a Gitee Enterprise Token to load the enterprise merge conditions.",
            ),
            merge_block_reason:
                "Configure a Gitee Enterprise Token in Settings to review or merge this pull request."
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
    let server_can_merge = current
        .get("can_merge")
        .and_then(Value::as_bool)
        .or_else(|| current.get("can_be_merge").and_then(Value::as_bool))
        .unwrap_or(false);
    let review_state = string_field(&detail, "review_state")
        .or_else(|| string_field(&current, "review_state"))
        .unwrap_or_default();
    let readiness = gitee_enterprise_merge_readiness(&current, &detail, review_state.as_str());
    let can_merge =
        has_token && server_can_merge && readiness.kind == PullRequestReadinessKind::Ready;
    let mut allowed_merge_methods = detail
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

    Ok(PullRequestActionState {
        can_comment: has_token,
        can_approve: has_token,
        can_request_changes: has_token,
        can_merge,
        review_state,
        readiness,
        merge_block_reason,
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
        review_state: String::new(),
        readiness,
        merge_block_reason,
        allowed_merge_methods: vec![
            PullRequestMergeMethod::Merge,
            PullRequestMergeMethod::Squash,
        ],
    })
}

fn gitee_enterprise_merge_readiness(
    current: &Value,
    detail: &Value,
    review_state: &str,
) -> PullRequestReadiness {
    let state = string_field(current, "state")
        .or_else(|| string_field(detail, "state"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if state == "merged" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Merged,
            "Gitee Enterprise reports that this pull request has been merged.",
        );
    }
    if state == "closed" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Closed,
            "Gitee Enterprise reports that this pull request is closed.",
        );
    }
    if state == "drafted"
        || detail.get("draft").and_then(Value::as_bool) == Some(true)
        || detail.get("is_draft").and_then(Value::as_bool) == Some(true)
    {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Draft,
            "Mark the Gitee Enterprise pull request as ready before merging.",
        );
    }

    // The enterprise v8 document defines conflict_result=true as “no
    // conflict”; false is the conflicting state.
    if current.get("conflict_result").and_then(Value::as_bool) == Some(false) {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Conflicts,
            "Gitee Enterprise reports merge conflicts between the source and target branches.",
        );
    }
    if review_state == "review_rejected" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::ChangesRequested,
            "A Gitee Enterprise reviewer requested changes.",
        );
    }
    if current.get("scan_quality_state").and_then(Value::as_bool) == Some(false) {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::ChecksFailed,
            "The Gitee Enterprise quality gate has not passed.",
        );
    }
    let note = current.get("note").unwrap_or(&Value::Null);
    let unresolved_notes = note
        .get("resolve_note_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && note.get("resolve").and_then(Value::as_u64).unwrap_or(0)
            < note.get("total").and_then(Value::as_u64).unwrap_or(0);
    if unresolved_notes {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::DiscussionsUnresolved,
            "Resolve all required Gitee Enterprise review comments before merging.",
        );
    }
    let review_requirement_unmet = ["primary_review", "general_review"].iter().any(|key| {
        current
            .get(key)
            .and_then(|review| review.get("meet_merger_conditions"))
            .and_then(Value::as_bool)
            == Some(false)
    });
    if review_state == "review_pending" || review_requirement_unmet {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::ReviewRequired,
            "Required Gitee Enterprise reviews have not been completed.",
        );
    }
    if current.get("strategy_result").and_then(Value::as_bool) == Some(false) {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Blocked,
            "A Gitee Enterprise merge strategy or repository rule is blocking this merge.",
        );
    }
    if current
        .get("can_merge")
        .and_then(Value::as_bool)
        .or_else(|| current.get("can_be_merge").and_then(Value::as_bool))
        == Some(true)
    {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Ready,
            "All merge conditions reported by Gitee Enterprise are satisfied.",
        );
    }
    PullRequestReadiness::new(
        PullRequestReadinessKind::Blocked,
        "Gitee Enterprise reports that this pull request does not currently meet the merge conditions.",
    )
}

fn gitee_public_merge_readiness(detail: &Value) -> PullRequestReadiness {
    let state = string_field(detail, "state")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if state == "merged" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Merged,
            "Gitee public v5 reports that this pull request has been merged.",
        );
    }
    if state == "closed" {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Closed,
            "Gitee public v5 reports that this pull request is closed.",
        );
    }
    if detail.get("draft").and_then(Value::as_bool) == Some(true)
        || detail.get("is_draft").and_then(Value::as_bool) == Some(true)
    {
        return PullRequestReadiness::new(
            PullRequestReadinessKind::Draft,
            "Mark the Gitee pull request as ready before merging.",
        );
    }
    match detail
        .get("mergeable")
        .and_then(Value::as_bool)
        .or_else(|| detail.get("can_merge").and_then(Value::as_bool))
    {
        Some(true) if state == "open" || state == "opened" => PullRequestReadiness::new(
            PullRequestReadinessKind::Ready,
            "Gitee public v5 reports that this pull request can be merged.",
        ),
        Some(false) => PullRequestReadiness::new(
            PullRequestReadinessKind::Blocked,
            "Gitee public v5 reports that this pull request cannot currently be merged.",
        ),
        _ => PullRequestReadiness::new(
            PullRequestReadinessKind::Unavailable,
            "Gitee public v5 did not return a mergeable state for this pull request.",
        ),
    }
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
                endpoints::ENTERPRISE_MERGE_REQUEST_REVIEWS_URL,
                repo_project_id,
                pull_request_iid,
            )?;
            let state = match action {
                PullRequestReviewAction::Comment => "commented",
                PullRequestReviewAction::Approve => "approved",
                PullRequestReviewAction::RequestChanges => "rejected",
            };
            if action != PullRequestReviewAction::Approve && body.trim().is_empty() {
                return Err("A review comment is required for this action.".into());
            }
            let request = enterprise_authenticated_builder(http::post(endpoint))
                .json(&serde_json::json!({ "comment": body.trim(), "state": state }));
            http::send(
                request,
                RequestOptions::new("Submit Gitee enterprise review"),
            )
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

fn enterprise_pull_request_endpoint(
    template: &str,
    repo_project_id: &str,
    pull_request_iid: &str,
) -> Result<String, String> {
    if repo_project_id.trim().is_empty() || pull_request_iid.trim().is_empty() {
        return Err("Gitee project id or pull request number is empty.".to_string());
    }
    Ok(template
        .replace("{api_base_url}", enterprise_api_base_url()?.as_str())
        .replace(
            "{project_id}",
            percent_encode_path(repo_project_id.trim()).as_str(),
        )
        .replace(
            "{merge_request_iid}",
            percent_encode_path(pull_request_iid.trim()).as_str(),
        ))
}

fn enterprise_authenticated_builder(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    let mut request = request
        .header("OAUTH", "enabled")
        .header(ACCEPT, "application/json");
    if let Some(enterprise) = configured_enterprise(RepoType::Gitee) {
        request = request.header("enterprise", enterprise);
    }
    if let Some(token) = configured_gitee_token(GiteeApiProfile::EnterpriseV8) {
        request = request.header("PRIVATE-TOKEN", token);
    }
    request
}

fn public_authenticated_builder(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    let mut request = request.header(ACCEPT, "application/json");
    if let Some(token) = configured_gitee_token(GiteeApiProfile::PublicV5) {
        request = request.header("Authorization", format!("token {token}"));
    }
    request
}

fn parse_gitee_merge_method(value: &str) -> Option<PullRequestMergeMethod> {
    PullRequestMergeMethod::parse(value)
}

fn public_authenticated_request(endpoint: String) -> reqwest::RequestBuilder {
    public_authenticated_builder(http::get(endpoint))
}

fn public_repo_endpoint(
    template: &str,
    locator: &RepoLocator,
    pull_request_iid: Option<&str>,
    file_path: Option<&str>,
) -> String {
    let mut endpoint = template
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
    if let Some(iid) = pull_request_iid {
        endpoint = endpoint.replace(
            "{pull_request_number}",
            percent_encode_path(iid.trim()).as_str(),
        );
    }
    if let Some(path) = file_path {
        endpoint = endpoint.replace("{file_path}", percent_encode_path(path).as_str());
    }
    endpoint
}

fn decode_base64_content(payload: &Value) -> Result<String, String> {
    let content = payload
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| "Gitee repository file response has no content.".to_string())?
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content.as_bytes())
        .map_err(|error| format!("Gitee repository file base64 decode failed: {error}"))?;
    String::from_utf8(bytes)
        .map_err(|error| format!("Gitee repository file is not UTF-8 text: {error}"))
}

fn decode_pull_request_diff(payload: &Value) -> Result<PullRequestDiffDetail, String> {
    let diffs = payload
        .get("diffs")
        .and_then(Value::as_array)
        .ok_or_else(|| "Gitee pull request diff response has no diff list.".to_string())?;
    let files = diffs.iter().map(decode_diff_file).collect::<Vec<_>>();
    let refs = payload.get("diff_refs").unwrap_or(&Value::Null);
    Ok(PullRequestDiffDetail {
        added_lines: value_u32(payload, "added_lines")
            .unwrap_or_else(|| files.iter().map(|file| file.added_lines).sum()),
        removed_lines: value_u32(payload, "removed_lines")
            .unwrap_or_else(|| files.iter().map(|file| file.removed_lines).sum()),
        base_sha: string_field(refs, "base_sha").unwrap_or_default(),
        head_sha: string_field(refs, "head_sha").unwrap_or_default(),
        start_sha: string_field(refs, "start_sha").unwrap_or_default(),
        too_many_commits: payload
            .get("too_more_commits")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        files,
    })
}

fn decode_diff_file(value: &Value) -> PullRequestDiffFile {
    let statistic = value.get("statistic").unwrap_or(&Value::Null);
    let path = string_field(statistic, "new_path")
        .or_else(|| string_field(statistic, "path"))
        .unwrap_or_else(|| "Unknown file".to_string());
    let old_path = string_field(statistic, "old_path").unwrap_or_else(|| path.clone());
    let mut lines = value
        .get("content")
        .and_then(|content| content.get("text"))
        .and_then(Value::as_array)
        .map(|values| values.iter().map(decode_diff_line).collect::<Vec<_>>())
        .unwrap_or_default();
    if lines.is_empty() {
        lines.push(PullRequestDiffLine {
            old_line: 0,
            new_line: 0,
            kind: "meta".to_string(),
            content: "Binary, image, or oversized diff content is not available.".to_string(),
        });
    }

    PullRequestDiffFile {
        path,
        old_path,
        change_type: string_field(statistic, "type").unwrap_or_else(|| {
            if value
                .get("new_file")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                "new_file".to_string()
            } else if value
                .get("deleted_file")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                "deleted_file".to_string()
            } else if value
                .get("renamed_file")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                "renamed_file".to_string()
            } else {
                "edit_file".to_string()
            }
        }),
        added_lines: value_u32(value, "added_lines").unwrap_or(0),
        removed_lines: value_u32(value, "removed_lines").unwrap_or(0),
        // Gitee documents statistic.viewed as file read permission, not the
        // current reviewer's viewed/unviewed progress used by GitHub's filter.
        viewed: false,
        lines,
    }
}

fn decode_diff_line(value: &Value) -> PullRequestDiffLine {
    let raw_kind = string_field(value, "type").unwrap_or_default();
    PullRequestDiffLine {
        old_line: nested_line_number(value, "old_line"),
        new_line: nested_line_number(value, "new_line"),
        kind: match raw_kind.as_str() {
            "new" => "addition",
            "old" => "deletion",
            "match" => "hunk",
            _ => "context",
        }
        .to_string(),
        content: html_diff_line_to_text(
            string_field(value, "line_content")
                .unwrap_or_default()
                .as_str(),
        ),
    }
}

fn nested_line_number(value: &Value, key: &str) -> i32 {
    value
        .get(key)
        .and_then(|line| line.get("line_num"))
        .and_then(|number| {
            number
                .as_i64()
                .or_else(|| number.as_str().and_then(|value| value.parse().ok()))
        })
        .and_then(|number| i32::try_from(number).ok())
        .unwrap_or(0)
}

fn html_line_to_text(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    let mut inside_tag = false;
    for character in value.chars() {
        match character {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => text.push(character),
            _ => {}
        }
    }
    decode_html_entities(text.as_str())
}

fn html_diff_line_to_text(value: &str) -> String {
    // The enterprise diff endpoint sometimes pretty-prints its syntax HTML
    // with literal CR/LF characters between spans. A PullRequestDiffLine is
    // one source line, so retaining those separators makes Slint lay out one
    // fixed-height diff row as multiple text lines. Remove only line-ending
    // controls after decoding; ordinary spaces and tabs remain source data.
    html_line_to_text(value)
        .chars()
        .filter(|character| !matches!(character, '\r' | '\n'))
        .collect()
}

fn decode_html_entities(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(entity_start) = remaining.find('&') {
        decoded.push_str(&remaining[..entity_start]);
        remaining = &remaining[entity_start..];
        let Some(entity_end) = remaining.find(';').filter(|end| *end <= 12) else {
            decoded.push('&');
            remaining = &remaining[1..];
            continue;
        };
        let entity = &remaining[1..entity_end];
        let replacement = match entity {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "amp" => Some('&'),
            numeric if numeric.starts_with("#x") || numeric.starts_with("#X") => {
                u32::from_str_radix(&numeric[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            numeric if numeric.starts_with('#') => {
                numeric[1..].parse::<u32>().ok().and_then(char::from_u32)
            }
            _ => None,
        };
        if let Some(replacement) = replacement {
            decoded.push(replacement);
        } else {
            decoded.push_str(&remaining[..=entity_end]);
        }
        remaining = &remaining[entity_end + 1..];
    }
    decoded.push_str(remaining);
    decoded
}

async fn get_public_pull_requests(
    locator: &RepoLocator,
    state: Option<PullRequestState>,
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
    let mut request = http::get(endpoint)
        .header(ACCEPT, "application/json")
        .query(&[
            ("per_page", &per_page.to_string()),
            ("page", &page.max(1).to_string()),
        ]);
    if let Some(state) = state {
        request = request.query(&[("state", state.gitee_public_query())]);
    }
    if let Some(token) = configured_gitee_token(GiteeApiProfile::PublicV5) {
        request = request.header("Authorization", format!("token {token}"));
    }
    decode_pull_request_page(request, page, per_page).await
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

    const COLLECTION_KEYS: [&str; 6] = [
        "items",
        "list",
        "content",
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
        decode_pull_request_diff, gitee_enterprise_merge_readiness, gitee_public_merge_readiness,
        pull_request_values, select_enterprise_project_id,
    };
    use crate::git::{PullRequestReadinessKind, parse_repo_locator};

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
    fn decodes_enterprise_diff_lines_and_strips_html() {
        let payload = json!({
            "added_lines": 1,
            "removed_lines": 1,
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
