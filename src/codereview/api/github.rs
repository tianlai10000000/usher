//! GitHub API URL templates.

pub(crate) const PUBLIC_API_BASE_URL: &str = "https://api.github.com";
pub(crate) const ENTERPRISE_API_VERSION_PATH: &str = "/api/v3";
pub(crate) const REPOSITORY_URL: &str = "{api_base_url}/repos/{namespace}/{repository}";
pub(crate) const SEARCH_PULL_REQUESTS_URL: &str = "{api_base_url}/search/issues";
pub(crate) const PULL_REQUEST_URL: &str =
    "{api_base_url}/repos/{namespace}/{repository}/pulls/{pull_request_number}";
pub(crate) const PULL_REQUEST_FILES_URL: &str =
    "{api_base_url}/repos/{namespace}/{repository}/pulls/{pull_request_number}/files";
pub(crate) const REPOSITORY_CONTENT_URL: &str =
    "{api_base_url}/repos/{namespace}/{repository}/contents/{file_path}";
