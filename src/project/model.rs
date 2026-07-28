//! Project persistence and view models.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ProjectConfig {
    pub(super) project_name: String,
    pub(super) project_alias: String,
    pub(super) local_repo: String,
    pub(super) remote_repo: String,
    /// Immutable id returned by the remote code-review provider.
    #[serde(default)]
    pub(super) repo_project_id: String,
    /// Provider selected from `remote_repo`: gitee, gitlab, or github.
    #[serde(default)]
    pub(super) repo_type: String,
    #[serde(default)]
    pub(super) logo: String,
    #[serde(default)]
    pub(super) project_tags: Vec<String>,
    #[serde(default)]
    pub(super) is_pr_auto_update: bool,
    #[serde(skip, default)]
    pub(super) pr_overview: ProjectPrOverview,
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectRepositoryMetadata {
    pub(crate) repo_project_id: String,
    pub(crate) repo_type: String,
    pub(crate) remote_repo: String,
}
#[derive(Debug, Clone, Default)]
pub(super) struct ProjectPrOverview {
    pub(super) loading: bool,
    pub(super) failed: bool,
    pub(super) fat_display: String,
    pub(super) uat_display: String,
    pub(super) prod_display: String,
}

#[derive(Debug, Clone)]
pub(super) struct ProjectDraft {
    pub(super) project_name: String,
    pub(super) project_alias: String,
    pub(super) local_repo: String,
    pub(super) remote_repo: String,
    pub(super) logo: String,
    pub(super) project_tags: Vec<String>,
    pub(super) is_pr_auto_update: bool,
}

#[derive(Debug, Clone)]
pub(super) enum ProjectSaveAction {
    Add,
    Update { original_alias: String },
}

impl ProjectSaveAction {
    pub(super) fn pending_key(&self, alias: &str) -> String {
        match self {
            Self::Add => format!("add:{alias}"),
            Self::Update { original_alias } => format!("update:{original_alias}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProjectViewDiagSnapshot {
    pub(super) query: String,
    pub(super) configured_items: usize,
    pub(super) visible_items: usize,
    pub(super) remote_logo_prefetch: usize,
    pub(super) pending_pr_tasks: usize,
    pub(super) local_logos: usize,
    pub(super) remote_logos: usize,
    pub(super) base64_logos: usize,
    pub(super) default_logos: usize,
    pub(super) logo_source_bytes: u64,
    pub(super) logo_decoded_rgba_bytes_estimate: u64,
}

pub(super) fn parse_project_tags(value: &str) -> Vec<String> {
    let raw_tags = value
        .split([',', '，', ';', '；'])
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(normalize_project_tag)
        .collect::<Vec<_>>();
    normalize_project_tags(&raw_tags)
}

pub(super) fn normalize_project_tags(tags: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for tag in tags {
        let normalized_tag = normalize_project_tag(tag);
        if normalized_tag.is_empty() {
            continue;
        }
        let lower = normalized_tag.to_lowercase();
        if seen.insert(lower) {
            normalized.push(normalized_tag);
        }
    }
    normalized
}

fn normalize_project_tag(value: impl AsRef<str>) -> String {
    let trimmed = value.as_ref().trim();
    match trimmed.to_ascii_lowercase().as_str() {
        "" => String::new(),
        "h5" => "H5".to_string(),
        "web" => "Web".to_string(),
        "rn" | "react native" | "react-native" => "RN".to_string(),
        other => {
            let mut chars = other.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            first.to_ascii_uppercase().to_string() + chars.as_str()
        }
    }
}
