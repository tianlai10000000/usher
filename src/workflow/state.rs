use crate::MainWindow;
use crate::i18n::LocalizedText;
use crate::jobs::{JobRunDescriptor, JobSnapshot, JobStepData, apply_job_run};
use crate::workflow::cd::{CdSnapshot, apply_cd_snapshot, default_cd_snapshot};
use crate::workflow::tag::{VersionBump, default_create_tag_steps};
use std::collections::HashMap;
use std::sync::OnceLock;
mod equality;
mod facade;
mod reducer;
pub(crate) use facade::{SharedWorkflowState, WorkflowIdentity};
pub(crate) use reducer::{CdCommand, WorkflowAction};

static WORKFLOW_SHARED_STATE: OnceLock<SharedWorkflowState> = OnceLock::new();

#[derive(Debug, Clone)]
pub(crate) struct WorkflowStore {
    pub(crate) current_project: WorkflowProjectContext,
    pub(crate) projects: HashMap<String, WorkflowViewState>,
    pub(crate) identity: WorkflowIdentity,
    pub(crate) cd_generation: u64,
}

impl WorkflowStore {
    pub(crate) fn current_state_ref(&self) -> &WorkflowViewState {
        static DEFAULT_VIEW: std::sync::LazyLock<WorkflowViewState> =
            std::sync::LazyLock::new(WorkflowViewState::default);
        self.projects
            .get(&self.current_project.id)
            .unwrap_or(&DEFAULT_VIEW)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkflowProjectContext {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) alias: String,
    pub(crate) local_repo: String,
    pub(crate) remote_repo: String,
    pub(crate) logo: String,
}

impl Default for WorkflowProjectContext {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            name: "Default Project".to_string(),
            alias: String::new(),
            local_repo: String::new(),
            remote_repo: String::new(),
            logo: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WorkflowViewState {
    pub(crate) branch_result: LocalizedText,
    pub(crate) tag_result: LocalizedText,
    pub(crate) tag_selected_tab: i32,
    pub(crate) tag_mode: String,
    pub(crate) tag_version_mode: VersionBump,
    pub(crate) tag_current_version: String,
    pub(crate) tag_next_version: String,
    pub(crate) tag_remove_target: String,
    pub(crate) tag_description: String,
    pub(crate) tag_run_generation: u64,
    pub(crate) tag_add_initializing: bool,
    pub(crate) tag_remove_initializing: bool,
    pub(crate) tag_add_ready: bool,
    pub(crate) tag_remove_ready: bool,
    pub(crate) tag_add_request_seq: u64,
    pub(crate) tag_remove_request_seq: u64,
    pub(crate) tag_running: bool,
    pub(crate) tag_job: JobSnapshot,
    pub(crate) job_run: JobRunDescriptor,
    pub(crate) cd: CdSnapshot,
}

impl Default for WorkflowViewState {
    fn default() -> Self {
        Self {
            branch_result: LocalizedText::message("workflow.ready_branches"),
            tag_result: LocalizedText::message("workflow.ready_tags"),
            tag_selected_tab: 0,
            tag_mode: "local".to_string(),
            tag_version_mode: VersionBump::default(),
            tag_current_version: "v0.0.0".to_string(),
            tag_next_version: "v0.0.1".to_string(),
            tag_remove_target: "v0.0.0".to_string(),
            tag_description: String::new(),
            tag_run_generation: 0,
            tag_add_initializing: false,
            tag_remove_initializing: false,
            tag_add_ready: false,
            tag_remove_ready: false,
            tag_add_request_seq: 0,
            tag_remove_request_seq: 0,
            tag_running: false,
            tag_job: JobSnapshot::queued(default_tag_steps()),
            job_run: JobRunDescriptor::default(),
            cd: default_cd_snapshot(),
        }
    }
}

impl WorkflowViewState {
    pub(crate) fn reset_tag_job(&mut self, steps: Vec<JobStepData>) {
        self.tag_job = JobSnapshot::queued(steps);
        self.job_run = JobRunDescriptor::default();
    }
}

impl Default for WorkflowStore {
    fn default() -> Self {
        let context = WorkflowProjectContext::default();
        Self {
            projects: HashMap::from([(context.id.clone(), WorkflowViewState::default())]),
            current_project: context,
            identity: WorkflowIdentity {
                project: "default".into(),
                generation: 0,
            },
            cd_generation: 0,
        }
    }
}
pub(crate) fn new_shared_state() -> SharedWorkflowState {
    SharedWorkflowState::new()
}

pub(crate) fn register_shared_state(shared_state: &SharedWorkflowState) {
    let _ = WORKFLOW_SHARED_STATE.set(shared_state.clone());
}

pub(crate) fn clear_current_project_heavy_state() {
    let Some(shared_state) = WORKFLOW_SHARED_STATE.get() else {
        return;
    };

    if let Err(error) = shared_state.submit(WorkflowAction::ClearHeavy) {
        log::warn!("[Workflow] Cleanup admission rejected: {error}");
    }
}

fn clear_heavy_view_state(view: &mut WorkflowViewState) {
    // Tag execution history is user-visible workflow state, not a disposable
    // cache. Only release reconstructable CD details here.
    view.cd.summary = LocalizedText::message("cd.state_cleared");
    let detail = LocalizedText::message("cd.reopen_for_details");
    view.cd.build_detail = detail.clone();
    view.cd.test_detail = detail.clone();
    view.cd.deploy_detail = detail.clone();
    view.cd.verify_detail = detail;
}

pub(crate) fn initialize_current_ui(ui: &MainWindow, shared_state: &SharedWorkflowState) {
    shared_state.install(ui);
    let snapshot = shared_state.snapshot();
    if snapshot.state().identity == shared_state.identity() {
        shared_state.project_ui(ui, &snapshot);
    }
}

pub(crate) fn reproject_registered_ui(ui: &MainWindow) {
    if let Some(shared_state) = WORKFLOW_SHARED_STATE.get() {
        initialize_current_ui(ui, shared_state);
    }
}

thread_local! { static PROJECTING_UI: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
pub(crate) fn is_projecting_ui() -> bool {
    PROJECTING_UI.with(std::cell::Cell::get)
}
pub(crate) fn with_ui_projection<T>(apply: impl FnOnce() -> T) -> T {
    struct Guard(bool);
    impl Drop for Guard {
        fn drop(&mut self) {
            PROJECTING_UI.with(|flag| flag.set(self.0));
        }
    }
    let _guard = Guard(PROJECTING_UI.with(|flag| flag.replace(true)));
    apply()
}
pub(crate) fn initialize_ui(
    ui: &MainWindow,
    context: &WorkflowProjectContext,
    state: &WorkflowViewState,
) {
    with_ui_projection(|| {
        ui.set_workflow_project_id(context.id.clone().into());
        let project_name = if context.id == "default" {
            crate::i18n::tr("projects.default_project").into_owned()
        } else {
            context.name.clone()
        };
        ui.set_workflow_project_name(project_name.into());
        ui.set_workflow_project_alias(context.alias.clone().into());
        ui.set_workflow_project_local_repo(context.local_repo.clone().into());
        ui.set_workflow_project_remote_repo(context.remote_repo.clone().into());
        ui.set_workflow_project_logo(context.logo.clone().into());
        ui.set_workflow_branch_result(state.branch_result.resolve().into());
        ui.set_workflow_tag_result(state.tag_result.resolve().into());
        ui.set_workflow_tag_selected_tab(state.tag_selected_tab);
        ui.set_workflow_tag_description(state.tag_description.clone().into());
        ui.set_workflow_tag_mode(state.tag_mode.clone().into());
        ui.set_workflow_tag_version_mode(state.tag_version_mode.as_ui_str().into());
        if state.tag_mode == "remote" {
            let unavailable = crate::i18n::tr("common.unavailable").into_owned();
            ui.set_workflow_tag_current_version(unavailable.clone().into());
            ui.set_workflow_tag_next_version(unavailable.clone().into());
            ui.set_workflow_tag_remove_target(unavailable.into());
        } else {
            ui.set_workflow_tag_current_version(state.tag_current_version.clone().into());
            ui.set_workflow_tag_next_version(state.tag_next_version.clone().into());
            ui.set_workflow_tag_remove_target(state.tag_remove_target.clone().into());
        }
        ui.set_workflow_tag_add_initializing(state.tag_add_initializing);
        ui.set_workflow_tag_remove_initializing(state.tag_remove_initializing);
        ui.set_workflow_tag_running(state.tag_running);
        apply_job_run(
            ui,
            &state.job_run,
            &state.tag_job,
            &state.tag_result,
            state.tag_running,
        );
        apply_cd_snapshot(ui, &state.cd);
    });
}

pub(crate) fn normalize_project_id(alias: &str, name: &str) -> String {
    let alias_trimmed = alias.trim();
    if !alias_trimmed.is_empty() {
        return alias_trimmed.to_lowercase();
    }
    let name_trimmed = name.trim();
    if name_trimmed.is_empty() {
        "default".to_string()
    } else {
        name_trimmed.to_lowercase().replace(' ', "-")
    }
}

pub(crate) fn normalize_project_name(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "Default Project".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn normalize_project_alias(value: &str) -> String {
    value.trim().to_string()
}

pub(crate) fn default_tag_steps() -> Vec<JobStepData> {
    default_create_tag_steps()
}

#[cfg(test)]
mod tests {
    use super::{
        VersionBump, WorkflowProjectContext, WorkflowViewState, clear_heavy_view_state,
        initialize_ui,
    };
    use crate::MainWindow;
    use crate::jobs::{JobConclusion, JobStepStatus};
    use std::time::Duration;

    #[test]
    fn route_cleanup_preserves_tag_execution_history() {
        let mut view = WorkflowViewState {
            tag_selected_tab: 1,
            tag_result: "Tag workflow failed at step 2.".into(),
            ..WorkflowViewState::default()
        };
        view.tag_job.steps[0].finish(
            JobConclusion::Success,
            Some(Duration::from_secs(1)),
            "Complete",
            false,
        );
        view.tag_job.steps[0].set_log("preserved log".into());
        view.job_run.active = true;
        view.job_run.source = "tag".to_string();

        clear_heavy_view_state(&mut view);

        assert_eq!(view.tag_selected_tab, 1);
        assert_eq!(
            view.tag_result.source_text(),
            "Tag workflow failed at step 2."
        );
        assert_eq!(view.tag_job.steps[0].status, JobStepStatus::Completed);
        assert_eq!(
            view.tag_job.steps[0].conclusion,
            Some(JobConclusion::Success)
        );
        assert_eq!(view.tag_job.steps[0].log.as_str(), "preserved log");
        assert_eq!(view.tag_job.steps[0].duration, Some(Duration::from_secs(1)));
        assert!(view.job_run.active);
        assert_eq!(view.job_run.source, "tag");
    }

    #[test]
    fn resetting_tag_plan_clears_the_published_job_run() {
        let mut view = WorkflowViewState::default();
        assert_eq!(view.tag_version_mode, VersionBump::Patch);
        view.tag_job.steps[0].finish(JobConclusion::Failure, None, "Failed", true);
        view.job_run.active = true;
        view.job_run.source = "tag".to_string();

        view.reset_tag_job(super::default_tag_steps());

        assert!(!view.job_run.active);
        assert!(view.job_run.source.is_empty());
        assert!(view.tag_job.steps.iter().all(|step| {
            step.status == JobStepStatus::Queued
                && step.conclusion.is_none()
                && !step.default_expanded
        }));
    }

    #[test]
    fn version_bump_projects_to_the_existing_slint_string_protocol() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().expect("main window");
        let view = WorkflowViewState {
            tag_version_mode: VersionBump::Major,
            ..WorkflowViewState::default()
        };

        initialize_ui(&ui, &WorkflowProjectContext::default(), &view);

        assert_eq!(ui.get_workflow_tag_version_mode().as_str(), "major");
    }
}

pub(crate) async fn shutdown_async() {
    if let Some(state) = WORKFLOW_SHARED_STATE.get() {
        state.shutdown_async().await;
    }
}
