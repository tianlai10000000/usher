mod merge_tree;
mod model;
pub(crate) use model::*;

use super::{GiteeApiProfile, ProjectContext, RepoType, gitee_api_profile};
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(crate) fn status_from_payload(provider: &str, value: &Value) -> ConflictStatus {
    use ConflictStatus::*;
    if matches!(
        value.get("state").and_then(Value::as_str),
        Some("closed" | "merged")
    ) || value.get("merged").and_then(Value::as_bool) == Some(true)
    {
        return NotApplicable;
    }
    match provider {
        "github" => match value.get("mergeable") {
            Some(Value::Bool(true)) => Clear,
            Some(Value::Bool(false)) => Conflicting,
            Some(Value::Null) => Checking,
            _ => Unknown,
        },
        "gitlab" => {
            let status = value
                .get("detailed_merge_status")
                .or_else(|| value.get("merge_status"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if matches!(
                status,
                "checking" | "unchecked" | "preparing" | "approvals_syncing"
            ) {
                return Checking;
            }
            if status == "conflict"
                || value.get("has_conflicts").and_then(Value::as_bool) == Some(true)
            {
                return Conflicting;
            }
            if matches!(status, "mergeable" | "can_be_merged") {
                return Clear;
            }
            Unknown
        }
        "gitee-v8" => match value.get("conflict_result").and_then(Value::as_bool) {
            Some(true) => Clear,
            Some(false) => Conflicting,
            None => match value.get("has_conflict").and_then(Value::as_bool) {
                Some(true) => Conflicting,
                Some(false) => Clear,
                None => Unknown,
            },
        },
        // Public v5 does not define false as a code conflict.
        _ => {
            if value.get("mergeable").and_then(Value::as_bool) == Some(true) {
                Clear
            } else {
                Unknown
            }
        }
    }
}

pub(crate) async fn read_status(
    repo_type: &str,
    project: &str,
    remote: &str,
    iid: &str,
) -> Result<ConflictStatus, ConflictError> {
    let context = ProjectContext::from_saved(repo_type, project, remote)
        .map_err(|_| ConflictError::Unavailable)?;
    let (profile, payload) = read_detail(&context, iid).await?;
    Ok(status_from_payload(profile, &payload))
}

async fn read_detail(
    context: &ProjectContext,
    iid: &str,
) -> Result<(&'static str, Value), ConflictError> {
    if iid.is_empty() || !iid.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ConflictError::Unavailable);
    }
    let locator = context.locator();
    match context.repo_type() {
        RepoType::GitHub => Ok((
            "github",
            super::github::conflict_detail(locator, iid).await?,
        )),
        RepoType::GitLab => Ok((
            "gitlab",
            super::gitlab::conflict_detail(locator, context.project_id(), iid).await?,
        )),
        RepoType::Gitee => {
            let enterprise = gitee_api_profile(locator) == GiteeApiProfile::EnterpriseV8;
            Ok((
                if enterprise { "gitee-v8" } else { "gitee-v5" },
                super::gitee::conflict_detail(locator, context.project_id(), iid).await?,
            ))
        }
    }
}

pub(crate) async fn load(
    repo_type: &str,
    project: &str,
    remote: &str,
    iid: &str,
    source_branch: &str,
    target_branch: &str,
    cancel: Arc<AtomicBool>,
) -> Result<ConflictSnapshot, ConflictError> {
    let context = ProjectContext::from_saved(repo_type, project, remote)
        .map_err(|_| ConflictError::Unavailable)?;
    let (profile, detail) = read_detail(&context, iid).await?;
    let status = status_from_payload(profile, &detail);
    if cancel.load(Ordering::Relaxed) {
        return Err(ConflictError::Cancelled);
    }
    let mut snapshot = ConflictSnapshot {
        status,
        head: String::new(),
        target: String::new(),
        source_branch: source_branch.into(),
        target_branch: target_branch.into(),
        local: false,
        files: vec![],
    };
    if matches!(
        status,
        ConflictStatus::Clear | ConflictStatus::NotApplicable
    ) {
        return Ok(snapshot);
    }
    if profile == "gitee-v8" {
        match super::gitee::load_conflicts(context.locator(), project, iid, snapshot.clone()).await
        {
            Ok(snapshot) => return Ok(snapshot),
            Err(ConflictError::Unsupported) => {}
            Err(error) => return Err(error),
        }
    }
    snapshot.head = detail
        .pointer("/head/sha")
        .or_else(|| detail.get("sha"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .into();
    snapshot.target_branch = detail
        .pointer("/base/ref")
        .or_else(|| detail.get("target_branch"))
        .and_then(Value::as_str)
        .unwrap_or(target_branch)
        .into();
    snapshot.source_branch = detail
        .pointer("/head/ref")
        .or_else(|| detail.get("source_branch"))
        .and_then(Value::as_str)
        .unwrap_or(source_branch)
        .into();
    snapshot.target = detail
        .pointer("/base/sha")
        .and_then(Value::as_str)
        .unwrap_or("")
        .into();
    tokio::time::timeout(
        std::time::Duration::from_secs(120),
        merge_tree::analyze(remote, context.repo_type(), iid, snapshot, cancel),
    )
    .await
    .map_err(|_| ConflictError::Unavailable)?
}

pub(super) fn http_error(error: crate::http::HttpError) -> ConflictError {
    if matches!(error.status.map(|s| s.as_u16()), Some(401 | 403)) {
        ConflictError::Authentication
    } else if matches!(error.status.map(|s| s.as_u16()), Some(404 | 501)) {
        ConflictError::Unsupported
    } else if error.kind == crate::http::HttpErrorKind::ResponseTooLarge {
        ConflictError::TooLarge
    } else {
        ConflictError::Unavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn provider_status_semantics_are_independent_of_merge_gates() {
        assert_eq!(
            status_from_payload("github", &json!({"mergeable":false,"draft":true})),
            ConflictStatus::Conflicting
        );
        assert_eq!(
            status_from_payload("github", &json!({"mergeable":null})),
            ConflictStatus::Checking
        );
        assert_eq!(
            status_from_payload(
                "gitee-v8",
                &json!({"conflict_result":true,"can_merge":false})
            ),
            ConflictStatus::Clear
        );
        assert_eq!(
            status_from_payload("gitee-v8", &json!({"conflict_result":false})),
            ConflictStatus::Conflicting
        );
        assert_eq!(
            status_from_payload("gitee-v5", &json!({"mergeable":false})),
            ConflictStatus::Unknown
        );
        assert_eq!(
            status_from_payload(
                "gitlab",
                &json!({"detailed_merge_status":"checking","has_conflicts":false})
            ),
            ConflictStatus::Checking
        );
        assert_eq!(
            status_from_payload(
                "gitlab",
                &json!({"detailed_merge_status":"not_approved","has_conflicts":false})
            ),
            ConflictStatus::Unknown
        );
        assert_eq!(
            status_from_payload("github", &json!({"state":"closed","mergeable":false})),
            ConflictStatus::NotApplicable
        );
    }
}
