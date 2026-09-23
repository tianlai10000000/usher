use crate::git::get_standard_environment_pull_request_counts;
use crate::terminal::open_terminal_for_project;
use crate::{MainWindow, ToastManager, ToastOptions};
use slint::{ComponentHandle, Global};
use std::sync::{Arc, OnceLock};

use super::logo::*;
use super::logo_loader::LogoLoader;
use super::model::{
    ProjectConfig, ProjectDraft, ProjectPrOverview, ProjectSaveAction, parse_project_tags,
};
use super::overview::ProjectPrOverviewTask;
use super::state::ProjectStateFacade;
use super::storage::load_projects;

#[derive(Clone)]
pub(crate) struct ProjectController {
    state: ProjectStateFacade,
    logo_loader: Arc<LogoLoader>,
    projection: Arc<super::projection::ProjectProjection>,
}

impl ProjectController {
    pub(crate) fn shutdown(&self) {
        self.projection.shutdown();
        self.logo_loader.shutdown();
        self.state.shutdown();
    }
}

pub(crate) fn sync_language(ui: &MainWindow) {
    if !ui.get_project_list_error().is_empty() {
        ui.set_project_list_error(
            crate::i18n::tr("projects.registry_read_failed")
                .into_owned()
                .into(),
        );
    }
    super::projection::sync_language(ui);
}

pub fn setup_project(ui: &MainWindow) -> ProjectController {
    let state_slot = Arc::new(OnceLock::<ProjectStateFacade>::new());
    let state_slot_for_failure = state_slot.clone();
    let ui_weak_for_success = ui.as_weak();
    let ui_weak_for_failure = ui.as_weak();
    let state = ProjectStateFacade::open_deferred(
        Arc::new(move |message_key| {
            let ui_weak = ui_weak_for_success.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    show_success(&ui, crate::i18n::tr(message_key).as_ref());
                }
            });
        }),
        Arc::new(move |expected, rollback, error| {
            log::error!("[Project] Failed to persist project registry: {error}");
            let Some(state) = state_slot_for_failure.get().cloned() else {
                return;
            };
            if state.rollback_if_current(expected, rollback) {
                let ui_weak = ui_weak_for_failure.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        show_error(&ui, crate::i18n::tr("projects.save_failed").as_ref());
                    }
                });
            } else {
                log::warn!("[Project] Skipped stale persistence rollback");
            }
        }),
    )
    .expect("failed to start project persistence worker");
    let _ = state_slot.set(state.clone());
    super::state::register(&state);
    super::hook_selection::bind(ui, state.clone());
    ui.set_project_list_loading(true);

    let projection = Arc::new(super::projection::ProjectProjection::start(
        ui,
        state.clone(),
    ));

    let ui_weak = ui.as_weak();
    let logo_loader = Arc::new(LogoLoader::start(state.clone(), ui.as_weak()));
    let loader_preview = logo_loader.clone();
    ui.on_project_preview_logo(move |logo| {
        let Some(_ui) = ui_weak.upgrade() else {
            return;
        };
        let url = logo.as_str().trim();
        loader_preview.set_preview_url(Some(url.to_owned()));
    });

    let ui_weak = ui.as_weak();
    let state_add = state.clone();
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
            let logo = logo.trim().to_owned();
            let project_tags = parse_project_tags(project_tags.as_str());

            if name.is_empty()
                || alias.is_empty()
                || local_repo.is_empty()
                || remote_repo.is_empty()
            {
                show_error(&ui, crate::i18n::tr("projects.fill_all_fields").as_ref());
                return;
            }

            if state_add.contains_alias(alias) {
                show_error(&ui, crate::i18n::tr("projects.alias_exists").as_ref());
                return;
            }
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
                    hooks_enabled: ui.get_project_hooks_enabled(),
                    hook_ids: super::hook_selection::selected_ids(&ui),
                },
                ui.as_weak(),
                state_add.clone(),
            );
        },
    );

    let ui_weak = ui.as_weak();
    let state_update = state.clone();
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
            let logo = logo.trim().to_owned();
            let project_tags = parse_project_tags(project_tags.as_str());

            if original_alias.is_empty() {
                show_error(
                    &ui,
                    crate::i18n::tr("projects.original_alias_missing").as_ref(),
                );
                return;
            }
            if name.is_empty()
                || alias.is_empty()
                || local_repo.is_empty()
                || remote_repo.is_empty()
            {
                show_error(&ui, crate::i18n::tr("projects.fill_all_fields").as_ref());
                return;
            }

            if !state_update.contains_alias(original_alias) {
                show_error(&ui, crate::i18n::tr("projects.not_found").as_ref());
                return;
            }

            if alias != original_alias && state_update.contains_alias(alias) {
                show_error(&ui, crate::i18n::tr("projects.alias_exists").as_ref());
                return;
            }
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
                    hooks_enabled: ui.get_project_hooks_enabled(),
                    hook_ids: super::hook_selection::selected_ids(&ui),
                },
                ui.as_weak(),
                state_update.clone(),
            );
        },
    );

    let ui_weak = ui.as_weak();
    let state_delete = state.clone();
    ui.on_project_delete_project(move |alias| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };

        log::info!("[Project] Delete project requested: alias='{}'", alias);

        let alias = alias.trim();
        if alias.is_empty() {
            show_error(&ui, crate::i18n::tr("projects.alias_missing").as_ref());
            return;
        }

        let state = state_delete.clone();
        let alias = alias.to_owned();
        let weak = ui.as_weak();
        crate::app_runtime::shared_runtime().spawn(async move {
            if let Err(error) = state.delete(alias) {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        show_error(&ui, crate::i18n::tr(&error).as_ref());
                    }
                });
            }
        });
    });

    let ui_weak = ui.as_weak();
    let loader_search = logo_loader.clone();
    ui.on_project_search_projects(move |query| {
        let Some(_ui) = ui_weak.upgrade() else {
            return;
        };

        loader_search.set_query(query.as_str());
    });

    let ui_weak = ui.as_weak();
    let state_reload = state.clone();
    ui.on_project_reload_projects(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };

        log::info!("[Project] Reload projects requested.");
        ui.set_project_list_error("".into());
        ui.set_project_list_loading(true);

        let ui_weak_task = ui.as_weak();
        let state_task = state_reload.clone();
        crate::app_runtime::shared_runtime().spawn(async move {
            let loaded = tokio::task::spawn_blocking(load_projects).await;
            let error = match loaded {
                Ok(projects) => {
                    state_task.hydrate(projects);
                    None
                }
                Err(error) => Some(error),
            };
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak_task.upgrade() {
                    if let Some(error) = error {
                        log::error!("[Project] Reload task failed: {error}");
                        ui.set_project_list_loading(false);
                        ui.set_project_list_error(
                            crate::i18n::tr("projects.registry_read_failed")
                                .into_owned()
                                .into(),
                        );
                        show_error(&ui, crate::i18n::tr("projects.reload_failed").as_ref());
                    } else {
                        show_success(&ui, crate::i18n::tr("projects.reloaded").as_ref());
                    }
                }
            });
        });
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

    let ui_weak = ui.as_weak();
    let state_initial = state.clone();
    crate::startup::run_after_first_frame(move || {
        crate::app_runtime::shared_runtime().spawn(async move {
            match tokio::task::spawn_blocking(load_projects).await {
                Ok(projects) => state_initial.hydrate(projects),
                Err(error) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        log::error!("[Project] Initial load worker failed: {error}");
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.set_project_list_loading(false);
                            ui.set_project_list_error(
                                crate::i18n::tr("projects.registry_read_failed")
                                    .into_owned()
                                    .into(),
                            );
                        }
                    });
                }
            }
        });
    });

    super::benchmark::setup(ui, &state);
    super::http_benchmark::setup(ui);
    ProjectController {
        state,
        logo_loader,
        projection,
    }
}

fn start_project_save(
    action: ProjectSaveAction,
    draft: ProjectDraft,
    ui_weak: slint::Weak<MainWindow>,
    state: ProjectStateFacade,
) {
    crate::app_runtime::shared_runtime().spawn(async move {
        let pending_key = action.pending_key(&draft.project_alias);
        let result = if !state.begin_save(pending_key.clone()) {
            Err("projects.save_in_progress".to_owned())
        } else {
            let saved_details = match &action {
                ProjectSaveAction::Update { original_alias } => state
                    .snapshot()
                    .state()
                    .projects
                    .iter()
                    .find(|project| project.project_alias == original_alias.as_str())
                    .filter(|project| project.remote_repo == draft.remote_repo)
                    .and_then(|project| {
                        let repo_type = match project.repo_type.as_str() {
                            "gitee" => crate::git::RepoType::Gitee,
                            "gitlab" => crate::git::RepoType::GitLab,
                            "github" => crate::git::RepoType::GitHub,
                            _ => return None,
                        };
                        if project.repo_project_id.trim().is_empty() {
                            return None;
                        }
                        Some(crate::git::RepoDetail {
                            repo_project_id: project.repo_project_id.clone(),
                            repo_type,
                        })
                    }),
                ProjectSaveAction::Add => None,
            };
            let result = match match saved_details {
                Some(details) => Ok(details),
                None => crate::git::get_repo_detail(&draft.remote_repo).await,
            } {
                Ok(details) => {
                    let config = ProjectConfig {
                        project_name: draft.project_name,
                        project_alias: draft.project_alias,
                        local_repo: draft.local_repo,
                        remote_repo: draft.remote_repo,
                        repo_project_id: details.repo_project_id,
                        hooks_enabled: draft.hooks_enabled,
                        hook_ids: draft.hook_ids,
                        repo_type: details.repo_type.as_str().to_owned(),
                        logo: normalize_project_logo(&draft.logo),
                        project_tags: draft.project_tags,
                        is_pr_auto_update: draft.is_pr_auto_update,
                        pr_overview: ProjectPrOverview::default(),
                    };
                    match action {
                        ProjectSaveAction::Add => state.add(config),
                        ProjectSaveAction::Update { original_alias } => {
                            state.update(original_alias, config)
                        }
                    }
                }
                Err(error) => {
                    log::warn!("[Project] Remote detail lookup failed: {error}");
                    Err("projects.remote_details_failed".into())
                }
            };
            state.finish_save(pending_key);
            result
        };
        if let Err(message) = result {
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    show_error(&ui, crate::i18n::tr(&message).as_ref());
                }
            });
        }
    });
}

pub(super) fn prefetch_project_pr_overview(
    state: ProjectStateFacade,
    tasks: Vec<ProjectPrOverviewTask>,
) {
    for task in tasks {
        let state_task = state.clone();
        crate::app_runtime::shared_runtime().spawn(async move {
            let result = get_standard_environment_pull_request_counts(
                &task.repo_type,
                &task.repo_project_id,
                &task.remote_repo,
            )
            .await;
            let counts = match result {
                Ok(counts) => Some((counts.fat, counts.uat, counts.prod)),
                Err(error) => {
                    log::warn!(
                        "[Project] Failed to load PR overview for '{}': {}",
                        task.project_alias,
                        error
                    );
                    None
                }
            };
            state_task.complete_pr_overview(task.project_alias, counts);
        });
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
        position: "bottom".into(),
        icon: "".into(),
        overlay: false,
        close_on_click: false,
        loading_type: "".into(),
        width: "auto".into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::webp::WebPEncoder;
    use image::{ExtendedColorType, ImageEncoder};
    use std::fs;

    fn clear_remote_logo_disk_cache(url: &str) {
        for ext in ["png", "jpg", "webp", "gif", "svg"] {
            if let Some(path) = remote_logo_cache_path(url, ext) {
                let _ = fs::remove_file(path);
            }
        }
    }

    fn encode_solid_webp(rgb: [u8; 3]) -> Vec<u8> {
        let mut pixels = Vec::with_capacity(2 * 2 * 3);
        for _ in 0..4 {
            pixels.extend_from_slice(&rgb);
        }
        let mut bytes = Vec::new();
        WebPEncoder::new_lossless(&mut bytes)
            .write_image(pixels.as_slice(), 2, 2, ExtendedColorType::Rgb8)
            .expect("webp encode");
        bytes
    }

    #[test]
    fn remote_logo_pending_default_does_not_poison_memory_cache() {
        let url = "https://example.test/usher-remote-logo-cache-poison.webp";
        clear_remote_logo_disk_cache(url);
        let cache_key = logo_image_cache_key(url);
        invalidate_cached_logo_image(&cache_key);

        let _ = resolve_project_logo_image(url);
        assert!(
            cached_logo_image(&cache_key).is_none(),
            "pending remote fallback must not be stored under the remote URL cache key"
        );

        let webp = encode_solid_webp([12, 34, 56]);
        let path = remote_logo_cache_path(url, "webp").expect("cache path");
        fs::write(&path, webp.as_slice()).expect("write cached webp");

        let loaded = resolve_project_logo_image(url);
        assert!(
            cached_logo_image(&cache_key).is_some(),
            "successful remote logo loads should populate the memory cache"
        );
        assert!(loaded.size().width > 0);
        assert!(loaded.size().height > 0);

        clear_remote_logo_disk_cache(url);
        invalidate_cached_logo_image(&cache_key);
    }

    #[test]
    fn load_image_from_bytes_decodes_webp() {
        let webp = encode_solid_webp([200, 100, 50]);
        let image =
            load_image_from_bytes(webp.as_slice(), "webp").expect("webp logo bytes should decode");
        assert_eq!(image.size().width, 2);
        assert_eq!(image.size().height, 2);
    }
}
