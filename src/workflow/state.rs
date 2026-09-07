use crate::MainWindow;
use crate::i18n::LocalizedText;
use crate::jobs::{JobRunDescriptor, JobSnapshot, JobStepData, JobStepDefinition, apply_job_run};
use crate::workflow::cd::{CdSnapshot, apply_cd_snapshot, default_cd_snapshot};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub(crate) type SharedWorkflowState = Arc<Mutex<WorkflowStore>>;

static WORKFLOW_SHARED_STATE: OnceLock<SharedWorkflowState> = OnceLock::new();

#[derive(Debug, Default)]
pub(crate) struct WorkflowStore {
    pub(crate) current_project: WorkflowProjectContext,
    pub(crate) projects: HashMap<String, WorkflowViewState>,
    pub(crate) cd_running: bool,
}

impl WorkflowStore {
    pub(crate) fn current_state_ref(&self) -> &WorkflowViewState {
        static DEFAULT_VIEW: std::sync::LazyLock<WorkflowViewState> =
            std::sync::LazyLock::new(WorkflowViewState::default);
        self.projects
            .get(&self.current_project.id)
            .unwrap_or(&DEFAULT_VIEW)
    }

    pub(crate) fn upsert_current<F>(&mut self, updater: F)
    where
        F: FnOnce(&mut WorkflowViewState),
    {
        let entry = self
            .projects
            .entry(self.current_project.id.clone())
            .or_default();
        updater(entry);
    }

    pub(crate) fn with_current<R, F>(&self, reader: F) -> R
    where
        F: FnOnce(&WorkflowProjectContext, &WorkflowViewState) -> R,
    {
        static DEFAULT_VIEW: std::sync::LazyLock<WorkflowViewState> =
            std::sync::LazyLock::new(WorkflowViewState::default);
        let view = self
            .projects
            .get(&self.current_project.id)
            .unwrap_or(&DEFAULT_VIEW);
        reader(&self.current_project, view)
    }

    pub(crate) fn set_project(&mut self, context: WorkflowProjectContext) -> WorkflowViewState {
        self.current_project = context;
        let current_id = self.current_project.id.clone();
        self.projects
            .retain(|id, _| id == "default" || id == &current_id);
        self.projects.entry(current_id).or_default().clone()
    }
}

#[derive(Debug, Clone)]
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
    pub(crate) tag_version_mode: String,
    pub(crate) tag_current_version: String,
    pub(crate) tag_next_version: String,
    pub(crate) tag_remove_target: String,
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
            tag_version_mode: "patch".to_string(),
            tag_current_version: "v0.0.0".to_string(),
            tag_next_version: "v0.0.1".to_string(),
            tag_remove_target: "v0.0.0".to_string(),
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

pub(crate) fn new_shared_state() -> SharedWorkflowState {
    let mut initial_store = WorkflowStore {
        current_project: WorkflowProjectContext::default(),
        projects: HashMap::new(),
        cd_running: false,
    };
    initial_store.projects.insert(
        initial_store.current_project.id.clone(),
        WorkflowViewState::default(),
    );
    Arc::new(Mutex::new(initial_store))
}

pub(crate) fn register_shared_state(shared_state: &SharedWorkflowState) {
    let _ = WORKFLOW_SHARED_STATE.set(shared_state.clone());
}

pub(crate) fn clear_current_project_heavy_state() {
    let Some(shared_state) = WORKFLOW_SHARED_STATE.get() else {
        return;
    };

    let Ok(mut guard) = shared_state.lock() else {
        return;
    };

    guard.upsert_current(clear_heavy_view_state);
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
    let Ok(guard) = shared_state.lock() else {
        let ctx = WorkflowProjectContext::default();
        let st = WorkflowViewState::default();
        initialize_ui(ui, &ctx, &st);
        return;
    };
    let state = guard.current_state_ref();
    initialize_ui(ui, &guard.current_project, state);
}

pub(crate) fn reproject_registered_ui(ui: &MainWindow) {
    if let Some(shared_state) = WORKFLOW_SHARED_STATE.get() {
        initialize_current_ui(ui, shared_state);
    }
}

pub(crate) fn initialize_ui(
    ui: &MainWindow,
    context: &WorkflowProjectContext,
    state: &WorkflowViewState,
) {
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
    ui.set_workflow_tag_mode(state.tag_mode.clone().into());
    ui.set_workflow_tag_version_mode(state.tag_version_mode.clone().into());
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
    vec![
        queued_step(
            "tag.step.enter_repository",
            &["git", "rev-parse", "--show-toplevel"],
            "tag.step.enter_repository_detail",
        ),
        queued_step(
            "tag.step.checkout_release",
            &["git", "checkout", "release"],
            "tag.step.checkout_release_detail",
        ),
        queued_step(
            "tag.step.pull_release",
            &["git", "pull", "origin", "release"],
            "tag.step.pull_release_detail",
        ),
        queued_step(
            "tag.step.checkout_main",
            &["git", "checkout", "main"],
            "tag.step.checkout_main_detail",
        ),
        queued_step(
            "tag.step.rebase_main",
            &["git", "rebase", "release"],
            "tag.step.rebase_main_detail",
        ),
        queued_step(
            "tag.step.push_main",
            &["git", "push", "origin", "main"],
            "tag.step.push_main_detail",
        ),
        queued_step(
            "tag.step.create_annotated",
            &["git", "tag", "-a", "<version>"],
            "tag.step.create_annotated_detail",
        ),
        queued_step(
            "tag.step.push_tag",
            &["git", "push", "origin", "<version>"],
            "tag.step.push_tag_detail",
        ),
        queued_step(
            "tag.step.complete",
            &["complete"],
            "tag.step.complete_detail",
        ),
    ]
}

fn queued_step(name_key: &'static str, command: &[&str], detail_key: &'static str) -> JobStepData {
    JobStepData::queued(
        JobStepDefinition::new(
            LocalizedText::message(name_key),
            command.iter().map(|part| (*part).to_string()).collect(),
        ),
        LocalizedText::message(detail_key),
    )
}

#[cfg(test)]
mod tests {
    use super::{WorkflowViewState, clear_heavy_view_state};
    use crate::jobs::{JobConclusion, JobStepStatus};

    #[test]
    fn route_cleanup_preserves_tag_execution_history() {
        let mut view = WorkflowViewState {
            tag_selected_tab: 1,
            tag_result: "Tag workflow failed at step 2.".into(),
            ..WorkflowViewState::default()
        };
        view.tag_job.steps[0].status = JobStepStatus::Completed;
        view.tag_job.steps[0].conclusion = Some(JobConclusion::Success);
        view.tag_job.steps[0].set_log("preserved log".into());
        view.tag_job.steps[0].duration = "1s".into();
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
        assert_eq!(view.tag_job.steps[0].duration.as_str(), "1s");
        assert!(view.job_run.active);
        assert_eq!(view.job_run.source, "tag");
    }

    #[test]
    fn resetting_tag_plan_clears_the_published_job_run() {
        let mut view = WorkflowViewState::default();
        view.job_run.active = true;
        view.job_run.source = "tag".to_string();

        view.reset_tag_job(super::default_tag_steps());

        assert!(!view.job_run.active);
        assert!(view.job_run.source.is_empty());
    }
}
