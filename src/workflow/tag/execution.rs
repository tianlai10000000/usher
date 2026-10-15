//! Local Git plan execution, bounded logs and measured job steps.
use super::command_output::{CommandFailure, CommandFailureKind, run_command_with_stdout};
pub(super) use super::command_output::{clamp_step_log, run_command};
use super::commands::{create_tag_plan, create_tag_steps, remove_tag_plan, remove_tag_steps};
use super::version::RemoteTagParser;
use crate::i18n::LocalizedText;
use crate::jobs::{JobConclusion, JobStepData, JobStepStatus};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
pub(super) fn set_step_log(step: &mut JobStepData, log: impl Into<String>) {
    step.set_log(clamp_step_log(log.into()));
}
pub(crate) struct TagRunSuccess {
    pub(super) summary: LocalizedText,
    pub(super) current_version: String,
    pub(super) steps: Vec<JobStepData>,
}

pub(crate) struct TagRunFailure {
    pub(super) summary: LocalizedText,
    pub(super) steps: Vec<JobStepData>,
}

pub(super) type TagProgressReporter = Arc<dyn Fn(Vec<JobStepData>, LocalizedText) + Send + Sync>;

pub(super) async fn run_local_tag_flow(
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
    let mut steps = create_tag_steps(&commands);

    for (index, planned) in commands.iter().enumerate() {
        let command = &planned.argv;
        let detail = &planned.detail;
        if cancel_token.load(Ordering::SeqCst) {
            steps[index].finish(
                JobConclusion::Cancelled,
                None,
                LocalizedText::message("tag.cancelled_before_step_detail"),
                false,
            );
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
        steps[index].start(detail.clone());
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
        let elapsed = step_started.elapsed();
        match result {
            Ok(success) => {
                steps[index].finish(JobConclusion::Success, Some(elapsed), detail.clone(), false);
                set_step_log(&mut steps[index], success.log);
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
            Err(failure) => {
                if failure.is_cancelled() {
                    steps[index].finish(
                        JobConclusion::Cancelled,
                        Some(elapsed),
                        LocalizedText::message("tag.cancelled_paused_detail"),
                        false,
                    );
                    set_step_log(&mut steps[index], failure.log.clone());
                    for remaining in steps.iter_mut().skip(index + 1) {
                        if remaining.status == JobStepStatus::Queued {
                            remaining.finish(
                                JobConclusion::Cancelled,
                                None,
                                LocalizedText::message("tag.paused_after_cancel"),
                                false,
                            );
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
                steps[index].finish(
                    JobConclusion::Failure,
                    Some(elapsed),
                    LocalizedText::message("tag.command_failed_detail"),
                    true,
                );
                set_step_log(&mut steps[index], failure.log);
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

    let complete_index = commands.len();
    steps[complete_index].finish(
        JobConclusion::Success,
        Some(Duration::ZERO),
        LocalizedText::message("tag.create_finished_detail"),
        false,
    );
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

pub(super) async fn run_local_remove_tag_flow(
    repo: &str,
    tag: &str,
    on_progress: Option<TagProgressReporter>,
    cancel_token: Arc<AtomicBool>,
) -> Result<TagRunSuccess, TagRunFailure> {
    let commands = remove_tag_plan(repo, tag);
    let mut steps = remove_tag_steps(&commands);

    for (index, planned) in commands.iter().enumerate() {
        let command = &planned.argv;
        if cancel_token.load(Ordering::SeqCst) {
            steps[index].finish(
                JobConclusion::Cancelled,
                None,
                LocalizedText::message("tag.cancelled_before_step_detail"),
                false,
            );
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
        steps[index].start(planned.detail.clone());
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
        let elapsed = step_started.elapsed();
        match result {
            Ok(success) => {
                steps[index].finish(
                    JobConclusion::Success,
                    Some(elapsed),
                    planned.detail.clone(),
                    false,
                );
                set_step_log(&mut steps[index], success.log);
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
            Err(failure) => {
                if failure.is_cancelled() {
                    steps[index].finish(
                        JobConclusion::Cancelled,
                        Some(elapsed),
                        LocalizedText::message("tag.cancelled_paused_detail"),
                        false,
                    );
                    set_step_log(&mut steps[index], failure.log.clone());
                    for remaining in steps.iter_mut().skip(index + 1) {
                        if remaining.status == JobStepStatus::Queued {
                            remaining.finish(
                                JobConclusion::Cancelled,
                                None,
                                LocalizedText::message("tag.paused_after_cancel"),
                                false,
                            );
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
                steps[index].finish(
                    JobConclusion::Failure,
                    Some(elapsed),
                    LocalizedText::message("tag.command_failed_detail"),
                    true,
                );
                set_step_log(&mut steps[index], failure.log);
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

    let refresh_index = commands.len();
    let refresh_step = (refresh_index + 1).to_string();
    if cancel_token.load(Ordering::SeqCst) {
        steps[refresh_index].finish(
            JobConclusion::Cancelled,
            None,
            LocalizedText::message("tag.cancelled_before_refresh_detail"),
            false,
        );
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
    steps[refresh_index].start(LocalizedText::message("tag.step.refresh_remote_detail"));
    if let Some(progress) = on_progress.as_ref() {
        progress(
            steps.clone(),
            LocalizedText::message_with(
                "tag.progress_running",
                [
                    ("current", refresh_step.clone()),
                    ("total", refresh_step.clone()),
                ],
            ),
        );
    }
    let refresh_summary = LocalizedText::message_with(
        "tag.progress_running",
        [
            ("current", refresh_step.clone()),
            ("total", refresh_step.clone()),
        ],
    );
    let refresh_result = inspect_latest_remote_tag_live(repo, &cancel_token, |live_log| {
        set_step_log(&mut steps[refresh_index], live_log);
        if let Some(progress) = on_progress.as_ref() {
            progress(steps.clone(), refresh_summary.clone());
        }
    })
    .await;
    let refresh_duration = refresh_started.elapsed();
    let (latest_tag, refresh_log) = match refresh_result {
        Ok(result) => result,
        Err(error) => {
            let cancelled = error.is_cancelled();
            let conclusion = if cancelled {
                JobConclusion::Cancelled
            } else {
                JobConclusion::Failure
            };
            let detail = if cancelled {
                LocalizedText::message("tag.cancelled_paused_detail")
            } else {
                LocalizedText::message("tag.refresh_remote_failed_detail")
            };
            steps[refresh_index].finish(conclusion, Some(refresh_duration), detail, true);
            set_step_log(&mut steps[refresh_index], error.log);
            return Err(TagRunFailure {
                summary: if cancelled {
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
    steps[refresh_index].finish(
        JobConclusion::Success,
        Some(refresh_duration),
        LocalizedText::message_with(
            "tag.step.latest_remote_tag",
            [("latest_tag", latest_tag.clone())],
        ),
        false,
    );
    set_step_log(&mut steps[refresh_index], refresh_log);
    if let Some(progress) = on_progress.as_ref() {
        progress(
            steps.clone(),
            LocalizedText::message_with(
                "tag.progress_completed",
                [("current", refresh_step.clone()), ("total", refresh_step)],
            ),
        );
    }

    let complete_index = refresh_index + 1;
    steps[complete_index].finish(
        JobConclusion::Success,
        Some(Duration::ZERO),
        LocalizedText::message_with(
            "tag.remove_finished_detail",
            [("latest_tag", latest_tag.clone())],
        ),
        false,
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
) -> Result<(String, String), CommandFailure>
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
    let fetch = run_command(fetch_command, cancel_token, &mut on_log).await?;

    let list_command = vec![
        "git".to_string(),
        "-C".to_string(),
        local_repo.to_string(),
        "ls-remote".to_string(),
        "--tags".to_string(),
        "--refs".to_string(),
        "origin".to_string(),
    ];
    let mut parser = RemoteTagParser::default();
    let list = run_command_with_stdout(
        list_command,
        cancel_token,
        |log| on_log(clamp_step_log(format!("{}\n{log}", fetch.log))),
        |stdout| parser.push(stdout).map_err(|error| error.to_string()),
    )
    .await?;
    let combined_log = clamp_step_log(format!("{}\n{}", fetch.log, list.log));
    let latest = parser.finish().map_err(|error| CommandFailure {
        kind: CommandFailureKind::OutputConsumer,
        log: clamp_step_log(format!("{combined_log}\nerror: {error}")),
        spawn_kind: None,
        exit_status: None,
    })?;
    Ok((latest, combined_log))
}
