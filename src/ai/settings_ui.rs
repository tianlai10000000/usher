//! AI Settings Save admission and guarded event-loop completion projection.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use slint::{ComponentHandle, Global, Timer};

use super::settings_state::{AiAgentSaveOutcome, AiAgentSettingsFacade};
use crate::settings_feedback::settings_toast;
use crate::settings_page::AiAgentSaveContext;
use crate::{AppState, MainWindow, SettingsPageBridge, ToastManager, app_runtime, i18n};

const UI_SAVE_CAPACITY: usize = 8;

#[derive(Default)]
struct SaveUiState {
    latest_attempt: AtomicU64,
    projected_request: AtomicU64,
    latest_ticket: AtomicU64,
    pending: Mutex<Vec<(u64, crate::settings::AiAgentSettings)>>,
}

struct SaveUiPermit {
    state: Arc<SaveUiState>,
    ticket: u64,
}

impl SaveUiState {
    fn reserve(
        self: &Arc<Self>,
        draft: &crate::settings::AiAgentSettings,
    ) -> Result<Option<SaveUiPermit>, String> {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if pending.last().is_some_and(|(ticket, value)| {
            *ticket == self.latest_ticket.load(Ordering::Acquire) && value == draft
        }) {
            return Ok(None);
        }
        if pending.len() >= UI_SAVE_CAPACITY {
            return Err("AI Agent settings UI completion queue is full".into());
        }
        let ticket = self.latest_ticket.fetch_add(1, Ordering::AcqRel) + 1;
        pending.push((ticket, draft.clone()));
        Ok(Some(SaveUiPermit {
            state: self.clone(),
            ticket,
        }))
    }
}

impl Drop for SaveUiPermit {
    fn drop(&mut self) {
        self.state
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|(ticket, _)| *ticket != self.ticket);
    }
}

struct SaveUiRequest {
    _permit: Option<SaveUiPermit>,
    attempt: u64,
    request: u64,
    context: Option<AiAgentSaveContext>,
    draft: crate::settings::AiAgentSettings,
}

pub(crate) fn bind(window: &MainWindow, page: &SettingsPageBridge, facade: AiAgentSettingsFacade) {
    let weak = window.as_weak();
    let page = page.clone();
    let state = Arc::new(SaveUiState::default());
    AppState::get(window).on_save_ai_agent_settings(
        move |provider, base_url, api_key, model, reply_language| {
            let draft = crate::settings::AiAgentSettings {
                provider: provider.to_string(),
                base_url: base_url.to_string(),
                api_key: api_key.to_string(),
                model: model.to_string(),
                reply_language: reply_language.to_string(),
            };
            let admission = state.reserve(&draft).and_then(|permit| {
                let Some(permit) = permit else {
                    return Ok(None);
                };
                facade
                    .save_draft(draft.clone())
                    .map(|receipt| receipt.map(|receipt| (receipt, permit)))
            });
            let (receipt, permit) = match admission {
                Ok(Some(accepted)) => accepted,
                Ok(None) => return,
                Err(error) => {
                    state.latest_attempt.fetch_add(1, Ordering::AcqRel);
                    if let Some(window) = weak.upgrade() {
                        window.set_settings_ai_agent_saved(false);
                        page.set_ai_agent_saved(false);
                        ToastManager::get(&window).invoke_error(settings_toast(
                            i18n::tr("settings.feedback.ai_agent_save_failed").as_ref(),
                        ));
                    }
                    log::error!("[AI Agent][Settings] Save admission failed: {error}");
                    return;
                }
            };
            let request = SaveUiRequest {
                _permit: Some(permit),
                attempt: state.latest_attempt.fetch_add(1, Ordering::AcqRel) + 1,
                request: receipt.id,
                context: page.capture_ai_agent_save(),
                draft,
            };
            if let Some(window) = weak.upgrade() {
                window.set_settings_ai_agent_saved(false);
                page.set_ai_agent_saved(false);
            }
            let weak = weak.clone();
            let page = page.clone();
            let state = state.clone();
            app_runtime::shared_runtime().spawn(async move {
                let result = tokio::task::spawn_blocking(move || receipt.wait())
                    .await
                    .unwrap_or_else(|error| {
                        Err(format!("AI Agent settings feedback worker failed: {error}"))
                    });
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(window) = weak.upgrade() {
                        complete_save(&window, page, state, request, result);
                    }
                });
            });
        },
    );
}

/// Event-loop only. Committed host state and the editable factory draft have
/// different guards: request order protects the former, visit + exact fields
/// protect the latter, and the latest attempt owns transient Save feedback.
fn complete_save(
    window: &MainWindow,
    page: SettingsPageBridge,
    state: Arc<SaveUiState>,
    request: SaveUiRequest,
    result: Result<AiAgentSaveOutcome, String>,
) {
    let mut draft_matches = false;
    let persisted = match result {
        Ok(outcome) => {
            if request.request > state.projected_request.load(Ordering::Acquire) {
                state
                    .projected_request
                    .store(request.request, Ordering::Release);
                crate::settings_page::apply_ai_agent_snapshot_to_window(
                    window,
                    outcome.snapshot.state(),
                );
            }
            if state.latest_attempt.load(Ordering::Acquire) == request.attempt {
                draft_matches = page.complete_ai_agent_save(
                    &request.context,
                    &request.draft,
                    outcome.snapshot.state(),
                );
            }
            outcome.persisted
        }
        Err(error) => Err(error),
    };
    if state.latest_attempt.load(Ordering::Acquire) != request.attempt {
        return;
    }
    let saved = persisted.is_ok() && draft_matches;
    window.set_settings_ai_agent_saved(saved);
    page.set_ai_agent_saved(saved);
    match persisted {
        Ok(()) => {
            ToastManager::get(window).invoke_success(settings_toast(
                i18n::tr("settings.feedback.ai_agent_saved").as_ref(),
            ));
            let weak = window.as_weak();
            Timer::single_shot(std::time::Duration::from_secs(2), move || {
                if state.latest_attempt.load(Ordering::Acquire) != request.attempt {
                    return;
                }
                if let Some(window) = weak.upgrade() {
                    window.set_settings_ai_agent_saved(false);
                    page.set_ai_agent_saved(false);
                }
            });
        }
        Err(error) => {
            ToastManager::get(window).invoke_error(settings_toast(
                i18n::tr("settings.feedback.ai_agent_save_failed").as_ref(),
            ));
            log::error!("[AI Agent][Settings] Save failed: {error}");
        }
    }
}

#[cfg(test)]
#[path = "settings_ui_tests.rs"]
mod tests;
