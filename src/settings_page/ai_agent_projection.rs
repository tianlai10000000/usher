//! Settings list pagination and isolated editor projection. No durable mutations.

use crate::settings::{AiAgentConfiguration, AiAgentHeader, AiAgentProfile, AiAgentSettings};
use crate::{AppState, MainWindow};
use slint::{Global, Model, ModelRc, VecModel};
use usher_ui_settings::{AiAgentHeaderItem, AppState as PageState, GitHubSettingsPage};

pub(crate) use super::ai_agent_list::apply_ai_agent_choices_to_page;

pub(super) fn apply_ai_agent_profile_to_page(page: &GitHubSettingsPage, profile: &AiAgentProfile) {
    let state = PageState::get(page);
    let connection = &profile.connection;
    state.set_ai_agent_editor_headers_object_mode(false);
    state.set_ai_agent_editor_headers_object_text("".into());
    state.set_ai_agent_editor_headers_input_revision(
        state
            .get_ai_agent_editor_headers_input_revision()
            .wrapping_add(1),
    );
    state.set_ai_agent_editor_id(profile.id.clone().into());
    state.set_ai_agent_editor_name(profile.name.clone().into());
    state.set_ai_agent_editor_provider(connection.provider.clone().into());
    state.set_ai_agent_editor_base_url(connection.base_url.clone().into());
    state.set_ai_agent_editor_api_key(connection.api_key.clone().into());
    state.set_ai_agent_editor_model(connection.model.clone().into());
    state.set_ai_agent_editor_reply_language(connection.reply_language.clone().into());
    super::sync_ai_reply_language_state(page, &connection.reply_language);
    state.set_ai_agent_editor_headers(ModelRc::new(VecModel::from(
        connection
            .headers
            .iter()
            .map(|header| AiAgentHeaderItem {
                enabled: header.enabled,
                name: header.name.clone().into(),
                value: header.value.clone().into(),
            })
            .collect::<Vec<_>>(),
    )));
}

pub(crate) fn restore_ai_agent_configuration_to_page(
    page: &GitHubSettingsPage,
    config: &AiAgentConfiguration,
) {
    let state = PageState::get(page);
    state.set_ai_agent_delete_confirm_open(false);
    super::ai_agent_drafts::close(page);
    state.set_ai_agent_editor_error("".into());
    state.set_ai_agent_editor_test_result("".into());
    apply_ai_agent_choices_to_page(page, config);
}

pub(super) fn ai_agent_draft_from_page(page: &GitHubSettingsPage) -> AiAgentSettings {
    let state = PageState::get(page);
    AiAgentSettings {
        provider: state.get_ai_agent_editor_provider().to_string(),
        base_url: state.get_ai_agent_editor_base_url().to_string(),
        api_key: state.get_ai_agent_editor_api_key().to_string(),
        model: state.get_ai_agent_editor_model().to_string(),
        reply_language: state.get_ai_agent_editor_reply_language().to_string(),
        headers: state
            .get_ai_agent_editor_headers()
            .iter()
            .map(|header| AiAgentHeader {
                enabled: header.enabled,
                name: header.name.to_string(),
                value: header.value.to_string(),
            })
            .collect(),
    }
}

pub(crate) fn ai_agent_profile_from_page(page: &GitHubSettingsPage) -> AiAgentProfile {
    let state = PageState::get(page);
    AiAgentProfile {
        id: state.get_ai_agent_editor_id().to_string(),
        name: state.get_ai_agent_editor_name().to_string(),
        connection: ai_agent_draft_from_page(page),
    }
}

pub(crate) fn validated_ai_agent_profile_from_page(
    page: &GitHubSettingsPage,
) -> Result<AiAgentProfile, &'static str> {
    let mut profile = ai_agent_profile_from_page(page);
    profile.connection.headers = super::ai_agent_header_drafts::normalized(page)?;
    Ok(profile)
}

pub(crate) fn apply_ai_agent_configuration_to_window(
    window: &MainWindow,
    config: &AiAgentConfiguration,
) {
    let state = AppState::get(window);
    let profile = config.active_profile().cloned().unwrap_or_default();
    let settings = &profile.connection;
    state.set_ai_agent_active_provider(config.active_id.clone().into());
    state.set_ai_agent_name(profile.name.into());
    state.set_ai_agent_base_url(settings.base_url.clone().into());
    state.set_ai_agent_api_key(settings.api_key.clone().into());
    state.set_ai_agent_model(settings.model.clone().into());
    state.set_ai_agent_provider(settings.provider.clone().into());
    crate::i18n::sync_ai_reply_language(window, &settings.reply_language);
}
