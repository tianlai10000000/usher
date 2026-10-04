use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use chrono::Utc;
use std::collections::{BTreeMap, HashMap, HashSet};
use usher_state_store::{
    LatestPersistence, LocalStateStore, Reduction, StateStore, Subscription as StoreSubscription,
};

use crate::desktop::{DesktopNotifier, NotifyRustDesktopNotifier};
use crate::native_worker::NativeWorker;
use crate::storage::{self, MAX_STORED_MESSAGES};
use crate::{Message, MessageDraft};

static MESSAGE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
const MAX_TITLE_CHARS: usize = 200;
const MAX_BODY_CHARS: usize = 4_096;
const MAX_SOURCE_CHARS: usize = 100;
const MAX_CLASSIFICATION_CHARS: usize = 64;
const MAX_TARGET_CHARS: usize = 512;
const MAX_DEDUPLICATION_KEY_CHARS: usize = 256;
const MAX_METADATA_ENTRIES: usize = 32;
const MAX_METADATA_KEY_CHARS: usize = 64;
const MAX_METADATA_VALUE_CHARS: usize = 512;
const UNFINISHED_HYDRATION: &str =
    "startup hydration did not enable durability for accepted messages";

type Listener = Arc<dyn Fn(MessageSnapshot) + Send + Sync>;

#[derive(Debug)]
pub enum MessageCenterError {
    Storage(std::io::Error),
    Persistence(String),
    NativeWorker(String),
    Shutdown { persistence: String, native: String },
    ReentrantShutdown,
    WorkerUnavailable,
}

impl std::fmt::Display for MessageCenterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(error) => write!(formatter, "message storage error: {error}"),
            Self::Persistence(error) => write!(formatter, "message persistence error: {error}"),
            Self::NativeWorker(error) => {
                write!(formatter, "native notification worker error: {error}")
            }
            Self::Shutdown {
                persistence,
                native,
            } => write!(
                formatter,
                "message shutdown failed: persistence: {persistence}; native notification worker: {native}"
            ),
            Self::ReentrantShutdown => write!(
                formatter,
                "message shutdown cannot wait from its own action or native callback"
            ),
            Self::WorkerUnavailable => write!(formatter, "message worker is unavailable"),
        }
    }
}

impl std::error::Error for MessageCenterError {}

impl From<std::io::Error> for MessageCenterError {
    fn from(error: std::io::Error) -> Self {
        Self::Storage(error)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageSnapshot {
    pub messages: Arc<[Message]>,
    pub unread_count: usize,
}

impl MessageSnapshot {
    fn from_messages(messages: Vec<Message>) -> Self {
        let unread_count = messages.iter().filter(|message| !message.read).count();
        Self {
            messages: messages.into(),
            unread_count,
        }
    }
}

#[derive(Clone)]
pub struct MessageCenter {
    inner: Arc<Inner>,
}

type MessageStore = dyn StateStore<MessageSnapshot, MessageAction, MessageActionResult>;

struct Inner {
    store: Arc<MessageStore>,
    storage_path: PathBuf,
    // Keeps the automatic persistence observer registered for this center.
    persistence_subscription: Mutex<Option<StoreSubscription>>,
    persistence_enabled: AtomicBool,
    application_active: AtomicBool,
    persistence: Arc<LatestPersistence<MessageSnapshot>>,
    native: NativeWorker,
    admission: Mutex<AdmissionState>,
    admission_done: Condvar,
    shutdown_serial: Mutex<()>,
}

#[derive(Default)]
struct AdmissionState {
    closing: bool,
    active_actions: usize,
    active_threads: HashMap<std::thread::ThreadId, usize>,
}

/// An admitted action may dispatch/reenter subscribers without holding a lock.
/// Shutdown stops admission, then waits for these complete actions (including
/// native enqueue) before unregistering persistence and draining both workers.
struct ActionPermit<'a> {
    inner: &'a Inner,
    thread_id: std::thread::ThreadId,
}

impl Drop for ActionPermit<'_> {
    fn drop(&mut self) {
        let mut state = self
            .inner
            .admission
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        state.active_actions -= 1;
        if let Some(count) = state.active_threads.get_mut(&self.thread_id) {
            *count -= 1;
            if *count == 0 {
                state.active_threads.remove(&self.thread_id);
            }
        }
        if state.active_actions == 0 {
            self.inner.admission_done.notify_all();
        }
    }
}

/// Closed set of domain mutations accepted by the message store. Public
/// methods below are typed action creators; only `dispatch` mutates state.
enum MessageAction {
    Hydrate(Vec<Message>),
    Publish(Box<Message>),
    SetRead { id: String, read: bool },
    MarkAllRead,
    Remove { id: String },
    Clear,
}

enum MessageActionResult {
    Publish {
        message: Box<Message>,
        inserted: bool,
    },
    Changed(bool),
    Count(usize),
}

/// Removes a snapshot listener when dropped.
pub struct Subscription {
    _store_subscription: StoreSubscription,
}

impl MessageCenter {
    pub fn open_default() -> Result<Self, MessageCenterError> {
        Self::open_with(
            storage::default_storage_path()?,
            Arc::new(NotifyRustDesktopNotifier),
        )
    }

    pub fn open_default_deferred() -> Result<Self, MessageCenterError> {
        Self::open_deferred_with(
            storage::default_storage_path()?,
            Arc::new(NotifyRustDesktopNotifier),
        )
    }

    pub fn open_with(
        path: impl Into<PathBuf>,
        notifier: Arc<dyn DesktopNotifier>,
    ) -> Result<Self, MessageCenterError> {
        let path = path.into();
        let messages = load_messages(&path)?;
        Self::open_initialized(path, notifier, messages, true)
    }

    pub fn open_deferred_with(
        path: impl Into<PathBuf>,
        notifier: Arc<dyn DesktopNotifier>,
    ) -> Result<Self, MessageCenterError> {
        Self::open_initialized(path.into(), notifier, Vec::new(), false)
    }

    fn open_initialized(
        path: PathBuf,
        notifier: Arc<dyn DesktopNotifier>,
        messages: Vec<Message>,
        install_persistence: bool,
    ) -> Result<Self, MessageCenterError> {
        let destination = path.clone();
        Self::open_initialized_with_persist(
            path,
            notifier,
            messages,
            install_persistence,
            move |snapshot| {
                storage::save(&destination, &snapshot.messages).map_err(|error| error.to_string())
            },
        )
    }

    fn open_initialized_with_persist(
        path: PathBuf,
        notifier: Arc<dyn DesktopNotifier>,
        messages: Vec<Message>,
        install_persistence: bool,
        persist: impl Fn(&MessageSnapshot) -> Result<(), String> + Send + Sync + 'static,
    ) -> Result<Self, MessageCenterError> {
        let persistence = Arc::new(LatestPersistence::start(
            "usher-message-persistence",
            Duration::ZERO,
            persist,
            |_, error| log::error!("[Messages] Failed to persist message center: {error}"),
        )?);
        let native = NativeWorker::start(notifier)?;
        let store: Arc<MessageStore> = Arc::new(LocalStateStore::new(
            MessageSnapshot::from_messages(messages),
            reduce_state,
        ));
        let center = Self {
            inner: Arc::new(Inner {
                store,
                storage_path: path,
                persistence_subscription: Mutex::new(None),
                persistence_enabled: AtomicBool::new(false),
                // Unknown activity is treated as foreground. This conservative
                // default prevents duplicate native + in-app presentation until
                // the host supplies its first focus state.
                application_active: AtomicBool::new(true),
                persistence,
                native,
                admission: Mutex::new(AdmissionState::default()),
                admission_done: Condvar::new(),
                shutdown_serial: Mutex::new(()),
            }),
        };
        if install_persistence {
            center.install_persistence_observer();
            center
                .inner
                .persistence_enabled
                .store(true, Ordering::Release);
        }
        Ok(center)
    }

    pub fn hydrate_from_storage(&self) -> Result<usize, MessageCenterError> {
        // The permit covers the complete accepted hydration operation, but no
        // file I/O holds the Action/closing admission mutex.
        let _admission = self.admit()?;
        let messages = load_messages(&self.inner.storage_path)?;
        let loaded = messages.len();
        self.inner.store.dispatch(MessageAction::Hydrate(messages));
        self.install_persistence_observer();
        let snapshot = self.inner.store.snapshot();
        self.inner
            .persistence
            .submit(snapshot.revision(), snapshot.state_arc());
        self.inner
            .persistence_enabled
            .store(true, Ordering::Release);
        Ok(loaded)
    }

    fn install_persistence_observer(&self) {
        let mut subscription = self
            .inner
            .persistence_subscription
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if subscription.is_some() {
            return;
        }
        let persistence = self.inner.persistence.clone();
        *subscription = Some(self.inner.store.observe_commits(Arc::new(move |snapshot| {
            if !persistence.submit(snapshot.revision(), snapshot.state_arc()) {
                log::error!("[Messages] State commit could not reach persistence worker");
            }
        })));
    }

    pub fn snapshot(&self) -> MessageSnapshot {
        self.inner.store.snapshot().state().clone()
    }

    pub fn subscribe(&self, listener: Listener) -> Subscription {
        Subscription {
            _store_subscription: self.inner.store.subscribe(Arc::new(move |snapshot| {
                listener(snapshot.state().clone());
            })),
        }
    }

    /// Updates the host application's focus state. Platform/UI integration owns
    /// detection; the message-center core only evaluates delivery policy.
    pub fn set_application_active(&self, active: bool) {
        self.inner
            .application_active
            .store(active, Ordering::Release);
    }

    pub fn application_active(&self) -> bool {
        self.inner.application_active.load(Ordering::Acquire)
    }

    /*
     * The remainder of the public action creators intentionally stays below
     * the deferred hydration boundary. Hydration is one typed Store action, so
     * subscribers and persistence observe the merged commit exactly once.
     */
    pub fn publish(&self, draft: MessageDraft) -> Result<Message, MessageCenterError> {
        let _admission = self.admit()?;
        let sequence = MESSAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let created_at = Utc::now();
        let native_delivery = draft.native_delivery;
        let deduplication_key = draft
            .deduplication_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(|key| truncate_chars(key, MAX_DEDUPLICATION_KEY_CHARS));
        let message = Message {
            id: format!("{}-{sequence}", created_at.timestamp_micros()),
            title: normalized_bounded(&draft.title, "Notification", MAX_TITLE_CHARS),
            body: truncate_chars(draft.body.trim(), MAX_BODY_CHARS),
            source: normalized_bounded(&draft.source, "Usher", MAX_SOURCE_CHARS),
            channel: normalized_bounded(&draft.channel, "application", MAX_CLASSIFICATION_CHARS),
            category: normalized_bounded(
                &draft.category,
                crate::categories::GENERAL,
                MAX_CLASSIFICATION_CHARS,
            ),
            severity: draft.severity,
            created_at,
            read: false,
            target: draft
                .target
                .as_deref()
                .map(str::trim)
                .filter(|target| !target.is_empty())
                .map(|target| truncate_chars(target, MAX_TARGET_CHARS)),
            deduplication_key,
            metadata: bounded_metadata(draft.metadata),
        };

        let dispatch = self
            .inner
            .store
            .dispatch(MessageAction::Publish(Box::new(message)));
        let snapshot = dispatch.snapshot.state().clone();
        let result = dispatch.output;
        let MessageActionResult::Publish { message, inserted } = result else {
            unreachable!("publish action returned a non-publish result");
        };
        if !inserted {
            log::info!(
                "[Messages] publish deduped id={} channel={} key={:?} title={:?} total={} unread={} read_flags={}",
                message.id,
                message.channel,
                message.deduplication_key,
                message.title,
                snapshot.messages.len(),
                snapshot.unread_count,
                snapshot.messages.iter().filter(|item| item.read).count()
            );
            return Ok(*message);
        }
        let read_flags = snapshot.messages.iter().filter(|item| item.read).count();
        log::info!(
            "[Messages] publish added id={} source={} category={} channel={} severity={:?} native={:?} title={:?} body_chars={} total={} unread={} read_flags={} app_active={}",
            message.id,
            message.source,
            message.category,
            message.channel,
            message.severity,
            native_delivery,
            message.title,
            message.body.chars().count(),
            snapshot.messages.len(),
            snapshot.unread_count,
            read_flags,
            self.application_active()
        );
        if native_delivery.should_deliver(self.application_active()) {
            self.inner.native.try_enqueue(&message);
        }
        Ok(*message)
    }

    pub fn set_read(&self, id: &str, read: bool) -> Result<bool, MessageCenterError> {
        let _admission = self.admit()?;
        let dispatch = self.inner.store.dispatch(MessageAction::SetRead {
            id: id.to_string(),
            read,
        });
        match dispatch.output {
            MessageActionResult::Changed(changed) => Ok(changed),
            _ => unreachable!("set-read action returned an incompatible result"),
        }
    }

    pub fn mark_all_read(&self) -> Result<usize, MessageCenterError> {
        let _admission = self.admit()?;
        let dispatch = self.inner.store.dispatch(MessageAction::MarkAllRead);
        match dispatch.output {
            MessageActionResult::Count(changed) => Ok(changed),
            _ => unreachable!("mark-all-read action returned an incompatible result"),
        }
    }

    pub fn remove(&self, id: &str) -> Result<bool, MessageCenterError> {
        let _admission = self.admit()?;
        let dispatch = self
            .inner
            .store
            .dispatch(MessageAction::Remove { id: id.to_string() });
        match dispatch.output {
            MessageActionResult::Changed(changed) => Ok(changed),
            _ => unreachable!("remove action returned an incompatible result"),
        }
    }

    pub fn clear(&self) -> Result<usize, MessageCenterError> {
        let _admission = self.admit()?;
        let dispatch = self.inner.store.dispatch(MessageAction::Clear);
        match dispatch.output {
            MessageActionResult::Count(removed) => Ok(removed),
            _ => unreachable!("clear action returned an incompatible result"),
        }
    }

    /// Wait for the latest accepted durable state, independently of native
    /// delivery. Failure is reported and remains eligible for explicit retry.
    pub fn flush(&self) -> Result<(), MessageCenterError> {
        if self.unfinished_hydration() {
            return Err(MessageCenterError::Persistence(UNFINISHED_HYDRATION.into()));
        }
        self.inner
            .persistence
            .flush()
            .map_err(MessageCenterError::Persistence)
    }

    pub fn retry_failed(&self) -> Result<bool, MessageCenterError> {
        let _admission = self.admit()?;
        self.inner
            .persistence
            .retry_failed()
            .map_err(MessageCenterError::Persistence)
    }

    #[cfg(test)]
    fn flush_native(&self) -> Result<(), MessageCenterError> {
        self.inner
            .native
            .flush()
            .map_err(MessageCenterError::NativeWorker)
    }

    pub fn shutdown(&self) {
        if let Err(error) = self.shutdown_checked() {
            log::error!("[Messages] Shutdown failed: {error}");
        }
    }

    /// Blocking owned-worker barrier for the application's background shutdown
    /// stage. The compatibility wrapper above preserves the existing API.
    pub fn shutdown_checked(&self) -> Result<(), MessageCenterError> {
        // Check before the serialization lock: another shutdown may already be
        // waiting for this callback's action to return. Rejecting self-wait
        // leaves the center usable and lets its lifecycle owner drain it.
        let current_thread = std::thread::current().id();
        if self
            .inner
            .admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_threads
            .contains_key(&current_thread)
            || self.inner.native.is_current_worker()
        {
            return Err(MessageCenterError::ReentrantShutdown);
        }
        let _shutdown = self
            .inner
            .shutdown_serial
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Close admission under a short lock. Subscribers may reenter actions;
        // no reducer or listener executes while this lock is held.
        {
            let mut admission = self
                .inner
                .admission
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            admission.closing = true;
            while admission.active_actions != 0 {
                admission = self
                    .inner
                    .admission_done
                    .wait(admission)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        }
        let unfinished_hydration = self.unfinished_hydration();
        self.inner
            .persistence_subscription
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        let persisted = self
            .inner
            .persistence
            .shutdown()
            .map_err(MessageCenterError::Persistence);
        let persisted = if unfinished_hydration {
            Err(MessageCenterError::Persistence(UNFINISHED_HYDRATION.into()))
        } else {
            persisted
        };
        let delivered = self
            .inner
            .native
            .shutdown()
            .map_err(MessageCenterError::NativeWorker);
        match (persisted, delivered) {
            (
                Err(MessageCenterError::Persistence(persistence)),
                Err(MessageCenterError::NativeWorker(native)),
            ) => Err(MessageCenterError::Shutdown {
                persistence,
                native,
            }),
            (Err(error), _) | (_, Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    fn admit(&self) -> Result<ActionPermit<'_>, MessageCenterError> {
        let mut admission = self
            .inner
            .admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if admission.closing {
            Err(MessageCenterError::WorkerUnavailable)
        } else {
            let thread_id = std::thread::current().id();
            admission.active_actions += 1;
            *admission.active_threads.entry(thread_id).or_default() += 1;
            Ok(ActionPermit {
                inner: &self.inner,
                thread_id,
            })
        }
    }

    fn unfinished_hydration(&self) -> bool {
        !self.inner.persistence_enabled.load(Ordering::Acquire)
            && self.inner.store.snapshot().revision() > 0
    }
}

fn load_messages(path: &Path) -> Result<Vec<Message>, MessageCenterError> {
    let mut messages = match storage::load(path) {
        Ok(messages) => messages,
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
            log::error!("[Messages] Ignoring corrupt message store: {error}");
            Vec::new()
        }
        Err(error) => return Err(error.into()),
    };
    messages.sort_by_key(|message| std::cmp::Reverse(message.created_at));
    messages.truncate(MAX_STORED_MESSAGES);
    Ok(messages)
}

fn reduce_state(
    current: &MessageSnapshot,
    action: MessageAction,
) -> Reduction<MessageSnapshot, MessageActionResult> {
    let mut messages = current.messages.to_vec();
    let (result, changed) = reduce_messages(&mut messages, action);
    let state = MessageSnapshot::from_messages(messages);
    if changed {
        Reduction::changed(state, result)
    } else {
        Reduction::unchanged(state, result)
    }
}

fn reduce_messages(
    messages: &mut Vec<Message>,
    action: MessageAction,
) -> (MessageActionResult, bool) {
    match action {
        MessageAction::Hydrate(stored) => {
            let current_ids = messages
                .iter()
                .map(|message| message.id.as_str())
                .collect::<HashSet<_>>();
            let current_deduplication_keys = messages
                .iter()
                .filter_map(|message| {
                    message
                        .deduplication_key
                        .as_deref()
                        .map(|key| (message.channel.as_str(), key))
                })
                .collect::<HashSet<_>>();
            let mut hydrated = stored
                .into_iter()
                .filter(|message| {
                    !current_ids.contains(message.id.as_str())
                        && !message.deduplication_key.as_deref().is_some_and(|key| {
                            current_deduplication_keys.contains(&(message.channel.as_str(), key))
                        })
                })
                .collect::<Vec<_>>();
            let changed = !hydrated.is_empty();
            hydrated.append(messages);
            hydrated.sort_by_key(|message| std::cmp::Reverse(message.created_at));
            hydrated.truncate(MAX_STORED_MESSAGES);
            let count = hydrated.len();
            *messages = hydrated;
            (MessageActionResult::Count(count), changed)
        }
        MessageAction::Publish(message) => {
            if let Some(key) = message.deduplication_key.as_deref()
                && let Some(existing) = messages.iter().find(|existing| {
                    existing.channel == message.channel
                        && existing.deduplication_key.as_deref() == Some(key)
                })
            {
                return (
                    MessageActionResult::Publish {
                        message: Box::new(existing.clone()),
                        inserted: false,
                    },
                    false,
                );
            }
            messages.insert(0, (*message).clone());
            messages.truncate(MAX_STORED_MESSAGES);
            (
                MessageActionResult::Publish {
                    message,
                    inserted: true,
                },
                true,
            )
        }
        MessageAction::SetRead { id, read } => {
            let changed = messages
                .iter_mut()
                .find(|message| message.id == id)
                .is_some_and(|message| {
                    let changed = message.read != read;
                    message.read = read;
                    changed
                });
            (MessageActionResult::Changed(changed), changed)
        }
        MessageAction::MarkAllRead => {
            let mut changed = 0;
            for message in messages.iter_mut().filter(|message| !message.read) {
                message.read = true;
                changed += 1;
            }
            (MessageActionResult::Count(changed), changed > 0)
        }
        MessageAction::Remove { id } => {
            let before = messages.len();
            messages.retain(|message| message.id != id);
            let changed = messages.len() != before;
            (MessageActionResult::Changed(changed), changed)
        }
        MessageAction::Clear => {
            let removed = messages.len();
            messages.clear();
            (MessageActionResult::Count(removed), removed > 0)
        }
    }
}

fn normalized_bounded(value: &str, fallback: &str, max_chars: usize) -> String {
    let value = value.trim();
    if value.is_empty() {
        truncate_chars(fallback, max_chars)
    } else {
        truncate_chars(value, max_chars)
    }
}

fn bounded_metadata(metadata: BTreeMap<String, String>) -> BTreeMap<String, String> {
    metadata
        .into_iter()
        .filter_map(|(key, value)| {
            let key = key.trim();
            if key.is_empty() {
                return None;
            }
            Some((
                truncate_chars(key, MAX_METADATA_KEY_CHARS),
                truncate_chars(value.trim(), MAX_METADATA_VALUE_CHARS),
            ))
        })
        .take(MAX_METADATA_ENTRIES)
        .collect()
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::{
        DesktopNotifier, Message, MessageDraft, MessageSeverity, NativeDelivery,
        NoopDesktopNotifier,
    };

    use super::MessageCenter;

    fn temporary_path(test_name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "usher-message-center-{test_name}-{}-{}.json",
            std::process::id(),
            super::MESSAGE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }

    #[derive(Default)]
    struct CountingNotifier(AtomicUsize);

    impl DesktopNotifier for CountingNotifier {
        fn show(&self, _message: &Message) -> Result<(), String> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    #[test]
    fn publish_and_read_state_update_unread_count() {
        let path = temporary_path("unread");
        let center = MessageCenter::open_with(&path, Arc::new(NoopDesktopNotifier)).unwrap();
        let message = center
            .publish(
                MessageDraft::new("Build finished", "All checks passed")
                    .severity(MessageSeverity::Success)
                    .desktop(false),
            )
            .unwrap();
        assert_eq!(center.snapshot().unread_count, 1);
        assert!(center.set_read(&message.id, true).unwrap());
        assert_eq!(center.snapshot().unread_count, 0);
        center.shutdown();
    }

    #[test]
    fn persisted_messages_reload_in_newest_first_order() {
        let path = temporary_path("reload");
        let center = MessageCenter::open_with(&path, Arc::new(NoopDesktopNotifier)).unwrap();
        center
            .publish(MessageDraft::new("First", "one").desktop(false))
            .unwrap();
        center
            .publish(MessageDraft::new("Second", "two").desktop(false))
            .unwrap();
        center.flush().unwrap();
        center.shutdown();

        let reloaded = MessageCenter::open_with(&path, Arc::new(NoopDesktopNotifier)).unwrap();
        let snapshot = reloaded.snapshot();
        assert_eq!(snapshot.messages.len(), 2);
        assert_eq!(snapshot.messages[0].title, "Second");
        reloaded.shutdown();
    }

    #[test]
    fn deferred_hydration_merges_messages_published_during_startup() {
        let path = temporary_path("deferred-hydration");
        let writer = MessageCenter::open_with(&path, Arc::new(NoopDesktopNotifier)).unwrap();
        writer
            .publish(MessageDraft::new("Stored", "before restart").desktop(false))
            .unwrap();
        writer.flush().unwrap();
        writer.shutdown();

        let center =
            MessageCenter::open_deferred_with(&path, Arc::new(NoopDesktopNotifier)).unwrap();
        assert!(center.snapshot().messages.is_empty());
        center
            .publish(MessageDraft::new("Live", "during startup").desktop(false))
            .unwrap();

        assert_eq!(center.hydrate_from_storage().unwrap(), 1);
        let snapshot = center.snapshot();
        assert_eq!(snapshot.messages.len(), 2);
        assert!(
            snapshot
                .messages
                .iter()
                .any(|message| message.title == "Stored")
        );
        assert!(
            snapshot
                .messages
                .iter()
                .any(|message| message.title == "Live")
        );
        center.flush().unwrap();
        center.shutdown();

        let reloaded = MessageCenter::open_with(&path, Arc::new(NoopDesktopNotifier)).unwrap();
        assert_eq!(reloaded.snapshot().messages.len(), 2);
        reloaded.shutdown();
    }

    #[test]
    fn subscribers_hydrate_immediately_and_follow_off_page_publishes() {
        let path = temporary_path("automatic-subscription");
        let center = MessageCenter::open_with(&path, Arc::new(NoopDesktopNotifier)).unwrap();
        center
            .publish(MessageDraft::new("Before page", "already in Store").desktop(false))
            .unwrap();

        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen_for_listener = seen.clone();
        let _subscription = center.subscribe(Arc::new(move |snapshot| {
            seen_for_listener
                .lock()
                .unwrap()
                .push((snapshot.messages.len(), snapshot.unread_count));
        }));
        assert_eq!(*seen.lock().unwrap(), vec![(1, 1)]);

        center
            .publish(MessageDraft::new("Another route", "new data").desktop(false))
            .unwrap();
        assert_eq!(*seen.lock().unwrap(), vec![(1, 1), (2, 2)]);
        center.shutdown();
    }

    #[test]
    fn every_changed_action_is_automatically_persisted() {
        let path = temporary_path("persist-actions");
        let center = MessageCenter::open_with(&path, Arc::new(NoopDesktopNotifier)).unwrap();
        let first = center
            .publish(MessageDraft::new("First", "one").desktop(false))
            .unwrap();
        let second = center
            .publish(MessageDraft::new("Second", "two").desktop(false))
            .unwrap();
        assert!(center.set_read(&first.id, true).unwrap());
        assert!(center.remove(&second.id).unwrap());
        center.flush().unwrap();
        center.shutdown();

        let reloaded = MessageCenter::open_with(&path, Arc::new(NoopDesktopNotifier)).unwrap();
        let snapshot = reloaded.snapshot();
        assert_eq!(snapshot.messages.len(), 1);
        assert_eq!(snapshot.messages[0].id, first.id);
        assert!(snapshot.messages[0].read);
        assert_eq!(snapshot.unread_count, 0);
        reloaded.shutdown();
    }

    #[test]
    fn adapter_retries_are_idempotent_within_the_same_channel() {
        let path = temporary_path("deduplication");
        let center = MessageCenter::open_with(&path, Arc::new(NoopDesktopNotifier)).unwrap();

        let first = center
            .publish(
                MessageDraft::new("Deployment finished", "revision one")
                    .channel("webhook")
                    .category(crate::categories::WEBHOOK)
                    .deduplication_key("delivery-42")
                    .metadata("provider", "example")
                    .desktop(false),
            )
            .unwrap();
        let retry = center
            .publish(
                MessageDraft::new("Deployment finished again", "duplicate")
                    .channel("webhook")
                    .category(crate::categories::WEBHOOK)
                    .deduplication_key(" delivery-42 ")
                    .desktop(false),
            )
            .unwrap();
        let application_message = center
            .publish(
                MessageDraft::new("Local event", "same provider key")
                    .channel("application")
                    .deduplication_key("delivery-42")
                    .desktop(false),
            )
            .unwrap();

        assert_eq!(retry.id, first.id);
        assert_ne!(application_message.id, first.id);
        assert_eq!(center.snapshot().messages.len(), 2);
        assert_eq!(center.snapshot().unread_count, 2);
        center.flush().unwrap();
        center.shutdown();

        let reloaded = MessageCenter::open_with(&path, Arc::new(NoopDesktopNotifier)).unwrap();
        let reloaded_snapshot = reloaded.snapshot();
        let webhook_message = reloaded_snapshot
            .messages
            .iter()
            .find(|message| message.channel == "webhook")
            .unwrap();
        assert_eq!(webhook_message.category, crate::categories::WEBHOOK);
        assert_eq!(
            webhook_message.deduplication_key.as_deref(),
            Some("delivery-42")
        );
        assert_eq!(
            webhook_message.metadata.get("provider").map(String::as_str),
            Some("example")
        );
        reloaded.shutdown();
    }

    #[test]
    fn native_delivery_defaults_to_unfocused_and_honors_explicit_policies() {
        let path = temporary_path("native-policy");
        let notifier = Arc::new(CountingNotifier::default());
        let center = MessageCenter::open_with(&path, notifier.clone()).unwrap();

        center
            .publish(MessageDraft::new("Foreground", "default policy"))
            .unwrap();
        center.set_application_active(false);
        center
            .publish(MessageDraft::new("Background", "default policy"))
            .unwrap();
        center.set_application_active(true);
        center
            .publish(
                MessageDraft::new("Critical", "always deliver")
                    .native_delivery(NativeDelivery::Always),
            )
            .unwrap();
        center.set_application_active(false);
        center
            .publish(
                MessageDraft::new("Silent", "never deliver").native_delivery(NativeDelivery::Never),
            )
            .unwrap();
        center.flush().unwrap();

        assert_eq!(center.snapshot().messages.len(), 4);
        center.flush_native().unwrap();
        assert_eq!(notifier.0.load(Ordering::Relaxed), 2);
        center.shutdown();
    }
}

#[cfg(test)]
#[path = "center_tests.rs"]
mod persistence_tests;
