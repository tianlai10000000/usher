//! Framework-neutral automatic state synchronization for cross-page state.
//!
//! Feature crates depend on [`StateStore`], not on [`LocalStateStore`]. A
//! future third-party Redux/Pinia-like engine can implement the same port
//! without leaking its concrete types into UI bridges or publishers.

use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak, mpsc};
use std::thread::JoinHandle;

/// Immutable, revisioned state delivered to selectors and subscribers.
pub struct StoreSnapshot<S> {
    revision: u64,
    state: Arc<S>,
}

impl<S> Clone for StoreSnapshot<S> {
    fn clone(&self) -> Self {
        Self {
            revision: self.revision,
            state: self.state.clone(),
        }
    }
}

impl<S> StoreSnapshot<S> {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn state(&self) -> &S {
        self.state.as_ref()
    }

    pub fn state_arc(&self) -> Arc<S> {
        self.state.clone()
    }

    pub fn select<T>(&self, selector: impl FnOnce(&S) -> T) -> T {
        selector(self.state())
    }
}

/// Result of reducing one action. Unchanged reductions do not increment the
/// revision, notify subscribers, or trigger persistence observers.
pub struct Reduction<S, O> {
    pub state: S,
    pub output: O,
    pub changed: bool,
}

impl<S, O> Reduction<S, O> {
    pub fn changed(state: S, output: O) -> Self {
        Self {
            state,
            output,
            changed: true,
        }
    }

    pub fn unchanged(state: S, output: O) -> Self {
        Self {
            state,
            output,
            changed: false,
        }
    }
}

/// Domain reducer boundary. Reducers are synchronous and side-effect free;
/// filesystem/network/OS work belongs in automatic commit subscribers.
pub trait Reducer<S, A, O>: Send + Sync + 'static {
    fn reduce(&self, current: &S, action: A) -> Reduction<S, O>;
}

impl<S, A, O, F> Reducer<S, A, O> for F
where
    F: Fn(&S, A) -> Reduction<S, O> + Send + Sync + 'static,
{
    fn reduce(&self, current: &S, action: A) -> Reduction<S, O> {
        self(current, action)
    }
}

pub struct DispatchResult<S, O> {
    pub snapshot: StoreSnapshot<S>,
    pub output: O,
    pub changed: bool,
}

/// Stable store port used by feature packages. Concrete state-management
/// libraries must be hidden behind an implementation of this trait.
pub trait StateStore<S, A, O>: Send + Sync
where
    S: Send + Sync + 'static,
    A: Send + 'static,
    O: Send + 'static,
{
    fn snapshot(&self) -> StoreSnapshot<S>;
    fn dispatch(&self, action: A) -> DispatchResult<S, O>;
    fn subscribe(&self, listener: StoreListener<S>) -> Subscription;
    /// Registers a non-blocking observer that runs for every changed commit
    /// before `dispatch` returns. Use this for ordered persistence enqueueing,
    /// never for UI work or nested dispatch.
    fn observe_commits(&self, observer: StoreListener<S>) -> Subscription;
}

/// Selector conveniences that remain independent of the concrete Store engine.
pub trait StateStoreExt<S, A, O>: StateStore<S, A, O>
where
    S: Send + Sync + 'static,
    A: Send + 'static,
    O: Send + 'static,
{
    fn select<T>(&self, selector: impl FnOnce(&S) -> T) -> T {
        self.snapshot().select(selector)
    }

    fn subscribe_selector<T>(
        &self,
        selector: Arc<dyn Fn(&S) -> T + Send + Sync>,
        listener: Arc<dyn Fn(T) + Send + Sync>,
    ) -> Subscription
    where
        T: Clone + PartialEq + Send + Sync + 'static,
    {
        let selected = Arc::new(Mutex::new(None::<T>));
        self.subscribe(Arc::new(move |snapshot| {
            let next = selector(snapshot.state());
            let mut previous = selected
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if previous.as_ref() != Some(&next) {
                *previous = Some(next.clone());
                drop(previous);
                listener(next);
            }
        }))
    }
}

impl<S, A, O, TStore> StateStoreExt<S, A, O> for TStore
where
    S: Send + Sync + 'static,
    A: Send + 'static,
    O: Send + 'static,
    TStore: StateStore<S, A, O> + ?Sized,
{
}

pub type StoreListener<S> = Arc<dyn Fn(StoreSnapshot<S>) + Send + Sync>;

/// Removes one automatic synchronization subscription when dropped.
pub struct Subscription {
    cancel: Option<Box<dyn FnOnce() + Send + Sync>>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel();
        }
    }
}

struct ListenerEntry<S> {
    id: u64,
    delivery: Mutex<DeliveryState<S>>,
    callback: StoreListener<S>,
}

struct DeliveryState<S> {
    highest_revision: Option<u64>,
    delivering: bool,
    pending_latest: Option<StoreSnapshot<S>>,
}

impl<S> ListenerEntry<S> {
    fn deliver(&self, snapshot: StoreSnapshot<S>) {
        let mut delivery = self
            .delivery
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if delivery
            .highest_revision
            .is_some_and(|revision| revision >= snapshot.revision())
        {
            return;
        }
        delivery.highest_revision = Some(snapshot.revision());
        if delivery.delivering {
            // Collapse backpressure to the latest immutable State. The active
            // callback completes first, then receives this newer revision.
            delivery.pending_latest = Some(snapshot);
            return;
        }
        delivery.delivering = true;
        drop(delivery);

        let mut next = Some(snapshot);
        while let Some(current) = next {
            (self.callback)(current);
            let mut delivery = self
                .delivery
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            next = delivery.pending_latest.take();
            if next.is_none() {
                delivery.delivering = false;
            }
        }
    }
}

struct LocalStoreInner<S, A, O> {
    current: RwLock<StoreSnapshot<S>>,
    reducer: Arc<dyn Reducer<S, A, O>>,
    dispatch_gate: Mutex<()>,
    listeners: Mutex<Vec<Arc<ListenerEntry<S>>>>,
    commit_observers: Mutex<Vec<(u64, StoreListener<S>)>>,
    next_listener_id: AtomicU64,
    marker: PhantomData<fn(A) -> O>,
}

/// Built-in store engine. It is intentionally small and framework-neutral;
/// consumers must type against [`StateStore`] so this can be replaced later.
pub struct LocalStateStore<S, A, O> {
    inner: Arc<LocalStoreInner<S, A, O>>,
}

impl<S, A, O> Clone for LocalStateStore<S, A, O> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<S, A, O> LocalStateStore<S, A, O>
where
    S: Send + Sync + 'static,
    A: Send + 'static,
    O: Send + 'static,
{
    pub fn new(initial_state: S, reducer: impl Reducer<S, A, O>) -> Self {
        Self {
            inner: Arc::new(LocalStoreInner {
                current: RwLock::new(StoreSnapshot {
                    revision: 0,
                    state: Arc::new(initial_state),
                }),
                reducer: Arc::new(reducer),
                dispatch_gate: Mutex::new(()),
                listeners: Mutex::new(Vec::new()),
                commit_observers: Mutex::new(Vec::new()),
                next_listener_id: AtomicU64::new(1),
                marker: PhantomData,
            }),
        }
    }
}

impl<S, A, O> StateStore<S, A, O> for LocalStateStore<S, A, O>
where
    S: Send + Sync + 'static,
    A: Send + 'static,
    O: Send + 'static,
{
    fn snapshot(&self) -> StoreSnapshot<S> {
        self.inner
            .current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn dispatch(&self, action: A) -> DispatchResult<S, O> {
        // Serialize reductions and revision assignment. Listener delivery is
        // outside this gate so listeners may safely dispatch another action.
        let gate = self
            .inner
            .dispatch_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let current = self.snapshot();
        let reduction = self.inner.reducer.reduce(current.state(), action);
        let snapshot = if reduction.changed {
            let next = StoreSnapshot {
                revision: current.revision().saturating_add(1),
                state: Arc::new(reduction.state),
            };
            *self
                .inner
                .current
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = next.clone();
            next
        } else {
            current
        };
        let listeners = reduction.changed.then(|| {
            self.inner
                .listeners
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        });
        if reduction.changed {
            // Commit observers run under the serial dispatch gate so enqueue
            // order matches revision order and flush after dispatch is sound.
            let observers = self
                .inner
                .commit_observers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .map(|(_, observer)| observer.clone())
                .collect::<Vec<_>>();
            for observer in observers {
                observer(snapshot.clone());
            }
        }
        drop(gate);

        if let Some(listeners) = listeners {
            for listener in listeners {
                listener.deliver(snapshot.clone());
            }
        }

        DispatchResult {
            snapshot,
            output: reduction.output,
            changed: reduction.changed,
        }
    }

    fn subscribe(&self, listener: StoreListener<S>) -> Subscription {
        let id = self.inner.next_listener_id.fetch_add(1, Ordering::Relaxed);
        let entry = Arc::new(ListenerEntry {
            id,
            delivery: Mutex::new(DeliveryState {
                highest_revision: None,
                delivering: false,
                pending_latest: None,
            }),
            callback: listener,
        });
        self.inner
            .listeners
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(entry.clone());

        // Pinia/Vuex-style automatic hydration: every new subscriber receives
        // the current state immediately, even if no action is dispatched later.
        entry.deliver(self.snapshot());

        let weak: Weak<LocalStoreInner<S, A, O>> = Arc::downgrade(&self.inner);
        Subscription {
            cancel: Some(Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    inner
                        .listeners
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .retain(|listener| listener.id != id);
                }
            })),
        }
    }

    fn observe_commits(&self, observer: StoreListener<S>) -> Subscription {
        let id = self.inner.next_listener_id.fetch_add(1, Ordering::Relaxed);
        self.inner
            .commit_observers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((id, observer));
        let weak: Weak<LocalStoreInner<S, A, O>> = Arc::downgrade(&self.inner);
        Subscription {
            cancel: Some(Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    inner
                        .commit_observers
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .retain(|(observer_id, _)| *observer_id != id);
                }
            })),
        }
    }
}

type PersistCallback<S> = dyn Fn(&S) -> Result<(), String> + Send + Sync;
type PersistenceErrorHandler<S> = dyn Fn(Arc<S>, String) + Send + Sync;

enum PersistenceCommand<S> {
    Persist(Arc<S>),
    Flush(mpsc::Sender<()>),
    Shutdown(mpsc::Sender<()>),
}

struct PersistenceWorker<S> {
    sender: mpsc::Sender<PersistenceCommand<S>>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl<S> PersistenceWorker<S>
where
    S: Send + Sync + 'static,
{
    fn start(
        thread_name: impl Into<String>,
        persist: Arc<PersistCallback<S>>,
        on_error: Arc<PersistenceErrorHandler<S>>,
    ) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let handle = std::thread::Builder::new()
            .name(thread_name.into())
            .spawn(move || {
                let mut pending = None;
                loop {
                    let command = match pending.take() {
                        Some(command) => command,
                        None => match receiver.recv() {
                            Ok(command) => command,
                            Err(_) => break,
                        },
                    };
                    match command {
                        PersistenceCommand::Persist(mut latest) => {
                            while let Ok(command) = receiver.try_recv() {
                                match command {
                                    PersistenceCommand::Persist(newer) => latest = newer,
                                    control => {
                                        pending = Some(control);
                                        break;
                                    }
                                }
                            }
                            if let Err(error) = persist(&latest) {
                                on_error(latest, error);
                            }
                        }
                        PersistenceCommand::Flush(done) => {
                            let _ = done.send(());
                        }
                        PersistenceCommand::Shutdown(done) => {
                            let _ = done.send(());
                            break;
                        }
                    }
                }
            })?;
        Ok(Self {
            sender,
            handle: Mutex::new(Some(handle)),
        })
    }

    fn flush(&self) -> Result<(), String> {
        let (sender, receiver) = mpsc::channel();
        self.sender
            .send(PersistenceCommand::Flush(sender))
            .map_err(|_| "persistence worker is unavailable".to_string())?;
        receiver
            .recv()
            .map_err(|_| "persistence worker stopped before flushing".to_string())
    }

    fn shutdown(&self) {
        let handle = self.handle.lock().ok().and_then(|mut handle| handle.take());
        let Some(handle) = handle else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        let _ = self.sender.send(PersistenceCommand::Shutdown(sender));
        let _ = receiver.recv();
        let _ = handle.join();
    }
}

/// Stable store facade for durable state. Every changed commit is queued to
/// one owned worker; callers dispatch typed actions and never save manually.
pub struct PersistentStore<S, A, O>
where
    S: Send + Sync + 'static,
    A: Send + 'static,
    O: Send + 'static,
{
    store: Arc<dyn StateStore<S, A, O>>,
    worker: Arc<PersistenceWorker<S>>,
    commit_subscription: Arc<Mutex<Option<Subscription>>>,
}

impl<S, A, O> Clone for PersistentStore<S, A, O>
where
    S: Send + Sync + 'static,
    A: Send + 'static,
    O: Send + 'static,
{
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            worker: self.worker.clone(),
            commit_subscription: self.commit_subscription.clone(),
        }
    }
}

impl<S, A, O> PersistentStore<S, A, O>
where
    S: Send + Sync + 'static,
    A: Send + 'static,
    O: Send + 'static,
{
    pub fn new(
        initial_state: S,
        reducer: impl Reducer<S, A, O>,
        thread_name: impl Into<String>,
        persist: impl Fn(&S) -> Result<(), String> + Send + Sync + 'static,
        on_error: impl Fn(Arc<S>, String) + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let store = Self::new_deferred(initial_state, reducer, thread_name, persist, on_error)?;
        store.enable_persistence();
        Ok(store)
    }

    /// Creates a durable store before startup hydration is available. Call
    /// [`Self::enable_persistence`] immediately after dispatching hydration so
    /// the loaded revision is not redundantly written back to disk.
    pub fn new_deferred(
        initial_state: S,
        reducer: impl Reducer<S, A, O>,
        thread_name: impl Into<String>,
        persist: impl Fn(&S) -> Result<(), String> + Send + Sync + 'static,
        on_error: impl Fn(Arc<S>, String) + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let store: Arc<dyn StateStore<S, A, O>> =
            Arc::new(LocalStateStore::new(initial_state, reducer));
        let worker = Arc::new(PersistenceWorker::start(
            thread_name,
            Arc::new(persist),
            Arc::new(on_error),
        )?);
        Ok(Self {
            store,
            worker,
            commit_subscription: Arc::new(Mutex::new(None)),
        })
    }

    pub fn enable_persistence(&self) {
        let Ok(mut subscription) = self.commit_subscription.lock() else {
            return;
        };
        if subscription.is_some() {
            return;
        }
        let persistence_sender = self.worker.sender.clone();
        *subscription = Some(self.store.observe_commits(Arc::new(move |snapshot| {
            let _ = persistence_sender.send(PersistenceCommand::Persist(snapshot.state_arc()));
        })));
    }

    pub fn snapshot(&self) -> StoreSnapshot<S> {
        self.store.snapshot()
    }

    pub fn dispatch(&self, action: A) -> DispatchResult<S, O> {
        self.store.dispatch(action)
    }

    pub fn subscribe(&self, listener: StoreListener<S>) -> Subscription {
        self.store.subscribe(listener)
    }

    pub fn flush(&self) -> Result<(), String> {
        self.worker.flush()
    }

    pub fn shutdown(&self) {
        let _ = self
            .commit_subscription
            .lock()
            .ok()
            .and_then(|mut subscription| subscription.take());
        self.worker.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::{LocalStateStore, PersistentStore, Reduction, StateStore, StateStoreExt};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Counter(i32);

    #[test]
    fn subscribers_hydrate_immediately_and_follow_changed_revisions() {
        let store = LocalStateStore::new(Counter(2), |state: &Counter, delta: i32| {
            if delta == 0 {
                Reduction::unchanged(state.clone(), state.0)
            } else {
                let next = Counter(state.0 + delta);
                Reduction::changed(next.clone(), next.0)
            }
        });
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_listener = seen.clone();
        let _subscription = store.subscribe(Arc::new(move |snapshot| {
            seen_for_listener
                .lock()
                .unwrap()
                .push((snapshot.revision(), snapshot.state().0));
        }));

        assert_eq!(*seen.lock().unwrap(), vec![(0, 2)]);
        assert_eq!(store.dispatch(3).output, 5);
        assert_eq!(store.dispatch(0).output, 5);
        assert_eq!(*seen.lock().unwrap(), vec![(0, 2), (1, 5)]);
    }

    #[test]
    fn selector_reads_one_projection_without_exposing_engine_state() {
        let store = LocalStateStore::new(Counter(7), |_: &Counter, next: i32| {
            Reduction::changed(Counter(next), ())
        });
        assert_eq!(store.snapshot().select(|state| state.0), 7);
    }

    #[test]
    fn concurrent_dispatch_never_delivers_a_stale_final_revision() {
        let store = LocalStateStore::new(Counter(0), |state: &Counter, (): ()| {
            Reduction::changed(Counter(state.0 + 1), ())
        });
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_listener = seen.clone();
        let _subscription = store.subscribe(Arc::new(move |snapshot| {
            seen_for_listener.lock().unwrap().push(snapshot.revision());
        }));

        let handles = (0..8)
            .map(|_| {
                let store = store.clone();
                std::thread::spawn(move || {
                    store.dispatch(());
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }

        let seen = seen.lock().unwrap();
        assert!(seen.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(seen.last(), Some(&8));
        assert_eq!(store.snapshot().state().0, 8);
    }

    #[test]
    fn dropping_subscription_stops_automatic_updates() {
        let store = LocalStateStore::new(Counter(0), |_: &Counter, next: i32| {
            Reduction::changed(Counter(next), ())
        });
        let callbacks = Arc::new(Mutex::new(0));
        let callbacks_for_listener = callbacks.clone();
        let subscription = store.subscribe(Arc::new(move |_| {
            *callbacks_for_listener.lock().unwrap() += 1;
        }));
        assert_eq!(*callbacks.lock().unwrap(), 1);
        drop(subscription);
        store.dispatch(9);
        assert_eq!(*callbacks.lock().unwrap(), 1);
    }

    #[test]
    fn commit_observer_runs_only_for_changes_before_dispatch_returns() {
        let store = LocalStateStore::new(Counter(0), |state: &Counter, delta: i32| {
            if delta == 0 {
                Reduction::unchanged(state.clone(), ())
            } else {
                Reduction::changed(Counter(state.0 + delta), ())
            }
        });
        let persisted = Arc::new(Mutex::new(Vec::new()));
        let persisted_for_observer = persisted.clone();
        let _observer = store.observe_commits(Arc::new(move |snapshot| {
            persisted_for_observer
                .lock()
                .unwrap()
                .push((snapshot.revision(), snapshot.state().0));
        }));

        store.dispatch(0);
        assert!(persisted.lock().unwrap().is_empty());
        store.dispatch(4);
        assert_eq!(*persisted.lock().unwrap(), vec![(1, 4)]);
    }

    #[test]
    fn selector_subscription_only_emits_when_projection_changes() {
        let store = LocalStateStore::new(Counter(1), |_: &Counter, next: i32| {
            Reduction::changed(Counter(next), ())
        });
        let parity = Arc::new(Mutex::new(Vec::new()));
        let parity_for_listener = parity.clone();
        let _subscription = store.subscribe_selector(
            Arc::new(|state: &Counter| state.0 % 2),
            Arc::new(move |selected| parity_for_listener.lock().unwrap().push(selected)),
        );

        store.dispatch(3);
        store.dispatch(4);
        assert_eq!(*parity.lock().unwrap(), vec![1, 0]);
    }

    #[test]
    fn persistent_store_writes_changed_commits_and_flushes_before_shutdown() {
        let persisted = Arc::new(Mutex::new(Vec::new()));
        let persisted_for_worker = persisted.clone();
        let errors = Arc::new(Mutex::new(Vec::new()));
        let errors_for_worker = errors.clone();
        let store = PersistentStore::new(
            Counter(1),
            |current: &Counter, next: i32| {
                if current.0 == next {
                    Reduction::unchanged(current.clone(), false)
                } else {
                    Reduction::changed(Counter(next), true)
                }
            },
            "state-store-test",
            move |state| {
                persisted_for_worker.lock().unwrap().push(state.0);
                Ok(())
            },
            move |_, error| errors_for_worker.lock().unwrap().push(error),
        )
        .unwrap();

        assert!(!store.dispatch(1).output);
        assert!(store.dispatch(2).output);
        assert!(store.dispatch(3).output);
        store.flush().unwrap();
        store.shutdown();

        assert_eq!(persisted.lock().unwrap().last(), Some(&3));
        assert!(errors.lock().unwrap().is_empty());
    }

    #[test]
    fn deferred_persistence_skips_hydration_and_writes_later_changes() {
        let persisted = Arc::new(Mutex::new(Vec::new()));
        let persisted_for_worker = persisted.clone();
        let store = PersistentStore::new_deferred(
            Counter(0),
            |_: &Counter, next: i32| Reduction::changed(Counter(next), ()),
            "deferred-state-store-test",
            move |state| {
                persisted_for_worker.lock().unwrap().push(state.0);
                Ok(())
            },
            |_, error| panic!("unexpected persistence error: {error}"),
        )
        .unwrap();

        store.dispatch(7);
        store.flush().unwrap();
        assert!(persisted.lock().unwrap().is_empty());

        store.enable_persistence();
        store.dispatch(8);
        store.flush().unwrap();
        store.shutdown();
        assert_eq!(*persisted.lock().unwrap(), vec![8]);
    }
}
