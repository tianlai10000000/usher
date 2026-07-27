use crate::workflow::cd::{CdSnapshot, apply_cd_snapshot, default_cd_snapshot};
use crate::{MainWindow, StepItem};
use slint::{ModelRc, VecModel};
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
    pub(crate) branch_result: String,
    pub(crate) tag_result: String,
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
    pub(crate) tag_steps: Vec<StepItem>,
    pub(crate) cd: CdSnapshot,
}

impl Default for WorkflowViewState {
    fn default() -> Self {
        Self {
            branch_result: "Ready to recreate branches.".to_string(),
            tag_result: "Ready to add or remove a tag.".to_string(),
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
            tag_steps: default_tag_steps(),
            cd: default_cd_snapshot(),
        }
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

    guard.upsert_current(|view| {
        view.tag_add_request_seq = view.tag_add_request_seq.saturating_add(1);
        view.tag_remove_request_seq = view.tag_remove_request_seq.saturating_add(1);
        for step in &mut view.tag_steps {
            step.log = "".into();
        }
        view.cd.summary = "Workflow state cleared after leaving the page.".to_string();
        view.cd.build_detail =
            "Re-open Workflow to inspect the latest CD step details.".to_string();
        view.cd.test_detail = "Re-open Workflow to inspect the latest CD step details.".to_string();
        view.cd.deploy_detail =
            "Re-open Workflow to inspect the latest CD step details.".to_string();
        view.cd.verify_detail =
            "Re-open Workflow to inspect the latest CD step details.".to_string();
    });
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

pub(crate) fn initialize_ui(
    ui: &MainWindow,
    context: &WorkflowProjectContext,
    state: &WorkflowViewState,
) {
    ui.set_workflow_project_id(context.id.clone().into());
    ui.set_workflow_project_name(context.name.clone().into());
    ui.set_workflow_project_alias(context.alias.clone().into());
    ui.set_workflow_project_local_repo(context.local_repo.clone().into());
    ui.set_workflow_project_remote_repo(context.remote_repo.clone().into());
    ui.set_workflow_project_logo(context.logo.clone().into());
    ui.set_workflow_branch_result(state.branch_result.clone().into());
    ui.set_workflow_tag_result(state.tag_result.clone().into());
    ui.set_workflow_tag_mode(state.tag_mode.clone().into());
    ui.set_workflow_tag_version_mode(state.tag_version_mode.clone().into());
    ui.set_workflow_tag_current_version(state.tag_current_version.clone().into());
    ui.set_workflow_tag_next_version(state.tag_next_version.clone().into());
    ui.set_workflow_tag_remove_target(state.tag_remove_target.clone().into());
    ui.set_workflow_tag_add_initializing(state.tag_add_initializing);
    ui.set_workflow_tag_remove_initializing(state.tag_remove_initializing);
    ui.set_workflow_tag_running(state.tag_running);
    ui.set_workflow_tag_steps(ModelRc::new(VecModel::from(state.tag_steps.clone())));
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

pub(crate) fn default_tag_steps() -> Vec<StepItem> {
    vec![
        StepItem {
            name: "Enter local repository".into(),
            status: "pending".into(),
            detail: "Resolve the configured local repository path.".into(),
            log: "".into(),
        },
        StepItem {
            name: "Checkout release".into(),
            status: "pending".into(),
            detail: "Switch to the release branch.".into(),
            log: "".into(),
        },
        StepItem {
            name: "Pull release".into(),
            status: "pending".into(),
            detail: "Pull the latest release branch changes.".into(),
            log: "".into(),
        },
        StepItem {
            name: "Checkout main".into(),
            status: "pending".into(),
            detail: "Switch back to the main branch.".into(),
            log: "".into(),
        },
        StepItem {
            name: "Rebase main on release".into(),
            status: "pending".into(),
            detail: "Rebase main onto release.".into(),
            log: "".into(),
        },
        StepItem {
            name: "Push main".into(),
            status: "pending".into(),
            detail: "Push rebased main to the remote.".into(),
            log: "".into(),
        },
        StepItem {
            name: "Create annotated tag".into(),
            status: "pending".into(),
            detail: "Create the new annotated semantic version tag.".into(),
            log: "".into(),
        },
        StepItem {
            name: "Push tag".into(),
            status: "pending".into(),
            detail: "Push the new tag to the remote.".into(),
            log: "".into(),
        },
        StepItem {
            name: "Complete".into(),
            status: "pending".into(),
            detail: "Finish the local tag workflow.".into(),
            log: "".into(),
        },
    ]
}
