//! Named deletion confirmation captures a stable Agent identity, never a row index.

use super::{Binding, UiAction};
use crate::i18n;
use slint::{ComponentHandle, Global};
use usher_ui_settings::{AppState as PageState, GitHubSettingsPage};

pub(super) fn bind(page: &GitHubSettingsPage, binding: Binding) {
    let deleting = binding.clone();
    PageState::get(page).on_delete_ai_agent(move |id| {
        let Some(page) = deleting.page.upgrade() else {
            return;
        };
        let snapshot = deleting.facade.snapshot();
        let Some(profile) = snapshot.state().profile(id.as_str()) else {
            return;
        };
        if snapshot.state().active_id == profile.id {
            return;
        }
        let state = PageState::get(&page);
        state.set_ai_agent_delete_id(profile.id.clone().into());
        let name = crate::ai::identity::display(&profile.name, &profile.connection.provider, "");
        state.set_ai_agent_delete_name(name.clone().into());
        state.set_ai_agent_delete_message(
            i18n::tr_args("settings.ai_agent.delete_confirmation", &[("name", &name)]).into(),
        );
        state.set_ai_agent_delete_confirm_open(true);
    });
    let deleting = binding.clone();
    PageState::get(page).on_confirm_delete_ai_agent(move || {
        let Some(page) = deleting.page.upgrade() else {
            return;
        };
        let state = PageState::get(&page);
        if !state.get_ai_agent_delete_confirm_open() {
            return;
        }
        let id = state.get_ai_agent_delete_id().to_string();
        state.set_ai_agent_delete_confirm_open(false);
        deleting.submit(UiAction::Delete(id));
    });
    let weak = page.as_weak();
    PageState::get(page).on_cancel_delete_ai_agent(move || {
        if let Some(page) = weak.upgrade() {
            PageState::get(&page).set_ai_agent_delete_confirm_open(false);
        }
    });
}
