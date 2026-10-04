use super::*;
use futures_util::future;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::oneshot;
fn metadata() -> ProjectRepositoryMetadata {
    ProjectRepositoryMetadata {
        repo_type: "github".into(),
        repo_project_id: "77".into(),
        remote_repo: "https://github.fixture/fixture/repo.git".into(),
    }
}
fn key(number: &str) -> Key {
    Key {
        repository: "test-repository".into(),
        number: number.into(),
    }
}
async fn until(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !predicate() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("actual driver state did not settle");
}
struct ReadGuard(Arc<AtomicUsize>);
impl Drop for ReadGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
#[tokio::test]
async fn storm_keeps_one_pending_round_and_four_real_raii_permits() {
    let owner = Owner::new();
    let reads = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(Mutex::new(Vec::new()));
    let driver = owner.clone();
    let active = reads.clone();
    let max = peak.clone();
    let history = started.clone();
    owner.start(
        &tokio::runtime::Handle::current(),
        |_| {},
        move |round| {
            let owner = driver.clone();
            let reads = active.clone();
            let peak = max.clone();
            let started = history.clone();
            Box::pin(async move {
                started.lock().unwrap().push(round.epoch);
                stream::iter(round.numbers)
                    .for_each_concurrent(MAX_READERS, move |_| {
                        let owner = owner.clone();
                        let reads = reads.clone();
                        let peak = peak.clone();
                        async move {
                            let Some(_permit) = owner.permit().await else {
                                return;
                            };
                            let count = reads.fetch_add(1, Ordering::SeqCst) + 1;
                            peak.fetch_max(count, Ordering::SeqCst);
                            let _guard = ReadGuard(reads);
                            future::pending::<()>().await;
                        }
                    })
                    .await;
            })
        },
    );
    let first = owner
        .replace(metadata(), "first".into(), vec!["1".into(); 10])
        .unwrap();
    until(|| reads.load(Ordering::SeqCst) == 4).await;
    let mut last = first;
    // No await: the real driver cannot take any intermediate pending input.
    for _ in 0..1000 {
        last = owner
            .replace(metadata(), "last".into(), vec!["2".into(); 10])
            .unwrap();
        assert_eq!(owner.statistics().0, 1);
    }
    until(|| started.lock().unwrap().last() == Some(&last) && reads.load(Ordering::SeqCst) == 4)
        .await;
    assert_eq!(*started.lock().unwrap(), vec![first, last]);
    assert_eq!(peak.load(Ordering::SeqCst), 4);
    owner.drain().await.unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(owner.statistics(), (0, 0, 0, true));
    assert!(
        owner
            .replace(metadata(), "late".into(), vec!["3".into()])
            .is_none()
    );
}
#[tokio::test]
async fn invalidation_cancels_accepted_read_future_without_new_requests() {
    let owner = Owner::new();
    let entered = Arc::new(AtomicUsize::new(0));
    let active = entered.clone();
    owner.start(
        &tokio::runtime::Handle::current(),
        |_| {},
        move |_| {
            let active = active.clone();
            Box::pin(async move {
                active.fetch_add(1, Ordering::SeqCst);
                let _guard = ReadGuard(active);
                future::pending::<()>().await;
            })
        },
    );
    owner.replace(metadata(), "repo".into(), vec!["1".into()]);
    until(|| entered.load(Ordering::SeqCst) == 1).await;
    owner.invalidate();
    until(|| entered.load(Ordering::SeqCst) == 0).await;
    assert_eq!(owner.statistics().0, 0);
    owner.drain().await.unwrap();
}
#[tokio::test]
async fn replacement_drops_actual_backoff_sleep_before_its_deadline() {
    let owner = Owner::new();
    let active = Arc::new(AtomicUsize::new(0));
    let seen = active.clone();
    let completed = Arc::new(AtomicUsize::new(0));
    let finished = completed.clone();
    owner.start(
        &tokio::runtime::Handle::current(),
        |_| {},
        move |_| {
            let active = seen.clone();
            let completed = finished.clone();
            Box::pin(async move {
                active.fetch_add(1, Ordering::SeqCst);
                let _guard = ReadGuard(active);
                tokio::time::sleep(Duration::from_secs(60)).await;
                completed.fetch_add(1, Ordering::SeqCst);
            })
        },
    );
    owner.replace(metadata(), "sleep".into(), vec![]);
    until(|| active.load(Ordering::SeqCst) == 1).await;
    owner.invalidate();
    until(|| active.load(Ordering::SeqCst) == 0).await;
    assert_eq!(completed.load(Ordering::SeqCst), 0);
    owner.drain().await.unwrap();
}
#[tokio::test]
async fn canceled_drain_keeps_the_real_join_handle_and_repeated_receipt() {
    let owner = Owner::new();
    let (tx, rx) = oneshot::channel::<()>();
    let completed = Arc::new(AtomicUsize::new(0));
    let finished = completed.clone();
    owner.install_test_driver(tokio::spawn(async move {
        rx.await.unwrap();
        finished.fetch_add(1, Ordering::SeqCst);
    }));
    assert!(
        tokio::time::timeout(Duration::from_millis(10), owner.drain())
            .await
            .is_err()
    );
    assert!(owner.has_test_driver());
    assert_eq!(completed.load(Ordering::SeqCst), 0);
    tx.send(()).unwrap();
    owner.drain().await.unwrap();
    assert_eq!(completed.load(Ordering::SeqCst), 1);
    owner.drain().await.unwrap();
}
#[tokio::test]
async fn real_driver_join_failure_is_sticky_and_close_rejects_mutation() {
    let owner = Owner::new();
    owner.install_test_driver(tokio::spawn(async { panic!("fixture driver panic") }));
    let first = owner.drain().await.unwrap_err();
    let second = owner.drain().await.unwrap_err();
    assert_eq!(first, second);
    assert!(first.contains("panic"));
    assert!(owner.replace(metadata(), "late".into(), vec![]).is_none());
}
fn facade() -> StatusFacade {
    StatusFacade::new_on(
        &tokio::runtime::Handle::current(),
        Arc::new(|_, _| Box::pin(async { ConflictStatus::Unknown })),
        Duration::from_secs(1),
    )
}
#[tokio::test]
async fn typed_observations_cannot_commit_through_a_new_scope_or_clear() {
    let facade = facade();
    let old = facade
        .owner
        .replace(metadata(), "repo".into(), vec![])
        .unwrap();
    assert!(facade.observe(old, key("1"), ConflictStatus::Clear));
    let new = facade.owner.invalidate();
    assert!(new > old);
    assert!(!facade.observe(old, key("1"), ConflictStatus::Conflicting));
    assert_eq!(facade.get(&key("1")), None);
    assert!(facade.observe(new, key("1"), ConflictStatus::Clear));
    assert!(!facade.observe(old, key("1"), ConflictStatus::Conflicting));
    assert_eq!(facade.get(&key("1")), Some(ConflictStatus::Clear));
    facade.owner.drain().await.unwrap();
    assert!(!facade.observe(new, key("1"), ConflictStatus::Clear));
}
#[tokio::test]
async fn replacement_preserves_valid_cache_but_invalidation_drops_eligibility() {
    let facade = facade();
    let first = facade.scope();
    assert!(facade.observe(first, key("1"), ConflictStatus::Checking));
    facade.owner.replace(metadata(), "repo".into(), vec![]);
    assert_eq!(facade.get(&key("1")), Some(ConflictStatus::Checking));
    facade.owner.invalidate();
    assert_eq!(facade.get(&key("1")), None);
    facade.owner.drain().await.unwrap();
}
#[tokio::test]
async fn actual_cache_remains_bounded_to_64_and_repository_is_part_of_identity() {
    let facade = facade();
    let scope = facade.scope();
    for i in 0..70 {
        assert!(facade.observe(scope, key(&i.to_string()), ConflictStatus::Clear));
    }
    assert_eq!(facade.store.snapshot().state().entries.len(), 64);
    assert_eq!(facade.get(&key("0")), None);
    assert_eq!(facade.get(&key("69")), Some(ConflictStatus::Clear));
    assert_eq!(
        facade.get(&Key {
            repository: "other".into(),
            number: "69".into()
        }),
        None
    );
    facade.owner.drain().await.unwrap();
}

#[derive(Default)]
struct CommitTestGate {
    released: Mutex<bool>,
    changed: std::sync::Condvar,
}
impl CommitTestGate {
    fn open(&self) {
        *self.released.lock().unwrap_or_else(|p| p.into_inner()) = true;
        self.changed.notify_all();
    }
    fn wait(&self, deadline: Duration) -> bool {
        let state = self.released.lock().unwrap_or_else(|p| p.into_inner());
        let (state, _) = self
            .changed
            .wait_timeout_while(state, deadline, |open| !*open)
            .unwrap_or_else(|p| p.into_inner());
        *state
    }
}
struct ReleaseCommitOnDrop(Arc<CommitTestGate>);
impl Drop for ReleaseCommitOnDrop {
    fn drop(&mut self) {
        self.0.open();
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalidation_serializes_with_the_actual_commit_and_rejects_late_repopulation() {
    let facade = facade();
    let old = facade.scope();
    let gate = Arc::new(CommitTestGate::default());
    let release = ReleaseCommitOnDrop(gate.clone());
    let held = gate.clone();
    let (entered_tx, entered_rx) = oneshot::channel();
    let entered = Arc::new(Mutex::new(Some(entered_tx)));
    // Observe commits rather than subscribe: subscribe hydrates immediately,
    // before the contributor below exists. Hold the actual changed dispatch.
    // RAII releases even if an assertion/await panics before normal cleanup.
    let subscription = facade.store.observe_commits(Arc::new(move |_| {
        if let Some(tx) = entered.lock().unwrap().take() {
            let _ = tx.send(());
            assert!(
                held.wait(Duration::from_secs(5)),
                "test commit hold deadline"
            );
        }
    }));
    let contributor = facade.clone();
    let mut commit = tokio::task::spawn_blocking(move || {
        contributor.observe(old, key("1"), ConflictStatus::Clear)
    });
    let mut invalidation = None;
    let inspection = async {
        tokio::time::timeout(Duration::from_secs(2), entered_rx)
            .await
            .map_err(|_| "commit-entry deadline")?
            .map_err(|_| "commit entry channel closed")?;
        let (attempt_tx, attempt_rx) = oneshot::channel();
        let invalidator = facade.clone();
        invalidation = Some(tokio::task::spawn_blocking(move || {
            let _ = attempt_tx.send(());
            invalidator.owner.invalidate()
        }));
        tokio::time::timeout(Duration::from_secs(2), attempt_rx)
            .await
            .map_err(|_| "invalidate-entry deadline")?
            .map_err(|_| "invalidate entry channel closed")?;
        if invalidation.as_ref().unwrap().is_finished() {
            return Err("invalidation passed an active old commit gate");
        }
        Ok(())
    }
    .await;
    // Release before asserting inspection, then reap real handles by borrowed wait.
    release.0.open();
    let committed = tokio::time::timeout(Duration::from_secs(2), &mut commit)
        .await
        .expect("commit reap deadline")
        .expect("actual commit JoinError");
    let new = if let Some(handle) = invalidation.as_mut() {
        Some(
            tokio::time::timeout(Duration::from_secs(2), handle)
                .await
                .expect("invalidate reap deadline")
                .expect("actual invalidation JoinError"),
        )
    } else {
        None
    };
    drop(subscription);
    let cache_after_invalidation = facade.get(&key("1"));
    let stale_accepted = facade.observe(old, key("1"), ConflictStatus::Conflicting);
    tokio::time::timeout(Duration::from_secs(2), facade.owner.drain())
        .await
        .expect("driver drain deadline")
        .unwrap();
    inspection.expect("bounded race inspection");
    assert!(committed);
    assert!(new.unwrap() > old);
    assert_eq!(cache_after_invalidation, None);
    assert!(!stale_accepted);
}
#[test]
fn test_commit_release_guard_unwinds_and_actor_is_reaped() {
    let gate = Arc::new(CommitTestGate::default());
    let held = gate.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let actor = std::thread::spawn(move || {
        let _ = done_tx.send(held.wait(Duration::from_secs(2)));
    });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _release = ReleaseCommitOnDrop(gate);
        panic!("intentional test assertion failure");
    }));
    assert!(result.is_err());
    assert!(
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("released actor receipt")
    );
    actor.join().unwrap();
}
#[test]
fn test_commit_hold_deadline_returns_failure_without_orphaning_actor() {
    let gate = Arc::new(CommitTestGate::default());
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let actor = std::thread::spawn(move || {
        let _ = done_tx.send(gate.wait(Duration::from_millis(10)));
    });
    assert!(
        !done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("timed-out actor receipt")
    );
    actor.join().unwrap();
}
