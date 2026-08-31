mod bindings;
pub mod branch;
pub mod cd;
mod pr_list;
#[cfg(feature = "test-fixtures")]
mod pr_list_fixture;
mod pr_list_state;
mod pr_review;
pub mod state;
mod syntax_highlight;
pub mod tag;

use crate::MainWindow;
use std::cell::RefCell;

#[derive(Clone)]
struct WorkflowLifecycleController {
    pr_list: pr_list::WorkflowPrListController,
    pr_review: pr_review::WorkflowPrReviewController,
}

thread_local! {
    static LIFECYCLE_CONTROLLER: RefCell<Option<WorkflowLifecycleController>> = const { RefCell::new(None) };
}

pub fn setup_workflow(ui: &MainWindow) {
    log::info!("[Workflow] Initializing workflow module");
    let shared_state = state::new_shared_state();
    state::register_shared_state(&shared_state);
    state::initialize_current_ui(ui, &shared_state);
    let pr_list_controller = pr_list::bind_pr_list(ui);
    let pr_review_controller = pr_review::bind_pr_review(ui, pr_list_controller.clone());
    LIFECYCLE_CONTROLLER.with(|registered| {
        *registered.borrow_mut() = Some(WorkflowLifecycleController {
            pr_list: pr_list_controller.clone(),
            pr_review: pr_review_controller.clone(),
        });
    });
    pr_review_controller.install_memory_benchmark_fixture(ui);
    bindings::bind_project_context(
        ui,
        shared_state.clone(),
        pr_list_controller,
        pr_review_controller,
    );
    branch::bind_branch(ui, shared_state.clone());
    tag::bind_tag(ui, shared_state.clone());
    tag::refresh_tag_context(ui, &shared_state, false);
    cd::bind_cd(ui, shared_state);
    log::info!("[Workflow] Workflow module initialized successfully");
}

pub(crate) fn release_unreachable_state(ui: &MainWindow) {
    let mode = std::env::var("USHER_MEMORY_WORKFLOW_LIFECYCLE").unwrap_or_default();
    if mode != "clear" {
        if mode == "retain" {
            log::info!("[Workflow][PR Review][Memory] reason=workflow_unreachable.retained_for_ab");
            crate::memory_diagnostics::log_checkpoint("workflow_unreachable.retained_for_ab");
        }
        return;
    }

    LIFECYCLE_CONTROLLER.with(|registered| {
        let Some(controller) = registered.borrow().as_ref().cloned() else {
            return;
        };
        controller.pr_review.release_unreachable(ui);
        controller.pr_list.release_unreachable(ui);
    });
}
