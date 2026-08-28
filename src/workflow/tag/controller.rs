use crate::app_runtime::shared_runtime;
use crate::jobs::{
    JobConclusion, JobRunDescriptor, JobStepData, JobStepDefinition, JobStepStatus,
    format_duration, register_job_run_source,
};
use crate::workflow::bindings::{show_error, show_success};
use crate::workflow::state::{SharedWorkflowState, default_tag_steps, initialize_current_ui};
use crate::{MainWindow, Route};
use slint::ComponentHandle;
use std::path::Path;
use std::process::{Command as StdCommand, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::sync::mpsc;
use tokio::time::{Duration as TokioDuration, sleep};

use super::commands::{create_tag_plan, remove_tag_plan};
use super::version::{is_valid_semver_tag, next_semver_for_mode, parse_semver_tag};

const TAG_MODE_LOCAL: &str = "local";
const TAG_MODE_REMOTE: &str = "remote";
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

fn format_command_log(rendered: &str, stdout: &str, stderr: &str) -> String {
    let mut lines = vec![format!("Run {rendered}")];
    lines.extend(stdout.lines().map(str::to_owned));
    lines.extend(stderr.lines().map(str::to_owned));
    clamp_step_log(lines.join("\n"))
}

fn set_step_log(step: &mut JobStepData, log: impl Into<String>) {
    step.log = clamp_step_log(log.into());
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
    if let Ok(mut slot) = cancel_slot().lock()
        && let Some(current) = slot.as_ref()
        && Arc::ptr_eq(current, token)
    {
        *slot = None;
    }
}

fn request_tag_cancel() -> bool {
    if let Ok(slot) = cancel_slot().lock()
        && let Some(token) = slot.as_ref()
    {
        token.store(true, Ordering::SeqCst);
        return true;
    }
    false
}

pub(crate) fn bind_tag(ui: &MainWindow, shared_state: SharedWorkflowState) {
    log::info!("[Workflow][Tag] Binding Tag tab callbacks");
    let ui_weak_stop = ui.as_weak();
    let ui_weak_log = ui.as_weak();
    register_job_run_source(
        "tag",
        move || {
            if let Some(ui) = ui_weak_stop.upgrade() {
                ui.invoke_workflow_tag_stop();
            }
        },
        move |index| {
            if let Some(ui) = ui_weak_log.upgrade() {
                ui.invoke_workflow_tag_load_step_log(index);
            }
        },
    );

    let ui_weak = ui.as_weak();
    let state_logs = shared_state.clone();
    ui.on_workflow_tag_load_step_log(move |index| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let requested = state_logs
            .lock()
            .ok()
            .map(|mut guard| {
                let mut requested = false;
                guard.upsert_current(|view| {
                    requested = view.tag_job.request_step_log(index.max(0) as usize);
                });
                requested
            })
            .unwrap_or(false);
        if requested {
            initialize_current_ui(&ui, &state_logs);
        }
    });

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
                view.reset_tag_job(default_tag_steps());
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
                if !view.tag_running {
                    view.reset_tag_job(default_tag_steps());
                    view.tag_result = if view.tag_mode == TAG_MODE_REMOTE {
                        "Remote mode is not available yet.".to_string()
                    } else {
                        format!(
                            "Local mode ready. Current latest tag is '{}', next {} version prepared.",
                            view.tag_current_version, version_mode
                        )
                    };
                }
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
            if let Ok(mut guard) = state_add.lock() {
                guard.upsert_current(|view| {
                    view.tag_selected_tab = 0;
                    // Always restore the create plan so a prior run's expanded /
                    // failed steps cannot leak into the Add tab surface.
                    view.reset_tag_job(default_tag_steps());
                    view.tag_result = format!(
                        "Local mode ready. Current latest tag is '{}', next {} version prepared.",
                        view.tag_current_version, view.tag_version_mode
                    );
                });
            }
            initialize_current_ui(&ui, &state_add);
            return;
        }

        if mode == TAG_MODE_REMOTE {
            if let Ok(mut guard) = state_add.lock() {
                guard.upsert_current(|view| {
                    view.tag_selected_tab = 0;
                    view.tag_add_request_seq = view.tag_add_request_seq.saturating_add(1);
                    view.tag_result = "Remote mode is not available yet.".to_string();
                    view.tag_add_initializing = false;
                    view.tag_add_ready = false;
                    view.reset_tag_job(default_tag_steps());
                });
            }
            initialize_current_ui(&ui, &state_add);
            return;
        }

        let request_seq = if let Ok(mut guard) = state_add.lock() {
            let mut request_seq = 0;
            guard.upsert_current(|view| {
                view.tag_selected_tab = 0;
                view.tag_add_request_seq = view.tag_add_request_seq.saturating_add(1);
                request_seq = view.tag_add_request_seq;
                view.tag_add_initializing = true;
                view.tag_add_ready = false;
                view.tag_result = format!(
                    "Loading latest local tag for add workflow in project '{}'.",
                    project_name
                );
                view.reset_tag_job(default_tag_steps());
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
            if let Ok(mut guard) = state_open_remove.lock() {
                guard.upsert_current(|view| {
                    view.tag_selected_tab = 1;
                    // Always restore the remove plan so a prior create run's
                    // steps/disclosures cannot leak into the Remove tab.
                    view.reset_tag_job(default_remove_tag_steps());
                    view.tag_result = format!(
                        "Remove mode ready. Latest remote tag is '{}'.",
                        view.tag_remove_target
                    );
                });
            }
            initialize_current_ui(&ui, &state_open_remove);
            return;
        }

        if mode == TAG_MODE_REMOTE {
            if let Ok(mut guard) = state_open_remove.lock() {
                guard.upsert_current(|view| {
                    view.tag_selected_tab = 1;
                    view.tag_remove_request_seq = view.tag_remove_request_seq.saturating_add(1);
                    view.tag_result = "Remote mode is not available yet.".to_string();
                    view.tag_remove_initializing = false;
                    view.tag_remove_ready = false;
                    view.tag_remove_target = "Unavailable".to_string();
                    view.reset_tag_job(default_remove_tag_steps());
                });
            }
            initialize_current_ui(&ui, &state_open_remove);
            return;
        }

        let request_seq = if let Ok(mut guard) = state_open_remove.lock() {
            let mut request_seq = 0;
            guard.upsert_current(|view| {
                view.tag_selected_tab = 1;
                view.tag_remove_request_seq = view.tag_remove_request_seq.saturating_add(1);
                request_seq = view.tag_remove_request_seq;
                view.tag_add_initializing = false;
                view.tag_remove_initializing = true;
                view.tag_remove_ready = false;
                view.tag_result = format!(
                    "Loading latest remote tag for remove workflow in project '{}'.",
                    project_name
                );
                view.reset_tag_job(default_remove_tag_steps());
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
                                if let Some(first) = entry.tag_job.steps.get_mut(0) {
                                    first.detail = format!(
                                        "Latest remote tag resolved for removal: '{}'.",
                                        latest_tag
                                    );
                                    set_step_log(first, lookup_log);
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
                    view.tag_result = "Remote mode is not available yet.".to_string();
                    view.tag_running = false;
                    view.tag_add_ready = false;
                    view.tag_remove_ready = false;
                    view.tag_add_request_seq = view.tag_add_request_seq.saturating_add(1);
                    view.tag_remove_request_seq = view.tag_remove_request_seq.saturating_add(1);
                    view.reset_tag_job(default_tag_steps());
                });
            } else {
                log::warn!(
                    "[Workflow][Tag] Failed to acquire state lock while handling unavailable remote mode"
                );
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
                view.tag_job.start(default_tag_steps());
                view.job_run = JobRunDescriptor {
                    active: true,
                    source: "tag".to_string(),
                    run_title: format!("Create tag {version}"),
                    workflow_name: "Tag workflow".to_string(),
                    context_name: project_name.clone(),
                    job_name: "create-tag".to_string(),
                    trigger_detail: "Started manually from Tag".to_string(),
                };
            });
            log::info!(
                "[Workflow][Tag] Run accepted: state switched to running for version '{}'",
                version
            );
        } else {
            log::warn!("[Workflow][Tag] Failed to acquire state lock before scheduling run");
        }
        initialize_current_ui(&ui, &state_run);
        ui.invoke_navigate(Route::JobRun);

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
                        let entry = guard.projects.entry(project_local.clone()).or_default();
                        // Keep tag_running latched for the whole run. Deriving it
                        // from step statuses flashes Stop away between steps
                        // (and on the final success/error progress tick) because
                        // no row is "running" for those moments.
                        entry.tag_job.replace_steps(steps);
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
            let job_started = Instant::now();
            let execution = run_local_tag_flow(
                &local_repo,
                &version,
                &description,
                Some(progress_reporter),
                cancel_token.clone(),
            )
            .await;
            let total_duration = format_duration(job_started.elapsed());
            let was_cancelled = cancel_token.load(Ordering::SeqCst);
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
                        let conclusion = match &execution {
                            Ok(_) => JobConclusion::Success,
                            Err(_) if was_cancelled => JobConclusion::Cancelled,
                            Err(_) => JobConclusion::Failure,
                        };
                        guard.projects.entry(project_id.clone()).or_default().tag_add_initializing = false;
                        match execution {
                            Ok(ref success) => {
                                let entry =
                                    guard.projects.entry(project_id.clone()).or_default();
                                entry.tag_current_version = success.current_version.clone();
                                entry.tag_next_version =
                                    next_semver_for_mode(&success.current_version, &bump_mode);
                                entry.tag_remove_target = success.current_version.clone();
                                entry.tag_add_ready = false;
                                entry.tag_remove_ready = false;
                                entry.tag_job.finish(
                                    conclusion,
                                    total_duration.clone(),
                                    success.steps.clone(),
                                );
                            }
                            Err(ref failure) => {
                                let entry =
                                    guard.projects.entry(project_id.clone()).or_default();
                                entry.tag_add_ready = false;
                                entry.tag_job.finish(
                                    conclusion,
                                    total_duration.clone(),
                                    failure.steps.clone(),
                                );
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
                    view.tag_result = "Remote mode is not available yet.".to_string();
                    view.tag_running = false;
                    view.tag_add_initializing = false;
                    view.tag_remove_initializing = false;
                    view.tag_add_ready = false;
                    view.tag_remove_ready = false;
                    view.tag_add_request_seq = view.tag_add_request_seq.saturating_add(1);
                    view.tag_remove_request_seq = view.tag_remove_request_seq.saturating_add(1);
                    view.reset_tag_job(default_remove_tag_steps());
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
                view.tag_job.start(default_remove_tag_steps());
                view.job_run = JobRunDescriptor {
                    active: true,
                    source: "tag".to_string(),
                    run_title: format!("Remove tag {tag}"),
                    workflow_name: "Tag workflow".to_string(),
                    context_name: project_name.clone(),
                    job_name: "remove-tag".to_string(),
                    trigger_detail: "Started manually from Tag".to_string(),
                };
            });
        }
        initialize_current_ui(&ui, &state_remove);
        ui.invoke_navigate(Route::JobRun);

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
                        let entry = guard.projects.entry(project_local.clone()).or_default();
                        // Same latch as create: only the terminal callback/stop
                        // handler clears tag_running.
                        entry.tag_job.replace_steps(steps);
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
            let job_started = Instant::now();
            let execution = run_local_remove_tag_flow(
                &local_repo,
                &tag,
                Some(progress_reporter),
                cancel_token.clone(),
            )
            .await;
            let total_duration = format_duration(job_started.elapsed());
            let was_cancelled = cancel_token.load(Ordering::SeqCst);
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
                        let conclusion = match &execution {
                            Ok(_) => JobConclusion::Success,
                            Err(_) if was_cancelled => JobConclusion::Cancelled,
                            Err(_) => JobConclusion::Failure,
                        };
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
                                entry.tag_job.finish(
                                    conclusion,
                                    total_duration.clone(),
                                    success.steps.clone(),
                                );
                            }
                            Err(ref failure) => {
                                let entry = guard.projects.entry(project_id.clone()).or_default();
                                entry.tag_remove_ready = false;
                                entry.tag_job.finish(
                                    conclusion,
                                    total_duration.clone(),
                                    failure.steps.clone(),
                                );
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
                for step in &mut view.tag_job.steps {
                    if step.status == JobStepStatus::InProgress {
                        step.status = JobStepStatus::Completed;
                        step.conclusion = Some(JobConclusion::Cancelled);
                        step.detail =
                            "Cancelled by user. Waiting for current command to stop.".into();
                        let current_log = step.log.to_string();
                        let updated_log = if current_log.is_empty() {
                            "[Stop] Cancel requested by user.".to_string()
                        } else {
                            format!("{}\n[Stop] Cancel requested by user.", current_log)
                        };
                        set_step_log(step, updated_log);
                    } else if step.status == JobStepStatus::Queued {
                        step.status = JobStepStatus::Completed;
                        step.conclusion = Some(JobConclusion::Cancelled);
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
            view.reset_tag_job(default_tag_steps());
            if mode == TAG_MODE_REMOTE {
                log::info!("[Workflow][Tag] Refresh context -> remote mode unavailable");
                view.tag_current_version = "Unavailable".to_string();
                view.tag_next_version = "Unavailable".to_string();
                view.tag_remove_target = "Unavailable".to_string();
                view.tag_result = "Remote mode is not available yet.".to_string();
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
    steps: Vec<JobStepData>,
}

struct TagRunFailure {
    summary: String,
    steps: Vec<JobStepData>,
}

type TagProgressReporter = Arc<dyn Fn(Vec<JobStepData>, String) + Send + Sync>;

fn default_remove_tag_steps() -> Vec<JobStepData> {
    vec![
        JobStepData::queued(
            JobStepDefinition::new(
                "Enter local repository",
                vec!["git".into(), "rev-parse".into(), "--show-toplevel".into()],
            ),
            "Resolve git repository path.",
        ),
        JobStepData::queued(
            JobStepDefinition::new(
                "Delete local tag",
                vec!["git".into(), "tag".into(), "-d".into(), "<tag>".into()],
            ),
            "Delete tag from local repository.",
        ),
        JobStepData::queued(
            JobStepDefinition::new(
                "Delete remote tag",
                vec![
                    "git".into(),
                    "push".into(),
                    "--delete".into(),
                    "origin".into(),
                    "<tag>".into(),
                ],
            ),
            "Delete tag from origin remote.",
        ),
        JobStepData::queued(
            JobStepDefinition::new(
                "Refresh latest remote tag",
                vec!["git".into(), "fetch".into(), "--tags".into()],
            ),
            "Fetch and resolve the latest remote semantic tag.",
        ),
        JobStepData::queued(
            JobStepDefinition::new("Workflow complete", vec!["complete".into()]),
            "Waiting for remove workflow completion.",
        ),
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
    let commands = create_tag_plan(repo, version, description);
    let mut steps: Vec<JobStepData> = commands
        .iter()
        .map(|planned| {
            JobStepData::queued(
                JobStepDefinition::new(planned.name, planned.argv.clone()),
                planned.detail.clone(),
            )
        })
        .collect();
    steps.push(JobStepData::queued(
        JobStepDefinition::new("Complete", vec!["complete".into()]),
        "Finish the local tag workflow.",
    ));

    for (index, planned) in commands.iter().enumerate() {
        let command = &planned.argv;
        let detail = &planned.detail;
        if cancel_token.load(Ordering::SeqCst) {
            steps[index].status = JobStepStatus::Completed;
            steps[index].conclusion = Some(JobConclusion::Cancelled);
            steps[index].detail = "Cancelled before step started.".into();
            set_step_log(&mut steps[index], "[Stop] Cancel requested by user.");
            return Err(TagRunFailure {
                summary: format!("Tag workflow cancelled by user before step {}.", index + 1),
                steps,
            });
        }
        log::info!(
            "[Workflow][Tag] Step {} starting: '{}' ({})",
            index + 1,
            steps[index].definition.name,
            command.join(" ")
        );
        let step_started = Instant::now();
        steps[index].status = JobStepStatus::InProgress;
        steps[index].conclusion = None;
        steps[index].duration = "…".into();
        steps[index].detail = detail.clone();
        if let Some(progress) = on_progress.as_ref() {
            progress(
                steps.clone(),
                format!(
                    "Running step {}/{}: {}",
                    index + 1,
                    commands.len(),
                    steps[index].definition.name
                ),
            );
        }

        let running_summary = format!(
            "Running step {}/{}: {}",
            index + 1,
            commands.len(),
            steps[index].definition.name
        );
        let result = run_command(command.clone(), &cancel_token, |live_log| {
            set_step_log(&mut steps[index], live_log);
            if let Some(progress) = on_progress.as_ref() {
                progress(steps.clone(), running_summary.clone());
            }
        })
        .await;
        steps[index].duration = format_duration(step_started.elapsed());
        match result {
            Ok(log) => {
                steps[index].status = JobStepStatus::Completed;
                steps[index].conclusion = Some(JobConclusion::Success);
                set_step_log(&mut steps[index], log);
                if let Some(progress) = on_progress.as_ref() {
                    progress(
                        steps.clone(),
                        format!(
                            "Step {}/{} completed: {}",
                            index + 1,
                            commands.len(),
                            steps[index].definition.name
                        ),
                    );
                }
                log::info!(
                    "[Workflow][Tag] Step {} succeeded: '{}'",
                    index + 1,
                    steps[index].definition.name
                );
            }
            Err(log) => {
                if cancel_token.load(Ordering::SeqCst) {
                    steps[index].status = JobStepStatus::Completed;
                    steps[index].conclusion = Some(JobConclusion::Cancelled);
                    set_step_log(&mut steps[index], log.clone());
                    steps[index].detail = "Cancelled by user. Workflow paused.".into();
                    for remaining in steps.iter_mut().skip(index + 1) {
                        if remaining.status == JobStepStatus::Queued {
                            remaining.status = JobStepStatus::Completed;
                            remaining.conclusion = Some(JobConclusion::Cancelled);
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
                        steps[index].definition.name
                    );
                    return Err(TagRunFailure {
                        summary: format!(
                            "Tag workflow cancelled by user at step {} ('{}').",
                            index + 1,
                            steps[index].definition.name
                        ),
                        steps,
                    });
                }
                steps[index].status = JobStepStatus::Completed;
                steps[index].conclusion = Some(JobConclusion::Failure);
                steps[index].default_expanded = true;
                set_step_log(&mut steps[index], log.clone());
                steps[index].detail = "Command failed. Check logs below.".into();
                if let Some(progress) = on_progress.as_ref() {
                    progress(
                        steps.clone(),
                        format!(
                            "Step {}/{} failed: {}",
                            index + 1,
                            commands.len(),
                            steps[index].definition.name
                        ),
                    );
                }
                log::error!(
                    "[Workflow][Tag] Step {} failed: '{}'",
                    index + 1,
                    steps[index].definition.name
                );
                return Err(TagRunFailure {
                    summary: format!(
                        "Tag workflow failed at step {} ('{}').",
                        index + 1,
                        steps[index].definition.name
                    ),
                    steps,
                });
            }
        }
    }

    let complete_index = 8;
    steps[complete_index].status = JobStepStatus::Completed;
    steps[complete_index].conclusion = Some(JobConclusion::Success);
    steps[complete_index].duration = "0s".into();
    steps[complete_index].detail = "Local tag workflow finished successfully.".into();
    set_step_log(
        &mut steps[complete_index],
        format!(
            "Tag '{}' has been created and pushed successfully.\nRepository: {}",
            version, repo
        ),
    );
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
    let commands = remove_tag_plan(repo, tag);
    let mut steps = default_remove_tag_steps();
    for (step, command) in steps.iter_mut().zip(commands.iter()) {
        step.definition.command = command.clone();
    }

    for (index, command) in commands.iter().enumerate() {
        if cancel_token.load(Ordering::SeqCst) {
            steps[index].status = JobStepStatus::Completed;
            steps[index].conclusion = Some(JobConclusion::Cancelled);
            steps[index].detail = "Cancelled before step started.".into();
            set_step_log(&mut steps[index], "[Stop] Cancel requested by user.");
            return Err(TagRunFailure {
                summary: format!(
                    "Tag remove workflow cancelled by user before step {}.",
                    index + 1
                ),
                steps,
            });
        }

        let step_started = Instant::now();
        steps[index].status = JobStepStatus::InProgress;
        steps[index].conclusion = None;
        steps[index].duration = "…".into();
        if let Some(progress) = on_progress.as_ref() {
            progress(
                steps.clone(),
                format!(
                    "Running remove step {}/{}: {}",
                    index + 1,
                    commands.len(),
                    steps[index].definition.name
                ),
            );
        }

        let running_summary = format!(
            "Running remove step {}/{}: {}",
            index + 1,
            commands.len(),
            steps[index].definition.name
        );
        let result = run_command(command.clone(), &cancel_token, |live_log| {
            set_step_log(&mut steps[index], live_log);
            if let Some(progress) = on_progress.as_ref() {
                progress(steps.clone(), running_summary.clone());
            }
        })
        .await;
        steps[index].duration = format_duration(step_started.elapsed());
        match result {
            Ok(log) => {
                steps[index].status = JobStepStatus::Completed;
                steps[index].conclusion = Some(JobConclusion::Success);
                set_step_log(&mut steps[index], log);
                if let Some(progress) = on_progress.as_ref() {
                    progress(
                        steps.clone(),
                        format!(
                            "Remove step {}/{} completed: {}",
                            index + 1,
                            commands.len(),
                            steps[index].definition.name
                        ),
                    );
                }
            }
            Err(log) => {
                if cancel_token.load(Ordering::SeqCst) {
                    steps[index].status = JobStepStatus::Completed;
                    steps[index].conclusion = Some(JobConclusion::Cancelled);
                    set_step_log(&mut steps[index], log.clone());
                    steps[index].detail = "Cancelled by user. Workflow paused.".into();
                    for remaining in steps.iter_mut().skip(index + 1) {
                        if remaining.status == JobStepStatus::Queued {
                            remaining.status = JobStepStatus::Completed;
                            remaining.conclusion = Some(JobConclusion::Cancelled);
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
                            steps[index].definition.name
                        ),
                        steps,
                    });
                }
                steps[index].status = JobStepStatus::Completed;
                steps[index].conclusion = Some(JobConclusion::Failure);
                steps[index].default_expanded = true;
                set_step_log(&mut steps[index], log);
                steps[index].detail = "Command failed. Check logs below.".into();
                if let Some(progress) = on_progress.as_ref() {
                    progress(
                        steps.clone(),
                        format!(
                            "Remove step {}/{} failed: {}",
                            index + 1,
                            commands.len(),
                            steps[index].definition.name
                        ),
                    );
                }
                return Err(TagRunFailure {
                    summary: format!(
                        "Tag remove workflow failed at step {} ('{}').",
                        index + 1,
                        steps[index].definition.name
                    ),
                    steps,
                });
            }
        }
    }

    let refresh_index = 3;
    if cancel_token.load(Ordering::SeqCst) {
        steps[refresh_index].status = JobStepStatus::Completed;
        steps[refresh_index].conclusion = Some(JobConclusion::Cancelled);
        steps[refresh_index].detail = "Cancelled before refreshing latest remote tag.".into();
        set_step_log(
            &mut steps[refresh_index],
            "[Stop] Cancel requested by user.",
        );
        return Err(TagRunFailure {
            summary: "Tag remove workflow cancelled before refreshing latest remote tag."
                .to_string(),
            steps,
        });
    }
    let refresh_started = Instant::now();
    steps[refresh_index].status = JobStepStatus::InProgress;
    steps[refresh_index].conclusion = None;
    steps[refresh_index].duration = "…".into();
    steps[refresh_index].detail =
        "Fetching remote tags and resolving the latest semantic tag.".into();
    if let Some(progress) = on_progress.as_ref() {
        progress(
            steps.clone(),
            format!(
                "Running remove step 4/4: {}",
                steps[refresh_index].definition.name
            ),
        );
    }
    let refresh_summary = format!(
        "Running remove step 4/4: {}",
        steps[refresh_index].definition.name
    );
    let refresh_result = inspect_latest_remote_tag_live(repo, &cancel_token, |live_log| {
        set_step_log(&mut steps[refresh_index], live_log);
        if let Some(progress) = on_progress.as_ref() {
            progress(steps.clone(), refresh_summary.clone());
        }
    })
    .await;
    let (latest_tag, refresh_log) = match refresh_result {
        Ok(result) => result,
        Err(error) => {
            steps[refresh_index].status = JobStepStatus::Completed;
            steps[refresh_index].conclusion = Some(if cancel_token.load(Ordering::SeqCst) {
                JobConclusion::Cancelled
            } else {
                JobConclusion::Failure
            });
            steps[refresh_index].default_expanded = true;
            steps[refresh_index].duration = format_duration(refresh_started.elapsed());
            steps[refresh_index].detail = if cancel_token.load(Ordering::SeqCst) {
                "Cancelled by user. Workflow paused.".into()
            } else {
                "Failed to refresh latest remote tag.".into()
            };
            set_step_log(&mut steps[refresh_index], error);
            return Err(TagRunFailure {
                summary: if cancel_token.load(Ordering::SeqCst) {
                    "Tag remove workflow cancelled while refreshing remote tags.".into()
                } else {
                    format!(
                        "Tag remove workflow failed at step {} ('{}').",
                        refresh_index + 1,
                        steps[refresh_index].definition.name
                    )
                },
                steps,
            });
        }
    };
    steps[refresh_index].status = JobStepStatus::Completed;
    steps[refresh_index].conclusion = Some(JobConclusion::Success);
    steps[refresh_index].duration = format_duration(refresh_started.elapsed());
    steps[refresh_index].detail = format!("Latest remote semantic tag is now '{}'.", latest_tag);
    set_step_log(&mut steps[refresh_index], refresh_log);
    if let Some(progress) = on_progress.as_ref() {
        progress(
            steps.clone(),
            format!(
                "Remove step 4/4 completed: {}",
                steps[refresh_index].definition.name
            ),
        );
    }

    let complete_index = 4;
    steps[complete_index].status = JobStepStatus::Completed;
    steps[complete_index].conclusion = Some(JobConclusion::Success);
    steps[complete_index].duration = "0s".into();
    steps[complete_index].detail = format!(
        "Local remove workflow finished successfully. Input updated to '{}'.",
        latest_tag
    );
    set_step_log(
        &mut steps[complete_index],
        format!(
            "Tag '{}' removed successfully.\nLatest remote tag: '{}'.",
            tag, latest_tag
        ),
    );
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

async fn read_command_stream<R>(reader: R, sender: mpsc::UnboundedSender<String>)
where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        match reader.read_until(b'\n', &mut bytes).await {
            Ok(0) => break,
            Ok(_) => {
                while bytes
                    .last()
                    .is_some_and(|byte| matches!(byte, b'\n' | b'\r'))
                {
                    bytes.pop();
                }
                let _ = sender.send(String::from_utf8_lossy(&bytes).into_owned());
            }
            Err(error) => {
                let _ = sender.send(format!("error: failed to read process output: {error}"));
                break;
            }
        }
    }
}

async fn inspect_latest_remote_tag_live<F>(
    local_repo: &str,
    cancel_token: &Arc<AtomicBool>,
    mut on_log: F,
) -> Result<(String, String), String>
where
    F: FnMut(String),
{
    let fetch_command = vec![
        "git".to_string(),
        "-C".to_string(),
        local_repo.to_string(),
        "fetch".to_string(),
        "origin".to_string(),
        "--tags".to_string(),
        "--force".to_string(),
    ];
    let fetch_log = run_command(fetch_command, cancel_token, &mut on_log).await?;

    let list_command = vec![
        "git".to_string(),
        "-C".to_string(),
        local_repo.to_string(),
        "ls-remote".to_string(),
        "--tags".to_string(),
        "--refs".to_string(),
        "origin".to_string(),
    ];
    let list_log = run_command(list_command, cancel_token, |log| {
        on_log(clamp_step_log(format!("{fetch_log}\n{log}")));
    })
    .await?;
    let combined_log = clamp_step_log(format!("{fetch_log}\n{list_log}"));

    let mut latest: Option<(String, (u64, u64, u64))> = None;
    for line in list_log.lines() {
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
    Ok((
        latest
            .map(|(tag, _)| tag)
            .unwrap_or_else(|| "v0.0.0".to_string()),
        combined_log,
    ))
}

fn append_live_log(log: &mut String, line: String) {
    if !log.is_empty() {
        log.push('\n');
    }
    log.push_str(&line);
    *log = clamp_step_log(std::mem::take(log));
}

async fn run_command<F>(
    command: Vec<String>,
    cancel_token: &Arc<AtomicBool>,
    mut on_log: F,
) -> Result<String, String>
where
    F: FnMut(String),
{
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
        return Err(format!("Run {}\nCancelled by user.", rendered));
    }
    log::info!("[Workflow][Tag] Executing command: {}", rendered);
    let mut process = TokioCommand::new(&command[0]);
    process.args(command.iter().skip(1));
    process
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = process.spawn().map_err(|error| {
        log::error!(
            "[Workflow][Tag] Command spawn failed: {} | error={}",
            rendered,
            error
        );
        format!(
            "Run {}\nerror: failed to spawn command: {}",
            rendered, error
        )
    })?;

    let stdout = child
        .stdout
        .take()
        .expect("piped command stdout must be available");
    let stderr = child
        .stderr
        .take()
        .expect("piped command stderr must be available");
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let stdout_task = tokio::spawn(read_command_stream(stdout, sender.clone()));
    let stderr_task = tokio::spawn(read_command_stream(stderr, sender));

    let mut command_log = format!("Run {rendered}");
    on_log(command_log.clone());
    let mut last_emit = Instant::now();
    let mut pending_emit = false;
    let status = loop {
        if cancel_token.load(Ordering::SeqCst) {
            let _ = child.start_kill();
            let _ = child.wait().await;
            append_live_log(&mut command_log, "Cancelled by user.".into());
            on_log(command_log.clone());
            return Err(command_log);
        }

        if let Some(status) = child.try_wait().map_err(|error| {
            format!("Run {rendered}\nerror: failed to wait for command: {error}")
        })? {
            break status;
        }

        tokio::select! {
            line = receiver.recv() => {
                if let Some(line) = line {
                    append_live_log(&mut command_log, line);
                    pending_emit = true;
                    if last_emit.elapsed() >= TokioDuration::from_millis(50) {
                        on_log(command_log.clone());
                        last_emit = Instant::now();
                        pending_emit = false;
                    }
                }
            }
            _ = sleep(TokioDuration::from_millis(40)) => {
                if pending_emit && last_emit.elapsed() >= TokioDuration::from_millis(50) {
                    on_log(command_log.clone());
                    last_emit = Instant::now();
                    pending_emit = false;
                }
            }
        }
    };

    let _ = stdout_task.await;
    let _ = stderr_task.await;
    while let Ok(line) = receiver.try_recv() {
        append_live_log(&mut command_log, line);
    }
    on_log(command_log.clone());

    if status.success() {
        log::info!("[Workflow][Tag] Command succeeded: {}", rendered);
        Ok(command_log)
    } else {
        log::error!(
            "[Workflow][Tag] Command failed: {} | status={}",
            rendered,
            status
        );
        Err(command_log)
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
        .map_err(|error| format!("Run {}\nFailed to spawn command: {}", rendered, error))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let log = format_command_log(&rendered, &stdout, &stderr);
    if output.status.success() {
        Ok(log)
    } else {
        Err(log)
    }
}

#[cfg(test)]
mod tests {
    use super::{format_command_log, format_duration, run_command};
    use crate::jobs::{JobConclusion, JobStepData, JobStepDefinition, JobStepStatus};
    use slint::Model;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    #[test]
    fn command_log_matches_githubs_continuous_run_output() {
        assert_eq!(
            format_command_log(
                "git -C /repo checkout release",
                "already up to date\n",
                "warning: example\n",
            ),
            "Run git -C /repo checkout release\nalready up to date\nwarning: example"
        );
    }

    #[test]
    fn command_log_omits_empty_stream_headings_and_blank_sections() {
        assert_eq!(
            format_command_log(
                "git -C /repo checkout release",
                "",
                "error: pathspec did not match\n",
            ),
            "Run git -C /repo checkout release\nerror: pathspec did not match"
        );
    }

    #[test]
    fn slint_step_exposes_one_log_line_model_row_per_rendered_line() {
        let step = JobStepData {
            definition: JobStepDefinition::new("Example", vec!["git".into(), "status".into()]),
            status: JobStepStatus::Completed,
            conclusion: Some(JobConclusion::Success),
            detail: String::new(),
            log: "Run git status\nclean\ncomplete".into(),
            duration: "0s".into(),
            default_expanded: false,
            log_requested: true,
        };
        let slint_step = step.to_slint();
        assert_eq!(slint_step.log_lines.row_count(), 3);
        assert_eq!(
            slint_step.log_lines.row_data(0).unwrap().text.as_str(),
            "Run git status"
        );
        assert_eq!(
            slint_step.log_lines.row_data(1).unwrap().text.as_str(),
            "clean"
        );
        assert_eq!(
            slint_step.log_lines.row_data(2).unwrap().text.as_str(),
            "complete"
        );
    }

    #[test]
    fn step_duration_matches_githubs_compact_units() {
        assert_eq!(format_duration(Duration::from_millis(999)), "0s");
        assert_eq!(format_duration(Duration::from_secs(65)), "1m 5s");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_output_is_published_before_process_completion() {
        let token = Arc::new(AtomicBool::new(false));
        let mut snapshots = Vec::new();
        let result = run_command(
            vec![
                "sh".into(),
                "-c".into(),
                "printf 'warning: first\\n'; sleep 0.15; printf 'error: second\\n' >&2".into(),
            ],
            &token,
            |snapshot| snapshots.push(snapshot),
        )
        .await
        .expect("shell command should succeed");

        assert!(result.contains("error: second"));
        assert!(
            snapshots
                .iter()
                .take(snapshots.len().saturating_sub(1))
                .any(|snapshot| snapshot.contains("warning: first"))
        );
    }
}
