use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

use super::persistence_tests::{Gate, path, seed};
use super::{MessageCenter, MessageCenterError};
use crate::{MessageDraft, NoopDesktopNotifier};

const WAIT: Duration = Duration::from_secs(5);

fn deferred(destination: &std::path::Path, writes: Arc<AtomicUsize>) -> MessageCenter {
    let path = destination.to_owned();
    MessageCenter::open_initialized_with_persist(
        path.clone(),
        Arc::new(NoopDesktopNotifier),
        Vec::new(),
        false,
        move |snapshot| {
            writes.fetch_add(1, Ordering::SeqCst);
            crate::storage::save(&path, &snapshot.messages).map_err(|error| error.to_string())
        },
    )
    .unwrap()
}

#[test]
fn disk_only_hydration_never_offers_a_write_and_business_changes_still_persist() {
    let destination = path("hydration-disk-only");
    let messages = seed(2);
    // Compact bytes make a redundant pretty-JSON replacement observable, even
    // when its parsed content is equal to the loaded history.
    let original = serde_json::to_vec(&messages).unwrap();
    std::fs::write(&destination, &original).unwrap();
    let writes = Arc::new(AtomicUsize::new(0));
    let center = deferred(&destination, writes.clone());
    assert_eq!(center.hydrate_from_storage().unwrap(), 2);
    center.flush().unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(&destination).unwrap(), original);
    assert!(center.set_read(&messages[0].id, true).unwrap());
    center.flush().unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        crate::storage::load(&destination).unwrap(),
        center.snapshot().messages.to_vec()
    );
    center.shutdown_checked().unwrap();
    std::fs::remove_file(destination).unwrap();
}

#[test]
fn early_publication_read_remove_and_clear_only_write_the_final_merged_state() {
    for clear in [false, true] {
        let destination = path("hydration-early-actions");
        let stored = seed(2);
        crate::storage::save(&destination, &stored).unwrap();
        let writes = Arc::new(AtomicUsize::new(0));
        let center = deferred(&destination, writes.clone());
        let early = center
            .publish(MessageDraft::new("Early", "Fixture").desktop(false))
            .unwrap();
        assert!(center.set_read(&early.id, true).unwrap());
        let removed = center
            .publish(MessageDraft::new("Removed", "Fixture").desktop(false))
            .unwrap();
        assert!(center.remove(&removed.id).unwrap());
        if clear {
            assert_eq!(center.clear().unwrap(), 1);
        }
        assert_eq!(writes.load(Ordering::SeqCst), 0);
        assert_eq!(center.hydrate_from_storage().unwrap(), 2);
        center.flush().unwrap();
        let snapshot = center.snapshot();
        assert_eq!(snapshot.messages.len(), if clear { 2 } else { 3 });
        assert!(
            snapshot
                .messages
                .iter()
                .all(|message| message.id != removed.id)
        );
        if !clear {
            assert!(
                snapshot
                    .messages
                    .iter()
                    .find(|message| message.id == early.id)
                    .unwrap()
                    .read
            );
        }
        assert_eq!(writes.load(Ordering::SeqCst), usize::from(!clear));
        assert_eq!(
            super::load_messages(&destination).unwrap(),
            snapshot.messages.to_vec()
        );
        center.shutdown_checked().unwrap();
        std::fs::remove_file(destination).unwrap();
    }
}

#[test]
fn hydration_subscriber_read_delete_and_clear_changes_are_not_missed_before_observer_install() {
    for clear in [false, true] {
        let destination = path("hydration-subscriber-actions");
        let stored = seed(2);
        crate::storage::save(&destination, &stored).unwrap();
        let writes = Arc::new(AtomicUsize::new(0));
        let center = deferred(&destination, writes.clone());
        let once = AtomicBool::new(false);
        let _subscription = center.subscribe(Arc::new({
            let center = center.clone();
            let stored = stored.clone();
            move |snapshot| {
                if snapshot.messages.is_empty() || once.swap(true, Ordering::SeqCst) {
                    return;
                }
                assert!(center.set_read(&stored[0].id, true).unwrap());
                assert!(center.remove(&stored[1].id).unwrap());
                if clear {
                    assert_eq!(center.clear().unwrap(), 1);
                }
            }
        }));
        assert_eq!(center.hydrate_from_storage().unwrap(), 2);
        center.flush().unwrap();
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        let snapshot = center.snapshot();
        assert_eq!(snapshot.messages.len(), usize::from(!clear));
        assert_eq!(snapshot.unread_count, 0);
        assert_eq!(
            crate::storage::load(&destination).unwrap(),
            snapshot.messages.to_vec()
        );
        center.shutdown_checked().unwrap();
        std::fs::remove_file(destination).unwrap();
    }
}

#[test]
fn concurrent_actions_during_hydration_persist_after_the_hydration_subscriber_returns() {
    let destination = path("hydration-concurrent-actions");
    let stored = seed(2);
    crate::storage::save(&destination, &stored).unwrap();
    let writes = Arc::new(AtomicUsize::new(0));
    let center = deferred(&destination, writes.clone());
    let gate = Arc::new(Gate::default());
    let once = AtomicBool::new(false);
    let _subscription = center.subscribe(Arc::new({
        let gate = gate.clone();
        move |snapshot| {
            if !snapshot.messages.is_empty() && !once.swap(true, Ordering::SeqCst) {
                gate.block();
            }
        }
    }));
    let hydration = std::thread::spawn({
        let center = center.clone();
        move || center.hydrate_from_storage()
    });
    gate.wait_entered();
    assert!(center.set_read(&stored[0].id, true).unwrap());
    assert!(center.remove(&stored[1].id).unwrap());
    let new = center
        .publish(MessageDraft::new("Concurrent", "Fixture").desktop(false))
        .unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    gate.release();
    assert_eq!(hydration.join().unwrap().unwrap(), 2);
    center.flush().unwrap();
    let snapshot = center.snapshot();
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert_eq!(snapshot.messages.len(), 2);
    assert!(snapshot.messages.iter().any(|message| message.id == new.id));
    assert_eq!(snapshot.unread_count, 1);
    assert_eq!(
        crate::storage::load(&destination).unwrap(),
        snapshot.messages.to_vec()
    );
    center.shutdown_checked().unwrap();
    std::fs::remove_file(destination).unwrap();
}

#[test]
fn repeated_hydration_cannot_resurrect_a_deletion_while_its_durable_write_is_pending() {
    let destination = path("hydration-repeat");
    let stored = seed(1);
    crate::storage::save(&destination, &stored).unwrap();
    let gate = Arc::new(Gate::default());
    let writes = Arc::new(AtomicUsize::new(0));
    let center = MessageCenter::open_initialized_with_persist(
        destination.clone(),
        Arc::new(NoopDesktopNotifier),
        Vec::new(),
        false,
        {
            let gate = gate.clone();
            let writes = writes.clone();
            let destination = destination.clone();
            move |snapshot| {
                writes.fetch_add(1, Ordering::SeqCst);
                gate.block();
                crate::storage::save(&destination, &snapshot.messages)
                    .map_err(|error| error.to_string())
            }
        },
    )
    .unwrap();
    assert_eq!(center.hydrate_from_storage().unwrap(), 1);
    assert!(center.remove(&stored[0].id).unwrap());
    gate.wait_entered();
    let revision = center.inner.store.snapshot().revision();
    assert_eq!(center.hydrate_from_storage().unwrap(), 1);
    assert_eq!(center.inner.store.snapshot().revision(), revision);
    assert!(center.snapshot().messages.is_empty());
    gate.release();
    center.shutdown_checked().unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert!(crate::storage::load(&destination).unwrap().is_empty());
    std::fs::remove_file(destination).unwrap();
}

#[test]
fn corrupt_history_is_preserved_until_a_valid_business_change_and_io_failure_remains_retryable() {
    let destination = path("hydration-corrupt");
    let corrupt = b"{broken message history";
    std::fs::write(&destination, corrupt).unwrap();
    let writes = Arc::new(AtomicUsize::new(0));
    let center = deferred(&destination, writes.clone());
    assert_eq!(center.hydrate_from_storage().unwrap(), 0);
    center.flush().unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(&destination).unwrap(), corrupt);
    center
        .publish(MessageDraft::new("Recovery", "Valid changed state").desktop(false))
        .unwrap();
    center.shutdown_checked().unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        crate::storage::load(&destination).unwrap(),
        center.snapshot().messages.to_vec()
    );
    std::fs::remove_file(destination).unwrap();

    let destination = path("hydration-retry");
    std::fs::create_dir(&destination).unwrap();
    let writes = Arc::new(AtomicUsize::new(0));
    let center = deferred(&destination, writes.clone());
    center
        .publish(MessageDraft::new("Accepted", "Retained across load error").desktop(false))
        .unwrap();
    assert!(matches!(
        center.hydrate_from_storage(),
        Err(MessageCenterError::Storage(_))
    ));
    assert!(center.flush().is_err());
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    std::fs::remove_dir(&destination).unwrap();
    assert_eq!(center.hydrate_from_storage().unwrap(), 0);
    center.shutdown_checked().unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        crate::storage::load(&destination).unwrap()[0].title,
        "Accepted"
    );
    std::fs::remove_file(destination).unwrap();
}

#[test]
fn concurrent_hydration_shares_one_result_and_checked_shutdown_waits_without_reentry_deadlock() {
    let destination = path("hydration-shutdown");
    crate::storage::save(&destination, &seed(1)).unwrap();
    let writes = Arc::new(AtomicUsize::new(0));
    let center = deferred(&destination, writes.clone());
    let gate = Arc::new(Gate::default());
    let once = AtomicBool::new(false);
    let _subscription = center.subscribe(Arc::new({
        let center = center.clone();
        let gate = gate.clone();
        move |snapshot| {
            if !snapshot.messages.is_empty() && !once.swap(true, Ordering::SeqCst) {
                assert!(matches!(
                    center.hydrate_from_storage(),
                    Err(MessageCenterError::ReentrantHydration)
                ));
                gate.block();
            }
        }
    }));
    let first = std::thread::spawn({
        let center = center.clone();
        move || center.hydrate_from_storage()
    });
    gate.wait_entered();
    let second = std::thread::spawn({
        let center = center.clone();
        move || center.hydrate_from_storage()
    });
    let deadline = Instant::now() + WAIT;
    while center.inner.admission.lock().unwrap().active_actions != 2 {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    let (done, receipt) = mpsc::channel();
    let shutdown = std::thread::spawn({
        let center = center.clone();
        move || done.send(center.shutdown_checked()).unwrap()
    });
    assert!(receipt.recv_timeout(Duration::from_millis(30)).is_err());
    gate.release();
    assert_eq!(first.join().unwrap().unwrap(), 1);
    assert_eq!(second.join().unwrap().unwrap(), 1);
    receipt.recv_timeout(WAIT).unwrap().unwrap();
    shutdown.join().unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert!(center.hydrate_from_storage().is_err());
    std::fs::remove_file(destination).unwrap();
}
