use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::thread::{self, JoinHandle};

use chrono::Utc;
use std::collections::BTreeMap;

use crate::desktop::{DesktopNotifier, NotifyRustDesktopNotifier};
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

type Listener = Arc<dyn Fn(MessageSnapshot) + Send + Sync>;

#[derive(Debug)]
pub enum MessageCenterError {
    Storage(std::io::Error),
    WorkerUnavailable,
}

impl std::fmt::Display for MessageCenterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(error) => write!(formatter, "message storage error: {error}"),
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
    pub messages: Vec<Message>,
    pub unread_count: usize,
}

impl MessageSnapshot {
    fn from_messages(messages: Vec<Message>) -> Self {
        let unread_count = messages.iter().filter(|message| !message.read).count();
        Self {
            messages,
            unread_count,
        }
    }
}

#[derive(Clone)]
pub struct MessageCenter {
    inner: Arc<Inner>,
}

struct Inner {
    messages: RwLock<Vec<Message>>,
    listeners: Mutex<Vec<(u64, Listener)>>,
    next_listener_id: AtomicU64,
    application_active: AtomicBool,
    worker: Mutex<Option<Worker>>,
}

struct Worker {
    sender: mpsc::Sender<WorkerCommand>,
    join: Option<JoinHandle<()>>,
}

enum WorkerCommand {
    Persist(Vec<Message>),
    Notify(Message),
    Flush(mpsc::Sender<()>),
    Shutdown(mpsc::Sender<()>),
}

/// Removes a snapshot listener when dropped.
pub struct Subscription {
    inner: std::sync::Weak<Inner>,
    id: u64,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        if let Ok(mut listeners) = inner.listeners.lock() {
            listeners.retain(|(id, _)| *id != self.id);
        }
    }
}

impl MessageCenter {
    pub fn open_default() -> Result<Self, MessageCenterError> {
        Self::open_with(
            storage::default_storage_path()?,
            Arc::new(NotifyRustDesktopNotifier),
        )
    }

    pub fn open_with(
        path: impl Into<PathBuf>,
        notifier: Arc<dyn DesktopNotifier>,
    ) -> Result<Self, MessageCenterError> {
        let path = path.into();
        let mut messages = match storage::load(&path) {
            Ok(messages) => messages,
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                log::error!("[Messages] Ignoring corrupt message store: {error}");
                Vec::new()
            }
            Err(error) => return Err(error.into()),
        };
        messages.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        messages.truncate(MAX_STORED_MESSAGES);
        let worker = Worker::start(path, notifier)?;
        Ok(Self {
            inner: Arc::new(Inner {
                messages: RwLock::new(messages),
                listeners: Mutex::new(Vec::new()),
                next_listener_id: AtomicU64::new(1),
                // Unknown activity is treated as foreground. This conservative
                // default prevents duplicate native + in-app presentation until
                // the host supplies its first focus state.
                application_active: AtomicBool::new(true),
                worker: Mutex::new(Some(worker)),
            }),
        })
    }

    pub fn snapshot(&self) -> MessageSnapshot {
        let messages = self
            .inner
            .messages
            .read()
            .map(|messages| messages.clone())
            .unwrap_or_default();
        MessageSnapshot::from_messages(messages)
    }

    pub fn subscribe(&self, listener: Listener) -> Subscription {
        let id = self.inner.next_listener_id.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut listeners) = self.inner.listeners.lock() {
            listeners.push((id, listener));
        }
        Subscription {
            inner: Arc::downgrade(&self.inner),
            id,
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

    pub fn publish(&self, draft: MessageDraft) -> Result<Message, MessageCenterError> {
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

        let mut duplicate = None;
        let snapshot = self.mutate(|messages| {
            if let Some(key) = message.deduplication_key.as_deref()
                && let Some(existing) = messages.iter().find(|existing| {
                    existing.channel == message.channel
                        && existing.deduplication_key.as_deref() == Some(key)
                })
            {
                duplicate = Some(existing.clone());
                return false;
            }
            messages.insert(0, message.clone());
            messages.truncate(MAX_STORED_MESSAGES);
            true
        })?;
        if let Some(existing) = duplicate {
            return Ok(existing);
        }
        if native_delivery.should_deliver(self.application_active()) {
            self.send_worker(WorkerCommand::Notify(message.clone()))?;
        }
        self.publish_snapshot(snapshot);
        Ok(message)
    }

    pub fn set_read(&self, id: &str, read: bool) -> Result<bool, MessageCenterError> {
        let mut changed = false;
        let snapshot = self.mutate(|messages| {
            if let Some(message) = messages.iter_mut().find(|message| message.id == id) {
                changed = message.read != read;
                message.read = read;
            }
            changed
        })?;
        if changed {
            self.publish_snapshot(snapshot);
        }
        Ok(changed)
    }

    pub fn mark_all_read(&self) -> Result<usize, MessageCenterError> {
        let mut changed = 0;
        let snapshot = self.mutate(|messages| {
            for message in messages.iter_mut().filter(|message| !message.read) {
                message.read = true;
                changed += 1;
            }
            changed > 0
        })?;
        if changed > 0 {
            self.publish_snapshot(snapshot);
        }
        Ok(changed)
    }

    pub fn remove(&self, id: &str) -> Result<bool, MessageCenterError> {
        let mut changed = false;
        let snapshot = self.mutate(|messages| {
            let before = messages.len();
            messages.retain(|message| message.id != id);
            changed = messages.len() != before;
            changed
        })?;
        if changed {
            self.publish_snapshot(snapshot);
        }
        Ok(changed)
    }

    pub fn clear(&self) -> Result<usize, MessageCenterError> {
        let mut removed = 0;
        let snapshot = self.mutate(|messages| {
            removed = messages.len();
            messages.clear();
            removed > 0
        })?;
        if removed > 0 {
            self.publish_snapshot(snapshot);
        }
        Ok(removed)
    }

    pub fn flush(&self) -> Result<(), MessageCenterError> {
        let (sender, receiver) = mpsc::channel();
        self.send_worker(WorkerCommand::Flush(sender))?;
        receiver
            .recv()
            .map_err(|_| MessageCenterError::WorkerUnavailable)
    }

    pub fn shutdown(&self) {
        let worker = self
            .inner
            .worker
            .lock()
            .ok()
            .and_then(|mut worker| worker.take());
        let Some(mut worker) = worker else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        let _ = worker.sender.send(WorkerCommand::Shutdown(sender));
        let _ = receiver.recv();
        if let Some(join) = worker.join.take() {
            let _ = join.join();
        }
    }

    fn mutate(
        &self,
        mutation: impl FnOnce(&mut Vec<Message>) -> bool,
    ) -> Result<MessageSnapshot, MessageCenterError> {
        let (snapshot, changed) = {
            let mut messages = self
                .inner
                .messages
                .write()
                .map_err(|_| MessageCenterError::WorkerUnavailable)?;
            let changed = mutation(&mut messages);
            (MessageSnapshot::from_messages(messages.clone()), changed)
        };
        if changed {
            self.send_worker(WorkerCommand::Persist(snapshot.messages.clone()))?;
        }
        Ok(snapshot)
    }

    fn send_worker(&self, command: WorkerCommand) -> Result<(), MessageCenterError> {
        let sender = self
            .inner
            .worker
            .lock()
            .ok()
            .and_then(|worker| worker.as_ref().map(|worker| worker.sender.clone()))
            .ok_or(MessageCenterError::WorkerUnavailable)?;
        sender
            .send(command)
            .map_err(|_| MessageCenterError::WorkerUnavailable)
    }

    fn publish_snapshot(&self, snapshot: MessageSnapshot) {
        let listeners = self
            .inner
            .listeners
            .lock()
            .map(|listeners| {
                listeners
                    .iter()
                    .map(|(_, listener)| listener.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for listener in listeners {
            listener(snapshot.clone());
        }
    }
}

impl Worker {
    fn start(
        path: PathBuf,
        notifier: Arc<dyn DesktopNotifier>,
    ) -> Result<Self, MessageCenterError> {
        let (sender, receiver) = mpsc::channel();
        let join = thread::Builder::new()
            .name("usher-message-center".to_string())
            .spawn(move || worker_loop(&path, notifier.as_ref(), receiver))
            .map_err(MessageCenterError::Storage)?;
        Ok(Self {
            sender,
            join: Some(join),
        })
    }
}

fn worker_loop(
    path: &Path,
    notifier: &dyn DesktopNotifier,
    receiver: mpsc::Receiver<WorkerCommand>,
) {
    while let Ok(command) = receiver.recv() {
        match command {
            WorkerCommand::Persist(messages) => {
                if let Err(error) = storage::save(path, &messages) {
                    log::error!("[Messages] Failed to persist message center: {error}");
                }
            }
            WorkerCommand::Notify(message) => {
                if let Err(error) = notifier.show(&message) {
                    log::warn!("[Messages] OS notification unavailable: {error}");
                }
            }
            WorkerCommand::Flush(acknowledge) => {
                let _ = acknowledge.send(());
            }
            WorkerCommand::Shutdown(acknowledge) => {
                let _ = acknowledge.send(());
                break;
            }
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
        let webhook_message = reloaded
            .snapshot()
            .messages
            .into_iter()
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
        assert_eq!(notifier.0.load(Ordering::Relaxed), 2);
        center.shutdown();
    }
}
