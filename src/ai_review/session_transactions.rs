//! Session transactions and owned blocking completion; no UI or task Store mutation.
//! Test-only drain receipts cover Session IO; controller terminal publishers have separate owners.
use super::sessions::PullRequestChatSession;
use super::storage::{
    JsonLoad, load_json_preserving_corrupt, reconcile_session_files_checked, session_paths,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock, Weak};
use tokio::sync::{Notify, oneshot};
use usher_ai_review::{
    ReviewConversationMessage, ReviewInput, ReviewSession, ReviewTaskId, ReviewTaskState,
};

const MAX_FAILURE_SAMPLES: usize = 16;
type Catalog = dyn Fn() -> Arc<ReviewTaskState> + Send + Sync;

#[derive(Clone)]
pub(super) enum SessionSnapshot {
    Review(ReviewSession),
    Chat(PullRequestChatSession),
}
impl SessionSnapshot {
    pub(super) fn input(&self) -> &ReviewInput {
        match self {
            Self::Review(s) => &s.input,
            Self::Chat(s) => &s.input,
        }
    }
    pub(super) fn conversation(&self) -> &[ReviewConversationMessage] {
        match self {
            Self::Review(s) => &s.conversation,
            Self::Chat(s) => &s.conversation,
        }
    }
    fn conversation_mut(&mut self) -> &mut Vec<ReviewConversationMessage> {
        match self {
            Self::Review(s) => &mut s.conversation,
            Self::Chat(s) => &mut s.conversation,
        }
    }
}
struct TaskOwner {
    transaction: Mutex<()>,
    attempt: AtomicU64,
}
struct AttemptIo {
    pending: AtomicUsize,
    settled: Notify,
}
#[derive(Clone)]
pub(super) struct AskAttempt {
    id: ReviewTaskId,
    owner: Arc<TaskOwner>,
    version: u64,
    operation: String,
    cancelled: Arc<AtomicBool>,
    user: ReviewConversationMessage,
    fallback: PullRequestChatSession,
    io: Arc<AttemptIo>,
}
impl AskAttempt {
    pub(super) fn assistant_identity(&self) -> String {
        format!("{}:assistant", self.operation)
    }
}
#[derive(Default)]
struct Completion {
    pending: usize,
    users_pending: usize,
    completed: u64,
    failures: u64,
    samples: Vec<String>,
}
#[derive(Debug, Clone)]
#[cfg(test)]
pub(super) struct SessionDrainReceipt {
    pub(super) completed_io: u64,
    pub(super) failure_count: u64,
    pub(super) failure_samples: Vec<String>,
}
#[derive(Debug)]
#[cfg(test)]
pub(super) struct SessionDrainError(pub(super) SessionDrainReceipt);
#[cfg(test)]
impl std::fmt::Display for SessionDrainError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} Session IO operation(s) failed: {:?}",
            self.0.failure_count, self.0.failure_samples
        )
    }
}
#[cfg(test)]
impl std::error::Error for SessionDrainError {}

pub(super) struct SessionTransactions {
    directory: PathBuf,
    catalog: Arc<Catalog>,
    closed: AtomicBool,
    owners: Mutex<HashMap<ReviewTaskId, Weak<TaskOwner>>>,
    // The real blocking closure owns its task gate and this read-side lease.
    // The existing task-writer thread reconciles under the write-side lease.
    io_gate: RwLock<()>,
    completion: Mutex<Completion>,
    users_settled: Condvar,
    settled: Notify,
    #[cfg(test)]
    write_probe: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    reconcile_probe: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    user_probe: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}
impl SessionTransactions {
    pub(super) fn new(directory: PathBuf, catalog: Arc<Catalog>) -> Arc<Self> {
        Arc::new(Self {
            directory,
            catalog,
            closed: AtomicBool::new(false),
            owners: Mutex::new(HashMap::new()),
            io_gate: RwLock::new(()),
            completion: Mutex::new(Completion::default()),
            users_settled: Condvar::new(),
            settled: Notify::new(),
            #[cfg(test)]
            write_probe: Mutex::new(None),
            #[cfg(test)]
            reconcile_probe: Mutex::new(None),
            #[cfg(test)]
            user_probe: Mutex::new(None),
        })
    }
    #[cfg(test)]
    pub(super) fn close_admission(&self) {
        let _completion = self.completion.lock().unwrap_or_else(|p| p.into_inner());
        self.closed.store(true, Ordering::Release);
    }
    fn owner(&self, id: &ReviewTaskId) -> Arc<TaskOwner> {
        let mut owners = self.owners.lock().unwrap_or_else(|p| p.into_inner());
        owners.retain(|_, owner| owner.strong_count() != 0);
        if let Some(owner) = owners.get(id).and_then(Weak::upgrade) {
            return owner;
        }
        let owner = Arc::new(TaskOwner {
            transaction: Mutex::new(()),
            attempt: AtomicU64::new(0),
        });
        owners.insert(id.clone(), Arc::downgrade(&owner));
        owner
    }
    fn require_live(&self, id: &ReviewTaskId) -> Result<(), String> {
        (self.catalog)()
            .tasks
            .contains_key(id)
            .then_some(())
            .ok_or_else(|| "AI review session task was retired".into())
    }
    /// Synchronous admission does no filesystem work. It starts the owned IO driver
    /// before returning, so cancelling/dropping the caller cannot discard an accepted user.
    pub(super) fn accept_user(
        self: &Arc<Self>,
        id: ReviewTaskId,
        mut user: ReviewConversationMessage,
        fallback: PullRequestChatSession,
        cancelled: Arc<AtomicBool>,
    ) -> Result<
        (
            AskAttempt,
            oneshot::Receiver<Result<SessionSnapshot, String>>,
        ),
        String,
    > {
        // Admission and close share the completion mutex; no operation slips in after close.
        let mut completion = self.completion.lock().unwrap_or_else(|p| p.into_inner());
        if self.closed.load(Ordering::Acquire) {
            return Err("AI review session admission is closed".into());
        }
        self.require_live(&id)?;
        let owner = self.owner(&id);
        let version = owner.attempt.fetch_add(1, Ordering::AcqRel) + 1;
        let operation = format!("{:032x}", rand::random::<u128>());
        user.message_id = Some(format!("{operation}:user"));
        let attempt = AskAttempt {
            id,
            owner,
            version,
            operation,
            cancelled,
            user,
            fallback,
            io: Arc::new(AttemptIo {
                pending: AtomicUsize::new(1),
                settled: Notify::new(),
            }),
        };
        completion.pending += 1;
        completion.users_pending += 1;
        drop(completion);
        let accepted = attempt.clone();
        let receipt = self.start_registered(true, Some(attempt.io.clone()), move |sessions| {
            sessions.write_user(&accepted, true)
        });
        Ok((attempt, receipt))
    }
    #[cfg(test)]
    pub(super) async fn retry_user(
        self: &Arc<Self>,
        attempt: AskAttempt,
    ) -> Result<SessionSnapshot, String> {
        self.require_live(&attempt.id)?;
        self.submit(true, Some(attempt.io.clone()), move |sessions| {
            sessions.write_user(&attempt, false)
        })
        .await
    }
    fn write_user(
        &self,
        attempt: &AskAttempt,
        initially_accepted: bool,
    ) -> Result<SessionSnapshot, String> {
        #[cfg(test)]
        {
            let probe = self.user_probe.lock().unwrap().clone();
            if let Some(probe) = probe {
                probe();
            }
        }
        let _io = self.io_gate.read().unwrap_or_else(|p| p.into_inner());
        let _task = attempt
            .owner
            .transaction
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if !initially_accepted {
            self.require_live(&attempt.id)?;
        }
        // Initial admission deliberately survives later cancellation/retirement. Reconcile
        // waits for every accepted-user receipt before removing a retired task's files.
        let mut snapshot = self
            .read_current(&attempt.id)?
            .unwrap_or_else(|| SessionSnapshot::Chat(attempt.fallback.clone()));
        merge_conversations(
            snapshot.conversation_mut(),
            std::slice::from_ref(&attempt.user),
        )?;
        self.write_current(&attempt.id, &snapshot)?;
        Ok(snapshot)
    }
    pub(super) async fn update_context(
        self: &Arc<Self>,
        attempt: AskAttempt,
        input: ReviewInput,
    ) -> Result<Option<SessionSnapshot>, String> {
        self.submit(false, Some(attempt.io.clone()), move |sessions| {
            sessions.guarded_update(&attempt, move |snapshot| {
                // A completed Review's authoritative context wins over a concurrent provisional Ask.
                if let SessionSnapshot::Chat(chat) = snapshot {
                    chat.input = input;
                }
                Ok(())
            })
        })
        .await
    }
    pub(super) async fn append_assistant(
        self: &Arc<Self>,
        attempt: AskAttempt,
        mut message: ReviewConversationMessage,
    ) -> Result<Option<SessionSnapshot>, String> {
        message.message_id = Some(attempt.assistant_identity());
        self.submit(false, Some(attempt.io.clone()), move |sessions| {
            sessions.guarded_update(&attempt, move |snapshot| {
                merge_conversations(snapshot.conversation_mut(), std::slice::from_ref(&message))
            })
        })
        .await
    }
    fn guarded_update(
        &self,
        attempt: &AskAttempt,
        update: impl FnOnce(&mut SessionSnapshot) -> Result<(), String>,
    ) -> Result<Option<SessionSnapshot>, String> {
        let _io = self.io_gate.read().unwrap_or_else(|p| p.into_inner());
        let _task = attempt
            .owner
            .transaction
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // This is side-effect admission. Cancellation after this point cannot abort a
        // real atomic write; its owned driver still finishes before another task write.
        if attempt.cancelled.load(Ordering::Acquire)
            || attempt.owner.attempt.load(Ordering::Acquire) != attempt.version
            || self.require_live(&attempt.id).is_err()
        {
            return Ok(None);
        }
        let Some(mut snapshot) = self.read_current(&attempt.id)? else {
            return Ok(None);
        };
        update(&mut snapshot)?;
        self.write_current(&attempt.id, &snapshot)?;
        Ok(Some(snapshot))
    }
    pub(super) async fn complete_review(
        self: &Arc<Self>,
        id: ReviewTaskId,
        mut result: ReviewSession,
    ) -> Result<ReviewSession, String> {
        let owner = self.owner(&id);
        self.submit(false, None, move |sessions| {
            let _io = sessions.io_gate.read().unwrap_or_else(|p| p.into_inner());
            let _task = owner.transaction.lock().unwrap_or_else(|p| p.into_inner());
            sessions.require_live(&id)?;
            if let Some(current) = sessions.read_current(&id)? {
                merge_conversations(&mut result.conversation, current.conversation())?;
            }
            sessions.write_current(&id, &SessionSnapshot::Review(result.clone()))?;
            Ok(result)
        })
        .await
    }
    pub(super) async fn read(
        self: &Arc<Self>,
        id: ReviewTaskId,
    ) -> Result<Option<SessionSnapshot>, String> {
        let owner = self.owner(&id);
        self.submit(false, None, move |sessions| {
            let _io = sessions.io_gate.read().unwrap_or_else(|p| p.into_inner());
            let _task = owner.transaction.lock().unwrap_or_else(|p| p.into_inner());
            sessions.require_live(&id)?;
            sessions.read_current(&id)
        })
        .await
    }
    fn read_current(&self, id: &ReviewTaskId) -> Result<Option<SessionSnapshot>, String> {
        let (review_path, chat_path) = session_paths(&self.directory, id);
        let review: Option<ReviewSession> = read_optional(&review_path)?;
        let chat: Option<PullRequestChatSession> = read_optional(&chat_path)?;
        match (review, chat) {
            (Some(mut review), chat) => {
                if let Some(chat) = chat {
                    merge_conversations(&mut review.conversation, &chat.conversation)?;
                }
                Ok(Some(SessionSnapshot::Review(review)))
            }
            (None, Some(chat)) => Ok(Some(SessionSnapshot::Chat(chat))),
            (None, None) => Ok(None),
        }
    }
    fn write_current(&self, id: &ReviewTaskId, snapshot: &SessionSnapshot) -> Result<(), String> {
        let (review, chat) = session_paths(&self.directory, id);
        let (path, data) = match snapshot {
            SessionSnapshot::Review(session) => (review, serde_json::to_vec(session)),
            SessionSnapshot::Chat(session) => (chat, serde_json::to_vec(session)),
        };
        #[cfg(test)]
        {
            let probe = self.write_probe.lock().unwrap().clone();
            if let Some(probe) = probe {
                probe();
            }
        }
        usher_fs::atomic_write(&path, &data.map_err(|e| e.to_string())?).map_err(|e| e.to_string())
    }
    /// Called only on the existing task persistence worker, never a Store reducer/UI.
    /// Uses the current catalog instead of the writer's potentially obsolete snapshot.
    pub(super) fn reconcile(&self) -> Result<(), String> {
        self.completion
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pending += 1;
        let mut receipt = ReconcileCompletion {
            sessions: self,
            error: None,
        };
        let (_io, current) = loop {
            let mut completion = self.completion.lock().unwrap_or_else(|p| p.into_inner());
            while completion.users_pending != 0 {
                completion = self
                    .users_settled
                    .wait(completion)
                    .unwrap_or_else(|p| p.into_inner());
            }
            drop(completion);
            #[cfg(test)]
            {
                let probe = self.reconcile_probe.lock().unwrap().clone();
                if let Some(probe) = probe {
                    probe();
                }
            }
            let io = self.io_gate.write().unwrap_or_else(|p| p.into_inner());
            let completion = self.completion.lock().unwrap_or_else(|p| p.into_inner());
            if completion.users_pending != 0 {
                // A user was accepted while we were acquiring the IO barrier. Release
                // it before waiting: the user's real writer needs the read-side lease.
                drop(completion);
                drop(io);
                continue;
            }
            let current = (self.catalog)();
            drop(completion);
            break (io, current);
        };
        let result =
            reconcile_session_files_checked(&self.directory, current.tasks.keys()).map(|_| ());
        receipt.error = result.as_ref().err().cloned();
        self.owners
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retain(|_, owner| owner.strong_count() != 0);
        drop(_io);
        drop(receipt);
        result
    }
    fn finish_registered(&self, user: bool, error: Option<String>) {
        let mut completion = self.completion.lock().unwrap_or_else(|p| p.into_inner());
        completion.pending -= 1;
        completion.completed += 1;
        if user {
            completion.users_pending -= 1;
        }
        if let Some(error) = error {
            completion.failures += 1;
            if completion.samples.len() < MAX_FAILURE_SAMPLES {
                completion.samples.push(error.chars().take(1024).collect());
            }
        }
        drop(completion);
        self.users_settled.notify_all();
        self.settled.notify_waiters();
    }
    fn start_registered<T: Send + 'static>(
        self: &Arc<Self>,
        user: bool,
        attempt_io: Option<Arc<AttemptIo>>,
        work: impl FnOnce(&SessionTransactions) -> Result<T, String> + Send + 'static,
    ) -> oneshot::Receiver<Result<T, String>> {
        let (send, receive) = oneshot::channel();
        let sessions = self.clone();
        let blocking_owner = self.clone();
        let runtime = crate::app_runtime::shared_runtime();
        let blocking = runtime.spawn_blocking(move || work(&blocking_owner));
        // The driver, not the request Future, owns the actual blocking JoinHandle.
        // tokio::spawn uses the already-entered shared application runtime.
        runtime.spawn(async move {
            let result = blocking.await.unwrap_or_else(|error| {
                Err(format!("AI review session IO worker failed: {error}"))
            });
            let error = result.as_ref().err().cloned();
            let _ = send.send(result);
            sessions.finish_registered(user, error);
            if let Some(io) = attempt_io {
                io.pending.fetch_sub(1, Ordering::AcqRel);
                io.settled.notify_waiters();
            }
        });
        receive
    }
    async fn submit<T: Send + 'static>(
        self: &Arc<Self>,
        user: bool,
        attempt_io: Option<Arc<AttemptIo>>,
        work: impl FnOnce(&SessionTransactions) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        {
            let mut completion = self.completion.lock().unwrap_or_else(|p| p.into_inner());
            completion.pending += 1;
            if user {
                completion.users_pending += 1;
            }
        }
        if let Some(io) = &attempt_io {
            io.pending.fetch_add(1, Ordering::AcqRel);
        }
        self.start_registered(user, attempt_io, work)
            .await
            .map_err(|_| "AI review session IO receipt was lost".to_string())?
    }
    pub(super) async fn wait_attempt(&self, attempt: &AskAttempt) {
        loop {
            let settled = attempt.io.settled.notified();
            tokio::pin!(settled);
            settled.as_mut().enable();
            if attempt.io.pending.load(Ordering::Acquire) == 0 {
                return;
            }
            settled.await;
        }
    }
    #[cfg(test)]
    pub(super) async fn drain(&self) -> Result<SessionDrainReceipt, SessionDrainError> {
        loop {
            let settled = self.settled.notified();
            // Enable the waiter before observing pending to avoid a completion wakeup race.
            tokio::pin!(settled);
            settled.as_mut().enable();
            let receipt = {
                let completion = self.completion.lock().unwrap_or_else(|p| p.into_inner());
                (completion.pending == 0).then(|| SessionDrainReceipt {
                    completed_io: completion.completed,
                    failure_count: completion.failures,
                    failure_samples: completion.samples.clone(),
                })
            };
            if let Some(receipt) = receipt {
                return if receipt.failure_count == 0 {
                    Ok(receipt)
                } else {
                    Err(SessionDrainError(receipt))
                };
            }
            settled.await;
        }
    }
    #[cfg(test)]
    pub(super) fn retained_task_owners(&self) -> usize {
        self.owners
            .lock()
            .unwrap()
            .values()
            .filter(|owner| owner.strong_count() != 0)
            .count()
    }
}
struct ReconcileCompletion<'a> {
    sessions: &'a SessionTransactions,
    error: Option<String>,
}
impl Drop for ReconcileCompletion<'_> {
    fn drop(&mut self) {
        let error = self.error.take().or_else(|| {
            std::thread::panicking()
                .then(|| "AI review session reconciliation interrupted".to_string())
        });
        self.sessions.finish_registered(false, error);
    }
}
fn read_optional<T: serde::de::DeserializeOwned>(
    path: &std::path::Path,
) -> Result<Option<T>, String> {
    match load_json_preserving_corrupt(path)? {
        JsonLoad::Loaded(value) => Ok(Some(value)),
        JsonLoad::Missing => Ok(None),
        JsonLoad::CorruptPreserved { backup, error } => Err(format!(
            "AI review session was invalid ({error}) and preserved at {}",
            backup.display()
        )),
    }
}
pub(super) fn merge_conversations(
    conversation: &mut Vec<ReviewConversationMessage>,
    additional: &[ReviewConversationMessage],
) -> Result<(), String> {
    for message in additional {
        let existing = conversation.iter_mut().find(|existing| {
            match (&existing.message_id, &message.message_id) {
                (Some(left), Some(right)) => left == right,
                (None, None) => {
                    existing.role == message.role
                        && existing.content == message.content
                        && existing.created_at_unix_ms == message.created_at_unix_ms
                        && existing.elapsed_ms == message.elapsed_ms
                }
                _ => false,
            }
        });
        if let Some(existing) = existing {
            if existing.role != message.role
                || existing.content != message.content
                || existing.created_at_unix_ms != message.created_at_unix_ms
                || existing.elapsed_ms != message.elapsed_ms
            {
                return Err(
                    "AI review session message identity conflicts with existing content".into(),
                );
            }
            if existing.prompt.is_none() {
                existing.prompt = message.prompt.clone();
            }
        } else {
            conversation.push(message.clone());
        }
    }
    conversation.sort_by_key(|message| message.created_at_unix_ms);
    Ok(())
}

#[cfg(test)]
#[path = "session_transactions_tests.rs"]
mod tests;
