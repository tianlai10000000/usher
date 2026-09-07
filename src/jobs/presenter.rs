//! Stable Step and nested log models for the currently presented Job source.
use super::log_model::{LogModel, LogSnapshot};
use super::{JobSnapshot, JobStepItem};
use crate::MainWindow;
use slint::{Model, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

struct View {
    source: String,
    rows: Rc<VecModel<JobStepItem>>,
    logs: Vec<Rc<LogModel>>,
}
thread_local! { static VIEW: RefCell<Option<View>> = const { RefCell::new(None) }; }
pub(super) fn apply(ui: &MainWindow, source: &str, snapshot: &JobSnapshot) {
    VIEW.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_none_or(|view| view.source != source) {
            *slot = Some(View {
                source: source.into(),
                rows: Rc::new(VecModel::default()),
                logs: Vec::new(),
            });
        }
        let view = slot.as_mut().unwrap();
        while view.rows.row_count() > snapshot.steps.len() {
            view.rows.remove(view.rows.row_count() - 1);
        }
        view.logs.truncate(snapshot.steps.len());
        for (index, step) in snapshot.steps.iter().enumerate() {
            let parsed = if step.log_requested || step.default_expanded {
                step.parsed_log.clone()
            } else {
                LogSnapshot::default()
            };
            if index == view.logs.len() {
                view.logs.push(Rc::new(LogModel::new(parsed)));
            } else {
                view.logs[index].update(parsed);
            }
            let item = step.to_slint_with_lines(view.logs[index].clone().into());
            if index == view.rows.row_count() {
                view.rows.push(item);
            } else if view.rows.row_data(index).as_ref() != Some(&item) {
                view.rows.set_row_data(index, item);
            }
        }
        ui.set_job_run_steps(view.rows.clone().into());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::{JobStepData, JobStepDefinition};
    #[test]
    fn progress_retains_outer_and_log_model_identity() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let mut step = JobStepData::queued(JobStepDefinition::new("Build", vec![]), "");
        step.log_requested = true;
        step.set_log("first\n".into());
        let mut snapshot = JobSnapshot::queued(vec![step]);
        apply(&ui, "test", &snapshot);
        let first = ui.get_job_run_steps();
        let lines = first.row_data(0).unwrap().log_lines;
        snapshot.steps[0].set_log("first\nwarning: next\n".into());
        apply(&ui, "test", &snapshot);
        let next = ui.get_job_run_steps();
        assert!(first == next);
        assert!(lines == next.row_data(0).unwrap().log_lines);
        assert_eq!(lines.row_count(), 2);
        assert_eq!(lines.row_data(1).unwrap().kind, "warning");
    }
}
