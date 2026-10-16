//! Tag UI intent capture and cancellable side-effect execution.
pub(super) use super::execution::{run_command, set_step_log};
#[cfg(test)]
use super::initialization_error::{
    LOCAL_TAG_ERROR_DETAIL_CHARS, LocalTagReadError, bounded_diagnostic_detail, format_command_log,
};
use super::state::{TagAction, TagEffect};
use super::version::VersionBump;
use crate::workflow::state::{SharedWorkflowState, WorkflowAction, WorkflowIdentity};
use crate::{MainWindow, Route};
use slint::ComponentHandle;
use std::sync::atomic::Ordering;
use std::time::Instant;

pub(crate) fn bind_tag(ui: &MainWindow, state: SharedWorkflowState) {
    super::benchmark::setup(ui);
    let stop = ui.as_weak();
    let log = ui.as_weak();
    crate::jobs::register_job_run_source(
        "tag",
        move || {
            if let Some(ui) = stop.upgrade() {
                ui.invoke_workflow_tag_stop();
            }
        },
        move |index| {
            if let Some(ui) = log.upgrade() {
                ui.invoke_workflow_tag_load_step_log(index);
            }
        },
    );
    macro_rules! bind {
        ($callback:ident,||$action:expr)=>{{let weak=ui.as_weak();let state=state.clone();ui.$callback(move||{
            if let Some(ui)=weak.upgrade(){submit(&ui,&state,$action);}
        });}};
        ($callback:ident,|$($arg:ident),*|$action:expr)=>{{let weak=ui.as_weak();let state=state.clone();ui.$callback(move|$($arg),*|{
            if let Some(ui)=weak.upgrade(){submit(&ui,&state,$action);}
        });}};
    }
    bind!(on_workflow_tag_enter, || TagAction::Enter);
    bind!(on_workflow_tag_change_mode, |mode| TagAction::Mode(
        mode.to_string()
    ));
    bind!(on_workflow_tag_select_version_mode, |mode| TagAction::Bump(
        VersionBump::from_ui(mode.as_str())
    ));
    bind!(on_workflow_tag_draft_edited, |next, target, description| {
        TagAction::Draft {
            next: next.to_string(),
            target: target.to_string(),
            description: description.to_string(),
        }
    });
    bind!(on_workflow_tag_open_add, || TagAction::Open {
        remove: false,
        force: false
    });
    bind!(on_workflow_tag_open_remove, || TagAction::Open {
        remove: true,
        force: false
    });
    bind!(on_workflow_tag_run, |version, description| TagAction::Run {
        remove: false,
        version: version.to_string(),
        description: description.to_string(),
        validation: None
    });
    bind!(on_workflow_tag_remove, |version| TagAction::Run {
        remove: true,
        version: version.to_string(),
        description: String::new(),
        validation: None
    });
    bind!(on_workflow_tag_stop, || TagAction::Stop);
    bind!(on_workflow_tag_load_step_log, |index| TagAction::LoadLog(
        index.max(0) as usize
    ));
}
fn submit(ui: &MainWindow, state: &SharedWorkflowState, action: TagAction) {
    if crate::workflow::state::is_projecting_ui() {
        return;
    }
    if let Err(error) = state.submit(WorkflowAction::Tag(action)) {
        log::warn!("[Workflow][Tag] Admission rejected: {error}");
        crate::workflow::bindings::show_error(
            ui,
            crate::i18n::tr("workflow.command_rejected").as_ref(),
        );
    }
}
pub(crate) fn execute(state: SharedWorkflowState, identity: WorkflowIdentity, effect: TagEffect) {
    match effect {
        TagEffect::CancelInit => state.activity().cancel_init(),
        TagEffect::CancelAll => state.activity().cancel_all(),
        TagEffect::Stop => state.activity().stop(),
        TagEffect::Toast(success, message) => state.post(identity, move |ui| {
            let text = message.resolve();
            if success {
                crate::workflow::bindings::show_success(ui, &text);
            } else {
                crate::workflow::bindings::show_error(ui, &text);
            }
        }),
        TagEffect::Initialize {
            remove,
            sequence,
            repo,
        } => {
            let token = state.activity().initialize();
            let facade = state.clone();
            state.spawn(async move {
                let result = super::initialization::inspect(&repo, remove, &token).await;
                if !token.load(Ordering::Acquire) {
                    facade
                        .publish(
                            identity,
                            WorkflowAction::Tag(TagAction::Initialized {
                                remove,
                                sequence,
                                result,
                            }),
                        )
                        .await;
                }
            });
        }
        TagEffect::Run {
            remove,
            run,
            repo,
            version,
            description,
        } => {
            let token = state.activity().start();
            let facade = state.clone();
            state.spawn(async move {
                let (shown, ready) = tokio::sync::oneshot::channel();
                let projection = facade.clone();
                let navigation_token = token.clone();
                facade.post(identity.clone(), move |ui| {
                    let snapshot = projection.snapshot();
                    let view = snapshot.state().current_state_ref();
                    if navigation_token.load(Ordering::Acquire)
                        || projection.is_closing()
                        || view.tag_run_generation != run
                        || !view.tag_running
                    {
                        return;
                    }
                    super::super::state::initialize_current_ui(ui, &projection);
                    ui.invoke_navigate(Route::JobRun);
                    let _ = shown.send(());
                });
                let mut ready = ready;
                let visible = loop {
                    tokio::select! {
                        result = &mut ready => break result.is_ok(),
                        _ = tokio::time::sleep(std::time::Duration::from_millis(40)) => {
                            if token.load(Ordering::Acquire) || facade.is_closing() {
                                break false;
                            }
                        }
                    }
                };
                if !visible || token.load(Ordering::Acquire) {
                    let result = Err(super::execution::TagRunFailure {
                        summary: crate::i18n::LocalizedText::message("tag.stop_requested"),
                        steps: facade
                            .snapshot()
                            .state()
                            .current_state_ref()
                            .tag_job
                            .steps
                            .clone(),
                    });
                    facade
                        .publish(
                            identity,
                            WorkflowAction::Tag(TagAction::Finished {
                                run,
                                result,
                                duration: std::time::Duration::ZERO,
                                cancelled: true,
                            }),
                        )
                        .await;
                    return;
                }
                let progress =
                    super::progress::Progress::new(facade.clone(), identity.clone(), run);
                let started = Instant::now();
                let result = if remove {
                    super::execution::run_local_remove_tag_flow(
                        &repo,
                        &version,
                        Some(progress.reporter()),
                        token.clone(),
                    )
                    .await
                } else {
                    super::execution::run_local_tag_flow(
                        &repo,
                        &version,
                        &description,
                        Some(progress.reporter()),
                        token.clone(),
                    )
                    .await
                };
                let duration = started.elapsed();
                let cancelled = token.load(Ordering::Acquire);
                progress.finish().await;
                facade
                    .publish(
                        identity,
                        WorkflowAction::Tag(TagAction::Finished {
                            run,
                            result,
                            duration,
                            cancelled,
                        }),
                    )
                    .await;
            });
        }
    }
}
#[cfg(test)]
#[path = "controller_tests.rs"]
mod tests;
