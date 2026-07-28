//! Project configuration persistence.

use std::{fs, path::PathBuf};

use directories::ProjectDirs;

use super::logo::normalize_project_logo;
use super::model::{ProjectConfig, ProjectRepositoryMetadata, normalize_project_tags};

pub(crate) fn load_repository_metadata(
    project_alias: &str,
) -> Result<ProjectRepositoryMetadata, String> {
    let alias = project_alias.trim();
    let project = load_projects()
        .into_iter()
        .find(|project| project.project_alias == alias)
        .ok_or_else(|| format!("Project configuration for '{alias}' was not found."))?;

    if project.repo_project_id.trim().is_empty() || project.repo_type.trim().is_empty() {
        return Err(
            "Repository metadata is missing. Save the project again to synchronize it.".to_string(),
        );
    }

    Ok(ProjectRepositoryMetadata {
        repo_project_id: project.repo_project_id,
        repo_type: project.repo_type,
        remote_repo: project.remote_repo,
    })
}

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
    let proj_dirs = ProjectDirs::from("com", "usher", "app")?;
    let base_dir = proj_dirs.data_local_dir();
    let project_dir = base_dir.join("project");
    Some(project_dir.join("projects.json"))
}

pub(super) fn write_projects(projects: &[ProjectConfig]) -> std::io::Result<()> {
    let Some(path) = projects_file_path() else {
        return Ok(());
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let data = serde_json::to_string_pretty(projects).unwrap_or_else(|_| "[]".to_string());
    fs::write(path, data)
}
