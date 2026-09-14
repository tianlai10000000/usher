mod log_layout;
mod log_model;
mod model;
mod presenter;

use crate::i18n::LocalizedText;
use crate::{JobStepItem, MainWindow};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub(crate) use model::{
    JobConclusion, JobSnapshot, JobStepData, JobStepDefinition, JobStepStatus, format_duration,
};

#[derive(Debug, Clone)]
pub(crate) struct JobRunDescriptor {
    pub(crate) active: bool,
    pub(crate) source: String,
    pub(crate) run_title: LocalizedText,
    pub(crate) workflow_name: LocalizedText,
    pub(crate) context_name: String,
    pub(crate) job_name: String,
    pub(crate) trigger_detail: LocalizedText,
}

impl Default for JobRunDescriptor {
    fn default() -> Self {
        Self {
            active: false,
            source: String::new(),
            run_title: LocalizedText::message("jobs.default_run_title"),
            workflow_name: LocalizedText::message("common.workflow"),
            context_name: "Usher".to_string(),
            job_name: "job".to_string(),
            trigger_detail: LocalizedText::message("jobs.started_manually"),
        }
    }
}

type StopHandler = Rc<dyn Fn()>;
type LoadLogHandler = Rc<dyn Fn(i32)>;

#[derive(Clone)]
struct JobRunActions {
    stop: StopHandler,
    load_step_log: LoadLogHandler,
}

thread_local! {
    static JOB_RUN_ACTIONS: RefCell<HashMap<String, JobRunActions>> =
        RefCell::new(HashMap::new());
}

pub(crate) fn setup_job_run(ui: &MainWindow) {
    log_layout::setup(ui);
    let ui_weak = ui.as_weak();
    ui.on_job_run_stop(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let source = ui.get_job_run_source().to_string();
        JOB_RUN_ACTIONS.with(|registry| {
            if let Some(actions) = registry.borrow().get(&source) {
                (actions.stop)();
            } else {
                log::warn!("[Jobs] Stop ignored: no action source registered for '{source}'");
            }
        });
    });

    let ui_weak = ui.as_weak();
    ui.on_job_run_load_step_log(move |index| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let source = ui.get_job_run_source().to_string();
        JOB_RUN_ACTIONS.with(|registry| {
            if let Some(actions) = registry.borrow().get(&source) {
                (actions.load_step_log)(index);
            } else {
                log::warn!(
                    "[Jobs] Log request ignored: no action source registered for '{source}'"
                );
            }
        });
    });
}

pub(crate) fn register_job_run_source(
    source: &str,
    stop: impl Fn() + 'static,
    load_step_log: impl Fn(i32) + 'static,
) {
    JOB_RUN_ACTIONS.with(|registry| {
        registry.borrow_mut().insert(
            source.to_string(),
            JobRunActions {
                stop: Rc::new(stop),
                load_step_log: Rc::new(load_step_log),
            },
        );
    });
}

pub(crate) fn apply_job_run(
    ui: &MainWindow,
    descriptor: &JobRunDescriptor,
    snapshot: &JobSnapshot,
    summary: &LocalizedText,
    running: bool,
) {
    ui.set_job_run_active(descriptor.active);
    ui.set_job_run_source(descriptor.source.clone().into());
    ui.set_job_run_title(descriptor.run_title.resolve().into());
    ui.set_job_run_workflow_name(descriptor.workflow_name.resolve().into());
    ui.set_job_run_context_name(descriptor.context_name.clone().into());
    ui.set_job_run_job_name(descriptor.job_name.clone().into());
    ui.set_job_run_trigger_detail(descriptor.trigger_detail.resolve().into());
    // The summary occupies a fixed single-line header row. Keep multiline
    // hook errors in the expanded log instead of letting newline characters
    // paint over the title and disclosure rows.
    let summary = summary.resolve().replace(['\r', '\n'], " ");
    ui.set_job_run_summary(summary.into());
    ui.set_job_run_status(snapshot.status.as_str().into());
    ui.set_job_run_conclusion(
        snapshot
            .conclusion
            .map(JobConclusion::as_str)
            .unwrap_or_default()
            .into(),
    );
    ui.set_job_run_total_duration(
        snapshot
            .total_duration
            .map(format_duration)
            .unwrap_or_default()
            .into(),
    );
    ui.set_job_run_running(running);
    presenter::apply(ui, &descriptor.source, snapshot);
}
