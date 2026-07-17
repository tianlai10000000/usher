//! Gitee API URL templates.

pub(crate) const ENTERPRISE_API_VERSION_PATH: &str = "/api/v8";
pub(crate) const ENTERPRISE_PROJECTS_URL: &str = "{api_base_url}/projects";
pub(crate) const ENTERPRISE_MERGE_REQUESTS_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests";
pub(crate) const ENTERPRISE_MERGE_REQUEST_DIFFS_URL: &str =
    "{api_base_url}/projects/{project_id}/merge_requests/{merge_request_iid}/diffs";
pub(crate) const ENTERPRISE_REPOSITORY_FILE_URL: &str =
    "{api_base_url}/projects/{project_id}/repository/files/{file_path}";
pub(crate) const PUBLIC_REPOSITORY_URL: &str =
    "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}";
pub(crate) const PUBLIC_PULL_REQUESTS_URL: &str =
    "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/pulls";
pub(crate) const PUBLIC_PULL_REQUEST_URL: &str =
    "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/pulls/{pull_request_number}";
pub(crate) const PUBLIC_PULL_REQUEST_FILES_URL: &str = "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/pulls/{pull_request_number}/files";
pub(crate) const PUBLIC_REPOSITORY_FILE_URL: &str =
    "{scheme}://{authority}/api/v5/repos/{namespace}/{repository}/contents/{file_path}";
