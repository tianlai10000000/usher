//! Latest-only list enrichment and epoch-fenced ephemeral observations.
#[path = "pr_conflict_status_owner.rs"]
mod owner;
#[cfg(test)]
#[path = "pr_conflict_status_tests.rs"]
mod tests;
use crate::{
    MainWindow, PrConflictsBridge, git::conflicts::ConflictStatus,
    project::ProjectRepositoryMetadata,
};
use futures_util::{StreamExt, future::BoxFuture, stream};
use owner::{MAX_READERS, Owner, Round};
use slint::{ComponentHandle, Model};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use usher_state_store::{LocalStateStore, Reduction, StateStore, Subscription};
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) struct ScopeEpoch(u64);
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
    epoch: ScopeEpoch,
    at: Instant,
}
#[derive(Clone)]
struct CacheState {
    epoch: ScopeEpoch,
    entries: Vec<Observation>,
}
enum Action {
    Advance(ScopeEpoch, ScopeEpoch, Instant),
    Observe(ScopeEpoch, Key, ConflictStatus, Instant),
}
type Store = dyn StateStore<CacheState, Action, bool>;
type Reader = Arc<
    dyn Fn(ProjectRepositoryMetadata, String) -> BoxFuture<'static, ConflictStatus> + Send + Sync,
>;
#[derive(Clone)]
pub(super) struct StatusFacade {
    store: Arc<Store>,
    owner: Arc<Owner>,
    reader: Reader,
    backoff: Duration,
}
impl StatusFacade {
    fn new_on(runtime: &tokio::runtime::Handle, reader: Reader, backoff: Duration) -> Self {
        let store: Arc<Store> = Arc::new(LocalStateStore::new(
            CacheState {
                epoch: ScopeEpoch(0),
                entries: vec![],
            },
            |current: &CacheState, action| {
                let mut next = current.clone();
                match action {
                    Action::Advance(epoch, floor, at) => {
                        if epoch < current.epoch {
                            return Reduction::unchanged(next, false);
                        }
                        next.epoch = epoch;
                        next.entries.retain(|o| {
                            o.epoch >= floor
                                && at.saturating_duration_since(o.at) < Duration::from_secs(30)
                        });
                        if next.epoch == current.epoch
                            && next.entries.len() == current.entries.len()
                        {
                            return Reduction::unchanged(next, false);
                        }
                    }
                    Action::Observe(epoch, key, status, at) => {
                        if epoch != current.epoch {
                            return Reduction::unchanged(next, false);
                        }
                        next.entries.retain(|o| {
                            o.key != key
                                && at.saturating_duration_since(o.at) < Duration::from_secs(30)
                        });
                        next.entries.push(Observation {
                            key,
                            status,
                            epoch,
                            at,
                        });
                        if next.entries.len() > 64 {
                            next.entries.remove(0);
                        }
                    }
                }
                Reduction::changed(next, true)
            },
        ));
        let facade = Self {
            store,
            owner: Owner::new(),
            reader,
            backoff,
        };
        let scope = facade.clone();
        let work = facade.clone();
        facade.owner.start(
            runtime,
            move |epoch| scope.advance(epoch),
            move |round| {
                let work = work.clone();
                Box::pin(async move { work.run_round(round).await })
            },
        );
        facade
    }
    pub(super) fn scope(&self) -> ScopeEpoch {
        self.owner.current()
    }
    // Only private projection subscriptions are installed. Their synchronous
    // body merely enqueues a UI callback; it never reenters this short gate.
    // dispatch/reduction stays on background contributors/driver, never UI.
    fn advance(&self, epoch: ScopeEpoch) {
        let c = self.owner.lock();
        if c.epoch == epoch {
            self.store
                .dispatch(Action::Advance(epoch, c.cache_floor, Instant::now()));
        }
    }
    pub(super) fn observe(&self, epoch: ScopeEpoch, key: Key, status: ConflictStatus) -> bool {
        let c = self.owner.lock();
        if c.closed || c.epoch != epoch {
            return false;
        }
        self.store
            .dispatch(Action::Advance(epoch, c.cache_floor, Instant::now()));
        self.store
            .dispatch(Action::Observe(epoch, key, status, Instant::now()))
            .output
    }
    pub(super) fn get(&self, key: &Key) -> Option<ConflictStatus> {
        let c = self.owner.lock();
        if c.closed {
            return None;
        }
        self.store.snapshot().select(|s| {
            s.entries
                .iter()
                .find(|o| {
                    o.epoch >= c.cache_floor
                        && o.key == *key
                        && o.at.elapsed() < Duration::from_secs(30)
                })
                .map(|o| o.status)
        })
    }
    fn observations(&self, epoch: ScopeEpoch) -> Option<Vec<Observation>> {
        let c = self.owner.lock();
        if c.closed || c.epoch != epoch {
            return None;
        }
        Some(self.store.snapshot().select(|s| {
            s.entries
                .iter()
                .filter(|o| o.epoch >= c.cache_floor && o.at.elapsed() < Duration::from_secs(30))
                .cloned()
                .collect()
        }))
    }
    async fn run_round(&self, round: Round) {
        let epoch = round.epoch;
        let metadata = round.metadata;
        let repository = round.repository;
        stream::iter(round.numbers)
            .for_each_concurrent(MAX_READERS, |number| {
                let this = self.clone();
                let metadata = metadata.clone();
                let repository = repository.clone();
                async move {
                    let Some(_permit) = this.owner.permit().await else {
                        return;
                    };
                    let key = Key {
                        repository,
                        number: number.clone(),
                    };
                    if !this.owner.is_current(epoch) || key != Key::new(&metadata, &number) {
                        return;
                    }
                    let cached = this.get(&key);
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
                            tokio::time::sleep(this.backoff).await;
                        }
                        if !this.owner.is_current(epoch) || key != Key::new(&metadata, &number) {
                            return;
                        }
                        status = (this.reader)(metadata.clone(), number.clone()).await;
                    }
                    if key == Key::new(&metadata, &number) {
                        this.observe(epoch, key, status);
                    }
                }
            })
            .await;
    }
}
pub(super) fn shared() -> StatusFacade {
    static SHARED: OnceLock<StatusFacade> = OnceLock::new();
    SHARED
        .get_or_init(|| {
            StatusFacade::new_on(
                crate::app_runtime::shared_runtime().handle(),
                Arc::new(|metadata, number| {
                    Box::pin(async move {
                        crate::git::conflicts::read_status(
                            &metadata.repo_type,
                            &metadata.repo_project_id,
                            &metadata.remote_repo,
                            &number,
                        )
                        .await
                        .unwrap_or(ConflictStatus::Unknown)
                    })
                }),
                Duration::from_secs(1),
            )
        })
        .clone()
}
thread_local! {static SUBSCRIPTION:std::cell::RefCell<Option<Subscription>>=const{std::cell::RefCell::new(None)};}
pub(super) fn bind(ui: &MainWindow) {
    let facade = shared();
    // Eager owner/drain registration during workflow composition, never a lazy
    // per-enrich spawn after the legacy async-drain registry has been taken.
    static REGISTERED: OnceLock<()> = OnceLock::new();
    let weak = ui.as_weak();
    let listener_weak = weak.clone();
    REGISTERED.get_or_init(|| {
        let owner = facade.owner.clone();
        crate::shutdown::register_async_drain("cancel_pr_conflict_status", move || {
            Box::pin(async move {
                if let Err(error) = owner.drain().await {
                    log::error!("[Workflow][Conflicts] {error}");
                }
            })
        });
        let changed = facade.clone();
        crate::settings::get_settings_manager().register_git_change_listener(Arc::new(move || {
            let epoch = changed.owner.invalidate();
            let weak = listener_weak.clone();
            let changed = changed.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if changed.owner.is_current(epoch)
                    && let Some(ui) = weak.upgrade()
                {
                    reset_projection(&ui);
                }
            });
        }));
    });
    let projection = facade.clone();
    let subscription = facade.store.subscribe(Arc::new(move |snapshot| {
        // No owner gate, Store read, UI access or arbitrary callback here.
        let epoch = snapshot.state().epoch;
        let weak = weak.clone();
        let projection = projection.clone();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let Some(observations) = projection.observations(epoch) else {
                return;
            };
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
                }) && row.conflict_status != o.status.as_str()
                {
                    row.conflict_status = o.status.as_str().into();
                    rows.set_row_data(i, row);
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
    }));
    SUBSCRIPTION.with(|s| *s.borrow_mut() = Some(subscription));
}
pub(super) fn enrich(ui: &MainWindow, metadata: ProjectRepositoryMetadata) {
    let facade = shared();
    let repository = Key::new(&metadata, "").repository;
    let rows = ui.get_workflow_pr_list_items();
    let mut numbers = vec![];
    for i in 0..rows.row_count() {
        if let Some(row) = rows.row_data(i)
            && !matches!(row.state.as_str(), "closed" | "merged")
        {
            numbers.push(row.number.to_string());
        }
    }
    if facade
        .owner
        .replace(metadata, repository.clone(), numbers)
        .is_none()
    {
        return;
    }
    ui.global::<PrConflictsBridge>()
        .set_list_repository(repository.into());
    for i in 0..rows.row_count() {
        if let Some(mut row) = rows.row_data(i)
            && !matches!(row.state.as_str(), "closed" | "merged")
        {
            row.conflict_status = "checking".into();
            rows.set_row_data(i, row);
        }
    }
}
fn reset_projection(ui: &MainWindow) {
    let b = ui.global::<PrConflictsBridge>();
    b.set_file_count(-1);
    b.set_list_repository("".into());
    b.set_repository("".into());
    b.set_status("unknown".into());
    let rows = ui.get_workflow_pr_list_items();
    for i in 0..rows.row_count() {
        if let Some(mut row) = rows.row_data(i)
            && !matches!(row.state.as_str(), "closed" | "merged")
        {
            row.conflict_status = "unknown".into();
            rows.set_row_data(i, row);
        }
    }
}
pub(super) fn invalidate(ui: &MainWindow) {
    shared().owner.invalidate();
    reset_projection(ui);
}
