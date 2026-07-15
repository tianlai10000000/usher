pub(crate) mod pr_stats;

use crate::http::{self, RequestOptions};
use crate::project::pr_stats::{PullRequestStateFilter, fetch_standard_environment_pr_counts};
use crate::terminal::diag_logging_enabled;
use crate::terminal::open_terminal_for_project;
use crate::{MainWindow, ProjectItem, ToastManager, ToastOptions};
use base64::Engine as _;
use directories::ProjectDirs;
use image::GenericImageView;
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, Global, ModelRc, VecModel};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

thread_local! {
    static PROJECT_LOGO_IMAGE_CACHE: RefCell<HashMap<String, slint::Image>> =
        RefCell::new(HashMap::new());
    static PROJECT_VIEW_DIAG_CACHE: RefCell<Option<ProjectViewDiagSnapshot>> =
        const { RefCell::new(None) };
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectConfig {
    project_name: String,
    project_alias: String,
    local_repo: String,
    remote_repo: String,
    /// Immutable id returned by the remote code-review provider.
    #[serde(default)]
    repo_project_id: String,
    /// Provider selected from `remote_repo`: gitee, gitlab, or github.
    #[serde(default)]
    repo_type: String,
    #[serde(default)]
    logo: String,
    #[serde(default)]
    project_tags: Vec<String>,
    #[serde(default)]
    is_pr_auto_update: bool,
    #[serde(skip, default)]
    pr_overview: ProjectPrOverview,
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectRepositoryMetadata {
    pub(crate) repo_project_id: String,
    pub(crate) repo_type: String,
    pub(crate) remote_repo: String,
}

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

#[derive(Debug, Clone, Default)]
struct ProjectPrOverview {
    loading: bool,
    failed: bool,
    fat_display: String,
    uat_display: String,
    prod_display: String,
}

#[derive(Debug, Clone)]
struct ProjectDraft {
    project_name: String,
    project_alias: String,
    local_repo: String,
    remote_repo: String,
    logo: String,
    project_tags: Vec<String>,
    is_pr_auto_update: bool,
}

#[derive(Debug, Clone)]
enum ProjectSaveAction {
    Add,
    Update { original_alias: String },
}

impl ProjectSaveAction {
    fn pending_key(&self, alias: &str) -> String {
        match self {
            Self::Add => format!("add:{alias}"),
            Self::Update { original_alias } => format!("update:{original_alias}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectViewDiagSnapshot {
    query: String,
    configured_items: usize,
    visible_items: usize,
    remote_logo_prefetch: usize,
    pending_pr_tasks: usize,
    local_logos: usize,
    remote_logos: usize,
    base64_logos: usize,
    default_logos: usize,
    logo_source_bytes: u64,
    logo_decoded_rgba_bytes_estimate: u64,
}

pub fn setup_project(ui: &MainWindow) {
    let projects = load_projects();
    let state = Arc::new(Mutex::new(projects));
    let pending_remote_logos = Arc::new(Mutex::new(HashSet::<String>::new()));
    let pending_pr_overview = Arc::new(Mutex::new(HashSet::<String>::new()));
    let pending_project_saves = Arc::new(Mutex::new(HashSet::<String>::new()));

    refresh_project_items(
        ui,
        &state,
        &pending_remote_logos,
        &pending_pr_overview,
        &ui.get_project_search_query(),
    );
    ui.set_project_logo_preview_image(resolve_project_logo_image(""));
    ui.set_project_logo_preview_status("Using app default logo".into());

    let ui_weak = ui.as_weak();
    let pending_preview_logos = pending_remote_logos.clone();
    ui.on_project_preview_logo(move |logo| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        refresh_logo_preview(&ui, &pending_preview_logos, logo.as_str());
    });

    let ui_weak = ui.as_weak();
    let state_add = state.clone();
    let pending_remote_logos_add = pending_remote_logos.clone();
    let pending_pr_overview_add = pending_pr_overview.clone();
    let pending_project_saves_add = pending_project_saves.clone();
    ui.on_project_add_project(
        move |name, alias, local_repo, remote_repo, logo, project_tags, is_pr_auto_update| {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };

            log::info!(
                "[Project] Add project requested: name='{}', alias='{}'",
                name,
                alias
            );

            let name = name.trim();
            let alias = alias.trim();
            let local_repo = local_repo.trim();
            let remote_repo = remote_repo.trim();
            let logo = normalize_project_logo(logo.as_str());
            let project_tags = parse_project_tags(project_tags.as_str());

            if name.is_empty()
                || alias.is_empty()
                || local_repo.is_empty()
                || remote_repo.is_empty()
            {
                show_error(&ui, "Please fill in all fields.");
                return;
            }

            let guard = match state_add.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    show_error(&ui, "Failed to update project list.");
                    return;
                }
            };

            if guard.iter().any(|item| item.project_alias == alias) {
                show_error(&ui, "Project alias already exists.");
                return;
            }

            drop(guard);
            start_project_save(
                ProjectSaveAction::Add,
                ProjectDraft {
                    project_name: name.to_string(),
                    project_alias: alias.to_string(),
                    local_repo: local_repo.to_string(),
                    remote_repo: remote_repo.to_string(),
                    logo,
                    project_tags,
                    is_pr_auto_update,
                },
                ui.as_weak(),
                state_add.clone(),
                pending_remote_logos_add.clone(),
                pending_pr_overview_add.clone(),
                pending_project_saves_add.clone(),
            );
        },
    );

    let ui_weak = ui.as_weak();
    let state_update = state.clone();
    let pending_remote_logos_update = pending_remote_logos.clone();
    let pending_pr_overview_update = pending_pr_overview.clone();
    let pending_project_saves_update = pending_project_saves.clone();
    ui.on_project_update_project(
        move |original_alias,
              name,
              alias,
              local_repo,
              remote_repo,
              logo,
              project_tags,
              is_pr_auto_update| {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };

            log::info!(
                "[Project] Update project requested: original_alias='{}', alias='{}'",
                original_alias,
                alias
            );

            let original_alias = original_alias.trim();
            let name = name.trim();
            let alias = alias.trim();
            let local_repo = local_repo.trim();
            let remote_repo = remote_repo.trim();
            let logo = normalize_project_logo(logo.as_str());
            let project_tags = parse_project_tags(project_tags.as_str());

            if original_alias.is_empty() {
                show_error(&ui, "Missing original project alias.");
                return;
            }
            if name.is_empty()
                || alias.is_empty()
                || local_repo.is_empty()
                || remote_repo.is_empty()
            {
                show_error(&ui, "Please fill in all fields.");
                return;
            }

            let guard = match state_update.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    show_error(&ui, "Failed to update project list.");
                    return;
                }
            };

            if !guard
                .iter()
                .any(|item| item.project_alias == original_alias)
            {
                show_error(&ui, "Project not found.");
                return;
            }

            if alias != original_alias && guard.iter().any(|item| item.project_alias == alias) {
                show_error(&ui, "Project alias already exists.");
                return;
            }

            drop(guard);
            start_project_save(
                ProjectSaveAction::Update {
                    original_alias: original_alias.to_string(),
                },
                ProjectDraft {
                    project_name: name.to_string(),
                    project_alias: alias.to_string(),
                    local_repo: local_repo.to_string(),
                    remote_repo: remote_repo.to_string(),
                    logo,
                    project_tags,
                    is_pr_auto_update,
                },
                ui.as_weak(),
                state_update.clone(),
                pending_remote_logos_update.clone(),
                pending_pr_overview_update.clone(),
                pending_project_saves_update.clone(),
            );
        },
    );

    let ui_weak = ui.as_weak();
    let state_delete = state.clone();
    let pending_remote_logos_delete = pending_remote_logos.clone();
    let pending_pr_overview_delete = pending_pr_overview.clone();
    ui.on_project_delete_project(move |alias| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };

        log::info!("[Project] Delete project requested: alias='{}'", alias);

        let alias = alias.trim();
        if alias.is_empty() {
            show_error(&ui, "Missing project alias.");
            return;
        }

        let mut guard = match state_delete.lock() {
            Ok(guard) => guard,
            Err(_) => {
                show_error(&ui, "Failed to update project list.");
                return;
            }
        };

        let Some(index) = guard.iter().position(|item| item.project_alias == alias) else {
            show_error(&ui, "Project not found.");
            return;
        };

        guard.remove(index);
        if let Err(err) = write_projects(&guard) {
            show_error(&ui, &format!("Failed to save project: {err}"));
            return;
        }

        drop(guard);
        let query = ui.get_project_search_query();
        refresh_project_items(
            &ui,
            &state_delete,
            &pending_remote_logos_delete,
            &pending_pr_overview_delete,
            &query,
        );
        show_success(&ui, "Project deleted.");
    });

    let ui_weak = ui.as_weak();
    let state_search = state.clone();
    let pending_remote_logos_search = pending_remote_logos.clone();
    let pending_pr_overview_search = pending_pr_overview.clone();
    ui.on_project_search_projects(move |query| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };

        refresh_project_items(
            &ui,
            &state_search,
            &pending_remote_logos_search,
            &pending_pr_overview_search,
            &query,
        );
    });

    let ui_weak = ui.as_weak();
    let state_reload = state.clone();
    let pending_remote_logos_reload = pending_remote_logos.clone();
    let pending_pr_overview_reload = pending_pr_overview.clone();
    ui.on_project_reload_projects(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };

        log::info!("[Project] Reload projects requested.");
        let list = load_projects();

        let mut guard = match state_reload.lock() {
            Ok(guard) => guard,
            Err(_) => {
                show_error(&ui, "Failed to reload projects.");
                return;
            }
        };

        *guard = list;
        drop(guard);
        let query = ui.get_project_search_query();
        refresh_project_items(
            &ui,
            &state_reload,
            &pending_remote_logos_reload,
            &pending_pr_overview_reload,
            &query,
        );
        show_success(&ui, "Projects reloaded.");
    });

    let ui_weak = ui.as_weak();
    ui.on_project_open_terminal(move |project_name, project_alias, local_repo| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };

        open_terminal_for_project(
            &ui,
            project_name.trim(),
            project_alias.trim(),
            local_repo.trim(),
        );
    });
}

fn start_project_save(
    action: ProjectSaveAction,
    draft: ProjectDraft,
    ui_weak: slint::Weak<MainWindow>,
    state: Arc<Mutex<Vec<ProjectConfig>>>,
    pending_remote_logos: Arc<Mutex<HashSet<String>>>,
    pending_pr_overview: Arc<Mutex<HashSet<String>>>,
    pending_project_saves: Arc<Mutex<HashSet<String>>>,
) {
    let pending_key = action.pending_key(draft.project_alias.as_str());
    let inserted = pending_project_saves
        .lock()
        .map(|mut saves| saves.insert(pending_key.clone()))
        .unwrap_or(false);
    if !inserted {
        if let Some(ui) = ui_weak.upgrade() {
            show_error(&ui, "A save for this project is already in progress.");
        }
        return;
    }

    crate::app_runtime::shared_runtime().spawn(async move {
        let details = crate::codereview::api::get_repo_detail(draft.remote_repo.as_str()).await;
        let _ = slint::invoke_from_event_loop(move || {
            if let Ok(mut saves) = pending_project_saves.lock() {
                saves.remove(pending_key.as_str());
            }
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };

            let details = match details {
                Ok(details) => details,
                Err(error) => {
                    log::warn!(
                        "[Project] Remote detail lookup failed for '{}': {}",
                        draft.remote_repo,
                        error
                    );
                    show_error(
                        &ui,
                        "Unable to load remote project details. Check the remote URL and provider token.",
                    );
                    return;
                }
            };

            let mut projects = match state.lock() {
                Ok(projects) => projects,
                Err(_) => {
                    show_error(&ui, "Failed to update project list.");
                    return;
                }
            };

            let config = ProjectConfig {
                project_name: draft.project_name,
                project_alias: draft.project_alias,
                local_repo: draft.local_repo,
                remote_repo: draft.remote_repo,
                repo_project_id: details.repo_project_id,
                repo_type: details.repo_type.as_str().to_string(),
                logo: draft.logo,
                project_tags: draft.project_tags,
                is_pr_auto_update: draft.is_pr_auto_update,
                pr_overview: ProjectPrOverview::default(),
            };

            let outcome = match action {
                ProjectSaveAction::Add => {
                    if projects
                        .iter()
                        .any(|item| item.project_alias == config.project_alias)
                    {
                        Err("Project alias already exists.".to_string())
                    } else {
                        projects.push(config);
                        if let Err(error) = write_projects(&projects) {
                            projects.pop();
                            Err(format!("Failed to save project: {error}"))
                        } else {
                            Ok("Project added.")
                        }
                    }
                }
                ProjectSaveAction::Update { original_alias } => {
                    let Some(index) = projects
                        .iter()
                        .position(|item| item.project_alias == original_alias)
                    else {
                        show_error(&ui, "Project not found.");
                        return;
                    };
                    if config.project_alias != original_alias
                        && projects
                            .iter()
                            .any(|item| item.project_alias == config.project_alias)
                    {
                        Err("Project alias already exists.".to_string())
                    } else {
                        let previous = std::mem::replace(&mut projects[index], config);
                        if let Err(error) = write_projects(&projects) {
                            projects[index] = previous;
                            Err(format!("Failed to save project: {error}"))
                        } else {
                            Ok("Project updated.")
                        }
                    }
                }
            };

            let message = match outcome {
                Ok(message) => message,
                Err(message) => {
                    show_error(&ui, message.as_str());
                    return;
                }
            };
            drop(projects);
            let query = ui.get_project_search_query();
            refresh_project_items(
                &ui,
                &state,
                &pending_remote_logos,
                &pending_pr_overview,
                query.as_str(),
            );
            show_success(&ui, message);
        });
    });
}

fn load_projects() -> Vec<ProjectConfig> {
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

fn write_projects(projects: &[ProjectConfig]) -> std::io::Result<()> {
    let Some(path) = projects_file_path() else {
        return Ok(());
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let data = serde_json::to_string_pretty(projects).unwrap_or_else(|_| "[]".to_string());
    fs::write(path, data)
}

fn refresh_project_items(
    ui: &MainWindow,
    state: &Arc<Mutex<Vec<ProjectConfig>>>,
    pending_remote_logos: &Arc<Mutex<HashSet<String>>>,
    pending_pr_overview: &Arc<Mutex<HashSet<String>>>,
    query: &str,
) {
    let Ok(mut guard) = state.lock() else {
        return;
    };
    let pr_tasks = collect_pr_overview_tasks(&mut guard, query);
    let (ui_items, remote_urls) = prepare_project_items(&guard, query);
    log_project_view_diagnostics(&guard, query, &ui_items, &remote_urls, pr_tasks.len());
    drop(guard);
    ui.set_project_items(ModelRc::new(VecModel::from(ui_items)));
    prefetch_remote_logos(
        ui.as_weak(),
        state.clone(),
        pending_remote_logos.clone(),
        pending_pr_overview.clone(),
        remote_urls,
    );
    prefetch_project_pr_overview(
        ui.as_weak(),
        state.clone(),
        pending_remote_logos.clone(),
        pending_pr_overview.clone(),
        pr_tasks,
    );
}

fn log_project_view_diagnostics(
    items: &[ProjectConfig],
    query: &str,
    ui_items: &[ProjectItem],
    remote_urls: &[String],
    pr_task_count: usize,
) {
    if !diag_logging_enabled() {
        return;
    }

    let normalized_query = query.trim().to_lowercase();

    let mut local_logo_count = 0usize;
    let mut remote_logo_count = 0usize;
    let mut base64_logo_count = 0usize;
    let mut default_logo_count = 0usize;
    let mut logo_source_bytes = 0u64;
    let mut logo_decoded_rgba_bytes_estimate = 0u64;

    for item in items
        .iter()
        .filter(|item| matches_query(item, normalized_query.as_str()))
    {
        let normalized = normalize_project_logo(item.logo.as_str());
        if normalized.is_empty() || normalized == default_logo_value() {
            default_logo_count += 1;
            continue;
        }

        if is_valid_logo_path(&normalized) {
            local_logo_count += 1;
            logo_source_bytes += fs::metadata(&normalized).map(|m| m.len()).unwrap_or(0);
            logo_decoded_rgba_bytes_estimate +=
                estimate_logo_decoded_rgba_bytes_from_path(Path::new(&normalized));
            continue;
        }

        if is_valid_logo_remote_url(&normalized) {
            remote_logo_count += 1;
            if let Some(path) = find_cached_remote_logo_path(&normalized) {
                logo_source_bytes += fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                logo_decoded_rgba_bytes_estimate +=
                    estimate_logo_decoded_rgba_bytes_from_path(path.as_path());
            }
            continue;
        }

        if is_valid_logo_base64(&normalized) {
            base64_logo_count += 1;
            let payload = normalized
                .split_once(',')
                .map(|(_, encoded)| encoded)
                .unwrap_or(normalized.as_str());
            let payload = sanitize_base64_payload(payload);
            logo_source_bytes += ((payload.len() as u64) * 3) / 4;
            logo_decoded_rgba_bytes_estimate +=
                estimate_logo_decoded_rgba_bytes_from_base64(&normalized);
            continue;
        }

        default_logo_count += 1;
    }

    let snapshot = ProjectViewDiagSnapshot {
        query: query.to_string(),
        configured_items: items.len(),
        visible_items: ui_items.len(),
        remote_logo_prefetch: remote_urls.len(),
        pending_pr_tasks: pr_task_count,
        local_logos: local_logo_count,
        remote_logos: remote_logo_count,
        base64_logos: base64_logo_count,
        default_logos: default_logo_count,
        logo_source_bytes,
        logo_decoded_rgba_bytes_estimate,
    };

    let should_log = PROJECT_VIEW_DIAG_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.as_ref() == Some(&snapshot) {
            false
        } else {
            *cache = Some(snapshot.clone());
            true
        }
    });

    if should_log {
        log::info!(
            "[Project][Diag] query='{}', configured_items={}, visible_items={}, remote_logo_prefetch={}, pending_pr_tasks={}, local_logos={}, remote_logos={}, base64_logos={}, default_logos={}, logo_source_bytes={}, logo_decoded_rgba_bytes_estimate={}",
            snapshot.query,
            snapshot.configured_items,
            snapshot.visible_items,
            snapshot.remote_logo_prefetch,
            snapshot.pending_pr_tasks,
            snapshot.local_logos,
            snapshot.remote_logos,
            snapshot.base64_logos,
            snapshot.default_logos,
            snapshot.logo_source_bytes,
            snapshot.logo_decoded_rgba_bytes_estimate,
        );
    }
}

fn estimate_logo_decoded_rgba_bytes_from_path(path: &Path) -> u64 {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "svg" {
        return 0;
    }
    let Ok(bytes) = fs::read(path) else {
        return 0;
    };
    estimate_logo_decoded_rgba_bytes_from_bytes(bytes.as_slice())
}

fn estimate_logo_decoded_rgba_bytes_from_base64(value: &str) -> u64 {
    let payload = value
        .split_once(',')
        .map(|(_, encoded)| encoded)
        .unwrap_or(value);
    let payload = sanitize_base64_payload(payload);
    let Ok(decoded) = base64::engine::general_purpose::STANDARD
        .decode(payload.as_bytes())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload.as_bytes()))
    else {
        return 0;
    };
    estimate_logo_decoded_rgba_bytes_from_bytes(decoded.as_slice())
}

fn estimate_logo_decoded_rgba_bytes_from_bytes(bytes: &[u8]) -> u64 {
    let Ok(image) = image::load_from_memory(bytes) else {
        return 0;
    };
    let (width, height) = image.dimensions();
    width as u64 * height as u64 * 4
}

fn prepare_project_items(items: &[ProjectConfig], query: &str) -> (Vec<ProjectItem>, Vec<String>) {
    (
        to_ui_items_filtered(items, query),
        collect_remote_logo_urls(items, query),
    )
}

fn to_ui_items_filtered(items: &[ProjectConfig], query: &str) -> Vec<ProjectItem> {
    let needle = query.trim().to_lowercase();
    items
        .iter()
        .filter(|item| matches_query(item, &needle))
        .cloned()
        .map(|item| {
            let logo = item.logo;
            ProjectItem {
                project_name: item.project_name.into(),
                project_alias: item.project_alias.into(),
                local_repo: item.local_repo.into(),
                remote_repo: item.remote_repo.into(),
                logo: logo.clone().into(),
                logo_image: resolve_project_logo_image(logo.as_str()),
                is_pr_auto_update: item.is_pr_auto_update,
                pr_count_loading: item.pr_overview.loading,
                pr_count_failed: item.pr_overview.failed,
                pr_fat_count_text: pr_count_display(
                    &item.pr_overview.fat_display,
                    item.pr_overview.failed,
                )
                .into(),
                pr_uat_count_text: pr_count_display(
                    &item.pr_overview.uat_display,
                    item.pr_overview.failed,
                )
                .into(),
                pr_prod_count_text: pr_count_display(
                    &item.pr_overview.prod_display,
                    item.pr_overview.failed,
                )
                .into(),
                project_tags_text: item.project_tags.join(", ").into(),
                project_tags: ModelRc::new(VecModel::from(
                    item.project_tags
                        .into_iter()
                        .map(Into::into)
                        .collect::<Vec<_>>(),
                )),
            }
        })
        .collect()
}

fn collect_remote_logo_urls(items: &[ProjectConfig], query: &str) -> Vec<String> {
    let needle = query.trim().to_lowercase();
    items
        .iter()
        .filter(|item| matches_query(item, &needle))
        .filter_map(|item| {
            if is_valid_logo_remote_url(item.logo.as_str()) {
                Some(item.logo.clone())
            } else {
                None
            }
        })
        .collect()
}

fn matches_query(item: &ProjectConfig, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let name = item.project_name.to_lowercase();
    let alias = item.project_alias.to_lowercase();
    name.contains(needle) || alias.contains(needle)
}

fn pr_count_display(value: &str, failed: bool) -> &str {
    if failed {
        "unknown"
    } else if value.is_empty() {
        "-"
    } else {
        value
    }
}

#[derive(Debug, Clone)]
struct ProjectPrOverviewTask {
    project_alias: String,
    remote_repo: String,
}

fn collect_pr_overview_tasks(
    items: &mut [ProjectConfig],
    query: &str,
) -> Vec<ProjectPrOverviewTask> {
    let needle = query.trim().to_lowercase();
    let mut tasks = Vec::new();

    for item in items.iter_mut().filter(|item| matches_query(item, &needle)) {
        if !item.is_pr_auto_update || item.pr_overview.loading || item.pr_overview.failed {
            continue;
        }
        if !item.pr_overview.fat_display.is_empty()
            && !item.pr_overview.uat_display.is_empty()
            && !item.pr_overview.prod_display.is_empty()
        {
            continue;
        }

        item.pr_overview.loading = true;
        item.pr_overview.failed = false;
        item.pr_overview.fat_display = "-".to_string();
        item.pr_overview.uat_display = "-".to_string();
        item.pr_overview.prod_display = "-".to_string();
        tasks.push(ProjectPrOverviewTask {
            project_alias: item.project_alias.clone(),
            remote_repo: item.remote_repo.clone(),
        });
    }

    tasks
}

pub(crate) fn normalize_project_logo(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if is_valid_logo_path(trimmed)
        || is_valid_logo_remote_url(trimmed)
        || is_valid_logo_base64(trimmed)
    {
        return trimmed.to_string();
    }

    default_logo_value()
}

fn parse_project_tags(value: &str) -> Vec<String> {
    let raw_tags = value
        .split([',', '，', ';', '；'])
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(normalize_project_tag)
        .collect::<Vec<_>>();
    normalize_project_tags(&raw_tags)
}

fn normalize_project_tags(tags: &[String]) -> Vec<String> {
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

fn is_valid_logo_path(value: &str) -> bool {
    Path::new(value).exists()
}

fn is_valid_logo_remote_url(value: &str) -> bool {
    reqwest::Url::parse(value)
        .map(|url| {
            let scheme = url.scheme();
            scheme == "http" || scheme == "https"
        })
        .unwrap_or(false)
}

fn is_valid_logo_base64(value: &str) -> bool {
    let payload = if let Some((prefix, encoded)) = value.split_once(",") {
        if !prefix.to_ascii_lowercase().contains(";base64") {
            return false;
        }
        encoded
    } else {
        value
    };
    let payload = sanitize_base64_payload(payload);

    base64::engine::general_purpose::STANDARD
        .decode(payload.as_bytes())
        .is_ok()
        || base64::engine::general_purpose::URL_SAFE
            .decode(payload.as_bytes())
            .is_ok()
}

fn default_logo_value() -> String {
    "logo.png".to_string()
}

pub(crate) fn resolve_project_logo_image(value: &str) -> slint::Image {
    let normalized = normalize_project_logo(value);
    let cache_key = logo_image_cache_key(&normalized);

    if let Some(image) = cached_logo_image(&cache_key) {
        return image;
    }

    let image = if normalized.is_empty() {
        load_default_logo_image()
    } else if is_valid_logo_path(&normalized) {
        slint::Image::load_from_path(Path::new(&normalized))
            .unwrap_or_else(|_| load_default_logo_image())
    } else if is_valid_logo_remote_url(&normalized) {
        if let Some(image) = load_cached_remote_logo_image(&normalized) {
            image
        } else {
            load_default_logo_image()
        }
    } else if is_valid_logo_base64(&normalized) {
        if let Some(image) = load_base64_logo_image(&normalized) {
            image
        } else {
            load_default_logo_image()
        }
    } else {
        load_default_logo_image()
    };

    cache_logo_image(cache_key, image.clone());
    image
}

fn cached_logo_image(key: &str) -> Option<slint::Image> {
    PROJECT_LOGO_IMAGE_CACHE.with(|cache| cache.borrow().get(key).cloned())
}

fn cache_logo_image(key: String, image: slint::Image) {
    PROJECT_LOGO_IMAGE_CACHE.with(|cache| {
        cache.borrow_mut().insert(key, image);
    });
}

fn logo_image_cache_key(normalized: &str) -> String {
    if normalized.is_empty() || normalized == default_logo_value() {
        return "default".to_string();
    }

    if is_valid_logo_path(normalized) {
        return format!("path:{normalized}");
    }

    if is_valid_logo_remote_url(normalized) {
        return format!("remote:{normalized}");
    }

    if is_valid_logo_base64(normalized) {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        normalized.hash(&mut hasher);
        return format!("base64:{:016x}", hasher.finish());
    }

    format!("fallback:{normalized}")
}

fn load_default_logo_image() -> slint::Image {
    load_image_from_bytes(include_bytes!("../../logo.png"), "png")
        .unwrap_or_else(slint::Image::default)
}

fn load_cached_remote_logo_image(url: &str) -> Option<slint::Image> {
    let path = find_cached_remote_logo_path(url)?;
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let bytes = fs::read(&path).ok()?;
    load_image_from_bytes(bytes.as_slice(), ext.as_str())
}

fn load_base64_logo_image(value: &str) -> Option<slint::Image> {
    let (payload, ext) = if let Some((prefix, encoded)) = value.split_once(',') {
        let lower = prefix.to_ascii_lowercase();
        if !lower.contains(";base64") {
            return None;
        }
        let ext = infer_ext_from_data_prefix(prefix).unwrap_or("png");
        (encoded, ext)
    } else {
        (value, "png")
    };
    let payload = sanitize_base64_payload(payload);

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(payload.as_bytes())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload.as_bytes()))
        .ok()?;
    load_image_from_bytes(decoded.as_slice(), ext)
}

fn load_image_from_bytes(bytes: &[u8], ext: &str) -> Option<slint::Image> {
    if ext == "svg" {
        return load_svg_image_from_bytes(bytes, ext);
    }

    let decoded = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = decoded.dimensions();
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        decoded.as_raw().as_slice(),
        width,
        height,
    );
    Some(slint::Image::from_rgba8(buffer))
}

fn load_svg_image_from_bytes(bytes: &[u8], ext: &str) -> Option<slint::Image> {
    let cache_path = byte_cache_path(bytes, ext)?;
    slint::Image::load_from_path(&cache_path).ok()
}

fn sanitize_base64_payload(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect()
}

fn byte_cache_path(bytes: &[u8], ext: &str) -> Option<PathBuf> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    ext.hash(&mut hasher);
    let hash = hasher.finish();

    let cache_dir = logo_cache_dir()?;
    let cache_path = cache_dir.join(format!("{hash:016x}.{ext}"));
    if !cache_path.exists() {
        fs::write(&cache_path, bytes).ok()?;
    }
    Some(cache_path)
}

fn logo_cache_dir() -> Option<PathBuf> {
    let cache_dir = std::env::temp_dir().join("usher-logo-cache");
    fs::create_dir_all(&cache_dir).ok()?;
    Some(cache_dir)
}

fn remote_logo_cache_base(url: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn remote_logo_cache_path(url: &str, ext: &str) -> Option<PathBuf> {
    Some(logo_cache_dir()?.join(format!("remote-{}.{}", remote_logo_cache_base(url), ext)))
}

fn find_cached_remote_logo_path(url: &str) -> Option<PathBuf> {
    for ext in ["png", "jpg", "webp", "gif", "svg"] {
        let path = remote_logo_cache_path(url, ext)?;
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn prefetch_remote_logos(
    ui_weak: slint::Weak<MainWindow>,
    state: Arc<Mutex<Vec<ProjectConfig>>>,
    pending_remote_logos: Arc<Mutex<HashSet<String>>>,
    pending_pr_overview: Arc<Mutex<HashSet<String>>>,
    remote_urls: Vec<String>,
) {
    for url in remote_urls {
        if find_cached_remote_logo_path(url.as_str()).is_some() {
            continue;
        }

        let mut should_spawn = false;
        if let Ok(mut guard) = pending_remote_logos.lock() {
            should_spawn = guard.insert(url.clone());
        }
        if !should_spawn {
            continue;
        }

        let ui_weak_task = ui_weak.clone();
        let state_task = state.clone();
        let pending_logo_task = pending_remote_logos.clone();
        let pending_pr_task = pending_pr_overview.clone();
        std::thread::spawn(move || {
            let downloaded = download_remote_logo_to_cache(url.as_str()).is_some();
            if let Ok(mut guard) = pending_logo_task.lock() {
                guard.remove(url.as_str());
            }
            if !downloaded {
                return;
            }

            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak_task.upgrade() else {
                    return;
                };
                let query = ui.get_project_search_query();
                refresh_project_items(
                    &ui,
                    &state_task,
                    &pending_logo_task,
                    &pending_pr_task,
                    &query,
                );
            });
        });
    }
}

fn prefetch_project_pr_overview(
    ui_weak: slint::Weak<MainWindow>,
    state: Arc<Mutex<Vec<ProjectConfig>>>,
    pending_remote_logos: Arc<Mutex<HashSet<String>>>,
    pending_pr_overview: Arc<Mutex<HashSet<String>>>,
    tasks: Vec<ProjectPrOverviewTask>,
) {
    for task in tasks {
        let mut should_spawn = false;
        if let Ok(mut guard) = pending_pr_overview.lock() {
            should_spawn = guard.insert(task.project_alias.clone());
        }
        if !should_spawn {
            continue;
        }

        let ui_weak_task = ui_weak.clone();
        let state_task = state.clone();
        let pending_logo_task = pending_remote_logos.clone();
        let pending_task = pending_pr_overview.clone();
        std::thread::spawn(move || {
            let result = fetch_standard_environment_pr_counts(
                &task.remote_repo,
                PullRequestStateFilter::Open,
                None,
            );

            if let Ok(mut guard) = state_task.lock() {
                if let Some(item) = guard
                    .iter_mut()
                    .find(|item| item.project_alias == task.project_alias)
                {
                    item.pr_overview.loading = false;
                    match result {
                        Ok(counts) => {
                            item.pr_overview.failed = false;
                            item.pr_overview.fat_display = counts.fat.to_string();
                            item.pr_overview.uat_display = counts.uat.to_string();
                            item.pr_overview.prod_display = counts.prod.to_string();
                        }
                        Err(error) => {
                            log::warn!(
                                "[Project] Failed to load PR overview for '{}': {}",
                                task.project_alias,
                                error
                            );
                            item.pr_overview.failed = true;
                            item.pr_overview.fat_display.clear();
                            item.pr_overview.uat_display.clear();
                            item.pr_overview.prod_display.clear();
                        }
                    }
                }
            }

            if let Ok(mut guard) = pending_task.lock() {
                guard.remove(task.project_alias.as_str());
            }

            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak_task.upgrade() else {
                    return;
                };
                let query = ui.get_project_search_query();
                refresh_project_items(&ui, &state_task, &pending_logo_task, &pending_task, &query);
            });
        });
    }
}

fn download_remote_logo_to_cache(url: &str) -> Option<PathBuf> {
    let response = http::send_blocking(
        http::blocking_get(url),
        RequestOptions::new("Download project logo"),
    )
    .ok()?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = response.bytes().ok()?;
    let ext = infer_ext_from_content_type(content_type.as_str())
        .or_else(|| infer_ext_from_url(url))
        .unwrap_or("png");
    let cache_path = remote_logo_cache_path(url, ext)?;
    if !cache_path.exists() {
        fs::write(&cache_path, bytes.as_ref()).ok()?;
    }
    Some(cache_path)
}

fn refresh_logo_preview(
    ui: &MainWindow,
    pending_remote_logos: &Arc<Mutex<HashSet<String>>>,
    logo: &str,
) {
    let trimmed = logo.trim();
    let normalized = normalize_project_logo(logo);
    ui.set_project_logo_preview_status(preview_logo_status(trimmed).into());
    ui.set_project_logo_preview_image(resolve_project_logo_image(&normalized));

    if !is_valid_logo_remote_url(&normalized) || find_cached_remote_logo_path(&normalized).is_some()
    {
        return;
    }

    let mut should_spawn = false;
    if let Ok(mut guard) = pending_remote_logos.lock() {
        should_spawn = guard.insert(normalized.clone());
    }
    if !should_spawn {
        return;
    }

    let ui_weak = ui.as_weak();
    let pending_task = pending_remote_logos.clone();
    std::thread::spawn(move || {
        let downloaded = download_remote_logo_to_cache(normalized.as_str()).is_some();
        if let Ok(mut guard) = pending_task.lock() {
            guard.remove(normalized.as_str());
        }
        if !downloaded {
            return;
        }

        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            if ui.get_project_new_logo().as_str() == normalized {
                ui.set_project_logo_preview_status("Remote URL".into());
                ui.set_project_logo_preview_image(resolve_project_logo_image(&normalized));
            }
        });
    });
}

fn preview_logo_status(value: &str) -> &'static str {
    if value.is_empty() {
        "Using app default logo"
    } else if is_valid_logo_path(value) {
        "Local file"
    } else if is_valid_logo_base64(value) {
        "Base64 image"
    } else if is_valid_logo_remote_url(value) {
        if find_cached_remote_logo_path(value).is_some() {
            "Remote URL"
        } else {
            "Remote URL, loading preview..."
        }
    } else {
        "Invalid input, using default logo"
    }
}

fn infer_ext_from_content_type(content_type: &str) -> Option<&'static str> {
    let lower = content_type.to_ascii_lowercase();
    if lower.contains("image/png") {
        Some("png")
    } else if lower.contains("image/jpeg") || lower.contains("image/jpg") {
        Some("jpg")
    } else if lower.contains("image/webp") {
        Some("webp")
    } else if lower.contains("image/gif") {
        Some("gif")
    } else if lower.contains("image/svg") {
        Some("svg")
    } else {
        None
    }
}

fn infer_ext_from_url(url: &str) -> Option<&'static str> {
    let lower = url.to_ascii_lowercase();
    if lower.contains(".png") {
        Some("png")
    } else if lower.contains(".jpg") || lower.contains(".jpeg") {
        Some("jpg")
    } else if lower.contains(".webp") {
        Some("webp")
    } else if lower.contains(".gif") {
        Some("gif")
    } else if lower.contains(".svg") {
        Some("svg")
    } else {
        None
    }
}

fn infer_ext_from_data_prefix(prefix: &str) -> Option<&'static str> {
    let lower = prefix.to_ascii_lowercase();
    if lower.starts_with("data:image/png") {
        Some("png")
    } else if lower.starts_with("data:image/jpeg") || lower.starts_with("data:image/jpg") {
        Some("jpg")
    } else if lower.starts_with("data:image/webp") {
        Some("webp")
    } else if lower.starts_with("data:image/gif") {
        Some("gif")
    } else if lower.starts_with("data:image/svg") {
        Some("svg")
    } else {
        None
    }
}

fn show_success(ui: &MainWindow, message: &str) {
    let manager = ToastManager::get(ui);
    manager.invoke_success(ToastOptions {
        message: message.into(),
        duration: 2200,
        position: "bottom".into(),
        icon: "".into(),
        overlay: false,
        close_on_click: false,
        loading_type: "".into(),
        width: "auto".into(),
    });
}

fn show_error(ui: &MainWindow, message: &str) {
    let manager = ToastManager::get(ui);
    manager.invoke_error(ToastOptions {
        message: message.into(),
        duration: 2600,
        position: "middle".into(),
        icon: "".into(),
        overlay: false,
        close_on_click: false,
        loading_type: "".into(),
        width: "auto".into(),
    });
}
