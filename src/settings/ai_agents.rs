//! Named Agent registry, stable identities, custom headers and schema migration.

use super::{AiAgentSettings, normalize_ai_agent_settings};
use serde::{Deserialize, Deserializer, Serialize};

pub const MAX_AI_AGENT_HEADERS: usize = 32;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiAgentHeader {
    #[serde(default = "header_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub value: String,
}
fn header_enabled() -> bool {
    true
}
impl Default for AiAgentHeader {
    fn default() -> Self {
        Self {
            enabled: true,
            name: String::new(),
            value: String::new(),
        }
    }
}
impl std::fmt::Debug for AiAgentHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiAgentHeader")
            .field("enabled", &self.enabled)
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .finish()
    }
}
impl AiAgentHeader {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        if self.name.contains(['\r', '\n']) {
            return Err("Invalid AI Agent header name".into());
        }
        let name = self.name.trim();
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
        {
            return Err("Invalid AI Agent header name".into());
        }
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "host"
                | "content-length"
                | "transfer-encoding"
                | "connection"
                | "keep-alive"
                | "proxy-connection"
                | "te"
                | "trailer"
                | "upgrade"
                | "proxy-authorization"
        ) {
            return Err("AI Agent header is controlled by the HTTP transport".into());
        }
        if !self
            .value
            .bytes()
            .all(|byte| byte == b'\t' || (byte >= 32 && byte != 127))
        {
            return Err("Invalid AI Agent header value".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AiAgentProfile {
    #[serde(default)]
    pub id: String,
    /// Empty migrated names resolve through the semantic UI catalog.
    #[serde(default)]
    pub name: String,
    #[serde(flatten)]
    pub connection: AiAgentSettings,
}
impl From<AiAgentSettings> for AiAgentProfile {
    fn from(connection: AiAgentSettings) -> Self {
        Self {
            id: connection.provider.clone(),
            name: String::new(),
            connection,
        }
    }
}
impl AiAgentProfile {
    pub fn new_id() -> String {
        format!("agent-{:032x}", rand::random::<u128>())
    }
    pub(crate) fn normalized(mut self) -> Self {
        self.id = self.id.trim().to_string();
        self.name = self.name.trim().to_string();
        self.connection = normalize_ai_agent_settings(self.connection);
        self.connection.base_url = self
            .connection
            .base_url
            .trim()
            .trim_end_matches('/')
            .to_string();
        self.connection.api_key = self.connection.api_key.trim().to_string();
        self.connection.model = self.connection.model.trim().to_string();
        for header in &mut self.connection.headers {
            if !header.name.contains(['\r', '\n']) {
                header.name = header.name.trim().to_string();
            }
        }
        self
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("AI Agent identity is required".into());
        }
        if self.connection.headers.len() > MAX_AI_AGENT_HEADERS {
            return Err("AI Agent has too many headers".into());
        }
        for header in &self.connection.headers {
            header.validate()?;
        }
        Ok(())
    }
}

pub type AiAgentProfiles = Vec<AiAgentProfile>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct AiAgentConfiguration {
    pub active_id: String,
    pub profiles: AiAgentProfiles,
}
impl From<AiAgentSettings> for AiAgentConfiguration {
    fn from(connection: AiAgentSettings) -> Self {
        let profile = AiAgentProfile::from(normalize_ai_agent_settings(connection)).normalized();
        Self {
            active_id: profile.id.clone(),
            profiles: vec![profile],
        }
    }
}
impl AiAgentConfiguration {
    pub fn profile(&self, id: &str) -> Option<&AiAgentProfile> {
        self.profiles.iter().find(|profile| profile.id == id)
    }
    pub(crate) fn profile_mut(&mut self, id: &str) -> Option<&mut AiAgentProfile> {
        self.profiles.iter_mut().find(|profile| profile.id == id)
    }
    pub fn active_profile(&self) -> Option<&AiAgentProfile> {
        self.profile(&self.active_id)
    }
    pub fn active(&self) -> Option<&AiAgentSettings> {
        self.active_profile().map(|p| &p.connection)
    }
    pub(crate) fn normalized(mut self) -> Self {
        self.profiles = self
            .profiles
            .into_iter()
            .map(AiAgentProfile::normalized)
            .collect();
        self.active_id = self.active_id.trim().to_string();
        if self.profile(&self.active_id).is_none() {
            self.active_id.clear();
        }
        self
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        let mut ids = std::collections::HashSet::new();
        for profile in &self.profiles {
            profile.validate()?;
            if !ids.insert(&profile.id) {
                return Err("Duplicate AI Agent identity".into());
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ProfilesWire {
    List(Vec<AiAgentProfile>),
    Fixed(FixedProfiles),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixedProfiles {
    #[serde(default)]
    custom: AiAgentProfile,
    #[serde(default = "deepseek_profile")]
    deepseek: AiAgentProfile,
    #[serde(default = "openai_profile")]
    openai: AiAgentProfile,
}
impl Default for FixedProfiles {
    fn default() -> Self {
        Self {
            custom: AiAgentProfile::default(),
            deepseek: deepseek_profile(),
            openai: openai_profile(),
        }
    }
}
fn deepseek_profile() -> AiAgentProfile {
    AiAgentSettings {
        provider: "deepseek".into(),
        base_url: "https://api.deepseek.com".into(),
        model: "deepseek-v4-flash".into(),
        ..Default::default()
    }
    .into()
}
fn openai_profile() -> AiAgentProfile {
    AiAgentSettings {
        provider: "openai".into(),
        base_url: "https://api.openai.com/v1".into(),
        model: "gpt-4o-mini".into(),
        ..Default::default()
    }
    .into()
}
impl FixedProfiles {
    fn migrate(self, active: String) -> AiAgentConfiguration {
        let profiles = [self.custom, self.deepseek, self.openai]
            .into_iter()
            .zip(["custom", "deepseek", "openai"])
            .map(|(mut profile, provider)| {
                profile.id = provider.into();
                profile.connection.provider = provider.into();
                profile
            })
            .collect();
        AiAgentConfiguration {
            active_id: active,
            profiles,
        }
    }
}
impl<'de> Deserialize<'de> for AiAgentConfiguration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize, Default)]
        struct Wire {
            active_id: Option<String>,
            active_provider: Option<String>,
            profiles: Option<ProfilesWire>,
            provider: Option<String>,
            name: Option<String>,
            base_url: Option<String>,
            api_key: Option<String>,
            model: Option<String>,
            reply_language: Option<String>,
            headers: Option<Vec<AiAgentHeader>>,
        }
        let wire = Wire::deserialize(deserializer)?;
        let mut configuration = match wire.profiles {
            Some(ProfilesWire::List(profiles)) => Self {
                active_id: wire.active_id.unwrap_or_default(),
                profiles,
            },
            Some(ProfilesWire::Fixed(profiles)) => {
                profiles.migrate(wire.active_provider.unwrap_or_else(|| "custom".into()))
            }
            None if wire.active_id.is_some() => Self {
                active_id: wire.active_id.unwrap(),
                profiles: vec![],
            },
            None if wire.active_provider.is_some() => {
                FixedProfiles::default().migrate(wire.active_provider.unwrap())
            }
            None if wire.provider.is_some()
                || wire.name.is_some()
                || wire.base_url.is_some()
                || wire.api_key.is_some()
                || wire.model.is_some()
                || wire.reply_language.is_some()
                || wire.headers.is_some() =>
            {
                let name = wire.name.unwrap_or_default();
                let mut configuration = Self::from(AiAgentSettings {
                    provider: wire.provider.unwrap_or_else(|| "custom".into()),
                    base_url: wire.base_url.unwrap_or_default(),
                    api_key: wire.api_key.unwrap_or_default(),
                    model: wire.model.unwrap_or_default(),
                    reply_language: wire.reply_language.unwrap_or_else(|| "auto".into()),
                    headers: wire.headers.unwrap_or_default(),
                });
                configuration.profiles[0].name = name;
                configuration
            }
            None => Self::default(),
        }
        .normalized();
        configuration.validate().map_err(serde::de::Error::custom)?;
        // Legacy fixed ids are stable; new list ids are persisted verbatim.
        configuration.active_id = configuration.active_id.trim().to_string();
        Ok(configuration)
    }
}

#[cfg(test)]
#[path = "ai_agents_tests.rs"]
mod tests;
