//! Gitee API URL templates.

pub(crate) const ENTERPRISE_API_VERSION_PATH: &str = "/api/v8";
pub(crate) const ENTERPRISE_PROJECTS_URL: &str = "{api_base_url}/projects";
pub(crate) const ENTERPRISE_MERGE_REQUESTS_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests";
pub(crate) const ENTERPRISE_MERGE_REQUEST_DIFFS_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}/diffs";
pub(crate) const ENTERPRISE_MERGE_REQUEST_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}";
pub(crate) const ENTERPRISE_MERGE_REQUEST_REVIEWS_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}/reviews";
pub(crate) const ENTERPRISE_MERGE_REQUEST_NOTES_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}/notes";
pub(crate) const ENTERPRISE_MERGE_REQUEST_COMMITS_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}/commits";
pub(crate) const ENTERPRISE_MERGE_REQUEST_CURRENT_STATE_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}/current_state";
pub(crate) const ENTERPRISE_MERGE_REQUEST_MERGE_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}/merge";
pub(crate) const ENTERPRISE_REPOSITORY_FILE_URL: &str =
    "{api_base_url}/projects/{project_id}/repository/files/{file_path}";
pub(crate) const ENTERPRISE_REPOSITORY_COMPARE_URL: &str =
    "{api_base_url}/projects/{project_id}/repository/compare";
pub(crate) const PUBLIC_REPOSITORY_URL: &str =
    "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}";
pub(crate) const PUBLIC_PULL_REQUESTS_URL: &str =
    "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/pulls";
pub(crate) const PUBLIC_PULL_REQUEST_URL: &str =
    "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/pulls/{pull_request_number}";
pub(crate) const PUBLIC_PULL_REQUEST_FILES_URL: &str = "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/pulls/{pull_request_number}/files";
pub(crate) const PUBLIC_PULL_REQUEST_COMMENTS_URL: &str = "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/pulls/{pull_request_number}/comments";
pub(crate) const PUBLIC_PULL_REQUEST_COMMITS_URL: &str = "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/pulls/{pull_request_number}/commits";
pub(crate) const PUBLIC_PULL_REQUEST_MERGE_URL: &str = "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/pulls/{pull_request_number}/merge";
pub(crate) const PUBLIC_REPOSITORY_FILE_URL: &str =
    "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/contents/{file_path}";
pub(crate) const PUBLIC_REPOSITORY_COMPARE_URL: &str =
    "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/compare/{base}...{head}";
