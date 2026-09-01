use crate::git::get_standard_environment_pull_request_counts;
use crate::platform::{open_directory_in_file_manager, open_directory_tooltip_key};
use crate::project::load_repository_metadata;
use crate::terminal::{reset_terminal_for_project, terminal_title_for_project};
use crate::workflow::pr_list::WorkflowPrListController;
use crate::workflow::pr_review::WorkflowPrReviewController;
use crate::workflow::state::{
    SharedWorkflowState, WorkflowProjectContext, initialize_ui, normalize_project_alias,
    normalize_project_id, normalize_project_name,
};
use crate::workflow::tag::refresh_tag_context;
use crate::{MainWindow, ToastManager, ToastOptions};
use slint::{ComponentHandle, Global};
use std::path::Path;

pub(crate) fn bind_project_context(
    ui: &MainWindow,
    shared_state: SharedWorkflowState,
    pr_list_controller: WorkflowPrListController,
    pr_review_controller: WorkflowPrReviewController,
) {
    sync_open_directory_tooltip(ui);

    let ui_weak = ui.as_weak();
    ui.on_workflow_open_project(
        move |project_name,
              project_alias,
              local_repo,
              remote_repo,
              logo,
              logo_image,
              is_pr_auto_update| {
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
            let previous_project_id = ui.get_workflow_project_id().to_string();
            let snapshot = {
                let Ok(mut guard) = shared_state.lock() else {
                    return;
                };
                guard.cd_running = false;
                guard.upsert_current(|view| {
                    view.tag_add_request_seq = view.tag_add_request_seq.saturating_add(1);
                    view.tag_remove_request_seq = view.tag_remove_request_seq.saturating_add(1);
                    view.tag_add_initializing = false;
                    view.tag_remove_initializing = false;
                });
                guard.set_project(context.clone())
            };
            initialize_ui(&ui, &context, &snapshot);
            ui.set_shell_drawer_title(terminal_title_for_project(&context.name).into());
            reset_terminal_for_project(
                &ui,
                &context.id,
                &context.local_repo,
                previous_project_id == context.id,
            );
            ui.set_workflow_project_logo_image(logo_image);
            ui.set_workflow_pr_count_auto_update(is_pr_auto_update);
            ui.set_workflow_pr_count_loading(is_pr_auto_update);
            ui.set_workflow_pr_count_failed(false);
            ui.set_workflow_pr_fat_count(if is_pr_auto_update {
                "-".into()
            } else {
                "".into()
            });
            ui.set_workflow_pr_uat_count(if is_pr_auto_update {
                "-".into()
            } else {
                "".into()
            });
            ui.set_workflow_pr_prod_count(if is_pr_auto_update {
                "-".into()
            } else {
                "".into()
            });
            refresh_tag_context(&ui, &shared_state, false);
            pr_list_controller.select_project(&ui, context.alias.clone());
            pr_review_controller.select_project(&ui, context.alias.clone());
            if is_pr_auto_update {
                let ui_weak_task = ui.as_weak();
                let project_id = context.id.clone();
                let project_alias = context.alias.clone();
                crate::app_runtime::shared_runtime().spawn(async move {
                    let metadata = tokio::task::spawn_blocking(move || {
                        load_repository_metadata(project_alias.as_str())
                    })
                    .await
                    .map_err(|error| format!("Failed to read project configuration: {error}"))
                    .and_then(|result| result);
                    let result = match metadata {
                        Ok(metadata) => {
                            get_standard_environment_pull_request_counts(
                                metadata.repo_type.as_str(),
                                metadata.repo_project_id.as_str(),
                                metadata.remote_repo.as_str(),
                            )
                            .await
                        }
                        Err(error) => Err(crate::git::GitError::configuration(error)),
                    };
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak_task.upgrade() else {
                            return;
                        };
                        if ui.get_workflow_project_id() != project_id {
                            return;
                        }
                        ui.set_workflow_pr_count_loading(false);
                        match result {
                            Ok(counts) => {
                                ui.set_workflow_pr_count_failed(false);
                                ui.set_workflow_pr_fat_count(counts.fat.to_string().into());
                                ui.set_workflow_pr_uat_count(counts.uat.to_string().into());
                                ui.set_workflow_pr_prod_count(counts.prod.to_string().into());
                            }
                            Err(error) => {
                                log::warn!(
                                    "[Workflow] Failed to fetch PR counts for '{}': {}",
                                    project_id,
                                    error
                                );
                                ui.set_workflow_pr_count_failed(true);
                                ui.set_workflow_pr_fat_count("unknown".into());
                                ui.set_workflow_pr_uat_count("unknown".into());
                                ui.set_workflow_pr_prod_count("unknown".into());
                            }
                        }
                    });
                });
            }
        },
    );

    let ui_weak = ui.as_weak();
    ui.on_workflow_copy_remote_repo(move |remote| {
        let remote = remote.trim();
        if remote.is_empty() {
            return false;
        }
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(remote)) {
            Ok(()) => true,
            Err(error) => {
                log::warn!("[Workflow] Failed to copy remote URL: {error}");
                if let Some(ui) = ui_weak.upgrade() {
                    show_error(&ui, crate::i18n::tr("workflow.copy_remote_failed").as_ref());
                }
                false
            }
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_workflow_open_local_repo(move |local_repo| {
        let local_repo = local_repo.trim();
        if local_repo.is_empty() {
            return;
        }
        match open_directory_in_file_manager(Path::new(local_repo)) {
            Ok(()) => {}
            Err(error) => {
                log::warn!("[Workflow] Failed to open local repository: {error}");
                if let Some(ui) = ui_weak.upgrade() {
                    show_error(
                        &ui,
                        crate::i18n::tr("workflow.open_local_repository_failed").as_ref(),
                    );
                }
            }
        }
    });
}

pub(crate) fn sync_open_directory_tooltip(ui: &MainWindow) {
    ui.set_workflow_open_local_folder_tooltip(
        crate::i18n::tr(open_directory_tooltip_key())
            .into_owned()
            .into(),
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
