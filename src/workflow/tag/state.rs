//! Pure Tag configuration, initialization and run lifecycle reducer.
use super::commands::{
    create_tag_plan, create_tag_steps, default_remove_tag_steps, remove_tag_plan, remove_tag_steps,
};
use super::execution::{TagRunFailure, TagRunSuccess};
use super::version::{VersionBump, is_valid_semver_tag, next_semver};
use crate::i18n::LocalizedText;
use crate::jobs::{JobConclusion, JobRunDescriptor, JobStepData, JobStepStatus};
use crate::workflow::state::{
    WorkflowAction, WorkflowProjectContext, WorkflowStore, WorkflowViewState,
};
use std::time::Duration;

pub(crate) enum TagAction {
    Mode(String),
    Bump(VersionBump),
    Draft {
        next: String,
        target: String,
        description: String,
    },
    Enter,
    Open {
        remove: bool,
        force: bool,
    },
    Initialized {
        remove: bool,
        sequence: u64,
        result: Result<String, LocalizedText>,
    },
    Run {
        remove: bool,
        version: String,
        description: String,
        validation: Option<LocalizedText>,
    },
    Progress {
        run: u64,
        steps: Vec<JobStepData>,
        summary: LocalizedText,
    },
    Finished {
        run: u64,
        result: Result<TagRunSuccess, TagRunFailure>,
        duration: Duration,
        cancelled: bool,
    },
    Stop,
    LoadLog(usize),
}
pub(crate) enum TagEffect {
    Initialize {
        remove: bool,
        sequence: u64,
        repo: String,
    },
    Run {
        remove: bool,
        run: u64,
        repo: String,
        version: String,
        description: String,
    },
    CancelInit,
    CancelAll,
    Stop,
    Toast(bool, LocalizedText),
}
fn ready(view: &WorkflowViewState) -> LocalizedText {
    LocalizedText::message_with(
        "tag.local_ready",
        [
            ("current_tag", view.tag_current_version.clone()),
            ("bump", view.tag_version_mode.as_ui_str().into()),
        ],
    )
}
fn invalidate(view: &mut WorkflowViewState) {
    view.tag_add_request_seq = view.tag_add_request_seq.wrapping_add(1);
    view.tag_remove_request_seq = view.tag_remove_request_seq.wrapping_add(1);
    view.tag_add_initializing = false;
    view.tag_remove_initializing = false;
}
fn reset_plan(view: &mut WorkflowViewState, remove: bool) {
    view.tag_run_generation = view.tag_run_generation.wrapping_add(1);
    view.reset_tag_job(steps(remove));
}
fn steps(remove: bool) -> Vec<JobStepData> {
    if remove {
        default_remove_tag_steps()
    } else {
        super::commands::default_create_tag_steps()
    }
}
pub(crate) fn prepare_action(state: &WorkflowStore, mut action: WorkflowAction) -> WorkflowAction {
    if let WorkflowAction::Tag(TagAction::Run { validation, .. }) = &mut action {
        let repo = &state.current_project.local_repo;
        *validation = if repo.trim().is_empty() {
            Some(LocalizedText::message("workflow.local_repository_empty"))
        } else if !std::path::Path::new(repo).exists() {
            Some(LocalizedText::message("workflow.local_repository_missing"))
        } else {
            None
        };
    }
    action
}
pub(crate) fn reduce(
    view: &mut WorkflowViewState,
    context: &WorkflowProjectContext,
    action: TagAction,
) -> Vec<TagEffect> {
    let mut effects = Vec::new();
    match action {
        TagAction::Draft {
            next,
            target,
            description,
        } => {
            view.tag_next_version = next;
            view.tag_remove_target = target;
            view.tag_description = description;
        }
        TagAction::LoadLog(index) => {
            view.tag_job.request_step_log(index);
        }
        TagAction::Mode(mode) => {
            if view.tag_running {
                return effects;
            }
            view.tag_mode = mode;
            invalidate(view);
            view.tag_add_ready = false;
            view.tag_remove_ready = false;
            reset_plan(view, false);
            effects.push(TagEffect::CancelInit);
            effects.extend(reduce(
                view,
                context,
                TagAction::Open {
                    remove: false,
                    force: true,
                },
            ));
        }
        TagAction::Bump(bump) => {
            if view.tag_running {
                return effects;
            }
            view.tag_version_mode = bump;
            reset_plan(view, view.tag_selected_tab == 1);
            if view.tag_current_version.is_empty() {
                return effects;
            }
            view.tag_next_version = next_semver(&view.tag_current_version, bump);
            view.tag_result = if view.tag_mode == "remote" {
                LocalizedText::message("tag.remote_unavailable")
            } else {
                ready(view)
            };
        }
        TagAction::Enter => {
            let remove = view.tag_selected_tab == 1;
            let initializing = if remove {
                view.tag_remove_initializing
            } else {
                view.tag_add_initializing
            };
            if view.tag_running || initializing {
                return effects;
            }
            effects.extend(reduce(
                view,
                context,
                TagAction::Open {
                    remove,
                    force: true,
                },
            ));
        }
        TagAction::Open { remove, force } => {
            view.tag_selected_tab = i32::from(remove);
            if view.tag_running {
                return effects;
            }
            let cached = if remove {
                view.tag_remove_ready && !view.tag_remove_initializing
            } else {
                view.tag_add_ready && !view.tag_add_initializing
            };
            if cached && !force && view.tag_mode == "local" {
                reset_plan(view, remove);
                view.tag_result = ready(view);
                return effects;
            }
            invalidate(view);
            effects.push(TagEffect::CancelInit);
            reset_plan(view, remove);
            if view.tag_mode == "remote" {
                view.tag_result = LocalizedText::message("tag.remote_unavailable");
                return effects;
            }
            view.tag_add_ready = false;
            view.tag_remove_ready = false;
            view.tag_add_initializing = !remove;
            view.tag_remove_initializing = remove;
            view.tag_result = LocalizedText::message("tag.loading_remote_context");
            effects.push(TagEffect::Initialize {
                remove,
                sequence: if remove {
                    view.tag_remove_request_seq
                } else {
                    view.tag_add_request_seq
                },
                repo: context.local_repo.clone(),
            });
        }
        TagAction::Initialized {
            remove,
            sequence,
            result,
        } => {
            if sequence
                != if remove {
                    view.tag_remove_request_seq
                } else {
                    view.tag_add_request_seq
                }
            {
                return effects;
            }
            view.tag_add_initializing = false;
            view.tag_remove_initializing = false;
            let succeeded = result.is_ok();
            match result {
                Ok(version) => {
                    view.tag_current_version = version.clone();
                    view.tag_next_version = next_semver(&version, view.tag_version_mode);
                    view.tag_remove_target = version;
                    view.tag_result = if context.local_repo.trim().is_empty() {
                        LocalizedText::message("tag.select_project")
                    } else {
                        ready(view)
                    };
                }
                Err(error) => {
                    view.tag_current_version.clear();
                    view.tag_next_version.clear();
                    view.tag_remove_target.clear();
                    view.tag_result = error.clone();
                    effects.push(TagEffect::Toast(false, error));
                }
            }
            if remove {
                view.tag_remove_ready = succeeded;
            } else {
                view.tag_add_ready = succeeded;
            }
        }
        TagAction::Run {
            remove,
            version,
            description,
            validation,
        } => {
            let error = if view.tag_running {
                Some("tag.already_running")
            } else if view.tag_add_initializing || view.tag_remove_initializing {
                Some("tag.initializing")
            } else if view.tag_mode == "remote" {
                Some("tag.remote_unavailable")
            } else if !remove && !view.tag_add_ready {
                Some("tag.remote_tag_not_ready")
            } else if !is_valid_semver_tag(version.trim()) {
                Some(if remove {
                    "tag.invalid_remove_version"
                } else {
                    "tag.invalid_version"
                })
            } else {
                None
            };
            if let Some(error) = error {
                effects.push(TagEffect::Toast(false, LocalizedText::message(error)));
                return effects;
            }
            if let Some(error) = validation {
                effects.push(TagEffect::Toast(false, error));
                return effects;
            }
            invalidate(view);
            view.tag_running = true;
            view.tag_add_ready = false;
            view.tag_remove_ready = false;
            view.tag_run_generation = view.tag_run_generation.wrapping_add(1);
            let version = version.trim().to_string();
            if remove {
                view.tag_remove_target = version.clone();
            } else {
                view.tag_next_version = version.clone();
                view.tag_description = description.clone();
            }
            let queued = if remove {
                remove_tag_steps(&remove_tag_plan(&context.local_repo, &version))
            } else {
                create_tag_steps(&create_tag_plan(
                    &context.local_repo,
                    &version,
                    &description,
                ))
            };
            view.tag_job.start(queued);
            view.tag_result = LocalizedText::message_with(
                if remove {
                    "tag.running_remove"
                } else {
                    "tag.running_create"
                },
                [
                    ("project", context.name.clone()),
                    ("mode", view.tag_mode.clone()),
                ],
            );
            view.job_run = JobRunDescriptor {
                active: true,
                source: "tag".into(),
                run_title: LocalizedText::message_with(
                    if remove {
                        "tag.remove_run_title"
                    } else {
                        "tag.create_run_title"
                    },
                    [(if remove { "tag" } else { "version" }, version.clone())],
                ),
                workflow_name: LocalizedText::message("tag.workflow_name"),
                context_name: context.name.clone(),
                job_name: if remove { "remove-tag" } else { "create-tag" }.into(),
                trigger_detail: LocalizedText::message("tag.started_manually"),
            };
            effects.push(TagEffect::CancelInit);
            effects.push(TagEffect::Run {
                remove,
                run: view.tag_run_generation,
                repo: context.local_repo.clone(),
                version,
                description,
            });
        }
        TagAction::Progress {
            run,
            steps,
            summary,
        } => {
            if run != view.tag_run_generation || !view.tag_running {
                return effects;
            }
            view.tag_job.replace_steps(steps);
            view.tag_result = summary;
        }
        TagAction::Finished {
            run,
            result,
            duration,
            cancelled,
        } => {
            if run != view.tag_run_generation {
                return effects;
            }
            view.tag_running = false;
            invalidate(view);
            let conclusion = if cancelled {
                JobConclusion::Cancelled
            } else if result.is_ok() {
                JobConclusion::Success
            } else {
                JobConclusion::Failure
            };
            let (summary, steps, success) = match result {
                Ok(result) => {
                    view.tag_current_version = result.current_version.clone();
                    view.tag_remove_target = result.current_version.clone();
                    view.tag_next_version =
                        next_semver(&result.current_version, view.tag_version_mode);
                    view.tag_add_ready = !cancelled && view.job_run.job_name == "create-tag";
                    view.tag_remove_ready = !cancelled && view.job_run.job_name == "remove-tag";
                    (result.summary, result.steps, true)
                }
                Err(result) => (result.summary, result.steps, false),
            };
            view.tag_job.finish(conclusion, duration, steps);
            view.tag_result = summary.clone();
            effects.push(TagEffect::Toast(success, summary));
        }
        TagAction::Stop => {
            if !view.tag_running {
                effects.push(TagEffect::Toast(
                    false,
                    LocalizedText::message("tag.no_running_workflow"),
                ));
                return effects;
            }
            view.tag_running = false;
            view.tag_result = LocalizedText::message("tag.stop_pausing");
            for step in &mut view.tag_job.steps {
                if step.status != JobStepStatus::Completed {
                    step.finish(
                        JobConclusion::Cancelled,
                        None,
                        LocalizedText::message("tag.paused_after_cancel"),
                        false,
                    );
                }
            }
            effects.push(TagEffect::Stop);
            effects.push(TagEffect::Toast(
                false,
                LocalizedText::message("tag.stop_requested"),
            ));
        }
    }
    effects
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
