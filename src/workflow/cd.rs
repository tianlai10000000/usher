use crate::app_runtime::shared_runtime;
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
    pub(crate) summary: String,
    pub(crate) build_status: String,
    pub(crate) build_detail: String,
    pub(crate) test_status: String,
    pub(crate) test_detail: String,
    pub(crate) deploy_status: String,
    pub(crate) deploy_detail: String,
    pub(crate) verify_status: String,
    pub(crate) verify_detail: String,
}

pub(crate) fn default_cd_snapshot() -> CdSnapshot {
    CdSnapshot {
        running: false,
        summary: "No deployment started.".to_string(),
        build_status: "pending".to_string(),
        build_detail: "Waiting to start.".to_string(),
        test_status: "pending".to_string(),
        test_detail: "Waiting to start.".to_string(),
        deploy_status: "pending".to_string(),
        deploy_detail: "Waiting to start.".to_string(),
        verify_status: "pending".to_string(),
        verify_detail: "Waiting to start.".to_string(),
    }
}

pub(crate) fn validate_cd_request(req: &CdRequest) -> Result<(), String> {
    if req.environment.trim().is_empty() {
        return Err("Environment is required.".to_string());
    }
    if req.release.trim().is_empty() {
        return Err("Release name is required.".to_string());
    }
    if req.commit.trim().is_empty() {
        return Err("Commit hash is required.".to_string());
    }
    Ok(())
}

pub(crate) fn deployment_snapshot(req: &CdRequest, stage: &str) -> CdSnapshot {
    let mut snapshot = default_cd_snapshot();
    snapshot.running = true;
    snapshot.summary = format!(
        "Deploying release '{}' to '{}' from commit '{}'.",
        req.release, req.environment, req.commit
    );

    match stage {
        "build" => {
            snapshot.build_status = "running".to_string();
            snapshot.build_detail = "Compiling build artifacts.".to_string();
        }
        "test" => {
            snapshot.build_status = "success".to_string();
            snapshot.build_detail = "Artifacts compiled successfully.".to_string();
            snapshot.test_status = "running".to_string();
            snapshot.test_detail = "Executing smoke and integration tests.".to_string();
        }
        "deploy" => {
            snapshot.build_status = "success".to_string();
            snapshot.build_detail = "Artifacts compiled successfully.".to_string();
            snapshot.test_status = "success".to_string();
            snapshot.test_detail = "Smoke and integration tests passed.".to_string();
            snapshot.deploy_status = "running".to_string();
            snapshot.deploy_detail = "Rolling out the selected release.".to_string();
        }
        "verify" => {
            snapshot.build_status = "success".to_string();
            snapshot.build_detail = "Artifacts compiled successfully.".to_string();
            snapshot.test_status = "success".to_string();
            snapshot.test_detail = "Smoke and integration tests passed.".to_string();
            snapshot.deploy_status = "success".to_string();
            snapshot.deploy_detail = "Release promoted to target environment.".to_string();
            snapshot.verify_status = "running".to_string();
            snapshot.verify_detail = "Checking post-deploy health and key paths.".to_string();
        }
        "done" => {
            snapshot.running = false;
            snapshot.summary = format!(
                "Deployment finished: release '{}' is live in '{}'.",
                req.release, req.environment
            );
            snapshot.build_status = "success".to_string();
            snapshot.build_detail = "Artifacts compiled successfully.".to_string();
            snapshot.test_status = "success".to_string();
            snapshot.test_detail = "Smoke and integration tests passed.".to_string();
            snapshot.deploy_status = "success".to_string();
            snapshot.deploy_detail = "Release promoted to target environment.".to_string();
            snapshot.verify_status = "success".to_string();
            snapshot.verify_detail = "Health checks and verification passed.".to_string();
        }
        _ => {}
    }

    snapshot
}

pub(crate) fn rollback_snapshot(commit: &str, stage: &str) -> CdSnapshot {
    let mut snapshot = default_cd_snapshot();
    snapshot.running = true;
    snapshot.summary = format!("Rolling back deployment to commit '{}'.", commit.trim());
    snapshot.build_status = "success".to_string();
    snapshot.build_detail = "Using previously verified artifacts.".to_string();
    snapshot.test_status = "success".to_string();
    snapshot.test_detail = "Reusing the last known stable test result.".to_string();

    match stage {
        "deploy" => {
            snapshot.deploy_status = "running".to_string();
            snapshot.deploy_detail = "Switching traffic back to the selected commit.".to_string();
        }
        "verify" => {
            snapshot.deploy_status = "rolled_back".to_string();
            snapshot.deploy_detail = "Traffic moved back to the selected commit.".to_string();
            snapshot.verify_status = "running".to_string();
            snapshot.verify_detail = "Verifying rollback health signals.".to_string();
        }
        "done" => {
            snapshot.running = false;
            snapshot.summary = format!("Rollback finished. Active commit is '{}'.", commit.trim());
            snapshot.deploy_status = "rolled_back".to_string();
            snapshot.deploy_detail = "Traffic moved back to the selected commit.".to_string();
            snapshot.verify_status = "success".to_string();
            snapshot.verify_detail = "Rollback verification passed.".to_string();
        }
        _ => {}
    }

    snapshot
}

pub(crate) fn apply_cd_snapshot(ui: &MainWindow, snapshot: &CdSnapshot) {
    ui.set_workflow_cd_running(snapshot.running);
    ui.set_workflow_cd_summary(snapshot.summary.clone().into());
    ui.set_workflow_cd_build_status(snapshot.build_status.clone().into());
    ui.set_workflow_cd_build_detail(snapshot.build_detail.clone().into());
    ui.set_workflow_cd_test_status(snapshot.test_status.clone().into());
    ui.set_workflow_cd_test_detail(snapshot.test_detail.clone().into());
    ui.set_workflow_cd_deploy_status(snapshot.deploy_status.clone().into());
    ui.set_workflow_cd_deploy_detail(snapshot.deploy_detail.clone().into());
    ui.set_workflow_cd_verify_status(snapshot.verify_status.clone().into());
    ui.set_workflow_cd_verify_detail(snapshot.verify_detail.clone().into());
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
                show_error(&ui, "A CD workflow is already running.");
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
        show_success(&ui, "CD workflow started.");

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
                                show_completion(&ui, "CD workflow completed", &snapshot.summary);
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
            show_error(&ui, "Rollback commit is required.");
            return;
        }

        {
            let Ok(mut guard) = state_rollback.lock() else {
                return;
            };
            if guard.cd_running {
                show_error(&ui, "CD workflow is running. Finish it before rollback.");
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
        show_success(&ui, "Rollback started.");

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
                                show_completion(&ui, "CD rollback completed", &snapshot.summary);
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
