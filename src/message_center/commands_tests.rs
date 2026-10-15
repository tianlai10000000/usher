use super::*;
use std::sync::{
    Condvar,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use usher_messages::NoopDesktopNotifier;

impl MessageCommandWorker {
    pub(crate) fn barrier(&self) {
        let (sender, receiver) = mpsc::channel();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while self.submit(Command::Barrier(sender.clone()), 0).is_err() {
            assert!(
                std::time::Instant::now() < deadline,
                "message barrier admission timed out"
            );
            std::thread::yield_now();
        }
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
    }
}

fn center(path: &std::path::Path, deferred: bool) -> MessageCenter {
    if deferred {
        MessageCenter::open_deferred_with(path, Arc::new(NoopDesktopNotifier)).unwrap()
    } else {
        MessageCenter::open_with(path, Arc::new(NoopDesktopNotifier)).unwrap()
    }
}

#[test]
fn real_message_worker_reduces_in_order_off_caller_and_drains_durable_state() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.json");
    let center = center(&path, false);
    let caller = std::thread::current().id();
    let threads = Arc::new(Mutex::new(Vec::new()));
    let seen = threads.clone();
    let _subscription = center.subscribe(Arc::new(move |snapshot| {
        if !snapshot.messages.is_empty() {
            seen.lock().unwrap().push(std::thread::current().id());
        }
    }));
    let commands = MessageCommandWorker::start(center.clone(), false).unwrap();
    let (sender, receiver) = mpsc::channel();
    let draft = MessageDraft::new("First", "Body")
        .channel("fixture")
        .deduplication_key("same")
        .desktop(false);
    commands
        .publish(draft.clone(), {
            let sender = sender.clone();
            move |result| {
                sender.send(result.unwrap()).unwrap();
            }
        })
        .unwrap();
    commands.mark_all_read().unwrap();
    commands
        .publish(draft, move |result| {
            sender.send(result.unwrap()).unwrap();
        })
        .unwrap();
    let first = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    let duplicate = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(first.id, duplicate.id);
    assert!(duplicate.read);
    commands.remove(first.id).unwrap();
    commands
        .publish(
            MessageDraft::new("Final", "Retained").desktop(false),
            |_| {},
        )
        .unwrap();
    commands.mark_all_read().unwrap();
    commands.shutdown().unwrap();
    center.shutdown();
    commands.shutdown().unwrap();
    center.shutdown();
    assert!(commands.clear().is_err());
    assert!(
        threads
            .lock()
            .unwrap()
            .iter()
            .all(|thread| *thread != caller)
    );
    let restored = MessageCenter::open_with(&path, Arc::new(NoopDesktopNotifier)).unwrap();
    assert_eq!(restored.snapshot().messages.len(), 1);
    assert_eq!(restored.snapshot().messages[0].title, "Final");
    assert!(restored.snapshot().messages[0].read);
    restored.shutdown();
}

#[test]
fn blocked_real_reducer_does_not_block_admission_and_shutdown_preserves_accepted_commands() {
    let directory = tempfile::tempdir().unwrap();
    let center = center(&directory.path().join("messages.json"), false);
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let blocked = gate.clone();
    let (entered, seen) = mpsc::channel();
    let once = AtomicBool::new(false);
    let _subscription = center.subscribe(Arc::new(move |snapshot| {
        if !snapshot.messages.is_empty() && !once.swap(true, Ordering::AcqRel) {
            entered.send(()).unwrap();
            let (lock, condition) = &*blocked;
            let mut open = lock.lock().unwrap();
            while !*open {
                open = condition.wait(open).unwrap();
            }
        }
    }));
    let commands = MessageCommandWorker::start(center.clone(), false).unwrap();
    commands
        .publish(
            MessageDraft::new("Blocked", "Fixture").desktop(false),
            |_| {},
        )
        .unwrap();
    seen.recv_timeout(Duration::from_secs(5)).unwrap();
    // The active publication counts toward the cap; each admitted mutation is
    // retained even while the real Store observer is blocked.
    for _ in 1..MAX_COMMANDS {
        commands.mark_all_read().unwrap();
    }
    assert!(commands.clear().is_err());
    let stopping = commands.clone();
    let (done, receipt) = mpsc::channel();
    let shutdown = std::thread::spawn(move || {
        done.send(stopping.shutdown()).unwrap();
    });
    while commands.0.admission.lock().unwrap().sender.is_some() {
        std::thread::yield_now();
    }
    assert!(commands.clear().is_err());
    assert!(receipt.recv_timeout(Duration::from_millis(30)).is_err());
    let (lock, condition) = &*gate;
    *lock.lock().unwrap() = true;
    condition.notify_all();
    receipt
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    shutdown.join().unwrap();
    center.shutdown();
    assert_eq!(center.snapshot().unread_count, 0);
}

#[test]
fn byte_budget_rejects_oversized_drafts_without_dropping_accepted_rows() {
    let directory = tempfile::tempdir().unwrap();
    let center = center(&directory.path().join("messages.json"), false);
    let commands = MessageCommandWorker::start(center.clone(), false).unwrap();
    assert!(
        commands
            .publish(
                MessageDraft::new("Oversized", "x".repeat(MAX_COMMAND_BYTES)),
                |_| panic!("rejected draft executed")
            )
            .is_err()
    );
    commands
        .publish(
            MessageDraft::new("Accepted", "Fixture").desktop(false),
            |_| {},
        )
        .unwrap();
    commands.shutdown().unwrap();
    center.shutdown();
    assert_eq!(center.snapshot().messages.len(), 1);
    assert_eq!(center.snapshot().messages[0].title, "Accepted");
}

#[test]
fn deferred_shutdown_hydrates_before_durable_close_even_without_first_frame() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.json");
    let stored = center(&path, false);
    stored
        .publish(MessageDraft::new("Stored", "Fixture").desktop(false))
        .unwrap();
    stored.shutdown();
    let pending = center(&path, true);
    let commands = MessageCommandWorker::start(pending.clone(), true).unwrap();
    commands.mark_all_read().unwrap();
    commands
        .publish(
            MessageDraft::new("Before first frame", "Fixture").desktop(false),
            |_| {},
        )
        .unwrap();
    commands.shutdown().unwrap();
    pending.shutdown();
    let restored = center(&path, false);
    assert_eq!(restored.snapshot().messages.len(), 2);
    assert!(
        restored
            .snapshot()
            .messages
            .iter()
            .any(|message| message.title == "Stored")
    );
    assert!(
        restored
            .snapshot()
            .messages
            .iter()
            .find(|message| message.title == "Stored")
            .unwrap()
            .read
    );
    assert_eq!(restored.snapshot().unread_count, 1);
    restored.shutdown();
}

#[test]
fn deferred_first_frame_hydration_applies_buffered_clear_and_mark_all_before_new_publications() {
    for clear in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("messages.json");
        let stored = center(&path, false);
        let old = stored
            .publish(MessageDraft::new("Stored", "Fixture").desktop(false))
            .unwrap();
        stored.shutdown();
        let disk_before_control = std::fs::read(&path).unwrap();
        let pending = center(&path, true);
        let commands = MessageCommandWorker::start(pending.clone(), true).unwrap();
        if clear {
            commands.clear().unwrap();
        } else {
            commands.mark_all_read().unwrap();
        }
        let (published, receipt) = mpsc::channel();
        commands
            .publish(
                MessageDraft::new("After buffered action", "Fixture").desktop(false),
                move |result| published.send(result).unwrap(),
            )
            .unwrap();

        // Both accepted commands remain pending until the real first-frame
        // control arrives: no publication/reduction or disk hydration happens.
        assert!(matches!(
            receipt.recv_timeout(Duration::from_millis(40)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(pending.snapshot().messages.is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), disk_before_control);

        commands.hydrate(StartupTimeline::new()).unwrap();
        commands.barrier();
        let new = receipt
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        let snapshot = pending.snapshot();
        assert_eq!(snapshot.messages[0].id, new.id);
        assert_eq!(snapshot.messages[0].title, "After buffered action");
        assert!(!snapshot.messages[0].read);
        assert_eq!(snapshot.unread_count, 1);
        if clear {
            assert_eq!(snapshot.messages.len(), 1);
            assert!(snapshot.messages.iter().all(|message| message.id != old.id));
        } else {
            assert_eq!(snapshot.messages.len(), 2);
            assert!(
                snapshot
                    .messages
                    .iter()
                    .find(|message| message.id == old.id)
                    .unwrap()
                    .read
            );
        }
        commands.shutdown().unwrap();
        pending.shutdown();
        let restored = center(&path, false);
        assert_eq!(restored.snapshot(), snapshot);
        restored.shutdown();
    }
}

#[test]
fn compiled_page_callbacks_enqueue_while_real_store_is_blocked_and_preserve_locale_projection() {
    let _locale_guard = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let center = center(&directory.path().join("messages.json"), false);
    let message = center
        .publish(MessageDraft::new("Fixture", "Body").desktop(false))
        .unwrap();
    let commands = MessageCommandWorker::start(center.clone(), false).unwrap();
    let window = crate::MainWindow::new().unwrap();
    let page = usher_messages::GitHubMessagesPage::new().unwrap();
    super::super::bind_page_callbacks(&center, &commands, &page, &window);
    let caller = std::thread::current().id();
    let (changed, seen) = mpsc::channel();
    let (entered, blocked) = mpsc::channel();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let listener_gate = gate.clone();
    let once = AtomicBool::new(false);
    let _subscription = center.subscribe(Arc::new(move |snapshot| {
        if snapshot.messages.len() == 2 && !once.swap(true, Ordering::AcqRel) {
            entered.send(()).unwrap();
            let (lock, condition) = &*listener_gate;
            let mut open = lock.lock().unwrap();
            while !*open {
                open = condition.wait(open).unwrap();
            }
        }
        if snapshot.unread_count == 0 {
            changed.send(std::thread::current().id()).unwrap();
        }
    }));
    commands
        .publish(
            MessageDraft::new("Block observer", "Fixture").desktop(false),
            |_| {},
        )
        .unwrap();
    blocked.recv_timeout(Duration::from_secs(5)).unwrap();
    // All three production callbacks return while an accepted Store operation
    // is deliberately blocked; none runs a competing reducer on this UI thread.
    page.invoke_set_read(message.id.clone().into(), true);
    page.invoke_mark_all_read();
    page.invoke_delete_message(message.id.into());
    assert_eq!(commands.0.admission.lock().unwrap().entries, 4);
    let (lock, condition) = &*gate;
    *lock.lock().unwrap() = true;
    condition.notify_all();
    commands.barrier();
    assert!(seen.try_iter().all(|thread| thread != caller));
    for locale in ["en", "zh-CN"] {
        usher_i18n::activate_locale(locale).unwrap();
        super::super::sync_page_language(&page, locale);
        super::super::apply_page_snapshot(&page, center.snapshot());
        assert_eq!(page.get_unread_count(), 0);
        assert_eq!(
            page.get_status_description().as_str(),
            usher_i18n::translate_for(locale, "messages.caught_up")
        );
    }
    page.invoke_clear_messages();
    commands.shutdown().unwrap();
    center.shutdown();
    assert!(center.snapshot().messages.is_empty());
}
