use crate::app_runtime::shared_runtime;
use crate::workflow::bindings::{show_error, show_success};
use crate::workflow::state::{SharedWorkflowState, default_tag_steps, initialize_current_ui};
use crate::{MainWindow, StepItem};
use slint::ComponentHandle;
use std::path::Path;
use std::process::{Command as StdCommand, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

const TAG_MODE_LOCAL: &str = "local";
const TAG_MODE_REMOTE: &str = "remote";
const TAG_BUMP_MAJOR: &str = "major";
const TAG_BUMP_MINOR: &str = "minor";
const TAG_BUMP_PATCH: &str = "patch";

const MAX_STEP_LOG_BYTES: usize = 32 * 1024;

fn clamp_step_log(mut log: String) -> String {
    if log.len() <= MAX_STEP_LOG_BYTES {
        return log;
    }

    let keep_from = log.len().saturating_sub(MAX_STEP_LOG_BYTES);
    let mut start = keep_from;
    while start < log.len() && !log.is_char_boundary(start) {
        start += 1;
    }

    let tail = log.split_off(start);
    format!(
        "[Truncated] Log was {} bytes; showing last {} bytes.\n\n{}",
        keep_from + tail.len(),
        tail.len(),
        tail
    )
}

static TAG_CANCEL_TOKEN: OnceLock<Mutex<Option<Arc<AtomicBool>>>> = OnceLock::new();

fn cancel_slot() -> &'static Mutex<Option<Arc<AtomicBool>>> {
    TAG_CANCEL_TOKEN.get_or_init(|| Mutex::new(None))
}

fn install_cancel_token(token: Arc<AtomicBool>) {
    if let Ok(mut slot) = cancel_slot().lock() {
        *slot = Some(token);
    }
}

fn clear_cancel_token(token: &Arc<AtomicBool>) {
    if let Ok(mut slot) = cancel_slot().lock() {
        if let Some(current) = slot.as_ref() {
            if Arc::ptr_eq(current, token) {
                *slot = None;
            }
        }
    }
}

fn request_tag_cancel() -> bool {
    if let Ok(slot) = cancel_slot().lock() {
        if let Some(token) = slot.as_ref() {
            token.store(true, Ordering::SeqCst);
            return true;
        }
    }
    false
}

pub(crate) fn bind_tag(ui: &MainWindow, shared_state: SharedWorkflowState) {
    log::info!("[Workflow][Tag] Binding Tag tab callbacks");
    let ui_weak = ui.as_weak();
    let state_mode = shared_state.clone();
    ui.on_workflow_tag_change_mode(move |mode| {
        log::info!("[Workflow][Tag] UI action: change mode -> {}", mode);
        let Some(ui) = ui_weak.upgrade() else {
            log::warn!("[Workflow][Tag] change mode ignored: UI already dropped");
            return;
        };
        if let Ok(mut guard) = state_mode.lock() {
            guard.upsert_current(|view| {
                view.tag_mode = mode.to_string();
                view.tag_running = false;
                view.tag_add_initializing = false;
                view.tag_remove_initializing = false;
                view.tag_add_ready = false;
                view.tag_remove_ready = false;
                view.tag_add_request_seq = view.tag_add_request_seq.saturating_add(1);
                view.tag_remove_request_seq = view.tag_remove_request_seq.saturating_add(1);
                view.tag_steps = default_tag_steps();
            });
            log::info!(
                "[Workflow][Tag] Mode updated in state, reset running flag and step list -> {}",
                mode
            );
        } else {
            log::warn!("[Workflow][Tag] Failed to acquire state lock on mode change");
        }
        refresh_tag_context(&ui, &state_mode, false);
    });

    let ui_weak = ui.as_weak();
    let state_bump = shared_state.clone();
    ui.on_workflow_tag_select_version_mode(move |version_mode| {
        log::info!(
            "[Workflow][Tag] UI action: select version bump mode -> {}",
            version_mode
        );
        let Some(ui) = ui_weak.upgrade() else {
            log::warn!("[Workflow][Tag] select version mode ignored: UI already dropped");
            return;
        };
        if let Ok(mut guard) = state_bump.lock() {
            guard.upsert_current(|view| {
                view.tag_version_mode = version_mode.to_string();
                view.tag_next_version =
                    next_semver_for_mode(&view.tag_current_version, version_mode.as_str());
            });
            log::info!(
                "[Workflow][Tag] Version bump mode updated -> {}",
                version_mode
            );
        } else {
            log::warn!("[Workflow][Tag] Failed to acquire state lock on version mode select");
        }
        initialize_current_ui(&ui, &state_bump);
    });

    let ui_weak = ui.as_weak();
    let state_add = shared_state.clone();
    let runtime = shared_runtime();
    ui.on_workflow_tag_open_add(move || {
        log::info!("[Workflow][Tag] UI action: open add tab");
        let Some(ui) = ui_weak.upgrade() else {
            log::warn!("[Workflow][Tag] open add tab ignored: UI already dropped");
            return;
        };

        let (project_id, project_name, local_repo, mode, bump_mode, add_ready, add_initializing, running) = state_add
            .lock()
            .ok()
            .map(|guard| {
                guard.with_current(|ctx, view| {
                    (
                        ctx.id.clone(),
                        ctx.name.clone(),
                        ctx.local_repo.clone(),
                        view.tag_mode.clone(),
                        view.tag_version_mode.clone(),
                        view.tag_add_ready,
                        view.tag_add_initializing,
                        view.tag_running,
                    )
                })
            })
            .unwrap_or_else(|| {
                (
                    "default".to_string(),
                    "Default Project".to_string(),
                    String::new(),
                    TAG_MODE_LOCAL.to_string(),
                    TAG_BUMP_PATCH.to_string(),
                    false,
                    false,
                    false,
                )
            });

        if mode == TAG_MODE_LOCAL && add_ready && !add_initializing && !running {
            log::debug!("[Workflow][Tag] open add tab reused cached initialization for '{}'", project_id);
            initialize_current_ui(&ui, &state_add);
            return;
        }

        if mode == TAG_MODE_REMOTE {
            if let Ok(mut guard) = state_add.lock() {
                guard.upsert_current(|view| {
                    view.tag_add_request_seq = view.tag_add_request_seq.saturating_add(1);
                    view.tag_result = "Remote mode is TODO in this iteration.".to_string();
                    view.tag_add_initializing = false;
                    view.tag_add_ready = false;
                    view.tag_steps = default_tag_steps();
                });
            }
            initialize_current_ui(&ui, &state_add);
            return;
        }

        let request_seq = if let Ok(mut guard) = state_add.lock() {
            let mut request_seq = 0;
            guard.upsert_current(|view| {
                view.tag_add_request_seq = view.tag_add_request_seq.saturating_add(1);
                request_seq = view.tag_add_request_seq;
                view.tag_add_initializing = true;
                view.tag_add_ready = false;
                view.tag_result = format!(
                    "Loading latest local tag for add workflow in project '{}'.",
                    project_name
                );
                view.tag_steps = default_tag_steps();
            });
            request_seq
        } else {
            0
        };
        initialize_current_ui(&ui, &state_add);

        let ui_weak_task = ui_weak.clone();
        let state_task = state_add.clone();
        runtime.spawn(async move {
            log::info!(
                "[Workflow][Tag] Add tab initialization started: project_id='{}', repo='{}'",
                project_id,
                local_repo
            );
            let resolved = tokio::task::spawn_blocking({
                let local_repo = local_repo.clone();
                move || read_latest_local_tag(&local_repo)
            })
            .await
            .map_err(|error| format!("Failed to join latest local tag task: {}", error))
            .and_then(|result| result);

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak_task.upgrade() {
                    if let Ok(mut guard) = state_task.lock() {
                        let current_project_id = guard.current_project.id.clone();
                        let entry = guard.projects.entry(project_id.clone()).or_default();
                        if entry.tag_add_request_seq != request_seq {
                            return;
                        }
                        match resolved {
                            Ok(latest_tag) => {
                                log::info!(
                                    "[Workflow][Tag] Add tab initialized with latest local tag '{}'",
                                    latest_tag
                                );
                                entry.tag_current_version = latest_tag.clone();
                                entry.tag_next_version = next_semver_for_mode(&latest_tag, &bump_mode);
                                entry.tag_remove_target = latest_tag.clone();
                                entry.tag_add_initializing = false;
                                entry.tag_add_ready = true;
                                entry.tag_result = format!(
                                    "Local mode ready. Current latest tag is '{}', next {} version prepared.",
                                    latest_tag, bump_mode
                                );
                            }
                            Err(error) => {
                                log::warn!("[Workflow][Tag] Add tab initialization failed: {}", error);
                                entry.tag_current_version = "v0.0.0".to_string();
                                entry.tag_next_version = next_semver_for_mode("v0.0.0", &bump_mode);
                                entry.tag_remove_target = "v0.0.0".to_string();
                                entry.tag_add_initializing = false;
                                entry.tag_add_ready = false;
                                entry.tag_result = error;
                            }
                        }
                        if current_project_id != project_id {
                            return;
                        }
                    }
                    initialize_current_ui(&ui, &state_task);
                }
            });
        });
    });

    let ui_weak = ui.as_weak();
    let state_open_remove = shared_state.clone();
    let runtime = shared_runtime();
    ui.on_workflow_tag_open_remove(move || {
        log::info!("[Workflow][Tag] UI action: open remove tab");
        let Some(ui) = ui_weak.upgrade() else {
            log::warn!("[Workflow][Tag] open remove tab ignored: UI already dropped");
            return;
        };

        let (project_id, project_name, local_repo, mode, remove_ready, remove_initializing, running) = state_open_remove
            .lock()
            .ok()
            .map(|guard| {
                guard.with_current(|ctx, view| {
                    (
                        ctx.id.clone(),
                        ctx.name.clone(),
                        ctx.local_repo.clone(),
                        view.tag_mode.clone(),
                        view.tag_remove_ready,
                        view.tag_remove_initializing,
                        view.tag_running,
                    )
                })
            })
            .unwrap_or_else(|| {
                (
                    "default".to_string(),
                    "Default Project".to_string(),
                    String::new(),
                    TAG_MODE_LOCAL.to_string(),
                    false,
                    false,
                    false,
                )
            });

        if mode == TAG_MODE_LOCAL && remove_ready && !remove_initializing && !running {
            log::debug!("[Workflow][Tag] open remove tab reused cached initialization for '{}'", project_id);
            initialize_current_ui(&ui, &state_open_remove);
            return;
        }

        if mode == TAG_MODE_REMOTE {
            if let Ok(mut guard) = state_open_remove.lock() {
                guard.upsert_current(|view| {
                    view.tag_remove_request_seq = view.tag_remove_request_seq.saturating_add(1);
                    view.tag_result = "Remote mode is TODO in this iteration.".to_string();
                    view.tag_remove_initializing = false;
                    view.tag_remove_ready = false;
                    view.tag_remove_target = "TODO".to_string();
                    view.tag_steps = default_remove_tag_steps();
                });
            }
            initialize_current_ui(&ui, &state_open_remove);
            return;
        }

        let request_seq = if let Ok(mut guard) = state_open_remove.lock() {
            let mut request_seq = 0;
            guard.upsert_current(|view| {
                view.tag_remove_request_seq = view.tag_remove_request_seq.saturating_add(1);
                request_seq = view.tag_remove_request_seq;
                view.tag_add_initializing = false;
                view.tag_remove_initializing = true;
                view.tag_remove_ready = false;
                view.tag_result = format!(
                    "Loading latest remote tag for remove workflow in project '{}'.",
                    project_name
                );
                view.tag_steps = default_remove_tag_steps();
            });
            request_seq
        } else {
            0
        };
        initialize_current_ui(&ui, &state_open_remove);

        let ui_weak_task = ui_weak.clone();
        let state_task = state_open_remove.clone();
        runtime.spawn(async move {
            log::info!(
                "[Workflow][Tag] Remove tab initialization started: project_id='{}', repo='{}'",
                project_id,
                local_repo
            );
            let resolved = tokio::task::spawn_blocking({
                let local_repo = local_repo.clone();
                move || inspect_latest_remote_tag(&local_repo)
            })
            .await
            .map_err(|error| format!("Failed to join remote tag lookup task: {}", error))
            .and_then(|result| result);

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak_task.upgrade() {
                    if let Ok(mut guard) = state_task.lock() {
                        let current_project_id = guard.current_project.id.clone();
                        let entry = guard.projects.entry(project_id.clone()).or_default();
                        if entry.tag_remove_request_seq != request_seq {
                            return;
                        }
                        match resolved {
                            Ok((latest_tag, lookup_log)) => {
                                log::info!(
                                    "[Workflow][Tag] Remove tab initialized with latest remote tag '{}'",
                                    latest_tag
                                );
                                entry.tag_remove_target = latest_tag.clone();
                                entry.tag_add_initializing = false;
                                entry.tag_remove_initializing = false;
                                entry.tag_remove_ready = true;
                                entry.tag_result = format!(
                                    "Remove mode ready. Latest remote tag is '{}'.",
                                    latest_tag
                                );
                                if let Some(first) = entry.tag_steps.get_mut(0) {
                                    first.detail = format!(
                                        "Latest remote tag resolved for removal: '{}'.",
                                        latest_tag
                                    )
                                    .into();
                                    first.log = clamp_step_log(lookup_log).into();
                                }
                            }
                            Err(error) => {
                                log::warn!(
                                    "[Workflow][Tag] Remove tab initialization failed: {}",
                                    error
                                );
                                entry.tag_remove_target = "v0.0.0".to_string();
                                entry.tag_add_initializing = false;
                                entry.tag_remove_initializing = false;
                                entry.tag_remove_ready = false;
                                entry.tag_result = error;
                            }
                        }
                        if current_project_id != project_id {
                            return;
                        }
                    }
                    initialize_current_ui(&ui, &state_task);
                }
            });
        });
    });

    let ui_weak = ui.as_weak();
    let state_run = shared_state.clone();
    let runtime = shared_runtime();
    ui.on_workflow_tag_run(move |version, description| {
        log::info!(
            "[Workflow][Tag] UI action: run workflow requested, version='{}', description_len={}",
            version,
            description.len()
        );
        let Some(ui) = ui_weak.upgrade() else {
            log::warn!("[Workflow][Tag] run ignored: UI already dropped");
            return;
        };

        let (project_name, local_repo, mode, bump_mode, already_running, add_initializing) = state_run
            .lock()
            .ok()
            .map(|guard| {
                guard.with_current(|ctx, view| {
                    (
                        ctx.name.clone(),
                        ctx.local_repo.clone(),
                        view.tag_mode.clone(),
                        view.tag_version_mode.clone(),
                        view.tag_running,
                        view.tag_add_initializing,
                    )
                })
            })
            .unwrap_or_else(|| {
                (
                    "Default Project".to_string(),
                    String::new(),
                    TAG_MODE_LOCAL.to_string(),
                    TAG_BUMP_PATCH.to_string(),
                    false,
                    false,
                )
            });
        log::info!(
            "[Workflow][Tag] Run context: project='{}', mode='{}', bump='{}', repo='{}', running={}",
            project_name,
            mode,
            bump_mode,
            local_repo,
            already_running
        );

        if already_running {
            log::warn!("[Workflow][Tag] Run rejected: workflow is already running");
            show_error(&ui, "Tag workflow is already running.");
            return;
        }
        if add_initializing {
            log::warn!("[Workflow][Tag] Run rejected: add tab is still initializing");
            show_error(&ui, "Add tab is still initializing latest tag. Please wait.");
            return;
        }

        if mode == TAG_MODE_REMOTE {
            log::warn!("[Workflow][Tag] Run rejected: remote mode not implemented");
            if let Ok(mut guard) = state_run.lock() {
                guard.upsert_current(|view| {
                    view.tag_result = "Remote mode is TODO in this iteration.".to_string();
                    view.tag_running = false;
                    view.tag_add_ready = false;
                    view.tag_remove_ready = false;
                    view.tag_add_request_seq = view.tag_add_request_seq.saturating_add(1);
                    view.tag_remove_request_seq = view.tag_remove_request_seq.saturating_add(1);
                    view.tag_steps = default_tag_steps();
                });
            } else {
                log::warn!("[Workflow][Tag] Failed to acquire state lock while handling remote TODO");
            }
            initialize_current_ui(&ui, &state_run);
            show_error(&ui, "Remote mode is not implemented yet.");
            return;
        }

        let version = version.trim().to_string();
        if !is_valid_semver_tag(&version) {
            log::warn!(
                "[Workflow][Tag] Run rejected: invalid semantic version -> '{}'",
                version
            );
            show_error(&ui, "New tag version must match semantic version format like v1.2.3.");
            return;
        }
        if local_repo.trim().is_empty() {
            log::warn!("[Workflow][Tag] Run rejected: empty local repository path");
            show_error(&ui, "Local repository path is empty. Open a project first.");
            return;
        }
        if !Path::new(&local_repo).exists() {
            log::warn!(
                "[Workflow][Tag] Run rejected: local repository path does not exist -> '{}'",
                local_repo
            );
            show_error(&ui, "Local repository path does not exist.");
            return;
        }

        if let Ok(mut guard) = state_run.lock() {
            guard.upsert_current(|view| {
                view.tag_running = true;
                view.tag_add_initializing = false;
                view.tag_add_ready = false;
                view.tag_remove_ready = false;
                view.tag_add_request_seq = view.tag_add_request_seq.saturating_add(1);
                view.tag_remove_request_seq = view.tag_remove_request_seq.saturating_add(1);
                view.tag_result = format!(
                    "Running local tag workflow for project '{}' in {} mode.",
                    project_name, mode
                );
                view.tag_next_version = version.clone();
                view.tag_steps = default_tag_steps();
            });
            log::info!(
                "[Workflow][Tag] Run accepted: state switched to running for version '{}'",
                version
            );
        } else {
            log::warn!("[Workflow][Tag] Failed to acquire state lock before scheduling run");
        }
        initialize_current_ui(&ui, &state_run);

        let ui_weak_task = ui_weak.clone();
        let state_task = state_run.clone();
        let description = description.to_string();
        let project_id = state_run
            .lock()
            .ok()
            .map(|guard| guard.current_project.id.clone())
            .unwrap_or_else(|| "default".to_string());
        let ui_weak_progress = ui_weak.clone();
        let state_progress = state_run.clone();
        let project_id_progress = project_id.clone();
        let progress_reporter: TagProgressReporter = Arc::new(move |steps, result| {
            let ui_weak_local = ui_weak_progress.clone();
            let state_local = state_progress.clone();
            let project_local = project_id_progress.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak_local.upgrade() {
                    if let Ok(mut guard) = state_local.lock() {
                        let current_project_id = guard.current_project.id.clone();
                        let still_running = steps.iter().any(|s| s.status.as_str() == "running");
                        let entry = guard.projects.entry(project_local.clone()).or_default();
                        entry.tag_running = still_running;
                        entry.tag_steps = steps;
                        entry.tag_result = result;
                        if current_project_id != project_local {
                            return;
                        }
                    } else {
                        log::warn!("[Workflow][Tag] Failed to acquire state lock for progress update");
                    }
                    initialize_current_ui(&ui, &state_local);
                }
            });
        });
        log::info!(
            "[Workflow][Tag] Spawning async local tag flow: project_id='{}', version='{}'",
            project_id,
            version
        );
        runtime.spawn(async move {
            let cancel_token = Arc::new(AtomicBool::new(false));
            install_cancel_token(cancel_token.clone());
            let execution = run_local_tag_flow(
                &local_repo,
                &version,
                &description,
                Some(progress_reporter),
                cancel_token.clone(),
            )
            .await;
            clear_cancel_token(&cancel_token);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak_task.upgrade() {
                    if let Ok(mut guard) = state_task.lock() {
                        let current_project_id = guard.current_project.id.clone();
                        let final_result = match execution {
                            Ok(ref success) => {
                                guard.cd_running = false;
                                log::info!(
                                    "[Workflow][Tag] Async run completed successfully: {}",
                                    success.summary
                                );
                                success.summary.clone()
                            }
                            Err(ref failure) => {
                                log::error!(
                                    "[Workflow][Tag] Async run completed with failure: {}",
                                    failure.summary
                                );
                                failure.summary.clone()
                            }
                        };
                        guard.projects.entry(project_id.clone()).or_default().tag_result = final_result;
                        guard.projects.entry(project_id.clone()).or_default().tag_running = false;
                        guard.projects.entry(project_id.clone()).or_default().tag_add_initializing = false;
                        match execution {
                            Ok(ref success) => {
                                guard.projects.entry(project_id.clone()).or_default().tag_current_version =
                                    success.current_version.clone();
                                guard.projects.entry(project_id.clone()).or_default().tag_next_version =
                                    next_semver_for_mode(&success.current_version, &bump_mode);
                                guard.projects.entry(project_id.clone()).or_default().tag_remove_target =
                                    success.current_version.clone();
                                guard.projects.entry(project_id.clone()).or_default().tag_add_ready = false;
                                guard.projects.entry(project_id.clone()).or_default().tag_remove_ready = false;
                                guard.projects.entry(project_id.clone()).or_default().tag_steps =
                                    success.steps.clone();
                            }
                            Err(ref failure) => {
                                guard.projects.entry(project_id.clone()).or_default().tag_add_ready = false;
                                guard.projects.entry(project_id.clone()).or_default().tag_steps =
                                    failure.steps.clone();
                            }
                        }
                        if current_project_id != project_id {
                            log::info!(
                                "[Workflow][Tag] Skip UI refresh: active project switched from '{}' to '{}'",
                                project_id,
                                current_project_id
                            );
                            return;
                        }
                    } else {
                        log::warn!("[Workflow][Tag] Failed to acquire state lock in event-loop callback");
                    }
                    initialize_current_ui(&ui, &state_task);
                    match execution {
                        Ok(ref success) => show_success(&ui, &success.summary),
                        Err(ref failure) => show_error(&ui, &failure.summary),
                    }
                } else {
                    log::warn!("[Workflow][Tag] Event-loop callback ignored: UI already dropped");
                }
            });
        });
    });

    let ui_weak = ui.as_weak();
    let state_remove = shared_state.clone();
    let runtime = shared_runtime();
    ui.on_workflow_tag_remove(move |tag| {
        log::info!(
            "[Workflow][Tag] UI action: remove tag requested, tag='{}'",
            tag
        );
        let Some(ui) = ui_weak.upgrade() else {
            log::warn!("[Workflow][Tag] remove ignored: UI already dropped");
            return;
        };

        let (project_name, local_repo, mode, bump_mode, already_running, remove_initializing) =
            state_remove
                .lock()
                .ok()
                .map(|guard| {
                    guard.with_current(|ctx, view| {
                        (
                            ctx.name.clone(),
                            ctx.local_repo.clone(),
                            view.tag_mode.clone(),
                            view.tag_version_mode.clone(),
                            view.tag_running,
                            view.tag_remove_initializing,
                        )
                    })
                })
                .unwrap_or_else(|| {
                    (
                        "Default Project".to_string(),
                        String::new(),
                        TAG_MODE_LOCAL.to_string(),
                        TAG_BUMP_PATCH.to_string(),
                        false,
                        false,
                    )
                });

        if already_running {
            log::warn!("[Workflow][Tag] Remove rejected: workflow is already running");
            show_error(&ui, "Tag workflow is already running.");
            return;
        }
        if remove_initializing {
            log::warn!("[Workflow][Tag] Remove rejected: remove tab is still initializing");
            show_error(
                &ui,
                "Remove tab is still initializing latest tag. Please wait.",
            );
            return;
        }
        if mode == TAG_MODE_REMOTE {
            if let Ok(mut guard) = state_remove.lock() {
                guard.upsert_current(|view| {
                    view.tag_result = "Remote mode is TODO in this iteration.".to_string();
                    view.tag_running = false;
                    view.tag_add_initializing = false;
                    view.tag_remove_initializing = false;
                    view.tag_add_ready = false;
                    view.tag_remove_ready = false;
                    view.tag_add_request_seq = view.tag_add_request_seq.saturating_add(1);
                    view.tag_remove_request_seq = view.tag_remove_request_seq.saturating_add(1);
                    view.tag_steps = default_remove_tag_steps();
                });
            }
            initialize_current_ui(&ui, &state_remove);
            show_error(&ui, "Remote mode is not implemented yet.");
            return;
        }

        let tag = tag.trim().to_string();
        if !is_valid_semver_tag(&tag) {
            show_error(
                &ui,
                "Tag to remove must match semantic version format like v1.2.3.",
            );
            return;
        }
        if local_repo.trim().is_empty() {
            show_error(&ui, "Local repository path is empty. Open a project first.");
            return;
        }
        if !Path::new(&local_repo).exists() {
            show_error(&ui, "Local repository path does not exist.");
            return;
        }

        if let Ok(mut guard) = state_remove.lock() {
            guard.upsert_current(|view| {
                view.tag_running = true;
                view.tag_add_initializing = false;
                view.tag_remove_initializing = false;
                view.tag_add_ready = false;
                view.tag_remove_ready = false;
                view.tag_add_request_seq = view.tag_add_request_seq.saturating_add(1);
                view.tag_remove_request_seq = view.tag_remove_request_seq.saturating_add(1);
                view.tag_result = format!(
                    "Running local tag remove workflow for project '{}' in {} mode.",
                    project_name, mode
                );
                view.tag_remove_target = tag.clone();
                view.tag_steps = default_remove_tag_steps();
            });
        }
        initialize_current_ui(&ui, &state_remove);

        let ui_weak_task = ui_weak.clone();
        let state_task = state_remove.clone();
        let project_id = state_remove
            .lock()
            .ok()
            .map(|guard| guard.current_project.id.clone())
            .unwrap_or_else(|| "default".to_string());
        let ui_weak_progress = ui_weak.clone();
        let state_progress = state_remove.clone();
        let project_id_progress = project_id.clone();
        let progress_reporter: TagProgressReporter = Arc::new(move |steps, result| {
            let ui_weak_local = ui_weak_progress.clone();
            let state_local = state_progress.clone();
            let project_local = project_id_progress.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak_local.upgrade() {
                    if let Ok(mut guard) = state_local.lock() {
                        let current_project_id = guard.current_project.id.clone();
                        let still_running = steps.iter().any(|s| s.status.as_str() == "running");
                        let entry = guard.projects.entry(project_local.clone()).or_default();
                        entry.tag_running = still_running;
                        entry.tag_steps = steps;
                        entry.tag_result = result;
                        if current_project_id != project_local {
                            return;
                        }
                    }
                    initialize_current_ui(&ui, &state_local);
                }
            });
        });

        runtime.spawn(async move {
            let cancel_token = Arc::new(AtomicBool::new(false));
            install_cancel_token(cancel_token.clone());
            let execution = run_local_remove_tag_flow(
                &local_repo,
                &tag,
                Some(progress_reporter),
                cancel_token.clone(),
            )
            .await;
            clear_cancel_token(&cancel_token);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak_task.upgrade() {
                    if let Ok(mut guard) = state_task.lock() {
                        let current_project_id = guard.current_project.id.clone();
                        let final_result = match execution {
                            Ok(ref success) => success.summary.clone(),
                            Err(ref failure) => failure.summary.clone(),
                        };
                        guard
                            .projects
                            .entry(project_id.clone())
                            .or_default()
                            .tag_result = final_result;
                        guard
                            .projects
                            .entry(project_id.clone())
                            .or_default()
                            .tag_running = false;
                        guard
                            .projects
                            .entry(project_id.clone())
                            .or_default()
                            .tag_add_initializing = false;
                        guard
                            .projects
                            .entry(project_id.clone())
                            .or_default()
                            .tag_remove_initializing = false;
                        match execution {
                            Ok(ref success) => {
                                let entry = guard.projects.entry(project_id.clone()).or_default();
                                entry.tag_current_version = success.current_version.clone();
                                entry.tag_next_version =
                                    next_semver_for_mode(&success.current_version, &bump_mode);
                                entry.tag_remove_target = success.current_version.clone();
                                entry.tag_add_ready = false;
                                entry.tag_remove_ready = false;
                                entry.tag_steps = success.steps.clone();
                            }
                            Err(ref failure) => {
                                let entry = guard.projects.entry(project_id.clone()).or_default();
                                entry.tag_remove_ready = false;
                                entry.tag_steps = failure.steps.clone();
                            }
                        }
                        if current_project_id != project_id {
                            return;
                        }
                    }
                    initialize_current_ui(&ui, &state_task);
                    match execution {
                        Ok(ref success) => show_success(&ui, &success.summary),
                        Err(ref failure) => show_error(&ui, &failure.summary),
                    }
                }
            });
        });
    });

    let ui_weak = ui.as_weak();
    let state_stop = shared_state.clone();
    ui.on_workflow_tag_stop(move || {
        log::warn!("[Workflow][Tag] UI action: stop requested");
        let Some(ui) = ui_weak.upgrade() else {
            log::warn!("[Workflow][Tag] stop ignored: UI already dropped");
            return;
        };

        let is_running = state_stop
            .lock()
            .ok()
            .map(|guard| guard.with_current(|_, view| view.tag_running))
            .unwrap_or(false);
        if !is_running {
            log::warn!("[Workflow][Tag] stop ignored: no active workflow");
            show_error(&ui, "No running tag workflow to stop.");
            return;
        }

        if !request_tag_cancel() {
            log::warn!("[Workflow][Tag] stop requested but no cancel token found");
        }

        if let Ok(mut guard) = state_stop.lock() {
            guard.upsert_current(|view| {
                view.tag_running = false;
                view.tag_result =
                    "Stop requested. Cancelling workflow and pausing remaining steps.".to_string();
                for step in &mut view.tag_steps {
                    if step.status.as_str() == "running" {
                        step.status = "paused".into();
                        step.detail =
                            "Cancelled by user. Waiting for current command to stop.".into();
                        let current_log = step.log.to_string();
                        step.log = if current_log.is_empty() {
                            "[Stop] Cancel requested by user.".into()
                        } else {
                            format!("{}\n[Stop] Cancel requested by user.", current_log).into()
                        };
                    } else if step.status.as_str() == "pending" {
                        step.status = "paused".into();
                        step.detail = "Paused due to cancellation request.".into();
                    }
                }
            });
            log::info!("[Workflow][Tag] stop state applied to UI model");
        } else {
            log::warn!("[Workflow][Tag] stop failed: cannot acquire state lock");
        }
        initialize_current_ui(&ui, &state_stop);
        show_error(&ui, "Tag workflow stop requested.");
    });
}

pub(crate) fn refresh_tag_context(
    ui: &MainWindow,
    shared_state: &SharedWorkflowState,
    announce_remote_todo: bool,
) {
    log::info!(
        "[Workflow][Tag] Refresh context start (announce_remote_todo={})",
        announce_remote_todo
    );
    let (mode, local_repo, bump_mode) = shared_state
        .lock()
        .ok()
        .map(|guard| {
            guard.with_current(|ctx, view| {
                (
                    view.tag_mode.clone(),
                    ctx.local_repo.clone(),
                    view.tag_version_mode.clone(),
                )
            })
        })
        .unwrap_or_else(|| {
            (
                TAG_MODE_LOCAL.to_string(),
                String::new(),
                TAG_BUMP_PATCH.to_string(),
            )
        });
    log::info!(
        "[Workflow][Tag] Refresh context snapshot: mode='{}', bump='{}', repo='{}'",
        mode,
        bump_mode,
        local_repo
    );

    let mut maybe_error = None;
    if let Ok(mut guard) = shared_state.lock() {
        guard.upsert_current(|view| {
            view.tag_running = false;
            view.tag_add_initializing = false;
            view.tag_remove_initializing = false;
            view.tag_add_ready = false;
            view.tag_remove_ready = false;
            view.tag_add_request_seq = view.tag_add_request_seq.saturating_add(1);
            view.tag_remove_request_seq = view.tag_remove_request_seq.saturating_add(1);
            view.tag_steps = default_tag_steps();
            if mode == TAG_MODE_REMOTE {
                log::info!("[Workflow][Tag] Refresh context -> remote TODO state");
                view.tag_current_version = "TODO".to_string();
                view.tag_next_version = "TODO".to_string();
                view.tag_remove_target = "TODO".to_string();
                view.tag_result = "Remote mode is TODO in this iteration.".to_string();
                return;
            }

            match read_latest_local_tag(&local_repo) {
                Ok(current_tag) => {
                    log::info!(
                        "[Workflow][Tag] Refresh context resolved current tag '{}'",
                        current_tag
                    );
                    view.tag_current_version = current_tag.clone();
                    view.tag_next_version = next_semver_for_mode(&current_tag, &bump_mode);
                    view.tag_remove_target = current_tag.clone();
                    view.tag_add_ready = true;
                    view.tag_result = if local_repo.trim().is_empty() {
                        "Select a project to load the local repository context.".to_string()
                    } else {
                        format!(
                            "Local mode ready. Current latest tag is '{}', next {} version prepared.",
                            current_tag, bump_mode
                        )
                    };
                }
                Err(error) => {
                    log::warn!("[Workflow][Tag] Refresh context failed to read tags: {}", error);
                    maybe_error = Some(error.clone());
                    view.tag_current_version = "v0.0.0".to_string();
                    view.tag_next_version = next_semver_for_mode("v0.0.0", &bump_mode);
                    view.tag_remove_target = "v0.0.0".to_string();
                    view.tag_add_ready = false;
                    view.tag_result = error;
                }
            }
        });
    } else {
        log::warn!("[Workflow][Tag] Refresh context failed: cannot acquire state lock");
    }
    initialize_current_ui(ui, shared_state);
    if announce_remote_todo && mode == TAG_MODE_REMOTE {
        show_error(ui, "Remote mode is not implemented yet.");
    } else if let Some(error) = maybe_error {
        show_error(ui, &error);
    }
    log::info!("[Workflow][Tag] Refresh context finished");
}

struct TagRunSuccess {
    summary: String,
    current_version: String,
    steps: Vec<StepItem>,
}

struct TagRunFailure {
    summary: String,
    steps: Vec<StepItem>,
}

type TagProgressReporter = Arc<dyn Fn(Vec<StepItem>, String) + Send + Sync>;

fn default_remove_tag_steps() -> Vec<StepItem> {
    vec![
        StepItem {
            name: "Enter local repository".into(),
            status: "pending".into(),
            detail: "Resolve git repository path.".into(),
            log: "".into(),
        },
        StepItem {
            name: "Delete local tag".into(),
            status: "pending".into(),
            detail: "Delete tag from local repository.".into(),
            log: "".into(),
        },
        StepItem {
            name: "Delete remote tag".into(),
            status: "pending".into(),
            detail: "Delete tag from origin remote.".into(),
            log: "".into(),
        },
        StepItem {
            name: "Refresh latest remote tag".into(),
            status: "pending".into(),
            detail: "Fetch and resolve the latest remote semantic tag.".into(),
            log: "".into(),
        },
        StepItem {
            name: "Workflow complete".into(),
            status: "pending".into(),
            detail: "Waiting for remove workflow completion.".into(),
            log: "".into(),
        },
    ]
}

async fn run_local_tag_flow(
    repo: &str,
    version: &str,
    description: &str,
    on_progress: Option<TagProgressReporter>,
    cancel_token: Arc<AtomicBool>,
) -> Result<TagRunSuccess, TagRunFailure> {
    log::info!(
        "[Workflow][Tag] Local flow started: repo='{}', version='{}', description_len={}",
        repo,
        version,
        description.len()
    );
    let mut steps = default_tag_steps();
    let commands: Vec<(&str, Vec<String>, String)> = vec![
        (
            "Enter local repository",
            vec![
                "git".to_string(),
                "-C".to_string(),
                repo.to_string(),
                "rev-parse".to_string(),
                "--show-toplevel".to_string(),
            ],
            format!("Using local repository at '{}'.", repo),
        ),
        (
            "Checkout release",
            vec![
                "git".to_string(),
                "-C".to_string(),
                repo.to_string(),
                "checkout".to_string(),
                "release".to_string(),
            ],
            "Switch to release branch.".to_string(),
        ),
        (
            "Pull release",
            vec![
                "git".to_string(),
                "-C".to_string(),
                repo.to_string(),
                "pull".to_string(),
                "origin".to_string(),
                "release".to_string(),
            ],
            "Update local release branch to latest.".to_string(),
        ),
        (
            "Checkout main",
            vec![
                "git".to_string(),
                "-C".to_string(),
                repo.to_string(),
                "checkout".to_string(),
                "main".to_string(),
            ],
            "Switch to main branch.".to_string(),
        ),
        (
            "Rebase main on release",
            vec![
                "git".to_string(),
                "-C".to_string(),
                repo.to_string(),
                "rebase".to_string(),
                "release".to_string(),
            ],
            "Rebase main onto release.".to_string(),
        ),
        (
            "Push main",
            vec![
                "git".to_string(),
                "-C".to_string(),
                repo.to_string(),
                "push".to_string(),
                "origin".to_string(),
                "main".to_string(),
            ],
            "Push main branch to remote.".to_string(),
        ),
        (
            "Create annotated tag",
            vec![
                "git".to_string(),
                "-C".to_string(),
                repo.to_string(),
                "tag".to_string(),
                "-a".to_string(),
                version.to_string(),
                "-m".to_string(),
                description.to_string(),
            ],
            format!("Create annotated tag '{}'.", version),
        ),
        (
            "Push tag",
            vec![
                "git".to_string(),
                "-C".to_string(),
                repo.to_string(),
                "push".to_string(),
                "origin".to_string(),
                version.to_string(),
            ],
            format!("Push tag '{}' to remote.", version),
        ),
    ];

    for (index, (_name, command, detail)) in commands.iter().enumerate() {
        if cancel_token.load(Ordering::SeqCst) {
            steps[index].status = "paused".into();
            steps[index].detail = "Cancelled before step started.".into();
            steps[index].log = "[Stop] Cancel requested by user.".into();
            return Err(TagRunFailure {
                summary: format!("Tag workflow cancelled by user before step {}.", index + 1),
                steps,
            });
        }
        log::info!(
            "[Workflow][Tag] Step {} starting: '{}' ({})",
            index + 1,
            steps[index].name,
            command.join(" ")
        );
        steps[index].status = "running".into();
        steps[index].detail = detail.clone().into();
        if let Some(progress) = on_progress.as_ref() {
            progress(
                steps.clone(),
                format!(
                    "Running step {}/{}: {}",
                    index + 1,
                    commands.len(),
                    steps[index].name
                ),
            );
        }

        let result = run_command(command.clone(), &cancel_token).await;
        match result {
            Ok(log) => {
                steps[index].status = "success".into();
                steps[index].log = clamp_step_log(log).into();
                if let Some(progress) = on_progress.as_ref() {
                    progress(
                        steps.clone(),
                        format!(
                            "Step {}/{} completed: {}",
                            index + 1,
                            commands.len(),
                            steps[index].name
                        ),
                    );
                }
                log::info!(
                    "[Workflow][Tag] Step {} succeeded: '{}'",
                    index + 1,
                    steps[index].name
                );
            }
            Err(log) => {
                if cancel_token.load(Ordering::SeqCst) {
                    steps[index].status = "paused".into();
                    steps[index].log = clamp_step_log(log.clone()).into();
                    steps[index].detail = "Cancelled by user. Workflow paused.".into();
                    for remaining in steps.iter_mut().skip(index + 1) {
                        if remaining.status.as_str() == "pending" {
                            remaining.status = "paused".into();
                            remaining.detail = "Paused due to cancellation request.".into();
                        }
                    }
                    if let Some(progress) = on_progress.as_ref() {
                        progress(
                            steps.clone(),
                            format!("Stop requested. Workflow paused at step {}.", index + 1),
                        );
                    }
                    log::warn!(
                        "[Workflow][Tag] Step {} cancelled by user: '{}'",
                        index + 1,
                        steps[index].name
                    );
                    return Err(TagRunFailure {
                        summary: format!(
                            "Tag workflow cancelled by user at step {} ('{}').",
                            index + 1,
                            steps[index].name
                        ),
                        steps,
                    });
                }
                steps[index].status = "error".into();
                steps[index].log = clamp_step_log(log.clone()).into();
                steps[index].detail = "Command failed. Check logs below.".into();
                if let Some(progress) = on_progress.as_ref() {
                    progress(
                        steps.clone(),
                        format!(
                            "Step {}/{} failed: {}",
                            index + 1,
                            commands.len(),
                            steps[index].name
                        ),
                    );
                }
                log::error!(
                    "[Workflow][Tag] Step {} failed: '{}'",
                    index + 1,
                    steps[index].name
                );
                return Err(TagRunFailure {
                    summary: format!(
                        "Tag workflow failed at step {} ('{}').",
                        index + 1,
                        steps[index].name
                    ),
                    steps,
                });
            }
        }
    }

    let complete_index = 8;
    steps[complete_index].status = "success".into();
    steps[complete_index].detail = "Local tag workflow finished successfully.".into();
    steps[complete_index].log = format!(
        "Tag '{}' has been created and pushed successfully.\nRepository: {}",
        version, repo
    )
    .into();
    if let Some(progress) = on_progress.as_ref() {
        progress(
            steps.clone(),
            format!(
                "Local tag workflow finished successfully. Active tag is '{}'.",
                version
            ),
        );
    }
    log::info!(
        "[Workflow][Tag] Local flow completed successfully: version='{}'",
        version
    );

    Ok(TagRunSuccess {
        summary: format!(
            "Local tag workflow completed successfully. Active tag is '{}'.",
            version
        ),
        current_version: version.to_string(),
        steps,
    })
}

async fn run_local_remove_tag_flow(
    repo: &str,
    tag: &str,
    on_progress: Option<TagProgressReporter>,
    cancel_token: Arc<AtomicBool>,
) -> Result<TagRunSuccess, TagRunFailure> {
    let mut steps = default_remove_tag_steps();
    let commands: Vec<Vec<String>> = vec![
        vec![
            "git".to_string(),
            "-C".to_string(),
            repo.to_string(),
            "rev-parse".to_string(),
            "--show-toplevel".to_string(),
        ],
        vec![
            "git".to_string(),
            "-C".to_string(),
            repo.to_string(),
            "tag".to_string(),
            "-d".to_string(),
            tag.to_string(),
        ],
        vec![
            "git".to_string(),
            "-C".to_string(),
            repo.to_string(),
            "push".to_string(),
            "--delete".to_string(),
            "origin".to_string(),
            tag.to_string(),
        ],
    ];

    for (index, command) in commands.iter().enumerate() {
        if cancel_token.load(Ordering::SeqCst) {
            steps[index].status = "paused".into();
            steps[index].detail = "Cancelled before step started.".into();
            steps[index].log = "[Stop] Cancel requested by user.".into();
            return Err(TagRunFailure {
                summary: format!(
                    "Tag remove workflow cancelled by user before step {}.",
                    index + 1
                ),
                steps,
            });
        }

        steps[index].status = "running".into();
        if let Some(progress) = on_progress.as_ref() {
            progress(
                steps.clone(),
                format!(
                    "Running remove step {}/{}: {}",
                    index + 1,
                    commands.len(),
                    steps[index].name
                ),
            );
        }

        match run_command(command.clone(), &cancel_token).await {
            Ok(log) => {
                steps[index].status = "success".into();
                steps[index].log = clamp_step_log(log).into();
                if let Some(progress) = on_progress.as_ref() {
                    progress(
                        steps.clone(),
                        format!(
                            "Remove step {}/{} completed: {}",
                            index + 1,
                            commands.len(),
                            steps[index].name
                        ),
                    );
                }
            }
            Err(log) => {
                if cancel_token.load(Ordering::SeqCst) {
                    steps[index].status = "paused".into();
                    steps[index].log = clamp_step_log(log.clone()).into();
                    steps[index].detail = "Cancelled by user. Workflow paused.".into();
                    for remaining in steps.iter_mut().skip(index + 1) {
                        if remaining.status.as_str() == "pending" {
                            remaining.status = "paused".into();
                            remaining.detail = "Paused due to cancellation request.".into();
                        }
                    }
                    if let Some(progress) = on_progress.as_ref() {
                        progress(
                            steps.clone(),
                            format!(
                                "Stop requested. Remove workflow paused at step {}.",
                                index + 1
                            ),
                        );
                    }
                    return Err(TagRunFailure {
                        summary: format!(
                            "Tag remove workflow cancelled by user at step {} ('{}').",
                            index + 1,
                            steps[index].name
                        ),
                        steps,
                    });
                }
                steps[index].status = "error".into();
                steps[index].log = clamp_step_log(log).into();
                steps[index].detail = "Command failed. Check logs below.".into();
                if let Some(progress) = on_progress.as_ref() {
                    progress(
                        steps.clone(),
                        format!(
                            "Remove step {}/{} failed: {}",
                            index + 1,
                            commands.len(),
                            steps[index].name
                        ),
                    );
                }
                return Err(TagRunFailure {
                    summary: format!(
                        "Tag remove workflow failed at step {} ('{}').",
                        index + 1,
                        steps[index].name
                    ),
                    steps,
                });
            }
        }
    }

    let refresh_index = 3;
    if cancel_token.load(Ordering::SeqCst) {
        steps[refresh_index].status = "paused".into();
        steps[refresh_index].detail = "Cancelled before refreshing latest remote tag.".into();
        steps[refresh_index].log = "[Stop] Cancel requested by user.".into();
        return Err(TagRunFailure {
            summary: "Tag remove workflow cancelled before refreshing latest remote tag."
                .to_string(),
            steps,
        });
    }
    steps[refresh_index].status = "running".into();
    steps[refresh_index].detail =
        "Fetching remote tags and resolving the latest semantic tag.".into();
    if let Some(progress) = on_progress.as_ref() {
        progress(
            steps.clone(),
            format!("Running remove step 4/4: {}", steps[refresh_index].name),
        );
    }
    let (latest_tag, refresh_log) = tokio::task::spawn_blocking({
        let repo = repo.to_string();
        move || inspect_latest_remote_tag(&repo)
    })
    .await
    .map_err(|error| TagRunFailure {
        summary: format!(
            "Tag remove workflow failed while joining refresh step: {}",
            error
        ),
        steps: {
            steps[refresh_index].status = "error".into();
            steps[refresh_index].detail = "Failed to join refresh task.".into();
            steps[refresh_index].log = format!("{}", error).into();
            steps.clone()
        },
    })?
    .map_err(|error| {
        steps[refresh_index].status = "error".into();
        steps[refresh_index].detail = "Failed to refresh latest remote tag.".into();
        steps[refresh_index].log = error.clone().into();
        TagRunFailure {
            summary: format!(
                "Tag remove workflow failed at step {} ('{}').",
                refresh_index + 1,
                steps[refresh_index].name
            ),
            steps: steps.clone(),
        }
    })?;
    steps[refresh_index].status = "success".into();
    steps[refresh_index].detail =
        format!("Latest remote semantic tag is now '{}'.", latest_tag).into();
    steps[refresh_index].log = refresh_log.into();
    if let Some(progress) = on_progress.as_ref() {
        progress(
            steps.clone(),
            format!("Remove step 4/4 completed: {}", steps[refresh_index].name),
        );
    }

    let complete_index = 4;
    steps[complete_index].status = "success".into();
    steps[complete_index].detail = format!(
        "Local remove workflow finished successfully. Input updated to '{}'.",
        latest_tag
    )
    .into();
    steps[complete_index].log = format!(
        "Tag '{}' removed successfully.\nLatest remote tag: '{}'.",
        tag, latest_tag
    )
    .into();
    if let Some(progress) = on_progress.as_ref() {
        progress(
            steps.clone(),
            format!(
                "Local tag remove workflow finished successfully. Removed '{}', latest tag '{}'.",
                tag, latest_tag
            ),
        );
    }

    Ok(TagRunSuccess {
        summary: format!(
            "Local tag remove workflow completed successfully. Removed '{}'. Latest remote tag is '{}'.",
            tag, latest_tag
        ),
        current_version: latest_tag,
        steps,
    })
}

async fn run_command(
    command: Vec<String>,
    cancel_token: &Arc<AtomicBool>,
) -> Result<String, String> {
    if command.is_empty() {
        log::error!("[Workflow][Tag] run_command called with empty command");
        return Err("Command vector is empty.".to_string());
    }
    let rendered = command.join(" ");
    if cancel_token.load(Ordering::SeqCst) {
        log::warn!(
            "[Workflow][Tag] Command cancelled before start: {}",
            rendered
        );
        return Err(format!("$ {}\n\nCancelled by user.", rendered));
    }
    log::info!("[Workflow][Tag] Executing command: {}", rendered);
    let run_with_capture = |command_vec: Vec<String>, capture: bool| {
        tokio::task::spawn_blocking(move || {
            let mut process = StdCommand::new(&command_vec[0]);
            for arg in command_vec.iter().skip(1) {
                process.arg(arg);
            }
            if capture {
                process
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()
                    .map(|output| (output.status, output.stdout, output.stderr))
            } else {
                process
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map(|status| (status, Vec::new(), Vec::new()))
            }
        })
    };

    let output = match run_with_capture(command.clone(), true).await {
        Ok(result) => match result {
            Ok(result) => result,
            Err(error) if error.raw_os_error() == Some(9) => {
                log::warn!(
                    "[Workflow][Tag] Command spawn failed with bad fd, retry without capture: {}",
                    rendered
                );
                let retry = run_with_capture(command.clone(), false)
                    .await
                    .map_err(|error| {
                        log::error!(
                            "[Workflow][Tag] Command join failed: {} | error={}",
                            rendered,
                            error
                        );
                        format!("$ {}\n\nFailed to join command:\n{}", rendered, error)
                    })?
                    .map_err(|error| {
                        log::error!(
                            "[Workflow][Tag] Command spawn failed: {} | error={}",
                            rendered,
                            error
                        );
                        format!("$ {}\n\nFailed to spawn command:\n{}", rendered, error)
                    })?;
                retry
            }
            Err(error) => {
                log::error!(
                    "[Workflow][Tag] Command spawn failed: {} | error={}",
                    rendered,
                    error
                );
                return Err(format!(
                    "$ {}\n\nFailed to spawn command:\n{}",
                    rendered, error
                ));
            }
        },
        Err(error) => {
            log::error!(
                "[Workflow][Tag] Command join failed: {} | error={}",
                rendered,
                error
            );
            return Err(format!(
                "$ {}\n\nFailed to join command:\n{}",
                rendered, error
            ));
        }
    };

    let (status, stdout_bytes, stderr_bytes) = output;
    if cancel_token.load(Ordering::SeqCst) {
        log::warn!(
            "[Workflow][Tag] Command result ignored due to cancellation: {}",
            rendered
        );
        return Err(format!("$ {}\n\nCancelled by user.", rendered));
    }

    let stdout = String::from_utf8_lossy(&stdout_bytes);
    let stderr = String::from_utf8_lossy(&stderr_bytes);
    let log = clamp_step_log(format!(
        "$ {}\n\nstdout:\n{}\n\nstderr:\n{}",
        rendered, stdout, stderr
    ));
    if status.success() {
        log::info!("[Workflow][Tag] Command succeeded: {}", rendered);
        Ok(log)
    } else {
        log::error!(
            "[Workflow][Tag] Command failed: {} | status={}",
            rendered,
            status
        );
        Err(log)
    }
}

fn read_latest_local_tag(local_repo: &str) -> Result<String, String> {
    log::info!(
        "[Workflow][Tag] Reading latest local tag from repo='{}'",
        local_repo
    );
    if local_repo.trim().is_empty() {
        log::info!("[Workflow][Tag] Local repo is empty, fallback to v0.0.0");
        return Ok("v0.0.0".to_string());
    }
    if !Path::new(local_repo).exists() {
        log::warn!(
            "[Workflow][Tag] Local repo path does not exist: '{}'",
            local_repo
        );
        return Err("The selected local repository path does not exist.".to_string());
    }

    let output = StdCommand::new("git")
        .args(["-C", local_repo, "tag", "--list", "--sort=-version:refname"])
        .output()
        .map_err(|error| format!("Failed to inspect local tags: {}", error))?;
    if !output.status.success() {
        log::warn!(
            "[Workflow][Tag] Failed reading local tags, git exit status={}",
            output.status
        );
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let tag = line.trim();
        if parse_semver_tag(tag).is_some() {
            log::info!("[Workflow][Tag] Latest semantic tag resolved: '{}'", tag);
            return Ok(tag.to_string());
        }
    }
    log::info!("[Workflow][Tag] No semantic tag found, fallback to v0.0.0");
    Ok("v0.0.0".to_string())
}

fn inspect_latest_remote_tag(local_repo: &str) -> Result<(String, String), String> {
    log::info!(
        "[Workflow][Tag] Reading latest remote tag from repo='{}'",
        local_repo
    );
    if local_repo.trim().is_empty() {
        return Ok((
            "v0.0.0".to_string(),
            "[Init] Local repo is empty, fallback to v0.0.0.".to_string(),
        ));
    }
    if !Path::new(local_repo).exists() {
        return Err("The selected local repository path does not exist.".to_string());
    }

    let fetch_cmd = vec![
        "git".to_string(),
        "-C".to_string(),
        local_repo.to_string(),
        "fetch".to_string(),
        "origin".to_string(),
        "--tags".to_string(),
        "--force".to_string(),
    ];
    run_command_blocking(fetch_cmd)?;

    let list_cmd = vec![
        "git".to_string(),
        "-C".to_string(),
        local_repo.to_string(),
        "ls-remote".to_string(),
        "--tags".to_string(),
        "--refs".to_string(),
        "origin".to_string(),
    ];
    run_command_blocking(list_cmd.clone())?;

    let output = StdCommand::new(&list_cmd[0])
        .args(list_cmd.iter().skip(1))
        .output()
        .map_err(|error| format!("Failed to inspect remote tags: {}", error))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut latest: Option<(String, (u64, u64, u64))> = None;
    for line in stdout.lines() {
        let Some(tag_ref) = line.split_whitespace().nth(1) else {
            continue;
        };
        let Some(tag) = tag_ref.strip_prefix("refs/tags/") else {
            continue;
        };
        let Some(parsed) = parse_semver_tag(tag) else {
            continue;
        };
        match latest.as_ref() {
            Some((_, current)) if *current >= parsed => {}
            _ => latest = Some((tag.to_string(), parsed)),
        }
    }

    let latest_tag = latest
        .map(|(tag, _)| tag)
        .unwrap_or_else(|| "v0.0.0".to_string());
    let combined_log = format!(
        "[Init] Refreshed remote tags and resolved latest semantic tag: {}",
        latest_tag
    );
    log::info!(
        "[Workflow][Tag] Latest remote semantic tag resolved: '{}'",
        latest_tag
    );
    Ok((latest_tag, combined_log))
}

fn run_command_blocking(command: Vec<String>) -> Result<String, String> {
    if command.is_empty() {
        return Err("Command vector is empty.".to_string());
    }
    let rendered = command.join(" ");
    let output = StdCommand::new(&command[0])
        .args(command.iter().skip(1))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("$ {}\n\nFailed to spawn command:\n{}", rendered, error))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let log = clamp_step_log(format!(
        "$ {}\n\nstdout:\n{}\n\nstderr:\n{}",
        rendered, stdout, stderr
    ));
    if output.status.success() {
        Ok(log)
    } else {
        Err(log)
    }
}

fn next_semver_for_mode(current_version: &str, version_mode: &str) -> String {
    let (major, minor, patch) = parse_semver_tag(current_version).unwrap_or((0, 0, 0));
    match version_mode {
        TAG_BUMP_MAJOR => format!("v{}.0.0", major + 1),
        TAG_BUMP_MINOR => format!("v{}.{}.0", major, minor + 1),
        _ => format!("v{}.{}.{}", major, minor, patch + 1),
    }
}

fn parse_semver_tag(tag: &str) -> Option<(u64, u64, u64)> {
    let normalized = tag.trim().trim_start_matches(['v', 'V']);
    let core = normalized.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn is_valid_semver_tag(tag: &str) -> bool {
    parse_semver_tag(tag).is_some()
}
