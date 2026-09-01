use crate::app_runtime::shared_runtime;
use crate::i18n::LocalizedText;
use crate::workflow::bindings::{show_error, show_success};
use crate::workflow::state::SharedWorkflowState;
use crate::{
    CustomComponentType, MainWindow, NotificationDelivery, NotificationManager, NotificationOptions,
};
use slint::{ComponentHandle, Global};
use std::time::Duration;

#[derive(Debug, Clone)]
pub(crate) struct CdRequest {
    pub(crate) environment: String,
    pub(crate) release: String,
    pub(crate) commit: String,
}

#[derive(Debug, Clone)]
pub(crate) struct CdSnapshot {
    pub(crate) running: bool,
    pub(crate) summary: LocalizedText,
    pub(crate) build_status: String,
    pub(crate) build_detail: LocalizedText,
    pub(crate) test_status: String,
    pub(crate) test_detail: LocalizedText,
    pub(crate) deploy_status: String,
    pub(crate) deploy_detail: LocalizedText,
    pub(crate) verify_status: String,
    pub(crate) verify_detail: LocalizedText,
}

pub(crate) fn default_cd_snapshot() -> CdSnapshot {
    CdSnapshot {
        running: false,
        summary: LocalizedText::message("cd.no_deployment"),
        build_status: "pending".to_string(),
        build_detail: LocalizedText::message("cd.waiting_to_start"),
        test_status: "pending".to_string(),
        test_detail: LocalizedText::message("cd.waiting_to_start"),
        deploy_status: "pending".to_string(),
        deploy_detail: LocalizedText::message("cd.waiting_to_start"),
        verify_status: "pending".to_string(),
        verify_detail: LocalizedText::message("cd.waiting_to_start"),
    }
}

pub(crate) fn validate_cd_request(req: &CdRequest) -> Result<(), String> {
    if req.environment.trim().is_empty() {
        return Err(crate::i18n::tr("cd.environment_required").into_owned());
    }
    if req.release.trim().is_empty() {
        return Err(crate::i18n::tr("cd.release_required").into_owned());
    }
    if req.commit.trim().is_empty() {
        return Err(crate::i18n::tr("cd.commit_required").into_owned());
    }
    Ok(())
}

pub(crate) fn deployment_snapshot(req: &CdRequest, stage: &str) -> CdSnapshot {
    let mut snapshot = default_cd_snapshot();
    snapshot.running = true;
    snapshot.summary = LocalizedText::message_with(
        "cd.deploying_summary",
        [
            ("release", req.release.clone()),
            ("environment", req.environment.clone()),
            ("commit", req.commit.clone()),
        ],
    );

    match stage {
        "build" => {
            snapshot.build_status = "running".to_string();
            snapshot.build_detail = LocalizedText::message("cd.compiling_artifacts");
        }
        "test" => {
            snapshot.build_status = "success".to_string();
            snapshot.build_detail = LocalizedText::message("cd.artifacts_compiled");
            snapshot.test_status = "running".to_string();
            snapshot.test_detail = LocalizedText::message("cd.executing_tests");
        }
        "deploy" => {
            snapshot.build_status = "success".to_string();
            snapshot.build_detail = LocalizedText::message("cd.artifacts_compiled");
            snapshot.test_status = "success".to_string();
            snapshot.test_detail = LocalizedText::message("cd.tests_passed");
            snapshot.deploy_status = "running".to_string();
            snapshot.deploy_detail = LocalizedText::message("cd.rolling_out");
        }
        "verify" => {
            snapshot.build_status = "success".to_string();
            snapshot.build_detail = LocalizedText::message("cd.artifacts_compiled");
            snapshot.test_status = "success".to_string();
            snapshot.test_detail = LocalizedText::message("cd.tests_passed");
            snapshot.deploy_status = "success".to_string();
            snapshot.deploy_detail = LocalizedText::message("cd.release_promoted");
            snapshot.verify_status = "running".to_string();
            snapshot.verify_detail = LocalizedText::message("cd.verifying_deployment");
        }
        "done" => {
            snapshot.running = false;
            snapshot.summary = LocalizedText::message_with(
                "cd.deployment_finished",
                [
                    ("release", req.release.clone()),
                    ("environment", req.environment.clone()),
                ],
            );
            snapshot.build_status = "success".to_string();
            snapshot.build_detail = LocalizedText::message("cd.artifacts_compiled");
            snapshot.test_status = "success".to_string();
            snapshot.test_detail = LocalizedText::message("cd.tests_passed");
            snapshot.deploy_status = "success".to_string();
            snapshot.deploy_detail = LocalizedText::message("cd.release_promoted");
            snapshot.verify_status = "success".to_string();
            snapshot.verify_detail = LocalizedText::message("cd.deployment_verified");
        }
        _ => {}
    }

    snapshot
}

pub(crate) fn rollback_snapshot(commit: &str, stage: &str) -> CdSnapshot {
    let mut snapshot = default_cd_snapshot();
    snapshot.running = true;
    snapshot.summary = LocalizedText::message_with(
        "cd.rolling_back_summary",
        [("commit", commit.trim().to_string())],
    );
    snapshot.build_status = "success".to_string();
    snapshot.build_detail = LocalizedText::message("cd.using_verified_artifacts");
    snapshot.test_status = "success".to_string();
    snapshot.test_detail = LocalizedText::message("cd.reusing_stable_test");

    match stage {
        "deploy" => {
            snapshot.deploy_status = "running".to_string();
            snapshot.deploy_detail = LocalizedText::message("cd.switching_traffic");
        }
        "verify" => {
            snapshot.deploy_status = "rolled_back".to_string();
            snapshot.deploy_detail = LocalizedText::message("cd.traffic_restored");
            snapshot.verify_status = "running".to_string();
            snapshot.verify_detail = LocalizedText::message("cd.verifying_rollback");
        }
        "done" => {
            snapshot.running = false;
            snapshot.summary = LocalizedText::message_with(
                "cd.rollback_finished",
                [("commit", commit.trim().to_string())],
            );
            snapshot.deploy_status = "rolled_back".to_string();
            snapshot.deploy_detail = LocalizedText::message("cd.traffic_restored");
            snapshot.verify_status = "success".to_string();
            snapshot.verify_detail = LocalizedText::message("cd.rollback_verified");
        }
        _ => {}
    }

    snapshot
}

pub(crate) fn apply_cd_snapshot(ui: &MainWindow, snapshot: &CdSnapshot) {
    ui.set_workflow_cd_running(snapshot.running);
    ui.set_workflow_cd_summary(snapshot.summary.resolve().into());
    ui.set_workflow_cd_build_status(snapshot.build_status.clone().into());
    ui.set_workflow_cd_build_detail(snapshot.build_detail.resolve().into());
    ui.set_workflow_cd_test_status(snapshot.test_status.clone().into());
    ui.set_workflow_cd_test_detail(snapshot.test_detail.resolve().into());
    ui.set_workflow_cd_deploy_status(snapshot.deploy_status.clone().into());
    ui.set_workflow_cd_deploy_detail(snapshot.deploy_detail.resolve().into());
    ui.set_workflow_cd_verify_status(snapshot.verify_status.clone().into());
    ui.set_workflow_cd_verify_detail(snapshot.verify_detail.resolve().into());
}

fn show_completion(ui: &MainWindow, title: &str, message: &str) {
    NotificationManager::get(ui).invoke_success(NotificationOptions {
        title: title.into(),
        message: message.into(),
        duration: 5000,
        icon: Default::default(),
        component: CustomComponentType::None,
        actions: Default::default(),
        tag: "cd-workflow".into(),
        source: "CD".into(),
        category: "cd_workflow".into(),
        native_delivery: NotificationDelivery::WhenUnfocused,
    });
}

pub(crate) fn bind_cd(ui: &MainWindow, shared_state: SharedWorkflowState) {
    let runtime = shared_runtime();

    let ui_weak = ui.as_weak();
    let runtime_start = runtime.clone();
    let state_start = shared_state.clone();
    ui.on_workflow_cd_start(move |environment, release, commit| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let req = CdRequest {
            environment: environment.to_string(),
            release: release.to_string(),
            commit: commit.to_string(),
        };

        if let Err(error) = validate_cd_request(&req) {
            show_error(&ui, &error);
            ui.set_workflow_cd_summary(error.into());
            return;
        }

        {
            let Ok(mut guard) = state_start.lock() else {
                return;
            };
            if guard.cd_running {
                show_error(&ui, crate::i18n::tr("cd.already_running").as_ref());
                return;
            }
            guard.cd_running = true;
        }

        let first_snapshot = deployment_snapshot(&req, "build");
        {
            let Ok(mut guard) = state_start.lock() else {
                return;
            };
            guard.upsert_current(|view| view.cd = first_snapshot.clone());
        }
        apply_cd_snapshot(&ui, &first_snapshot);
        show_success(&ui, crate::i18n::tr("cd.started").as_ref());

        let ui_weak_task = ui_weak.clone();
        let state_task = state_start.clone();
        let project_id = state_start
            .lock()
            .ok()
            .map(|guard| guard.current_project.id.clone())
            .unwrap_or_else(|| "default".to_string());
        runtime_start.spawn(async move {
            let stages = ["test", "deploy", "verify", "done"];
            for stage in stages {
                tokio::time::sleep(Duration::from_millis(700)).await;
                let snapshot = deployment_snapshot(&req, stage);
                let completed = stage == "done";
                let _ = slint::invoke_from_event_loop({
                    let ui_weak_task = ui_weak_task.clone();
                    let state_task = state_task.clone();
                    let project_id = project_id.clone();
                    move || {
                        if let Ok(mut guard) = state_task.lock() {
                            let current_project_id = guard.current_project.id.clone();
                            guard.projects.entry(project_id.clone()).or_default().cd =
                                snapshot.clone();
                            if current_project_id != project_id {
                                return;
                            }
                        }
                        if let Some(ui) = ui_weak_task.upgrade() {
                            apply_cd_snapshot(&ui, &snapshot);
                            if completed {
                                let summary = snapshot.summary.resolve();
                                show_completion(
                                    &ui,
                                    crate::i18n::tr("cd.completed").as_ref(),
                                    summary.as_str(),
                                );
                            }
                        }
                    }
                });
            }
            if let Ok(mut guard) = state_task.lock() {
                guard.cd_running = false;
            }
        });
    });

    let ui_weak = ui.as_weak();
    let runtime_rollback = runtime.clone();
    let state_rollback = shared_state.clone();
    ui.on_workflow_cd_rollback(move |commit| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let commit = commit.to_string();
        if commit.trim().is_empty() {
            show_error(&ui, crate::i18n::tr("cd.rollback_required").as_ref());
            return;
        }

        {
            let Ok(mut guard) = state_rollback.lock() else {
                return;
            };
            if guard.cd_running {
                show_error(&ui, crate::i18n::tr("cd.finish_before_rollback").as_ref());
                return;
            }
            guard.cd_running = true;
        }

        let first_snapshot = rollback_snapshot(&commit, "deploy");
        {
            let Ok(mut guard) = state_rollback.lock() else {
                return;
            };
            guard.upsert_current(|view| view.cd = first_snapshot.clone());
        }
        apply_cd_snapshot(&ui, &first_snapshot);
        show_success(&ui, crate::i18n::tr("cd.rollback_started").as_ref());

        let ui_weak_task = ui_weak.clone();
        let state_task = state_rollback.clone();
        let project_id = state_rollback
            .lock()
            .ok()
            .map(|guard| guard.current_project.id.clone())
            .unwrap_or_else(|| "default".to_string());
        runtime_rollback.spawn(async move {
            for stage in ["verify", "done"] {
                tokio::time::sleep(Duration::from_millis(700)).await;
                let snapshot = rollback_snapshot(&commit, stage);
                let completed = stage == "done";
                let _ = slint::invoke_from_event_loop({
                    let ui_weak_task = ui_weak_task.clone();
                    let state_task = state_task.clone();
                    let project_id = project_id.clone();
                    move || {
                        if let Ok(mut guard) = state_task.lock() {
                            let current_project_id = guard.current_project.id.clone();
                            guard.projects.entry(project_id.clone()).or_default().cd =
                                snapshot.clone();
                            if current_project_id != project_id {
                                return;
                            }
                        }
                        if let Some(ui) = ui_weak_task.upgrade() {
                            apply_cd_snapshot(&ui, &snapshot);
                            if completed {
                                let summary = snapshot.summary.resolve();
                                show_completion(
                                    &ui,
                                    crate::i18n::tr("cd.rollback_completed").as_ref(),
                                    summary.as_str(),
                                );
                            }
                        }
                    }
                });
            }
            if let Ok(mut guard) = state_task.lock() {
                guard.cd_running = false;
            }
        });
    });
}
