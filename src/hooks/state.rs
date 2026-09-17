use super::types::*;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use usher_state_store::{PersistentStore, Reduction, StoreSnapshot, Subscription};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct HookState {
    pub enabled: bool,
    #[serde(
        serialize_with = "serialize_hooks",
        deserialize_with = "deserialize_hooks"
    )]
    pub hooks: Arc<Vec<HookDefinition>>,
    pub runs: Vec<HookRun>,
    #[serde(skip)]
    pub events: Arc<Vec<EventDefinition>>,
    #[serde(skip)]
    pub ready: bool,
    #[serde(skip)]
    pub error: String,
    #[serde(skip)]
    pub durable_revision: u64,
    #[serde(skip)]
    pub catalog_revision: u64,
}

fn serialize_hooks<S: serde::Serializer>(
    hooks: &Arc<Vec<HookDefinition>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    hooks.as_ref().serialize(serializer)
}
fn deserialize_hooks<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Arc<Vec<HookDefinition>>, D::Error> {
    Vec::<HookDefinition>::deserialize(deserializer).map(Arc::new)
}

fn same_durable_run(a: &HookRun, b: &HookRun) -> bool {
    a.id == b.id
        && a.hook_id == b.hook_id
        && a.config_revision == b.config_revision
        && a.name == b.name
        && a.event_id == b.event_id
        && a.event == b.event
        && a.project == b.project
        && a.test == b.test
        && a.created_at_ms == b.created_at_ms
        && a.started_at_ms == b.started_at_ms
        && a.finished_at_ms == b.finished_at_ms
        && a.conclusion == b.conclusion
        && a.exit_code == b.exit_code
        && a.signal == b.signal
        && a.detail == b.detail
}

pub(super) enum Action {
    Hydrate(HookState),
    LoadFailed(String),
    Register(EventDefinition),
    Enable(bool),
    SetHookEnabled { id: String, enabled: bool },
    Save(HookDefinition),
    Delete(String),
    ClearRuns(Vec<String>),
    Run(HookRun),
}

fn reduce(state: &HookState, action: Action) -> Reduction<HookState, Result<(), String>> {
    let mut next = state.clone();
    let catalog_changed = !matches!(&action, Action::Run(_) | Action::ClearRuns(_));
    let mut durable_changed = !matches!(
        &action,
        Action::Hydrate(_) | Action::LoadFailed(_) | Action::Register(_)
    );
    match action {
        Action::Hydrate(mut loaded) => {
            loaded.events = next.events;
            loaded.ready = true;
            next = loaded;
        }
        Action::LoadFailed(error) => next.error = error,
        Action::Register(event) => {
            if next.events.iter().any(|e| e.name == event.name) {
                return Reduction::unchanged(next, Ok(()));
            }
            Arc::make_mut(&mut next.events).push(event);
        }
        Action::Enable(enabled) => {
            if !state.ready {
                return Reduction::unchanged(
                    next,
                    Err("Hook configuration has not loaded.".into()),
                );
            }
            if next.enabled == enabled {
                return Reduction::unchanged(next, Ok(()));
            }
            next.enabled = enabled;
        }
        Action::SetHookEnabled { id, enabled } => {
            if !state.ready {
                return Reduction::unchanged(
                    next,
                    Err("Hook configuration has not loaded.".into()),
                );
            }
            let Some(index) = next.hooks.iter().position(|hook| hook.id == id) else {
                return Reduction::unchanged(next, Err("Hook definition was not found.".into()));
            };
            if next.hooks[index].enabled == enabled {
                return Reduction::unchanged(next, Ok(()));
            }
            let hook = &mut Arc::make_mut(&mut next.hooks)[index];
            hook.enabled = enabled;
            hook.revision += 1;
        }
        Action::Save(mut hook) => {
            if !state.ready {
                return Reduction::unchanged(
                    next,
                    Err("Hook configuration has not loaded.".into()),
                );
            }
            if let Err(error) = hook.validate() {
                return Reduction::unchanged(next, Err(error));
            }
            if !state.events.iter().any(|event| event.name == hook.event) {
                return Reduction::unchanged(
                    next,
                    Err("This Hook event is not registered by an enabled module.".into()),
                );
            }
            if let Some(existing) = Arc::make_mut(&mut next.hooks)
                .iter_mut()
                .find(|h| h.id == hook.id)
            {
                hook.revision = existing.revision + 1;
                *existing = hook;
            } else if next.hooks.len() < 32 {
                hook.revision = 1;
                Arc::make_mut(&mut next.hooks).push(hook);
            } else {
                return Reduction::unchanged(
                    next,
                    Err("At most 32 Hooks can be configured.".into()),
                );
            }
        }
        Action::Delete(id) => {
            if !next.hooks.iter().any(|h| h.id == id) {
                return Reduction::unchanged(next, Ok(()));
            }
            Arc::make_mut(&mut next.hooks).retain(|h| h.id != id);
        }
        Action::ClearRuns(ids) => {
            next.runs
                .retain(|run| run.conclusion.is_none() || !ids.contains(&run.id));
            if next.runs.len() == state.runs.len() {
                return Reduction::unchanged(next, Ok(()));
            }
        }
        Action::Run(run) => {
            if let Some(existing) = next.runs.iter_mut().find(|r| r.id == run.id) {
                durable_changed = !same_durable_run(existing, &run);
                *existing = run;
            } else {
                next.runs.push(run);
            }
            while next.runs.len() > MAX_RUNS {
                let Some(index) = next.runs.iter().position(|r| r.conclusion.is_some()) else {
                    break;
                };
                next.runs.remove(index);
            }
        }
    }
    next.durable_revision = state.durable_revision + u64::from(durable_changed);
    next.catalog_revision = state.catalog_revision + u64::from(catalog_changed);
    Reduction::changed(next, Ok(()))
}

#[derive(Clone)]
pub(crate) struct HookStateFacade {
    store: PersistentStore<HookState, Action, Result<(), String>>,
    path: PathBuf,
}

impl HookStateFacade {
    pub(super) fn new(path: PathBuf) -> std::io::Result<Self> {
        let destination = path.clone();
        let persisted_revision = Mutex::new(0);
        let store = PersistentStore::new_deferred(
            HookState::default(),
            reduce,
            "hook-state",
            move |state| {
                let mut persisted = persisted_revision.lock().unwrap();
                if *persisted == state.durable_revision {
                    return Ok(());
                }
                super::benchmark::record_write();
                let data = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
                usher_fs::atomic_write_private(&destination, &data).map_err(|e| e.to_string())?;
                *persisted = state.durable_revision;
                Ok(())
            },
            |_, error| log::error!("[Hooks] event=persistence.failed reason={error}"),
        )?;
        Ok(Self { store, path })
    }

    /// Called on a worker before accepting commands. A damaged file is preserved.
    pub(super) fn hydrate(&self) -> Result<(), String> {
        let loaded: Result<HookState, String> = (|| {
            if !self.path.exists() {
                return Ok(HookState::default());
            }
            if std::fs::metadata(&self.path)
                .map_err(|e| e.to_string())?
                .len()
                > 8 * 1024 * 1024
            {
                return Err("Hook configuration exceeds 8 MiB; file was preserved.".into());
            }
            let mut state: HookState =
                serde_json::from_slice(&std::fs::read(&self.path).map_err(|e| e.to_string())?)
                    .map_err(|e| {
                        format!("Cannot read Hook configuration: {e}. File was preserved.")
                    })?;
            if state.hooks.len() > 32 || state.runs.len() > MAX_RUNS {
                return Err("Hook configuration exceeds retention limits.".into());
            }
            let mut ids = std::collections::HashSet::new();
            for hook in Arc::make_mut(&mut state.hooks) {
                if hook.event == LEGACY_PR_REVIEW_SUBMITTED {
                    hook.event = PR_REVIEW_APPROVED.into();
                }
                hook.validate()?;
                if !ids.insert(hook.id.clone()) {
                    return Err("Duplicate Hook ID in configuration.".into());
                }
            }
            ids.clear();
            for run in &mut state.runs {
                if !valid_id(&run.id) || !ids.insert(run.id.clone()) {
                    return Err("Invalid or duplicate Hook run ID.".into());
                }
                if run.conclusion.is_none() {
                    run.conclusion = Some(HookConclusion::Interrupted);
                    run.finished_at_ms = Some(now_ms());
                    run.detail =
                        "Application stopped before this Hook finished; it was not replayed."
                            .into();
                }
            }
            Ok(state)
        })();
        match loaded {
            Ok(state) => {
                self.dispatch(Action::Hydrate(state))?;
                self.store.enable_persistence();
                Ok(())
            }
            Err(error) => {
                self.dispatch(Action::LoadFailed(error.clone()))?;
                Err(error)
            }
        }
    }
    pub(crate) fn snapshot(&self) -> StoreSnapshot<HookState> {
        self.store.snapshot()
    }
    pub(crate) fn subscribe(
        &self,
        listener: Arc<dyn Fn(StoreSnapshot<HookState>) + Send + Sync>,
    ) -> Subscription {
        self.store.subscribe(listener)
    }
    pub(super) fn dispatch(&self, action: Action) -> Result<(), String> {
        self.store.dispatch(action).output
    }
    pub(super) fn flush(&self) -> Result<(), String> {
        self.store.flush()
    }
    pub(super) fn shutdown(&self) {
        self.store.shutdown();
    }
}
