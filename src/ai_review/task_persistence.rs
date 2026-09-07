//! Feature policy over a bounded latest-snapshot writer. Session cleanup stays here.
use super::storage::{reconcile_session_files, remove_stale_temporary_files};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;
use usher_ai_review::ReviewTaskState;
use usher_state_store::{LatestPersistence, Subscription};

#[derive(Clone)]
pub(super) struct TaskPersistenceSender {
    writer: Arc<LatestPersistence<ReviewTaskState>>,
    revision: Arc<Mutex<(Option<u64>, u64)>>,
}
impl TaskPersistenceSender {
    pub(super) fn send(
        &self,
        store_revision: u64,
        value: Arc<ReviewTaskState>,
    ) -> Result<(), String> {
        // Equal Store revisions may be explicitly persisted after enabling
        // writes; older revisions may never overwrite a later commit.
        let mut revisions = self.revision.lock().unwrap_or_else(|p| p.into_inner());
        if revisions.0.is_some_and(|latest| store_revision < latest) {
            return Err("task persistence snapshot is obsolete".into());
        }
        revisions.0 = Some(store_revision);
        revisions.1 += 1;
        self.writer
            .submit(revisions.1, value)
            .then_some(())
            .ok_or_else(|| "task persistence is closed".into())
    }
}
pub(super) struct TaskPersistenceWorker {
    pub(super) sender: TaskPersistenceSender,
    subscription: Mutex<Option<Subscription>>,
    writes_allowed: Arc<AtomicBool>,
}
impl TaskPersistenceWorker {
    pub(super) fn start(path: PathBuf, session_dir: PathBuf, allowed: bool) -> Self {
        let writes_allowed = Arc::new(AtomicBool::new(allowed));
        let permission = writes_allowed.clone();
        let cleanup = Once::new();
        let writer = LatestPersistence::start(
            "ai-review-tasks",
            Duration::ZERO,
            move |latest: &ReviewTaskState| {
                cleanup.call_once(|| {
                    remove_stale_temporary_files(path.parent().unwrap_or_else(|| Path::new(".")))
                });
                if !permission.load(Ordering::Acquire) {
                    return Ok(());
                }
                persist_task_state(&path, latest)?;
                let _ = reconcile_session_files(&session_dir, latest.tasks.keys());
                Ok(())
            },
            |_, error| log::error!("[AI Review][Task Store] Persistence failed: {error}"),
        )
        .expect("failed to start AI review task persistence worker");
        Self::with_writer(writer, writes_allowed)
    }
    fn with_writer(
        writer: LatestPersistence<ReviewTaskState>,
        writes_allowed: Arc<AtomicBool>,
    ) -> Self {
        Self {
            sender: TaskPersistenceSender {
                writer: Arc::new(writer),
                revision: Arc::new(Mutex::new((None, 0))),
            },
            subscription: Mutex::new(None),
            writes_allowed,
        }
    }
    pub(super) fn set_writes_allowed(&self, allowed: bool) {
        self.writes_allowed.store(allowed, Ordering::Release);
    }
    pub(super) fn retain_subscription(&self, subscription: Subscription) {
        *self.subscription.lock().unwrap_or_else(|p| p.into_inner()) = Some(subscription);
    }
    pub(super) fn shutdown(&self) {
        self.subscription
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        let _ = self.sender.writer.shutdown();
    }
}
fn persist_task_state(path: &Path, state: &ReviewTaskState) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?;
    usher_fs::atomic_write(path, &data).map_err(|error| error.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use usher_ai_review::ReviewTaskId;
    fn snapshot(id: &str) -> Arc<ReviewTaskState> {
        Arc::new(ReviewTaskState {
            order: vec![ReviewTaskId(id.into())],
            ..Default::default()
        })
    }
    #[test]
    fn idle_and_repeated_shutdown_do_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let worker = TaskPersistenceWorker::start(path.clone(), dir.path().join("sessions"), true);
        worker.shutdown();
        worker.shutdown();
        assert!(!path.exists());
        assert!(worker.sender.send(1, snapshot("late")).is_err());
    }
    #[test]
    fn disabled_writes_and_io_errors_still_complete_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("disabled.json");
        let worker = TaskPersistenceWorker::start(path.clone(), dir.path().join("sessions"), false);
        worker.sender.send(1, snapshot("disabled")).unwrap();
        worker.shutdown();
        assert!(!path.exists());
        let worker = TaskPersistenceWorker::start(
            dir.path().to_path_buf(),
            dir.path().join("sessions"),
            true,
        );
        worker.sender.send(1, snapshot("error")).unwrap();
        worker.shutdown();
        worker.shutdown();
        assert!(dir.path().is_dir());
    }
    #[test]
    fn hydration_can_explicitly_persist_after_writes_are_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let worker = TaskPersistenceWorker::start(path.clone(), dir.path().join("sessions"), false);
        let state = snapshot("hydrated");
        worker.sender.send(1, state.clone()).unwrap();
        worker.sender.writer.flush().unwrap();
        assert!(!path.exists());
        worker.set_writes_allowed(true);
        worker.sender.send(1, state).unwrap();
        worker.shutdown();
        let saved: ReviewTaskState = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved.order[0].0, "hydrated");
    }
    #[test]
    fn shutdown_during_write_flushes_the_latest_pending_task_snapshot() {
        let (started, ready) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let gate = Mutex::new(gate);
        let saved = Arc::new(Mutex::new(Vec::new()));
        let record = saved.clone();
        let writer = LatestPersistence::start(
            "task-write-test",
            Duration::ZERO,
            move |state: &ReviewTaskState| {
                if state.order[0].0 == "first" {
                    started.send(()).unwrap();
                    gate.lock().unwrap().recv().unwrap();
                }
                record.lock().unwrap().push(state.order[0].0.clone());
                Ok(())
            },
            |_, _| {},
        )
        .unwrap();
        let worker = TaskPersistenceWorker::with_writer(writer, Arc::new(AtomicBool::new(true)));
        worker.sender.send(1, snapshot("first")).unwrap();
        ready.recv_timeout(Duration::from_secs(1)).unwrap();
        worker.sender.send(2, snapshot("middle")).unwrap();
        worker.sender.send(3, snapshot("last")).unwrap();
        assert!(worker.sender.send(2, snapshot("obsolete")).is_err());
        let (done, wait) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            worker.shutdown();
            done.send(()).unwrap();
        });
        assert!(wait.recv_timeout(Duration::from_millis(30)).is_err());
        release.send(()).unwrap();
        wait.recv_timeout(Duration::from_secs(1)).unwrap();
        thread.join().unwrap();
        assert_eq!(*saved.lock().unwrap(), vec!["first", "last"]);
    }
}
