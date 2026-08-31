//! Project registry state, typed actions, and automatic persistence facade.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

use usher_state_store::{PersistentStore, Reduction, StoreSnapshot, Subscription};

use super::model::{ProjectConfig, ProjectPrOverview, ProjectRepositoryMetadata};
use super::overview::{ProjectPrOverviewTask, collect_pr_overview_tasks};
use super::storage::write_projects;

#[derive(Clone, Default)]
pub(super) struct ProjectState {
    pub(super) projects: Vec<ProjectConfig>,
    persistence_feedback: Option<&'static str>,
    pending_remote_logos: HashSet<String>,
    pending_pr_overview: HashSet<String>,
    pending_saves: HashSet<String>,
}

enum ProjectAction {
    Hydrate(Vec<ProjectConfig>),
    Add(ProjectConfig),
    Update {
        original_alias: String,
        project: ProjectConfig,
    },
    Delete(String),
    RollbackIfCurrent {
        expected: Vec<ProjectConfig>,
        rollback: Vec<ProjectConfig>,
    },
    BeginSave(String),
    FinishSave(String),
    BeginRemoteLogo(String),
    FinishRemoteLogo(String),
    PreparePrOverview(String),
    CompletePrOverview {
        alias: String,
        counts: Option<(u32, u32, u32)>,
    },
}

enum ProjectOutput {
    None,
    Started(bool),
    Mutation(Result<(), String>),
    RolledBack(bool),
    OverviewTasks(Vec<ProjectPrOverviewTask>),
}

type ProjectStore = PersistentStore<ProjectState, ProjectAction, ProjectOutput>;
type PersistenceFailure = dyn Fn(Vec<ProjectConfig>, Vec<ProjectConfig>, String) + Send + Sync;
type PersistenceSuccess = dyn Fn(&'static str) + Send + Sync;

#[derive(Clone)]
pub(super) struct ProjectStateFacade {
    store: ProjectStore,
    last_persisted: Arc<Mutex<Vec<ProjectConfig>>>,
}

static REGISTERED_STATE: OnceLock<ProjectStateFacade> = OnceLock::new();

pub(super) fn register(state: &ProjectStateFacade) {
    let _ = REGISTERED_STATE.set(state.clone());
}

pub(crate) fn load_repository_metadata(
    project_alias: &str,
) -> Result<ProjectRepositoryMetadata, String> {
    let alias = project_alias.trim();
    let state = REGISTERED_STATE
        .get()
        .ok_or_else(|| "Project state is not initialized.".to_string())?;
    let snapshot = state.snapshot();
    let project = snapshot
        .state()
        .projects
        .iter()
        .find(|project| project.project_alias == alias)
        .ok_or_else(|| format!("Project configuration for '{alias}' was not found."))?;
    if project.repo_project_id.trim().is_empty() || project.repo_type.trim().is_empty() {
        return Err(
            "Repository metadata is missing. Save the project again to synchronize it.".to_string(),
        );
    }
    Ok(ProjectRepositoryMetadata {
        repo_project_id: project.repo_project_id.clone(),
        repo_type: project.repo_type.clone(),
        remote_repo: project.remote_repo.clone(),
    })
}

impl ProjectStateFacade {
    pub(super) fn open_deferred(
        on_success: Arc<PersistenceSuccess>,
        on_failure: Arc<PersistenceFailure>,
    ) -> std::io::Result<Self> {
        let last_persisted = Arc::new(Mutex::new(Vec::new()));
        let persisted_for_write = last_persisted.clone();
        let persisted_for_failure = last_persisted.clone();
        let store = PersistentStore::new_deferred(
            ProjectState::default(),
            reduce_project_state,
            "project-state",
            move |state| {
                {
                    let mut persisted = persisted_for_write
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if registries_match_persisted(&persisted, &state.projects) {
                        *persisted = state.projects.clone();
                        return Ok(());
                    }
                }
                write_projects(&state.projects).map_err(|error| error.to_string())?;
                *persisted_for_write
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = state.projects.clone();
                if let Some(message) = state.persistence_feedback {
                    on_success(message);
                }
                Ok(())
            },
            move |failed, error| {
                let rollback = persisted_for_failure
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone();
                on_failure(failed.projects.clone(), rollback, error);
            },
        )?;
        Ok(Self {
            store,
            last_persisted,
        })
    }

    pub(super) fn hydrate(&self, projects: Vec<ProjectConfig>) {
        *self
            .last_persisted
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = projects.clone();
        self.store.dispatch(ProjectAction::Hydrate(projects));
        self.store.enable_persistence();
    }

    pub(super) fn snapshot(&self) -> StoreSnapshot<ProjectState> {
        self.store.snapshot()
    }

    pub(super) fn subscribe(
        &self,
        listener: Arc<dyn Fn(StoreSnapshot<ProjectState>) + Send + Sync>,
    ) -> Subscription {
        self.store.subscribe(listener)
    }

    pub(super) fn contains_alias(&self, alias: &str) -> bool {
        self.snapshot()
            .state()
            .projects
            .iter()
            .any(|project| project.project_alias == alias)
    }

    pub(super) fn begin_save(&self, key: String) -> bool {
        match self.store.dispatch(ProjectAction::BeginSave(key)).output {
            ProjectOutput::Started(started) => started,
            _ => false,
        }
    }

    pub(super) fn finish_save(&self, key: String) {
        self.store.dispatch(ProjectAction::FinishSave(key));
    }

    pub(super) fn add(&self, project: ProjectConfig) -> Result<(), String> {
        mutation_output(self.store.dispatch(ProjectAction::Add(project)).output)
    }

    pub(super) fn update(
        &self,
        original_alias: String,
        project: ProjectConfig,
    ) -> Result<(), String> {
        mutation_output(
            self.store
                .dispatch(ProjectAction::Update {
                    original_alias,
                    project,
                })
                .output,
        )
    }

    pub(super) fn delete(&self, alias: String) -> Result<(), String> {
        mutation_output(self.store.dispatch(ProjectAction::Delete(alias)).output)
    }

    pub(super) fn rollback_if_current(
        &self,
        expected: Vec<ProjectConfig>,
        rollback: Vec<ProjectConfig>,
    ) -> bool {
        match self
            .store
            .dispatch(ProjectAction::RollbackIfCurrent { expected, rollback })
            .output
        {
            ProjectOutput::RolledBack(rolled_back) => rolled_back,
            _ => false,
        }
    }

    pub(super) fn begin_remote_logo(&self, url: String) -> bool {
        match self
            .store
            .dispatch(ProjectAction::BeginRemoteLogo(url))
            .output
        {
            ProjectOutput::Started(started) => started,
            _ => false,
        }
    }

    pub(super) fn finish_remote_logo(&self, url: String) {
        self.store.dispatch(ProjectAction::FinishRemoteLogo(url));
    }

    pub(super) fn prepare_pr_overview(&self, query: String) -> Vec<ProjectPrOverviewTask> {
        match self
            .store
            .dispatch(ProjectAction::PreparePrOverview(query))
            .output
        {
            ProjectOutput::OverviewTasks(tasks) => tasks,
            _ => Vec::new(),
        }
    }

    pub(super) fn complete_pr_overview(&self, alias: String, counts: Option<(u32, u32, u32)>) {
        self.store
            .dispatch(ProjectAction::CompletePrOverview { alias, counts });
    }

    pub(super) fn pending_pr_overview_count(&self) -> usize {
        self.snapshot().state().pending_pr_overview.len()
    }

    pub(super) fn shutdown(&self) {
        self.store.shutdown();
    }
}

fn mutation_output(output: ProjectOutput) -> Result<(), String> {
    match output {
        ProjectOutput::Mutation(result) => result,
        _ => Err("Project registry action returned an incompatible result.".to_string()),
    }
}

fn reduce_project_state(
    current: &ProjectState,
    action: ProjectAction,
) -> Reduction<ProjectState, ProjectOutput> {
    let mut next = current.clone();
    match action {
        ProjectAction::Hydrate(projects) => {
            next.projects = projects;
            next.persistence_feedback = None;
            next.pending_remote_logos.clear();
            next.pending_pr_overview.clear();
            next.pending_saves.clear();
            Reduction::changed(next, ProjectOutput::None)
        }
        ProjectAction::Add(project) => {
            if next
                .projects
                .iter()
                .any(|item| item.project_alias == project.project_alias)
            {
                return Reduction::unchanged(
                    current.clone(),
                    ProjectOutput::Mutation(Err("Project alias already exists.".to_string())),
                );
            }
            next.projects.push(project);
            next.persistence_feedback = Some("Project added.");
            Reduction::changed(next, ProjectOutput::Mutation(Ok(())))
        }
        ProjectAction::Update {
            original_alias,
            project,
        } => {
            let Some(index) = next
                .projects
                .iter()
                .position(|item| item.project_alias == original_alias)
            else {
                return Reduction::unchanged(
                    current.clone(),
                    ProjectOutput::Mutation(Err("Project not found.".to_string())),
                );
            };
            if project.project_alias != original_alias
                && next
                    .projects
                    .iter()
                    .any(|item| item.project_alias == project.project_alias)
            {
                return Reduction::unchanged(
                    current.clone(),
                    ProjectOutput::Mutation(Err("Project alias already exists.".to_string())),
                );
            }
            next.projects[index] = project;
            next.persistence_feedback = Some("Project updated.");
            Reduction::changed(next, ProjectOutput::Mutation(Ok(())))
        }
        ProjectAction::Delete(alias) => {
            let Some(index) = next
                .projects
                .iter()
                .position(|item| item.project_alias == alias)
            else {
                return Reduction::unchanged(
                    current.clone(),
                    ProjectOutput::Mutation(Err("Project not found.".to_string())),
                );
            };
            next.projects.remove(index);
            next.persistence_feedback = Some("Project deleted.");
            Reduction::changed(next, ProjectOutput::Mutation(Ok(())))
        }
        ProjectAction::RollbackIfCurrent { expected, rollback } => {
            if !registries_match_persisted(&next.projects, &expected) {
                return Reduction::unchanged(current.clone(), ProjectOutput::RolledBack(false));
            }
            next.projects = rollback;
            next.persistence_feedback = None;
            Reduction::changed(next, ProjectOutput::RolledBack(true))
        }
        ProjectAction::BeginSave(key) => {
            let started = next.pending_saves.insert(key);
            if started {
                Reduction::changed(next, ProjectOutput::Started(true))
            } else {
                Reduction::unchanged(current.clone(), ProjectOutput::Started(false))
            }
        }
        ProjectAction::FinishSave(key) => {
            changed_set_removal(current, next.pending_saves.remove(&key), next)
        }
        ProjectAction::BeginRemoteLogo(url) => {
            let started = next.pending_remote_logos.insert(url);
            if started {
                Reduction::changed(next, ProjectOutput::Started(true))
            } else {
                Reduction::unchanged(current.clone(), ProjectOutput::Started(false))
            }
        }
        ProjectAction::FinishRemoteLogo(url) => {
            changed_set_removal(current, next.pending_remote_logos.remove(&url), next)
        }
        ProjectAction::PreparePrOverview(query) => {
            let tasks = collect_pr_overview_tasks(&mut next.projects, &query)
                .into_iter()
                .filter(|task| next.pending_pr_overview.insert(task.project_alias.clone()))
                .collect::<Vec<_>>();
            if tasks.is_empty() {
                Reduction::unchanged(current.clone(), ProjectOutput::OverviewTasks(tasks))
            } else {
                Reduction::changed(next, ProjectOutput::OverviewTasks(tasks))
            }
        }
        ProjectAction::CompletePrOverview { alias, counts } => {
            next.pending_pr_overview.remove(&alias);
            let Some(project) = next
                .projects
                .iter_mut()
                .find(|project| project.project_alias == alias)
            else {
                return Reduction::changed(next, ProjectOutput::None);
            };
            project.pr_overview.loading = false;
            match counts {
                Some((fat, uat, prod)) => {
                    project.pr_overview.failed = false;
                    project.pr_overview.fat_display = fat.to_string();
                    project.pr_overview.uat_display = uat.to_string();
                    project.pr_overview.prod_display = prod.to_string();
                }
                None => {
                    project.pr_overview = ProjectPrOverview {
                        failed: true,
                        ..ProjectPrOverview::default()
                    };
                }
            }
            Reduction::changed(next, ProjectOutput::None)
        }
    }
}

fn changed_set_removal(
    current: &ProjectState,
    changed: bool,
    next: ProjectState,
) -> Reduction<ProjectState, ProjectOutput> {
    if changed {
        Reduction::changed(next, ProjectOutput::None)
    } else {
        Reduction::unchanged(current.clone(), ProjectOutput::None)
    }
}

pub(super) fn registries_match_persisted(left: &[ProjectConfig], right: &[ProjectConfig]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.project_name == right.project_name
                && left.project_alias == right.project_alias
                && left.local_repo == right.local_repo
                && left.remote_repo == right.remote_repo
                && left.repo_project_id == right.repo_project_id
                && left.repo_type == right.repo_type
                && left.logo == right.logo
                && left.project_tags == right.project_tags
                && left.is_pr_auto_update == right.is_pr_auto_update
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(alias: &str) -> ProjectConfig {
        ProjectConfig {
            project_name: alias.to_string(),
            project_alias: alias.to_string(),
            local_repo: format!("/{alias}"),
            remote_repo: format!("https://example.test/{alias}.git"),
            repo_project_id: alias.to_string(),
            repo_type: "github".to_string(),
            logo: String::new(),
            project_tags: Vec::new(),
            is_pr_auto_update: false,
            pr_overview: ProjectPrOverview::default(),
        }
    }

    #[test]
    fn typed_registry_actions_reject_duplicates_and_rollback_only_current_state() {
        let initial = ProjectState {
            projects: vec![project("alpha")],
            ..ProjectState::default()
        };
        let added = reduce_project_state(&initial, ProjectAction::Add(project("beta")));
        assert!(added.changed);
        assert_eq!(added.state.persistence_feedback, Some("Project added."));
        let ProjectOutput::Mutation(Ok(())) = added.output else {
            panic!("expected successful mutation");
        };
        let duplicate = reduce_project_state(&added.state, ProjectAction::Add(project("beta")));
        assert!(!duplicate.changed);

        let rolled_back = reduce_project_state(
            &added.state,
            ProjectAction::RollbackIfCurrent {
                expected: added.state.projects.clone(),
                rollback: initial.projects.clone(),
            },
        );
        assert!(rolled_back.changed);
        assert_eq!(rolled_back.state.projects, initial.projects);
        assert_eq!(rolled_back.state.persistence_feedback, None);
    }
}
