use crate::MainWindow;
use crate::workflow::bindings::{show_error, show_success};
use crate::workflow::state::SharedWorkflowState;
use slint::ComponentHandle;

#[derive(Debug, Clone)]
pub(crate) struct BranchRequest {
    pub(crate) source: String,
    pub(crate) target: String,
    pub(crate) recreate_remote: bool,
}

pub(crate) fn recreate_branch_plan(req: BranchRequest) -> Result<String, String> {
    let source = req.source.trim();
    let target = req.target.trim();
    if source.is_empty() || target.is_empty() {
        return Err("Source branch and target branch are required.".to_string());
    }
    if source == target {
        return Err("Source branch and target branch must be different.".to_string());
    }

    let remote_clause = if req.recreate_remote {
        "Remote tracking branch will be recreated after local validation."
    } else {
        "Remote tracking branch recreation is disabled."
    };

    Ok(format!(
        "Validated branch recreation: use '{source}' as the clean source, archive or delete '{target}', then recreate '{target}' from '{source}'. {remote_clause}"
    ))
}

pub(crate) fn bind_branch(ui: &MainWindow, shared_state: SharedWorkflowState) {
    let ui_weak = ui.as_weak();
    ui.on_workflow_branch_submit(move |source, target, recreate_remote| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        match recreate_branch_plan(BranchRequest {
            source: source.to_string(),
            target: target.to_string(),
            recreate_remote,
        }) {
            Ok(result) => {
                if let Ok(mut guard) = shared_state.lock() {
                    guard.upsert_current(|view| view.branch_result = result.clone());
                }
                ui.set_workflow_branch_result(result.into());
                show_success(&ui, "Branch workflow validated.");
            }
            Err(error) => {
                if let Ok(mut guard) = shared_state.lock() {
                    guard.upsert_current(|view| view.branch_result = error.clone());
                }
                ui.set_workflow_branch_result(error.clone().into());
                show_error(&ui, &error);
            }
        }
    });
}
