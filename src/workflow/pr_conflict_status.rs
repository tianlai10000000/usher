//! Bounded ephemeral observations shared by the list and conflict workspace.
use crate::{
    MainWindow, PrConflictsBridge, git::conflicts::ConflictStatus,
    project::ProjectRepositoryMetadata,
};
use slint::{ComponentHandle, Model};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use usher_state_store::{LocalStateStore, Reduction, StateStore, Subscription};
static GENERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Key {
    pub(super) repository: String,
    pub(super) number: String,
}

impl Key {
    pub(super) fn new(meta: &ProjectRepositoryMetadata, number: &str) -> Self {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        crate::settings::get_settings_manager().with_git(|git| {
            let config = match meta.repo_type.as_str() {
                "github" => &git.platform.github,
                "gitlab" => &git.platform.gitlab,
                _ => &git.platform.gitee,
            };
            config.token.hash(&mut hash);
            config.enterprise_token.hash(&mut hash);
            config.api_base_url.hash(&mut hash);
            config.enterprise.hash(&mut hash);
        });
        Self {
            repository: format!(
                "{}\n{}\n{}\n{:x}",
                meta.repo_type,
                meta.repo_project_id,
                meta.remote_repo,
                hash.finish()
            ),
            number: number.into(),
        }
    }
}
#[derive(Clone)]
pub(super) struct Observation {
    pub(super) key: Key,
    pub(super) status: ConflictStatus,
    at: Instant,
}
enum Action {
    Observe(Key, ConflictStatus),
    Clear(Instant),
}
type Store = dyn StateStore<Vec<Observation>, Action, ()>;
#[derive(Clone)]
pub(super) struct StatusFacade(Arc<Store>);
pub(super) fn shared() -> StatusFacade {
    static STORE: OnceLock<StatusFacade> = OnceLock::new();
    STORE
        .get_or_init(|| {
            StatusFacade(Arc::new(LocalStateStore::new(
                Vec::<Observation>::new(),
                |current: &Vec<Observation>, action| {
                    let mut next = current.clone();
                    match action {
                        Action::Clear(at) => next.retain(|entry| entry.at > at),
                        Action::Observe(key, status) => {
                            next.retain(|entry| {
                                entry.key != key && entry.at.elapsed() < Duration::from_secs(30)
                            });
                            next.push(Observation {
                                key,
                                status,
                                at: Instant::now(),
                            });
                            if next.len() > 64 {
                                next.remove(0);
                            }
                        }
                    }
                    Reduction::changed(next, ())
                },
            )))
        })
        .clone()
}
impl StatusFacade {
    pub(super) fn observe(&self, key: Key, status: ConflictStatus) {
        self.0.dispatch(Action::Observe(key, status));
    }
    pub(super) fn clear(&self, at: Instant) {
        self.0.dispatch(Action::Clear(at));
    }
    pub(super) fn get(&self, key: &Key) -> Option<ConflictStatus> {
        self.0.snapshot().select(|s| {
            s.iter()
                .find(|o| o.key == *key && o.at.elapsed() < Duration::from_secs(30))
                .map(|o| o.status)
        })
    }
    fn subscribe(
        &self,
        listener: impl Fn(Vec<Observation>) + Send + Sync + 'static,
    ) -> Subscription {
        self.0
            .subscribe(Arc::new(move |snapshot| listener(snapshot.state().clone())))
    }
}
thread_local! { static SUBSCRIPTION: std::cell::RefCell<Option<Subscription>> = const { std::cell::RefCell::new(None) }; }
pub(super) fn bind(ui: &MainWindow) {
    let weak = ui.as_weak();
    let subscription = shared().subscribe(move |_| {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            let observations = shared().0.snapshot().state().clone();
            let bridge = ui.global::<PrConflictsBridge>();
            let repo = bridge.get_list_repository();
            let rows = ui.get_workflow_pr_list_items();
            for i in 0..rows.row_count() {
                let Some(mut row) = rows.row_data(i) else {
                    continue;
                };
                if matches!(row.state.as_str(), "closed" | "merged") {
                    continue;
                }
                if let Some(o) = observations.iter().find(|o| {
                    o.key.repository == repo.as_str() && o.key.number == row.number.as_str()
                }) {
                    if row.conflict_status != o.status.as_str() {
                        row.conflict_status = o.status.as_str().into();
                        rows.set_row_data(i, row);
                    }
                }
            }
            if !bridge.get_local()
                && let Some(o) = observations.iter().find(|o| {
                    o.key.repository == bridge.get_repository().as_str()
                        && o.key.number == bridge.get_number().as_str()
                })
            {
                bridge.set_status(o.status.as_str().into());
            }
        });
    });
    SUBSCRIPTION.with(|s| *s.borrow_mut() = Some(subscription));
}

pub(super) fn enrich(ui: &MainWindow, metadata: ProjectRepositoryMetadata) {
    use futures_util::{StreamExt, stream};
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    ui.global::<PrConflictsBridge>()
        .set_list_repository(Key::new(&metadata, "").repository.into());
    let rows = ui.get_workflow_pr_list_items();
    let mut pending = vec![];
    for i in 0..rows.row_count() {
        if let Some(mut row) = rows.row_data(i) {
            if matches!(row.state.as_str(), "closed" | "merged") {
                continue;
            }
            pending.push(row.number.to_string());
            row.conflict_status = "checking".into();
            rows.set_row_data(i, row);
        }
    }
    crate::app_runtime::shared_runtime().spawn(async move {
        stream::iter(pending)
            .for_each_concurrent(4, |number| {
                let metadata = metadata.clone();
                async move {
                    if GENERATION.load(Ordering::SeqCst) != generation {
                        return;
                    }
                    let key = Key::new(&metadata, &number);
                    let cache = shared();
                    let cached = cache.get(&key);
                    let mut status = cached.unwrap_or(ConflictStatus::Unknown);
                    let budget = if cached == Some(ConflictStatus::Checking) {
                        2
                    } else {
                        3
                    };
                    for attempt in 0..budget {
                        if !matches!(status, ConflictStatus::Unknown | ConflictStatus::Checking) {
                            break;
                        }
                        if attempt > 0 || cached == Some(ConflictStatus::Checking) {
                            if status != ConflictStatus::Checking {
                                break;
                            }
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                        if GENERATION.load(Ordering::SeqCst) != generation {
                            return;
                        }
                        status = crate::git::conflicts::read_status(
                            &metadata.repo_type,
                            &metadata.repo_project_id,
                            &metadata.remote_repo,
                            &number,
                        )
                        .await
                        .unwrap_or(ConflictStatus::Unknown);
                    }
                    if GENERATION.load(Ordering::SeqCst) == generation
                        && key == Key::new(&metadata, &number)
                    {
                        cache.observe(key, status);
                    }
                }
            })
            .await;
    });
}

pub(super) fn invalidate(ui: &MainWindow) {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    ui.global::<PrConflictsBridge>().set_file_count(-1);
    ui.global::<PrConflictsBridge>()
        .set_list_repository("".into());
    ui.global::<PrConflictsBridge>().set_repository("".into());
    ui.global::<PrConflictsBridge>()
        .set_status("unknown".into());
    let rows = ui.get_workflow_pr_list_items();
    for i in 0..rows.row_count() {
        if let Some(mut row) = rows.row_data(i) {
            if !matches!(row.state.as_str(), "closed" | "merged") {
                row.conflict_status = "unknown".into();
                rows.set_row_data(i, row);
            }
        }
    }
    let at = Instant::now();
    crate::app_runtime::shared_runtime().spawn_blocking(move || shared().clear(at));
}
