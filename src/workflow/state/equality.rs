//! Semantic snapshot equality; derived log caches do not change state identity.
use super::{WorkflowStore, WorkflowViewState};
macro_rules! fields_equal {
    ($a:expr, $b:expr; $($field:ident),+ $(,)?) => {
        true $(&& $a.$field == $b.$field)+
    };
}
impl PartialEq for WorkflowViewState {
    fn eq(&self, other: &Self) -> bool {
        fields_equal!(
            self, other;
            branch_result, tag_result, tag_selected_tab, tag_mode,
            tag_version_mode, tag_current_version, tag_next_version,
            tag_remove_target, tag_description, tag_run_generation,
            tag_add_initializing, tag_remove_initializing,
            tag_add_ready, tag_remove_ready,
            tag_add_request_seq, tag_remove_request_seq, tag_running, cd,
        ) && fields_equal!(self.tag_job, other.tag_job; status, conclusion, total_duration)
            && self.tag_job.steps.len() == other.tag_job.steps.len()
            && self
                .tag_job
                .steps
                .iter()
                .zip(&other.tag_job.steps)
                .all(|(a, b)| {
                    fields_equal!(
                        a, b;
                        definition, status, conclusion, detail, log, duration,
                        default_expanded, log_requested,
                    )
                })
            && fields_equal!(
                self.job_run, other.job_run;
                active, source, run_title, workflow_name,
                context_name, job_name, trigger_detail,
            )
    }
}
impl PartialEq for WorkflowStore {
    fn eq(&self, other: &Self) -> bool {
        fields_equal!(self, other; current_project, identity, cd_generation, projects)
    }
}
