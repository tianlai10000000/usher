//! Chat-session schema and interrupted-task recovery. No UI ownership.
//! Production Session IO is owned by `SessionTransactions`. The legacy Chat
//! loader below exists only for corruption-preservation tests.
use super::storage::{JsonLoad, load_json_preserving_corrupt};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use usher_ai_review::{
    ReviewConversationMessage, ReviewInput, ReviewTaskState, ReviewTaskStatus, ReviewTaskSummary,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct PullRequestChatSession {
    pub(super) task: ReviewTaskSummary,
    pub(super) input: ReviewInput,
    #[serde(default)]
    pub(super) conversation: Vec<ReviewConversationMessage>,
}

// Test-only legacy baseline seam; normal builds cannot bypass SessionTransactions.
#[cfg(test)]
pub(super) async fn load_chat_session(
    path: PathBuf,
) -> Result<Option<PullRequestChatSession>, String> {
    tokio::task::spawn_blocking(move || match load_json_preserving_corrupt(&path)? {
        JsonLoad::Loaded(session) => Ok(Some(session)),
        JsonLoad::Missing => Ok(None),
        JsonLoad::CorruptPreserved { backup, error } => Err(format!(
            "AI review chat history was invalid ({error}) and was preserved at {}.",
            backup.display()
        )),
    })
    .await
    .map_err(|error| format!("AI review chat session loader failed: {error}"))?
}

pub(super) fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(u128::from(u64::MAX)) as u64
        })
}

pub(super) fn load_task_state(path: &Path) -> (ReviewTaskState, bool) {
    let (mut state, writes_allowed) = match load_json_preserving_corrupt::<ReviewTaskState>(path) {
        Ok(JsonLoad::Loaded(state)) => (state, true),
        Ok(JsonLoad::Missing) => (ReviewTaskState::default(), true),
        Ok(JsonLoad::CorruptPreserved { backup, error }) => {
            log::error!(
                "[AI Review][Task Store] Invalid state preserved at {}: {error}",
                backup.display()
            );
            (ReviewTaskState::default(), true)
        }
        Err(error) => {
            log::error!("[AI Review][Task Store] {error}");
            (ReviewTaskState::default(), false)
        }
    };
    for task in state.tasks.values_mut() {
        if task.status.is_active() {
            task.status = ReviewTaskStatus::Failed;
            task.error = "ai_review.failure.interrupted".to_string();
            task.updated_at_unix_ms = unix_millis();
        }
    }
    (state, writes_allowed)
}
