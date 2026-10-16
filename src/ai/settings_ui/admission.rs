//! Bounded ownership of accepted commands through event-loop completion.

use crate::settings::AiAgentProfile;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

pub(super) const UI_SAVE_CAPACITY: usize = 8;

#[derive(Clone, PartialEq, Eq)]
pub(super) enum UiAction {
    Create(AiAgentProfile),
    Update(AiAgentProfile),
    Delete(String),
    Activate(String),
}

impl UiAction {
    pub(super) fn feedback(&self, success: bool) -> &'static str {
        match (self, success) {
            (Self::Create(_) | Self::Update(_), true) => "settings.feedback.ai_agent_saved",
            (Self::Create(_) | Self::Update(_), false) => "settings.feedback.ai_agent_save_failed",
            (Self::Delete(_), true) => "settings.feedback.ai_agent_deleted",
            (Self::Delete(_), false) => "settings.feedback.ai_agent_delete_failed",
            (Self::Activate(_), true) => "settings.feedback.ai_agent_activated",
            (Self::Activate(_), false) => "settings.feedback.ai_agent_activation_failed",
        }
    }
}

#[derive(Default)]
pub(super) struct SaveUiState {
    pub(super) latest_attempt: AtomicU64,
    latest_ticket: AtomicU64,
    pub(super) pending: Mutex<Vec<(u64, UiAction)>>,
}

pub(super) struct SaveUiPermit {
    state: Arc<SaveUiState>,
    ticket: u64,
}

impl SaveUiState {
    pub(super) fn reserve(
        self: &Arc<Self>,
        action: &UiAction,
    ) -> Result<Option<SaveUiPermit>, String> {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if pending.last().is_some_and(|(ticket, value)| {
            *ticket == self.latest_ticket.load(Ordering::Acquire) && value == action
        }) {
            return Ok(None);
        }
        if pending.len() >= UI_SAVE_CAPACITY {
            return Err("AI Agent settings UI completion queue is full".into());
        }
        let ticket = self.latest_ticket.fetch_add(1, Ordering::AcqRel) + 1;
        pending.push((ticket, action.clone()));
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
