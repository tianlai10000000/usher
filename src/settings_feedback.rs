//! Worker-backed Settings save feedback and transient saved indicators.
use crate::{MainWindow, SettingsPageBridge, ToastManager, ToastOptions, app_runtime, i18n};
use slint::{ComponentHandle, Global, Timer};

#[derive(Clone, Copy)]
pub(crate) enum SettingsSaveTarget {
    GitTokens,
    GitHosting,
    AiAgent,
}

fn set_settings_saved(
    ui: &MainWindow,
    page: &SettingsPageBridge,
    target: SettingsSaveTarget,
    saved: bool,
) {
    match target {
        SettingsSaveTarget::GitTokens => {
            ui.set_settings_git_tokens_saved(saved);
            page.set_git_tokens_saved(saved);
        }
        SettingsSaveTarget::GitHosting => {
            ui.set_settings_git_hosting_config_saved(saved);
            page.set_git_hosting_config_saved(saved);
        }
        SettingsSaveTarget::AiAgent => {
            ui.set_settings_ai_agent_saved(saved);
            page.set_ai_agent_saved(saved);
        }
    }
}

pub(crate) fn settings_toast(message: &str) -> ToastOptions {
    ToastOptions {
        message: message.into(),
        duration: 3_000,
        position: "bottom".into(),
        icon: "".into(),
        overlay: false,
        close_on_click: true,
        loading_type: "".into(),
        width: "auto".into(),
    }
}

pub(crate) fn save_settings_with_feedback<F>(
    ui_weak: slint::Weak<MainWindow>,
    settings_page: SettingsPageBridge,
    target: SettingsSaveTarget,
    operation: &'static str,
    success_message_key: &'static str,
    failure_message_key: &'static str,
    save: F,
) where
    F: FnOnce() -> std::result::Result<(), String> + Send + 'static,
{
    app_runtime::shared_runtime().spawn(async move {
        let result = tokio::task::spawn_blocking(save)
            .await
            .unwrap_or_else(|error| Err(format!("settings worker failed: {error}")));
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            match result {
                Ok(()) => {
                    if matches!(
                        target,
                        SettingsSaveTarget::GitTokens | SettingsSaveTarget::GitHosting
                    ) {
                        crate::workflow::invalidate_conflict_status(&ui);
                    }
                    set_settings_saved(&ui, &settings_page, target, true);
                    ToastManager::get(&ui)
                        .invoke_success(settings_toast(i18n::tr(success_message_key).as_ref()));
                    let reset_weak = ui.as_weak();
                    let reset_settings_page = settings_page.clone();
                    Timer::single_shot(std::time::Duration::from_secs(2), move || {
                        if let Some(ui) = reset_weak.upgrade() {
                            set_settings_saved(&ui, &reset_settings_page, target, false);
                        }
                    });
                    log::info!("[Settings] {operation} saved");
                }
                Err(error) => {
                    set_settings_saved(&ui, &settings_page, target, false);
                    ToastManager::get(&ui)
                        .invoke_error(settings_toast(i18n::tr(failure_message_key).as_ref()));
                    log::error!("[Settings] Failed to save {operation}: {error}");
                }
            }
        });
    });
}
