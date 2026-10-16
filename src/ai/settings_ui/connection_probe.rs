//! Shared cancellable probe ownership and semantic terminal results.

use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
pub(super) struct Probe {
    revision: AtomicU64,
    task: Mutex<Option<tokio::task::AbortHandle>>,
}

impl Probe {
    pub(super) fn next_ticket(&self) -> u64 {
        self.revision.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub(super) fn attach(&self, task: tokio::task::AbortHandle) {
        *self.task.lock().unwrap() = Some(task);
    }

    pub(super) fn cancel(&self) {
        self.next_ticket();
        if let Some(task) = self.task.lock().unwrap().take() {
            task.abort();
        }
    }

    pub(super) fn finish(&self, ticket: u64) -> bool {
        if self
            .revision
            .compare_exchange(ticket, ticket + 1, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        self.task.lock().unwrap().take();
        true
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        if let Some(task) = self.task.get_mut().unwrap().take() {
            task.abort();
        }
    }
}

pub(super) fn feedback_key<E>(
    result: &Result<crate::ai::agent::ConnectionTestResult, E>,
) -> &'static str {
    match result {
        Ok(result) if result.model_listed => "settings.feedback.ai_agent_connected",
        Ok(_) => "settings.feedback.ai_agent_connected_model_unlisted",
        Err(_) => "settings.feedback.ai_agent_test_failed",
    }
}
