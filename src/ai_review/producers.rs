//! Admission and whole-operation receipts for AI Review background producers.
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

#[derive(Default)]
struct State {
    closed: bool,
    active: usize,
    finalizer_started: bool,
    result: Option<Result<(), String>>,
}

#[derive(Default)]
struct Inner {
    state: Mutex<State>,
    changed: Notify,
}

#[derive(Clone, Default)]
pub(super) struct Producers(Arc<Inner>);

pub(super) struct ProducerPermit(Arc<Inner>);

impl Drop for ProducerPermit {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap_or_else(|p| p.into_inner());
        state.active -= 1;
        drop(state);
        self.0.changed.notify_waiters();
    }
}

impl Producers {
    /// Register synchronously, before accepting a Session operation or spawning
    /// a future. The permit covers all terminal continuations of that future.
    pub(super) fn admit(&self) -> Result<ProducerPermit, &'static str> {
        let mut state = self.0.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.closed {
            return Err("AI Review producer admission is closed");
        }
        state.active += 1;
        Ok(ProducerPermit(self.0.clone()))
    }

    /// The first caller owns cancellation/recovery setup before the finalizer.
    pub(super) fn close(&self) -> bool {
        let mut state = self.0.state.lock().unwrap_or_else(|p| p.into_inner());
        !std::mem::replace(&mut state.closed, true)
    }

    pub(super) fn is_closed(&self) -> bool {
        self.0
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .closed
    }

    pub(super) fn completed(&self) -> Option<Result<(), String>> {
        self.0
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .result
            .clone()
    }

    pub(super) fn finish_with(&self, finalize: impl FnOnce() + Send + 'static) {
        {
            let mut state = self.0.state.lock().unwrap_or_else(|p| p.into_inner());
            if state.finalizer_started {
                return;
            }
            state.finalizer_started = true;
            state.closed = true;
        }
        let producers = self.clone();
        // The owned task survives cancellation of any shutdown waiter.
        crate::app_runtime::shared_runtime().spawn(async move {
            producers.wait_idle().await;
            let result = tokio::task::spawn_blocking(finalize)
                .await
                .map_err(|error| format!("AI Review final shutdown worker failed: {error}"));
            producers
                .0
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .result = Some(result);
            producers.0.changed.notify_waiters();
        });
    }

    async fn wait_idle(&self) {
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self
                .0
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .active
                == 0
            {
                return;
            }
            changed.await;
        }
    }

    pub(super) async fn finished(&self) -> Result<(), String> {
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some(result) = self.completed() {
                return result;
            }
            changed.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn before_first_poll_permit_and_cancelled_waiter_keep_one_final_barrier() {
        let producers = Producers::default();
        let admitted = producers.admit().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        assert!(producers.close());
        assert!(producers.admit().is_err());
        producers.finish_with({
            let calls = calls.clone();
            move || {
                calls.fetch_add(1, Ordering::SeqCst);
            }
        });
        let waiter = crate::app_runtime::shared_runtime().spawn({
            let producers = producers.clone();
            async move { producers.finished().await }
        });
        waiter.abort();
        assert!(
            tokio::time::timeout(Duration::from_millis(30), producers.finished())
                .await
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        drop(admitted);
        producers.finished().await.unwrap();
        producers.finish_with(|| panic!("duplicate finalizer"));
        producers.finished().await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
