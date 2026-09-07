//! One debounced preference writer, flushed explicitly during application shutdown.
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;
use usher_state_store::LatestPersistence;

static SAVE_WORKER: OnceLock<SaveWorker> = OnceLock::new();
struct SaveWorker {
    writer: LatestPersistence<()>,
    revision: AtomicU64,
}
impl SaveWorker {
    fn new(save: impl Fn() -> Result<(), String> + Send + Sync + 'static) -> Self {
        Self {
            writer: LatestPersistence::start(
                "settings-save",
                Duration::from_millis(100),
                move |_| save(),
                |_, error| {
                    log::error!("[Settings] Failed to save settings: {error}");
                },
            )
            .expect("failed to start settings persistence worker"),
            revision: AtomicU64::new(0),
        }
    }
    fn request(&self) {
        let revision = self.revision.fetch_add(1, Ordering::AcqRel) + 1;
        self.writer.submit(revision, Arc::new(()));
    }
}
pub(super) fn request() {
    SAVE_WORKER
        .get_or_init(|| SaveWorker::new(|| super::get_settings_manager().save()))
        .request();
}
pub(super) fn shutdown() {
    if let Some(worker) = SAVE_WORKER.get() {
        if let Err(error) = worker.writer.shutdown() {
            log::error!("[Settings] Shutdown flush failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    #[test]
    fn burst_is_one_save_and_shutdown_flushes_the_last_preference() {
        let preference = Arc::new(AtomicUsize::new(0));
        let observed = Arc::new(AtomicUsize::new(0));
        let writes = Arc::new(AtomicUsize::new(0));
        let current = preference.clone();
        let saved = observed.clone();
        let count = writes.clone();
        let worker = SaveWorker::new(move || {
            saved.store(current.load(Ordering::Acquire), Ordering::Release);
            count.fetch_add(1, Ordering::AcqRel);
            Ok(())
        });
        for value in 1..=1000 {
            preference.store(value, Ordering::Release);
            worker.request();
        }
        assert_eq!(writes.load(Ordering::Acquire), 0);
        worker.writer.shutdown().unwrap();
        worker.writer.shutdown().unwrap();
        assert_eq!(observed.load(Ordering::Acquire), 1000);
        assert_eq!(writes.load(Ordering::Acquire), 1);
    }
}
