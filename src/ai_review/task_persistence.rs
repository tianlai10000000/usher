//! Ordered task snapshot persistence and shutdown. Session cleanup stays here,
//! while the controller only owns the commit subscription and lifecycle call.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;

use usher_ai_review::ReviewTaskState;
use usher_state_store::Subscription;

use super::storage::{reconcile_session_files, remove_stale_temporary_files};

pub(super) enum TaskPersistenceCommand {
    Persist(Arc<ReviewTaskState>),
    Shutdown,
}

pub(super) struct TaskPersistenceWorker {
    pub(super) sender: mpsc::Sender<TaskPersistenceCommand>,
    handle: Mutex<Option<JoinHandle<()>>>,
    subscription: Mutex<Option<Subscription>>,
    writes_allowed: Arc<AtomicBool>,
}

impl TaskPersistenceWorker {
    pub(super) fn start(path: PathBuf, session_dir: PathBuf, writes_allowed: bool) -> Self {
        let (sender, receiver) = mpsc::channel();
        let writes_allowed = Arc::new(AtomicBool::new(writes_allowed));
        let writes_allowed_worker = writes_allowed.clone();
        let handle = std::thread::Builder::new()
            .name("ai-review-tasks".into())
            .spawn(move || {
                remove_stale_temporary_files(path.parent().unwrap_or_else(|| Path::new(".")));
                run(receiver, |latest| {
                    if !writes_allowed_worker.load(Ordering::Acquire) {
                        return;
                    }
                    if let Err(error) = persist_task_state(&path, &latest) {
                        log::error!("[AI Review][Task Store] Persistence failed: {error}");
                        return;
                    }
                    let _ = reconcile_session_files(&session_dir, latest.tasks.keys());
                });
            })
            .expect("failed to start AI review task persistence worker");
        Self {
            sender,
            handle: Mutex::new(Some(handle)),
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
        let mut handle = self.handle.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(worker) = handle.take() {
            let _ = self.sender.send(TaskPersistenceCommand::Shutdown);
            let _ = worker.join();
        }
    }
}

fn run(
    receiver: mpsc::Receiver<TaskPersistenceCommand>,
    mut persist: impl FnMut(Arc<ReviewTaskState>),
) {
    while let Ok(command) = receiver.recv() {
        let TaskPersistenceCommand::Persist(mut latest) = command else {
            break;
        };
        let mut shutdown = false;
        while let Ok(command) = receiver.try_recv() {
            match command {
                TaskPersistenceCommand::Persist(newer) => latest = newer,
                TaskPersistenceCommand::Shutdown => {
                    shutdown = true;
                    break;
                }
            }
        }
        // Complete the latest snapshot before the control barrier, including
        // when the callback skips disabled writes or handles an I/O error.
        persist(latest);
        if shutdown {
            break;
        }
    }
}

fn persist_task_state(path: &Path, state: &ReviewTaskState) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?;
    usher_fs::atomic_write(path, &data).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use usher_ai_review::ReviewTaskId;

    fn snapshot(revision: &str) -> Arc<ReviewTaskState> {
        Arc::new(ReviewTaskState {
            order: vec![ReviewTaskId(revision.into())],
            ..Default::default()
        })
    }

    fn assert_done(done: mpsc::Receiver<()>, handle: JoinHandle<()>) {
        done.recv_timeout(Duration::from_secs(2))
            .expect("worker did not stop");
        handle.join().unwrap();
    }

    #[test]
    fn idle_shutdown_exits_with_sender_still_alive() {
        let (sender, receiver) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            run(receiver, |_| panic!("no snapshot was queued"));
            done.send(()).unwrap();
        });
        sender.send(TaskPersistenceCommand::Shutdown).unwrap();
        assert_done(finished, handle);
    }

    #[test]
    fn queued_shutdown_persists_latest_before_barrier_and_ignores_later_data() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tasks.json");
        let (sender, receiver) = mpsc::channel();
        sender
            .send(TaskPersistenceCommand::Persist(snapshot("old")))
            .unwrap();
        sender
            .send(TaskPersistenceCommand::Persist(snapshot("latest")))
            .unwrap();
        sender.send(TaskPersistenceCommand::Shutdown).unwrap();
        sender
            .send(TaskPersistenceCommand::Persist(snapshot("after-close")))
            .unwrap();
        let (done, finished) = mpsc::channel();
        let output = path.clone();
        let handle = std::thread::spawn(move || {
            run(receiver, |state| {
                persist_task_state(&output, &state).unwrap()
            });
            done.send(()).unwrap();
        });
        assert_done(finished, handle);
        let persisted: ReviewTaskState =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(persisted.order, snapshot("latest").order);
    }

    #[test]
    fn shutdown_during_write_drains_newer_snapshot_even_when_write_fails() {
        let (sender, receiver) = mpsc::channel();
        let (entered, writing) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let observed = seen.clone();
        let directory = tempfile::tempdir().unwrap();
        let invalid_file = directory.path().to_path_buf();
        let handle = std::thread::spawn(move || {
            run(receiver, |state| {
                observed.lock().unwrap().push(state.order[0].0.clone());
                if state.order[0].0 == "writing" {
                    entered.send(()).unwrap();
                    resume.recv_timeout(Duration::from_secs(2)).unwrap();
                }
                assert!(persist_task_state(&invalid_file, &state).is_err());
            });
            done.send(()).unwrap();
        });
        sender
            .send(TaskPersistenceCommand::Persist(snapshot("writing")))
            .unwrap();
        writing.recv_timeout(Duration::from_secs(2)).unwrap();
        sender
            .send(TaskPersistenceCommand::Persist(snapshot("latest")))
            .unwrap();
        sender.send(TaskPersistenceCommand::Shutdown).unwrap();
        release.send(()).unwrap();
        assert_done(finished, handle);
        assert_eq!(*seen.lock().unwrap(), ["writing", "latest"]);
    }

    #[test]
    fn disabled_worker_drains_and_repeated_shutdown_is_safe() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tasks.json");
        let worker =
            TaskPersistenceWorker::start(path.clone(), directory.path().join("sessions"), false);
        worker
            .sender
            .send(TaskPersistenceCommand::Persist(snapshot("disabled")))
            .unwrap();
        let (done, finished) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            worker.shutdown();
            worker.shutdown();
            done.send(()).unwrap();
        });
        assert_done(finished, handle);
        assert!(!path.exists());
    }
}
