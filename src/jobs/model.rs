//! Application-wide reusable Job definitions and runtime snapshots.

use crate::{JobLogLineItem, JobStepItem};
use slint::{ModelRc, SharedString, VecModel};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobExecutionStatus {
    Queued,
    InProgress,
    Completed,
}

impl JobExecutionStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobConclusion {
    Success,
    Failure,
    Cancelled,
}

impl JobConclusion {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobStepStatus {
    Queued,
    InProgress,
    Completed,
}

impl JobStepStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }
}

/// Immutable job input. Commands stay as argv so workflow values never pass
/// through shell interpolation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JobStepDefinition {
    pub(crate) name: String,
    pub(crate) command: Vec<String>,
}

impl JobStepDefinition {
    pub(crate) fn new(name: impl Into<String>, command: Vec<String>) -> Self {
        Self {
            name: name.into(),
            command,
        }
    }

    pub(crate) fn rendered_command(&self) -> String {
        self.command.join(" ")
    }
}

/// Runtime snapshot for one definition. This is Send-safe; Slint models are
/// created only in `to_slint` on the event loop.
#[derive(Debug, Clone)]
pub(crate) struct JobStepData {
    pub(crate) definition: JobStepDefinition,
    pub(crate) status: JobStepStatus,
    pub(crate) conclusion: Option<JobConclusion>,
    pub(crate) detail: String,
    pub(crate) log: String,
    pub(crate) duration: String,
    pub(crate) default_expanded: bool,
    /// Backend-owned lazy-load latch. Raw output is collected independently,
    /// but the Slint line model is not materialized until a disclosure opens.
    pub(crate) log_requested: bool,
}

impl JobStepData {
    pub(crate) fn queued(definition: JobStepDefinition, detail: impl Into<String>) -> Self {
        Self {
            definition,
            status: JobStepStatus::Queued,
            conclusion: None,
            detail: detail.into(),
            log: String::new(),
            duration: String::new(),
            default_expanded: false,
            log_requested: false,
        }
    }

    pub(crate) fn to_slint(&self) -> JobStepItem {
        let materialize_log = self.log_requested || self.default_expanded;
        let log_lines: Vec<JobLogLineItem> = if !materialize_log || self.log.is_empty() {
            Vec::new()
        } else {
            self.log.lines().map(parse_log_line).collect()
        };
        JobStepItem {
            name: self.definition.name.as_str().into(),
            command: self.definition.rendered_command().into(),
            status: self.status.as_str().into(),
            conclusion: self
                .conclusion
                .map(JobConclusion::as_str)
                .unwrap_or_default()
                .into(),
            detail: self.detail.as_str().into(),
            // Keep the public field for declarative/static consumers without
            // copying backend output into Slint before it is requested.
            log: if materialize_log {
                self.log.as_str().into()
            } else {
                SharedString::default()
            },
            has_log: !self.log.is_empty() || self.status == JobStepStatus::InProgress,
            log_loaded: materialize_log,
            log_lines: ModelRc::new(VecModel::from(log_lines)),
            duration: self.duration.as_str().into(),
            default_expanded: self.default_expanded,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct JobSnapshot {
    pub(crate) status: JobExecutionStatus,
    pub(crate) conclusion: Option<JobConclusion>,
    pub(crate) total_duration: String,
    pub(crate) steps: Vec<JobStepData>,
}

impl JobSnapshot {
    pub(crate) fn queued(steps: Vec<JobStepData>) -> Self {
        Self {
            status: JobExecutionStatus::Queued,
            conclusion: None,
            total_duration: String::new(),
            steps,
        }
    }

    pub(crate) fn start(&mut self, steps: Vec<JobStepData>) {
        self.status = JobExecutionStatus::InProgress;
        self.conclusion = None;
        self.total_duration.clear();
        self.steps = steps;
    }

    pub(crate) fn replace_steps(&mut self, mut steps: Vec<JobStepData>) {
        preserve_log_requests(&self.steps, &mut steps);
        self.steps = steps;
    }

    pub(crate) fn request_step_log(&mut self, index: usize) -> bool {
        let Some(step) = self.steps.get_mut(index) else {
            return false;
        };
        if step.status == JobStepStatus::Queued || step.log_requested {
            return false;
        }
        step.log_requested = true;
        true
    }

    pub(crate) fn finish(
        &mut self,
        conclusion: JobConclusion,
        total_duration: String,
        steps: Vec<JobStepData>,
    ) {
        self.status = JobExecutionStatus::Completed;
        self.conclusion = Some(conclusion);
        self.total_duration = total_duration;
        let mut steps = steps;
        preserve_log_requests(&self.steps, &mut steps);
        self.steps = steps;
    }
}

fn preserve_log_requests(previous: &[JobStepData], next: &mut [JobStepData]) {
    for (next_step, previous_step) in next.iter_mut().zip(previous) {
        next_step.log_requested |= previous_step.log_requested;
    }
}

fn decode_workflow_command(value: &str) -> String {
    value
        .replace("%0D", "\r")
        .replace("%0A", "\n")
        .replace("%3A", ":")
        .replace("%2C", ",")
        .replace("%25", "%")
}

fn semantic_line(
    kind: &'static str,
    prefix: &'static str,
    text: impl Into<String>,
) -> JobLogLineItem {
    JobLogLineItem {
        kind: kind.into(),
        prefix: prefix.into(),
        text: text.into().into(),
    }
}

fn parse_workflow_annotation(line: &str) -> Option<JobLogLineItem> {
    let rest = line.strip_prefix("::")?;
    let separator = rest.find("::")?;
    let (header, message_with_separator) = rest.split_at(separator);
    let command = header.split([' ', ',']).next()?.to_ascii_lowercase();
    let message = decode_workflow_command(&message_with_separator[2..]);
    match command.as_str() {
        "notice" => Some(semantic_line("notice", "Notice: ", message)),
        "warning" => Some(semantic_line("warning", "Warning: ", message)),
        "error" => Some(semantic_line("error", "Error: ", message)),
        "debug" => Some(semantic_line("debug", "Debug: ", message)),
        _ => None,
    }
}

fn split_prefixed_line<'a>(line: &'a str, lowercase: &str, prefix: &str) -> Option<&'a str> {
    lowercase
        .strip_prefix(prefix)
        .map(|suffix| &line[line.len() - suffix.len()..])
}

fn parse_log_line(line: &str) -> JobLogLineItem {
    if let Some(annotation) = parse_workflow_annotation(line) {
        return annotation;
    }

    let trimmed = line.trim_start();
    let lowercase = trimmed.to_ascii_lowercase();
    if let Some(message) = split_prefixed_line(trimmed, &lowercase, "warning:") {
        return semantic_line("warning", "Warning:", message);
    }
    if let Some(message) = split_prefixed_line(trimmed, &lowercase, "error:") {
        return semantic_line("error", "Error:", message);
    }
    if let Some(message) = split_prefixed_line(trimmed, &lowercase, "fatal:") {
        return semantic_line("error", "Fatal:", message);
    }
    if let Some(message) = split_prefixed_line(trimmed, &lowercase, "notice:") {
        return semantic_line("notice", "Notice:", message);
    }
    if let Some(message) = split_prefixed_line(trimmed, &lowercase, "debug:") {
        return semantic_line("debug", "Debug:", message);
    }

    let embedded_kind = if lowercase.contains(": warning:") {
        "warning"
    } else if lowercase.contains(": error:") || lowercase.contains(": fatal:") {
        "error"
    } else {
        "plain"
    };
    semantic_line(embedded_kind, "", line)
}

pub(crate) fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    if total_seconds < 60 {
        format!("{total_seconds}s")
    } else {
        format!("{}m {}s", total_seconds / 60, total_seconds % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        JobConclusion, JobExecutionStatus, JobSnapshot, JobStepData, JobStepDefinition,
        format_duration, parse_log_line,
    };
    use slint::Model;
    use std::time::Duration;

    #[test]
    fn definition_preserves_argv_boundaries() {
        let definition = JobStepDefinition::new(
            "Tag",
            vec!["git".into(), "tag".into(), "release notes".into()],
        );
        assert_eq!(definition.command[2], "release notes");
    }

    #[test]
    fn duration_uses_github_compact_units() {
        assert_eq!(format_duration(Duration::from_millis(999)), "0s");
        assert_eq!(format_duration(Duration::from_secs(65)), "1m 5s");
    }

    #[test]
    fn snapshot_owns_the_complete_job_lifecycle() {
        let definition = JobStepDefinition::new("Checkout", vec!["git".into(), "status".into()]);
        let queued = JobStepData::queued(definition, "Inspect repository.");
        let mut snapshot = JobSnapshot::queued(vec![queued.clone()]);

        assert_eq!(snapshot.status, JobExecutionStatus::Queued);
        assert_eq!(snapshot.conclusion, None);

        snapshot.start(vec![queued.clone()]);
        assert_eq!(snapshot.status, JobExecutionStatus::InProgress);

        snapshot.finish(JobConclusion::Success, "2s".into(), vec![queued]);
        assert_eq!(snapshot.status, JobExecutionStatus::Completed);
        assert_eq!(snapshot.conclusion, Some(JobConclusion::Success));
        assert_eq!(snapshot.total_duration, "2s");
    }

    #[test]
    fn log_lines_follow_github_annotation_levels() {
        let warning = parse_log_line("::warning file=src/lib.rs,line=4::unsafe%20fallback");
        assert_eq!(warning.kind.as_str(), "warning");
        assert_eq!(warning.prefix.as_str(), "Warning: ");
        assert_eq!(warning.text.as_str(), "unsafe%20fallback");

        let error = parse_log_line("error: command failed");
        assert_eq!(error.kind.as_str(), "error");
        assert_eq!(error.prefix.as_str(), "Error:");
        assert_eq!(error.text.as_str(), " command failed");

        assert_eq!(
            parse_log_line("the warning threshold is 4").kind.as_str(),
            "plain"
        );
    }

    #[test]
    fn log_model_is_materialized_only_after_request() {
        let mut step = JobStepData::queued(
            JobStepDefinition::new("Build", vec!["cargo".into(), "build".into()]),
            "Build project.",
        );
        step.status = super::JobStepStatus::Completed;
        step.log = "warning: cached dependency".into();

        assert_eq!(step.to_slint().log_lines.row_count(), 0);
        step.log_requested = true;
        let item = step.to_slint();
        assert_eq!(item.log_lines.row_count(), 1);
        assert_eq!(item.log_lines.row_data(0).unwrap().kind.as_str(), "warning");
    }
}
