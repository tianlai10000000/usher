use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub(crate) const PR_REVIEW_APPROVED: &str = "pr.review.approved.v1";
pub(crate) const PR_MERGED: &str = "pr.merged.v1";
pub(super) const LEGACY_PR_REVIEW_SUBMITTED: &str = "pr.review.submitted.v1";
#[cfg(feature = "ai-code-review")]
pub(crate) const AI_REVIEW_COMPLETED: &str = "ai.review.completed.v1";
pub(crate) const CATEGORY_PR_REVIEW: &str = "pr_review";
pub(crate) const CATEGORY_AI_REVIEW: &str = "ai_review";
pub(crate) const CATEGORY_OTHER: &str = "other";
pub(super) const MAX_RUNS: usize = 64;
pub(super) const MAX_SCRIPT_BYTES: usize = 64 * 1024;
pub(super) const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
pub(super) const MAX_TAIL_BYTES: usize = 32 * 1024;

pub(crate) fn new_id(prefix: &str) -> String {
    format!("{prefix}-{:016x}", rand::random::<u64>())
}

pub(crate) fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HookLanguage {
    Shell,
    #[serde(
        rename = "javascript_typescript",
        alias = "java_script",
        alias = "type_script",
        alias = "javascript",
        alias = "typescript"
    )]
    JavaScriptTypeScript,
}

impl HookLanguage {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Shell => "Shell",
            Self::JavaScriptTypeScript => "JavaScript/TypeScript",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct HookDefinition {
    pub id: String,
    pub revision: u64,
    pub name: String,
    pub enabled: bool,
    pub event: String,
    /// Empty matches every project, including application-scoped events.
    pub project: String,
    pub outcome: String,
    pub language: HookLanguage,
    /// An executable path/name, never a shell command with arguments.
    pub interpreter: String,
    pub script: String,
    pub timeout_seconds: u64,
}

impl Default for HookDefinition {
    fn default() -> Self {
        Self {
            id: new_id("hook"),
            revision: 0,
            name: "PR Review".into(),
            enabled: false,
            event: PR_REVIEW_APPROVED.into(),
            project: String::new(),
            outcome: String::new(),
            language: HookLanguage::Shell,
            interpreter: String::new(),
            script: String::new(),
            timeout_seconds: 60,
        }
    }
}

impl HookDefinition {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !valid_id(&self.id) {
            return Err("Invalid Hook ID.".into());
        }
        if self.name.trim().is_empty() {
            return Err("Hook name is required.".into());
        }
        if self.script.trim().is_empty() {
            return Err("Hook script is empty.".into());
        }
        if self.script.len() > MAX_SCRIPT_BYTES {
            return Err("Hook script exceeds 64 KiB.".into());
        }
        if !(1..=3600).contains(&self.timeout_seconds) {
            return Err("Timeout must be between 1 and 3600 seconds.".into());
        }
        if self.name.len() > 256
            || self.interpreter.len() > 4096
            || self.project.len() > 4096
            || self.outcome.len() > 128
        {
            return Err("Hook configuration field is too long.".into());
        }
        if self.interpreter.contains(['\0', '\n', '\r']) {
            return Err("Interpreter must be one executable path.".into());
        }
        Ok(())
    }

    pub(crate) fn matches(&self, event: &HookTrigger) -> bool {
        self.enabled
            && self.event == event.name
            && (self.project.is_empty() || event.project.as_deref() == Some(self.project.as_str()))
            && (self.outcome.is_empty() || self.outcome == event.outcome)
    }
}

pub(super) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 96
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct HookTrigger {
    pub event_id: String,
    pub name: String,
    pub source_module: String,
    pub occurred_at_ms: i64,
    pub project: Option<String>,
    pub cwd: Option<PathBuf>,
    pub outcome: String,
    pub payload: serde_json::Value,
    pub test: bool,
}

impl HookTrigger {
    pub(crate) fn new(name: &str, source: &str, payload: serde_json::Value) -> Self {
        Self {
            event_id: new_id("event"),
            name: name.into(),
            source_module: source.into(),
            occurred_at_ms: now_ms(),
            project: None,
            cwd: None,
            outcome: "success".into(),
            payload,
            test: false,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct EventDefinition {
    pub name: String,
    pub category: String,
    pub project_scoped: bool,
    pub sample: serde_json::Value,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HookConclusion {
    Success,
    Failure,
    TimedOut,
    Cancelled,
    StartFailed,
    Interrupted,
    Skipped,
}

impl HookConclusion {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
            Self::StartFailed => "start_failed",
            Self::Interrupted => "interrupted",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct HookRun {
    pub id: String,
    pub hook_id: String,
    pub config_revision: u64,
    pub name: String,
    pub event_id: String,
    pub event: String,
    pub project: Option<String>,
    pub test: bool,
    pub created_at_ms: i64,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
    pub conclusion: Option<HookConclusion>,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub detail: String,
    pub seq: u64,
    #[serde(skip)]
    pub log: Arc<String>,
}

impl HookRun {
    pub(super) fn queued(hook: &HookDefinition, trigger: &HookTrigger) -> Self {
        Self {
            id: new_id("run"),
            hook_id: hook.id.clone(),
            config_revision: hook.revision,
            name: hook.name.clone(),
            event_id: trigger.event_id.clone(),
            event: trigger.name.clone(),
            project: trigger.project.clone(),
            test: trigger.test,
            created_at_ms: now_ms(),
            started_at_ms: None,
            finished_at_ms: None,
            conclusion: None,
            exit_code: None,
            signal: None,
            detail: String::new(),
            seq: 0,
            log: Arc::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct HookEvent {
    pub run_id: String,
    pub hook_id: String,
    pub event_id: String,
    pub seq: u64,
    pub time_ms: i64,
    pub kind: String,
    pub data: serde_json::Value,
}

#[derive(Clone, Debug)]
pub(crate) struct DispatchReceipt {
    pub event_id: String,
    pub runs: Vec<String>,
    pub skipped: Vec<String>,
}
