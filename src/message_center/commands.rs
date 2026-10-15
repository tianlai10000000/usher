//! Ordered, nonblocking admission for message mutations originating in the UI.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;

use usher_messages::{Message, MessageCenter, MessageCenterError, MessageDraft};

use crate::startup::StartupTimeline;

const MAX_COMMANDS: usize = 256;
const MAX_COMMAND_BYTES: usize = 1024 * 1024;
type Published = Box<dyn FnOnce(Result<Message, MessageCenterError>) + Send>;

enum Command {
    Publish(MessageDraft, Published),
    SetRead(String, bool),
    MarkAllRead,
    Remove(String),
    Clear,
    Hydrate(StartupTimeline),
    #[cfg(test)]
    Barrier(mpsc::Sender<()>),
}

struct Envelope {
    command: Command,
    bytes: usize,
}

struct Admission {
    sender: Option<mpsc::SyncSender<Envelope>>,
    entries: usize,
    bytes: usize,
    hydration_submitted: bool,
}

struct Lifecycle {
    worker: Option<JoinHandle<Result<(), String>>>,
    result: Option<Result<(), String>>,
}

struct Inner {
    admission: Arc<Mutex<Admission>>,
    lifecycle: Mutex<Lifecycle>,
    worker_id: std::thread::ThreadId,
}

#[derive(Clone)]
pub(crate) struct MessageCommandWorker(Arc<Inner>);

impl MessageCommandWorker {
    pub(crate) fn start(center: MessageCenter, deferred: bool) -> std::io::Result<Self> {
        // One reserved control slot lets first-frame hydration follow accepted
        // business commands even when their active-inclusive budget is full.
        let (sender, receiver) = mpsc::sync_channel::<Envelope>(MAX_COMMANDS + 1);
        let admission = Arc::new(Mutex::new(Admission {
            sender: Some(sender),
            entries: 0,
            bytes: 0,
            hydration_submitted: !deferred,
        }));
        let release = Arc::downgrade(&admission);
        let worker = std::thread::Builder::new()
            .name("message-ui-commands".into())
            .spawn(move || {
                let mut needs_hydration = deferred;
                let mut before_hydration = VecDeque::new();
                loop {
                    if needs_hydration {
                        match receiver.recv() {
                            Ok(Envelope {
                                command: Command::Hydrate(timeline),
                                ..
                            }) => hydrate(&center, Some(&timeline)),
                            Ok(envelope) => {
                                before_hydration.push_back(envelope);
                                continue;
                            }
                            Err(_) => hydrate(&center, None),
                        }
                        needs_hydration = false;
                    }
                    let Some(envelope) = before_hydration
                        .pop_front()
                        .or_else(|| receiver.recv().ok())
                    else {
                        break;
                    };
                    match envelope.command {
                        Command::Publish(draft, completion) => completion(center.publish(draft)),
                        Command::SetRead(id, read) => report(center.set_read(&id, read)),
                        Command::MarkAllRead => report(center.mark_all_read()),
                        Command::Remove(id) => report(center.remove(&id)),
                        Command::Clear => report(center.clear()),
                        Command::Hydrate(_) => {}
                        #[cfg(test)]
                        Command::Barrier(completion) => {
                            let _ = completion.send(());
                        }
                    }
                    if envelope.bytes != 0
                        && let Some(admission) = release.upgrade()
                    {
                        let mut admission = admission.lock().unwrap_or_else(|p| p.into_inner());
                        admission.entries -= 1;
                        admission.bytes -= envelope.bytes;
                    }
                }
                // Background publishers retain their admission until their
                // own shutdown stage; the composition root closes the center.
                center.flush().map_err(|error| error.to_string())
            })?;
        Ok(Self(Arc::new(Inner {
            admission,
            worker_id: worker.thread().id(),
            lifecycle: Mutex::new(Lifecycle {
                worker: Some(worker),
                result: None,
            }),
        })))
    }

    fn submit(&self, command: Command, bytes: usize) -> Result<(), String> {
        let bytes = bytes.saturating_add(std::mem::size_of::<Envelope>());
        let mut admission = self.0.admission.lock().unwrap_or_else(|p| p.into_inner());
        if admission.entries >= MAX_COMMANDS
            || bytes > MAX_COMMAND_BYTES.saturating_sub(admission.bytes)
        {
            return Err("message command entry/byte budget exhausted".into());
        }
        admission
            .sender
            .as_ref()
            .ok_or("message commands are closed")?
            .try_send(Envelope { command, bytes })
            .map_err(|error| format!("message command admission failed: {error}"))?;
        admission.entries += 1;
        admission.bytes += bytes;
        Ok(())
    }

    pub(crate) fn publish(
        &self,
        draft: MessageDraft,
        completion: impl FnOnce(Result<Message, MessageCenterError>) + Send + 'static,
    ) -> Result<(), String> {
        let bytes = [
            &draft.title,
            &draft.body,
            &draft.source,
            &draft.channel,
            &draft.category,
        ]
        .into_iter()
        .map(|value| value.capacity())
        .sum::<usize>()
        .saturating_add(draft.target.as_ref().map_or(0, String::capacity))
        .saturating_add(draft.deduplication_key.as_ref().map_or(0, String::capacity))
        .saturating_add(
            draft
                .metadata
                .iter()
                .map(|(key, value)| {
                    key.capacity()
                        .saturating_add(value.capacity())
                        .saturating_add(std::mem::size_of::<(String, String)>())
                })
                .sum::<usize>(),
        );
        self.submit(Command::Publish(draft, Box::new(completion)), bytes)
    }

    pub(crate) fn set_read(&self, id: String, read: bool) -> Result<(), String> {
        let bytes = id.capacity();
        self.submit(Command::SetRead(id, read), bytes)
    }

    pub(crate) fn mark_all_read(&self) -> Result<(), String> {
        self.submit(Command::MarkAllRead, 0)
    }
    pub(crate) fn remove(&self, id: String) -> Result<(), String> {
        let bytes = id.capacity();
        self.submit(Command::Remove(id), bytes)
    }
    pub(crate) fn clear(&self) -> Result<(), String> {
        self.submit(Command::Clear, 0)
    }

    pub(crate) fn hydrate(&self, timeline: StartupTimeline) -> Result<(), String> {
        let mut admission = self.0.admission.lock().unwrap_or_else(|p| p.into_inner());
        if admission.hydration_submitted {
            return Ok(());
        }
        admission
            .sender
            .as_ref()
            .ok_or("message commands are closed")?
            .try_send(Envelope {
                command: Command::Hydrate(timeline),
                bytes: 0,
            })
            .map_err(|error| format!("message hydration admission failed: {error}"))?;
        admission.hydration_submitted = true;
        Ok(())
    }

    /// Blocking barrier, invoked only by the application's background drain.
    pub(crate) fn shutdown(&self) -> Result<(), String> {
        if self.0.worker_id == std::thread::current().id() {
            return Err("message command worker cannot join itself".into());
        }
        let mut lifecycle = self.0.lifecycle.lock().unwrap_or_else(|p| p.into_inner());
        self.0
            .admission
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .sender
            .take();
        if let Some(worker) = lifecycle.worker.take() {
            lifecycle.result = Some(
                worker
                    .join()
                    .unwrap_or_else(|_| Err("message command worker panicked".into())),
            );
        }
        lifecycle.result.clone().unwrap_or(Ok(()))
    }
}

fn hydrate(center: &MessageCenter, timeline: Option<&StartupTimeline>) {
    match center.hydrate_from_storage() {
        Ok(count) => {
            log::info!("[Messages] Deferred hydration loaded {count} messages");
            if let Some(timeline) = timeline {
                timeline.checkpoint("messages_hydrated");
            }
        }
        Err(error) => log::error!("[Messages] Deferred hydration failed: {error}"),
    }
}

fn report<T>(result: Result<T, MessageCenterError>) {
    if let Err(error) = result {
        log::error!("[Messages] Queued mutation failed: {error}");
    }
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
