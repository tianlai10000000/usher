//! A single lifetime driver with one replaceable pending round.
//! No UI/reducer work occurs in admission; dropped rounds cancel their actual
//! HTTP/backoff futures and release their RAII permits before a successor runs.
use super::ScopeEpoch;
use crate::project::ProjectRepositoryMetadata;
use futures_util::future::{BoxFuture, poll_fn};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;

pub(super) const MAX_READERS: usize = 4;
pub(super) struct Round {
    pub(super) epoch: ScopeEpoch,
    pub(super) metadata: ProjectRepositoryMetadata,
    pub(super) repository: String,
    pub(super) numbers: Vec<String>,
}
pub(super) struct Control {
    pub(super) epoch: ScopeEpoch,
    pub(super) cache_floor: ScopeEpoch,
    pub(super) closed: bool,
    pending: Option<Round>,
    active: Option<ScopeEpoch>,
}
pub(super) struct Owner {
    control: Mutex<Control>,
    changed: Notify,
    permits: Arc<Semaphore>,
    driver: Mutex<Option<JoinHandle<()>>>,
    receipt: Mutex<Option<Result<(), String>>>,
    drain_serial: tokio::sync::Mutex<()>,
}
impl Owner {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            control: Mutex::new(Control {
                epoch: ScopeEpoch(0),
                cache_floor: ScopeEpoch(0),
                closed: false,
                pending: None,
                active: None,
            }),
            changed: Notify::new(),
            permits: Arc::new(Semaphore::new(MAX_READERS)),
            driver: Mutex::new(None),
            receipt: Mutex::new(None),
            drain_serial: tokio::sync::Mutex::new(()),
        })
    }
    pub(super) fn lock(&self) -> MutexGuard<'_, Control> {
        self.control.lock().unwrap_or_else(|p| p.into_inner())
    }
    pub(super) fn current(&self) -> ScopeEpoch {
        self.lock().epoch
    }
    pub(super) fn is_current(&self, epoch: ScopeEpoch) -> bool {
        let c = self.lock();
        !c.closed && c.epoch == epoch
    }
    fn next(c: &mut Control) -> ScopeEpoch {
        c.epoch = ScopeEpoch(
            c.epoch
                .0
                .checked_add(1)
                .expect("conflict scope epoch exhausted"),
        );
        c.epoch
    }
    pub(super) fn replace(
        &self,
        metadata: ProjectRepositoryMetadata,
        repository: String,
        numbers: Vec<String>,
    ) -> Option<ScopeEpoch> {
        let epoch = {
            let mut c = self.lock();
            if c.closed {
                return None;
            }
            let epoch = Self::next(&mut c);
            c.pending = Some(Round {
                epoch,
                metadata,
                repository,
                numbers,
            });
            epoch
        };
        self.changed.notify_one();
        Some(epoch)
    }
    pub(super) fn invalidate(&self) -> ScopeEpoch {
        let epoch = {
            let mut c = self.lock();
            if c.closed {
                return c.epoch;
            }
            let epoch = Self::next(&mut c);
            c.cache_floor = epoch;
            c.pending = None;
            epoch
        };
        self.changed.notify_one();
        epoch
    }
    pub(super) fn close(&self) {
        {
            let mut c = self.lock();
            if c.closed {
                return;
            }
            c.closed = true;
            let epoch = Self::next(&mut c);
            c.cache_floor = epoch;
            c.pending = None;
        }
        self.permits.close();
        self.changed.notify_one();
    }
    pub(super) async fn permit(&self) -> Option<OwnedSemaphorePermit> {
        self.permits.clone().acquire_owned().await.ok()
    }
    /// The constructor installs this real handle before exposing the facade.
    pub(super) fn start(
        self: &Arc<Self>,
        runtime: &tokio::runtime::Handle,
        on_scope: impl Fn(ScopeEpoch) + Send + Sync + 'static,
        run: impl Fn(Round) -> BoxFuture<'static, ()> + Send + Sync + 'static,
    ) {
        let this = self.clone();
        let handle = runtime.spawn(async move {
            loop {
                // notify_one retains a permit even if this future has not been polled.
                let notified = this.changed.notified();
                let (epoch, closed, round) = {
                    let mut c = this.lock();
                    let round = c.pending.take();
                    c.active = round.as_ref().map(|r| r.epoch);
                    (c.epoch, c.closed, round)
                };
                on_scope(epoch);
                if closed {
                    break;
                }
                let Some(round) = round else {
                    notified.await;
                    continue;
                };
                let epoch = round.epoch;
                let work = run(round);
                tokio::pin!(work);
                loop {
                    let notified = this.changed.notified();
                    if !this.is_current(epoch) {
                        break;
                    }
                    tokio::select! {biased;
                        _=notified=>{if !this.is_current(epoch){break;}},
                        _=&mut work=>break,
                    }
                }
                // `work` (all nested HTTP/sleep futures and permits) is dropped
                // before taking the only pending replacement in the next loop.
                {
                    let mut c = this.lock();
                    if c.active == Some(epoch) {
                        c.active = None;
                    }
                }
            }
        });
        let mut driver = self.driver.lock().unwrap_or_else(|p| p.into_inner());
        assert!(driver.is_none(), "conflict status driver already installed");
        *driver = Some(handle);
    }
    /// Cancellation-safe: timeout/drop of this waiter leaves the actual handle
    /// in the owner. Serial callers get the same truthful cached join receipt.
    pub(super) async fn drain(&self) -> Result<(), String> {
        self.close();
        let _serial = self.drain_serial.lock().await;
        if let Some(result) = self
            .receipt
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            return result;
        }
        let result = poll_fn(|cx| {
            let mut driver = self.driver.lock().unwrap_or_else(|p| p.into_inner());
            Pin::new(
                driver
                    .as_mut()
                    .expect("conflict driver installed before admission"),
            )
            .poll(cx)
        })
        .await
        .map_err(|error| format!("conflict status driver join failed: {error}"));
        self.driver.lock().unwrap_or_else(|p| p.into_inner()).take();
        self.lock().active = None; // actual driver settled, including unwind/JoinError.
        *self.receipt.lock().unwrap_or_else(|p| p.into_inner()) = Some(result.clone());
        result
    }
    #[cfg(test)]
    pub(super) fn install_test_driver(&self, handle: JoinHandle<()>) {
        let mut driver = self.driver.lock().unwrap_or_else(|p| p.into_inner());
        assert!(driver.is_none());
        *driver = Some(handle);
    }
    #[cfg(test)]
    pub(super) fn has_test_driver(&self) -> bool {
        self.driver
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    }
    #[cfg(test)]
    pub(super) fn statistics(&self) -> (usize, usize, usize, bool) {
        let c = self.lock();
        (
            usize::from(c.pending.is_some()),
            usize::from(c.active.is_some()),
            MAX_READERS - self.permits.available_permits(),
            c.closed,
        )
    }
}
