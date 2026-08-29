//! Project configuration persistence.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

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
    let base_dir = crate::app_paths::data_local_dir()?;
    let project_dir = base_dir.join("project");
    Some(project_dir.join("projects.json"))
}

/// Serializes concurrent background writers so a slow write cannot interleave
/// with a newer one.
static PROJECTS_WRITE_LOCK: Mutex<()> = Mutex::new(());

pub(super) fn write_projects_from_state(state: &Mutex<Vec<ProjectConfig>>) -> std::io::Result<()> {
    let Some(path) = projects_file_path() else {
        return Ok(());
    };
    write_projects_from_state_to(&path, state)
}

#[cfg(test)]
fn write_projects_to(path: &Path, projects: &[ProjectConfig]) -> std::io::Result<()> {
    let _guard = PROJECTS_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_projects_to_locked(path, projects)
}

fn write_projects_from_state_to(
    path: &Path,
    state: &Mutex<Vec<ProjectConfig>>,
) -> std::io::Result<()> {
    let _guard = PROJECTS_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let snapshot = state
        .lock()
        .map_err(|_| std::io::Error::other("project registry lock poisoned"))?
        .clone();
    write_projects_to_locked(path, &snapshot)
}

fn write_projects_to_locked(path: &Path, projects: &[ProjectConfig]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let data = serde_json::to_string_pretty(projects).unwrap_or_else(|_| "[]".to_string());
    // Write to a temporary sibling and rename so readers never observe a
    // partially written registry.
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, data)?;

    #[cfg(target_os = "windows")]
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::super::model::ProjectPrOverview;
    use super::{
        PROJECTS_WRITE_LOCK, ProjectConfig, write_projects_from_state_to, write_projects_to,
    };
    use std::fs;
    use std::sync::{Arc, Mutex};

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
            pr_overview: ProjectPrOverview::default(),
        }
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
        assert!(
            !path.with_extension("json.tmp").exists(),
            "temporary file must be renamed away"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn queued_writers_cannot_overwrite_a_newer_project_snapshot() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "usher-project-order-test-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("projects.json");
        let state = Arc::new(Mutex::new(vec![sample_project("alpha")]));

        let write_gate = PROJECTS_WRITE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let first_state = state.clone();
        let first_path = path.clone();
        let first =
            std::thread::spawn(move || write_projects_from_state_to(&first_path, &first_state));
        state.lock().unwrap().push(sample_project("beta"));
        let second_state = state.clone();
        let second_path = path.clone();
        let second =
            std::thread::spawn(move || write_projects_from_state_to(&second_path, &second_state));
        drop(write_gate);

        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
        let parsed: Vec<ProjectConfig> =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            parsed
                .iter()
                .map(|item| item.project_alias.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "beta"]
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
