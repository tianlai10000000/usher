mod bindings;
pub mod branch;
pub mod cd;
pub mod pr;
pub mod state;
pub mod tag;

use crate::MainWindow;

pub fn setup_workflow(ui: &MainWindow) {
    log::info!("[Workflow] Initializing workflow module");
    let shared_state = state::new_shared_state();
    state::initialize_current_ui(ui, &shared_state);
    bindings::bind_project_context(ui, shared_state.clone());
    branch::bind_branch(ui, shared_state.clone());
    tag::bind_tag(ui, shared_state.clone());
    tag::refresh_tag_context(ui, &shared_state, false);
    pr::bind_pr(ui, shared_state.clone());
    cd::bind_cd(ui, shared_state);
    log::info!("[Workflow] Workflow module initialized successfully");
}
