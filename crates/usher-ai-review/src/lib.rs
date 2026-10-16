//! UI-independent AI code-review domain contracts and pure transformations.
//!
//! The application injects model, repository-context, persistence, runtime,
//! and presentation adapters. This crate never depends on Slint, reqwest, or a
//! concrete code-hosting provider.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use usher_state_store::Reduction;

pub const DEFAULT_MAX_CONCURRENT_TASKS: u8 = 3;
pub const MIN_MAX_CONCURRENT_TASKS: u8 = 1;
pub const MAX_MAX_CONCURRENT_TASKS: u8 = 8;
pub const DEFAULT_DIFF_CHUNK_BYTES: usize = 64 * 1024;
pub const MAX_RULE_BYTES: u64 = 512 * 1024;
pub const MAX_RETAINED_TASK_SUMMARIES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuleId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectReviewKey(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRule {
    pub id: RuleId,
    pub name: String,
    pub markdown_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", content = "rule_id", rename_all = "snake_case")]
pub enum RuleBinding {
    #[default]
    UseDefault,
    Preset(RuleId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewConfigurationState {
    #[serde(default = "default_max_concurrent_tasks")]
    pub max_concurrent_tasks: u8,
    #[serde(default)]
    pub rules: Vec<ReviewRule>,
    #[serde(default)]
    pub default_rule_id: Option<RuleId>,
    #[serde(default)]
    pub project_bindings: BTreeMap<ProjectReviewKey, RuleBinding>,
}

impl Default for ReviewConfigurationState {
    fn default() -> Self {
        Self {
            max_concurrent_tasks: DEFAULT_MAX_CONCURRENT_TASKS,
            rules: Vec::new(),
            default_rule_id: None,
            project_bindings: BTreeMap::new(),
        }
    }
}

impl ReviewConfigurationState {
    pub fn normalize(mut self) -> Self {
        self.max_concurrent_tasks = self
            .max_concurrent_tasks
            .clamp(MIN_MAX_CONCURRENT_TASKS, MAX_MAX_CONCURRENT_TASKS);
        self.rules.retain(|rule| {
            !rule.id.0.trim().is_empty()
                && !rule.name.trim().is_empty()
                && !rule.markdown_path.as_os_str().is_empty()
        });
        self.rules.sort_by(|left, right| left.name.cmp(&right.name));
        self.rules.dedup_by(|left, right| left.id == right.id);
        if self
            .default_rule_id
            .as_ref()
            .is_some_and(|id| !self.rules.iter().any(|rule| &rule.id == id))
        {
            self.default_rule_id = None;
        }
        let valid_ids = self
            .rules
            .iter()
            .map(|rule| rule.id.clone())
            .collect::<Vec<_>>();
        for binding in self.project_bindings.values_mut() {
            if matches!(binding, RuleBinding::Preset(id) if !valid_ids.contains(id)) {
                *binding = RuleBinding::UseDefault;
            }
        }
        self
    }

    pub fn rule(&self, id: &RuleId) -> Option<&ReviewRule> {
        self.rules.iter().find(|rule| &rule.id == id)
    }

    pub fn resolved_rule_for(&self, project: &ProjectReviewKey) -> Option<&ReviewRule> {
        match self.project_bindings.get(project) {
            Some(RuleBinding::Preset(id)) => self.rule(id),
            Some(RuleBinding::UseDefault) | None => {
                self.default_rule_id.as_ref().and_then(|id| self.rule(id))
            }
        }
    }
}

const fn default_max_concurrent_tasks() -> u8 {
    DEFAULT_MAX_CONCURRENT_TASKS
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewConfigAction {
    UpsertRule(ReviewRule),
    DeleteRule {
        id: RuleId,
        replacement_default: Option<RuleId>,
    },
    SetDefaultRule(Option<RuleId>),
    SetProjectRule {
        project: ProjectReviewKey,
        binding: RuleBinding,
    },
    SetMaxConcurrentTasks(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewConfigError {
    InvalidRule,
    UnknownRule,
    DuplicateName,
    InvalidConcurrency,
}

pub fn reduce_configuration(
    current: &ReviewConfigurationState,
    action: ReviewConfigAction,
) -> Reduction<ReviewConfigurationState, Result<(), ReviewConfigError>> {
    let mut next = current.clone();
    let result = match action {
        ReviewConfigAction::UpsertRule(mut rule) => {
            rule.name = rule.name.trim().to_string();
            if rule.id.0.trim().is_empty()
                || rule.name.is_empty()
                || rule.markdown_path.as_os_str().is_empty()
            {
                Err(ReviewConfigError::InvalidRule)
            } else if next
                .rules
                .iter()
                .any(|item| item.id != rule.id && item.name.eq_ignore_ascii_case(&rule.name))
            {
                Err(ReviewConfigError::DuplicateName)
            } else {
                match next.rules.iter_mut().find(|item| item.id == rule.id) {
                    Some(existing) => *existing = rule,
                    None => next.rules.push(rule),
                }
                next.rules.sort_by(|left, right| left.name.cmp(&right.name));
                Ok(())
            }
        }
        ReviewConfigAction::DeleteRule {
            id,
            replacement_default,
        } => {
            let rule_is_missing = !next.rules.iter().any(|rule| rule.id == id);
            let replacement_is_invalid = replacement_default.as_ref().is_some_and(|replacement| {
                replacement == &id || !next.rules.iter().any(|rule| &rule.id == replacement)
            });
            if rule_is_missing || replacement_is_invalid {
                Err(ReviewConfigError::UnknownRule)
            } else {
                next.rules.retain(|rule| rule.id != id);
                if next.default_rule_id.as_ref() == Some(&id) {
                    next.default_rule_id = replacement_default;
                }
                for binding in next.project_bindings.values_mut() {
                    if matches!(binding, RuleBinding::Preset(rule_id) if rule_id == &id) {
                        *binding = RuleBinding::UseDefault;
                    }
                }
                Ok(())
            }
        }
        ReviewConfigAction::SetDefaultRule(id) => {
            if id
                .as_ref()
                .is_some_and(|id| !next.rules.iter().any(|rule| &rule.id == id))
            {
                Err(ReviewConfigError::UnknownRule)
            } else {
                next.default_rule_id = id;
                Ok(())
            }
        }
        ReviewConfigAction::SetProjectRule { project, binding } => {
            if matches!(&binding, RuleBinding::Preset(id) if !next.rules.iter().any(|rule| &rule.id == id))
            {
                Err(ReviewConfigError::UnknownRule)
            } else {
                next.project_bindings.insert(project, binding);
                Ok(())
            }
        }
        ReviewConfigAction::SetMaxConcurrentTasks(value) => {
            if !(MIN_MAX_CONCURRENT_TASKS..=MAX_MAX_CONCURRENT_TASKS).contains(&value) {
                Err(ReviewConfigError::InvalidConcurrency)
            } else {
                next.max_concurrent_tasks = value;
                Ok(())
            }
        }
    };

    if result.is_err() || &next == current {
        Reduction::unchanged(current.clone(), result)
    } else {
        Reduction::changed(next, result)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiModelRef {
    pub provider: String,
    pub base_url: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewTarget {
    pub project_key: ProjectReviewKey,
    pub project_name: String,
    pub pull_request_number: String,
    pub pull_request_title: String,
    pub base_sha: String,
    pub head_sha: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub author_name: Option<String>,
    #[serde(default)]
    pub author_email: Option<String>,
    #[serde(default)]
    pub source_branch: String,
    #[serde(default)]
    pub target_branch: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RepositoryContext {
    pub local_repo: Option<PathBuf>,
    pub remote_repo: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleSnapshot {
    pub id: RuleId,
    pub name: String,
    pub markdown_path: PathBuf,
    pub content: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewInput {
    pub target: ReviewTarget,
    pub model: AiModelRef,
    pub unified_diff: String,
    #[serde(default)]
    pub unified_diff_truncated: bool,
    pub repository_context: RepositoryContext,
    pub base_rule: Option<RuleSnapshot>,
    #[serde(default)]
    pub additional_rules: Vec<RuleSnapshot>,
    #[serde(default)]
    pub extra_requirements: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FindingDiffSide {
    Left,
    #[default]
    Right,
}

impl FindingDiffSide {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFinding {
    pub severity: FindingSeverity,
    pub file: String,
    #[serde(default)]
    pub line: i32,
    #[serde(default)]
    pub side: FindingDiffSide,
    pub title: String,
    pub explanation: String,
    #[serde(default)]
    pub suggestion: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReviewResult {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub findings: Vec<ReviewFinding>,
    #[serde(default)]
    pub skipped_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReviewTaskId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewTaskStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl ReviewTaskStatus {
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

impl FindingSeverity {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

/// Non-secret request attribution, kept raw so presentation can change locale.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub name: String,
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewTaskSummary {
    pub id: ReviewTaskId,
    pub target: ReviewTarget,
    pub rule_name: String,
    pub model_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentIdentity>,
    pub status: ReviewTaskStatus,
    pub progress_percent: u8,
    #[serde(default)]
    pub completed_chunks: u32,
    #[serde(default)]
    pub total_chunks: u32,
    pub finding_count: u32,
    pub error: String,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReviewTaskState {
    pub tasks: BTreeMap<ReviewTaskId, ReviewTaskSummary>,
    pub order: Vec<ReviewTaskId>,
}

impl ReviewTaskState {
    pub fn active_count(&self) -> usize {
        self.tasks
            .values()
            .filter(|task| task.status.is_active())
            .count()
    }

    pub fn task_for_target(&self, target: &ReviewTarget) -> Option<&ReviewTaskSummary> {
        self.order.iter().rev().find_map(|id| {
            self.tasks.get(id).filter(|task| {
                task.target.project_key == target.project_key
                    && task.target.pull_request_number == target.pull_request_number
                    && task.target.head_sha == target.head_sha
            })
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewTaskAction {
    Hydrate(ReviewTaskState),
    Enqueue(ReviewTaskSummary),
    Start {
        id: ReviewTaskId,
        updated_at_unix_ms: u64,
    },
    SetTarget {
        id: ReviewTaskId,
        target: ReviewTarget,
        updated_at_unix_ms: u64,
    },
    SetChunkProgress {
        id: ReviewTaskId,
        completed_chunks: u32,
        total_chunks: u32,
        updated_at_unix_ms: u64,
    },
    Complete {
        id: ReviewTaskId,
        finding_count: u32,
        updated_at_unix_ms: u64,
    },
    Fail {
        id: ReviewTaskId,
        error: String,
        updated_at_unix_ms: u64,
    },
    Cancel {
        id: ReviewTaskId,
        updated_at_unix_ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewTaskActionResult {
    Changed,
    Duplicate(ReviewTaskId),
    UnknownTask,
    InvalidTransition,
}

pub fn reduce_tasks(
    current: &ReviewTaskState,
    action: ReviewTaskAction,
) -> Reduction<ReviewTaskState, ReviewTaskActionResult> {
    match action {
        ReviewTaskAction::Hydrate(stored) => {
            let mut next = stored;
            next.order.retain(|id| next.tasks.contains_key(id));
            next.tasks.retain(|id, _| next.order.contains(id));
            for id in &current.order {
                if let Some(task) = current.tasks.get(id) {
                    if !next.order.contains(id) {
                        next.order.push(id.clone());
                    }
                    next.tasks.insert(id.clone(), task.clone());
                }
            }
            while next.order.len() > MAX_RETAINED_TASK_SUMMARIES {
                let Some(index) = next.order.iter().position(|id| {
                    next.tasks
                        .get(id)
                        .is_some_and(|task| !task.status.is_active())
                }) else {
                    break;
                };
                let removed = next.order.remove(index);
                next.tasks.remove(&removed);
            }
            if &next == current {
                Reduction::unchanged(current.clone(), ReviewTaskActionResult::Changed)
            } else {
                Reduction::changed(next, ReviewTaskActionResult::Changed)
            }
        }
        ReviewTaskAction::Enqueue(summary) => {
            if let Some(existing) = current.tasks.values().find(|task| {
                task.status.is_active()
                    && task.target.project_key == summary.target.project_key
                    && task.target.pull_request_number == summary.target.pull_request_number
            }) {
                Reduction::unchanged(
                    current.clone(),
                    ReviewTaskActionResult::Duplicate(existing.id.clone()),
                )
            } else if current.tasks.contains_key(&summary.id) {
                Reduction::unchanged(
                    current.clone(),
                    ReviewTaskActionResult::Duplicate(summary.id),
                )
            } else {
                let mut next = current.clone();
                next.order.push(summary.id.clone());
                next.tasks.insert(summary.id.clone(), summary);
                while next.order.len() > MAX_RETAINED_TASK_SUMMARIES {
                    let Some(index) = next.order.iter().position(|id| {
                        next.tasks
                            .get(id)
                            .is_some_and(|task| !task.status.is_active())
                    }) else {
                        break;
                    };
                    let removed = next.order.remove(index);
                    next.tasks.remove(&removed);
                }
                Reduction::changed(next, ReviewTaskActionResult::Changed)
            }
        }
        ReviewTaskAction::Start {
            id,
            updated_at_unix_ms,
        } => match current.tasks.get(&id) {
            Some(task) if task.status == ReviewTaskStatus::Queued => {
                let mut next = current.clone();
                let task = next.tasks.get_mut(&id).expect("validated task exists");
                task.status = ReviewTaskStatus::Running;
                task.progress_percent = task.progress_percent.max(1);
                task.updated_at_unix_ms = updated_at_unix_ms;
                Reduction::changed(next, ReviewTaskActionResult::Changed)
            }
            Some(_) => {
                Reduction::unchanged(current.clone(), ReviewTaskActionResult::InvalidTransition)
            }
            None => Reduction::unchanged(current.clone(), ReviewTaskActionResult::UnknownTask),
        },
        ReviewTaskAction::SetTarget {
            id,
            target,
            updated_at_unix_ms,
        } => match current.tasks.get(&id) {
            Some(task) if task.status.is_active() => {
                if task.target == target && task.updated_at_unix_ms == updated_at_unix_ms {
                    return Reduction::unchanged(current.clone(), ReviewTaskActionResult::Changed);
                }
                let mut next = current.clone();
                let task = next.tasks.get_mut(&id).expect("validated task exists");
                task.target = target;
                task.updated_at_unix_ms = updated_at_unix_ms;
                Reduction::changed(next, ReviewTaskActionResult::Changed)
            }
            Some(_) => {
                Reduction::unchanged(current.clone(), ReviewTaskActionResult::InvalidTransition)
            }
            None => Reduction::unchanged(current.clone(), ReviewTaskActionResult::UnknownTask),
        },
        ReviewTaskAction::SetChunkProgress {
            id,
            completed_chunks,
            total_chunks,
            updated_at_unix_ms,
        } => match current.tasks.get(&id) {
            Some(task) if task.status == ReviewTaskStatus::Running => {
                let completed_chunks = completed_chunks.min(total_chunks);
                let progress_percent = if total_chunks == 0 {
                    task.progress_percent.max(1)
                } else {
                    ((u64::from(completed_chunks) * 96) / u64::from(total_chunks)).clamp(1, 96)
                        as u8
                };
                if task.total_chunks == total_chunks
                    && task.completed_chunks == completed_chunks
                    && task.progress_percent == progress_percent
                    && task.updated_at_unix_ms == updated_at_unix_ms
                {
                    return Reduction::unchanged(current.clone(), ReviewTaskActionResult::Changed);
                }
                let mut next = current.clone();
                let task = next.tasks.get_mut(&id).expect("validated task exists");
                task.total_chunks = total_chunks;
                task.completed_chunks = completed_chunks;
                task.progress_percent = progress_percent;
                task.updated_at_unix_ms = updated_at_unix_ms;
                Reduction::changed(next, ReviewTaskActionResult::Changed)
            }
            Some(_) => {
                Reduction::unchanged(current.clone(), ReviewTaskActionResult::InvalidTransition)
            }
            None => Reduction::unchanged(current.clone(), ReviewTaskActionResult::UnknownTask),
        },
        ReviewTaskAction::Complete {
            id,
            finding_count,
            updated_at_unix_ms,
        } => match current.tasks.get(&id) {
            Some(task) if task.status == ReviewTaskStatus::Running => {
                let mut next = current.clone();
                let task = next.tasks.get_mut(&id).expect("validated task exists");
                task.status = ReviewTaskStatus::Completed;
                task.progress_percent = 100;
                if task.total_chunks > 0 {
                    task.completed_chunks = task.total_chunks;
                }
                task.finding_count = finding_count;
                task.error.clear();
                task.updated_at_unix_ms = updated_at_unix_ms;
                Reduction::changed(next, ReviewTaskActionResult::Changed)
            }
            Some(_) => {
                Reduction::unchanged(current.clone(), ReviewTaskActionResult::InvalidTransition)
            }
            None => Reduction::unchanged(current.clone(), ReviewTaskActionResult::UnknownTask),
        },
        ReviewTaskAction::Fail {
            id,
            error,
            updated_at_unix_ms,
        } => match current.tasks.get(&id) {
            Some(task) if task.status.is_active() => {
                let mut next = current.clone();
                let task = next.tasks.get_mut(&id).expect("validated task exists");
                task.status = ReviewTaskStatus::Failed;
                task.error = error;
                task.updated_at_unix_ms = updated_at_unix_ms;
                Reduction::changed(next, ReviewTaskActionResult::Changed)
            }
            Some(_) => {
                Reduction::unchanged(current.clone(), ReviewTaskActionResult::InvalidTransition)
            }
            None => Reduction::unchanged(current.clone(), ReviewTaskActionResult::UnknownTask),
        },
        ReviewTaskAction::Cancel {
            id,
            updated_at_unix_ms,
        } => match current.tasks.get(&id) {
            Some(task) if task.status.is_active() => {
                let mut next = current.clone();
                let task = next.tasks.get_mut(&id).expect("validated task exists");
                task.status = ReviewTaskStatus::Cancelled;
                task.updated_at_unix_ms = updated_at_unix_ms;
                Reduction::changed(next, ReviewTaskActionResult::Changed)
            }
            Some(_) => {
                Reduction::unchanged(current.clone(), ReviewTaskActionResult::InvalidTransition)
            }
            None => Reduction::unchanged(current.clone(), ReviewTaskActionResult::UnknownTask),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewConversationRole {
    User,
    Assistant,
}

impl ReviewConversationRole {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewConversationMessage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    pub role: ReviewConversationRole,
    pub content: String,
    pub created_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PromptProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PromptContextEntry {
    pub name: String,
    pub bytes: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PromptProvenance {
    pub prompt_id: String,
    pub prompt_version: u32,
    pub effective_reply_language: String,
    pub request_count: u32,
    #[serde(default)]
    pub context: Vec<PromptContextEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewSession {
    pub task: ReviewTaskSummary,
    pub input: ReviewInput,
    pub result: ReviewResult,
    #[serde(default)]
    pub conversation: Vec<ReviewConversationMessage>,
    #[serde(default)]
    pub prompt: PromptProvenance,
}

/// Small synchronous scheduler state used by the application runtime adapter.
/// It never owns review inputs or diff buffers.
#[derive(Debug, Clone)]
pub struct ReviewScheduler {
    max_concurrent: usize,
    active: usize,
    queued: VecDeque<ReviewTaskId>,
}

impl ReviewScheduler {
    pub fn new(max_concurrent: u8) -> Self {
        Self {
            max_concurrent: usize::from(
                max_concurrent.clamp(MIN_MAX_CONCURRENT_TASKS, MAX_MAX_CONCURRENT_TASKS),
            ),
            active: 0,
            queued: VecDeque::new(),
        }
    }

    pub fn enqueue(&mut self, id: ReviewTaskId) {
        self.queued.push_back(id);
    }

    pub fn set_max_concurrent(&mut self, value: u8) {
        self.max_concurrent =
            usize::from(value.clamp(MIN_MAX_CONCURRENT_TASKS, MAX_MAX_CONCURRENT_TASKS));
    }

    pub fn take_ready(&mut self) -> Vec<ReviewTaskId> {
        let available = self.max_concurrent.saturating_sub(self.active);
        let ready = (0..available)
            .filter_map(|_| self.queued.pop_front())
            .collect::<Vec<_>>();
        self.active += ready.len();
        ready
    }

    pub fn finish(&mut self) {
        self.active = self.active.saturating_sub(1);
    }

    pub fn cancel_queued(&mut self, id: &ReviewTaskId) -> bool {
        let before = self.queued.len();
        self.queued.retain(|queued| queued != id);
        before != self.queued.len()
    }

    pub const fn active(&self) -> usize {
        self.active
    }

    pub fn queued(&self) -> usize {
        self.queued.len()
    }
}

pub fn merge_review_results(results: impl IntoIterator<Item = ReviewResult>) -> ReviewResult {
    let mut merged = ReviewResult::default();
    let mut seen_findings = std::collections::BTreeSet::new();
    let mut seen_skipped = std::collections::BTreeSet::new();
    for result in results {
        if !result.summary.trim().is_empty() {
            if !merged.summary.is_empty() {
                merged.summary.push_str("\n\n");
            }
            merged.summary.push_str(result.summary.trim());
        }
        for finding in result.findings {
            let key = (
                finding.file.clone(),
                finding.line,
                finding.side,
                finding.title.to_lowercase(),
            );
            if seen_findings.insert(key) {
                merged.findings.push(finding);
            }
        }
        for file in result.skipped_files {
            if seen_skipped.insert(file.clone()) {
                merged.skipped_files.push(file);
            }
        }
    }
    merged
}

pub fn chunk_diff(diff: &str, max_bytes: usize) -> Vec<String> {
    let max_bytes = max_bytes.max(1024);
    if diff.len() <= max_bytes {
        return (!diff.trim().is_empty())
            .then(|| diff.to_string())
            .into_iter()
            .collect();
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    for section in split_diff_sections(diff) {
        if !current.is_empty() && current.len().saturating_add(section.len()) > max_bytes {
            chunks.push(std::mem::take(&mut current));
        }
        if section.len() <= max_bytes {
            current.push_str(section);
            continue;
        }
        // Oversized file section: split on line boundaries and re-emit the
        // file headers plus a synthetic hunk marker on every continuation
        // chunk so each chunk stays a parseable unified diff on its own.
        // `current` is always empty here because the flush above triggers
        // whenever it is non-empty and the section exceeds `max_bytes`.
        let file_header = section_file_header(section);
        let mut position = None;
        let mut section_continued = false;
        let mut remainder = section;
        while !remainder.is_empty() {
            let split_at = remainder
                .find('\n')
                .map_or(remainder.len(), |index| index + 1);
            let (line_with_newline, rest) = remainder.split_at(split_at);
            remainder = rest;
            let logical_line = line_with_newline.trim_end_matches(['\r', '\n']);
            if let Some(next) = parse_hunk_position(logical_line) {
                position = Some(next);
            }
            let mut line = line_with_newline;
            if !current.is_empty() && current.len().saturating_add(line.len()) > max_bytes {
                chunks.push(std::mem::take(&mut current));
                section_continued = true;
            }
            while !line.is_empty() {
                if current.is_empty() && section_continued {
                    current.push_str(continuation_header(&file_header, position).as_str());
                }
                let budget = max_bytes.saturating_sub(current.len());
                if line.len() <= budget {
                    current.push_str(line);
                    break;
                }
                // One line larger than the remaining budget is cut at a char
                // boundary; taking at least one char guarantees progress.
                let piece = match floor_char_boundary(line, budget) {
                    0 => line.chars().next().map_or(line.len(), char::len_utf8),
                    boundary => boundary,
                };
                current.push_str(&line[..piece]);
                line = &line[piece..];
                chunks.push(std::mem::take(&mut current));
                section_continued = true;
            }
            advance_diff_position(&mut position, logical_line);
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Header prepended to every continuation chunk of one oversized file section
/// so downstream consumers can attribute lines to the right file and treat
/// the cut point as a fresh, position-less hunk.
fn section_file_header(section: &str) -> String {
    let mut header = String::new();
    for line in section.lines().take(8) {
        if line.starts_with("@@") {
            break;
        }
        if line.starts_with("diff --git ") || line.starts_with("--- ") || line.starts_with("+++ ") {
            header.push_str(line);
            header.push('\n');
        }
    }
    header
}

#[derive(Clone, Copy)]
struct DiffPosition {
    old_line: u64,
    new_line: u64,
}

fn parse_hunk_position(line: &str) -> Option<DiffPosition> {
    let mut parts = line.split_whitespace();
    (parts.next()? == "@@").then_some(())?;
    let old_line = parts
        .next()?
        .strip_prefix('-')?
        .split(',')
        .next()?
        .parse()
        .ok()?;
    let new_line = parts
        .next()?
        .strip_prefix('+')?
        .split(',')
        .next()?
        .parse()
        .ok()?;
    Some(DiffPosition { old_line, new_line })
}

fn continuation_header(file_header: &str, position: Option<DiffPosition>) -> String {
    let position = position.unwrap_or(DiffPosition {
        old_line: 0,
        new_line: 0,
    });
    format!(
        "{file_header}@@ -{},0 +{},0 @@ (continued)\n",
        position.old_line, position.new_line
    )
}

fn advance_diff_position(position: &mut Option<DiffPosition>, line: &str) {
    let Some(position) = position.as_mut() else {
        return;
    };
    match line.as_bytes().first().copied() {
        Some(b' ') => {
            position.old_line = position.old_line.saturating_add(1);
            position.new_line = position.new_line.saturating_add(1);
        }
        Some(b'-') => position.old_line = position.old_line.saturating_add(1),
        Some(b'+') => position.new_line = position.new_line.saturating_add(1),
        _ => {}
    }
}

fn floor_char_boundary(text: &str, index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    let mut index = index;
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn split_diff_sections(diff: &str) -> Vec<&str> {
    let mut starts = diff
        .match_indices("diff --git ")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if starts.is_empty() {
        return vec![diff];
    }
    if starts[0] != 0 {
        starts.insert(0, 0);
    }
    starts.push(diff.len());
    starts
        .windows(2)
        .filter_map(|range| diff.get(range[0]..range[1]))
        .collect()
}

pub fn parse_review_response(response: &str) -> Result<ReviewResult, serde_json::Error> {
    let trimmed = response.trim();
    let json = if let Some(rest) = trimmed.strip_prefix("```json") {
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else if let Some(rest) = trimmed.strip_prefix("```") {
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else {
        trimmed
    };
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str, name: &str) -> ReviewRule {
        ReviewRule {
            id: RuleId(id.into()),
            name: name.into(),
            markdown_path: PathBuf::from(format!("/{id}.md")),
        }
    }

    fn continuation_payload(chunk: &str) -> &str {
        let marker = "(continued)\n";
        let payload_start = chunk
            .find(marker)
            .map(|index| index + marker.len())
            .expect("continuation chunk should include a synthetic hunk header");
        &chunk[payload_start..]
    }

    #[test]
    fn deleting_a_rule_repairs_default_and_project_bindings() {
        let project = ProjectReviewKey("github:42".into());
        let state = ReviewConfigurationState {
            rules: vec![rule("a", "A"), rule("b", "B")],
            default_rule_id: Some(RuleId("a".into())),
            project_bindings: BTreeMap::from([(
                project.clone(),
                RuleBinding::Preset(RuleId("a".into())),
            )]),
            ..Default::default()
        };
        let reduction = reduce_configuration(
            &state,
            ReviewConfigAction::DeleteRule {
                id: RuleId("a".into()),
                replacement_default: Some(RuleId("b".into())),
            },
        );
        assert!(reduction.changed);
        assert_eq!(reduction.state.default_rule_id, Some(RuleId("b".into())));
        assert_eq!(
            reduction.state.project_bindings.get(&project),
            Some(&RuleBinding::UseDefault)
        );
    }

    #[test]
    fn concurrency_is_bounded() {
        let state = ReviewConfigurationState::default();
        assert!(reduce_configuration(&state, ReviewConfigAction::SetMaxConcurrentTasks(8)).changed);
        assert_eq!(
            reduce_configuration(&state, ReviewConfigAction::SetMaxConcurrentTasks(9)).output,
            Err(ReviewConfigError::InvalidConcurrency)
        );
    }

    #[test]
    fn diff_chunks_preserve_all_content() {
        // Each section fits in one chunk, so splitting happens only on
        // section boundaries and concatenation restores the exact input.
        let diff = format!(
            "diff --git a/a b/a\n{}diff --git a/b b/b\n{}",
            "+one\n".repeat(150),
            "+two\n".repeat(150)
        );
        let chunks = chunk_diff(&diff, 1024);
        assert_eq!(chunks.concat(), diff);
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn oversized_file_chunks_repeat_file_headers_on_continuations() {
        let diff = format!(
            "diff --git a/big b/big\n--- a/big\n+++ b/big\n@@ -1,0 +1,400 @@\n{}",
            "+line\n".repeat(400)
        );
        let chunks = chunk_diff(&diff, 1024);
        assert!(chunks.len() >= 2);
        assert!(chunks.iter().all(|chunk| chunk.len() <= 1024));
        let mut restored = chunks[0].clone();
        for chunk in &chunks[1..] {
            assert!(chunk.starts_with("diff --git a/big b/big\n--- a/big\n+++ b/big\n@@ -"));
            restored.push_str(continuation_payload(chunk));
        }
        assert_eq!(restored, diff);
    }

    #[test]
    fn oversized_single_line_is_cut_without_exceeding_chunk_budget() {
        let diff = format!("diff --git a/a b/a\n@@ -0,0 +1 @@\n+{}\n", "x".repeat(3000));
        let chunks = chunk_diff(&diff, 1024);
        assert!(chunks.len() >= 2);
        assert!(chunks.iter().all(|chunk| chunk.len() <= 1024));
        let mut restored = chunks[0].clone();
        for chunk in &chunks[1..] {
            assert!(chunk.starts_with("diff --git a/a b/a\n@@ -"));
            restored.push_str(continuation_payload(chunk));
        }
        assert_eq!(restored, diff);
    }

    #[test]
    fn chunking_preserves_missing_trailing_newline() {
        let diff = format!(
            "diff --git a/a b/a\n@@ -0,0 +1,221 @@\n{}+tail",
            "+pad\n".repeat(220)
        );
        let chunks = chunk_diff(&diff, 1024);
        assert!(chunks.len() >= 2);
        assert!(chunks.last().is_some_and(|chunk| chunk.ends_with("+tail")));
        let mut restored = chunks[0].clone();
        for chunk in &chunks[1..] {
            assert!(chunk.starts_with("diff --git a/a b/a\n@@ -"));
            restored.push_str(continuation_payload(chunk));
        }
        assert_eq!(restored, diff);
    }

    #[test]
    fn continuation_headers_track_the_next_diff_line() {
        let diff = format!(
            "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -10,300 +20,300 @@\n{}",
            " context\n-old\n+new\n".repeat(150)
        );
        let chunks = chunk_diff(&diff, 1024);
        assert!(chunks.len() >= 2);
        assert!(chunks[1..].iter().all(|chunk| {
            chunk
                .lines()
                .any(|line| line.starts_with("@@ -") && line.contains(" +"))
        }));
        assert!(
            chunks[1..]
                .iter()
                .any(|chunk| !chunk.contains("@@ -10,0 +20,0 @@ (continued)"))
        );
    }

    #[test]
    fn parses_fenced_review_json() {
        let result = parse_review_response(
            "```json\n{\"summary\":\"ok\",\"findings\":[],\"skipped_files\":[]}\n```",
        )
        .unwrap();
        assert_eq!(result.summary, "ok");
    }

    #[test]
    fn legacy_conversation_message_without_elapsed_time_remains_readable() {
        let message: ReviewConversationMessage = serde_json::from_str(
            r#"{"role":"assistant","content":"legacy","created_at_unix_ms":1}"#,
        )
        .expect("legacy review conversation");

        assert_eq!(message.message_id, None);
        assert_eq!(message.elapsed_ms, None);
        assert_eq!(message.prompt, None);
        assert_eq!(message.agent, None);
    }

    #[test]
    fn legacy_conversation_message_roundtrip_omits_message_identity() {
        let legacy = serde_json::json!({
            "role": "user",
            "content": "legacy",
            "created_at_unix_ms": 1
        });
        let message: ReviewConversationMessage =
            serde_json::from_value(legacy.clone()).expect("legacy review conversation");

        assert_eq!(message.message_id, None);
        assert_eq!(serde_json::to_value(message).unwrap(), legacy);
    }

    #[test]
    fn conversation_identity_roundtrip_preserves_distinct_submissions() {
        let first = ReviewConversationMessage {
            agent: None,
            message_id: Some("operation-1:user".to_owned()),
            role: ReviewConversationRole::User,
            content: "same question".to_owned(),
            created_at_unix_ms: 1,
            elapsed_ms: None,
            prompt: None,
        };
        let second = ReviewConversationMessage {
            agent: None,
            message_id: Some("operation-2:user".to_owned()),
            ..first.clone()
        };
        let messages = vec![first, second];
        let restored: Vec<ReviewConversationMessage> =
            serde_json::from_slice(&serde_json::to_vec(&messages).unwrap()).unwrap();

        assert_eq!(restored, messages);
        assert_ne!(restored[0].message_id, restored[1].message_id);
    }

    #[test]
    fn legacy_review_session_without_prompt_provenance_remains_readable() {
        let session: ReviewSession = serde_json::from_str(
            r#"{
                "task": {
                    "id": "legacy-task",
                    "target": {
                        "project_key": "github:42",
                        "project_name": "Usher",
                        "pull_request_number": "42",
                        "pull_request_title": "Legacy session",
                        "base_sha": "base",
                        "head_sha": "head",
                        "author": "octocat",
                        "source_branch": "feature",
                        "target_branch": "main",
                        "updated_at": "2026-08-19T00:00:00Z"
                    },
                    "rule_name": "Default",
                    "status": "completed",
                    "progress_percent": 100,
                    "finding_count": 0,
                    "error": "",
                    "model_name": "fixture",
                    "created_at_unix_ms": 1,
                    "updated_at_unix_ms": 2
                },
                "input": {
                    "target": {
                        "project_key": "github:42",
                        "project_name": "Usher",
                        "pull_request_number": "42",
                        "pull_request_title": "Legacy session",
                        "base_sha": "base",
                        "head_sha": "head",
                        "author": "octocat",
                        "source_branch": "feature",
                        "target_branch": "main",
                        "updated_at": "2026-08-19T00:00:00Z"
                    },
                    "model": {"provider":"fixture","base_url":"","model":"fixture"},
                    "unified_diff": "diff",
                    "unified_diff_truncated": false,
                    "repository_context": {},
                    "base_rule": null,
                    "additional_rules": [],
                    "extra_requirements": ""
                },
                "result": {"summary":"ok","findings":[],"skipped_files":[]}
            }"#,
        )
        .expect("legacy review session");

        assert_eq!(session.prompt, PromptProvenance::default());
        assert_eq!(session.task.agent, None);
        assert_eq!(session.task.target.author_name, None);
        assert_eq!(session.task.target.author_email, None);
        assert_eq!(session.input.target.author_name, None);
        assert_eq!(session.input.target.author_email, None);
    }

    #[test]
    fn review_target_round_trips_pr_author_identity() {
        let mut target = target("42");
        target.author_name = Some("Example User".into());
        target.author_email = Some("user@example.com".into());
        let json = serde_json::to_value(&target).unwrap();
        let restored: ReviewTarget = serde_json::from_value(json).unwrap();
        assert_eq!(restored, target);
    }

    fn target(number: &str) -> ReviewTarget {
        ReviewTarget {
            author_name: None,
            author_email: None,
            project_key: ProjectReviewKey("github:42".into()),
            project_name: "Usher".into(),
            pull_request_number: number.into(),
            pull_request_title: "Review scheduler".into(),
            base_sha: "base".into(),
            head_sha: format!("head-{number}"),
            author: "octocat".into(),
            source_branch: "feature".into(),
            target_branch: "main".into(),
            updated_at: "2026-08-19T00:00:00Z".into(),
        }
    }

    fn task(id: &str, number: &str) -> ReviewTaskSummary {
        ReviewTaskSummary {
            agent: None,
            id: ReviewTaskId(id.into()),
            target: target(number),
            rule_name: "Default".into(),
            model_name: "fixture".into(),
            status: ReviewTaskStatus::Queued,
            progress_percent: 0,
            completed_chunks: 0,
            total_chunks: 0,
            finding_count: 0,
            error: String::new(),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
        }
    }

    #[test]
    fn legacy_task_summary_without_chunk_counts_remains_readable() {
        let task: ReviewTaskSummary = serde_json::from_str(
            r#"{
                "id":"legacy-task",
                "target":{
                    "project_key":"github:42",
                    "project_name":"Usher",
                    "pull_request_number":"7",
                    "pull_request_title":"Legacy review",
                    "base_sha":"base",
                    "head_sha":"head"
                },
                "rule_name":"Default",
                "model_name":"fixture",
                "status":"running",
                "progress_percent":41,
                "finding_count":0,
                "error":"",
                "created_at_unix_ms":1,
                "updated_at_unix_ms":2
            }"#,
        )
        .expect("legacy task summary");

        assert_eq!(task.completed_chunks, 0);
        assert_eq!(task.total_chunks, 0);
        assert_eq!(task.progress_percent, 41);
    }

    #[test]
    fn task_hydration_preserves_tasks_created_during_startup() {
        let stored = reduce_tasks(
            &ReviewTaskState::default(),
            ReviewTaskAction::Enqueue(task("stored", "7")),
        )
        .state;
        let current = reduce_tasks(
            &ReviewTaskState::default(),
            ReviewTaskAction::Enqueue(task("live", "8")),
        )
        .state;

        let hydrated = reduce_tasks(&current, ReviewTaskAction::Hydrate(stored)).state;

        assert_eq!(
            hydrated.order,
            vec![ReviewTaskId("stored".into()), ReviewTaskId("live".into())]
        );
        assert!(hydrated.tasks.contains_key(&ReviewTaskId("stored".into())));
        assert!(hydrated.tasks.contains_key(&ReviewTaskId("live".into())));
    }

    #[test]
    fn chunk_progress_is_normalized_and_drives_percentage() {
        let queued = task("task-progress", "8");
        let state = reduce_tasks(
            &ReviewTaskState::default(),
            ReviewTaskAction::Enqueue(queued.clone()),
        )
        .state;
        let state = reduce_tasks(
            &state,
            ReviewTaskAction::Start {
                id: queued.id.clone(),
                updated_at_unix_ms: 2,
            },
        )
        .state;
        let state = reduce_tasks(
            &state,
            ReviewTaskAction::SetChunkProgress {
                id: queued.id.clone(),
                completed_chunks: 12,
                total_chunks: 28,
                updated_at_unix_ms: 3,
            },
        )
        .state;
        let running = state.tasks.get(&queued.id).expect("running task");
        assert_eq!(running.completed_chunks, 12);
        assert_eq!(running.total_chunks, 28);
        assert_eq!(running.progress_percent, 41);

        let completed = reduce_tasks(
            &state,
            ReviewTaskAction::Complete {
                id: queued.id.clone(),
                finding_count: 0,
                updated_at_unix_ms: 4,
            },
        )
        .state;
        let completed = completed.tasks.get(&queued.id).expect("completed task");
        assert_eq!(completed.completed_chunks, 28);
        assert_eq!(completed.progress_percent, 100);
    }

    #[test]
    fn duplicate_active_review_reuses_existing_task() {
        let first = task("task-1", "7");
        let state = reduce_tasks(
            &ReviewTaskState::default(),
            ReviewTaskAction::Enqueue(first.clone()),
        )
        .state;
        let duplicate = reduce_tasks(&state, ReviewTaskAction::Enqueue(task("task-2", "7")));
        assert!(!duplicate.changed);
        assert_eq!(
            duplicate.output,
            ReviewTaskActionResult::Duplicate(first.id)
        );
    }

    #[test]
    fn scheduler_applies_dynamic_limit_without_cancelling_active_work() {
        let mut scheduler = ReviewScheduler::new(3);
        for index in 0..6 {
            scheduler.enqueue(ReviewTaskId(format!("task-{index}")));
        }
        assert_eq!(scheduler.take_ready().len(), 3);
        assert_eq!(scheduler.active(), 3);
        scheduler.set_max_concurrent(1);
        assert!(scheduler.take_ready().is_empty());
        scheduler.finish();
        scheduler.finish();
        assert!(scheduler.take_ready().is_empty());
        scheduler.finish();
        assert_eq!(scheduler.take_ready().len(), 1);
        scheduler.set_max_concurrent(4);
        assert_eq!(scheduler.take_ready().len(), 2);
        assert_eq!(scheduler.active(), 3);
    }

    #[test]
    fn fixture_queue_high_water_never_exceeds_configured_limit() {
        let mut scheduler = ReviewScheduler::new(3);
        for index in 0..12 {
            scheduler.enqueue(ReviewTaskId(format!("fixture-{index}")));
        }
        let mut high_water = 0;
        while scheduler.queued() > 0 || scheduler.active() > 0 {
            let ready = scheduler.take_ready();
            high_water = high_water.max(scheduler.active());
            if ready.is_empty() && scheduler.active() == 0 {
                break;
            }
            if scheduler.active() > 0 {
                scheduler.finish();
            }
        }
        assert_eq!(high_water, 3);
        assert_eq!(scheduler.active(), 0);
        assert_eq!(scheduler.queued(), 0);
    }

    #[test]
    fn merged_results_deduplicate_findings_and_skipped_files() {
        let finding = ReviewFinding {
            severity: FindingSeverity::High,
            file: "src/lib.rs".into(),
            line: 12,
            side: FindingDiffSide::Right,
            title: "Unchecked state".into(),
            explanation: "The transition can fail.".into(),
            suggestion: "Handle the error.".into(),
        };
        let merged = merge_review_results([
            ReviewResult {
                summary: "First chunk".into(),
                findings: vec![finding.clone()],
                skipped_files: vec!["large.bin".into()],
            },
            ReviewResult {
                summary: "Second chunk".into(),
                findings: vec![finding],
                skipped_files: vec!["large.bin".into()],
            },
        ]);
        assert_eq!(merged.findings.len(), 1);
        assert_eq!(merged.skipped_files, vec!["large.bin"]);
        assert!(merged.summary.contains("First chunk"));
        assert!(merged.summary.contains("Second chunk"));
    }

    #[test]
    fn persisted_finding_without_side_defaults_to_right() {
        let finding: ReviewFinding = serde_json::from_str(
            r#"{"severity":"medium","file":"src/lib.rs","line":7,"title":"Issue","explanation":"Details","suggestion":"Fix it"}"#,
        )
        .unwrap();

        assert_eq!(finding.side, FindingDiffSide::Right);
    }

    #[test]
    fn findings_on_opposite_diff_sides_are_not_deduplicated() {
        let finding = ReviewFinding {
            severity: FindingSeverity::Medium,
            file: "src/lib.rs".into(),
            line: 7,
            side: FindingDiffSide::Left,
            title: "Replacement issue".into(),
            explanation: "Details".into(),
            suggestion: String::new(),
        };
        let mut right = finding.clone();
        right.side = FindingDiffSide::Right;

        let merged = merge_review_results([
            ReviewResult {
                findings: vec![finding],
                ..ReviewResult::default()
            },
            ReviewResult {
                findings: vec![right],
                ..ReviewResult::default()
            },
        ]);

        assert_eq!(merged.findings.len(), 2);
    }
}
