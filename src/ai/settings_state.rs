//! Typed AI Agent configuration with bounded, ordered worker-owned saves.

use std::sync::{Arc, Condvar, Mutex, OnceLock, mpsc};

use usher_state_store::{PersistentStore, Reduction, StoreSnapshot};

use crate::settings::{self, AiAgentSettings};

const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
const DEEPSEEK_MODEL: &str = "deepseek-v4-flash";
const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const OPENAI_MODEL: &str = "gpt-4o-mini";
const SAVE_CAPACITY: usize = 8;

#[derive(Debug)]
enum AiAgentSettingsAction {
    Replace(AiAgentSettings),
}

type AiAgentStore = PersistentStore<AiAgentSettings, AiAgentSettingsAction, ()>;

#[derive(Clone)]
pub(crate) struct AiAgentSettingsFacade {
    store: AiAgentStore,
    commands: Arc<Mutex<SaveAdmission>>,
    stopped: Arc<(Mutex<Option<Result<(), String>>>, Condvar)>,
}

struct SaveAdmission {
    sender: Option<tokio::sync::mpsc::Sender<SaveCommand>>,
    pending: Vec<(u64, AiAgentSettings)>,
    next_id: u64,
}

struct SaveCommand {
    id: u64,
    draft: AiAgentSettings,
    completion: mpsc::Sender<AiAgentSaveOutcome>,
}

pub(crate) struct AiAgentSaveReceipt {
    pub(crate) id: u64,
    result: mpsc::Receiver<AiAgentSaveOutcome>,
}

pub(crate) struct AiAgentSaveOutcome {
    pub(crate) snapshot: StoreSnapshot<AiAgentSettings>,
    pub(crate) persisted: Result<(), String>,
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
    pub(crate) fn open(initial: AiAgentSettings) -> std::io::Result<Self> {
        let facade = Self::with_persistence(initial, reduce_ai_agent_settings, |state| {
            settings::save_ai_agent_settings(
                &state.provider,
                &state.base_url,
                &state.api_key,
                &state.model,
                &state.reply_language,
            )
        })?;
        let _ = REGISTERED_STATE.set(facade.clone());
        Ok(facade)
    }

    #[cfg(test)]
    pub(crate) fn memory(initial: AiAgentSettings) -> Self {
        Self::with_persistence(initial, reduce_ai_agent_settings, |_| Ok(()))
            .expect("failed to start in-memory AI Agent settings state")
    }

    #[cfg(test)]
    pub(super) fn with_test_persistence(
        initial: AiAgentSettings,
        persist: impl Fn(&AiAgentSettings) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        Self::with_persistence(initial, reduce_ai_agent_settings, persist).unwrap()
    }

    fn with_persistence(
        initial: AiAgentSettings,
        reducer: impl Fn(&AiAgentSettings, AiAgentSettingsAction) -> Reduction<AiAgentSettings, ()>
        + Send
        + Sync
        + 'static,
        persist: impl Fn(&AiAgentSettings) -> Result<(), String> + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let store = PersistentStore::new(
            settings::normalize_ai_agent_settings(initial),
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
                    let commit = save_store.dispatch(AiAgentSettingsAction::Replace(command.draft));
                    let persisted = if commit.changed {
                        save_store.flush()
                    } else {
                        save_store.retry_failed().and_then(|_| save_store.flush())
                    };
                    AiAgentSaveOutcome {
                        snapshot: commit.snapshot,
                        persisted,
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

    pub(crate) fn snapshot(&self) -> StoreSnapshot<AiAgentSettings> {
        self.store.snapshot()
    }

    /// Capture the complete draft and FIFO identity synchronously. Admission is
    /// bounded/nonblocking; reduction, retries and durable barriers run off UI.
    /// An identical in-flight draft shares the existing operation (no new Toast).
    pub(crate) fn save_draft(
        &self,
        draft: AiAgentSettings,
    ) -> Result<Option<AiAgentSaveReceipt>, String> {
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
            .is_some_and(|(_, pending)| *pending == draft)
        {
            return Ok(None);
        }
        if admission.pending.len() >= SAVE_CAPACITY {
            return Err("AI Agent settings save queue is full".into());
        }
        let id = admission.next_id.wrapping_add(1);
        let (completion, result) = mpsc::channel();
        admission
            .sender
            .as_ref()
            .unwrap()
            .try_send(SaveCommand {
                id,
                draft: draft.clone(),
                completion,
            })
            .map_err(|_| "AI Agent settings command worker unavailable")?;
        admission.next_id = id;
        admission.pending.push((id, draft));
        Ok(Some(AiAgentSaveReceipt { id, result }))
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

pub(crate) fn apply_provider_preset(current: &AiAgentSettings, provider: &str) -> AiAgentSettings {
    let mut next = current.clone();
    match provider {
        "deepseek" => {
            next.provider = "deepseek".to_string();
            next.base_url = DEEPSEEK_BASE_URL.to_string();
            next.model = DEEPSEEK_MODEL.to_string();
        }
        "openai" => {
            next.provider = "openai".to_string();
            next.base_url = OPENAI_BASE_URL.to_string();
            next.model = OPENAI_MODEL.to_string();
        }
        _ => next.provider = "custom".to_string(),
    }
    next
}

pub(crate) fn current() -> AiAgentSettings {
    REGISTERED_STATE
        .get()
        .map(|state| state.snapshot().state().clone())
        .unwrap_or_else(|| settings::load_settings().ai.agent)
}

fn reduce_ai_agent_settings(
    current: &AiAgentSettings,
    action: AiAgentSettingsAction,
) -> Reduction<AiAgentSettings, ()> {
    let next = match action {
        AiAgentSettingsAction::Replace(settings) => settings::normalize_ai_agent_settings(settings),
    };
    if next == *current {
        Reduction::unchanged(next, ())
    } else {
        Reduction::changed(next, ())
    }
}

#[cfg(test)]
#[path = "settings_state_tests.rs"]
mod tests;
