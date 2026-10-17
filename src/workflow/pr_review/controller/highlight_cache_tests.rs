use super::*;
use crate::workflow::pr_review::controller::highlight_session::HighlightSession;
use std::sync::{Weak, mpsc};
use std::time::Duration;

fn warm(cache: &HighlightResourceCache<Vec<u8>>, value: u8) -> Weak<Vec<u8>> {
    Arc::downgrade(cache.lease().get_or_init(|| Arc::new(vec![value; 65536])))
}

#[test]
fn graph_is_shared_but_retired_session_slots_and_inputs_are_not_retained() {
    let cache = HighlightResourceCache::default();
    let session = HighlightSession::default();
    let mut slots = Vec::new();
    let mut grammar = None;
    for _ in 0..48 {
        let cached = cache.lease();
        let slot = session.lease();
        let resource =
            slot.get_or_init(|| cached.get_or_init(|| Arc::new(vec![0_u8; 65536])).clone());
        let weak = Arc::downgrade(resource);
        if let Some(previous) = &grammar {
            assert!(Weak::ptr_eq(previous, &weak));
        }
        grammar = Some(weak);
        slots.push(Arc::downgrade(&slot));
        session.detach();
        drop(slot);
        assert!(!session.is_loaded());
        assert!(slots.iter().all(|slot| slot.upgrade().is_none()));
    }
    assert!(cache.request_idle_check(None));
    let retired = cache.confirm_idle().unwrap();
    assert!(!cache.is_loaded());
    drop(retired);
    assert!(cache.next_release().is_none());
    assert!(grammar.unwrap().upgrade().is_none());
}

#[test]
fn repeated_empty_resets_queue_one_check_without_loading_resources() {
    let cache = HighlightResourceCache::<Vec<u8>>::default();
    assert!(cache.request_idle_check(None));
    for _ in 0..100 {
        assert!(!cache.request_idle_check(None));
    }
    assert!(cache.confirm_idle().is_none());
    assert!(!cache.is_loaded());
    assert!(!cache.state.lock().unwrap().releasing);
}

#[test]
fn coalesced_check_preserves_a_reclaimed_graph_and_retires_latest_idle_intent() {
    let cache = HighlightResourceCache::default();
    let grammar = warm(&cache, 1);
    assert!(cache.request_idle_check(None));
    for _ in 0..100 {
        assert!(!cache.request_idle_check(None));
    }
    cache.activate(); // Real asynchronous open declares interest without loading.
    assert!(cache.confirm_idle().is_none());
    assert!(grammar.upgrade().is_some());
    assert!(cache.request_idle_check(None));
    cache.activate();
    assert!(!cache.request_idle_check(None)); // Reset/claim/reset shares the first check.
    drop(cache.confirm_idle().unwrap());
    assert!(cache.next_release().is_none());
    assert!(grammar.upgrade().is_none());
    assert!(cache.request_idle_check(None));
    assert!(cache.confirm_idle().is_none()); // An idle empty cache never loads.
}

#[test]
fn async_read_error_releases_only_its_still_unclaimed_intent() {
    let cache = HighlightResourceCache::<Vec<u8>>::default();
    let failed_read = cache.activate();
    assert!(!cache.is_loaded());
    cache.lease();
    assert!(!cache.request_idle_check(Some(failed_read)));
    let newer_read = cache.activate();
    assert!(!cache.request_idle_check(Some(failed_read)));
    assert!(cache.request_idle_check(Some(newer_read)));
    drop(cache.confirm_idle().unwrap());
    assert!(cache.next_release().is_none());
    cache.state.lock().unwrap().intent = u64::MAX;
    let saturated = cache.activate();
    assert!(!cache.request_idle_check(Some(saturated)));
    assert!(cache.request_idle_check(None)); // Explicit close still works.
}

#[test]
fn admitted_old_factory_cannot_fill_replacement_cache_or_session_slots() {
    let cache = HighlightResourceCache::default();
    let session = HighlightSession::default();
    let old_cache = cache.lease();
    let old_session = session.lease();
    session.detach();
    assert!(cache.request_idle_check(None));
    let retired = cache.confirm_idle().unwrap(); // Factory has not started yet.
    old_session.get_or_init(|| old_cache.get_or_init(|| Arc::new(vec![1; 1024])).clone());
    assert!(!cache.is_loaded());
    assert!(!session.is_loaded());
    let current_cache = cache.lease();
    let current_session = session.lease();
    assert!(!Arc::ptr_eq(&old_cache, &current_cache));
    assert!(!Arc::ptr_eq(&old_session, &current_session));
    let new = current_session.get_or_init(|| {
        current_cache
            .get_or_init(|| Arc::new(vec![2; 1024]))
            .clone()
    });
    assert_eq!(new[0], 2);
    assert_eq!(old_session.get().unwrap()[0], 1);
    drop(retired);
    assert!(cache.next_release().is_none());
}

#[test]
fn one_release_worker_drains_only_confirmed_idle_slots() {
    let cache = HighlightResourceCache::default();
    let first = warm(&cache, 1);
    assert!(cache.request_idle_check(None));
    let retired_first = cache.confirm_idle().unwrap();
    let second = warm(&cache, 2);
    assert!(cache.request_idle_check(None));
    assert!(cache.confirm_idle().is_none()); // Existing release worker owns the drain.
    drop(retired_first);
    assert!(first.upgrade().is_none());
    let retired_second = cache.next_release().unwrap();
    let third = warm(&cache, 3);
    assert!(cache.request_idle_check(None)); // Same-operation reclaim can still follow.
    drop(retired_second);
    assert!(second.upgrade().is_none());
    assert!(cache.next_release().is_none()); // Never race the queued UI confirmation.
    cache.activate();
    assert!(cache.confirm_idle().is_none());
    assert!(third.upgrade().is_some());
    assert!(!cache.state.lock().unwrap().releasing);
}

#[test]
fn failed_enqueue_is_retryable_and_queued_weak_does_not_retain_owner() {
    let cache = Arc::new(HighlightResourceCache::<Vec<u8>>::default());
    let weak = Arc::downgrade(&cache);
    assert!(cache.request_idle_check(None));
    cache.check_failed();
    assert!(cache.request_idle_check(None));
    drop(cache);
    assert!(weak.upgrade().is_none());
}

#[test]
fn ordinary_idle_release_destructs_on_worker_outside_cache_lock() {
    struct RecordDrop {
        owner: Weak<HighlightResourceCache<Self>>,
        ui: std::thread::ThreadId,
        done: mpsc::Sender<bool>,
    }
    impl Drop for RecordDrop {
        fn drop(&mut self) {
            let unlocked = self.owner.upgrade().unwrap().state.try_lock().is_ok();
            self.done
                .send(unlocked && std::thread::current().id() != self.ui)
                .unwrap();
        }
    }
    let cache = Arc::new(HighlightResourceCache::default());
    let (done, finished) = mpsc::channel();
    let worker = crate::app_runtime::latest_compute::LatestCompute::new();
    let owner = Arc::downgrade(&cache);
    let ui = std::thread::current().id();
    let (ready, output) = mpsc::channel();
    worker.submit(
        cache.lease(),
        move |slot, _| {
            slot.get_or_init(|| {
                Arc::new(RecordDrop {
                    owner: owner.clone(),
                    ui,
                    done: done.clone(),
                })
            });
            Some(slot)
        },
        move || {
            ready.send(()).unwrap();
        },
    );
    output.recv_timeout(Duration::from_secs(5)).unwrap();
    worker.cancel(); // Reset clears completed output on UI while cache still owns it.
    assert!(finished.try_recv().is_err());
    assert!(cache.request_idle_check(None));
    let slot = cache.confirm_idle().unwrap();
    let weak = Arc::downgrade(&cache);
    crate::app_runtime::release_resources(slot, move || {
        weak.upgrade().and_then(|c| c.next_release())
    });
    assert!(finished.recv_timeout(Duration::from_secs(5)).unwrap());
    assert!(!cache.is_loaded());
}

#[test]
fn blocked_release_worker_drains_new_confirmed_slot_without_parallel_spawn() {
    struct SlowDrop {
        entered: mpsc::Sender<std::thread::ThreadId>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl Drop for SlowDrop {
        fn drop(&mut self) {
            self.entered.send(std::thread::current().id()).unwrap();
            self.release
                .get_mut()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
    }
    let cache = Arc::new(HighlightResourceCache::default());
    let (entered, running) = mpsc::channel();
    let (release_first, first_gate) = mpsc::channel();
    cache.lease().get_or_init(|| {
        Arc::new(SlowDrop {
            entered: entered.clone(),
            release: Mutex::new(first_gate),
        })
    });
    assert!(cache.request_idle_check(None));
    let slot = cache.confirm_idle().unwrap();
    let weak = Arc::downgrade(&cache);
    let (done, finished) = mpsc::channel();
    crate::app_runtime::release_resources(slot, move || {
        let next = weak.upgrade().and_then(|cache| cache.next_release());
        if next.is_none() {
            done.send(()).unwrap();
        }
        next
    });
    let first_thread = running.recv_timeout(Duration::from_secs(5)).unwrap();
    let (release_second, second_gate) = mpsc::channel();
    cache.lease().get_or_init(|| {
        Arc::new(SlowDrop {
            entered,
            release: Mutex::new(second_gate),
        })
    });
    assert!(cache.request_idle_check(None));
    assert!(cache.confirm_idle().is_none()); // No second release job is admitted.
    release_first.send(()).unwrap();
    assert_eq!(
        running.recv_timeout(Duration::from_secs(5)).unwrap(),
        first_thread
    );
    release_second.send(()).unwrap();
    finished.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(!cache.state.lock().unwrap().releasing);
    assert!(!cache.is_loaded());
}
