//! Saved-profile probes owned by the visible Settings Agent list.

use slint::{ComponentHandle, Global};
use std::sync::Arc;
use usher_ui_settings::{AppState, GitHubSettingsPage};

use super::super::settings_state::AiAgentSettingsFacade;
use super::connection_probe::{Probe, feedback_key};
use crate::settings::AiAgentProfile;

#[derive(Clone)]
struct Binding {
    page: slint::Weak<GitHubSettingsPage>,
    facade: AiAgentSettingsFacade,
    probe: Arc<Probe>,
}

struct Request {
    ticket: u64,
    profile: AiAgentProfile,
}

pub(super) fn bind(page: &GitHubSettingsPage, facade: AiAgentSettingsFacade) {
    let binding = Binding {
        page: page.as_weak(),
        facade,
        probe: Arc::default(),
    };
    let cancel = binding.clone();
    AppState::get(page).on_cancel_ai_agent_list_test(move || cancel.cancel());
    AppState::get(page).on_test_ai_agent(move |id| binding.start(id.as_str()));
}

impl Binding {
    fn cancel(&self) {
        self.probe.cancel();
        if let Some(page) = self.page.upgrade() {
            let state = AppState::get(&page);
            state.set_ai_agent_list_testing_id("".into());
            state.set_ai_agent_list_feedback_open(false);
            state.set_ai_agent_list_feedback_key("".into());
            state.set_ai_agent_list_feedback("".into());
        }
    }

    fn start(&self, id: &str) {
        let Some(page) = self.page.upgrade() else {
            return;
        };
        let state = AppState::get(&page);
        if !page.get_ai_agent_list_active() || !state.get_ai_agent_list_testing_id().is_empty() {
            return;
        }
        let Some(profile) = self.facade.snapshot().state().profile(id).cloned() else {
            return;
        };
        let request = Request {
            ticket: self.probe.next_ticket(),
            profile,
        };
        state.set_ai_agent_list_testing_id(id.into());
        state.set_ai_agent_list_feedback_open(false);
        state.set_ai_agent_list_feedback_key("".into());
        state.set_ai_agent_list_feedback("".into());
        let page = self.page.clone();
        let facade = self.facade.clone();
        let completed = Arc::downgrade(&self.probe);
        let task = crate::app_runtime::shared_runtime().spawn(async move {
            let result = crate::ai::agent::test_connection(&request.profile.connection).await;
            let success = result.is_ok();
            let key = feedback_key(&result);
            let _ = slint::invoke_from_event_loop(move || {
                let Some(probe) = completed.upgrade() else {
                    return;
                };
                Binding {
                    page,
                    facade,
                    probe,
                }
                .complete(request, success, key);
            });
        });
        self.probe.attach(task.abort_handle());
    }

    fn complete(&self, request: Request, success: bool, key: &'static str) {
        if !self.probe.finish(request.ticket) {
            return;
        }
        let Some(page) = self.page.upgrade() else {
            return;
        };
        let state = AppState::get(&page);
        state.set_ai_agent_list_testing_id("".into());
        if !page.get_ai_agent_list_active()
            || self.facade.snapshot().state().profile(&request.profile.id) != Some(&request.profile)
        {
            return;
        }
        state.set_ai_agent_list_feedback_success(success);
        state.set_ai_agent_list_feedback_key(key.into());
        state.set_ai_agent_list_feedback(crate::i18n::tr(key).into_owned().into());
        state.set_ai_agent_list_feedback_open(true);
    }
}

#[cfg(test)]
#[path = "list_connection_test_tests.rs"]
mod tests;
