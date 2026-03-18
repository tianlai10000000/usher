use crate::workflow::state::{
    SharedWorkflowState, WorkflowProjectContext, initialize_ui, normalize_project_alias,
    normalize_project_id, normalize_project_name,
};
use crate::workflow::tag::refresh_tag_context;
use crate::{MainWindow, ToastManager, ToastOptions};
use slint::{ComponentHandle, Global};

pub(crate) fn bind_project_context(ui: &MainWindow, shared_state: SharedWorkflowState) {
    let ui_weak = ui.as_weak();
    ui.on_workflow_open_project(
        move |project_name, project_alias, local_repo, remote_repo, logo, logo_image| {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let context = WorkflowProjectContext {
                id: normalize_project_id(&project_alias, &project_name),
                name: normalize_project_name(&project_name),
                alias: normalize_project_alias(&project_alias),
                local_repo: local_repo.to_string(),
                remote_repo: remote_repo.to_string(),
                logo: logo.to_string(),
            };
            let snapshot = {
                let Ok(mut guard) = shared_state.lock() else {
                    return;
                };
                guard.cd_running = false;
                guard.set_project(context.clone())
            };
            initialize_ui(&ui, &context, &snapshot);
            ui.set_workflow_project_logo_image(logo_image);
            refresh_tag_context(&ui, &shared_state, false);
            show_success(
                &ui,
                &format!("Switched workflow context to '{}'.", context.name),
            );
        },
    );
}

pub(crate) fn show_success(ui: &MainWindow, message: &str) {
    let manager = ToastManager::get(ui);
    manager.invoke_success(ToastOptions {
        message: message.into(),
        duration: 2200,
        position: "bottom".into(),
        icon: "".into(),
        overlay: false,
        close_on_click: false,
        loading_type: "".into(),
        width: "auto".into(),
    });
}

pub(crate) fn show_error(ui: &MainWindow, message: &str) {
    let manager = ToastManager::get(ui);
    manager.invoke_error(ToastOptions {
        message: message.into(),
        duration: 2600,
        position: "bottom".into(),
        icon: "".into(),
        overlay: false,
        close_on_click: false,
        loading_type: "".into(),
        width: "auto".into(),
    });
}
