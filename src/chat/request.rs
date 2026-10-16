//! Chat request admission captures connection and attribution together.

use crate::ai::agent::CompletionMessage;
use crate::ai::{agent, identity::AgentIdentity, prompts};
use crate::chat::chat::send_message_to_server;
use crate::chat::session::ChatMode;
use crate::{MainWindow, settings::AiAgentProfile};
use std::cell::RefCell;

thread_local! {
    static IN_FLIGHT_AGENT: RefCell<Option<AgentIdentity>> = const { RefCell::new(None) };
}

pub(super) fn capture(ui: &MainWindow, mode: ChatMode) -> Option<AiAgentProfile> {
    if mode != ChatMode::Ai {
        return None;
    }
    let profile = crate::ai::settings_state::current_profile();
    IN_FLIGHT_AGENT.with(|slot| *slot.borrow_mut() = Some(AgentIdentity::from_profile(&profile)));
    sync_agent_identity(ui);
    Some(profile)
}

pub(super) fn finish(ui: &MainWindow, mode: ChatMode) {
    if mode == ChatMode::Ai {
        IN_FLIGHT_AGENT.with(|slot| *slot.borrow_mut() = None);
        sync_agent_identity(ui);
    }
}

pub(crate) fn sync_agent_identity(ui: &MainWindow) {
    let identity = IN_FLIGHT_AGENT
        .with(|slot| slot.borrow().clone())
        .unwrap_or_else(|| {
            AgentIdentity::from_profile(&crate::ai::settings_state::current_profile())
        });
    ui.set_chat_ai_model_name(
        if identity.model.trim().is_empty() {
            String::new()
        } else {
            identity.label()
        }
        .into(),
    );
}

pub(super) async fn send_chat_message(
    mode: ChatMode,
    user_message: &str,
    ai_history: Vec<CompletionMessage>,
    settings: Option<crate::settings::AiAgentSettings>,
) -> Result<String, crate::http::HttpError> {
    match mode {
        ChatMode::Ai => {
            let settings = settings.expect("AI requests capture a profile at admission");
            let envelope = prompts::general_chat(&settings, &ai_history).map_err(|error| {
                log::error!("[AI Prompt] Failed to prepare general chat prompt: {error}");
                crate::http::HttpError {
                    kind: crate::http::HttpErrorKind::Build,
                    operation: "Prepare AI chat prompt".to_string(),
                    method: None,
                    url: None,
                    status: None,
                    message: "Unable to prepare the built-in AI prompt".to_string(),
                    response_body: None,
                }
            })?;
            envelope.log_metadata("general_chat");
            agent::complete(&settings, &envelope.messages, "Send AI chat message").await
        }
        ChatMode::Rag => send_message_to_server(user_message).await,
    }
}
