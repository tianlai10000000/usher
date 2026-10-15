use crate::i18n::LocalizedText;
use crate::workflow::bindings::show_error;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CdStepStatus {
    Pending,
    Running,
    Success,
    RolledBack,
}

impl CdStepStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Success => "success",
            Self::RolledBack => "rolled_back",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeploymentStage {
    Build,
    Test,
    Deploy,
    Verify,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RollbackStage {
    Deploy,
    Verify,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CdSnapshot {
    pub(crate) running: bool,
    pub(crate) summary: LocalizedText,
    pub(crate) build_status: CdStepStatus,
    pub(crate) build_detail: LocalizedText,
    pub(crate) test_status: CdStepStatus,
    pub(crate) test_detail: LocalizedText,
    pub(crate) deploy_status: CdStepStatus,
    pub(crate) deploy_detail: LocalizedText,
    pub(crate) verify_status: CdStepStatus,
    pub(crate) verify_detail: LocalizedText,
}

pub(crate) fn default_cd_snapshot() -> CdSnapshot {
    CdSnapshot {
        running: false,
        summary: LocalizedText::message("cd.no_deployment"),
        build_status: CdStepStatus::Pending,
        build_detail: LocalizedText::message("cd.waiting_to_start"),
        test_status: CdStepStatus::Pending,
        test_detail: LocalizedText::message("cd.waiting_to_start"),
        deploy_status: CdStepStatus::Pending,
        deploy_detail: LocalizedText::message("cd.waiting_to_start"),
        verify_status: CdStepStatus::Pending,
        verify_detail: LocalizedText::message("cd.waiting_to_start"),
    }
}

pub(crate) fn validate_cd_request(req: &CdRequest) -> Result<(), LocalizedText> {
    if req.environment.trim().is_empty() {
        return Err(LocalizedText::message("cd.environment_required"));
    }
    if req.release.trim().is_empty() {
        return Err(LocalizedText::message("cd.release_required"));
    }
    if req.commit.trim().is_empty() {
        return Err(LocalizedText::message("cd.commit_required"));
    }
    Ok(())
}

pub(crate) fn deployment_snapshot(req: &CdRequest, stage: DeploymentStage) -> CdSnapshot {
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
        DeploymentStage::Build => {
            snapshot.build_status = CdStepStatus::Running;
            snapshot.build_detail = LocalizedText::message("cd.compiling_artifacts");
        }
        DeploymentStage::Test => {
            snapshot.build_status = CdStepStatus::Success;
            snapshot.build_detail = LocalizedText::message("cd.artifacts_compiled");
            snapshot.test_status = CdStepStatus::Running;
            snapshot.test_detail = LocalizedText::message("cd.executing_tests");
        }
        DeploymentStage::Deploy => {
            snapshot.build_status = CdStepStatus::Success;
            snapshot.build_detail = LocalizedText::message("cd.artifacts_compiled");
            snapshot.test_status = CdStepStatus::Success;
            snapshot.test_detail = LocalizedText::message("cd.tests_passed");
            snapshot.deploy_status = CdStepStatus::Running;
            snapshot.deploy_detail = LocalizedText::message("cd.rolling_out");
        }
        DeploymentStage::Verify => {
            snapshot.build_status = CdStepStatus::Success;
            snapshot.build_detail = LocalizedText::message("cd.artifacts_compiled");
            snapshot.test_status = CdStepStatus::Success;
            snapshot.test_detail = LocalizedText::message("cd.tests_passed");
            snapshot.deploy_status = CdStepStatus::Success;
            snapshot.deploy_detail = LocalizedText::message("cd.release_promoted");
            snapshot.verify_status = CdStepStatus::Running;
            snapshot.verify_detail = LocalizedText::message("cd.verifying_deployment");
        }
        DeploymentStage::Done => {
            snapshot.running = false;
            snapshot.summary = LocalizedText::message_with(
                "cd.deployment_finished",
                [
                    ("release", req.release.clone()),
                    ("environment", req.environment.clone()),
                ],
            );
            snapshot.build_status = CdStepStatus::Success;
            snapshot.build_detail = LocalizedText::message("cd.artifacts_compiled");
            snapshot.test_status = CdStepStatus::Success;
            snapshot.test_detail = LocalizedText::message("cd.tests_passed");
            snapshot.deploy_status = CdStepStatus::Success;
            snapshot.deploy_detail = LocalizedText::message("cd.release_promoted");
            snapshot.verify_status = CdStepStatus::Success;
            snapshot.verify_detail = LocalizedText::message("cd.deployment_verified");
        }
    }

    snapshot
}

pub(crate) fn rollback_snapshot(commit: &str, stage: RollbackStage) -> CdSnapshot {
    let mut snapshot = default_cd_snapshot();
    snapshot.running = true;
    snapshot.summary = LocalizedText::message_with(
        "cd.rolling_back_summary",
        [("commit", commit.trim().to_string())],
    );
    snapshot.build_status = CdStepStatus::Success;
    snapshot.build_detail = LocalizedText::message("cd.using_verified_artifacts");
    snapshot.test_status = CdStepStatus::Success;
    snapshot.test_detail = LocalizedText::message("cd.reusing_stable_test");

    match stage {
        RollbackStage::Deploy => {
            snapshot.deploy_status = CdStepStatus::Running;
            snapshot.deploy_detail = LocalizedText::message("cd.switching_traffic");
        }
        RollbackStage::Verify => {
            snapshot.deploy_status = CdStepStatus::RolledBack;
            snapshot.deploy_detail = LocalizedText::message("cd.traffic_restored");
            snapshot.verify_status = CdStepStatus::Running;
            snapshot.verify_detail = LocalizedText::message("cd.verifying_rollback");
        }
        RollbackStage::Done => {
            snapshot.running = false;
            snapshot.summary = LocalizedText::message_with(
                "cd.rollback_finished",
                [("commit", commit.trim().to_string())],
            );
            snapshot.deploy_status = CdStepStatus::RolledBack;
            snapshot.deploy_detail = LocalizedText::message("cd.traffic_restored");
            snapshot.verify_status = CdStepStatus::Success;
            snapshot.verify_detail = LocalizedText::message("cd.rollback_verified");
        }
    }

    snapshot
}

pub(crate) fn apply_cd_snapshot(ui: &MainWindow, snapshot: &CdSnapshot) {
    ui.set_workflow_cd_running(snapshot.running);
    ui.set_workflow_cd_summary(snapshot.summary.resolve().into());
    ui.set_workflow_cd_build_status(snapshot.build_status.as_str().into());
    ui.set_workflow_cd_build_detail(snapshot.build_detail.resolve().into());
    ui.set_workflow_cd_test_status(snapshot.test_status.as_str().into());
    ui.set_workflow_cd_test_detail(snapshot.test_detail.resolve().into());
    ui.set_workflow_cd_deploy_status(snapshot.deploy_status.as_str().into());
    ui.set_workflow_cd_deploy_detail(snapshot.deploy_detail.resolve().into());
    ui.set_workflow_cd_verify_status(snapshot.verify_status.as_str().into());
    ui.set_workflow_cd_verify_detail(snapshot.verify_detail.resolve().into());
}

pub(crate) fn show_completion(ui: &MainWindow, title: &str, message: &str) {
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

pub(crate) fn bind_cd(ui: &MainWindow, state: SharedWorkflowState) {
    let weak = ui.as_weak();
    let start = state.clone();
    ui.on_workflow_cd_start(move |environment, release, commit| {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) = start.submit(crate::workflow::state::WorkflowAction::Cd(
                crate::workflow::state::CdCommand::Deploy(CdRequest {
                    environment: environment.to_string(),
                    release: release.to_string(),
                    commit: commit.to_string(),
                }),
            )) {
                log::warn!("[Workflow][CD] Admission rejected: {error}");
                show_error(&ui, crate::i18n::tr("workflow.command_rejected").as_ref());
            }
        }
    });
    let weak = ui.as_weak();
    ui.on_workflow_cd_rollback(move |commit| {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) = state.submit(crate::workflow::state::WorkflowAction::Cd(
                crate::workflow::state::CdCommand::Rollback(commit.to_string()),
            )) {
                log::warn!("[Workflow][CD] Admission rejected: {error}");
                show_error(&ui, crate::i18n::tr("workflow.command_rejected").as_ref());
            }
        }
    });
}

pub(crate) fn execute(
    state: SharedWorkflowState,
    identity: crate::workflow::state::WorkflowIdentity,
    (command, generation): (crate::workflow::state::CdCommand, u64),
) {
    use crate::workflow::state::{CdCommand, WorkflowAction};
    let facade = state.clone();
    state.spawn(async move {
        let snapshots = match &command {
            CdCommand::Deploy(request) => [
                DeploymentStage::Test,
                DeploymentStage::Deploy,
                DeploymentStage::Verify,
                DeploymentStage::Done,
            ]
            .into_iter()
            .map(|stage| deployment_snapshot(request, stage))
            .collect::<Vec<_>>(),
            CdCommand::Rollback(commit) => [RollbackStage::Verify, RollbackStage::Done]
                .into_iter()
                .map(|stage| rollback_snapshot(commit, stage))
                .collect(),
        };
        for snapshot in snapshots {
            tokio::time::sleep(Duration::from_millis(700)).await;
            if facade.is_closing() || facade.identity() != identity {
                return;
            }
            let completed = !snapshot.running;
            let completion = if completed {
                Some(LocalizedText::message(match &command {
                    CdCommand::Deploy(_) => "cd.completed",
                    CdCommand::Rollback(_) => "cd.rollback_completed",
                }))
            } else {
                None
            };
            if !facade
                .publish(
                    identity.clone(),
                    WorkflowAction::CdProgress {
                        generation,
                        snapshot,
                        completion,
                    },
                )
                .await
            {
                return;
            }
            if completed {
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> CdRequest {
        CdRequest {
            environment: "production".into(),
            release: "v1.2.3".into(),
            commit: "abc123".into(),
        }
    }

    fn statuses(snapshot: &CdSnapshot) -> [CdStepStatus; 4] {
        [
            snapshot.build_status,
            snapshot.test_status,
            snapshot.deploy_status,
            snapshot.verify_status,
        ]
    }

    fn details(snapshot: &CdSnapshot) -> [String; 4] {
        [
            snapshot.build_detail.source_text(),
            snapshot.test_detail.source_text(),
            snapshot.deploy_detail.source_text(),
            snapshot.verify_detail.source_text(),
        ]
    }

    #[test]
    fn every_deployment_stage_produces_a_complete_typed_snapshot() {
        use CdStepStatus::{Pending, Running, Success};
        let cases = [
            (
                DeploymentStage::Build,
                true,
                [Running, Pending, Pending, Pending],
                [
                    "cd.compiling_artifacts",
                    "cd.waiting_to_start",
                    "cd.waiting_to_start",
                    "cd.waiting_to_start",
                ],
            ),
            (
                DeploymentStage::Test,
                true,
                [Success, Running, Pending, Pending],
                [
                    "cd.artifacts_compiled",
                    "cd.executing_tests",
                    "cd.waiting_to_start",
                    "cd.waiting_to_start",
                ],
            ),
            (
                DeploymentStage::Deploy,
                true,
                [Success, Success, Running, Pending],
                [
                    "cd.artifacts_compiled",
                    "cd.tests_passed",
                    "cd.rolling_out",
                    "cd.waiting_to_start",
                ],
            ),
            (
                DeploymentStage::Verify,
                true,
                [Success, Success, Success, Running],
                [
                    "cd.artifacts_compiled",
                    "cd.tests_passed",
                    "cd.release_promoted",
                    "cd.verifying_deployment",
                ],
            ),
            (
                DeploymentStage::Done,
                false,
                [Success, Success, Success, Success],
                [
                    "cd.artifacts_compiled",
                    "cd.tests_passed",
                    "cd.release_promoted",
                    "cd.deployment_verified",
                ],
            ),
        ];

        for (stage, running, expected_statuses, expected_details) in cases {
            let snapshot = deployment_snapshot(&request(), stage);
            assert_eq!(snapshot.running, running, "{stage:?}");
            assert_eq!(statuses(&snapshot), expected_statuses, "{stage:?}");
            assert_eq!(
                details(&snapshot),
                expected_details.map(|key| LocalizedText::message(key).source_text()),
                "{stage:?}"
            );
        }
    }

    #[test]
    fn every_rollback_stage_produces_a_complete_typed_snapshot() {
        use CdStepStatus::{Pending, RolledBack, Running, Success};
        let cases = [
            (
                RollbackStage::Deploy,
                true,
                [Success, Success, Running, Pending],
                [
                    "cd.using_verified_artifacts",
                    "cd.reusing_stable_test",
                    "cd.switching_traffic",
                    "cd.waiting_to_start",
                ],
            ),
            (
                RollbackStage::Verify,
                true,
                [Success, Success, RolledBack, Running],
                [
                    "cd.using_verified_artifacts",
                    "cd.reusing_stable_test",
                    "cd.traffic_restored",
                    "cd.verifying_rollback",
                ],
            ),
            (
                RollbackStage::Done,
                false,
                [Success, Success, RolledBack, Success],
                [
                    "cd.using_verified_artifacts",
                    "cd.reusing_stable_test",
                    "cd.traffic_restored",
                    "cd.rollback_verified",
                ],
            ),
        ];

        for (stage, running, expected_statuses, expected_details) in cases {
            let snapshot = rollback_snapshot("abc123", stage);
            assert_eq!(snapshot.running, running, "{stage:?}");
            assert_eq!(statuses(&snapshot), expected_statuses, "{stage:?}");
            assert_eq!(
                details(&snapshot),
                expected_details.map(|key| LocalizedText::message(key).source_text()),
                "{stage:?}"
            );
        }
    }

    #[test]
    fn typed_statuses_project_to_the_existing_slint_protocol() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().expect("main window");
        let snapshot = rollback_snapshot("abc123", RollbackStage::Verify);

        apply_cd_snapshot(&ui, &snapshot);

        assert_eq!(ui.get_workflow_cd_build_status(), "success");
        assert_eq!(ui.get_workflow_cd_test_status(), "success");
        assert_eq!(ui.get_workflow_cd_deploy_status(), "rolled_back");
        assert_eq!(ui.get_workflow_cd_verify_status(), "running");
        assert!(ui.get_workflow_cd_running());
    }
}
