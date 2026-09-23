//! Project configuration persistence.

use std::{
    fs,
    path::{Path, PathBuf},
};

use super::logo::normalize_project_logo;
use super::model::{ProjectConfig, normalize_project_tags};

pub(super) fn load_projects() -> Vec<ProjectConfig> {
    let Some(path) = projects_file_path() else {
        return Vec::new();
    };

    if !path.exists() {
        return Vec::new();
    }

    match fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<Vec<ProjectConfig>>(&raw) {
            Ok(mut list) => {
                for item in &mut list {
                    item.logo = normalize_project_logo(item.logo.as_str());
                    item.project_tags = normalize_project_tags(&item.project_tags);
                }
                list
            }
            Err(err) => {
                log::error!("[Project] Failed to parse projects file: {}", err);
                Vec::new()
            }
        },
        Err(err) => {
            log::error!("[Project] Failed to read projects file: {}", err);
            Vec::new()
        }
    }
}

fn projects_file_path() -> Option<PathBuf> {
    let base_dir = crate::app_paths::data_local_dir()?;
    let project_dir = base_dir.join("project");
    Some(project_dir.join("projects.json"))
}

pub(super) fn write_projects(projects: &[ProjectConfig]) -> std::io::Result<()> {
    let Some(path) = projects_file_path() else {
        return Ok(());
    };
    write_projects_to(&path, projects)
}

fn write_projects_to(path: &Path, projects: &[ProjectConfig]) -> std::io::Result<()> {
    let data = serde_json::to_string_pretty(projects).unwrap_or_else(|_| "[]".to_string());
    usher_fs::atomic_write(path, data.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::super::model::ProjectPrOverview;
    use super::{ProjectConfig, write_projects_to};
    use std::fs;

    fn sample_project(alias: &str) -> ProjectConfig {
        ProjectConfig {
            project_name: alias.to_string(),
            project_alias: alias.to_string(),
            local_repo: String::new(),
            remote_repo: String::new(),
            repo_project_id: String::new(),
            repo_type: String::new(),
            logo: String::new(),
            project_tags: Vec::new(),
            is_pr_auto_update: false,
            hooks_enabled: false,
            hook_ids: Vec::new(),
            pr_overview: ProjectPrOverview::default(),
        }
    }

    #[test]
    fn legacy_projects_default_to_no_hooks_and_selection_survives_disk_round_trip() {
        let legacy = serde_json::json!({"project_name":"legacy", "project_alias":"legacy", "local_repo":"", "remote_repo":""});
        let mut project: ProjectConfig = serde_json::from_value(legacy).unwrap();
        assert!(!project.hooks_enabled);
        assert!(project.hook_ids.is_empty());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("projects.json");
        project.hook_ids = vec!["hook-one".into(), "hook-two".into()];
        project.hooks_enabled = true;
        write_projects_to(&path, &[project.clone()]).unwrap();
        let restored: Vec<ProjectConfig> =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(restored[0].hook_ids, project.hook_ids);
        assert!(restored[0].hooks_enabled);
        project.hook_ids.clear();
        write_projects_to(&path, &[project]).unwrap();
        let restored: Vec<ProjectConfig> =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(restored[0].hook_ids.is_empty());
        assert!(restored[0].hooks_enabled);
    }

    #[test]
    fn legacy_hook_selections_migrate_to_project_enabled() {
        let legacy = serde_json::json!({
            "project_name": "legacy",
            "project_alias": "legacy",
            "local_repo": "",
            "remote_repo": "",
            "hook_ids": ["hook-one"]
        });
        let project: ProjectConfig = serde_json::from_value(legacy).unwrap();
        assert!(project.hooks_enabled);
        assert_eq!(project.hook_ids, ["hook-one"]);
    }

    #[test]
    fn write_projects_replaces_the_registry_atomically() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "usher-project-storage-test-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("projects.json");

        write_projects_to(&path, &[sample_project("alpha")]).unwrap();
        write_projects_to(&path, &[sample_project("alpha"), sample_project("beta")]).unwrap();

        let parsed: Vec<ProjectConfig> =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "atomic replacement must not leave a temporary file"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
