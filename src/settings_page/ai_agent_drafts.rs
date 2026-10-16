//! Visit-local list controls and one explicit editor draft, isolated from activation.

use super::ai_agent_projection::{apply_ai_agent_choices_to_page, apply_ai_agent_profile_to_page};
use crate::ai::settings_state::AiAgentSettingsFacade;
use crate::settings::{AiAgentConfiguration, AiAgentProfile};
use slint::{ComponentHandle, Global};
use usher_ui_settings::{AppState, GitHubSettingsPage};

fn snapshot(facade: &Option<AiAgentSettingsFacade>) -> AiAgentConfiguration {
    facade
        .as_ref()
        .map(|state| state.snapshot().state().clone())
        .unwrap_or_default()
}

fn open(page: &GitHubSettingsPage, profile: AiAgentProfile, creating: bool) {
    let state = AppState::get(page);
    state.invoke_cancel_ai_agent_test();
    state.set_ai_agent_editor_new(creating);
    state.set_ai_agent_editor_revision(state.get_ai_agent_editor_revision().wrapping_add(1));
    state.set_ai_agent_editor_feedback_open(false);
    state.set_ai_agent_editor_error_key("".into());
    state.set_ai_agent_editor_error("".into());
    state.set_ai_agent_editor_test_result_key("".into());
    state.set_ai_agent_editor_test_result("".into());
    state.set_ai_agent_editor_busy(false);
    state.set_ai_agent_editor_testing(false);
    apply_ai_agent_profile_to_page(page, &profile);
    state.set_ai_agent_editor_open(true);
}

pub(crate) fn close(page: &GitHubSettingsPage) {
    let state = AppState::get(page);
    state.invoke_cancel_ai_agent_test();
    state.set_ai_agent_editor_open(false);
    state.set_ai_agent_editor_feedback_open(false);
    state.set_ai_agent_editor_revision(state.get_ai_agent_editor_revision().wrapping_add(1));
    state.set_ai_agent_editor_testing(false);
    state.set_ai_agent_editor_busy(false);
    // Do not retain unsaved credentials after the dialog is dismissed.
    apply_ai_agent_profile_to_page(page, &AiAgentProfile::default());
}

pub(crate) fn bind(page: &GitHubSettingsPage, facade: Option<AiAgentSettingsFacade>) {
    let weak = page.as_weak();
    AppState::get(page).on_open_new_ai_agent(move || {
        if let Some(page) = weak.upgrade() {
            open(
                &page,
                AiAgentProfile {
                    id: AiAgentProfile::new_id(),
                    ..Default::default()
                },
                true,
            );
        }
    });
    let weak = page.as_weak();
    let saved = facade.clone();
    AppState::get(page).on_edit_ai_agent(move |id| {
        if let Some(page) = weak.upgrade() {
            if let Some(profile) = snapshot(&saved).profile(id.as_str()) {
                open(&page, profile.clone(), false);
            }
        }
    });
    let weak = page.as_weak();
    AppState::get(page).on_cancel_ai_agent_editor(move || {
        if let Some(page) = weak.upgrade() {
            close(&page);
        }
    });
    let weak = page.as_weak();
    let saved = facade.clone();
    AppState::get(page).on_query_ai_agents(move |query| {
        if let Some(page) = weak.upgrade() {
            let state = AppState::get(&page);
            state.set_ai_agent_query(query);
            state.set_ai_agent_page(1);
            apply_ai_agent_choices_to_page(&page, &snapshot(&saved));
        }
    });
    let weak = page.as_weak();
    let saved = facade;
    AppState::get(page).on_select_ai_agents_page(move |selected| {
        if let Some(page) = weak.upgrade() {
            AppState::get(&page).set_ai_agent_page(selected);
            apply_ai_agent_choices_to_page(&page, &snapshot(&saved));
        }
    });
    // Provider selection is an explicit edit within this dialog. Typing fields
    // never infers a Provider or changes another saved Agent's configuration.
    let weak = page.as_weak();
    AppState::get(page).on_select_ai_agent_provider(move |provider| {
        if let Some(page) = weak.upgrade() {
            let state = AppState::get(&page);
            state.set_ai_agent_editor_provider(provider.clone());
            let preset = match provider.as_str() {
                "deepseek" => Some("https://api.deepseek.com"),
                "openai" => Some("https://api.openai.com/v1"),
                _ => None,
            };
            let current = state.get_ai_agent_editor_base_url();
            let current = current.trim().trim_end_matches('/');
            if let Some(preset) = preset {
                if current.is_empty()
                    || matches!(
                        current,
                        "https://api.deepseek.com" | "https://api.openai.com/v1"
                    )
                {
                    state.set_ai_agent_editor_base_url(preset.into());
                }
            }
            state.invoke_cancel_ai_agent_test();
            state.set_ai_agent_editor_feedback_open(false);
            state.set_ai_agent_editor_error_key("".into());
            state.set_ai_agent_editor_error("".into());
            state.set_ai_agent_editor_test_result_key("".into());
            state.set_ai_agent_editor_test_result("".into());
        }
    });
    super::ai_agent_header_drafts::bind(page);
}
