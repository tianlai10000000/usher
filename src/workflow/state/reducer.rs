//! Pure project-scoped Workflow Actions and side-effect descriptions.
use super::{WorkflowIdentity, WorkflowProjectContext, WorkflowStore};
use crate::i18n::LocalizedText;
use crate::workflow::tag::state::{TagAction, TagEffect};
use crate::workflow::{
    branch::{BranchRequest, recreate_branch_plan},
    cd::{
        CdRequest, CdSnapshot, DeploymentStage, RollbackStage, deployment_snapshot,
        rollback_snapshot,
    },
};
use usher_state_store::Reduction;

#[derive(Debug, Clone)]
pub(crate) enum CdCommand {
    Deploy(CdRequest),
    Rollback(String),
}
pub(crate) enum WorkflowAction {
    SelectProject((WorkflowProjectContext, WorkflowIdentity)),
    Branch(BranchRequest),
    Cd(CdCommand),
    CdProgress {
        generation: u64,
        snapshot: CdSnapshot,
        completion: Option<LocalizedText>,
    },
    Tag(TagAction),
    ClearHeavy,
}
pub(super) enum WorkflowEffect {
    OpenProject,
    CdCompleted {
        title: LocalizedText,
        summary: LocalizedText,
    },
    Tag(TagEffect),
    Cd((CdCommand, u64)),
    Toast(bool, LocalizedText),
}
#[derive(Default)]
pub(super) struct WorkflowOutput {
    pub(super) effects: Vec<WorkflowEffect>,
}
pub(super) fn create_store()
-> std::sync::Arc<dyn usher_state_store::StateStore<WorkflowStore, WorkflowAction, WorkflowOutput>>
{
    std::sync::Arc::new(usher_state_store::LocalStateStore::new(
        WorkflowStore::default(),
        reduce,
    ))
}
pub(super) fn reduce(
    current: &WorkflowStore,
    action: WorkflowAction,
) -> Reduction<WorkflowStore, WorkflowOutput> {
    let mut next = current.clone();
    let mut output = WorkflowOutput::default();
    match action {
        WorkflowAction::SelectProject((context, identity)) => {
            next.current_project = context;
            next.identity = identity;
            next.cd_generation = next.cd_generation.wrapping_add(1);
            let id = next.current_project.id.clone();
            next.projects
                .retain(|key, _| key == "default" || *key == id);
            let view = next.projects.entry(id).or_default();
            view.cd.running = false;
            view.tag_running = false;
            view.tag_add_initializing = false;
            view.tag_remove_initializing = false;
            view.tag_add_request_seq = view.tag_add_request_seq.wrapping_add(1);
            view.tag_remove_request_seq = view.tag_remove_request_seq.wrapping_add(1);
            output
                .effects
                .push(WorkflowEffect::Tag(TagEffect::CancelAll));
            output.effects.push(WorkflowEffect::OpenProject);
        }
        WorkflowAction::Branch(request) => {
            let result = recreate_branch_plan(request);
            let (text, success) = match result {
                Ok(text) => (text, true),
                Err(text) => (text, false),
            };
            next.projects
                .entry(next.current_project.id.clone())
                .or_default()
                .branch_result = text.clone();
            output.effects.push(WorkflowEffect::Toast(
                success,
                if success {
                    LocalizedText::message("branch.validated")
                } else {
                    text
                },
            ));
        }
        WorkflowAction::Cd(command) => {
            let view = next
                .projects
                .entry(next.current_project.id.clone())
                .or_default();
            if view.cd.running {
                output.effects.push(WorkflowEffect::Toast(
                    false,
                    LocalizedText::message("cd.already_running"),
                ));
                return Reduction::unchanged(next, output);
            }
            let error = match &command {
                CdCommand::Deploy(request) => {
                    crate::workflow::cd::validate_cd_request(request).err()
                }
                CdCommand::Rollback(commit) if commit.trim().is_empty() => {
                    Some(LocalizedText::message("cd.rollback_required"))
                }
                _ => None,
            };
            if let Some(error) = error {
                view.cd.summary = error;
                output
                    .effects
                    .push(WorkflowEffect::Toast(false, view.cd.summary.clone()));
            } else {
                view.cd = match &command {
                    CdCommand::Deploy(request) => {
                        deployment_snapshot(request, DeploymentStage::Build)
                    }
                    CdCommand::Rollback(commit) => rollback_snapshot(commit, RollbackStage::Deploy),
                };
                next.cd_generation = next.cd_generation.wrapping_add(1);
                output.effects.push(WorkflowEffect::Toast(
                    true,
                    LocalizedText::message(match &command {
                        CdCommand::Deploy(_) => "cd.started",
                        CdCommand::Rollback(_) => "cd.rollback_started",
                    }),
                ));
                output
                    .effects
                    .push(WorkflowEffect::Cd((command, next.cd_generation)));
            }
        }
        WorkflowAction::CdProgress {
            generation,
            snapshot,
            completion,
        } => {
            if generation != next.cd_generation {
                return Reduction::unchanged(next, output);
            }
            if let Some(title) = completion {
                output.effects.push(WorkflowEffect::CdCompleted {
                    title,
                    summary: snapshot.summary.clone(),
                });
            }
            next.projects
                .entry(next.current_project.id.clone())
                .or_default()
                .cd = snapshot;
        }
        WorkflowAction::Tag(action) => {
            let context = next.current_project.clone();
            let view = next.projects.entry(context.id.clone()).or_default();
            let effects = crate::workflow::tag::state::reduce(view, &context, action);
            output
                .effects
                .extend(effects.into_iter().map(WorkflowEffect::Tag));
        }
        WorkflowAction::ClearHeavy => super::clear_heavy_view_state(
            next.projects
                .entry(next.current_project.id.clone())
                .or_default(),
        ),
    }
    if next == *current {
        Reduction::unchanged(next, output)
    } else {
        Reduction::changed(next, output)
    }
}
