//! A worker leases a session slot before it is queued. Detaching that slot
//! prevents a late worker from repopulating the next session's resources.
use std::sync::{Arc, Mutex, OnceLock};

pub(super) struct HighlightSession<T> {
    current: Mutex<Arc<OnceLock<Arc<T>>>>,
}

impl<T> Default for HighlightSession<T> {
    fn default() -> Self {
        Self {
            current: Mutex::new(Arc::new(OnceLock::new())),
        }
    }
}

impl<T> HighlightSession<T> {
    pub(super) fn lease(&self) -> Arc<OnceLock<Arc<T>>> {
        self.current
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub(super) fn detach(&self) {
        *self.current.lock().unwrap_or_else(|p| p.into_inner()) = Arc::new(OnceLock::new());
    }

    pub(super) fn is_loaded(&self) -> bool {
        self.current
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get()
            .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_worker_cannot_repopulate_detached_session() {
        let owner = HighlightSession::<String>::default();
        let queued = owner.lease();
        owner.detach();
        let old = queued.get_or_init(|| Arc::new("old grammar".into()));
        let weak = Arc::downgrade(old);
        assert!(!owner.is_loaded());
        let current = owner.lease();
        let new = current.get_or_init(|| Arc::new("new grammar".into()));
        assert!(!Arc::ptr_eq(old, new));
        drop(queued);
        assert!(weak.upgrade().is_none());
        assert!(owner.is_loaded());
    }

    #[test]
    fn completed_resources_drop_when_last_worker_releases_lease() {
        let owner = HighlightSession::<Vec<u8>>::default();
        let worker = owner.lease();
        let weak = Arc::downgrade(worker.get_or_init(|| Arc::new(vec![0; 1024])));
        assert!(owner.is_loaded());
        owner.detach();
        assert!(weak.upgrade().is_some());
        drop(worker);
        assert!(weak.upgrade().is_none());
        assert!(!owner.is_loaded());
    }
}
