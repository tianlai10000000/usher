use super::*;
use crate::git::conflicts::{
    ConflictError, ConflictFile, ConflictSnapshot, MAX_CONTENT, MAX_FILES, http_error, parse_blocks,
};
use crate::http::{self, RequestOptions};
use base64::Engine;
use serde_json::Value;

pub(in crate::git) async fn conflict_detail(
    locator: &RepoLocator,
    project: &str,
    iid: &str,
) -> Result<Value, ConflictError> {
    let request = if gitee_api_profile(locator) == GiteeApiProfile::EnterpriseV8 {
        enterprise_pull_request_get(
            enterprise_pull_request_endpoint(
                endpoints::ENTERPRISE_MERGE_REQUEST_CURRENT_STATE_URL,
                project,
                iid,
            )
            .map_err(|_| ConflictError::Unavailable)?,
        )
    } else {
        public_authenticated_request(public_repo_endpoint(
            endpoints::PUBLIC_PULL_REQUEST_URL,
            locator,
            Some(iid),
            None,
        ))
    };
    let payload: Value = http::send_json(
        request,
        RequestOptions::new("Load Gitee conflict status").response_byte_limit(1024 * 1024),
    )
    .await
    .map_err(http_error)?;
    Ok(enterprise_data_object(&payload).clone())
}

pub(in crate::git) async fn load_conflicts(
    _locator: &RepoLocator,
    project: &str,
    iid: &str,
    snapshot: ConflictSnapshot,
) -> Result<ConflictSnapshot, ConflictError> {
    let before_head = resolve_revision(project, &format!("refs/pull/{iid}/head")).await?;
    let before_target =
        resolve_revision(project, &format!("refs/heads/{}", snapshot.target_branch)).await?;
    let endpoint = enterprise_pull_request_endpoint(
        endpoints::ENTERPRISE_MERGE_REQUEST_CONFLICTS_URL,
        project,
        iid,
    )
    .map_err(|_| ConflictError::Unavailable)?;
    let payload: Value = http::send_json(
        enterprise_pull_request_get(endpoint),
        RequestOptions::new("Load Gitee conflict files").response_byte_limit(16 * 1024 * 1024),
    )
    .await
    .map_err(http_error)?;
    let payload = enterprise_data_object(&payload).clone();
    let mut snapshot = tokio::task::spawn_blocking(move || decode(&payload, snapshot))
        .await
        .map_err(|_| ConflictError::Unavailable)??;
    let head = resolve_revision(project, &snapshot.head).await?;
    let target = resolve_revision(project, &snapshot.target).await?;
    if before_head != head || before_target != target {
        return Err(ConflictError::Stale);
    }
    snapshot.head = head;
    snapshot.target = target;
    snapshot.status = crate::git::conflicts::ConflictStatus::Conflicting;
    Ok(snapshot)
}

async fn resolve_revision(project: &str, revision: &str) -> Result<String, ConflictError> {
    if revision.is_empty() {
        return Err(ConflictError::InvalidContent);
    }
    let endpoint = enterprise_repository_commit_endpoint(
        endpoints::ENTERPRISE_REPOSITORY_COMMIT_URL,
        project,
        revision,
    )
    .map_err(|_| ConflictError::Unavailable)?;
    let payload: Value = http::send_json(
        enterprise_pull_request_get(endpoint),
        RequestOptions::new("Resolve Gitee conflict revision").response_byte_limit(1024 * 1024),
    )
    .await
    .map_err(http_error)?;
    let value = enterprise_data_object(&payload);
    let sha = value
        .get("id")
        .or_else(|| value.get("sha"))
        .and_then(Value::as_str)
        .ok_or(ConflictError::InvalidContent)?;
    if !matches!(sha.len(), 40 | 64) || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ConflictError::InvalidContent);
    }
    Ok(sha.to_owned())
}

fn decode(
    payload: &Value,
    mut snapshot: ConflictSnapshot,
) -> Result<ConflictSnapshot, ConflictError> {
    let files = payload
        .get("files")
        .and_then(Value::as_array)
        .ok_or(ConflictError::InvalidContent)?;
    if files.len() > MAX_FILES {
        return Err(ConflictError::TooLarge);
    }
    // The native endpoint may return refs instead of immutable SHA values. Do
    // not label them as SHA or silently cache this response as a fixed revision.
    snapshot.head = payload
        .get("commit_sha")
        .and_then(Value::as_str)
        .unwrap_or("")
        .into();
    snapshot.target = payload
        .get("target_commit_sha")
        .and_then(Value::as_str)
        .unwrap_or("")
        .into();
    snapshot.source_branch = payload
        .get("source_branch")
        .and_then(Value::as_str)
        .unwrap_or(&snapshot.source_branch)
        .into();
    snapshot.target_branch = payload
        .get("target_branch")
        .and_then(Value::as_str)
        .unwrap_or(&snapshot.target_branch)
        .into();
    let mut bytes = 0usize;
    for file in files {
        let path = file
            .get("their_path")
            .and_then(Value::as_str)
            .filter(|p| !p.is_empty())
            .or_else(|| {
                file.get("our_path")
                    .and_then(Value::as_str)
                    .filter(|p| !p.is_empty())
            })
            .ok_or(ConflictError::InvalidContent)?
            .to_owned();
        let target_path = file
            .get("our_path")
            .and_then(Value::as_str)
            .unwrap_or(&path)
            .to_owned();
        let content = if file.get("type").and_then(Value::as_str) == Some("text") {
            match file.get("content").and_then(Value::as_str) {
                Some(text) => match file
                    .get("encoding")
                    .and_then(Value::as_str)
                    .unwrap_or("utf-8")
                {
                    "utf-8" | "utf8" | "text" => Some(text.to_owned()),
                    "base64" => base64::prelude::BASE64_STANDARD
                        .decode(text)
                        .ok()
                        .and_then(|b| String::from_utf8(b).ok()),
                    _ => None,
                },
                None => None,
            }
        } else {
            None
        }
        .filter(|text| !text.as_bytes().contains(&0));
        bytes = bytes.saturating_add(content.as_ref().map_or(0, String::len));
        if bytes > MAX_CONTENT {
            return Err(ConflictError::TooLarge);
        }
        let blocks = content
            .as_ref()
            .map(|s| parse_blocks(s))
            .transpose()?
            .unwrap_or_default();
        snapshot.files.push(ConflictFile {
            path,
            target_path,
            content: content.map(Into::into),
            blocks,
        });
    }
    if snapshot.files.is_empty() {
        return Err(ConflictError::Stale);
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_read_ignores_write_permissions_and_keeps_both_paths() {
        let payload = serde_json::json!({"can_operation_conflict":false,"commit_sha":"refs/pull/2/head","files":[{"their_path":"new.rs","our_path":"old.rs","type":"text","encoding":"utf-8","content":"<<<<<<< source\na\n=======\nb\n>>>>>>> target\n"},{"their_path":"binary","type":"binary"}]});
        let snapshot = ConflictSnapshot {
            status: crate::git::conflicts::ConflictStatus::Conflicting,
            head: String::new(),
            target: String::new(),
            source_branch: String::new(),
            target_branch: String::new(),
            local: false,
            files: vec![],
        };
        let result = decode(&payload, snapshot).unwrap();
        assert_eq!(result.files[0].target_path, "old.rs");
        assert_eq!(result.files[0].blocks.len(), 1);
        assert!(result.files[1].content.is_none());
        assert_eq!(result.head, "refs/pull/2/head");
    }
}
