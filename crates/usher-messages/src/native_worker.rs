use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use crate::{DesktopNotifier, Message};

pub(crate) const MAX_NATIVE_MESSAGES: usize = 64;
pub(crate) const MAX_NATIVE_PAYLOAD_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_SINGLE_NATIVE_PAYLOAD_BYTES: usize = 128 * 1024;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NativeStats {
    pub owned_messages: usize,
    pub payload_bytes: usize,
    pub peak_owned_messages: usize,
    pub peak_payload_bytes: usize,
    pub accepted: u64,
    pub completed: u64,
    pub rejected: u64,
    pub failed: u64,
}

struct Delivery {
    sequence: u64,
    bytes: usize,
    message: Box<Message>,
}

#[derive(Default)]
struct State {
    queue: VecDeque<Delivery>,
    stats: NativeStats,
    closing: bool,
    stopped: bool,
    fatal_error: Option<String>,
}

#[derive(Default)]
struct Mailbox {
    state: Mutex<State>,
    wake: Condvar,
}

/// Native notifications are events: admitted entries retain their FIFO order.
/// Bytes and entries remain charged until the notifier returns, including the
/// active call. Persistence has a separate worker and barrier.
pub(crate) struct NativeWorker {
    mailbox: Arc<Mailbox>,
    handle: Mutex<Option<JoinHandle<()>>>,
    thread_id: std::thread::ThreadId,
}

impl NativeWorker {
    pub(crate) fn start(notifier: Arc<dyn DesktopNotifier>) -> std::io::Result<Self> {
        let mailbox = Arc::new(Mailbox::default());
        let work = mailbox.clone();
        let handle = std::thread::Builder::new()
            .name("usher-native-notifications".into())
            .spawn(move || {
                loop {
                    let delivery = {
                        let mut state = work.state.lock().unwrap_or_else(|p| p.into_inner());
                        loop {
                            if let Some(delivery) = state.queue.pop_front() {
                                break delivery;
                            }
                            if state.closing {
                                state.stopped = true;
                                work.wake.notify_all();
                                return;
                            }
                            state = work.wake.wait(state).unwrap_or_else(|p| p.into_inner());
                        }
                    };
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        notifier.show(&delivery.message)
                    }));
                    let panicked = result.is_err();
                    let result = result.unwrap_or_else(|_| Err("native notifier panicked".into()));
                    if let Err(error) = &result {
                        log::warn!("[Messages] OS notification unavailable: {error}");
                    }
                    let mut state = work.state.lock().unwrap_or_else(|p| p.into_inner());
                    if panicked {
                        state.fatal_error = Some("native notifier panicked".into());
                    }
                    state.stats.owned_messages -= 1;
                    state.stats.payload_bytes -= delivery.bytes;
                    state.stats.completed = delivery.sequence;
                    state.stats.failed = state
                        .stats
                        .failed
                        .saturating_add(u64::from(result.is_err()));
                    // Drop payload ownership before satisfying its delivery barrier.
                    drop(delivery);
                    work.wake.notify_all();
                }
            })?;
        let thread_id = handle.thread().id();
        Ok(Self {
            mailbox,
            handle: Mutex::new(Some(handle)),
            thread_id,
        })
    }

    /// Never waits for capacity or native delivery. A rejected native event
    /// degrades to the already accepted durable Inbox message.
    pub(crate) fn try_enqueue(&self, message: &Message) -> bool {
        let bytes = payload_bytes(message);
        let mut state = self.mailbox.state.lock().unwrap_or_else(|p| p.into_inner());
        let over_budget = bytes > MAX_SINGLE_NATIVE_PAYLOAD_BYTES
            || state.stats.owned_messages >= MAX_NATIVE_MESSAGES
            || bytes > MAX_NATIVE_PAYLOAD_BYTES.saturating_sub(state.stats.payload_bytes);
        if state.closing || over_budget || state.stats.accepted == u64::MAX {
            state.stats.rejected = state.stats.rejected.saturating_add(1);
            let rejected = state.stats.rejected;
            let owned = state.stats.owned_messages;
            let payload = state.stats.payload_bytes;
            drop(state);
            // Count every rejection, but bound repeated overload diagnostics.
            if rejected.is_power_of_two() {
                log::warn!(
                    "[Messages] Native delivery rejected rejected={rejected} owned={owned} payload_bytes={payload} incoming_bytes={bytes}"
                );
            }
            return false;
        }
        state.stats.accepted += 1;
        let sequence = state.stats.accepted;
        state.queue.push_back(Delivery {
            sequence,
            bytes,
            message: Box::new(message.clone()),
        });
        state.stats.owned_messages += 1;
        state.stats.payload_bytes += bytes;
        state.stats.peak_owned_messages = state
            .stats
            .peak_owned_messages
            .max(state.stats.owned_messages);
        state.stats.peak_payload_bytes = state
            .stats
            .peak_payload_bytes
            .max(state.stats.payload_bytes);
        drop(state);
        self.mailbox.wake.notify_one();
        true
    }

    pub(crate) fn stats(&self) -> NativeStats {
        self.mailbox
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .stats
    }

    pub(crate) fn is_current_worker(&self) -> bool {
        std::thread::current().id() == self.thread_id
    }

    pub(crate) fn flush(&self) -> Result<(), String> {
        let mut state = self.mailbox.state.lock().unwrap_or_else(|p| p.into_inner());
        let target = state.stats.accepted;
        loop {
            if let Some(error) = &state.fatal_error {
                return Err(error.clone());
            }
            if state.stats.completed >= target {
                return Ok(());
            }
            if state.stopped {
                return Err("native worker stopped before delivery barrier".into());
            }
            state = self
                .mailbox
                .wake
                .wait(state)
                .unwrap_or_else(|p| p.into_inner());
        }
    }

    pub(crate) fn shutdown(&self) -> Result<(), String> {
        if self.is_current_worker() {
            return Err("native notification worker cannot wait for itself".into());
        }
        let mut handle = self.handle.lock().unwrap_or_else(|p| p.into_inner());
        {
            let mut state = self.mailbox.state.lock().unwrap_or_else(|p| p.into_inner());
            state.closing = true;
            self.mailbox.wake.notify_all();
        }
        if let Some(handle) = handle.take()
            && handle.join().is_err()
        {
            let mut state = self.mailbox.state.lock().unwrap_or_else(|p| p.into_inner());
            state.fatal_error = Some("native notification worker panicked".into());
            state.stopped = true;
            self.mailbox.wake.notify_all();
        }
        let result = self.flush();
        let stats = self.stats();
        log::debug!(
            "[Messages] Native worker drained accepted={} completed={} rejected={} failed={} peak_owned={} peak_payload_bytes={}",
            stats.accepted,
            stats.completed,
            stats.rejected,
            stats.failed,
            stats.peak_owned_messages,
            stats.peak_payload_bytes
        );
        result
    }
}

impl Drop for NativeWorker {
    fn drop(&mut self) {
        let mut state = self.mailbox.state.lock().unwrap_or_else(|p| p.into_inner());
        state.closing = true;
        self.mailbox.wake.notify_all();
    }
}

/// UTF-8 payload bytes, excluding fixed object/allocator overhead. The entry
/// limit separately bounds the number of owned messages. Publication already
/// normalizes metadata to the message-center's bounded field contract.
pub(crate) fn payload_bytes(message: &Message) -> usize {
    message.id.len()
        + message.title.len()
        + message.body.len()
        + message.source.len()
        + message.channel.len()
        + message.category.len()
        + message.target.as_ref().map_or(0, String::len)
        + message.deduplication_key.as_ref().map_or(0, String::len)
        + message
            .metadata
            .iter()
            .map(|(key, value)| key.len() + value.len())
            .sum::<usize>()
}
