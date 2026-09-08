//! Application-wide reusable Job definitions and runtime snapshots.

#[cfg(test)]
use super::log_model::LogModel;
use super::log_model::LogSnapshot;
#[cfg(test)]
use super::log_model::parse_log_line;
use crate::i18n::LocalizedText;
use crate::{JobLogLineItem, JobStepItem};
use slint::{ModelRc, SharedString};
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
    pub(crate) name: LocalizedText,
    pub(crate) command: Vec<String>,
}

impl JobStepDefinition {
    pub(crate) fn new(name: impl Into<LocalizedText>, command: Vec<String>) -> Self {
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
/// created only by the presenter on the event loop.
#[derive(Debug, Clone)]
pub(crate) struct JobStepData {
    pub(crate) definition: JobStepDefinition,
    pub(crate) status: JobStepStatus,
    pub(crate) conclusion: Option<JobConclusion>,
    pub(crate) detail: LocalizedText,
    pub(crate) log: String,
    pub(crate) parsed_log: LogSnapshot,
    pub(crate) duration: Option<Duration>,
    pub(crate) default_expanded: bool,
    /// Backend-owned lazy-load latch. Raw output is collected independently,
    /// but the Slint line model is not materialized until a disclosure opens.
    pub(crate) log_requested: bool,
}

impl JobStepData {
    pub(crate) fn queued(definition: JobStepDefinition, detail: impl Into<LocalizedText>) -> Self {
        Self {
            definition,
            status: JobStepStatus::Queued,
            conclusion: None,
            detail: detail.into(),
            log: String::new(),
            parsed_log: LogSnapshot::default(),
            duration: None,
            default_expanded: false,
            log_requested: false,
        }
    }

    /// Called by the feature worker when raw output changes, never by scrolling.
    pub(crate) fn set_log(&mut self, log: String) {
        if self.log == log {
            return;
        }
        self.parsed_log = self.parsed_log.update(&log);
        self.log = log;
    }

    pub(crate) fn start(&mut self, detail: impl Into<LocalizedText>) {
        self.status = JobStepStatus::InProgress;
        self.conclusion = None;
        self.detail = detail.into();
        self.duration = None;
        self.default_expanded = false;
    }

    pub(crate) fn finish(
        &mut self,
        conclusion: JobConclusion,
        duration: Option<Duration>,
        detail: impl Into<LocalizedText>,
        default_expanded: bool,
    ) {
        self.status = JobStepStatus::Completed;
        self.conclusion = Some(conclusion);
        self.detail = detail.into();
        self.duration = duration;
        self.default_expanded = default_expanded;
    }

    fn formatted_duration(&self) -> String {
        self.duration.map(format_duration).unwrap_or_else(|| {
            if self.status == JobStepStatus::InProgress {
                "…".to_string()
            } else {
                String::new()
            }
        })
    }

    #[cfg(test)]
    pub(crate) fn to_slint(&self) -> JobStepItem {
        let snapshot = if self.log_requested || self.default_expanded {
            self.parsed_log.clone()
        } else {
            LogSnapshot::default()
        };
        self.to_slint_with_lines(ModelRc::new(LogModel::new(snapshot)))
    }
    pub(super) fn to_slint_with_lines(&self, log_lines: ModelRc<JobLogLineItem>) -> JobStepItem {
        let materialize_log = self.log_requested || self.default_expanded;
        JobStepItem {
            name: self.definition.name.resolve().into(),
            command: self.definition.rendered_command().into(),
            status: self.status.as_str().into(),
            conclusion: self
                .conclusion
                .map(JobConclusion::as_str)
                .unwrap_or_default()
                .into(),
            detail: self.detail.resolve().into(),
            // Keep the public field for declarative/static consumers without
            // copying backend output into Slint before it is requested.
            log: if materialize_log {
                self.log.as_str().into()
            } else {
                SharedString::default()
            },
            has_log: !self.log.is_empty() || self.status == JobStepStatus::InProgress,
            log_loaded: materialize_log,
            log_lines,
            duration: self.formatted_duration().into(),
            default_expanded: self.default_expanded,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct JobSnapshot {
    pub(crate) status: JobExecutionStatus,
    pub(crate) conclusion: Option<JobConclusion>,
    pub(crate) total_duration: Option<Duration>,
    pub(crate) steps: Vec<JobStepData>,
}

impl JobSnapshot {
    pub(crate) fn queued(steps: Vec<JobStepData>) -> Self {
        Self {
            status: JobExecutionStatus::Queued,
            conclusion: None,
            total_duration: None,
            steps,
        }
    }

    pub(crate) fn start(&mut self, steps: Vec<JobStepData>) {
        self.status = JobExecutionStatus::InProgress;
        self.conclusion = None;
        self.total_duration = None;
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
        total_duration: Duration,
        steps: Vec<JobStepData>,
    ) {
        self.status = JobExecutionStatus::Completed;
        self.conclusion = Some(conclusion);
        self.total_duration = Some(total_duration);
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

        snapshot.finish(JobConclusion::Success, Duration::from_secs(2), vec![queued]);
        assert_eq!(snapshot.status, JobExecutionStatus::Completed);
        assert_eq!(snapshot.conclusion, Some(JobConclusion::Success));
        assert_eq!(snapshot.total_duration, Some(Duration::from_secs(2)));

        snapshot.start(snapshot.steps.clone());
        assert_eq!(snapshot.status, JobExecutionStatus::InProgress);
        assert_eq!(snapshot.conclusion, None);
        assert_eq!(snapshot.total_duration, None);
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
        step.finish(
            JobConclusion::Success,
            Some(Duration::from_secs(1)),
            "Build complete.",
            false,
        );
        step.set_log("warning: cached dependency".into());

        assert_eq!(step.to_slint().log_lines.row_count(), 0);
        step.log_requested = true;
        let item = step.to_slint();
        assert_eq!(item.log_lines.row_count(), 1);
        assert_eq!(item.log_lines.row_data(0).unwrap().kind.as_str(), "warning");
    }

    #[test]
    fn step_lifecycle_projects_only_semantic_duration_values() {
        let mut step = JobStepData::queued(
            JobStepDefinition::new("Build", vec!["cargo".into(), "build".into()]),
            "Waiting.",
        );
        assert_eq!(step.duration, None);
        assert_eq!(step.to_slint().duration, "");

        step.start("Building.");
        assert_eq!(step.status, super::JobStepStatus::InProgress);
        assert_eq!(step.conclusion, None);
        assert_eq!(step.duration, None);
        assert_eq!(step.to_slint().duration, "…");

        step.finish(
            JobConclusion::Failure,
            Some(Duration::from_millis(999)),
            "Build failed.",
            true,
        );
        assert_eq!(step.status, super::JobStepStatus::Completed);
        assert_eq!(step.conclusion, Some(JobConclusion::Failure));
        assert_eq!(step.to_slint().duration, "0s");
        assert!(step.default_expanded);

        step.finish(
            JobConclusion::Cancelled,
            None,
            "Cancelled before start.",
            false,
        );
        assert_eq!(step.to_slint().duration, "");
        assert!(!step.default_expanded);
    }

    #[test]
    fn set_log_keeps_raw_and_parsed_snapshots_consistent() {
        let mut step = JobStepData::queued(JobStepDefinition::new("Build", vec![]), "Waiting.");
        step.set_log("first\nwarning: retained".into());

        assert_eq!(step.log, "first\nwarning: retained");
        step.log_requested = true;
        let item = step.to_slint();
        assert_eq!(item.log_lines.row_count(), 2);
        assert_eq!(item.log_lines.row_data(0).unwrap().text, "first");
        assert_eq!(item.log_lines.row_data(1).unwrap().kind, "warning");
    }
}
