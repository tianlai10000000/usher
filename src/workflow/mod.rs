mod bindings;
pub mod branch;
pub mod cd;
mod pr_conflict_status;
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
    pr_conflict_status::bind(ui);
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

pub(crate) fn sync_language(ui: &MainWindow) {
    bindings::sync_open_directory_tooltip(ui);
    state::reproject_registered_ui(ui);
    LIFECYCLE_CONTROLLER.with(|registered| {
        if let Some(controller) = registered.borrow().as_ref() {
            controller.pr_list.sync_language(ui);
            controller.pr_review.sync_language(ui);
        }
    });
}

pub(crate) fn invalidate_conflict_status(ui: &MainWindow) {
    pr_conflict_status::invalidate(ui);
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

pub(crate) fn pr_review_route_changed(
    ui: &MainWindow,
    previous: crate::Route,
    current: crate::Route,
) {
    let controller = LIFECYCLE_CONTROLLER.with(|registered| registered.borrow().clone());
    if let Some(controller) = controller {
        controller.pr_review.route_changed(ui, previous, current);
    }
}
