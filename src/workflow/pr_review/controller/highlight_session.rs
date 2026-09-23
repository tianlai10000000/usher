//! A worker leases a session slot before it is queued. Detaching that slot
//! prevents a late worker from repopulating the next session's resources.
use std::sync::{Arc, Mutex, OnceLock};

pub(super) type HighlightLease<T> = Arc<OnceLock<Arc<T>>>;

pub(super) struct HighlightSession<T> {
    current: Mutex<HighlightLease<T>>,
}

impl<T> Default for HighlightSession<T> {
    fn default() -> Self {
        Self {
            current: Mutex::new(Arc::new(OnceLock::new())),
        }
    }
}

impl<T> HighlightSession<T> {
    pub(super) fn lease(&self) -> HighlightLease<T> {
        self.current
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub(super) fn detach(&self) {
        let previous = {
            let mut current = self.current.lock().unwrap_or_else(|p| p.into_inner());
            std::mem::replace(&mut *current, Arc::new(OnceLock::new()))
        };
        // Resource destruction must not run while holding the session lock.
        drop(previous);
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
    fn detached_resource_destructor_runs_outside_the_session_lock() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct RecordDrop {
            owner: std::sync::Weak<HighlightSession<Self>>,
            unlocked: Arc<AtomicBool>,
        }
        impl Drop for RecordDrop {
            fn drop(&mut self) {
                let owner = self.owner.upgrade().unwrap();
                self.unlocked
                    .store(owner.current.try_lock().is_ok(), Ordering::SeqCst);
            }
        }
        let owner = Arc::new(HighlightSession::default());
        let unlocked = Arc::new(AtomicBool::new(false));
        owner.lease().get_or_init(|| {
            Arc::new(RecordDrop {
                owner: Arc::downgrade(&owner),
                unlocked: unlocked.clone(),
            })
        });
        owner.detach();
        assert!(!owner.is_loaded());
        assert!(unlocked.load(Ordering::SeqCst));
    }

    #[test]
    fn successive_review_resets_do_not_collect_old_resource_slots() {
        let owner = HighlightSession::<Vec<u8>>::default();
        let mut previous = Vec::new();
        for review in 0..48 {
            let current = owner.lease();
            let resource = current.get_or_init(|| Arc::new(vec![0; 65536 << (review % 3)]));
            previous.push(Arc::downgrade(resource));
            owner.detach();
            assert!(!owner.is_loaded());
            assert_eq!(previous[review].strong_count(), 1);
            drop(current);
            assert!(previous.iter().all(|weak| weak.strong_count() == 0));
        }
    }

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
