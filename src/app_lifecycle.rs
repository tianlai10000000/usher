//! Cross-feature cleanup triggered by completed route transitions.

use std::rc::Rc;

use crate::router::{RouteLifecycleEvent, RouteLifecycleHooks, is_workflow_route};
use crate::{MainWindow, Route};

pub(crate) fn hooks(initial_route: Route) -> RouteLifecycleHooks {
    let hooks = RouteLifecycleHooks::default();
    hooks.on_transition(Rc::new(|window, event| {
        window.invoke_chat_route_visible_changed(event.current == Route::Chat);
        if event.previous == Route::JobRun && event.current != Route::JobRun {
            crate::hooks::cancel_replay();
        }
    }));
    hooks.on_scope_unreachable(
        "workflow",
        initial_route,
        Rc::new(is_workflow_route),
        Rc::new(release_workflow),
    );
    hooks
}

fn release_workflow(window: &MainWindow, event: RouteLifecycleEvent) {
    log::debug!(
        "[Lifecycle] Releasing Workflow state after {:?} -> {:?}",
        event.previous,
        event.current
    );
    crate::workflow::release_unreachable_state(window);
    crate::workflow::state::clear_current_project_heavy_state();
    window.set_workflow_project_logo("".into());
    window.set_workflow_project_logo_image(crate::project::resolve_project_logo_image(""));
    window.set_workflow_pr_count_auto_update(false);
    window.set_workflow_pr_count_loading(false);
    window.set_workflow_pr_count_failed(false);
    window.set_workflow_pr_fat_count("".into());
    window.set_workflow_pr_uat_count("".into());
    window.set_workflow_pr_prod_count("".into());
    window.set_workflow_cd_summary("".into());
    window.set_workflow_cd_build_detail("".into());
    window.set_workflow_cd_test_detail("".into());
    window.set_workflow_cd_deploy_detail("".into());
    window.set_workflow_cd_verify_detail("".into());
}
