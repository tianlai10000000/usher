//! GitHub API URL templates.

pub(crate) const PUBLIC_API_BASE_URL: &str = "https://api.github.com";
pub(crate) const PUBLIC_GRAPHQL_URL: &str = "https://api.github.com/graphql";
pub(crate) const ENTERPRISE_API_VERSION_PATH: &str = "/api/v3";
pub(crate) const REPOSITORY_URL: &str = "{api_base_url}/repos/{namespace}/{repository}";
pub(crate) const SEARCH_PULL_REQUESTS_URL: &str = "{api_base_url}/search/issues";
pub(crate) const PULL_REQUEST_URL: &str =
    "{api_base_url}/repos/{namespace}/{repository}/pulls/{pull_request_number}";
pub(crate) const PULL_REQUEST_FILES_URL: &str =
    "{api_base_url}/repos/{namespace}/{repository}/pulls/{pull_request_number}/files";
pub(crate) const PULL_REQUEST_REVIEWS_URL: &str =
    "{api_base_url}/repos/{namespace}/{repository}/pulls/{pull_request_number}/reviews";
pub(crate) const PULL_REQUEST_COMMITS_URL: &str =
    "{api_base_url}/repos/{namespace}/{repository}/pulls/{pull_request_number}/commits";
pub(crate) const REPOSITORY_COMPARE_URL: &str =
    "{api_base_url}/repos/{namespace}/{repository}/compare/{base}...{head}";
pub(crate) const PULL_REQUEST_ISSUE_COMMENTS_URL: &str =
    "{api_base_url}/repos/{namespace}/{repository}/issues/{pull_request_number}/comments";
pub(crate) const PULL_REQUEST_ISSUE_TIMELINE_URL: &str =
    "{api_base_url}/repos/{namespace}/{repository}/issues/{pull_request_number}/timeline";
pub(crate) const PULL_REQUEST_MERGE_URL: &str =
    "{api_base_url}/repos/{namespace}/{repository}/pulls/{pull_request_number}/merge";
pub(crate) const REPOSITORY_CONTENT_URL: &str =
    "{api_base_url}/repos/{namespace}/{repository}/contents/{file_path}";
