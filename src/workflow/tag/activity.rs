//! Process cancellation ownership, separate from immutable Workflow state.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
pub(crate) struct TagActivity {
    init: Mutex<Option<Arc<AtomicBool>>>,
    run: Mutex<Option<Arc<AtomicBool>>>,
}
impl TagActivity {
    fn replace(slot: &Mutex<Option<Arc<AtomicBool>>>) -> Arc<AtomicBool> {
        let mut slot = slot.lock().unwrap();
        if let Some(old) = slot.take() {
            old.store(true, Ordering::Release);
        }
        let token = Arc::new(AtomicBool::new(false));
        *slot = Some(token.clone());
        token
    }
    fn cancel(slot: &Mutex<Option<Arc<AtomicBool>>>) {
        if let Some(token) = slot.lock().unwrap().take() {
            token.store(true, Ordering::Release);
        }
    }
    pub(crate) fn initialize(&self) -> Arc<AtomicBool> {
        Self::replace(&self.init)
    }
    pub(crate) fn start(&self) -> Arc<AtomicBool> {
        Self::replace(&self.run)
    }
    pub(crate) fn cancel_init(&self) {
        Self::cancel(&self.init);
    }
    pub(crate) fn stop(&self) {
        Self::cancel(&self.run);
    }
    pub(crate) fn cancel_all(&self) {
        self.cancel_init();
        self.stop();
    }
}
