//! Bridge between the root window and the independently generated Settings page.
//!
//! Slint component factories create a separate global-state graph for their
//! product. This module deliberately mirrors the small Settings contract and
//! forwards callbacks on the Slint event loop instead of pretending those
//! globals are shared across generated crates.

use std::sync::{Arc, Mutex};

use slint::Model;
use slint::{ComponentFactory, ComponentHandle, Global, ModelRc, VecModel};
use usher_preview::{IMAGE_PROVIDER_ID, MARKDOWN_PROVIDER_ID, PLAIN_TEXT_PROVIDER_ID};
#[cfg(feature = "ai-code-review")]
use usher_ui_settings::AiReviewRuleItem;
use usher_ui_settings::{
    AppState as SettingsAppState, GitHubSettingsPage, LanguageOption as SettingsLanguageOption,
};

use crate::{AppState, MainWindow, UpdateLogic};

#[cfg(feature = "ai-code-review")]
enum AddReviewRuleError {
    Name(&'static str),
    Path(&'static str),
}

#[cfg(feature = "ai-code-review")]
#[derive(Default)]
struct AiReviewValidationErrors {
    name_key: Option<&'static str>,
    path_key: Option<&'static str>,
}

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
    ai_agent_settings: Arc<Mutex<Option<crate::ai::settings_state::AiAgentSettingsFacade>>>,
    #[cfg(feature = "ai-code-review")]
    ai_review_config: Arc<Mutex<Option<crate::ai_review::ReviewConfigFacade>>>,
    #[cfg(feature = "ai-code-review")]
    ai_review_validation_errors: Arc<Mutex<AiReviewValidationErrors>>,
}

impl SettingsPageBridge {
    pub(crate) fn install(&self, window: &MainWindow) {
        let page_slot = self.page.clone();
        let ai_agent_settings = self.ai_agent_settings.clone();
        #[cfg(feature = "ai-code-review")]
        let ai_review_config = self.ai_review_config.clone();
        #[cfg(feature = "ai-code-review")]
        let ai_review_validation_errors = self.ai_review_validation_errors.clone();
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
            let ai_agent_settings = ai_agent_settings
                .lock()
                .expect("AI Agent settings bridge poisoned")
                .clone();
            if let Some(settings) = &ai_agent_settings {
                apply_ai_agent_snapshot_to_page(&page, settings.snapshot().state());
            }
            bind_callbacks(&window, &page, ai_agent_settings);
            #[cfg(feature = "ai-code-review")]
            if let Some(config) = ai_review_config
                .lock()
                .expect("AI review settings bridge poisoned")
                .clone()
            {
                apply_ai_review_snapshot(&page, config.snapshot().state());
                *ai_review_validation_errors
                    .lock()
                    .expect("AI review validation state poisoned") =
                    AiReviewValidationErrors::default();
                bind_ai_review_callbacks(
                    &window,
                    &page,
                    config,
                    ai_review_validation_errors.clone(),
                );
            }
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
        let bridge = self.clone();
        window.on_settings_select_pointer_released(move |x, y| {
            bridge.with_page(|page| page.invoke_select_pointer_released(x, y));
        });
        let bridge = self.clone();
        window.on_settings_select_dismiss(move || {
            bridge.with_page(|page| page.invoke_dismiss_select());
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

    pub(crate) fn sync_theme(&self, mode: i32, effective_dark_mode: bool) {
        self.with_page(|page| {
            let state = SettingsAppState::get(page);
            state.set_theme_mode(mode);
            state.set_effective_dark_mode(effective_dark_mode);
        });
    }

    pub(crate) fn sync_language(&self, preference: &str) {
        self.with_page(|page| {
            let reply_language = SettingsAppState::get(page)
                .get_ai_agent_reply_language_selection()
                .id
                .to_string();
            sync_language_state(page, preference);
            sync_ai_reply_language_state(page, &reply_language);
            #[cfg(feature = "ai-code-review")]
            apply_ai_review_validation_errors(page, &self.ai_review_validation_errors);
        });
    }

    pub(crate) fn configure_ai_agent_settings(
        &self,
        settings: crate::ai::settings_state::AiAgentSettingsFacade,
    ) {
        *self
            .ai_agent_settings
            .lock()
            .expect("AI Agent settings bridge poisoned") = Some(settings.clone());
        self.with_page(|page| apply_ai_agent_snapshot_to_page(page, settings.snapshot().state()));
    }

    /// Discard page-local drafts and restore the latest committed host/domain
    /// state before the stable Settings factory becomes visible again.
    /// Must run on the Slint event loop.
    pub(crate) fn reset_drafts_from_committed(&self, window: &MainWindow) {
        let ai_agent_snapshot = self
            .ai_agent_settings
            .lock()
            .expect("AI Agent settings bridge poisoned")
            .as_ref()
            .map(crate::ai::settings_state::AiAgentSettingsFacade::snapshot);
        self.with_page(|page| {
            page.set_route_entry_revision(page.get_route_entry_revision().wrapping_add(1));
            sync_state_from_window(window, page);
            if let Some(snapshot) = &ai_agent_snapshot {
                apply_ai_agent_snapshot_to_page(page, snapshot.state());
            }
        });
    }

    #[cfg(feature = "ai-code-review")]
    pub(crate) fn configure_ai_review(&self, config: crate::ai_review::ReviewConfigFacade) {
        *self
            .ai_review_config
            .lock()
            .expect("AI review settings bridge poisoned") = Some(config.clone());
        self.with_page(|page| apply_ai_review_snapshot(page, config.snapshot().state()));
    }

    #[cfg(feature = "ai-code-review")]
    pub(crate) fn apply_ai_review_configuration(
        &self,
        state: &usher_ai_review::ReviewConfigurationState,
    ) {
        self.with_page(|page| apply_ai_review_snapshot(page, state));
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

#[cfg(feature = "ai-code-review")]
fn apply_ai_review_snapshot(
    page: &GitHubSettingsPage,
    state: &usher_ai_review::ReviewConfigurationState,
) {
    let rules = state
        .rules
        .iter()
        .map(|rule| AiReviewRuleItem {
            id: rule.id.0.clone().into(),
            name: rule.name.clone().into(),
            path: rule.markdown_path.to_string_lossy().into_owned().into(),
            is_default: state.default_rule_id.as_ref() == Some(&rule.id),
        })
        .collect::<Vec<_>>();
    let current = page.get_ai_review_rules();
    if let Some(model) = current
        .as_any()
        .downcast_ref::<VecModel<AiReviewRuleItem>>()
    {
        sync_ai_review_rule_model(model, &rules);
    } else {
        page.set_ai_review_rules(ModelRc::new(VecModel::from(rules)));
    }
    page.set_ai_review_max_concurrent_tasks(i32::from(state.max_concurrent_tasks));
}

#[cfg(feature = "ai-code-review")]
fn sync_ai_review_rule_model(model: &VecModel<AiReviewRuleItem>, rules: &[AiReviewRuleItem]) {
    for (index, rule) in rules.iter().enumerate() {
        if let Some(current) = model.row_data(index)
            && current.id == rule.id
        {
            if !ai_review_rule_items_equal(&current, rule) {
                model.set_row_data(index, rule.clone());
            }
            continue;
        }

        if let Some(existing_index) = (index + 1..model.row_count()).find(|candidate| {
            model
                .row_data(*candidate)
                .is_some_and(|current| current.id == rule.id)
        }) {
            model.remove(existing_index);
        }
        model.insert(index, rule.clone());
    }

    while model.row_count() > rules.len() {
        model.remove(model.row_count() - 1);
    }
}

#[cfg(feature = "ai-code-review")]
fn ai_review_rule_items_equal(left: &AiReviewRuleItem, right: &AiReviewRuleItem) -> bool {
    left.id == right.id
        && left.name == right.name
        && left.path == right.path
        && left.is_default == right.is_default
}

#[cfg(feature = "ai-code-review")]
fn bind_ai_review_callbacks(
    _window: &MainWindow,
    page: &GitHubSettingsPage,
    config: crate::ai_review::ReviewConfigFacade,
    validation_errors: Arc<Mutex<AiReviewValidationErrors>>,
) {
    let config_max = config.clone();
    page.on_ai_review_set_max_concurrent_tasks(move |value| {
        if let Ok(value) = u8::try_from(value)
            && let Err(error) = config_max.set_max_concurrent_tasks(value)
        {
            log::warn!("[AI Review][Settings] Invalid concurrency: {error:?}");
        }
    });

    let config_add = config.clone();
    let page_weak = page.as_weak();
    page.on_ai_review_add_rule(move |name, path| {
        let config = config_add.clone();
        let page = page_weak.clone();
        let validation_errors = validation_errors.clone();
        let name = name.to_string();
        let path = std::path::PathBuf::from(path.as_str().trim());
        crate::app_runtime::shared_runtime().spawn(async move {
            let validation_path = path.clone();
            let validation = tokio::task::spawn_blocking(move || {
                crate::ai_review::validate_rule_file(&validation_path)
            })
            .await;
            let result = match validation {
                Ok(Ok(())) => {
                    let was_empty = config.snapshot().state().rules.is_empty();
                    match config.upsert_rule(None, name, path) {
                        Ok(id) => {
                            if was_empty
                                && let Err(error) = config.set_default_rule(Some(id))
                            {
                                log::warn!(
                                    "[AI Review][Settings] Failed to select initial default rule: {error:?}"
                                );
                            }
                            Ok(())
                        }
                        Err(usher_ai_review::ReviewConfigError::DuplicateName) => Err(
                            AddReviewRuleError::Name(
                                "settings.ai_review.validation.duplicate_name",
                            ),
                        ),
                        Err(error) => {
                            log::warn!("[AI Review][Settings] Failed to add rule: {error:?}");
                            Err(AddReviewRuleError::Name(
                                "settings.ai_review.validation.invalid_name",
                            ))
                        }
                    }
                }
                Ok(Err(error)) => Err(AddReviewRuleError::Path(error.message_key())),
                Err(error) => {
                    log::warn!("[AI Review][Settings] Rule validation worker failed: {error}");
                    Err(AddReviewRuleError::Path(
                        "settings.ai_review.validation.worker_failed",
                    ))
                }
            };
            {
                let mut errors = validation_errors
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                match result {
                    Ok(()) => {
                        errors.name_key = None;
                        errors.path_key = None;
                    }
                    Err(AddReviewRuleError::Name(key)) => {
                        errors.name_key = Some(key);
                        errors.path_key = None;
                    }
                    Err(AddReviewRuleError::Path(key)) => {
                        errors.name_key = None;
                        errors.path_key = Some(key);
                    }
                }
            }
            let _ = slint::invoke_from_event_loop(move || {
                let Some(page) = page.upgrade() else {
                    return;
                };
                match result {
                    Ok(()) => {
                        page.set_ai_review_rule_name_error("".into());
                        page.set_ai_review_rule_path_error("".into());
                    }
                    Err(AddReviewRuleError::Name(key)) => {
                        page.set_ai_review_rule_name_error(crate::i18n::tr(key).as_ref().into());
                        page.set_ai_review_rule_path_error("".into());
                    }
                    Err(AddReviewRuleError::Path(key)) => {
                        page.set_ai_review_rule_name_error("".into());
                        page.set_ai_review_rule_path_error(crate::i18n::tr(key).as_ref().into());
                    }
                }
                page.set_ai_review_rule_add_pending(false);
            });
        });
    });

    let config_delete = config.clone();
    page.on_ai_review_delete_rule(move |id| {
        if let Err(error) = config_delete.delete_rule(usher_ai_review::RuleId(id.to_string())) {
            log::warn!("[AI Review][Settings] Failed to delete rule: {error:?}");
        }
    });

    page.on_ai_review_set_default_rule(move |id| {
        if let Err(error) = config.set_default_rule(Some(usher_ai_review::RuleId(id.to_string()))) {
            log::warn!("[AI Review][Settings] Failed to set default rule: {error:?}");
        }
    });
}

#[cfg(feature = "ai-code-review")]
fn apply_ai_review_validation_errors(
    page: &GitHubSettingsPage,
    validation_errors: &Arc<Mutex<AiReviewValidationErrors>>,
) {
    let errors = validation_errors
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    page.set_ai_review_rule_name_error(
        errors
            .name_key
            .map(|key| crate::i18n::tr(key).into_owned())
            .unwrap_or_default()
            .into(),
    );
    page.set_ai_review_rule_path_error(
        errors
            .path_key
            .map(|key| crate::i18n::tr(key).into_owned())
            .unwrap_or_default()
            .into(),
    );
}

fn sync_state_from_window(window: &MainWindow, page: &GitHubSettingsPage) {
    let source = AppState::get(window);
    let target = SettingsAppState::get(page);

    target.set_theme_mode(source.get_theme_mode());
    target.set_effective_dark_mode(source.get_effective_dark_mode());
    sync_language_state(page, source.get_language_selection().id.as_str());
    target.set_version(source.get_version());
    target.set_auto_update(source.get_auto_update());
    target.set_update_channel(source.get_update_channel());
    target.set_application_console_logging(source.get_application_console_logging());
    target.set_application_file_logging(source.get_application_file_logging());
    target.set_tracing_logger_active(source.get_tracing_logger_active());
    target.set_terminal_verbose_logging(source.get_terminal_verbose_logging());
    target.set_memory_diagnostics_logging(source.get_memory_diagnostics_logging());
    target.set_ui_sound_effects(source.get_ui_sound_effects());
    target.set_artifact_preview_presentation_mode(source.get_artifact_preview_presentation_mode());
    target.set_artifact_preview_markdown_enabled(source.get_artifact_preview_markdown_enabled());
    target.set_artifact_preview_image_enabled(source.get_artifact_preview_image_enabled());
    target
        .set_artifact_preview_plain_text_enabled(source.get_artifact_preview_plain_text_enabled());
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
    sync_ai_reply_language_state(
        page,
        source.get_ai_agent_reply_language_selection().id.as_str(),
    );

    page.set_git_tokens_saved(window.get_settings_git_tokens_saved());
    page.set_git_hosting_config_saved(window.get_settings_git_hosting_config_saved());
    page.set_ai_agent_saved(window.get_settings_ai_agent_saved());
    page.set_ai_agent_testing(window.get_settings_ai_agent_testing());
    page.set_update_checking(window.get_settings_update_checking());
    page.set_viewport_width(window.get_settings_page_viewport_width());
    page.set_viewport_height(window.get_settings_page_viewport_height());
}

fn sync_language_state(page: &GitHubSettingsPage, preference: &str) {
    let target = SettingsAppState::get(page);
    let canonical = usher_i18n::canonical_preference(preference);
    let choices = crate::i18n::language_choices();
    let selected_label = choices
        .iter()
        .find(|choice| choice.id == canonical)
        .map(|choice| choice.label.as_str())
        .unwrap_or(canonical);
    target.set_language_selection(SettingsLanguageOption {
        id: canonical.into(),
        label: selected_label.into(),
    });
}

fn sync_ai_reply_language_state(page: &GitHubSettingsPage, preference: &str) {
    let target = SettingsAppState::get(page);
    let canonical = if preference
        .trim()
        .eq_ignore_ascii_case(usher_i18n::AUTO_LANGUAGE)
    {
        usher_i18n::AUTO_LANGUAGE
    } else {
        usher_i18n::canonical_preference(preference)
    };
    let choices = crate::i18n::ai_reply_language_choices();
    let selected_label = choices
        .iter()
        .find(|choice| choice.id == canonical)
        .map(|choice| choice.label.as_str())
        .unwrap_or(canonical);
    target.set_ai_agent_reply_language_selection(SettingsLanguageOption {
        id: canonical.into(),
        label: selected_label.into(),
    });
}

fn apply_ai_agent_snapshot_to_page(
    page: &GitHubSettingsPage,
    settings: &crate::settings::AiAgentSettings,
) {
    let state = SettingsAppState::get(page);
    state.set_ai_agent_base_url(settings.base_url.clone().into());
    state.set_ai_agent_api_key(settings.api_key.clone().into());
    state.set_ai_agent_model(settings.model.clone().into());
    state.set_ai_agent_provider(settings.provider.clone().into());
    sync_ai_reply_language_state(page, &settings.reply_language);
}

fn ai_agent_draft_from_page(page: &GitHubSettingsPage) -> crate::settings::AiAgentSettings {
    let state = SettingsAppState::get(page);
    crate::settings::AiAgentSettings {
        provider: state.get_ai_agent_provider().to_string(),
        base_url: state.get_ai_agent_base_url().to_string(),
        api_key: state.get_ai_agent_api_key().to_string(),
        model: state.get_ai_agent_model().to_string(),
        reply_language: state.get_ai_agent_reply_language_selection().id.to_string(),
    }
}

fn apply_ai_agent_snapshot_to_window(
    window: &MainWindow,
    settings: &crate::settings::AiAgentSettings,
) {
    let state = AppState::get(window);
    state.set_ai_agent_base_url(settings.base_url.clone().into());
    state.set_ai_agent_api_key(settings.api_key.clone().into());
    state.set_ai_agent_model(settings.model.clone().into());
    state.set_ai_agent_provider(settings.provider.clone().into());
    crate::i18n::sync_ai_reply_language(window, &settings.reply_language);
}

fn bind_callbacks(
    window: &MainWindow,
    page: &GitHubSettingsPage,
    ai_agent_settings: Option<crate::ai::settings_state::AiAgentSettingsFacade>,
) {
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

    let window_weak = window.as_weak();
    page.on_open_log_directory(move || {
        if let Some(window) = window_weak.upgrade() {
            window.invoke_open_log_directory();
        }
    });

    let window_weak = window.as_weak();
    page.on_export_diagnostics(move || {
        if let Some(window) = window_weak.upgrade() {
            window.invoke_export_diagnostics();
        }
    });

    let state = SettingsAppState::get(page);

    let window_weak = window.as_weak();
    state.on_set_language(move |language| {
        if let Some(window) = window_weak.upgrade() {
            AppState::get(&window).invoke_set_language(language);
        }
    });

    let page_weak = page.as_weak();
    state.on_select_ai_agent_provider(move |provider| {
        if let Some(page) = page_weak.upgrade() {
            let draft = ai_agent_draft_from_page(&page);
            let draft = crate::ai::settings_state::apply_provider_preset(&draft, provider.as_str());
            apply_ai_agent_snapshot_to_page(&page, &draft);
        }
    });

    let page_weak = page.as_weak();
    state.on_set_ai_agent_base_url(move |_| {
        if let Some(page) = page_weak.upgrade() {
            SettingsAppState::get(&page).set_ai_agent_provider("custom".into());
        }
    });

    let page_weak = page.as_weak();
    state.on_set_ai_agent_model(move |_| {
        if let Some(page) = page_weak.upgrade() {
            SettingsAppState::get(&page).set_ai_agent_provider("custom".into());
        }
    });

    let page_weak = page.as_weak();
    state.on_select_ai_agent_reply_language(move |language| {
        if let Some(page) = page_weak.upgrade() {
            sync_ai_reply_language_state(&page, language.as_str());
        }
    });

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

    let page_weak = page.as_weak();
    let window_weak = window.as_weak();
    state.on_set_update_channel(move |channel| {
        if let Some(page) = page_weak.upgrade() {
            SettingsAppState::get(&page).set_update_channel(channel.clone());
        }
        if let Some(window) = window_weak.upgrade() {
            let state = AppState::get(&window);
            state.set_update_channel(channel.clone());
            state.invoke_set_update_channel(channel);
        }
    });

    let window_weak = window.as_weak();
    state.on_toggle_application_console_logging(move |enabled| {
        if let Some(window) = window_weak.upgrade() {
            let state = AppState::get(&window);
            state.set_application_console_logging(enabled);
            state.invoke_toggle_application_console_logging(enabled);
        }
    });

    let window_weak = window.as_weak();
    state.on_toggle_application_file_logging(move |enabled| {
        if let Some(window) = window_weak.upgrade() {
            let state = AppState::get(&window);
            state.set_application_file_logging(enabled);
            state.invoke_toggle_application_file_logging(enabled);
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
    state.on_toggle_memory_diagnostics_logging(move |enabled| {
        if let Some(window) = window_weak.upgrade() {
            let state = AppState::get(&window);
            state.set_memory_diagnostics_logging(enabled);
            state.invoke_toggle_memory_diagnostics_logging(enabled);
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

    let page_weak = page.as_weak();
    let window_weak = window.as_weak();
    state.on_set_artifact_preview_presentation_mode(move |mode| {
        let mode = mode.clamp(0, 1);
        if let Some(page) = page_weak.upgrade() {
            SettingsAppState::get(&page).set_artifact_preview_presentation_mode(mode);
        }
        if let Some(window) = window_weak.upgrade() {
            let state = AppState::get(&window);
            state.set_artifact_preview_presentation_mode(mode);
            state.invoke_set_artifact_preview_presentation_mode(mode);
        }
    });

    let page_weak = page.as_weak();
    let window_weak = window.as_weak();
    state.on_set_artifact_preview_provider_enabled(move |provider_id, enabled| {
        if let Some(page) = page_weak.upgrade() {
            let state = SettingsAppState::get(&page);
            match provider_id.as_str() {
                MARKDOWN_PROVIDER_ID => state.set_artifact_preview_markdown_enabled(enabled),
                IMAGE_PROVIDER_ID => state.set_artifact_preview_image_enabled(enabled),
                PLAIN_TEXT_PROVIDER_ID => state.set_artifact_preview_plain_text_enabled(enabled),
                _ => {}
            }
        }
        if let Some(window) = window_weak.upgrade() {
            let state = AppState::get(&window);
            match provider_id.as_str() {
                MARKDOWN_PROVIDER_ID => state.set_artifact_preview_markdown_enabled(enabled),
                IMAGE_PROVIDER_ID => state.set_artifact_preview_image_enabled(enabled),
                PLAIN_TEXT_PROVIDER_ID => state.set_artifact_preview_plain_text_enabled(enabled),
                _ => {}
            }
            state.invoke_set_artifact_preview_provider_enabled(provider_id, enabled);
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
    let page_weak = page.as_weak();
    let settings = ai_agent_settings;
    state.on_save_ai_agent_settings(move |provider, base_url, api_key, model, reply_language| {
        if let Some(window) = window_weak.upgrade() {
            if let Some(settings) = &settings {
                let snapshot = settings.replace(crate::settings::AiAgentSettings {
                    provider: provider.to_string(),
                    base_url: base_url.to_string(),
                    api_key: api_key.to_string(),
                    model: model.to_string(),
                    reply_language: reply_language.to_string(),
                });
                if let Some(page) = page_weak.upgrade() {
                    apply_ai_agent_snapshot_to_page(&page, snapshot.state());
                }
                apply_ai_agent_snapshot_to_window(&window, snapshot.state());
            } else {
                let state = AppState::get(&window);
                state.set_ai_agent_provider(provider.clone());
                state.set_ai_agent_base_url(base_url.clone());
                state.set_ai_agent_api_key(api_key.clone());
                state.set_ai_agent_model(model.clone());
                crate::i18n::sync_ai_reply_language(&window, reply_language.as_str());
            }
            AppState::get(&window).invoke_save_ai_agent_settings(
                provider,
                base_url,
                api_key,
                model,
                reply_language,
            );
        }
    });

    let window_weak = window.as_weak();
    state.on_test_ai_agent_connection(move |base_url, api_key, model| {
        if let Some(window) = window_weak.upgrade() {
            AppState::get(&window).invoke_test_ai_agent_connection(base_url, api_key, model);
        }
    });
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::Duration;

    use i_slint_backend_testing::ElementHandle;
    #[cfg(feature = "ai-code-review")]
    use slint::Model;
    use slint::platform::PointerEventButton;
    use slint::{ComponentHandle, Global};
    use usher_preview::IMAGE_PROVIDER_ID;
    use usher_ui_settings::{AppState as SettingsAppState, GitHubSettingsPage};

    #[cfg(feature = "ai-code-review")]
    use super::apply_ai_review_snapshot;
    use super::{SettingsPageBridge, bind_callbacks, sync_language_state, sync_state_from_window};
    use crate::{AppState, MainWindow, Route};

    fn language_rows(page: &GitHubSettingsPage) -> Vec<(String, bool)> {
        let mut rows = ["System default", "跟随系统", "English", "简体中文"]
            .into_iter()
            .flat_map(|label| ElementHandle::find_by_accessible_label(page, label))
            .filter_map(|element| {
                Some((
                    element.absolute_position().y,
                    element.accessible_label().unwrap().to_string(),
                    element.accessible_checked()?,
                ))
            })
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| left.0.total_cmp(&right.0));
        rows.into_iter()
            .map(|(_, label, checked)| (label, checked))
            .collect()
    }

    fn ai_reply_language_rows(page: &GitHubSettingsPage) -> Vec<(String, bool)> {
        let mut rows = [
            "Follow interface language",
            "跟随界面语言",
            "English",
            "简体中文",
        ]
        .into_iter()
        .flat_map(|label| ElementHandle::find_by_accessible_label(page, label))
        .filter_map(|element| {
            Some((
                element.absolute_position().y,
                element.accessible_label()?.to_string(),
                element.accessible_checked()?,
            ))
        })
        .collect::<Vec<_>>();
        rows.sort_by(|left, right| left.0.total_cmp(&right.0));
        rows.into_iter()
            .map(|(_, label, checked)| (label, checked))
            .collect()
    }

    fn open_language_picker(page: &GitHubSettingsPage) {
        let picker =
            ElementHandle::find_by_element_id(page, "GitHubSettingsSectionMenuButton::menu-touch")
                .max_by(|left, right| {
                    left.absolute_position()
                        .y
                        .total_cmp(&right.absolute_position().y)
                })
                .expect("language picker button");
        picker.mock_single_click(PointerEventButton::Left);
    }

    fn ai_reply_language_trigger_label(page: &GitHubSettingsPage) -> String {
        [
            "Follow interface language",
            "跟随界面语言",
            "English",
            "简体中文",
        ]
        .into_iter()
        .flat_map(|label| ElementHandle::find_by_accessible_label(page, label))
        .filter(|element| element.accessible_checked().is_none())
        .max_by(|left, right| {
            left.absolute_position()
                .y
                .total_cmp(&right.absolute_position().y)
        })
        .and_then(|element| element.accessible_label().map(|label| label.to_string()))
        .expect("AI reply language trigger label")
    }

    fn open_section_picker(page: &GitHubSettingsPage) {
        let picker =
            ElementHandle::find_by_element_id(page, "GitHubSettingsSectionMenuButton::menu-touch")
                .min_by(|left, right| {
                    left.absolute_position()
                        .y
                        .total_cmp(&right.absolute_position().y)
                })
                .expect("settings section picker button");
        picker.mock_single_click(PointerEventButton::Left);
    }

    fn section_picker_trigger_label(page: &GitHubSettingsPage) -> String {
        [
            "General",
            "通用",
            "Code hosting",
            "代码托管",
            "AI Agent",
            "AI Review",
            "Diagnostics",
            "诊断",
        ]
        .into_iter()
        .flat_map(|label| ElementHandle::find_by_accessible_label(page, label))
        .filter(|element| element.accessible_checked().is_none())
        .min_by(|left, right| {
            left.absolute_position()
                .y
                .total_cmp(&right.absolute_position().y)
        })
        .and_then(|element| element.accessible_label().map(|label| label.to_string()))
        .expect("settings section trigger label")
    }

    fn mounted_page(bridge: &SettingsPageBridge) -> GitHubSettingsPage {
        bridge
            .page
            .lock()
            .expect("settings page bridge poisoned")
            .clone()
            .and_then(|page| page.upgrade())
            .expect("mounted settings page")
    }

    fn select_language(page: &GitHubSettingsPage, label: &str) {
        let option = ElementHandle::find_by_accessible_label(page, label)
            .find(|element| element.accessible_checked().is_some())
            .unwrap_or_else(|| panic!("language option {label:?}"));
        option.mock_single_click(PointerEventButton::Left);
        assert!(language_rows(page).is_empty(), "selection closes the menu");
    }

    fn click_settings_position(page: &GitHubSettingsPage, position: slint::LogicalPosition) {
        use slint::platform::WindowEvent;
        page.window()
            .dispatch_event(WindowEvent::PointerMoved { position });
        page.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        page.window().dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
        page.invoke_select_pointer_released(position.x, position.y);
    }

    fn select_settings_section(page: &GitHubSettingsPage, label: &str) {
        ElementHandle::find_by_accessible_label(page, label)
            .find(|element| element.accessible_checked().is_some())
            .unwrap_or_else(|| panic!("settings section {label:?}"))
            .mock_single_click(PointerEventButton::Left);
    }

    fn select_ai_agent_provider(page: &GitHubSettingsPage, label: &str) {
        ElementHandle::find_by_accessible_label(page, label)
            .find(|element| element.accessible_checked().is_some())
            .unwrap_or_else(|| panic!("AI Agent provider {label:?}"))
            .mock_single_click(PointerEventButton::Left);
    }

    fn settings_field_value(page: &GitHubSettingsPage, label: &str) -> String {
        ElementHandle::find_by_accessible_label(page, label)
            .find_map(|element| element.accessible_value())
            .map(|value| value.to_string())
            .unwrap_or_else(|| panic!("Settings field {label:?}"))
    }

    fn save_ai_agent_draft(page: &GitHubSettingsPage) {
        let draft = super::ai_agent_draft_from_page(page);
        SettingsAppState::get(page).invoke_save_ai_agent_settings(
            draft.provider.into(),
            draft.base_url.into(),
            draft.api_key.into(),
            draft.model.into(),
            draft.reply_language.into(),
        );
    }

    fn expected_language_rows(system_label: &str, selected_label: &str) -> Vec<(String, bool)> {
        vec![
            (system_label.to_string(), system_label == selected_label),
            ("English".to_string(), selected_label == "English"),
            ("简体中文".to_string(), selected_label == "简体中文"),
        ]
    }

    #[test]
    fn language_select_floats_dismisses_and_supports_keyboard_selection() {
        use slint::platform::{Key, WindowEvent};

        let _locale_guard = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let page = GitHubSettingsPage::new().expect("settings page");
        crate::i18n::apply_preference("en").expect("English labels");
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        let weak = page.as_weak();
        SettingsAppState::get(&page).on_set_language(move |value| {
            sync_language_state(&weak.upgrade().unwrap(), value.as_str());
        });

        let popup_count = || usize::from(!language_rows(&page).is_empty());
        let press = |key: Key| {
            page.window()
                .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
            page.window()
                .dispatch_event(WindowEvent::KeyReleased { text: key.into() });
        };
        for (width, height) in [(550.0, 600.0), (1000.0, 768.0), (1280.0, 900.0)] {
            page.set_viewport_width(width);
            page.set_viewport_height(height);
            for dark in [false, true] {
                SettingsAppState::get(&page).set_effective_dark_mode(dark);
                sync_language_state(&page, "en");
                let appearance = || {
                    ElementHandle::find_by_accessible_label(&page, "Light")
                        .next()
                        .expect("following appearance control")
                        .absolute_position()
                };
                let before = appearance();
                open_language_picker(&page);
                assert_eq!(popup_count(), 1);
                let menu = ElementHandle::find_by_element_id(&page, "GitHubSelectPopup::menu")
                    .next()
                    .expect("language menu surface");
                let menu_position = menu.absolute_position();
                let menu_size = menu.size();
                assert!(menu_position.x >= 8.0 && menu_position.y >= 8.0);
                assert!(menu_position.x + menu_size.width <= width - 7.5);
                assert!(menu_position.y + menu_size.height <= height - 7.5);
                for label in ["System default", "English", "简体中文"] {
                    let row = ElementHandle::find_by_accessible_label(&page, label)
                        .find(|element| element.accessible_checked().is_some())
                        .expect("visible language option");
                    let position = row.absolute_position();
                    let size = row.size();
                    assert!(
                        (position.x - menu_position.x - 4.0).abs() < 0.5,
                        "{label}: option must start inside the menu, not the page"
                    );
                    assert!((size.width - (menu_size.width - 8.0)).abs() < 0.5);
                    assert!(position.y >= menu_position.y + 3.5);
                    assert!(position.y + size.height <= menu_position.y + menu_size.height - 3.5);
                }
                assert_eq!(
                    appearance(),
                    before,
                    "opening must not move following fields"
                );
                click_settings_position(&page, slint::LogicalPosition::new(4.0, 4.0));
                assert_eq!(popup_count(), 0, "blank outside click dismisses");
                assert_eq!(
                    SettingsAppState::get(&page).get_language_selection().id,
                    "en"
                );

                open_language_picker(&page);
                press(Key::DownArrow);
                press(Key::Escape);
                assert_eq!(popup_count(), 0, "Escape dismisses");
                assert_eq!(
                    SettingsAppState::get(&page).get_language_selection().id,
                    "en"
                );

                // Focus returns to the trigger after dismissal.
                press(Key::Return);
                assert_eq!(popup_count(), 1, "Enter reopens the focused trigger");
                press(Key::DownArrow);
                press(Key::Return);
                assert_eq!(popup_count(), 0, "Enter commits and closes");
                assert_eq!(
                    SettingsAppState::get(&page).get_language_selection().id,
                    "zh-CN"
                );
                assert_eq!(appearance(), before);
            }
        }
        crate::i18n::apply_preference(usher_i18n::fallback_locale().id)
            .expect("restore fallback language");
    }

    #[test]
    fn language_select_does_not_consume_another_controls_first_click() {
        use std::cell::Cell;
        use std::rc::Rc;
        let _locale_guard = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let page = GitHubSettingsPage::new().unwrap();
        page.set_viewport_width(550.0);
        page.set_viewport_height(600.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        crate::i18n::apply_preference("en").unwrap();
        sync_language_state(&page, "en");
        let clicks = Rc::new(Cell::new(0));
        let observed = clicks.clone();
        SettingsAppState::get(&page).on_toggle_theme(move |_| observed.set(observed.get() + 1));
        open_language_picker(&page);
        let light = ElementHandle::find_by_accessible_label(&page, "Light")
            .next()
            .unwrap();
        let position = light.absolute_position();
        let size = light.size();
        click_settings_position(
            &page,
            slint::LogicalPosition::new(
                position.x + size.width / 2.0,
                position.y + size.height / 2.0,
            ),
        );
        assert_eq!(clicks.get(), 1, "the same click must dismiss and activate");
        assert!(language_rows(&page).is_empty());
        crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
    }

    #[test]
    fn language_picker_rows_keep_labels_and_selection_aligned_after_switching() {
        let _locale_guard = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let page = GitHubSettingsPage::new().expect("settings page");
        page.set_viewport_width(550.0);
        page.set_viewport_height(600.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        crate::i18n::apply_preference("en").expect("select English");
        sync_language_state(&page, "en");

        let page_weak = page.as_weak();
        SettingsAppState::get(&page).on_set_language(move |preference| {
            let canonical = crate::i18n::apply_preference(preference.as_str())
                .expect("select requested language");
            if let Some(page) = page_weak.upgrade() {
                sync_language_state(&page, canonical);
            }
        });

        open_language_picker(&page);
        assert_eq!(
            language_rows(&page),
            expected_language_rows("System default", "English")
        );

        select_language(&page, "简体中文");
        open_language_picker(&page);
        assert_eq!(
            language_rows(&page),
            expected_language_rows("跟随系统", "简体中文")
        );

        select_language(&page, "跟随系统");
        let system_label = usher_i18n::translate("language.system_default").into_owned();
        open_language_picker(&page);
        assert_eq!(
            language_rows(&page),
            expected_language_rows(system_label.as_str(), system_label.as_str())
        );

        select_language(&page, system_label.as_str());
        open_language_picker(&page);
        select_language(&page, "English");
        open_language_picker(&page);
        assert_eq!(
            language_rows(&page),
            expected_language_rows("System default", "English")
        );

        select_language(&page, "简体中文");
        let recreated = GitHubSettingsPage::new().expect("recreated settings page");
        recreated.set_viewport_width(550.0);
        recreated.set_viewport_height(600.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        sync_language_state(&recreated, "zh-CN");
        open_language_picker(&recreated);
        assert_eq!(
            language_rows(&recreated),
            expected_language_rows("跟随系统", "简体中文")
        );

        crate::i18n::apply_preference(usher_i18n::fallback_locale().id)
            .expect("restore fallback language");
    }

    #[test]
    fn language_and_section_menus_stay_aligned_in_the_stable_factory() {
        let _locale_guard = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let window = MainWindow::new().expect("main window");
        let bridge = SettingsPageBridge::default();
        crate::i18n::apply_preference("zh-CN").expect("select Chinese");
        crate::i18n::sync_main_window(&window, "zh-CN");
        bridge.install(&window);

        let window_weak = window.as_weak();
        let bridge_for_language = bridge.clone();
        AppState::get(&window).on_set_language(move |preference| {
            let window = window_weak.upgrade().expect("main window");
            let canonical = crate::i18n::apply_preference(preference.as_str())
                .expect("select requested language");
            crate::i18n::sync_main_window(&window, canonical);
            bridge_for_language.sync_language(canonical);
        });

        window.set_current_route(Route::Settings);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        let page = mounted_page(&bridge);
        page.set_viewport_width(550.0);
        page.set_viewport_height(600.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        assert_eq!(section_picker_trigger_label(&page), "通用");
        open_language_picker(&page);
        assert_eq!(
            language_rows(&page),
            expected_language_rows("跟随系统", "简体中文")
        );

        window.invoke_post_dispatch_pointer_released(0.0, 0.0);
        assert!(
            language_rows(&page).is_empty(),
            "window chrome dismisses factory menu"
        );
        open_language_picker(&page);
        window.invoke_dismiss_transient_overlays();
        assert!(
            language_rows(&page).is_empty(),
            "native title-bar dismissal reaches factory"
        );
        open_language_picker(&page);

        select_language(&page, "English");
        open_section_picker(&page);
        for label in [
            "General",
            "Code hosting",
            "AI Agent",
            "AI Review",
            "Diagnostics",
        ] {
            assert!(
                ElementHandle::find_by_accessible_label(&page, label)
                    .any(|element| element.accessible_checked().is_some()),
                "English section menu must contain {label:?}"
            );
        }
        open_section_picker(&page);
        open_language_picker(&page);
        assert_eq!(
            language_rows(&page),
            expected_language_rows("System default", "English")
        );
        select_language(&page, "简体中文");
        open_section_picker(&page);
        select_settings_section(&page, "AI Agent");
        assert_eq!(section_picker_trigger_label(&page), "AI Agent");
        let stable_page = page.as_weak();
        drop(page);
        window.set_current_route(Route::Home);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        assert!(
            stable_page.upgrade().is_some(),
            "Settings factory product must remain mounted while hidden"
        );
        window.set_current_route(Route::Settings);
        bridge.reset_drafts_from_committed(&window);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));

        let page = stable_page.upgrade().expect("stable Settings page");
        page.set_viewport_width(550.0);
        page.set_viewport_height(600.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        assert_eq!(section_picker_trigger_label(&page), "AI Agent");
        open_section_picker(&page);
        for label in ["通用", "代码托管", "AI Agent", "AI Review", "诊断"] {
            assert!(
                ElementHandle::find_by_accessible_label(&page, label)
                    .any(|element| element.accessible_checked().is_some()),
                "stable section menu must contain {label:?}"
            );
        }
        select_settings_section(&page, "通用");
        open_language_picker(&page);
        assert_eq!(
            language_rows(&page),
            expected_language_rows("跟随系统", "简体中文")
        );

        crate::i18n::apply_preference(usher_i18n::fallback_locale().id)
            .expect("restore fallback language");
    }

    #[test]
    fn ai_reply_language_draft_resets_in_the_stable_factory() {
        let _locale_guard = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let window = MainWindow::new().expect("main window");
        let bridge = SettingsPageBridge::default();
        let ai_agent_settings = crate::ai::settings_state::AiAgentSettingsFacade::memory(
            crate::settings::AiAgentSettings::default(),
        );
        bridge.configure_ai_agent_settings(ai_agent_settings.clone());
        crate::i18n::apply_preference("zh-CN").expect("select Chinese");
        crate::i18n::sync_main_window(&window, "zh-CN");
        crate::i18n::sync_ai_reply_language(&window, usher_i18n::AUTO_LANGUAGE);
        bridge.install(&window);

        window.set_current_route(Route::Settings);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        let page = mounted_page(&bridge);
        page.set_viewport_width(1200.0);
        page.set_viewport_height(768.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        select_settings_section(&page, "AI Agent");
        assert_eq!(
            SettingsAppState::get(&page)
                .get_ai_agent_reply_language_selection()
                .id,
            usher_i18n::AUTO_LANGUAGE
        );
        open_language_picker(&page);
        assert_eq!(
            ai_reply_language_rows(&page),
            expected_language_rows("跟随界面语言", "跟随界面语言")
        );

        select_language(&page, "English");
        assert_eq!(ai_reply_language_trigger_label(&page), "English");
        assert_eq!(
            ai_agent_settings.snapshot().state().reply_language,
            usher_i18n::AUTO_LANGUAGE
        );
        open_language_picker(&page);
        assert_eq!(
            ai_reply_language_rows(&page),
            expected_language_rows("跟随界面语言", "English")
        );
        let stable_page = page.as_weak();
        drop(page);
        window.set_current_route(Route::Home);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        assert!(stable_page.upgrade().is_some());
        window.set_current_route(Route::Settings);
        bridge.reset_drafts_from_committed(&window);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));

        let page = stable_page.upgrade().expect("stable Settings page");
        page.set_viewport_width(1200.0);
        page.set_viewport_height(768.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        select_settings_section(&page, "AI Agent");
        assert_eq!(
            SettingsAppState::get(&page)
                .get_ai_agent_reply_language_selection()
                .id,
            usher_i18n::AUTO_LANGUAGE
        );
        open_language_picker(&page);
        assert_eq!(
            ai_reply_language_rows(&page),
            expected_language_rows("跟随界面语言", "跟随界面语言")
        );
        select_language(&page, "English");
        save_ai_agent_draft(&page);
        drop(page);
        window.set_current_route(Route::Home);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        window.set_current_route(Route::Settings);
        bridge.reset_drafts_from_committed(&window);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));

        let page = mounted_page(&bridge);
        page.set_viewport_width(1200.0);
        page.set_viewport_height(768.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        select_settings_section(&page, "AI Agent");
        assert_eq!(ai_reply_language_trigger_label(&page), "English");
        open_language_picker(&page);
        assert_eq!(
            ai_reply_language_rows(&page),
            expected_language_rows("跟随界面语言", "English")
        );

        ai_agent_settings.shutdown();
        crate::i18n::apply_preference(usher_i18n::fallback_locale().id)
            .expect("restore fallback language");
    }

    #[test]
    fn ai_agent_draft_is_discarded_until_save_in_the_stable_factory() {
        let _locale_guard = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let window = MainWindow::new().expect("main window");
        crate::i18n::apply_preference("en").expect("select English");
        crate::i18n::sync_main_window(&window, "en");
        let bridge = SettingsPageBridge::default();
        let ai_agent_settings = crate::ai::settings_state::AiAgentSettingsFacade::memory(
            crate::settings::AiAgentSettings {
                provider: "deepseek".into(),
                base_url: "https://api.deepseek.com".into(),
                api_key: "secret".into(),
                model: "deepseek-v4-flash".into(),
                reply_language: "auto".into(),
            },
        );
        bridge.configure_ai_agent_settings(ai_agent_settings.clone());
        bridge.install(&window);

        window.set_current_route(Route::Settings);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        let page = mounted_page(&bridge);
        page.set_viewport_width(1200.0);
        page.set_viewport_height(768.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        select_settings_section(&page, "AI Agent");
        select_ai_agent_provider(&page, "OpenAI");

        let state = SettingsAppState::get(&page);
        assert_eq!(state.get_ai_agent_provider(), "openai");
        assert_eq!(
            settings_field_value(&page, "Base URL"),
            "https://api.openai.com/v1"
        );
        assert_eq!(settings_field_value(&page, "Model"), "gpt-4o-mini");
        assert!(
            ElementHandle::find_by_accessible_label(&page, "OpenAI")
                .any(|element| element.accessible_checked() == Some(true))
        );
        assert_eq!(
            ai_agent_settings.snapshot().state().provider,
            "deepseek",
            "editing the page must not commit the Store"
        );
        state.set_ai_agent_base_url("https://draft.invalid/v1".into());
        state.invoke_set_ai_agent_base_url("https://draft.invalid/v1".into());
        state.set_ai_agent_api_key("draft-secret".into());
        state.invoke_set_ai_agent_api_key("draft-secret".into());
        state.set_ai_agent_model("draft-model".into());
        state.invoke_set_ai_agent_model("draft-model".into());
        state.invoke_select_ai_agent_reply_language("zh-CN".into());
        assert_eq!(state.get_ai_agent_provider(), "custom");
        assert_eq!(
            ai_agent_settings.snapshot().state().provider,
            "deepseek",
            "editing every field must remain a page-local draft"
        );

        let stable_page = page.as_weak();
        drop(page);
        window.set_current_route(Route::Home);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        assert!(stable_page.upgrade().is_some());
        window.set_current_route(Route::Settings);
        bridge.reset_drafts_from_committed(&window);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        let page = stable_page.upgrade().expect("stable Settings page");
        page.set_viewport_width(1200.0);
        page.set_viewport_height(768.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        select_settings_section(&page, "AI Agent");

        assert_eq!(
            SettingsAppState::get(&page).get_ai_agent_provider(),
            "deepseek"
        );
        assert_eq!(
            settings_field_value(&page, "Base URL"),
            "https://api.deepseek.com"
        );
        assert_eq!(settings_field_value(&page, "Model"), "deepseek-v4-flash");
        assert_eq!(
            SettingsAppState::get(&page).get_ai_agent_api_key(),
            "secret"
        );
        assert_eq!(
            SettingsAppState::get(&page)
                .get_ai_agent_reply_language_selection()
                .id,
            usher_i18n::AUTO_LANGUAGE
        );

        select_ai_agent_provider(&page, "OpenAI");
        save_ai_agent_draft(&page);
        assert_eq!(ai_agent_settings.snapshot().state().provider, "openai");
        drop(page);
        window.set_current_route(Route::Home);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        window.set_current_route(Route::Settings);
        bridge.reset_drafts_from_committed(&window);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        let page = mounted_page(&bridge);
        page.set_viewport_width(1200.0);
        page.set_viewport_height(768.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        select_settings_section(&page, "AI Agent");

        assert_eq!(
            SettingsAppState::get(&page).get_ai_agent_provider(),
            "openai"
        );
        assert_eq!(
            settings_field_value(&page, "Base URL"),
            "https://api.openai.com/v1"
        );
        assert_eq!(settings_field_value(&page, "Model"), "gpt-4o-mini");

        ai_agent_settings.shutdown();
        crate::i18n::apply_preference(usher_i18n::fallback_locale().id)
            .expect("restore fallback language");
    }

    #[test]
    fn unsaved_code_hosting_drafts_are_discarded_in_the_stable_factory() {
        let _locale_guard = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let window = MainWindow::new().expect("main window");
        crate::i18n::apply_preference("en").expect("select English");
        let bridge = SettingsPageBridge::default();
        let committed = AppState::get(&window);
        committed.set_git_github_token("saved-github".into());
        committed.set_git_gitlab_token("saved-gitlab".into());
        committed.set_git_gitee_token("saved-gitee".into());
        committed.set_git_gitee_enterprise_token("saved-gitee-enterprise".into());
        committed.set_git_github_api_base_url("https://github.saved/api/v3".into());
        committed.set_git_github_enterprise("github-saved".into());
        committed.set_git_gitlab_api_base_url("https://gitlab.saved/api/v4".into());
        committed.set_git_gitlab_enterprise("gitlab-saved".into());
        committed.set_git_gitee_api_base_url("https://gitee.saved/api/v8".into());
        committed.set_git_gitee_enterprise("gitee-saved".into());
        bridge.install(&window);

        window.set_current_route(Route::Settings);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        let page = mounted_page(&bridge);
        page.set_viewport_width(1200.0);
        page.set_viewport_height(768.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        select_settings_section(&page, "Code hosting");

        let github_token_input = ElementHandle::find_by_accessible_label(&page, "GitHub token")
            .find(|element| element.accessible_value().is_some())
            .expect("GitHub token input");
        github_token_input.mock_single_click(PointerEventButton::Left);
        github_token_input.set_accessible_value("draft-github");
        let draft = SettingsAppState::get(&page);
        assert_eq!(draft.get_git_github_token(), "draft-github");
        draft.set_git_gitlab_token("draft-gitlab".into());
        draft.set_git_gitee_token("draft-gitee".into());
        draft.set_git_gitee_enterprise_token("draft-gitee-enterprise".into());
        draft.set_git_github_api_base_url("https://github.draft/api/v3".into());
        draft.set_git_github_enterprise("github-draft".into());
        draft.set_git_gitlab_api_base_url("https://gitlab.draft/api/v4".into());
        draft.set_git_gitlab_enterprise("gitlab-draft".into());
        draft.set_git_gitee_api_base_url("https://gitee.draft/api/v8".into());
        draft.set_git_gitee_enterprise("gitee-draft".into());

        let stable_page = page.as_weak();
        drop(page);
        window.set_current_route(Route::Home);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        assert!(stable_page.upgrade().is_some());
        window.set_current_route(Route::Settings);
        bridge.reset_drafts_from_committed(&window);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        let page = stable_page.upgrade().expect("stable Settings page");
        let restored = SettingsAppState::get(&page);

        assert_eq!(restored.get_git_github_token(), "saved-github");
        assert_eq!(restored.get_git_gitlab_token(), "saved-gitlab");
        assert_eq!(restored.get_git_gitee_token(), "saved-gitee");
        assert_eq!(
            restored.get_git_gitee_enterprise_token(),
            "saved-gitee-enterprise"
        );
        assert_eq!(
            restored.get_git_github_api_base_url(),
            "https://github.saved/api/v3"
        );
        assert_eq!(restored.get_git_github_enterprise(), "github-saved");
        assert_eq!(
            restored.get_git_gitlab_api_base_url(),
            "https://gitlab.saved/api/v4"
        );
        assert_eq!(restored.get_git_gitlab_enterprise(), "gitlab-saved");
        assert_eq!(
            restored.get_git_gitee_api_base_url(),
            "https://gitee.saved/api/v8"
        );
        assert_eq!(restored.get_git_gitee_enterprise(), "gitee-saved");
    }

    #[test]
    fn preview_state_and_callbacks_cross_the_settings_factory_boundary() {
        i_slint_backend_testing::init_no_event_loop();
        let window = MainWindow::new().expect("main window");
        let page = GitHubSettingsPage::new().expect("settings page");
        let window_state = AppState::get(&window);

        window_state.set_artifact_preview_presentation_mode(1);
        window_state.set_artifact_preview_markdown_enabled(false);
        window_state.set_artifact_preview_image_enabled(true);
        window_state.set_artifact_preview_plain_text_enabled(false);
        sync_state_from_window(&window, &page);

        let page_state = SettingsAppState::get(&page);
        assert_eq!(page_state.get_artifact_preview_presentation_mode(), 1);
        assert!(!page_state.get_artifact_preview_markdown_enabled());
        assert!(page_state.get_artifact_preview_image_enabled());
        assert!(!page_state.get_artifact_preview_plain_text_enabled());

        let forwarded_mode = Rc::new(Cell::new(-1));
        let observed_mode = forwarded_mode.clone();
        window_state.on_set_artifact_preview_presentation_mode(move |mode| {
            observed_mode.set(mode);
        });
        let forwarded_provider = Rc::new(RefCell::new(None));
        let observed_provider = forwarded_provider.clone();
        window_state.on_set_artifact_preview_provider_enabled(move |provider_id, enabled| {
            *observed_provider.borrow_mut() = Some((provider_id.to_string(), enabled));
        });

        bind_callbacks(&window, &page, None);
        window_state.set_artifact_preview_presentation_mode(0);
        page_state.set_artifact_preview_presentation_mode(0);
        page_state.invoke_set_artifact_preview_presentation_mode(1);
        page_state.invoke_set_artifact_preview_provider_enabled(IMAGE_PROVIDER_ID.into(), false);

        assert_eq!(forwarded_mode.get(), 1);
        assert_eq!(page_state.get_artifact_preview_presentation_mode(), 1);
        assert_eq!(window_state.get_artifact_preview_presentation_mode(), 1);
        assert!(!page_state.get_artifact_preview_image_enabled());
        assert!(!window_state.get_artifact_preview_image_enabled());
        assert_eq!(
            forwarded_provider.borrow().as_ref(),
            Some(&(IMAGE_PROVIDER_ID.to_owned(), false))
        );
    }

    #[test]
    fn ai_reply_language_crosses_the_settings_factory_boundary() {
        let _locale_guard = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        usher_i18n::activate_locale("en");
        let window = MainWindow::new().expect("main window");
        crate::i18n::sync_ai_reply_language(&window, "zh-CN");
        let page = GitHubSettingsPage::new().expect("settings page");
        sync_state_from_window(&window, &page);

        let page_state = SettingsAppState::get(&page);
        assert_eq!(
            page_state.get_ai_agent_reply_language_selection().id,
            "zh-CN"
        );
        let forwarded = Rc::new(RefCell::new(String::new()));
        let observed = forwarded.clone();
        AppState::get(&window).on_save_ai_agent_settings(move |_, _, _, _, reply_language| {
            *observed.borrow_mut() = reply_language.to_string();
        });
        bind_callbacks(&window, &page, None);
        page_state.invoke_save_ai_agent_settings(
            "custom".into(),
            "https://example.invalid/v1".into(),
            "secret".into(),
            "model".into(),
            "en".into(),
        );

        assert_eq!(forwarded.borrow().as_str(), "en");
        assert_eq!(
            AppState::get(&window)
                .get_ai_agent_reply_language_selection()
                .id,
            "en"
        );
        usher_i18n::activate_locale(usher_i18n::fallback_locale().id);
    }

    #[cfg(feature = "ai-code-review")]
    #[test]
    fn concurrency_updates_preserve_rule_model_identity_and_order() {
        use std::path::PathBuf;

        use usher_ai_review::{ReviewConfigurationState, ReviewRule, RuleId};

        i_slint_backend_testing::init_no_event_loop();
        let page = GitHubSettingsPage::new().expect("settings page");
        let mut state = ReviewConfigurationState {
            rules: vec![
                ReviewRule {
                    id: RuleId("a".into()),
                    name: "Alpha".into(),
                    markdown_path: PathBuf::from("/alpha.md"),
                },
                ReviewRule {
                    id: RuleId("b".into()),
                    name: "Beta".into(),
                    markdown_path: PathBuf::from("/beta.md"),
                },
            ],
            default_rule_id: Some(RuleId("a".into())),
            ..Default::default()
        };

        apply_ai_review_snapshot(&page, &state);
        let initial_model = page.get_ai_review_rules();
        let initial_ids = initial_model
            .iter()
            .map(|rule| rule.id.to_string())
            .collect::<Vec<_>>();

        state.max_concurrent_tasks = 6;
        apply_ai_review_snapshot(&page, &state);
        let concurrency_model = page.get_ai_review_rules();
        assert_eq!(initial_model, concurrency_model);
        assert_eq!(
            concurrency_model
                .iter()
                .map(|rule| rule.id.to_string())
                .collect::<Vec<_>>(),
            initial_ids
        );

        state.default_rule_id = Some(RuleId("b".into()));
        apply_ai_review_snapshot(&page, &state);
        let default_model = page.get_ai_review_rules();
        assert_eq!(initial_model, default_model);
        assert!(!default_model.row_data(0).expect("Alpha rule").is_default);
        assert!(default_model.row_data(1).expect("Beta rule").is_default);
    }
}
