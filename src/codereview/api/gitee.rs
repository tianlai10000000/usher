//! Gitee API URL templates.

pub(crate) const ENTERPRISE_API_VERSION_PATH: &str = "/api/v8";
pub(crate) const ENTERPRISE_PROJECTS_URL: &str = "{api_base_url}/projects";
pub(crate) const ENTERPRISE_CURRENT_USER_URL: &str = "{api_base_url}/user";
pub(crate) const ENTERPRISE_USERS_URL: &str = "{api_base_url}/users";
pub(crate) const ENTERPRISE_PROJECT_MEMBERS_URL: &str =
    "{api_base_url}/projects/{project_id}/members";
pub(crate) const ENTERPRISE_PROJECT_EVENTS_URL: &str =
    "{api_base_url}/projects/{project_id}/events";
pub(crate) const ENTERPRISE_MERGE_REQUESTS_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests";
pub(crate) const ENTERPRISE_MERGE_REQUEST_DIFFS_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}/diffs";
pub(crate) const ENTERPRISE_MERGE_REQUEST_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}";
pub(crate) const ENTERPRISE_MERGE_REQUEST_CONFLICTS_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}/conflicts";
pub(crate) const ENTERPRISE_MERGE_REQUEST_REVIEWS_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}/reviews";
pub(crate) const ENTERPRISE_WEB_PULL_REQUEST_REVIEWS_URL: &str =
    "{scheme}://{authority}/api/code/api/{full_path}/-/pull_requests/{pull_request_iid}/reviews";
pub(crate) const ENTERPRISE_MERGE_REQUEST_NOTES_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}/notes";
pub(crate) const ENTERPRISE_MERGE_REQUEST_COMMITS_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}/commits";
pub(crate) const ENTERPRISE_MERGE_REQUEST_CURRENT_STATE_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}/current_state";
pub(crate) const ENTERPRISE_MERGE_REQUEST_MERGE_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}/merge";
pub(crate) const ENTERPRISE_REPOSITORY_COMMIT_REFS_URL: &str =
    "{api_base_url}/projects/{project_id}/repository/commits/{commit_sha}/refs";
pub(crate) const ENTERPRISE_REPOSITORY_COMMIT_URL: &str =
    "{api_base_url}/projects/{project_id}/repository/commits/{commit_sha}";
pub(crate) const ENTERPRISE_REPOSITORY_COMMIT_REVERT_URL: &str =
    "{api_base_url}/projects/{project_id}/repository/commits/{commit_sha}/revert";
pub(crate) const ENTERPRISE_REPOSITORY_FILE_URL: &str =
    "{api_base_url}/projects/{project_id}/repository/files/{file_path}";
pub(crate) const ENTERPRISE_REPOSITORY_COMPARE_DIFFS_URL: &str =
    "{api_base_url}/projects/{project_id}/repository/compare/diffs";
pub(crate) const PUBLIC_REPOSITORY_URL: &str =
    "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}";
pub(crate) const PUBLIC_PULL_REQUESTS_URL: &str =
    "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/pulls";
pub(crate) const PUBLIC_PULL_REQUEST_URL: &str =
    "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/pulls/{pull_request_number}";
pub(crate) const PUBLIC_PULL_REQUEST_FILES_URL: &str = "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/pulls/{pull_request_number}/files";
pub(crate) const PUBLIC_PULL_REQUEST_COMMENTS_URL: &str = "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/pulls/{pull_request_number}/comments";
pub(crate) const PUBLIC_PULL_REQUEST_OPERATE_LOGS_URL: &str = "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/pulls/{pull_request_number}/operate_logs";
pub(crate) const PUBLIC_PULL_REQUEST_COMMITS_URL: &str = "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/pulls/{pull_request_number}/commits";
pub(crate) const PUBLIC_PULL_REQUEST_MERGE_URL: &str = "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/pulls/{pull_request_number}/merge";
pub(crate) const PUBLIC_REPOSITORY_FILE_URL: &str =
    "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/contents/{file_path}";
pub(crate) const PUBLIC_REPOSITORY_COMPARE_URL: &str =
    "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/compare/{base}...{head}";
