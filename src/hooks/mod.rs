//! Application-wide, event-driven user scripts. Feature modules publish domain
//! events; this service owns execution and history independently of UI routes.
mod benchmark;
mod bridge;
mod errors;
mod presentation;
#[cfg(test)]
mod presentation_tests;
mod process_tree;
mod projection;
mod replay;
#[cfg(test)]
mod replay_tests;
mod runner;
mod state;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod tests;
mod types;

pub(crate) use bridge::{
    apply_settings, bind_settings, cancel_replay, handle_dialog_key, open_target, setup,
    sync_job_language, sync_language,
};
pub(crate) use errors::{HookFailureKind, HookValidationError};
pub(crate) use presentation::notification_copy;
pub(crate) use state::{HookState, HookStateFacade};
pub(crate) use types::*;

use state::Action;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::{Semaphore, broadcast, watch};

static SERVICE: OnceLock<Arc<HookService>> = OnceLock::new();

pub(crate) fn current() -> Option<Arc<HookService>> {
    SERVICE.get().cloned()
}

/// Called from a feature's background completion path, before UI delivery.
pub(crate) async fn emit(event: HookTrigger) -> Result<DispatchReceipt, String> {
    let service = current().ok_or("Hook service is not initialized")?;
    let receipt = service.emit(event).await?;
    log::info!(
        "[Hooks] event=dispatch.completed trigger={} accepted={} skipped={}",
        receipt.event_id,
        receipt.runs.len(),
        receipt.skipped.len()
    );
    Ok(receipt)
}

type Observer = Arc<dyn Fn(&HookEvent, &HookRun) + Send + Sync>;
type ProjectHookSelector = Arc<dyn Fn(&str, &str) -> bool + Send + Sync>;

pub(crate) struct HookService {
    pub state: HookStateFacade,
    directory: PathBuf,
    admission: tokio::sync::Mutex<()>,
    configuration: Mutex<()>,
    active: Mutex<HashMap<String, watch::Sender<bool>>>,
    projects: Mutex<HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>>,
    slots: Semaphore,
    closed: AtomicBool,
    events: broadcast::Sender<HookEvent>,
    observer: Mutex<Option<Observer>>,
    project_hook_selected: ProjectHookSelector,
}

impl HookService {
    fn new(path: PathBuf, directory: PathBuf) -> std::io::Result<Arc<Self>> {
        Self::with_project_selector(path, directory, Arc::new(crate::project::hook_selected))
    }

    fn with_project_selector(
        path: PathBuf,
        directory: PathBuf,
        project_hook_selected: ProjectHookSelector,
    ) -> std::io::Result<Arc<Self>> {
        let (events, _) = broadcast::channel(256);
        Ok(Arc::new(Self {
            state: HookStateFacade::new(path)?,
            directory,
            admission: tokio::sync::Mutex::new(()),
            configuration: Mutex::new(()),
            active: Mutex::default(),
            projects: Mutex::default(),
            slots: Semaphore::new(2),
            closed: AtomicBool::new(false),
            events,
            observer: Mutex::default(),
            project_hook_selected,
        }))
    }

    /// Registration is code-owned; scripts cannot register or emit lifecycle events.
    pub(crate) fn register_event(&self, event: EventDefinition) -> Result<(), String> {
        let _configuration = self.configuration.lock().unwrap();
        if self.closed.load(Ordering::Acquire) {
            return Err("Hook service is shutting down.".into());
        }
        if !valid_id(&event.category)
            || event.name.starts_with("hook.")
            || event.name.starts_with("execution.")
            || event.name.len() > 128
            || !event
                .name
                .rsplit_once(".v")
                .is_some_and(|(domain, version)| {
                    !domain.is_empty() && version.parse::<u32>().is_ok_and(|v| v > 0)
                })
        {
            return Err(
                "Hook source events require a valid category and domain name ending in .v<number>."
                    .into(),
            );
        }
        self.state.dispatch(Action::Register(event))
    }

    pub(crate) async fn emit(
        self: &Arc<Self>,
        event: HookTrigger,
    ) -> Result<DispatchReceipt, String> {
        let _admission = self.admission.lock().await;
        self.ensure_ready()?;
        if event.test || event.source_module == "hooks" {
            return Err(
                "Business dispatch cannot impersonate test or Hook lifecycle events.".into(),
            );
        }
        if serde_json::to_vec(&event).map_err(|e| e.to_string())?.len() > 256 * 1024 {
            return Err("Hook event context exceeds 256 KiB.".into());
        }
        let state = self.state.snapshot();
        let definition = state
            .state()
            .events
            .iter()
            .find(|e| e.name == event.name)
            .ok_or_else(|| format!("Unregistered Hook event: {}", event.name))?;
        if definition.project_scoped && event.project.as_deref().is_none_or(str::is_empty) {
            return Err("Project-scoped Hook events require a project ID.".into());
        }
        let mut receipt = DispatchReceipt {
            event_id: event.event_id.clone(),
            runs: Vec::new(),
            skipped: Vec::new(),
        };
        if !state.state().enabled {
            receipt.skipped.push("Hooks are disabled".into());
            return Ok(receipt);
        }
        for hook in state
            .state()
            .hooks
            .iter()
            .filter(|hook| hook.matches(&event))
        {
            if !self.project_allows(&event, &hook.id) {
                receipt
                    .skipped
                    .push(format!("{}: not selected by project", hook.name));
                continue;
            }
            if state
                .state()
                .runs
                .iter()
                .any(|r| !r.test && r.event_id == event.event_id && r.hook_id == hook.id)
            {
                receipt
                    .skipped
                    .push(format!("{}: duplicate event", hook.name));
                continue;
            }
            match self.admit(hook.clone(), event.clone()).await {
                Ok(run) => receipt.runs.push(run),
                Err(error) => receipt.skipped.push(format!("{}: {error}", hook.name)),
            }
        }
        Ok(receipt)
    }

    fn project_allows(&self, event: &HookTrigger, hook_id: &str) -> bool {
        event
            .project
            .as_deref()
            .is_none_or(|project| (self.project_hook_selected)(project, hook_id))
    }

    pub(crate) async fn test(
        self: &Arc<Self>,
        hook: HookDefinition,
        cwd: Option<PathBuf>,
    ) -> Result<String, String> {
        let _admission = self.admission.lock().await;
        self.ensure_ready()?;
        hook.validate()?;
        if self
            .state
            .snapshot()
            .state()
            .runs
            .iter()
            .any(|r| r.test && r.hook_id == hook.id && r.conclusion.is_none())
        {
            return Err("A test for this Hook is already running.".into());
        }
        let snapshot = self.state.snapshot();
        let definition = snapshot
            .state()
            .events
            .iter()
            .find(|e| e.name == hook.event)
            .ok_or("Hook event is not registered")?;
        let mut event = HookTrigger::new(&hook.event, "settings.test", definition.sample.clone());
        event.test = true;
        event.project = (!hook.project.is_empty()).then(|| hook.project.clone());
        event.cwd = cwd;
        event.outcome = if hook.outcome.is_empty() {
            "success".into()
        } else {
            hook.outcome.clone()
        };
        self.admit(hook, event).await
    }

    fn ensure_ready(&self) -> Result<(), String> {
        if self.closed.load(Ordering::Acquire) {
            return Err("Hook service is shutting down.".into());
        }
        let snapshot = self.state.snapshot();
        if !snapshot.state().ready {
            return Err(if snapshot.state().error.is_empty() {
                "Hook configuration is still loading.".into()
            } else {
                snapshot.state().error.clone()
            });
        }
        Ok(())
    }

    async fn admit(
        self: &Arc<Self>,
        hook: HookDefinition,
        event: HookTrigger,
    ) -> Result<String, String> {
        if self.active.lock().unwrap().len() >= 32 {
            return Err("Hook queue is full (32 runs).".into());
        }
        let run = HookRun::queued(&hook, &event);
        let id = run.id.clone();
        let (cancel, receiver) = watch::channel(false);
        self.active.lock().unwrap().insert(id.clone(), cancel);
        let previous = self.state.snapshot();
        if let Err(error) = self.state.dispatch(Action::Run(run.clone())) {
            self.active.lock().unwrap().remove(&id);
            return Err(error);
        }
        let current = self.state.snapshot();
        for old in previous
            .state()
            .runs
            .iter()
            .filter(|old| !current.state().runs.iter().any(|r| r.id == old.id))
        {
            let _ = tokio::fs::remove_file(self.directory.join(format!("{}.jsonl", old.id))).await;
        }
        let service = self.clone();
        let active_id = id.clone();
        crate::app_runtime::shared_runtime().spawn(async move {
            runner::run(service.clone(), hook, event, run, receiver).await;
            service.active.lock().unwrap().remove(&active_id);
        });
        Ok(id)
    }

    pub(crate) fn cancel(&self, run_id: &str) {
        if let Some(sender) = self.active.lock().unwrap().get(run_id) {
            let _ = sender.send(true);
        }
    }

    pub(crate) async fn clear_history(self: &Arc<Self>) -> Result<(), String> {
        let _admission = self.admission.lock().await;
        self.ensure_ready()?;
        // A terminal snapshot may be published just before its runner closes
        // the journal. Exclude those still-owned runs as well as queued work.
        let ids: Vec<_> = {
            let active = self.active.lock().unwrap();
            self.state
                .snapshot()
                .state()
                .runs
                .iter()
                .filter(|run| run.conclusion.is_some() && !active.contains_key(&run.id))
                .map(|run| run.id.clone())
                .collect()
        };
        let service = self.clone();
        let removed = ids.clone();
        tokio::task::spawn_blocking(move || {
            let _configuration = service.configuration.lock().unwrap();
            service.state.dispatch(Action::ClearRuns(removed))?;
            service.state.flush()
        })
        .await
        .map_err(|error| error.to_string())??;
        for id in ids {
            if let Err(error) =
                tokio::fs::remove_file(self.directory.join(format!("{id}.jsonl"))).await
            {
                if error.kind() != std::io::ErrorKind::NotFound {
                    return Err(format!(
                        "History cleared, but cannot remove journal {id}: {error}"
                    ));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn subscribe_events(&self) -> broadcast::Receiver<HookEvent> {
        self.events.subscribe()
    }

    /// Lagging subscribers replay by sequence from the bounded durable journal.
    pub(crate) async fn events_after(&self, id: &str, seq: u64) -> Result<Vec<HookEvent>, String> {
        if !self
            .state
            .snapshot()
            .state()
            .runs
            .iter()
            .any(|r| r.id == id)
        {
            return Err("Hook run was not found or has expired.".into());
        }
        let path = self.directory.join(format!("{id}.jsonl"));
        let mut events = Vec::new();
        // A page is bounded in both record count and decoded input bytes. Resume
        // with the last returned sequence; consumers need not retain earlier pages.
        let mut bytes = 0;
        replay::scan(
            &path,
            seq,
            u64::MAX,
            || false,
            |event| {
                bytes += serde_json::to_vec(&event).map_or(MAX_RECORD_BYTES, |record| record.len());
                events.push(event);
                events.len() < 256 && bytes < 256 * 1024
            },
        )
        .await?;
        Ok(events)
    }

    pub(crate) async fn wait(&self, id: &str) -> Result<HookRun, String> {
        let mut events = self.subscribe_events();
        loop {
            let run = self
                .state
                .snapshot()
                .state()
                .runs
                .iter()
                .find(|r| r.id == id)
                .cloned()
                .ok_or("Hook run was not found or has expired")?;
            if run.conclusion.is_some() {
                return Ok(run);
            }
            // Snapshots are authoritative even when broadcast output has lagged.
            tokio::select! { _ = events.recv() => {}, _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {} }
        }
    }

    pub(crate) fn save(&self, hook: HookDefinition) -> Result<(), String> {
        let _configuration = self.configuration.lock().unwrap();
        self.ensure_ready()?;
        self.state.dispatch(Action::Save(hook))?;
        self.state.flush()
    }
    pub(crate) fn delete(&self, id: String) -> Result<(), String> {
        let _configuration = self.configuration.lock().unwrap();
        self.ensure_ready()?;
        self.state.dispatch(Action::Delete(id))?;
        self.state.flush()
    }
    pub(crate) fn enable(&self, enabled: bool) -> Result<(), String> {
        let _configuration = self.configuration.lock().unwrap();
        self.ensure_ready()?;
        self.state.dispatch(Action::Enable(enabled))?;
        self.state.flush()
    }
    pub(crate) fn set_hook_enabled(&self, id: String, enabled: bool) -> Result<(), String> {
        let _configuration = self.configuration.lock().unwrap();
        self.ensure_ready()?;
        self.state
            .dispatch(Action::SetHookEnabled { id, enabled })?;
        self.state.flush()
    }

    fn publish(&self, event: HookEvent, run: &HookRun) {
        if event.kind != "output" {
            log::info!(
                "[Hooks] event={} run={} hook={} trigger={} seq={}",
                event.kind,
                event.run_id,
                event.hook_id,
                event.event_id,
                event.seq
            );
            if let Some(observer) = self.observer.lock().unwrap().as_ref().cloned() {
                observer(&event, run);
            }
        }
        let _ = self.events.send(event);
    }

    fn project_lock(&self, project: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut projects = self.projects.lock().unwrap();
        projects.retain(|_, value| value.strong_count() > 0);
        if let Some(lock) = projects.get(project).and_then(std::sync::Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        projects.insert(project.into(), Arc::downgrade(&lock));
        lock
    }

    async fn shutdown(self: &Arc<Self>) {
        {
            let _admission = self.admission.lock().await;
            self.closed.store(true, Ordering::Release);
            for cancel in self.active.lock().unwrap().values() {
                let _ = cancel.send(true);
            }
        }
        while !self.active.lock().unwrap().is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let service = self.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let _configuration = service.configuration.lock().unwrap();
            service.state.shutdown();
        })
        .await;
    }
}
