//! Dialog-owned cancellable probes with immutable draft capture.

use super::connection_probe::{Probe, feedback_key};
use crate::settings_page::ai_agent_projection::validated_ai_agent_profile_from_page;
use slint::{ComponentHandle, Global};
use std::sync::Arc;
use usher_ui_settings::{AppState, GitHubSettingsPage};

pub(super) fn bind(page: &GitHubSettingsPage) {
    let probe = Arc::new(Probe::default());
    let cancel = probe.clone();
    let weak = page.as_weak();
    AppState::get(page).on_cancel_ai_agent_test(move || {
        cancel.cancel();
        if let Some(page) = weak.upgrade() {
            AppState::get(&page).set_ai_agent_editor_testing(false);
        }
    });
    let weak = page.as_weak();
    AppState::get(page).on_test_ai_agent_editor(move || {
        let Some(page) = weak.upgrade() else { return };
        let state = AppState::get(&page);
        if !page.get_route_active()
            || !state.get_ai_agent_editor_open()
            || state.get_ai_agent_editor_testing()
            || state.get_ai_agent_editor_busy()
        {
            return;
        }
        let revision = state.get_ai_agent_editor_revision();
        let Some(draft) = super::feedback::validated_draft(&page) else {
            return;
        };
        let header_revision = state.get_ai_agent_editor_headers_input_revision();
        let ticket = probe.next_ticket();
        state.set_ai_agent_editor_testing(true);
        state.set_ai_agent_editor_feedback_open(false);
        state.set_ai_agent_editor_error_key("".into());
        state.set_ai_agent_editor_error("".into());
        state.set_ai_agent_editor_test_result_key("".into());
        state.set_ai_agent_editor_test_result("".into());
        let weak = page.as_weak();
        let completed = Arc::downgrade(&probe);
        let task = crate::app_runtime::shared_runtime().spawn(async move {
            let result = crate::ai::agent::test_connection(&draft.connection).await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(probe) = completed.upgrade() else {
                    return;
                };
                if !probe.finish(ticket) {
                    return;
                }
                let Some(page) = weak.upgrade() else { return };
                let state = AppState::get(&page);
                if state.get_ai_agent_editor_revision() != revision {
                    return;
                }
                state.set_ai_agent_editor_testing(false);
                if !page.get_route_active()
                    || !state.get_ai_agent_editor_open()
                    || state.get_ai_agent_editor_headers_input_revision() != header_revision
                    || !validated_ai_agent_profile_from_page(&page)
                        .is_ok_and(|current| current == draft)
                {
                    return;
                }
                state.set_ai_agent_editor_test_success(result.is_ok());
                let key = feedback_key(&result);
                state.set_ai_agent_editor_feedback_open(true);
                state.set_ai_agent_editor_test_result_key(key.into());
                state.set_ai_agent_editor_test_result(crate::i18n::tr(key).into_owned().into());
            });
        });
        probe.attach(task.abort_handle());
    });
}
