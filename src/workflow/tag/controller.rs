use crate::app_runtime::shared_runtime;
use crate::i18n::LocalizedText;
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

use super::commands::{create_tag_plan, remove_tag_plan};
use super::version::{is_valid_semver_tag, next_semver_for_mode, parse_semver_tag};

const TAG_MODE_LOCAL: &str = "local";
const TAG_MODE_REMOTE: &str = "remote";
const TAG_BUMP_PATCH: &str = "patch";

pub(super) use super::command_output::{clamp_step_log, run_command};

fn format_command_log(rendered: &str, stdout: &str, stderr: &str) -> String {
    let mut lines = vec![format!("Run {rendered}")];
    lines.extend(stdout.lines().map(str::to_owned));
    lines.extend(stderr.lines().map(str::to_owned));
    clamp_step_log(lines.join("\n"))
}

pub(super) fn set_step_log(step: &mut JobStepData, log: impl Into<String>) {
    step.set_log(clamp_step_log(log.into()));
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
    super::benchmark::setup(ui);
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
                        LocalizedText::message("tag.remote_unavailable")
                    } else {
                        LocalizedText::message_with(
                            "tag.local_ready",
                            [
                                ("current_tag", view.tag_current_version.clone()),
                                ("bump", version_mode.to_string()),
                            ],
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
                    view.tag_result = LocalizedText::message_with(
                        "tag.local_ready",
                        [
                            ("current_tag", view.tag_current_version.clone()),
                            ("bump", view.tag_version_mode.clone()),
                        ],
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
                    view.tag_result = LocalizedText::message("tag.remote_unavailable");
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
                view.tag_result = if project_id == "default" {
                    LocalizedText::message("tag.loading_add_default")
                } else {
                    LocalizedText::message_with(
                        "tag.loading_add",
                        [("project", project_name.clone())],
                    )
                };
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
                                entry.tag_result = LocalizedText::message_with(
                                    "tag.local_ready",
                                    [
                                        ("current_tag", latest_tag),
                                        ("bump", bump_mode.clone()),
                                    ],
                                );
                            }
                            Err(error) => {
                                log::warn!("[Workflow][Tag] Add tab initialization failed: {}", error);
                                entry.tag_current_version = "v0.0.0".to_string();
                                entry.tag_next_version = next_semver_for_mode("v0.0.0", &bump_mode);
                                entry.tag_remove_target = "v0.0.0".to_string();
                                entry.tag_add_initializing = false;
                                entry.tag_add_ready = false;
                                entry.tag_result =
                                    LocalizedText::message("tag.load_local_failed");
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

        let (
            project_id,
            project_name,
            local_repo,
            mode,
            remove_ready,
            remove_initializing,
            running,
        ) = state_open_remove
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
            log::debug!(
                "[Workflow][Tag] open remove tab reused cached initialization for '{}'",
                project_id
            );
            if let Ok(mut guard) = state_open_remove.lock() {
                guard.upsert_current(|view| {
                    view.tag_selected_tab = 1;
                    // Always restore the remove plan so a prior create run's
                    // steps/disclosures cannot leak into the Remove tab.
                    view.reset_tag_job(default_remove_tag_steps());
                    view.tag_result = LocalizedText::message_with(
                        "tag.remove_ready",
                        [("latest_tag", view.tag_remove_target.clone())],
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
                    view.tag_result = LocalizedText::message("tag.remote_unavailable");
                    view.tag_remove_initializing = false;
                    view.tag_remove_ready = false;
                    view.tag_remove_target.clear();
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
                view.tag_result = if project_id == "default" {
                    LocalizedText::message("tag.loading_remove_default")
                } else {
                    LocalizedText::message_with(
                        "tag.loading_remove",
                        [("project", project_name.clone())],
                    )
                };
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
                        entry.tag_result = LocalizedText::message_with(
                            "tag.remove_ready",
                            [("latest_tag", latest_tag.clone())],
                        );
                        if let Some(first) = entry.tag_job.steps.get_mut(0) {
                            first.detail = LocalizedText::message_with(
                                "tag.step.remove_target_resolved",
                                [("latest_tag", latest_tag)],
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
                        entry.tag_result = LocalizedText::message("tag.load_remote_failed");
                    }
                }
                if current_project_id != project_id {
                    return;
                }
            }
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak_task.upgrade() {
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
            show_error(&ui, crate::i18n::tr("tag.already_running").as_ref());
            return;
        }
        if add_initializing {
            log::warn!("[Workflow][Tag] Run rejected: add tab is still initializing");
            show_error(&ui, crate::i18n::tr("tag.initializing").as_ref());
            return;
        }

        if mode == TAG_MODE_REMOTE {
            log::warn!("[Workflow][Tag] Run rejected: remote mode not implemented");
            if let Ok(mut guard) = state_run.lock() {
                guard.upsert_current(|view| {
                    view.tag_result = LocalizedText::message("tag.remote_unavailable");
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
            show_error(&ui, crate::i18n::tr("tag.remote_unavailable").as_ref());
            return;
        }

        let version = version.trim().to_string();
        if !is_valid_semver_tag(&version) {
            log::warn!(
                "[Workflow][Tag] Run rejected: invalid semantic version -> '{}'",
                version
            );
            show_error(&ui, crate::i18n::tr("tag.invalid_version").as_ref());
            return;
        }
        if local_repo.trim().is_empty() {
            log::warn!("[Workflow][Tag] Run rejected: empty local repository path");
            show_error(
                &ui,
                crate::i18n::tr("workflow.local_repository_empty").as_ref(),
            );
            return;
        }
        if !Path::new(&local_repo).exists() {
            log::warn!(
                "[Workflow][Tag] Run rejected: local repository path does not exist -> '{}'",
                local_repo
            );
            show_error(
                &ui,
                crate::i18n::tr("workflow.local_repository_missing").as_ref(),
            );
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
                view.tag_result = LocalizedText::message_with(
                    "tag.running_create",
                    [
                        ("project", project_name.clone()),
                        ("mode", mode.clone()),
                    ],
                );
                view.tag_next_version = version.clone();
                view.tag_job.start(default_tag_steps());
                view.job_run = JobRunDescriptor {
                    active: true,
                    source: "tag".to_string(),
                    run_title: LocalizedText::message_with(
                        "tag.create_run_title",
                        [("version", version.clone())],
                    ),
                    workflow_name: LocalizedText::message("tag.workflow_name"),
                    context_name: project_name.clone(),
                    job_name: "create-tag".to_string(),
                    trigger_detail: LocalizedText::message("tag.started_manually"),
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
                        Ok(ref success) => {
                            let message = success.summary.resolve();
                            show_success(&ui, message.as_str());
                        }
                        Err(ref failure) => {
                            let message = failure.summary.resolve();
                            show_error(&ui, message.as_str());
                        }
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
            show_error(&ui, crate::i18n::tr("tag.already_running").as_ref());
            return;
        }
        if remove_initializing {
            log::warn!("[Workflow][Tag] Remove rejected: remove tab is still initializing");
            show_error(&ui, crate::i18n::tr("tag.remove_initializing").as_ref());
            return;
        }
        if mode == TAG_MODE_REMOTE {
            if let Ok(mut guard) = state_remove.lock() {
                guard.upsert_current(|view| {
                    view.tag_result = LocalizedText::message("tag.remote_unavailable");
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
            show_error(&ui, crate::i18n::tr("tag.remote_unavailable").as_ref());
            return;
        }

        let tag = tag.trim().to_string();
        if !is_valid_semver_tag(&tag) {
            show_error(&ui, crate::i18n::tr("tag.invalid_remove_version").as_ref());
            return;
        }
        if local_repo.trim().is_empty() {
            show_error(
                &ui,
                crate::i18n::tr("workflow.local_repository_empty").as_ref(),
            );
            return;
        }
        if !Path::new(&local_repo).exists() {
            show_error(
                &ui,
                crate::i18n::tr("workflow.local_repository_missing").as_ref(),
            );
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
                view.tag_result = LocalizedText::message_with(
                    "tag.running_remove",
                    [("project", project_name.clone()), ("mode", mode.clone())],
                );
                view.tag_remove_target = tag.clone();
                view.tag_job.start(default_remove_tag_steps());
                view.job_run = JobRunDescriptor {
                    active: true,
                    source: "tag".to_string(),
                    run_title: LocalizedText::message_with(
                        "tag.remove_run_title",
                        [("tag", tag.clone())],
                    ),
                    workflow_name: LocalizedText::message("tag.workflow_name"),
                    context_name: project_name.clone(),
                    job_name: "remove-tag".to_string(),
                    trigger_detail: LocalizedText::message("tag.started_manually"),
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
                        Ok(ref success) => {
                            let message = success.summary.resolve();
                            show_success(&ui, message.as_str());
                        }
                        Err(ref failure) => {
                            let message = failure.summary.resolve();
                            show_error(&ui, message.as_str());
                        }
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
            show_error(&ui, crate::i18n::tr("tag.no_running_workflow").as_ref());
            return;
        }

        if !request_tag_cancel() {
            log::warn!("[Workflow][Tag] stop requested but no cancel token found");
        }

        if let Ok(mut guard) = state_stop.lock() {
            guard.upsert_current(|view| {
                view.tag_running = false;
                view.tag_result = LocalizedText::message("tag.stop_pausing");
                for step in &mut view.tag_job.steps {
                    if step.status == JobStepStatus::InProgress {
                        step.status = JobStepStatus::Completed;
                        step.conclusion = Some(JobConclusion::Cancelled);
                        step.detail = LocalizedText::message("tag.cancel_waiting_for_command");
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
                        step.detail = LocalizedText::message("tag.paused_after_cancel");
                    }
                }
            });
            log::info!("[Workflow][Tag] stop state applied to UI model");
        } else {
            log::warn!("[Workflow][Tag] stop failed: cannot acquire state lock");
        }
        initialize_current_ui(&ui, &state_stop);
        show_error(&ui, crate::i18n::tr("tag.stop_requested").as_ref());
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
    let (project_id, mode, local_repo, bump_mode) = shared_state
        .lock()
        .ok()
        .map(|guard| {
            guard.with_current(|ctx, view| {
                (
                    ctx.id.clone(),
                    view.tag_mode.clone(),
                    ctx.local_repo.clone(),
                    view.tag_version_mode.clone(),
                )
            })
        })
        .unwrap_or_else(|| {
            (
                "default".to_string(),
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

    if mode == TAG_MODE_REMOTE {
        log::info!("[Workflow][Tag] Refresh context -> remote mode unavailable");
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
                view.tag_current_version.clear();
                view.tag_next_version.clear();
                view.tag_remove_target.clear();
                view.tag_result = LocalizedText::message("tag.remote_unavailable");
            });
        } else {
            log::warn!("[Workflow][Tag] Refresh context failed: cannot acquire state lock");
        }
        initialize_current_ui(ui, shared_state);
        if announce_remote_todo {
            show_error(ui, crate::i18n::tr("tag.remote_unavailable").as_ref());
        }
        log::info!("[Workflow][Tag] Refresh context finished");
        return;
    }

    // Write the loading state first and release the lock, then resolve the
    // latest local tag on a blocking worker. The request sequence rejects
    // stale completions, mirroring the Add tab initialization flow.
    let request_seq = if let Ok(mut guard) = shared_state.lock() {
        let mut request_seq = 0;
        guard.upsert_current(|view| {
            view.tag_running = false;
            view.tag_add_initializing = true;
            view.tag_remove_initializing = false;
            view.tag_add_ready = false;
            view.tag_remove_ready = false;
            view.tag_add_request_seq = view.tag_add_request_seq.saturating_add(1);
            request_seq = view.tag_add_request_seq;
            view.tag_remove_request_seq = view.tag_remove_request_seq.saturating_add(1);
            view.reset_tag_job(default_tag_steps());
            view.tag_result = LocalizedText::message("tag.loading_context");
        });
        request_seq
    } else {
        log::warn!("[Workflow][Tag] Refresh context failed: cannot acquire state lock");
        0
    };
    initialize_current_ui(ui, shared_state);

    let ui_weak_task = ui.as_weak();
    let state_task = shared_state.clone();
    shared_runtime().spawn(async move {
        let resolved = tokio::task::spawn_blocking({
            let local_repo = local_repo.clone();
            move || read_latest_local_tag(&local_repo)
        })
        .await
        .map_err(|error| format!("Failed to join latest local tag task: {}", error))
        .and_then(|result| result);

        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = ui_weak_task.upgrade() else {
                return;
            };
            let mut maybe_error = None;
            if let Ok(mut guard) = state_task.lock() {
                let current_project_id = guard.current_project.id.clone();
                let entry = guard.projects.entry(project_id.clone()).or_default();
                if entry.tag_add_request_seq != request_seq {
                    return;
                }
                match resolved {
                    Ok(current_tag) => {
                        log::info!(
                            "[Workflow][Tag] Refresh context resolved current tag '{}'",
                            current_tag
                        );
                        entry.tag_current_version = current_tag.clone();
                        entry.tag_next_version = next_semver_for_mode(&current_tag, &bump_mode);
                        entry.tag_remove_target = current_tag.clone();
                        entry.tag_add_initializing = false;
                        entry.tag_add_ready = true;
                        entry.tag_result = if local_repo.trim().is_empty() {
                            LocalizedText::message("tag.select_project")
                        } else {
                            LocalizedText::message_with(
                                "tag.local_ready",
                                [("current_tag", current_tag), ("bump", bump_mode.clone())],
                            )
                        };
                    }
                    Err(error) => {
                        log::warn!(
                            "[Workflow][Tag] Refresh context failed to read tags: {}",
                            error
                        );
                        maybe_error = Some(error.clone());
                        entry.tag_current_version = "v0.0.0".to_string();
                        entry.tag_next_version = next_semver_for_mode("v0.0.0", &bump_mode);
                        entry.tag_remove_target = "v0.0.0".to_string();
                        entry.tag_add_initializing = false;
                        entry.tag_add_ready = false;
                        entry.tag_result = LocalizedText::message("tag.load_local_failed");
                    }
                }
                if current_project_id != project_id {
                    return;
                }
            } else {
                log::warn!(
                    "[Workflow][Tag] Refresh context completion failed: cannot acquire state lock"
                );
            }
            initialize_current_ui(&ui, &state_task);
            if maybe_error.is_some() {
                show_error(&ui, crate::i18n::tr("tag.load_local_failed").as_ref());
            }
            log::info!("[Workflow][Tag] Refresh context finished");
        });
    });
}

struct TagRunSuccess {
    summary: LocalizedText,
    current_version: String,
    steps: Vec<JobStepData>,
}

struct TagRunFailure {
    summary: LocalizedText,
    steps: Vec<JobStepData>,
}

type TagProgressReporter = Arc<dyn Fn(Vec<JobStepData>, LocalizedText) + Send + Sync>;

fn default_remove_tag_steps() -> Vec<JobStepData> {
    vec![
        JobStepData::queued(
            JobStepDefinition::new(
                LocalizedText::message("tag.step.enter_repository"),
                vec!["git".into(), "rev-parse".into(), "--show-toplevel".into()],
            ),
            LocalizedText::message("tag.step.enter_repository_detail"),
        ),
        JobStepData::queued(
            JobStepDefinition::new(
                LocalizedText::message("tag.step.delete_local"),
                vec!["git".into(), "tag".into(), "-d".into(), "<tag>".into()],
            ),
            LocalizedText::message("tag.step.delete_local_detail"),
        ),
        JobStepData::queued(
            JobStepDefinition::new(
                LocalizedText::message("tag.step.delete_remote"),
                vec![
                    "git".into(),
                    "push".into(),
                    "--delete".into(),
                    "origin".into(),
                    "<tag>".into(),
                ],
            ),
            LocalizedText::message("tag.step.delete_remote_detail"),
        ),
        JobStepData::queued(
            JobStepDefinition::new(
                LocalizedText::message("tag.step.refresh_remote"),
                vec!["git".into(), "fetch".into(), "--tags".into()],
            ),
            LocalizedText::message("tag.step.refresh_remote_detail"),
        ),
        JobStepData::queued(
            JobStepDefinition::new(
                LocalizedText::message("tag.step.complete"),
                vec!["complete".into()],
            ),
            LocalizedText::message("tag.step.remove_complete_waiting"),
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
                JobStepDefinition::new(planned.name.clone(), planned.argv.clone()),
                planned.detail.clone(),
            )
        })
        .collect();
    steps.push(JobStepData::queued(
        JobStepDefinition::new(
            LocalizedText::message("tag.step.complete"),
            vec!["complete".into()],
        ),
        LocalizedText::message("tag.step.complete_detail"),
    ));

    for (index, planned) in commands.iter().enumerate() {
        let command = &planned.argv;
        let detail = &planned.detail;
        if cancel_token.load(Ordering::SeqCst) {
            steps[index].status = JobStepStatus::Completed;
            steps[index].conclusion = Some(JobConclusion::Cancelled);
            steps[index].detail = LocalizedText::message("tag.cancelled_before_step_detail");
            set_step_log(&mut steps[index], "[Stop] Cancel requested by user.");
            return Err(TagRunFailure {
                summary: LocalizedText::message_with(
                    "tag.cancelled_before_step",
                    [("step", (index + 1).to_string())],
                ),
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
                LocalizedText::message_with(
                    "tag.progress_running",
                    [
                        ("current", (index + 1).to_string()),
                        ("total", commands.len().to_string()),
                    ],
                ),
            );
        }

        let running_summary = LocalizedText::message_with(
            "tag.progress_running",
            [
                ("current", (index + 1).to_string()),
                ("total", commands.len().to_string()),
            ],
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
                        LocalizedText::message_with(
                            "tag.progress_completed",
                            [
                                ("current", (index + 1).to_string()),
                                ("total", commands.len().to_string()),
                            ],
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
                    steps[index].detail = LocalizedText::message("tag.cancelled_paused_detail");
                    for remaining in steps.iter_mut().skip(index + 1) {
                        if remaining.status == JobStepStatus::Queued {
                            remaining.status = JobStepStatus::Completed;
                            remaining.conclusion = Some(JobConclusion::Cancelled);
                            remaining.detail = LocalizedText::message("tag.paused_after_cancel");
                        }
                    }
                    if let Some(progress) = on_progress.as_ref() {
                        progress(
                            steps.clone(),
                            LocalizedText::message_with(
                                "tag.stop_paused_at",
                                [("step", (index + 1).to_string())],
                            ),
                        );
                    }
                    log::warn!(
                        "[Workflow][Tag] Step {} cancelled by user: '{}'",
                        index + 1,
                        steps[index].definition.name
                    );
                    return Err(TagRunFailure {
                        summary: LocalizedText::message_with(
                            "tag.cancelled_at_step",
                            [("step", (index + 1).to_string())],
                        ),
                        steps,
                    });
                }
                steps[index].status = JobStepStatus::Completed;
                steps[index].conclusion = Some(JobConclusion::Failure);
                steps[index].default_expanded = true;
                set_step_log(&mut steps[index], log.clone());
                steps[index].detail = LocalizedText::message("tag.command_failed_detail");
                if let Some(progress) = on_progress.as_ref() {
                    progress(
                        steps.clone(),
                        LocalizedText::message_with(
                            "tag.progress_failed",
                            [
                                ("current", (index + 1).to_string()),
                                ("total", commands.len().to_string()),
                            ],
                        ),
                    );
                }
                log::error!(
                    "[Workflow][Tag] Step {} failed: '{}'",
                    index + 1,
                    steps[index].definition.name
                );
                return Err(TagRunFailure {
                    summary: LocalizedText::message_with(
                        "tag.failed_at_step",
                        [("step", (index + 1).to_string())],
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
    steps[complete_index].detail = LocalizedText::message("tag.create_finished_detail");
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
            LocalizedText::message_with("tag.create_finished", [("tag", version.to_string())]),
        );
    }
    log::info!(
        "[Workflow][Tag] Local flow completed successfully: version='{}'",
        version
    );

    Ok(TagRunSuccess {
        summary: LocalizedText::message_with(
            "tag.create_completed",
            [("tag", version.to_string())],
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
            steps[index].detail = LocalizedText::message("tag.cancelled_before_step_detail");
            set_step_log(&mut steps[index], "[Stop] Cancel requested by user.");
            return Err(TagRunFailure {
                summary: LocalizedText::message_with(
                    "tag.cancelled_before_step",
                    [("step", (index + 1).to_string())],
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
                LocalizedText::message_with(
                    "tag.progress_running",
                    [
                        ("current", (index + 1).to_string()),
                        ("total", commands.len().to_string()),
                    ],
                ),
            );
        }

        let running_summary = LocalizedText::message_with(
            "tag.progress_running",
            [
                ("current", (index + 1).to_string()),
                ("total", commands.len().to_string()),
            ],
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
                        LocalizedText::message_with(
                            "tag.progress_completed",
                            [
                                ("current", (index + 1).to_string()),
                                ("total", commands.len().to_string()),
                            ],
                        ),
                    );
                }
            }
            Err(log) => {
                if cancel_token.load(Ordering::SeqCst) {
                    steps[index].status = JobStepStatus::Completed;
                    steps[index].conclusion = Some(JobConclusion::Cancelled);
                    set_step_log(&mut steps[index], log.clone());
                    steps[index].detail = LocalizedText::message("tag.cancelled_paused_detail");
                    for remaining in steps.iter_mut().skip(index + 1) {
                        if remaining.status == JobStepStatus::Queued {
                            remaining.status = JobStepStatus::Completed;
                            remaining.conclusion = Some(JobConclusion::Cancelled);
                            remaining.detail = LocalizedText::message("tag.paused_after_cancel");
                        }
                    }
                    if let Some(progress) = on_progress.as_ref() {
                        progress(
                            steps.clone(),
                            LocalizedText::message_with(
                                "tag.stop_paused_at",
                                [("step", (index + 1).to_string())],
                            ),
                        );
                    }
                    return Err(TagRunFailure {
                        summary: LocalizedText::message_with(
                            "tag.cancelled_at_step",
                            [("step", (index + 1).to_string())],
                        ),
                        steps,
                    });
                }
                steps[index].status = JobStepStatus::Completed;
                steps[index].conclusion = Some(JobConclusion::Failure);
                steps[index].default_expanded = true;
                set_step_log(&mut steps[index], log);
                steps[index].detail = LocalizedText::message("tag.command_failed_detail");
                if let Some(progress) = on_progress.as_ref() {
                    progress(
                        steps.clone(),
                        LocalizedText::message_with(
                            "tag.progress_failed",
                            [
                                ("current", (index + 1).to_string()),
                                ("total", commands.len().to_string()),
                            ],
                        ),
                    );
                }
                return Err(TagRunFailure {
                    summary: LocalizedText::message_with(
                        "tag.failed_at_step",
                        [("step", (index + 1).to_string())],
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
        steps[refresh_index].detail = LocalizedText::message("tag.cancelled_before_refresh_detail");
        set_step_log(
            &mut steps[refresh_index],
            "[Stop] Cancel requested by user.",
        );
        return Err(TagRunFailure {
            summary: LocalizedText::message("tag.cancelled_before_refresh"),
            steps,
        });
    }
    let refresh_started = Instant::now();
    steps[refresh_index].status = JobStepStatus::InProgress;
    steps[refresh_index].conclusion = None;
    steps[refresh_index].duration = "…".into();
    steps[refresh_index].detail = LocalizedText::message("tag.step.refresh_remote_detail");
    if let Some(progress) = on_progress.as_ref() {
        progress(
            steps.clone(),
            LocalizedText::message_with(
                "tag.progress_running",
                [("current", "4".to_string()), ("total", "4".to_string())],
            ),
        );
    }
    let refresh_summary = LocalizedText::message_with(
        "tag.progress_running",
        [("current", "4".to_string()), ("total", "4".to_string())],
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
                LocalizedText::message("tag.cancelled_paused_detail")
            } else {
                LocalizedText::message("tag.refresh_remote_failed_detail")
            };
            set_step_log(&mut steps[refresh_index], error);
            return Err(TagRunFailure {
                summary: if cancel_token.load(Ordering::SeqCst) {
                    LocalizedText::message("tag.cancelled_during_refresh")
                } else {
                    LocalizedText::message_with(
                        "tag.failed_at_step",
                        [("step", (refresh_index + 1).to_string())],
                    )
                },
                steps,
            });
        }
    };
    steps[refresh_index].status = JobStepStatus::Completed;
    steps[refresh_index].conclusion = Some(JobConclusion::Success);
    steps[refresh_index].duration = format_duration(refresh_started.elapsed());
    steps[refresh_index].detail = LocalizedText::message_with(
        "tag.step.latest_remote_tag",
        [("latest_tag", latest_tag.clone())],
    );
    set_step_log(&mut steps[refresh_index], refresh_log);
    if let Some(progress) = on_progress.as_ref() {
        progress(
            steps.clone(),
            LocalizedText::message_with(
                "tag.progress_completed",
                [("current", "4".to_string()), ("total", "4".to_string())],
            ),
        );
    }

    let complete_index = 4;
    steps[complete_index].status = JobStepStatus::Completed;
    steps[complete_index].conclusion = Some(JobConclusion::Success);
    steps[complete_index].duration = "0s".into();
    steps[complete_index].detail = LocalizedText::message_with(
        "tag.remove_finished_detail",
        [("latest_tag", latest_tag.clone())],
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
            LocalizedText::message_with(
                "tag.remove_finished",
                [
                    ("removed_tag", tag.to_string()),
                    ("latest_tag", latest_tag.clone()),
                ],
            ),
        );
    }

    Ok(TagRunSuccess {
        summary: LocalizedText::message_with(
            "tag.remove_completed",
            [
                ("removed_tag", tag.to_string()),
                ("latest_tag", latest_tag.clone()),
            ],
        ),
        current_version: latest_tag,
        steps,
    })
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
    // Run `ls-remote` once and reuse its stdout for parsing instead of
    // spawning the same network command a second time.
    let (_, stdout) = run_command_blocking(list_cmd)?;

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

/// Runs a command to completion, returning `(formatted_log, stdout)` so
/// callers can parse the output without re-executing the command.
fn run_command_blocking(command: Vec<String>) -> Result<(String, String), String> {
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
        Ok((log, stdout.into_owned()))
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
        let mut step = JobStepData {
            definition: JobStepDefinition::new("Example", vec!["git".into(), "status".into()]),
            status: JobStepStatus::Completed,
            conclusion: Some(JobConclusion::Success),
            detail: String::new().into(),
            log: String::new(),
            parsed_log: Default::default(),
            duration: "0s".into(),
            default_expanded: false,
            log_requested: true,
        };
        step.set_log("Run git status\nclean\ncomplete".into());
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
