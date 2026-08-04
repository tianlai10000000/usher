use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Semantic message level. Presentation and native adapters map this to their
/// own visual/urgency vocabulary without changing the domain model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSeverity {
    Info,
    Success,
    Warning,
    Error,
}

/// OS-level delivery policy for one publication. The policy is intentionally
/// transient: the durable message records what happened, not how one process
/// instance chose to present it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NativeDelivery {
    Never,
    #[default]
    WhenUnfocused,
    Always,
}

impl NativeDelivery {
    pub(crate) fn should_deliver(self, application_active: bool) -> bool {
        match self {
            Self::Never => false,
            Self::WhenUnfocused => !application_active,
            Self::Always => true,
        }
    }
}

/// Stable built-in category keys. Categories remain strings so feature and
/// adapter crates can add new values without changing this closed core model.
pub mod categories {
    pub const GENERAL: &str = "general";
    pub const WEBHOOK: &str = "webhook";
    pub const SOFTWARE_UPDATE: &str = "software_update";
    pub const CD_WORKFLOW: &str = "cd_workflow";
}

/// Durable application message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub title: String,
    pub body: String,
    pub source: String,
    /// Extensible ingress channel such as `application` or a future `webhook`.
    /// It is intentionally a string so adding an adapter does not change the
    /// closed message-center core.
    #[serde(default = "default_channel")]
    pub channel: String,
    /// Semantic business classification, independent from transport/channel.
    #[serde(default = "default_category")]
    pub category: String,
    pub severity: MessageSeverity,
    pub created_at: DateTime<Utc>,
    pub read: bool,
    /// Optional application route or feature-owned target. The core stores it
    /// as data and deliberately does not depend on Usher's router.
    pub target: Option<String>,
    /// Namespaced key supplied by external adapters to make retries idempotent.
    #[serde(default)]
    pub deduplication_key: Option<String>,
    /// Small, non-sensitive routing/display attributes. Raw webhook payloads,
    /// credentials, and authorization material must never be stored here.
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

/// Input accepted by publishers. New optional delivery metadata can be added
/// without changing stored-message consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageDraft {
    pub title: String,
    pub body: String,
    pub source: String,
    pub channel: String,
    pub category: String,
    pub severity: MessageSeverity,
    pub target: Option<String>,
    pub native_delivery: NativeDelivery,
    pub deduplication_key: Option<String>,
    pub metadata: BTreeMap<String, String>,
}

impl MessageDraft {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            source: "Usher".to_string(),
            channel: default_channel(),
            category: default_category(),
            severity: MessageSeverity::Info,
            target: None,
            native_delivery: NativeDelivery::WhenUnfocused,
            deduplication_key: None,
            metadata: BTreeMap::new(),
        }
    }

    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    pub fn severity(mut self, severity: MessageSeverity) -> Self {
        self.severity = severity;
        self
    }

    pub fn channel(mut self, channel: impl Into<String>) -> Self {
        self.channel = channel.into();
        self
    }

    pub fn category(mut self, category: impl Into<String>) -> Self {
        self.category = category.into();
        self
    }

    pub fn deduplication_key(mut self, key: impl Into<String>) -> Self {
        self.deduplication_key = Some(key.into());
        self
    }

    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    /// Compatibility helper for callers that only need on/off behavior.
    /// `true` retains the old unconditional semantics; new code should prefer
    /// [`Self::native_delivery`].
    pub fn desktop(mut self, desktop: bool) -> Self {
        self.native_delivery = if desktop {
            NativeDelivery::Always
        } else {
            NativeDelivery::Never
        };
        self
    }

    pub fn native_delivery(mut self, delivery: NativeDelivery) -> Self {
        self.native_delivery = delivery;
        self
    }
}

fn default_channel() -> String {
    "application".to_string()
}

fn default_category() -> String {
    categories::GENERAL.to_string()
}
