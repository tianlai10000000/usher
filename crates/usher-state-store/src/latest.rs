//! Latest-state persistence; intermediate snapshots are intentionally replaceable.
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

type Persist<S> = dyn Fn(&S) -> Result<(), String> + Send + Sync;
type OnError<S> = dyn Fn(Arc<S>, String) + Send + Sync;
struct Pending<S> {
    value: Option<(u64, Arc<S>)>,
    accepted: Option<u64>,
    attempted: Option<u64>,
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
/// newer successful snapshot); callbacks must not call flush/shutdown themselves.
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
                accepted: None,
                attempted: None,
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
                    let (revision, value) = {
                        let mut state = work.state.lock().unwrap_or_else(|p| p.into_inner());
                        loop {
                            if state.closing && state.value.is_none() {
                                state.stopped = true;
                                work.wake.notify_all();
                                return;
                            }
                            if state.value.is_some() {
                                let urgent = state.closing
                                    || state.urgent.is_some_and(|target| {
                                        state.attempted.is_none_or(|done| done < target)
                                    });
                                let delay = state
                                    .deadline
                                    .map(|deadline| {
                                        deadline.saturating_duration_since(Instant::now())
                                    })
                                    .unwrap_or_default();
                                if urgent || delay.is_zero() {
                                    break state.value.take().unwrap();
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
                    let result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| persist(&value)))
                            .unwrap_or_else(|_| Err("persistence callback panicked".into()));
                    if let Err(error) = &result {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            on_error(value.clone(), error.clone())
                        }));
                    }
                    let mut state = work.state.lock().unwrap_or_else(|p| p.into_inner());
                    state.attempted = Some(revision);
                    if result.is_ok() {
                        state.successful = Some(revision);
                    }
                    state.error = result.err();
                    if state.urgent.is_some_and(|target| target <= revision) {
                        state.urgent = None;
                    }
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
        let old = state.value.replace((revision, value));
        state.accepted = Some(revision);
        state.deadline = Some(Instant::now() + self.debounce);
        drop(state);
        drop(old);
        self.mailbox.wake.notify_all();
        true
    }

    pub fn flush(&self) -> Result<(), String> {
        let mut state = self.mailbox.state.lock().unwrap_or_else(|p| p.into_inner());
        let Some(target) = state.accepted else {
            return Ok(());
        };
        state.urgent = Some(state.urgent.unwrap_or(0).max(target));
        self.mailbox.wake.notify_all();
        loop {
            if state.successful.is_some_and(|done| done >= target) {
                return Ok(());
            }
            if state.attempted.is_some_and(|done| done >= target) {
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
    use std::sync::mpsc;
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
