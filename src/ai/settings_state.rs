//! Typed, persisted state for committed AI Agent configuration.

use std::sync::OnceLock;

use usher_state_store::{PersistentStore, Reduction, StoreSnapshot};

use crate::settings::{self, AiAgentSettings};

const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
const DEEPSEEK_MODEL: &str = "deepseek-v4-flash";
const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const OPENAI_MODEL: &str = "gpt-4o-mini";

#[derive(Debug)]
enum AiAgentSettingsAction {
    Replace(AiAgentSettings),
}

type AiAgentStore = PersistentStore<AiAgentSettings, AiAgentSettingsAction, ()>;

#[derive(Clone)]
pub(crate) struct AiAgentSettingsFacade {
    store: AiAgentStore,
}

static REGISTERED_STATE: OnceLock<AiAgentSettingsFacade> = OnceLock::new();

impl AiAgentSettingsFacade {
    pub(crate) fn open(initial: AiAgentSettings) -> std::io::Result<Self> {
        let store = PersistentStore::new(
            settings::normalize_ai_agent_settings(initial),
            reduce_ai_agent_settings,
            "ai-agent-settings",
            |state| {
                settings::save_ai_agent_settings(
                    &state.provider,
                    &state.base_url,
                    &state.api_key,
                    &state.model,
                    &state.reply_language,
                )
            },
            |_, error| log::error!("[AI Agent][Settings] Persistence failed: {error}"),
        )?;
        let facade = Self { store };
        let _ = REGISTERED_STATE.set(facade.clone());
        Ok(facade)
    }

    #[cfg(test)]
    pub(crate) fn memory(initial: AiAgentSettings) -> Self {
        Self {
            store: PersistentStore::new(
                settings::normalize_ai_agent_settings(initial),
                reduce_ai_agent_settings,
                "ai-agent-settings-test",
                |_| Ok(()),
                |_, error| panic!("in-memory AI Agent settings persistence failed: {error}"),
            )
            .expect("failed to start in-memory AI Agent settings state"),
        }
    }

    pub(crate) fn snapshot(&self) -> StoreSnapshot<AiAgentSettings> {
        self.store.snapshot()
    }

    pub(crate) fn replace(&self, settings: AiAgentSettings) -> StoreSnapshot<AiAgentSettings> {
        self.dispatch(AiAgentSettingsAction::Replace(settings))
    }

    pub(crate) fn flush(&self) -> Result<(), String> {
        self.store.flush()
    }

    pub(crate) fn shutdown(&self) {
        self.store.shutdown();
    }

    fn dispatch(&self, action: AiAgentSettingsAction) -> StoreSnapshot<AiAgentSettings> {
        self.store.dispatch(action).snapshot
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
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn provider_selection_updates_the_complete_preset_atomically() {
        let initial = AiAgentSettings {
            provider: "deepseek".into(),
            base_url: DEEPSEEK_BASE_URL.into(),
            api_key: "secret".into(),
            model: DEEPSEEK_MODEL.into(),
            reply_language: "en".into(),
        };
        let draft = apply_provider_preset(&initial, "openai");

        assert_eq!(draft.provider, "openai");
        assert_eq!(draft.base_url, OPENAI_BASE_URL);
        assert_eq!(draft.model, OPENAI_MODEL);
        assert_eq!(draft.api_key, "secret");
        assert_eq!(draft.reply_language, "en");
    }

    #[test]
    fn changed_commits_persist_the_complete_snapshot_on_the_worker() {
        let persisted = Arc::new(Mutex::new(Vec::new()));
        let persisted_for_worker = persisted.clone();
        let store = PersistentStore::new(
            AiAgentSettings::default(),
            reduce_ai_agent_settings,
            "ai-agent-settings-persistence-test",
            move |state| {
                persisted_for_worker.lock().unwrap().push(state.clone());
                Ok(())
            },
            |_, error| panic!("AI Agent persistence failed: {error}"),
        )
        .unwrap();

        store.dispatch(AiAgentSettingsAction::Replace(apply_provider_preset(
            &AiAgentSettings::default(),
            "openai",
        )));
        store.flush().unwrap();
        store.shutdown();

        let persisted = persisted.lock().unwrap();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].provider, "openai");
        assert_eq!(persisted[0].base_url, OPENAI_BASE_URL);
        assert_eq!(persisted[0].model, OPENAI_MODEL);
        assert_eq!(persisted[0].reply_language, usher_i18n::AUTO_LANGUAGE);
    }
}
