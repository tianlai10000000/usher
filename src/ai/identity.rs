//! Raw Agent attribution and locale-aware presentation shared by AI consumers.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AgentIdentity {
    pub(crate) name: String,
    pub(crate) provider: String,
    pub(crate) model: String,
}

impl AgentIdentity {
    pub(crate) fn from_profile(profile: &crate::settings::AiAgentProfile) -> Self {
        Self {
            name: profile.name.clone(),
            provider: profile.connection.provider.clone(),
            model: profile.connection.model.clone(),
        }
    }

    pub(crate) fn label(&self) -> String {
        display(&self.name, &self.provider, &self.model)
    }
}

pub(crate) fn display(name: &str, provider: &str, model: &str) -> String {
    display_for(usher_i18n::active_locale().id, name, provider, model)
}

fn display_for(locale: &str, name: &str, provider: &str, model: &str) -> String {
    let name = if name.trim().is_empty() {
        match provider {
            "deepseek" => "DeepSeek".to_string(),
            "openai" => "OpenAI".to_string(),
            "custom" => usher_i18n::translate_for(locale, "common.custom").into_owned(),
            _ => usher_i18n::translate_for(locale, "settings.ai_agent.title").into_owned(),
        }
    } else {
        name.trim().to_string()
    };
    if model.trim().is_empty() {
        return name;
    }
    usher_i18n::translate_for_with(
        locale,
        "ai_agent.identity",
        &[("name", name.as_str()), ("model", model.trim())],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_models_remain_literal_in_both_locales() {
        for locale in ["en", "zh-CN"] {
            assert_eq!(
                display_for(locale, "My 自定义 Agent", "custom", "model-v2"),
                "My 自定义 Agent · model-v2"
            );
            assert_eq!(
                display_for(locale, "", "deepseek", "deepseek-chat"),
                "DeepSeek · deepseek-chat"
            );
        }
    }

    #[test]
    fn unnamed_custom_agents_localize_only_at_presentation() {
        assert_eq!(
            display_for("en", "", "custom", "fixture"),
            "Custom · fixture"
        );
        assert_eq!(
            display_for("zh-CN", "", "custom", "fixture"),
            "自定义 · fixture"
        );
        assert_eq!(display_for("en", "", "", "legacy"), "AI Agent · legacy");
    }
}
