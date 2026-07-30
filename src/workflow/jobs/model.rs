use crate::JobStepItem;
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
        }
    }

    pub(crate) fn to_slint(&self) -> JobStepItem {
        let log_lines: Vec<SharedString> = if self.log.is_empty() {
            Vec::new()
        } else {
            self.log.lines().map(SharedString::from).collect()
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
            log: self.log.as_str().into(),
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

    pub(crate) fn finish(
        &mut self,
        conclusion: JobConclusion,
        total_duration: String,
        steps: Vec<JobStepData>,
    ) {
        self.status = JobExecutionStatus::Completed;
        self.conclusion = Some(conclusion);
        self.total_duration = total_duration;
        self.steps = steps;
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
        format_duration,
    };
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
}
