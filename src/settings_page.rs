//! Bridge between the root window and the independently generated Settings page.
//!
//! Slint component factories create a separate global-state graph for their
//! product. This module deliberately mirrors the small Settings contract and
//! forwards callbacks on the Slint event loop instead of pretending those
//! globals are shared across generated crates.

use std::sync::{Arc, Mutex};

use slint::{ComponentFactory, ComponentHandle, Global};
use usher_ui_settings::{AppState as SettingsAppState, GitHubSettingsPage};

use crate::{AppState, MainWindow, UpdateLogic};

/// Dual-write a host-owned update-checking busy flag onto the isolated Settings
/// page. Must run on the Slint event loop. Do not rely on Slint `changed` alone:
/// already-equal assignments such as `false→false` do not propagate.
pub(crate) fn mirror_update_checking(ui: &MainWindow, page: &SettingsPageBridge, checking: bool) {
    ui.global::<UpdateLogic>().set_checking_update(checking);
    page.set_update_checking(checking);
}

/// Dual-write the AI Agent connection-test busy flag. Event-loop only.
pub(crate) fn mirror_ai_agent_testing(ui: &MainWindow, page: &SettingsPageBridge, testing: bool) {
    ui.set_settings_ai_agent_testing(testing);
    page.set_ai_agent_testing(testing);
}

#[derive(Clone, Default)]
pub(crate) struct SettingsPageBridge {
    page: Arc<Mutex<Option<slint::Weak<GitHubSettingsPage>>>>,
}

impl SettingsPageBridge {
    pub(crate) fn install(&self, window: &MainWindow) {
        let page_slot = self.page.clone();
        let window_weak = window.as_weak();

        window.set_settings_page_factory(ComponentFactory::new(move |_| {
            let window = window_weak.upgrade()?;
            let page = match GitHubSettingsPage::new() {
                Ok(page) => page,
                Err(error) => {
                    log::error!("[Settings] Failed to create isolated page: {error}");
                    return None;
                }
            };

            sync_state_from_window(&window, &page);
            bind_callbacks(&window, &page);
            *page_slot.lock().expect("settings page bridge poisoned") = Some(page.as_weak());
            Some(page)
        }));

        let bridge = self.clone();
        window.on_settings_update_checking_changed(move |checking| {
            bridge.with_page(|page| page.set_update_checking(checking));
        });

        let bridge = self.clone();
        window.on_settings_page_viewport_changed(move |width, height| {
            bridge.with_page(|page| {
                page.set_viewport_width(width);
                page.set_viewport_height(height);
            });
        });
    }

    pub(crate) fn set_git_tokens_saved(&self, saved: bool) {
        self.with_page(|page| page.set_git_tokens_saved(saved));
    }

    pub(crate) fn set_git_hosting_config_saved(&self, saved: bool) {
        self.with_page(|page| page.set_git_hosting_config_saved(saved));
    }

    pub(crate) fn set_ai_agent_saved(&self, saved: bool) {
        self.with_page(|page| page.set_ai_agent_saved(saved));
    }

    pub(crate) fn set_ai_agent_testing(&self, testing: bool) {
        self.with_page(|page| page.set_ai_agent_testing(testing));
    }

    pub(crate) fn set_update_checking(&self, checking: bool) {
        self.with_page(|page| page.set_update_checking(checking));
    }

    pub(crate) fn set_ai_agent_provider_index(&self, index: i32) {
        self.with_page(|page| page.set_ai_agent_provider_index(index));
    }

    pub(crate) fn sync_theme(&self, mode: i32, effective_dark_mode: bool) {
        self.with_page(|page| {
            let state = SettingsAppState::get(page);
            state.set_theme_mode(mode);
            state.set_effective_dark_mode(effective_dark_mode);
        });
    }

    fn with_page(&self, update: impl FnOnce(&GitHubSettingsPage)) {
        let weak = self
            .page
            .lock()
            .expect("settings page bridge poisoned")
            .clone();
        if let Some(page) = weak.and_then(|page| page.upgrade()) {
            update(&page);
        }
    }
}

fn sync_state_from_window(window: &MainWindow, page: &GitHubSettingsPage) {
    let source = AppState::get(window);
    let target = SettingsAppState::get(page);

    target.set_theme_mode(source.get_theme_mode());
    target.set_effective_dark_mode(source.get_effective_dark_mode());
    target.set_version(source.get_version());
    target.set_auto_update(source.get_auto_update());
    target.set_tracing_logger_active(source.get_tracing_logger_active());
    target.set_terminal_verbose_logging(source.get_terminal_verbose_logging());
    target.set_ui_sound_effects(source.get_ui_sound_effects());
    target.set_git_github_token(source.get_git_github_token());
    target.set_git_gitlab_token(source.get_git_gitlab_token());
    target.set_git_gitee_token(source.get_git_gitee_token());
    target.set_git_gitee_enterprise_token(source.get_git_gitee_enterprise_token());
    target.set_git_github_api_base_url(source.get_git_github_api_base_url());
    target.set_git_github_enterprise(source.get_git_github_enterprise());
    target.set_git_gitlab_api_base_url(source.get_git_gitlab_api_base_url());
    target.set_git_gitlab_enterprise(source.get_git_gitlab_enterprise());
    target.set_git_gitee_api_base_url(source.get_git_gitee_api_base_url());
    target.set_git_gitee_enterprise(source.get_git_gitee_enterprise());
    target.set_ai_agent_base_url(source.get_ai_agent_base_url());
    target.set_ai_agent_api_key(source.get_ai_agent_api_key());
    target.set_ai_agent_model(source.get_ai_agent_model());
    target.set_ai_agent_provider(source.get_ai_agent_provider());

    page.set_git_tokens_saved(window.get_settings_git_tokens_saved());
    page.set_git_hosting_config_saved(window.get_settings_git_hosting_config_saved());
    page.set_ai_agent_saved(window.get_settings_ai_agent_saved());
    page.set_ai_agent_testing(window.get_settings_ai_agent_testing());
    page.set_ai_agent_provider_index(window.get_settings_ai_agent_provider_index());
    page.set_update_checking(window.get_settings_update_checking());
    page.set_viewport_width(window.get_settings_page_viewport_width());
    page.set_viewport_height(window.get_settings_page_viewport_height());
}

fn bind_callbacks(window: &MainWindow, page: &GitHubSettingsPage) {
    let window_weak = window.as_weak();
    page.on_check_update(move || {
        if let Some(window) = window_weak.upgrade() {
            window.invoke_check_update();
        }
    });

    let window_weak = window.as_weak();
    page.on_simulate_crash(move || {
        if let Some(window) = window_weak.upgrade() {
            window.invoke_simulate_crash();
        }
    });

    let state = SettingsAppState::get(page);

    let page_weak = page.as_weak();
    let window_weak = window.as_weak();
    state.on_toggle_theme(move |mode| {
        if let Some(page) = page_weak.upgrade() {
            SettingsAppState::get(&page).set_theme_mode(mode);
        }
        if let Some(window) = window_weak.upgrade() {
            AppState::get(&window).invoke_toggle_theme(mode);
        }
    });

    let window_weak = window.as_weak();
    state.on_toggle_auto_update(move |enabled| {
        if let Some(window) = window_weak.upgrade() {
            let state = AppState::get(&window);
            state.set_auto_update(enabled);
            state.invoke_toggle_auto_update(enabled);
        }
    });

    let window_weak = window.as_weak();
    state.on_toggle_tracing_logger_active(move |enabled| {
        if let Some(window) = window_weak.upgrade() {
            let state = AppState::get(&window);
            state.set_tracing_logger_active(enabled);
            state.invoke_toggle_tracing_logger_active(enabled);
        }
    });

    let window_weak = window.as_weak();
    state.on_toggle_terminal_verbose_logging(move |enabled| {
        if let Some(window) = window_weak.upgrade() {
            let state = AppState::get(&window);
            state.set_terminal_verbose_logging(enabled);
            state.invoke_toggle_terminal_verbose_logging(enabled);
        }
    });

    let window_weak = window.as_weak();
    state.on_toggle_ui_sound_effects(move |enabled| {
        if let Some(window) = window_weak.upgrade() {
            let state = AppState::get(&window);
            state.set_ui_sound_effects(enabled);
            state.invoke_toggle_ui_sound_effects(enabled);
        }
    });

    let window_weak = window.as_weak();
    state.on_save_git_tokens(move |github, gitlab, gitee, gitee_enterprise| {
        if let Some(window) = window_weak.upgrade() {
            let state = AppState::get(&window);
            state.set_git_github_token(github.clone());
            state.set_git_gitlab_token(gitlab.clone());
            state.set_git_gitee_token(gitee.clone());
            state.set_git_gitee_enterprise_token(gitee_enterprise.clone());
            state.invoke_save_git_tokens(github, gitlab, gitee, gitee_enterprise);
        }
    });

    let window_weak = window.as_weak();
    state.on_save_git_hosting_config(
        move |github_url,
              github_enterprise,
              gitlab_url,
              gitlab_enterprise,
              gitee_url,
              gitee_enterprise| {
            if let Some(window) = window_weak.upgrade() {
                let state = AppState::get(&window);
                state.set_git_github_api_base_url(github_url.clone());
                state.set_git_github_enterprise(github_enterprise.clone());
                state.set_git_gitlab_api_base_url(gitlab_url.clone());
                state.set_git_gitlab_enterprise(gitlab_enterprise.clone());
                state.set_git_gitee_api_base_url(gitee_url.clone());
                state.set_git_gitee_enterprise(gitee_enterprise.clone());
                state.invoke_save_git_hosting_config(
                    github_url,
                    github_enterprise,
                    gitlab_url,
                    gitlab_enterprise,
                    gitee_url,
                    gitee_enterprise,
                );
            }
        },
    );

    let window_weak = window.as_weak();
    state.on_save_ai_agent_settings(move |provider, base_url, api_key, model| {
        if let Some(window) = window_weak.upgrade() {
            let state = AppState::get(&window);
            state.set_ai_agent_provider(provider.clone());
            state.set_ai_agent_base_url(base_url.clone());
            state.set_ai_agent_api_key(api_key.clone());
            state.set_ai_agent_model(model.clone());
            state.invoke_save_ai_agent_settings(provider, base_url, api_key, model);
        }
    });

    let window_weak = window.as_weak();
    state.on_test_ai_agent_connection(move |base_url, api_key, model| {
        if let Some(window) = window_weak.upgrade() {
            AppState::get(&window).invoke_test_ai_agent_connection(base_url, api_key, model);
        }
    });
}
