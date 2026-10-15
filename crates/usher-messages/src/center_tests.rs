use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak, mpsc};
use std::time::{Duration, Instant};

use super::{MessageCenter, MessageCenterError, MessageSnapshot};
use crate::{DesktopNotifier, Message, MessageDraft, NoopDesktopNotifier};

const WAIT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct Gate {
    state: Mutex<(bool, bool)>,
    wake: Condvar,
}

impl Gate {
    pub(super) fn block(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.wake.notify_all();
        while !state.1 {
            let (next, timeout) = self.wake.wait_timeout(state, WAIT).unwrap();
            state = next;
            assert!(!timeout.timed_out(), "test gate was not released");
        }
    }

    pub(super) fn wait_entered(&self) {
        let mut state = self.state.lock().unwrap();
        while !state.0 {
            let (next, timeout) = self.wake.wait_timeout(state, WAIT).unwrap();
            state = next;
            assert!(!timeout.timed_out(), "worker did not enter test gate");
        }
    }

    pub(super) fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.wake.notify_all();
    }
}

pub(super) fn path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "usher-p75-{name}-{}-{}.json",
        std::process::id(),
        super::MESSAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

pub(super) fn seed(count: usize) -> Vec<Message> {
    (0..count)
        .map(|i| Message {
            id: format!("seed-{i}"),
            title: format!("Seed {i}"),
            body: "b".repeat(2048),
            source: "test".into(),
            channel: "application".into(),
            category: "general".into(),
            severity: crate::MessageSeverity::Info,
            created_at: chrono::Utc::now(),
            read: false,
            target: None,
            deduplication_key: None,
            metadata: Default::default(),
        })
        .collect()
}

#[test]
fn failed_durable_flush_requires_explicit_same_revision_retry() {
    let destination = path("retry");
    let failing = Arc::new(AtomicBool::new(true));
    let calls = Arc::new(AtomicUsize::new(0));
    let center = MessageCenter::open_initialized_with_persist(
        destination.clone(),
        Arc::new(NoopDesktopNotifier),
        Vec::new(),
        true,
        {
            let failing = failing.clone();
            let calls = calls.clone();
            let destination = destination.clone();
            move |snapshot| {
                calls.fetch_add(1, Ordering::SeqCst);
                if failing.load(Ordering::SeqCst) {
                    Err("injected storage failure".into())
                } else {
                    crate::storage::save(&destination, &snapshot.messages)
                        .map_err(|error| error.to_string())
                }
            }
        },
    )
    .unwrap();
    center
        .publish(MessageDraft::new("Failure", "retained").desktop(false))
        .unwrap();
    let revision = center.inner.store.snapshot().revision();
    assert!(matches!(
        center.flush(),
        Err(MessageCenterError::Persistence(_))
    ));
    assert!(center.flush().is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!destination.exists());
    failing.store(false, Ordering::SeqCst);
    assert!(center.retry_failed().unwrap());
    center.flush().unwrap();
    assert!(!center.retry_failed().unwrap());
    assert_eq!(center.inner.store.snapshot().revision(), revision);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        crate::storage::load(&destination).unwrap(),
        center.snapshot().messages.to_vec()
    );
    center.shutdown_checked().unwrap();
    std::fs::remove_file(destination).unwrap();
}

#[test]
fn checked_shutdown_reports_durable_failure_without_retrying() {
    let calls = Arc::new(AtomicUsize::new(0));
    let center = MessageCenter::open_initialized_with_persist(
        path("shutdown-error"),
        Arc::new(NoopDesktopNotifier),
        Vec::new(),
        true,
        {
            let calls = calls.clone();
            move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                Err("write denied".into())
            }
        },
    )
    .unwrap();
    center
        .publish(MessageDraft::new("Failure", "durability").desktop(false))
        .unwrap();
    assert!(center.shutdown_checked().is_err());
    assert!(center.shutdown_checked().is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(center.retry_failed().is_err());
}

#[test]
fn blocked_writer_keeps_only_active_and_latest_snapshot_then_reloads_newest() {
    let destination = path("latest");
    let gate = Arc::new(Gate::default());
    let writes = Arc::new(AtomicUsize::new(0));
    let center = MessageCenter::open_initialized_with_persist(
        destination.clone(),
        Arc::new(NoopDesktopNotifier),
        seed(64),
        true,
        {
            let gate = gate.clone();
            let writes = writes.clone();
            let destination = destination.clone();
            move |snapshot| {
                if writes.fetch_add(1, Ordering::SeqCst) == 0 {
                    gate.block();
                }
                crate::storage::save(&destination, &snapshot.messages)
                    .map_err(|error| error.to_string())
            }
        },
    )
    .unwrap();
    let owners = Arc::new(Mutex::new(Vec::<Weak<MessageSnapshot>>::new()));
    let _observer = center.inner.store.observe_commits(Arc::new({
        let owners = owners.clone();
        move |snapshot| {
            owners
                .lock()
                .unwrap()
                .push(Arc::downgrade(&snapshot.state_arc()))
        }
    }));
    assert_eq!(center.mark_all_read().unwrap(), 64);
    gate.wait_entered();
    for i in 0..1000 {
        assert!(
            center
                .set_read(&format!("seed-{}", i % 64), (i / 64) % 2 == 1)
                .unwrap()
        );
    }
    let live = owners
        .lock()
        .unwrap()
        .iter()
        .filter_map(Weak::upgrade)
        .map(|snapshot| Arc::as_ptr(&snapshot) as usize)
        .collect::<HashSet<_>>();
    assert_eq!(live.len(), 2);
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    let (done, result) = mpsc::channel();
    let worker = std::thread::spawn({
        let center = center.clone();
        move || done.send(center.flush()).unwrap()
    });
    assert!(result.recv_timeout(Duration::from_millis(30)).is_err());
    gate.release();
    result.recv_timeout(WAIT).unwrap().unwrap();
    worker.join().unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 2);
    let reloaded = crate::storage::load(&destination).unwrap();
    assert_eq!(reloaded, center.snapshot().messages.to_vec());
    assert_eq!(reloaded.iter().filter(|message| !message.read).count(), 24);
    center.shutdown_checked().unwrap();
    std::fs::remove_file(destination).unwrap();
}

#[test]
fn deferred_empty_hydration_does_not_persist_revision_zero() {
    let destination = path("revision-zero");
    let center =
        MessageCenter::open_deferred_with(&destination, Arc::new(NoopDesktopNotifier)).unwrap();
    assert_eq!(center.hydrate_from_storage().unwrap(), 0);
    assert_eq!(center.inner.store.snapshot().revision(), 0);
    center.flush().unwrap();
    assert!(!destination.exists());
    center.shutdown_checked().unwrap();
    assert!(!destination.exists());
}

#[test]
fn deferred_shutdown_reports_accepted_state_without_finished_hydration() {
    let destination = path("deferred-unhydrated-shutdown");
    let notifier = Arc::new(CountNotifier::default());
    let center = MessageCenter::open_deferred_with(&destination, notifier.clone()).unwrap();
    center
        .publish(MessageDraft::new("Early", "accepted before hydration").desktop(true))
        .unwrap();
    assert!(
        matches!(center.flush(), Err(MessageCenterError::Persistence(error)) if error.contains("hydration"))
    );
    assert!(
        matches!(center.shutdown_checked(), Err(MessageCenterError::Persistence(error)) if error.contains("hydration"))
    );
    assert!(center.shutdown_checked().is_err());
    assert_eq!(notifier.0.load(Ordering::Relaxed), 1);
    assert!(!destination.exists());
}

#[test]
fn deferred_shutdown_without_business_changes_is_successful() {
    let destination = path("deferred-empty-shutdown");
    let center =
        MessageCenter::open_deferred_with(&destination, Arc::new(NoopDesktopNotifier)).unwrap();
    assert!(!center.set_read("missing", true).unwrap());
    assert_eq!(center.mark_all_read().unwrap(), 0);
    center.shutdown_checked().unwrap();
    center.shutdown_checked().unwrap();
    assert!(!destination.exists());
}

#[test]
fn subscribers_can_reenter_snapshot_and_action_creators() {
    let destination = path("reentrant");
    let center = MessageCenter::open_with(&destination, Arc::new(NoopDesktopNotifier)).unwrap();
    let _subscription = center.subscribe(Arc::new({
        let center = center.clone();
        move |snapshot| {
            if let Some(message) = snapshot.messages.iter().find(|message| !message.read) {
                assert!(!center.snapshot().messages.is_empty());
                center.set_read(&message.id, true).unwrap();
            }
        }
    }));
    let (done, result) = mpsc::channel();
    let worker = std::thread::spawn({
        let center = center.clone();
        move || {
            done.send(center.publish(MessageDraft::new("Reentry", "subscriber").desktop(false)))
                .unwrap();
        }
    });
    result.recv_timeout(WAIT).unwrap().unwrap();
    worker.join().unwrap();
    assert_eq!(center.snapshot().unread_count, 0);
    center.shutdown_checked().unwrap();
    std::fs::remove_file(destination).unwrap();
}

#[test]
fn subscriber_shutdown_reentry_fails_without_closing_the_center() {
    let destination = path("subscriber-self-shutdown");
    let center = MessageCenter::open_with(&destination, Arc::new(NoopDesktopNotifier)).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let _subscription = center.subscribe(Arc::new({
        let center = center.clone();
        let calls = calls.clone();
        move |snapshot| {
            if let Some(message) = snapshot.messages.iter().find(|message| !message.read) {
                assert!(matches!(
                    center.shutdown_checked(),
                    Err(MessageCenterError::ReentrantShutdown)
                ));
                calls.fetch_add(1, Ordering::SeqCst);
                center.set_read(&message.id, true).unwrap();
            }
        }
    }));
    let (done, result) = mpsc::channel();
    let worker = std::thread::spawn({
        let center = center.clone();
        move || {
            done.send(center.publish(MessageDraft::new("Reentry", "close").desktop(false)))
                .unwrap();
        }
    });
    result.recv_timeout(WAIT).unwrap().unwrap();
    worker.join().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!center.inner.admission.lock().unwrap().closing);
    center
        .publish(MessageDraft::new("Still usable", "second").desktop(false))
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    center.shutdown_checked().unwrap();
    std::fs::remove_file(destination).unwrap();
}

#[test]
fn subscriber_shutdown_reentry_fails_before_an_external_shutdown_serial_lock() {
    let destination = path("subscriber-racing-shutdown");
    let center = MessageCenter::open_with(&destination, Arc::new(NoopDesktopNotifier)).unwrap();
    let gate = Arc::new(Gate::default());
    let (callback_done, callback_result) = mpsc::channel();
    let _subscription = center.subscribe(Arc::new({
        let center = center.clone();
        let gate = gate.clone();
        move |snapshot| {
            if !snapshot.messages.is_empty() {
                gate.block();
                callback_done
                    .send(matches!(
                        center.shutdown_checked(),
                        Err(MessageCenterError::ReentrantShutdown)
                    ))
                    .unwrap();
            }
        }
    }));
    let publish = std::thread::spawn({
        let center = center.clone();
        move || center.publish(MessageDraft::new("Admitted", "event").desktop(false))
    });
    gate.wait_entered();
    let (done, result) = mpsc::channel();
    let shutdown = std::thread::spawn({
        let center = center.clone();
        move || done.send(center.shutdown_checked()).unwrap()
    });
    let until = Instant::now() + WAIT;
    while !center.inner.admission.lock().unwrap().closing {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    gate.release();
    assert!(callback_result.recv_timeout(WAIT).unwrap());
    publish.join().unwrap().unwrap();
    result.recv_timeout(WAIT).unwrap().unwrap();
    shutdown.join().unwrap();
    assert_eq!(crate::storage::load(&destination).unwrap().len(), 1);
    std::fs::remove_file(destination).unwrap();
}

#[derive(Default)]
struct SelfClosingNotifier {
    center: Mutex<Option<MessageCenter>>,
    refused: AtomicBool,
}

impl DesktopNotifier for SelfClosingNotifier {
    fn show(&self, _: &Message) -> Result<(), String> {
        let center = self.center.lock().unwrap().as_ref().unwrap().clone();
        self.refused.store(
            matches!(
                center.shutdown_checked(),
                Err(MessageCenterError::ReentrantShutdown)
            ),
            Ordering::SeqCst,
        );
        Ok(())
    }
}

#[test]
fn native_notifier_shutdown_reentry_fails_without_self_join() {
    let destination = path("native-self-shutdown");
    let notifier = Arc::new(SelfClosingNotifier::default());
    let center = MessageCenter::open_with(&destination, notifier.clone()).unwrap();
    *notifier.center.lock().unwrap() = Some(center.clone());
    center
        .publish(MessageDraft::new("Native", "reentry").desktop(true))
        .unwrap();
    let (done, result) = mpsc::channel();
    let worker = std::thread::spawn({
        let center = center.clone();
        move || done.send(center.flush_native()).unwrap()
    });
    result.recv_timeout(WAIT).unwrap().unwrap();
    worker.join().unwrap();
    assert!(notifier.refused.load(Ordering::SeqCst));
    assert!(!center.inner.admission.lock().unwrap().closing);
    notifier.center.lock().unwrap().take();
    center.shutdown_checked().unwrap();
    std::fs::remove_file(destination).unwrap();
}

#[derive(Default)]
struct SlowNotifier {
    gate: Gate,
    seen: Mutex<Vec<String>>,
}

impl DesktopNotifier for SlowNotifier {
    fn show(&self, message: &Message) -> Result<(), String> {
        if self.seen.lock().unwrap().is_empty() {
            self.gate.block();
        }
        self.seen.lock().unwrap().push(message.id.clone());
        Ok(())
    }
}

#[test]
fn slow_native_delivery_does_not_block_durable_flush_or_lose_inbox_messages() {
    let destination = path("slow-native");
    let notifier = Arc::new(SlowNotifier::default());
    let center = MessageCenter::open_with(&destination, notifier.clone()).unwrap();
    let first = center
        .publish(MessageDraft::new("0", "event").desktop(true))
        .unwrap();
    notifier.gate.wait_entered();
    let mut expected = vec![first.id];
    for i in 1..100 {
        let message = center
            .publish(MessageDraft::new(i.to_string(), "event").desktop(true))
            .unwrap();
        if i < 64 {
            expected.push(message.id);
        }
    }
    let stats = center.inner.native.stats();
    assert_eq!(stats.owned_messages, 64);
    assert_eq!(stats.accepted, 64);
    assert_eq!(stats.rejected, 36);
    assert!(stats.payload_bytes <= crate::native_worker::MAX_NATIVE_PAYLOAD_BYTES);
    let (done, result) = mpsc::channel();
    let worker = std::thread::spawn({
        let center = center.clone();
        move || done.send(center.flush()).unwrap()
    });
    result.recv_timeout(WAIT).unwrap().unwrap();
    worker.join().unwrap();
    assert_eq!(crate::storage::load(&destination).unwrap().len(), 100);
    let (done, result) = mpsc::channel();
    let native = std::thread::spawn({
        let center = center.clone();
        move || done.send(center.flush_native()).unwrap()
    });
    assert!(result.recv_timeout(Duration::from_millis(30)).is_err());
    notifier.gate.release();
    result.recv_timeout(WAIT).unwrap().unwrap();
    native.join().unwrap();
    assert_eq!(*notifier.seen.lock().unwrap(), expected);
    assert_eq!(center.inner.native.stats().owned_messages, 0);
    center.shutdown_checked().unwrap();
    std::fs::remove_file(destination).unwrap();
}

#[test]
fn native_byte_budget_includes_inflight_and_oversized_single_event_is_rejected() {
    let notifier = Arc::new(SlowNotifier::default());
    let native = crate::native_worker::NativeWorker::start(notifier.clone()).unwrap();
    let mut message = seed(1).remove(0);
    message.body = "x".repeat(100_000);
    assert!(native.try_enqueue(&message));
    notifier.gate.wait_entered();
    for i in 1..30 {
        message.id = i.to_string();
        native.try_enqueue(&message);
    }
    let stats = native.stats();
    assert!(stats.owned_messages < crate::native_worker::MAX_NATIVE_MESSAGES);
    assert_eq!(stats.owned_messages, 10);
    assert!(stats.payload_bytes <= crate::native_worker::MAX_NATIVE_PAYLOAD_BYTES);
    message.body = "x".repeat(crate::native_worker::MAX_SINGLE_NATIVE_PAYLOAD_BYTES + 1);
    assert!(!native.try_enqueue(&message));
    assert_eq!(native.stats().owned_messages, stats.owned_messages);
    assert_eq!(native.stats().payload_bytes, stats.payload_bytes);
    notifier.gate.release();
    native.shutdown().unwrap();
    assert_eq!(notifier.seen.lock().unwrap().len(), 10);
    assert_eq!(native.stats().payload_bytes, 0);
}

#[test]
fn checked_shutdown_waits_for_pending_durable_state_and_admitted_native_events() {
    let destination = path("drain-both");
    let write_gate = Arc::new(Gate::default());
    let notifier = Arc::new(SlowNotifier::default());
    let writes = Arc::new(AtomicUsize::new(0));
    let center = MessageCenter::open_initialized_with_persist(
        destination.clone(),
        notifier.clone(),
        Vec::new(),
        true,
        {
            let gate = write_gate.clone();
            let writes = writes.clone();
            let destination = destination.clone();
            move |snapshot| {
                if writes.fetch_add(1, Ordering::SeqCst) == 0 {
                    gate.block();
                }
                crate::storage::save(&destination, &snapshot.messages)
                    .map_err(|error| error.to_string())
            }
        },
    )
    .unwrap();
    let first = center
        .publish(MessageDraft::new("First", "active").desktop(true))
        .unwrap();
    write_gate.wait_entered();
    notifier.gate.wait_entered();
    let second = center
        .publish(MessageDraft::new("Second", "pending").desktop(true))
        .unwrap();
    let (done, result) = mpsc::channel();
    let shutdown = std::thread::spawn({
        let center = center.clone();
        move || done.send(center.shutdown_checked()).unwrap()
    });
    assert!(result.recv_timeout(Duration::from_millis(30)).is_err());
    write_gate.release();
    // Durable drain alone cannot satisfy the two-worker shutdown barrier.
    assert!(result.recv_timeout(Duration::from_millis(30)).is_err());
    notifier.gate.release();
    result.recv_timeout(WAIT).unwrap().unwrap();
    shutdown.join().unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 2);
    assert_eq!(*notifier.seen.lock().unwrap(), vec![first.id, second.id]);
    assert_eq!(
        crate::storage::load(&destination).unwrap(),
        center.snapshot().messages.to_vec()
    );
    std::fs::remove_file(destination).unwrap();
}

#[test]
fn shutdown_closes_new_actions_but_drains_an_already_admitted_publish() {
    let destination = path("admission");
    let notifier = Arc::new(CountNotifier::default());
    let center = MessageCenter::open_with(&destination, notifier.clone()).unwrap();
    let gate = Arc::new(Gate::default());
    let _subscription = center.subscribe(Arc::new({
        let gate = gate.clone();
        move |snapshot| {
            if !snapshot.messages.is_empty() {
                gate.block();
            }
        }
    }));
    let publish = std::thread::spawn({
        let center = center.clone();
        move || center.publish(MessageDraft::new("Admitted", "event").desktop(true))
    });
    gate.wait_entered();
    let shutdown = std::thread::spawn({
        let center = center.clone();
        move || center.shutdown_checked()
    });
    let until = Instant::now() + WAIT;
    while !center.inner.admission.lock().unwrap().closing {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(center.publish(MessageDraft::new("Late", "event")).is_err());
    assert!(center.set_read("missing", true).is_err());
    assert!(center.mark_all_read().is_err());
    assert!(center.remove("missing").is_err());
    assert!(center.clear().is_err());
    assert!(center.hydrate_from_storage().is_err());
    assert!(center.retry_failed().is_err());
    gate.release();
    let message = publish.join().unwrap().unwrap();
    shutdown.join().unwrap().unwrap();
    assert_eq!(
        crate::storage::load(&destination).unwrap()[0].id,
        message.id
    );
    assert_eq!(notifier.0.load(Ordering::Relaxed), 1);
    center.shutdown_checked().unwrap();
    std::fs::remove_file(destination).unwrap();
}

#[derive(Default)]
struct CountNotifier(AtomicUsize);
impl DesktopNotifier for CountNotifier {
    fn show(&self, _: &Message) -> Result<(), String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

struct ErrorNotifier {
    panic: bool,
}
impl DesktopNotifier for ErrorNotifier {
    fn show(&self, _: &Message) -> Result<(), String> {
        assert!(!self.panic, "injected native panic");
        Err("native service unavailable".into())
    }
}

#[test]
fn ordinary_native_error_degrades_delivery_but_keeps_durable_flush_successful() {
    let destination = path("native-error");
    let center =
        MessageCenter::open_with(&destination, Arc::new(ErrorNotifier { panic: false })).unwrap();
    center
        .publish(MessageDraft::new("Inbox", "native failure").desktop(true))
        .unwrap();
    center.flush().unwrap();
    center.flush_native().unwrap();
    assert_eq!(center.inner.native.stats().failed, 1);
    center.shutdown_checked().unwrap();
    assert_eq!(crate::storage::load(&destination).unwrap().len(), 1);
    std::fs::remove_file(destination).unwrap();
}

#[test]
fn native_panic_is_observable_by_native_barrier_and_checked_shutdown() {
    let destination = path("native-panic");
    let center =
        MessageCenter::open_with(&destination, Arc::new(ErrorNotifier { panic: true })).unwrap();
    center
        .publish(MessageDraft::new("Inbox", "panic").desktop(true))
        .unwrap();
    center.flush().unwrap();
    assert!(matches!(
        center.flush_native(),
        Err(MessageCenterError::NativeWorker(_))
    ));
    assert!(matches!(
        center.shutdown_checked(),
        Err(MessageCenterError::NativeWorker(_))
    ));
    assert!(center.shutdown_checked().is_err());
    assert_eq!(crate::storage::load(&destination).unwrap().len(), 1);
    std::fs::remove_file(destination).unwrap();
}

#[test]
fn checked_shutdown_retains_both_durable_and_native_failure_causes() {
    let center = MessageCenter::open_initialized_with_persist(
        path("dual-failure"),
        Arc::new(ErrorNotifier { panic: true }),
        Vec::new(),
        true,
        |_| Err("injected durable failure".into()),
    )
    .unwrap();
    center
        .publish(MessageDraft::new("Inbox", "dual failure").desktop(true))
        .unwrap();
    match center.shutdown_checked().unwrap_err() {
        MessageCenterError::Shutdown {
            persistence,
            native,
        } => {
            assert_eq!(persistence, "injected durable failure");
            assert!(native.contains("panicked"));
        }
        other => panic!("both causes were not retained: {other}"),
    }
    assert!(matches!(
        center.shutdown_checked(),
        Err(MessageCenterError::Shutdown { .. })
    ));
}
