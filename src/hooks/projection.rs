//! Worker-owned Job projection, retaining parsed rows for one selected live tail.
use super::*;
use crate::{
    i18n,
    jobs::{JobConclusion, JobRunDescriptor, JobSnapshot, JobStepData, JobStepDefinition},
};
use std::time::Duration;

pub(super) struct RunProjection {
    pub id: String,
    pub seq: u64,
    pub full_log: bool,
    pub descriptor: JobRunDescriptor,
    pub job: JobSnapshot,
    pub detail: i18n::LocalizedText,
    pub event: String,
    pub test: bool,
    pub running: bool,
}

#[derive(Default)]
pub(super) struct ProjectionCache(Option<(String, JobStepData)>);
impl ProjectionCache {
    pub fn prepare(&mut self, run: &HookRun) -> RunProjection {
        let previous = self
            .0
            .take()
            .filter(|(id, _)| id == &run.id)
            .map(|(_, step)| step);
        let projection = project(run, previous);
        self.0 = Some((run.id.clone(), projection.job.steps[0].clone()));
        projection
    }
}

pub(super) fn project_run(run: &HookRun) -> RunProjection {
    project(run, None)
}

fn project(run: &HookRun, previous: Option<JobStepData>) -> RunProjection {
    super::benchmark::record_projection();
    let mut step = JobStepData::queued(
        JobStepDefinition::new(run.name.clone(), Vec::new()),
        i18n::LocalizedText::message("hooks.queued"),
    );
    if let Some(previous) = previous {
        step.log = previous.log;
        step.parsed_log = previous.parsed_log;
    }
    if run.started_at_ms.is_some() {
        step.start(i18n::LocalizedText::message("hooks.running"));
    }
    step.set_log(run.log.as_ref().clone());
    step.log_requested = true;
    let mut job = JobSnapshot::queued(Vec::new());
    if let Some(conclusion) = run.conclusion {
        let conclusion = match conclusion {
            HookConclusion::Success => JobConclusion::Success,
            HookConclusion::Cancelled | HookConclusion::Skipped => JobConclusion::Cancelled,
            _ => JobConclusion::Failure,
        };
        let duration = Duration::from_millis(
            run.finished_at_ms
                .unwrap_or(run.created_at_ms)
                .saturating_sub(run.started_at_ms.unwrap_or(run.created_at_ms))
                .max(0) as u64,
        );
        step.finish(conclusion, Some(duration), run.detail_text(), true);
        job.finish(conclusion, duration, vec![step]);
    } else if run.started_at_ms.is_some() {
        job.start(vec![step]);
    } else {
        job.replace_steps(vec![step]);
    }
    RunProjection {
        id: run.id.clone(),
        seq: run.seq,
        full_log: false,
        descriptor: JobRunDescriptor {
            active: true,
            source: "hooks".into(),
            run_title: run.name.clone().into(),
            workflow_name: "Hooks".into(),
            context_name: run.project.clone().unwrap_or_else(|| "Usher".into()),
            job_name: run.name.clone(),
            trigger_detail: i18n::LocalizedText::empty(),
        },
        job,
        detail: run.detail_text(),
        event: run.event.clone(),
        test: run.test,
        running: run.conclusion.is_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn running_job_refreshes_locale_without_rebuilding_log_models() {
        use slint::{ComponentHandle, Model};
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = crate::MainWindow::new().unwrap();
        let mut run = HookRun::queued(
            &HookDefinition::default(),
            &HookTrigger::new(PR_REVIEW_APPROVED, "test", serde_json::json!({})),
        );
        run.started_at_ms = Some(now_ms());
        run.log = Arc::new("retained output\n".into());
        let projection = project_run(&run);
        crate::jobs::apply_job_run(
            &ui,
            &projection.descriptor,
            &projection.job,
            &"".into(),
            true,
        );
        let original = ui.get_job_run_steps();
        let lines = original.row_data(0).unwrap().log_lines;
        for locale in ["en", "zh-CN", "en"] {
            crate::i18n::apply_preference(locale).unwrap();
            super::super::bridge::refresh_job_copy(&ui, &run);
            let current = ui.get_job_run_steps();
            assert!(std::ptr::eq(original.as_any(), current.as_any()));
            assert!(std::ptr::eq(
                lines.as_any(),
                current.row_data(0).unwrap().log_lines.as_any()
            ));
            assert_eq!(
                current.row_data(0).unwrap().detail.as_str(),
                crate::i18n::tr("hooks.running").as_ref()
            );
        }
    }
    #[test]
    fn selected_log_reuses_rows_but_switching_run_drops_previous_parser_state() {
        let mut run = HookRun::queued(
            &HookDefinition::default(),
            &HookTrigger::new(PR_REVIEW_APPROVED, "test", serde_json::json!({})),
        );
        run.log = Arc::new("first\nsecond\n".into());
        let mut cache = ProjectionCache::default();
        let first = cache.prepare(&run);
        run.seq += 1;
        run.log = Arc::new("first\nchanged\n".into());
        let second = cache.prepare(&run);
        assert_eq!(
            first.job.steps[0]
                .parsed_log
                .shared_rows(&second.job.steps[0].parsed_log),
            1
        );
        run.conclusion = Some(HookConclusion::Success);
        run.finished_at_ms = Some(now_ms());
        let finished = cache.prepare(&run);
        assert_eq!(
            second.job.steps[0]
                .parsed_log
                .shared_rows(&finished.job.steps[0].parsed_log),
            2
        );
        assert_eq!(finished.job.conclusion, Some(JobConclusion::Success));
        run.id = new_id("run");
        let switched = cache.prepare(&run);
        assert_eq!(
            finished.job.steps[0]
                .parsed_log
                .shared_rows(&switched.job.steps[0].parsed_log),
            0
        );
    }
}
