//! Ordered Agent CRUD/activation admission and isolated dialog completion.

use slint::{ComponentHandle, Global};
use std::sync::{Arc, atomic::Ordering};
use usher_ui_settings::{AppState as PageState, GitHubSettingsPage};

mod admission;
mod connection_probe;
mod connection_test;
mod delete_confirmation;
mod feedback;
mod list_connection_test;

pub(crate) fn sync_language(page: &GitHubSettingsPage) {
    feedback::refresh(page);
}
use super::settings_state::{AiAgentSaveOutcome, AiAgentSettingsFacade};
use crate::settings_feedback::settings_toast;
#[cfg(test)]
use crate::settings_page::ai_agent_projection::ai_agent_profile_from_page;
use crate::settings_page::ai_agent_projection::{
    apply_ai_agent_choices_to_page, validated_ai_agent_profile_from_page,
};
use crate::{MainWindow, ToastManager, app_runtime, i18n};
use admission::{SaveUiPermit, SaveUiState, UiAction};

struct UiRequest {
    _permit: SaveUiPermit,
    attempt: u64,
    request: u64,
    target_id: String,
    editor_revision: i32,
    header_input_revision: i32,
    action: UiAction,
}

#[derive(Clone)]
struct Binding {
    window: slint::Weak<MainWindow>,
    page: slint::Weak<GitHubSettingsPage>,
    state: Arc<SaveUiState>,
    facade: AiAgentSettingsFacade,
}

pub(crate) fn bind_page(
    window: &MainWindow,
    page: &GitHubSettingsPage,
    facade: AiAgentSettingsFacade,
) {
    let binding = Binding {
        window: window.as_weak(),
        page: page.as_weak(),
        state: Arc::default(),
        facade,
    };
    let save = binding.clone();
    PageState::get(page).on_save_ai_agent_editor(move || {
        let Some(page) = save.page.upgrade() else {
            return;
        };
        let state = PageState::get(&page);
        if state.get_ai_agent_editor_busy() {
            return;
        }
        let Some(draft) = feedback::validated_draft(&page) else {
            return;
        };
        if draft.name.trim().is_empty()
            || draft.connection.base_url.trim().is_empty()
            || draft.connection.model.trim().is_empty()
        {
            feedback::error(&page, "settings.feedback.ai_agent_invalid_editor");
            return;
        }
        save.submit(if state.get_ai_agent_editor_new() {
            UiAction::Create(draft)
        } else {
            UiAction::Update(draft)
        });
    });
    let activate = binding.clone();
    PageState::get(page).on_activate_ai_agent(move |id| {
        activate.submit(UiAction::Activate(id.to_string()));
    });
    list_connection_test::bind(page, binding.facade.clone());
    delete_confirmation::bind(page, binding);
    connection_test::bind(page);
}

impl Binding {
    fn submit(&self, action: UiAction) {
        let Some(page) = self.page.upgrade() else {
            return;
        };
        let admission = self.state.reserve(&action).and_then(|permit| {
            let Some(permit) = permit else {
                return Ok(None);
            };
            let receipt = match &action {
                UiAction::Create(draft) => self.facade.create_profile(draft.clone()),
                UiAction::Update(draft) => self.facade.update_profile(draft.clone()),
                UiAction::Delete(id) => self.facade.delete_profile(id),
                UiAction::Activate(id) => self.facade.activate_profile(id),
            }?;
            Ok(receipt.map(|receipt| (receipt, permit)))
        });
        let (receipt, permit) = match admission {
            Ok(Some(value)) => value,
            Ok(None) => return,
            Err(_) => {
                self.state.latest_attempt.fetch_add(1, Ordering::AcqRel);
                if matches!(action, UiAction::Create(_) | UiAction::Update(_)) {
                    feedback::error(&page, action.feedback(false));
                } else if let Some(window) = self.window.upgrade() {
                    ToastManager::get(&window)
                        .invoke_error(settings_toast(i18n::tr(action.feedback(false)).as_ref()));
                }
                return;
            }
        };
        let request = UiRequest {
            _permit: permit,
            request: receipt.id,
            target_id: receipt.profile_id.clone(),
            attempt: self.state.latest_attempt.fetch_add(1, Ordering::AcqRel) + 1,
            editor_revision: PageState::get(&page).get_ai_agent_editor_revision(),
            header_input_revision: PageState::get(&page)
                .get_ai_agent_editor_headers_input_revision(),
            action,
        };
        if matches!(request.action, UiAction::Create(_) | UiAction::Update(_)) {
            let state = PageState::get(&page);
            state.set_ai_agent_editor_busy(true);
            state.set_ai_agent_editor_error_key("".into());
            state.set_ai_agent_editor_error("".into());
        }
        let binding = self.clone();
        app_runtime::shared_runtime().spawn(async move {
            let result = tokio::task::spawn_blocking(move || receipt.wait())
                .await
                .unwrap_or_else(|_| Err("AI Agent command feedback worker stopped".into()));
            let _ = slint::invoke_from_event_loop(move || {
                binding.complete(request, result);
            });
        });
    }

    fn complete(&self, request: UiRequest, result: Result<AiAgentSaveOutcome, String>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        // Receipts can arrive out of order. Project the current Store snapshot,
        // retaining the command result only for its own draft/feedback ownership.
        if let Ok(outcome) = &result {
            log::debug!(
                "[AI Agent][Settings] Command {} completed at revision {}",
                request.request,
                outcome.snapshot.revision()
            );
        }
        let latest = self.facade.snapshot();
        crate::settings_page::apply_ai_agent_configuration_to_window(&window, latest.state());
        crate::chat::sync_agent_identity(&window);
        let Some(page) = self.page.upgrade() else {
            return;
        };
        apply_ai_agent_choices_to_page(&page, latest.state());
        let state = PageState::get(&page);
        let own_editor = state.get_ai_agent_editor_revision() == request.editor_revision
            && state.get_ai_agent_editor_id().as_str() == request.target_id;
        let saving = matches!(request.action, UiAction::Create(_) | UiAction::Update(_));
        let success = result
            .as_ref()
            .is_ok_and(|outcome| outcome.persisted.is_ok() && outcome.applied.is_ok());
        if saving && own_editor {
            state.set_ai_agent_editor_busy(false);
            if matches!(request.action, UiAction::Create(_))
                && result.as_ref().is_ok_and(|outcome| outcome.applied.is_ok())
            {
                state.set_ai_agent_editor_new(false);
            }
            let draft_matches = match &request.action {
                UiAction::Create(draft) | UiAction::Update(draft) => {
                    state.get_ai_agent_editor_headers_input_revision()
                        == request.header_input_revision
                        && validated_ai_agent_profile_from_page(&page)
                            .is_ok_and(|current| current == *draft)
                }
                _ => false,
            };
            if success && draft_matches {
                if matches!(request.action, UiAction::Create(_)) {
                    state.set_ai_agent_query("".into());
                    state.set_ai_agent_page(1);
                    apply_ai_agent_choices_to_page(&page, latest.state());
                }
                crate::settings_page::ai_agent_drafts::close(&page);
            } else if !success {
                feedback::error(&page, request.action.feedback(false));
            }
        }
        if self.state.latest_attempt.load(Ordering::Acquire) == request.attempt {
            let text = settings_toast(i18n::tr(request.action.feedback(success)).as_ref());
            if success {
                ToastManager::get(&window).invoke_success(text);
            } else {
                ToastManager::get(&window).invoke_error(text);
            }
        }
    }
}

#[cfg(test)]
#[path = "settings_ui_tests.rs"]
mod tests;
