use crate::MainWindow;
use crate::i18n::LocalizedText;
use crate::workflow::bindings::show_error;
use crate::workflow::state::SharedWorkflowState;
use slint::ComponentHandle;

#[derive(Debug, Clone)]
pub(crate) struct BranchRequest {
    pub(crate) source: String,
    pub(crate) target: String,
    pub(crate) recreate_remote: bool,
}

pub(crate) fn recreate_branch_plan(req: BranchRequest) -> Result<LocalizedText, LocalizedText> {
    let source = req.source.trim();
    let target = req.target.trim();
    if source.is_empty() || target.is_empty() {
        return Err(LocalizedText::message("branch.source_target_required"));
    }
    if source == target {
        return Err(LocalizedText::message("branch.source_target_different"));
    }

    let key = if req.recreate_remote {
        "branch.recreate_plan_with_remote"
    } else {
        "branch.recreate_plan_without_remote"
    };
    Ok(LocalizedText::message_with(
        key,
        [
            ("source", source.to_string()),
            ("target", target.to_string()),
        ],
    ))
}

pub(crate) fn bind_branch(ui: &MainWindow, shared_state: SharedWorkflowState) {
    let ui_weak = ui.as_weak();
    ui.on_workflow_branch_submit(move |source, target, recreate_remote| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        if let Err(error) = shared_state.submit(crate::workflow::state::WorkflowAction::Branch(
            BranchRequest {
                source: source.to_string(),
                target: target.to_string(),
                recreate_remote,
            },
        )) {
            log::warn!("[Workflow][Branch] Admission rejected: {error}");
            show_error(&ui, crate::i18n::tr("workflow.command_rejected").as_ref());
        }
    });
}
