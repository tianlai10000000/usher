//! Latest-state persistence; intermediate snapshots are intentionally replaceable.
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

type Persist<S> = dyn Fn(&S) -> Result<(), String> + Send + Sync;
type OnError<S> = dyn Fn(Arc<S>, String) + Send + Sync;
struct WriteAttempt<S> {
    id: u64,
    revision: u64,
    value: Arc<S>,
}
struct Pending<S> {
    value: Option<WriteAttempt<S>>,
    dirty: Option<(u64, Arc<S>)>,
    accepted: Option<u64>,
    attempted: Option<u64>,
    scheduled_attempt: u64,
    completed_attempt: u64,
    active_attempt: Option<u64>,
    successful: Option<u64>,
    error: Option<String>,
    deadline: Option<Instant>,
    urgent: Option<u64>,
    closing: bool,
    stopped: bool,
}
struct Mailbox<S> {
    state: Mutex<Pending<S>>,
    wake: Condvar,
}

/// One active callback and one replaceable pending snapshot. Revisions must
/// increase within this writer. Flush waits for its accepted revision (or a
/// newer successful snapshot) and its current write attempt. A failed latest
/// snapshot stays dirty until explicitly retried or superseded; callbacks must
/// not call flush/shutdown themselves.
/// This is for state, never for audit/event streams that require every entry.
pub struct LatestPersistence<S> {
    mailbox: Arc<Mailbox<S>>,
    handle: Mutex<Option<JoinHandle<()>>>,
    debounce: Duration,
}
impl<S: Send + Sync + 'static> LatestPersistence<S> {
    pub fn start(
        name: impl Into<String>,
        debounce: Duration,
        persist: impl Fn(&S) -> Result<(), String> + Send + Sync + 'static,
        on_error: impl Fn(Arc<S>, String) + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let mailbox = Arc::new(Mailbox {
            state: Mutex::new(Pending {
                value: None,
                dirty: None,
                accepted: None,
                attempted: None,
                scheduled_attempt: 0,
                completed_attempt: 0,
                active_attempt: None,
                successful: None,
                error: None,
                deadline: None,
                urgent: None,
                closing: false,
                stopped: false,
            }),
            wake: Condvar::new(),
        });
        let work = mailbox.clone();
        let persist: Arc<Persist<S>> = Arc::new(persist);
        let on_error: Arc<OnError<S>> = Arc::new(on_error);
        let handle = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                loop {
                    let attempt = {
                        let mut state = work.state.lock().unwrap_or_else(|p| p.into_inner());
                        loop {
                            if state.closing && state.value.is_none() {
                                state.stopped = true;
                                work.wake.notify_all();
                                return;
                            }
                            if state.value.is_some() {
                                let urgent = state.closing
                                    || state
                                        .urgent
                                        .is_some_and(|target| state.completed_attempt < target);
                                let delay = state
                                    .deadline
                                    .map(|deadline| {
                                        deadline.saturating_duration_since(Instant::now())
                                    })
                                    .unwrap_or_default();
                                if urgent || delay.is_zero() {
                                    let attempt = state.value.take().unwrap();
                                    state.active_attempt = Some(attempt.id);
                                    break attempt;
                                }
                                state = work
                                    .wake
                                    .wait_timeout(state, delay)
                                    .unwrap_or_else(|p| p.into_inner())
                                    .0;
                            } else {
                                state = work.wake.wait(state).unwrap_or_else(|p| p.into_inner());
                            }
                        }
                    };
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        persist(&attempt.value)
                    }))
                    .unwrap_or_else(|_| Err("persistence callback panicked".into()));
                    if let Err(error) = &result {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            on_error(attempt.value.clone(), error.clone())
                        }));
                    }
                    let mut state = work.state.lock().unwrap_or_else(|p| p.into_inner());
                    state.attempted = Some(attempt.revision);
                    state.completed_attempt = attempt.id;
                    state.active_attempt = None;
                    let mut saved = None;
                    if result.is_ok() {
                        state.successful = Some(attempt.revision);
                        if state
                            .dirty
                            .as_ref()
                            .is_some_and(|(revision, _)| *revision <= attempt.revision)
                        {
                            saved = state.dirty.take();
                        }
                    }
                    state.error = result.err();
                    if state.urgent.is_some_and(|target| target <= attempt.id) {
                        state.urgent = None;
                    }
                    drop(state);
                    drop(saved);
                    drop(attempt);
                    work.wake.notify_all();
                }
            })?;
        Ok(Self {
            mailbox,
            handle: Mutex::new(Some(handle)),
            debounce,
        })
    }

    /// Returns false for stale revisions and after shutdown starts.
    pub fn submit(&self, revision: u64, value: Arc<S>) -> bool {
        let mut state = self.mailbox.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.closing || state.accepted.is_some_and(|accepted| revision <= accepted) {
            return false;
        }
        let Some(attempt) = state.scheduled_attempt.checked_add(1) else {
            return false;
        };
        state.scheduled_attempt = attempt;
        let old_dirty = state.dirty.replace((revision, value.clone()));
        let old = state.value.replace(WriteAttempt {
            id: attempt,
            revision,
            value,
        });
        state.accepted = Some(revision);
        state.deadline = Some(Instant::now() + self.debounce);
        drop(state);
        drop(old);
        drop(old_dirty);
        self.mailbox.wake.notify_all();
        true
    }

    /// Explicitly schedule one retry of the latest failed state without changing
    /// its business revision. Concurrent callers share a pending/active attempt.
    /// Returns false when healthy or already writing; call flush to await it.
    /// Flush and shutdown never retry a completed failure automatically.
    pub fn retry_failed(&self) -> Result<bool, String> {
        let mut state = self.mailbox.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.closing {
            return Err("persistence is closed".into());
        }
        if state.value.is_some() || state.active_attempt.is_some() || state.error.is_none() {
            return Ok(false);
        }
        let Some((revision, value)) = state.dirty.as_ref() else {
            return Ok(false);
        };
        let revision = *revision;
        let value = value.clone();
        let attempt = state
            .scheduled_attempt
            .checked_add(1)
            .ok_or("persistence attempt counter exhausted")?;
        state.scheduled_attempt = attempt;
        state.value = Some(WriteAttempt {
            id: attempt,
            revision,
            value,
        });
        state.deadline = Some(Instant::now());
        drop(state);
        self.mailbox.wake.notify_all();
        Ok(true)
    }

    pub fn flush(&self) -> Result<(), String> {
        let mut state = self.mailbox.state.lock().unwrap_or_else(|p| p.into_inner());
        let Some(target) = state.accepted else {
            return Ok(());
        };
        let target_attempt = state.scheduled_attempt;
        state.urgent = Some(state.urgent.unwrap_or(0).max(target_attempt));
        self.mailbox.wake.notify_all();
        loop {
            if state.successful.is_some_and(|done| done >= target) {
                return Ok(());
            }
            if state.completed_attempt >= target_attempt
                && state.attempted.is_some_and(|done| done >= target)
            {
                return Err(state
                    .error
                    .clone()
                    .unwrap_or_else(|| "persistence failed".into()));
            }
            if state.stopped {
                return Err("persistence worker stopped before flush".into());
            }
            state = self
                .mailbox
                .wake
                .wait(state)
                .unwrap_or_else(|p| p.into_inner());
        }
    }

    /// Reject further submissions, drain the latest accepted state, then join.
    /// Concurrent/repeated callers observe the same completed barrier.
    pub fn shutdown(&self) -> Result<(), String> {
        let mut handle = self.handle.lock().unwrap_or_else(|p| p.into_inner());
        {
            let mut state = self.mailbox.state.lock().unwrap_or_else(|p| p.into_inner());
            state.closing = true;
            self.mailbox.wake.notify_all();
        }
        if let Some(handle) = handle.take() {
            handle
                .join()
                .map_err(|_| "persistence worker panicked".to_string())?;
        }
        self.flush()
    }
}
impl<S> Drop for LatestPersistence<S> {
    fn drop(&mut self) {
        // Explicit shutdown joins; abandonment still releases the parked worker.
        let mut state = self.mailbox.state.lock().unwrap_or_else(|p| p.into_inner());
        state.closing = true;
        self.mailbox.wake.notify_all();
    }
}

/// One replaceable UI payload and one scheduled delivery. Consumers take the
/// latest value when their queued callback runs, rather than capturing each one.
pub struct LatestDelivery<T> {
    state: Mutex<Delivery<T>>,
}
struct Delivery<T> {
    value: Option<T>,
    revision: Option<u64>,
    queued: bool,
    closed: bool,
}
impl<T> Default for LatestDelivery<T> {
    fn default() -> Self {
        Self {
            state: Mutex::new(Delivery {
                value: None,
                revision: None,
                queued: false,
                closed: false,
            }),
        }
    }
}
impl<T> LatestDelivery<T> {
    /// True means the caller must schedule one callback. False means either a
    /// callback is already queued, or this revision was obsolete/closed.
    pub fn offer(&self, revision: u64, value: T) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.closed || state.revision.is_some_and(|seen| revision <= seen) {
            return false;
        }
        state.value = Some(value);
        state.revision = Some(revision);
        !std::mem::replace(&mut state.queued, true)
    }
    pub fn take(&self) -> Option<T> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.queued = false;
        state.value.take()
    }
    pub fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.closed = true;
        state.value = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc;
    #[test]
    fn failed_revision_zero_can_be_retried_flushed_and_shutdown() {
        let failing = Arc::new(AtomicBool::new(true));
        let failure = failing.clone();
        let attempts = Arc::new(AtomicUsize::new(0));
        let count = attempts.clone();
        let worker = LatestPersistence::start(
            "revision-zero-retry-test",
            Duration::ZERO,
            move |_: &u64| {
                count.fetch_add(1, Ordering::SeqCst);
                if failure.load(Ordering::SeqCst) {
                    Err("initial snapshot failed".into())
                } else {
                    Ok(())
                }
            },
            |_, _| {},
        )
        .unwrap();
        assert!(worker.submit(0, Arc::new(7)));
        assert_eq!(worker.flush().unwrap_err(), "initial snapshot failed");
        failing.store(false, Ordering::SeqCst);
        assert!(worker.retry_failed().unwrap());
        worker.flush().unwrap();
        assert_eq!(worker.mailbox.state.lock().unwrap().successful, Some(0));
        worker.shutdown().unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert!(worker.retry_failed().is_err());
    }

    #[test]
    fn failed_state_requires_explicit_retry_and_keeps_its_business_revision() {
        let fail = Arc::new(AtomicBool::new(true));
        let failing = fail.clone();
        let attempts = Arc::new(AtomicUsize::new(0));
        let count = attempts.clone();
        let worker = LatestPersistence::start(
            "same-state-retry-test",
            Duration::ZERO,
            move |_: &u64| {
                count.fetch_add(1, Ordering::SeqCst);
                if failing.load(Ordering::SeqCst) {
                    Err("disk failure".into())
                } else {
                    Ok(())
                }
            },
            |_, _| {},
        )
        .unwrap();
        let value = Arc::new(7);
        let retained = Arc::downgrade(&value);
        assert!(worker.submit(1, value));
        assert_eq!(worker.flush().unwrap_err(), "disk failure");
        for _ in 0..100 {
            assert_eq!(worker.flush().unwrap_err(), "disk failure");
        }
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert_eq!(retained.strong_count(), 1);
        assert!(!worker.submit(1, Arc::new(99)));
        assert!(worker.retry_failed().unwrap());
        assert_eq!(worker.flush().unwrap_err(), "disk failure");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        fail.store(false, Ordering::SeqCst);
        assert!(worker.retry_failed().unwrap());
        worker.flush().unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        let state = worker.mailbox.state.lock().unwrap();
        assert_eq!(state.accepted, Some(1));
        assert_eq!(state.successful, Some(1));
        assert!(state.dirty.is_none());
        drop(state);
        worker.shutdown().unwrap();
        assert_eq!(retained.strong_count(), 0);
        assert!(worker.retry_failed().is_err());
    }

    #[test]
    fn newer_pending_state_supersedes_failed_input_without_retrying_the_old_state() {
        let (started, ready) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let gate = Mutex::new(gate);
        let written = Arc::new(Mutex::new(Vec::new()));
        let record = written.clone();
        let worker = LatestPersistence::start(
            "superseded-retry-test",
            Duration::from_secs(60),
            move |value: &u64| {
                record.lock().unwrap().push(*value);
                if *value == 1 {
                    started.send(()).unwrap();
                    gate.lock().unwrap().recv().unwrap();
                    Err("old write failed".into())
                } else {
                    Ok(())
                }
            },
            |_, _| {},
        )
        .unwrap();
        let worker = Arc::new(worker);
        let old = Arc::new(1);
        let old_owner = Arc::downgrade(&old);
        worker.submit(1, old);
        let flush_worker = worker.clone();
        let flushing = std::thread::spawn(move || flush_worker.flush());
        ready.recv_timeout(Duration::from_secs(1)).unwrap();
        let mut replaced = Vec::new();
        for revision in 2..=1000 {
            let value = Arc::new(revision);
            replaced.push(Arc::downgrade(&value));
            worker.submit(revision, value);
        }
        assert!(!worker.retry_failed().unwrap());
        assert_eq!(
            replaced
                .iter()
                .filter(|value| value.strong_count() != 0)
                .count(),
            1
        );
        release.send(()).unwrap();
        // Whether the newer write completes before this waiter resumes or not,
        // the old flush may report its failure or the superseding success.
        let _ = flushing.join().unwrap();
        worker.shutdown().unwrap();
        assert_eq!(*written.lock().unwrap(), vec![1, 1000]);
        assert_eq!(old_owner.strong_count(), 0);
        assert!(replaced.iter().all(|value| value.strong_count() == 0));
    }

    #[test]
    fn concurrent_retry_flush_and_shutdown_share_one_attempt_and_wait_for_it() {
        let (started, ready) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let gate = Mutex::new(gate);
        let attempts = Arc::new(AtomicUsize::new(0));
        let count = attempts.clone();
        let worker = Arc::new(
            LatestPersistence::start(
                "concurrent-retry-test",
                Duration::ZERO,
                move |_: &u64| {
                    if count.fetch_add(1, Ordering::SeqCst) == 0 {
                        return Err("first write failed".into());
                    }
                    started.send(()).unwrap();
                    gate.lock().unwrap().recv().unwrap();
                    Ok(())
                },
                |_, _| {},
            )
            .unwrap(),
        );
        worker.submit(5, Arc::new(7));
        assert_eq!(worker.flush().unwrap_err(), "first write failed");
        assert!(worker.retry_failed().unwrap());
        ready.recv_timeout(Duration::from_secs(1)).unwrap();
        let mut callers = Vec::new();
        let (done, results) = mpsc::channel();
        let (entered, entering) = mpsc::channel();
        for _ in 0..8 {
            let caller = worker.clone();
            let done = done.clone();
            let entered = entered.clone();
            callers.push(std::thread::spawn(move || {
                assert!(!caller.retry_failed().unwrap());
                entered.send(()).unwrap();
                done.send(caller.flush()).unwrap();
            }));
        }
        for _ in 0..8 {
            entering.recv_timeout(Duration::from_secs(1)).unwrap();
        }
        // A retry changes its attempt barrier, so old attempted=5 must not let
        // any concurrent flush return the previous failure immediately.
        assert!(results.recv_timeout(Duration::from_millis(30)).is_err());
        let stopping = worker.clone();
        let stop_done = done.clone();
        let stop = std::thread::spawn(move || stop_done.send(stopping.shutdown()).unwrap());
        let deadline = Instant::now() + Duration::from_secs(1);
        while !worker.mailbox.state.lock().unwrap().closing {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(worker.retry_failed().is_err());
        assert!(!worker.submit(6, Arc::new(8)));
        assert!(results.try_recv().is_err());
        release.send(()).unwrap();
        for _ in 0..9 {
            results
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap();
        }
        for caller in callers {
            caller.join().unwrap();
        }
        stop.join().unwrap();
        worker.shutdown().unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn shutdown_reports_a_failed_dirty_state_without_automatically_retrying_it() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let count = attempts.clone();
        let worker = LatestPersistence::start(
            "failed-shutdown-test",
            Duration::ZERO,
            move |_: &u64| {
                count.fetch_add(1, Ordering::SeqCst);
                Err("disk failure".into())
            },
            |_, _| {},
        )
        .unwrap();
        worker.submit(1, Arc::new(1));
        assert_eq!(worker.flush().unwrap_err(), "disk failure");
        assert_eq!(worker.shutdown().unwrap_err(), "disk failure");
        assert_eq!(worker.shutdown().unwrap_err(), "disk failure");
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(worker.retry_failed().is_err());
    }

    #[test]
    fn slow_write_keeps_one_pending_and_flush_waits_for_latest_revision() {
        let (started, ready) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let gate = Mutex::new(gate);
        let written = Arc::new(Mutex::new(Vec::new()));
        let record = written.clone();
        let worker = Arc::new(
            LatestPersistence::start(
                "latest-test",
                Duration::ZERO,
                move |value: &(u64, Vec<u8>)| {
                    if value.0 == 1 {
                        started.send(()).unwrap();
                        gate.lock().unwrap().recv().unwrap();
                    }
                    record.lock().unwrap().push(value.0);
                    Ok(())
                },
                |_, _| {},
            )
            .unwrap(),
        );
        worker.submit(1, Arc::new((1, vec![1; 8192])));
        ready.recv_timeout(Duration::from_secs(1)).unwrap();
        let mut weak = Vec::new();
        for revision in 2..=1000 {
            let value = Arc::new((revision, vec![1; 8192]));
            weak.push(Arc::downgrade(&value));
            worker.submit(revision, value);
        }
        assert_eq!(
            weak.iter()
                .filter(|value| value.strong_count() != 0)
                .count(),
            1
        );
        assert!(!worker.submit(999, Arc::new((999, vec![]))));
        let flush = worker.clone();
        let (done, wait) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            done.send(flush.flush()).unwrap();
        });
        assert!(wait.recv_timeout(Duration::from_millis(30)).is_err());
        release.send(()).unwrap();
        wait.recv_timeout(Duration::from_secs(1)).unwrap().unwrap();
        handle.join().unwrap();
        worker.shutdown().unwrap();
        worker.shutdown().unwrap();
        assert_eq!(*written.lock().unwrap(), vec![1, 1000]);
        assert!(!worker.submit(1001, Arc::new((1001, vec![]))));
    }
    #[test]
    fn shutdown_during_write_drains_pending_and_rejects_later_submissions() {
        let (began, ready) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let gate = Mutex::new(gate);
        let written = Arc::new(Mutex::new(Vec::new()));
        let record = written.clone();
        let worker = Arc::new(
            LatestPersistence::start(
                "shutdown-write-test",
                Duration::ZERO,
                move |value: &u64| {
                    if *value == 1 {
                        began.send(()).unwrap();
                        gate.lock().unwrap().recv().unwrap();
                    }
                    record.lock().unwrap().push(*value);
                    Ok(())
                },
                |_, _| {},
            )
            .unwrap(),
        );
        worker.submit(1, Arc::new(1));
        ready.recv_timeout(Duration::from_secs(1)).unwrap();
        worker.submit(2, Arc::new(2));
        let stopping = worker.clone();
        let (done, wait) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            done.send(stopping.shutdown()).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(1);
        while !worker.mailbox.state.lock().unwrap().closing {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(!worker.submit(3, Arc::new(3)));
        assert!(wait.try_recv().is_err());
        release.send(()).unwrap();
        wait.recv_timeout(Duration::from_secs(1)).unwrap().unwrap();
        handle.join().unwrap();
        assert_eq!(*written.lock().unwrap(), vec![1, 2]);
        worker.shutdown().unwrap();
    }
    #[test]
    fn debounce_flush_and_error_recovery_have_explicit_barriers() {
        let written = Arc::new(Mutex::new(Vec::new()));
        let record = written.clone();
        let worker = LatestPersistence::start(
            "debounce-test",
            Duration::from_secs(60),
            move |value: &u64| {
                record.lock().unwrap().push(*value);
                if *value == 2 {
                    Err("disk failure".into())
                } else {
                    Ok(())
                }
            },
            |_, _| {},
        )
        .unwrap();
        worker.submit(1, Arc::new(1));
        worker.submit(2, Arc::new(2));
        assert!(written.lock().unwrap().is_empty());
        assert_eq!(worker.flush().unwrap_err(), "disk failure");
        worker.submit(3, Arc::new(3));
        worker.shutdown().unwrap();
        assert_eq!(*written.lock().unwrap(), vec![2, 3]);
    }
    #[test]
    fn busy_ui_has_one_delivery_and_rejects_stale_or_closed_updates() {
        let delivery = LatestDelivery::default();
        assert!(delivery.offer(0, 0));
        for revision in 1..1000 {
            assert!(!delivery.offer(revision, revision));
        }
        assert_eq!(delivery.take(), Some(999));
        assert!(!delivery.offer(998, 998));
        assert!(delivery.offer(1000, 1000));
        delivery.close();
        assert_eq!(delivery.take(), None);
        assert!(!delivery.offer(1001, 1001));
    }
}
