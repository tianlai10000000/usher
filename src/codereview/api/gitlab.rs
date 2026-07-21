//! GitLab API URL templates.

pub(crate) const API_VERSION_PATH: &str = "/api/v4";
pub(crate) const PROJECT_URL: &str = "{api_base_url}/projects/{project_id_or_path}";
pub(crate) const MERGE_REQUESTS_URL: &str = "{api_base_url}/projects/{project_id}/merge_requests";
pub(crate) const MERGE_REQUEST_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}";
pub(crate) const MERGE_REQUEST_DIFFS_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}/diffs";
pub(crate) const MERGE_REQUEST_NOTES_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}/notes";
pub(crate) const MERGE_REQUEST_COMMITS_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}/commits";
pub(crate) const REPOSITORY_FILE_RAW_URL: &str =
    "{api_base_url}/projects/{project_id}/repository/files/{file_path}/raw";
pub(crate) const REPOSITORY_COMPARE_URL: &str =
    "{api_base_url}/projects/{project_id}/repository/compare";
