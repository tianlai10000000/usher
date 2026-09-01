use crate::git::get_standard_environment_pull_request_counts;
use crate::terminal::diag_logging_enabled;
use crate::terminal::open_terminal_for_project;
use crate::{MainWindow, ProjectItem, ToastManager, ToastOptions};
use base64::Engine as _;
use image::GenericImageView;
use slint::{ComponentHandle, Global, ModelRc, VecModel};
use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use super::logo::*;
use super::model::{
    ProjectConfig, ProjectDraft, ProjectPrOverview, ProjectSaveAction, ProjectViewDiagSnapshot,
    parse_project_tags,
};
use super::overview::ProjectPrOverviewTask;
use super::state::ProjectStateFacade;
use super::storage::load_projects;

thread_local! {
    static PROJECT_VIEW_DIAG_CACHE: RefCell<Option<ProjectViewDiagSnapshot>> =
        const { RefCell::new(None) };
}

#[derive(Clone)]
pub(crate) struct ProjectController {
    state: ProjectStateFacade,
    _subscription: Arc<usher_state_store::Subscription>,
}

impl ProjectController {
    pub(crate) fn shutdown(&self) {
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
    let logo = ui.get_project_new_logo();
    ui.set_project_logo_preview_status(
        crate::i18n::tr(preview_logo_status_key(logo.as_str()))
            .into_owned()
            .into(),
    );
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
            let ui_weak = ui_weak_for_failure.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if state.rollback_if_current(expected, rollback) {
                    if let Some(ui) = ui_weak.upgrade() {
                        show_error(&ui, crate::i18n::tr("projects.save_failed").as_ref());
                    }
                } else {
                    log::warn!(
                        "[Project] Skipped persistence rollback because the registry changed again"
                    );
                }
            });
        }),
    )
    .expect("failed to start project persistence worker");
    let _ = state_slot.set(state.clone());
    super::state::register(&state);
    ui.set_project_list_loading(true);

    let ui_weak = ui.as_weak();
    let state_for_projection = state.clone();
    let subscription = state.subscribe(Arc::new(move |_| {
        let ui_weak = ui_weak.clone();
        let state = state_for_projection.clone();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let query = ui.get_project_search_query();
            refresh_project_items(&ui, &state, query.as_str());
        });
    }));

    let ui_weak = ui.as_weak();
    let state_preview = state.clone();
    ui.on_project_preview_logo(move |logo| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        refresh_logo_preview(&ui, &state_preview, logo.as_str());
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
            let logo = normalize_project_logo(logo.as_str());
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
            let logo = normalize_project_logo(logo.as_str());
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

        if let Err(error) = state_delete.delete(alias.to_string()) {
            show_error(&ui, crate::i18n::tr(error.as_str()).as_ref());
        }
    });

    let ui_weak = ui.as_weak();
    let state_search = state.clone();
    ui.on_project_search_projects(move |query| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };

        refresh_project_items(&ui, &state_search, query.as_str());
    });

    let ui_weak = ui.as_weak();
    let state_reload = state.clone();
    ui.on_project_reload_projects(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };

        log::info!("[Project] Reload projects requested.");
        let query = ui.get_project_search_query();
        ui.set_project_list_error("".into());
        ui.set_project_list_loading(true);

        let ui_weak_task = ui.as_weak();
        let state_task = state_reload.clone();
        crate::app_runtime::shared_runtime().spawn(async move {
            let loaded = tokio::task::spawn_blocking(load_projects).await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak_task.upgrade() else {
                    return;
                };
                ui.set_project_list_loading(false);

                let list = match loaded {
                    Ok(list) => list,
                    Err(error) => {
                        log::error!("[Project] Reload task failed: {error}");
                        ui.set_project_list_error(
                            crate::i18n::tr("projects.registry_read_failed")
                                .into_owned()
                                .into(),
                        );
                        show_error(&ui, crate::i18n::tr("projects.reload_failed").as_ref());
                        return;
                    }
                };

                state_task.hydrate(list);
                refresh_project_items(&ui, &state_task, query.as_str());
                show_success(&ui, crate::i18n::tr("projects.reloaded").as_ref());
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
            let loaded = tokio::task::spawn_blocking(load_projects).await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                match loaded {
                    Ok(projects) => {
                        state_initial.hydrate(projects);
                        let query = ui.get_project_search_query();
                        refresh_project_items(&ui, &state_initial, query.as_str());
                    }
                    Err(error) => {
                        log::error!("[Project] Initial load worker failed: {error}");
                        ui.set_project_list_error(
                            crate::i18n::tr("projects.registry_read_failed")
                                .into_owned()
                                .into(),
                        );
                    }
                }
                ui.set_project_logo_preview_image(resolve_project_logo_image(""));
                ui.set_project_logo_preview_status(
                    crate::i18n::tr("projects.using_default_logo")
                        .into_owned()
                        .into(),
                );
                ui.set_project_list_loading(false);
            });
        });
    });

    ProjectController {
        state,
        _subscription: Arc::new(subscription),
    }
}

fn start_project_save(
    action: ProjectSaveAction,
    draft: ProjectDraft,
    ui_weak: slint::Weak<MainWindow>,
    state: ProjectStateFacade,
) {
    let pending_key = action.pending_key(draft.project_alias.as_str());
    if !state.begin_save(pending_key.clone()) {
        if let Some(ui) = ui_weak.upgrade() {
            show_error(&ui, crate::i18n::tr("projects.save_in_progress").as_ref());
        }
        return;
    }

    crate::app_runtime::shared_runtime().spawn(async move {
        let details = crate::git::get_repo_detail(draft.remote_repo.as_str()).await;
        let _ = slint::invoke_from_event_loop(move || {
            state.finish_save(pending_key);
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
                        crate::i18n::tr("projects.remote_details_failed").as_ref(),
                    );
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
                ProjectSaveAction::Add => state.add(config),
                ProjectSaveAction::Update { original_alias } => {
                    state.update(original_alias, config)
                }
            };

            if let Err(message) = outcome {
                show_error(&ui, crate::i18n::tr(message.as_str()).as_ref());
            }
        });
    });
}

fn refresh_project_items(ui: &MainWindow, state: &ProjectStateFacade, query: &str) {
    let pr_tasks = state.prepare_pr_overview(query.to_string());
    let snapshot = state.snapshot();
    let projects = &snapshot.state().projects;
    let (ui_items, remote_urls) = prepare_project_items(projects, query);
    log_project_view_diagnostics(
        projects,
        query,
        &ui_items,
        &remote_urls,
        state.pending_pr_overview_count(),
    );
    ui.set_project_items(ModelRc::new(VecModel::from(ui_items)));
    prefetch_remote_logos(state.clone(), remote_urls);
    prefetch_project_pr_overview(state.clone(), pr_tasks);
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
                repo_type: item.repo_type.into(),
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

fn prefetch_remote_logos(state: ProjectStateFacade, remote_urls: Vec<String>) {
    for url in remote_urls {
        if find_cached_remote_logo_path(url.as_str()).is_some() {
            continue;
        }

        if !state.begin_remote_logo(url.clone()) {
            continue;
        }

        let state_task = state.clone();
        crate::app_runtime::shared_runtime().spawn(async move {
            let downloaded = download_remote_logo_to_cache(url.as_str()).await.is_some();
            if !downloaded {
                state_task.finish_remote_logo(url);
                return;
            }

            let _ = slint::invoke_from_event_loop(move || {
                invalidate_cached_logo_image(&logo_image_cache_key(url.as_str()));
                state_task.finish_remote_logo(url);
            });
        });
    }
}

fn prefetch_project_pr_overview(state: ProjectStateFacade, tasks: Vec<ProjectPrOverviewTask>) {
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
        position: "middle".into(),
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
