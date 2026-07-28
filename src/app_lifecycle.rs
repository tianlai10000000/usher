//! Cross-feature cleanup triggered by completed route transitions.

use slint::{ModelRc, VecModel};

use crate::MainWindow;
use crate::router::RouteLifecycleEvent;

pub(crate) fn handle_route_transition(window: &MainWindow, event: RouteLifecycleEvent) {
    if !event.workflow_became_unreachable {
        return;
    }

    log::debug!(
        "[Lifecycle] Releasing Workflow state after {:?} -> {:?}",
        event.previous,
        event.current
    );
    crate::workflow::state::clear_current_project_heavy_state();
    window.set_workflow_project_logo("".into());
    window.set_workflow_project_logo_image(crate::project::resolve_project_logo_image(""));
    window.set_workflow_pr_count_auto_update(false);
    window.set_workflow_pr_count_loading(false);
    window.set_workflow_pr_count_failed(false);
    window.set_workflow_pr_fat_count("".into());
    window.set_workflow_pr_uat_count("".into());
    window.set_workflow_pr_prod_count("".into());
    window.set_workflow_tag_result("".into());
    window.set_workflow_tag_steps(ModelRc::new(VecModel::from(Vec::new())));
    window.set_workflow_cd_summary("".into());
    window.set_workflow_cd_build_detail("".into());
    window.set_workflow_cd_test_detail("".into());
    window.set_workflow_cd_deploy_detail("".into());
    window.set_workflow_cd_verify_detail("".into());
}
