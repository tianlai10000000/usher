//! Semantic, locale-refreshable transient Agent feedback.
use slint::Global;
use usher_ui_settings::{AppState, GitHubSettingsPage};

pub(super) fn error(page: &GitHubSettingsPage, key: &str) {
    let state = AppState::get(page);
    state.set_ai_agent_editor_error_key(key.into());
    state.set_ai_agent_editor_error(crate::i18n::tr(key).into_owned().into());
    state.set_ai_agent_editor_feedback_open(true);
}

pub(super) fn validated_draft(
    page: &GitHubSettingsPage,
) -> Option<crate::settings::AiAgentProfile> {
    match crate::settings_page::ai_agent_projection::validated_ai_agent_profile_from_page(page) {
        Ok(draft) => Some(draft),
        Err(key) => {
            error(page, key);
            None
        }
    }
}

pub(super) fn refresh(page: &GitHubSettingsPage) {
    let state = AppState::get(page);
    let error_key = state.get_ai_agent_editor_error_key();
    if !error_key.is_empty() {
        state.set_ai_agent_editor_error(crate::i18n::tr(error_key.as_str()).into_owned().into());
    }
    let test_key = state.get_ai_agent_editor_test_result_key();
    if !test_key.is_empty() {
        state.set_ai_agent_editor_test_result(
            crate::i18n::tr(test_key.as_str()).into_owned().into(),
        );
    }
    let list_key = state.get_ai_agent_list_feedback_key();
    if !list_key.is_empty() {
        state.set_ai_agent_list_feedback(crate::i18n::tr(list_key.as_str()).into_owned().into());
    }
}
