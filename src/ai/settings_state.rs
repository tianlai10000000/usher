//! Typed AI Agent configuration with bounded, ordered worker-owned saves.

use std::sync::{Arc, Condvar, Mutex, OnceLock, mpsc};

use usher_state_store::{PersistentStore, Reduction, StoreSnapshot};

use crate::settings::{self, AiAgentConfiguration, AiAgentProfile};
mod actions;
use actions::{AiAgentSettingsAction, reduce_ai_agent_settings};

const SAVE_CAPACITY: usize = 8;

type AiAgentStore =
    PersistentStore<AiAgentConfiguration, AiAgentSettingsAction, Result<(), String>>;

#[derive(Clone)]
pub(crate) struct AiAgentSettingsFacade {
    store: AiAgentStore,
    commands: Arc<Mutex<SaveAdmission>>,
    stopped: Arc<(Mutex<Option<Result<(), String>>>, Condvar)>,
}

struct SaveAdmission {
    sender: Option<tokio::sync::mpsc::Sender<SaveCommand>>,
    pending: Vec<(u64, AiAgentSettingsAction)>,
    next_id: u64,
}

struct SaveCommand {
    id: u64,
    action: AiAgentSettingsAction,
    completion: mpsc::Sender<AiAgentSaveOutcome>,
}

pub(crate) struct AiAgentSaveReceipt {
    pub(crate) id: u64,
    pub(crate) profile_id: String,
    result: mpsc::Receiver<AiAgentSaveOutcome>,
}

pub(crate) struct AiAgentSaveOutcome {
    pub(crate) snapshot: StoreSnapshot<AiAgentConfiguration>,
    pub(crate) persisted: Result<(), String>,
    pub(crate) applied: Result<(), String>,
}

impl AiAgentSaveReceipt {
    /// A receipt is awaited only by a worker, never by a Slint callback.
    pub(crate) fn wait(self) -> Result<AiAgentSaveOutcome, String> {
        self.result
            .recv()
            .map_err(|_| "AI Agent settings command worker stopped".into())
    }
}

static REGISTERED_STATE: OnceLock<AiAgentSettingsFacade> = OnceLock::new();

impl AiAgentSettingsFacade {
    pub(crate) fn open(initial: AiAgentConfiguration) -> std::io::Result<Self> {
        let facade = Self::with_persistence(initial, reduce_ai_agent_settings, |state| {
            settings::save_ai_agent_settings(state)
        })?;
        let _ = REGISTERED_STATE.set(facade.clone());
        Ok(facade)
    }

    #[cfg(test)]
    pub(crate) fn memory(initial: impl Into<AiAgentConfiguration>) -> Self {
        Self::with_persistence(initial.into(), reduce_ai_agent_settings, |_| Ok(()))
            .expect("failed to start in-memory AI Agent settings state")
    }

    #[cfg(test)]
    pub(super) fn with_test_persistence(
        initial: impl Into<AiAgentConfiguration>,
        persist: impl Fn(&AiAgentConfiguration) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        Self::with_persistence(initial.into(), reduce_ai_agent_settings, persist).unwrap()
    }

    fn with_persistence(
        initial: AiAgentConfiguration,
        reducer: impl Fn(
            &AiAgentConfiguration,
            AiAgentSettingsAction,
        ) -> Reduction<AiAgentConfiguration, Result<(), String>>
        + Send
        + Sync
        + 'static,
        persist: impl Fn(&AiAgentConfiguration) -> Result<(), String> + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let initial = initial.normalized();
        initial.validate().map_err(std::io::Error::other)?;
        let store = PersistentStore::new(
            initial,
            reducer,
            "ai-agent-settings",
            persist,
            |_, error| log::error!("[AI Agent][Settings] Persistence failed: {error}"),
        )?;
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<SaveCommand>(SAVE_CAPACITY);
        let commands = Arc::new(Mutex::new(SaveAdmission {
            sender: Some(sender),
            pending: Vec::new(),
            next_id: 0,
        }));
        let stopped = Arc::new((Mutex::new(None), Condvar::new()));
        let worker_commands = Arc::downgrade(&commands);
        let worker_stopped = stopped.clone();
        let worker_store = store.clone();
        crate::app_runtime::shared_runtime().spawn(async move {
            while let Some(command) = receiver.recv().await {
                let id = command.id;
                let save_store = worker_store.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let commit = save_store.dispatch(command.action);
                    let persisted = if let Err(error) = &commit.output {
                        Err(error.clone())
                    } else if commit.changed {
                        save_store.flush()
                    } else {
                        save_store.retry_failed().and_then(|_| save_store.flush())
                    };
                    AiAgentSaveOutcome {
                        snapshot: commit.snapshot,
                        persisted,
                        applied: commit.output,
                    }
                })
                .await;
                if let Some(commands) = worker_commands.upgrade() {
                    commands
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .pending
                        .retain(|(pending_id, _)| *pending_id != id);
                }
                match result {
                    Ok(outcome) => {
                        let _ = command.completion.send(outcome);
                    }
                    Err(error) => {
                        log::error!("[AI Agent][Settings] Save worker failed: {error}");
                    }
                }
            }
            let result = tokio::task::spawn_blocking(move || {
                let result = worker_store.flush();
                worker_store.shutdown();
                result
            })
            .await
            .unwrap_or_else(|error| Err(format!("AI Agent settings shutdown failed: {error}")));
            *worker_stopped.0.lock().unwrap() = Some(result);
            worker_stopped.1.notify_all();
        });
        Ok(Self {
            store,
            commands,
            stopped,
        })
    }

    pub(crate) fn snapshot(&self) -> StoreSnapshot<AiAgentConfiguration> {
        self.store.snapshot()
    }

    /// Create/Update/Delete/Activate share one ordered nonblocking admission.
    pub(crate) fn create_profile(
        &self,
        mut profile: AiAgentProfile,
    ) -> Result<Option<AiAgentSaveReceipt>, String> {
        if profile.id.trim().is_empty() {
            profile.id = AiAgentProfile::new_id();
        }
        self.submit(AiAgentSettingsAction::Create(profile))
    }

    pub(crate) fn update_profile(
        &self,
        profile: AiAgentProfile,
    ) -> Result<Option<AiAgentSaveReceipt>, String> {
        self.submit(AiAgentSettingsAction::Update(profile))
    }

    pub(crate) fn delete_profile(&self, id: &str) -> Result<Option<AiAgentSaveReceipt>, String> {
        self.submit(AiAgentSettingsAction::Delete(id.to_string()))
    }

    pub(crate) fn activate_profile(&self, id: &str) -> Result<Option<AiAgentSaveReceipt>, String> {
        self.submit(AiAgentSettingsAction::Activate(id.to_string()))
    }

    fn submit(&self, action: AiAgentSettingsAction) -> Result<Option<AiAgentSaveReceipt>, String> {
        let mut admission = self
            .commands
            .lock()
            .map_err(|_| "AI settings admission poisoned")?;
        if admission.sender.is_none() {
            return Err("AI Agent settings are closed".into());
        }
        if admission
            .pending
            .last()
            .is_some_and(|(_, pending)| *pending == action)
        {
            return Ok(None);
        }
        if admission.pending.len() >= SAVE_CAPACITY {
            return Err("AI Agent settings save queue is full".into());
        }
        let id = admission.next_id.wrapping_add(1);
        let profile_id = action.target_id().to_string();
        let (completion, result) = mpsc::channel();
        admission
            .sender
            .as_ref()
            .unwrap()
            .try_send(SaveCommand {
                id,
                action: action.clone(),
                completion,
            })
            .map_err(|_| "AI Agent settings command worker unavailable")?;
        admission.next_id = id;
        admission.pending.push((id, action));
        Ok(Some(AiAgentSaveReceipt {
            id,
            profile_id,
            result,
        }))
    }

    /// Close admission before draining every accepted save. The caller must be
    /// a worker; the app's async drain owns this barrier before UI teardown.
    pub(crate) fn shutdown_checked(&self) -> Result<(), String> {
        self.commands
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .sender
            .take();
        let result = self
            .stopped
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.stopped
            .1
            .wait_while(result, |result| result.is_none())
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .unwrap()
            .clone()
    }

    pub(crate) async fn shutdown_async(&self) -> Result<(), String> {
        let facade = self.clone();
        tokio::task::spawn_blocking(move || facade.shutdown_checked())
            .await
            .map_err(|error| format!("AI Agent settings drain failed: {error}"))?
    }

    pub(crate) fn shutdown(&self) {
        if let Err(error) = self.shutdown_checked() {
            log::error!("[AI Agent][Settings] Shutdown flush failed: {error}");
        }
    }
}

pub(crate) fn current_profile() -> AiAgentProfile {
    match REGISTERED_STATE.get() {
        Some(state) => state
            .snapshot()
            .state()
            .active_profile()
            .cloned()
            .unwrap_or_default(),
        None => settings::load_settings()
            .ai
            .agent
            .active_profile()
            .cloned()
            .unwrap_or_default(),
    }
}

#[cfg(test)]
#[path = "settings_state_tests.rs"]
mod tests;
