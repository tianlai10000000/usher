//! Opt-in, offline Job fixture shared by the before/after Release executables.
use super::controller::{run_command, set_step_log};
use crate::jobs::{
    JobConclusion, JobRunDescriptor, JobSnapshot, JobStepData, JobStepDefinition, JobStepStatus,
};
use crate::{AppState, MainWindow, Route};
use slint::{ComponentHandle, Global, Model};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

pub(super) fn setup(ui: &MainWindow) {
    if std::env::var("USHER_P5_BENCHMARK").as_deref() != Ok("58")
        || std::env::var("USHER_BENCHMARK_OFFLINE").as_deref() != Ok("1")
        || std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_none()
    {
        return;
    }
    AppState::get(ui).set_benchmark_job_log_created_rows(0);
    let cancel = Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    crate::jobs::register_job_run_source(
        "p5",
        move || {
            stop.store(true, Ordering::Release);
        },
        |_| {},
    );
    let window = ui.as_weak();
    crate::startup::run_after_first_frame(move || {
        crate::app_runtime::shared_runtime().spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let python = std::env::var("USHER_P5_PYTHON").unwrap_or_else(|_| "/usr/bin/python3".into());
            let script = r#"import os,time,threading
os.write(1,b'x'*(4*1024*1024)+b'\n')
def emit(fd):
    for i in range(16):
        data = b'x\n'*256 if fd == 1 else '::warning::中文 stream\n'.encode()
        os.write(fd,data+('TAIL-%s-%s\n'%(fd,i)).encode())
        time.sleep(.25)
a=threading.Thread(target=emit,args=(1,));b=threading.Thread(target=emit,args=(2,))
a.start();b.start();a.join();b.join()
"#;
            let command = vec![python.clone(), "-c".into(), script.into()];
            let mut step = JobStepData::queued(JobStepDefinition::new("Output stress", vec!["fixture".into()]), "Bounded output");
            step.status = JobStepStatus::InProgress;
            step.default_expanded = true;
            step.log_requested = true;
            let mut snapshot = JobSnapshot::queued(vec![step]);
            snapshot.start(snapshot.steps.clone());
            let started = Instant::now();
            let output = run_command(command, &cancel, |log| {
                set_step_log(&mut snapshot.steps[0], log);
                publish(window.clone(), snapshot.clone(), true);
            }).await;
            let output_ok = output.as_ref().is_ok_and(|log| log.contains("TAIL-1-15") && log.contains("TAIL-2-15"));
            let token = Arc::new(AtomicBool::new(false));
            let trigger = token.clone();
            tokio::spawn(async move { tokio::time::sleep(Duration::from_millis(250)).await; trigger.store(true, Ordering::Release); });
            let cancel_started = Instant::now();
            let cancelled = run_command(vec![python, "-c".into(), "import os,time\nwhile True:\n os.write(1,b'cancel-log\\n'*512);time.sleep(.01)".into()], &token, |_| {}).await;
            let cancel_ok = cancelled.as_ref().is_err_and(|log| log.contains("Cancelled by user."));
            let cancel_ms = cancel_started.elapsed().as_millis();
            snapshot.steps[0].status = JobStepStatus::Completed;
            snapshot.steps[0].conclusion = Some(JobConclusion::Success);
            snapshot.finish(JobConclusion::Success, "5s".into(), snapshot.steps.clone());
            publish(window.clone(), snapshot, false);
            tokio::time::sleep(Duration::from_secs(2)).await;
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = window.upgrade() {
                    let item = ui.get_job_run_steps().row_data(0).unwrap();
                    let tail = item.log.contains("TAIL-1-15") && item.log.contains("TAIL-2-15");
                    log::info!("[P5][58] result output_ok={output_ok} cancel_ok={cancel_ok} tail={tail} cancel_ms={cancel_ms} rows={} created={} elapsed_ms={}", item.log_lines.row_count(), AppState::get(&ui).get_benchmark_job_log_created_rows(), started.elapsed().as_millis());
                    if output_ok && cancel_ok && tail { log::info!("[P5][58] complete"); }
                }
            });
        });
    });
}
fn publish(window: slint::Weak<MainWindow>, snapshot: JobSnapshot, running: bool) {
    let began = Instant::now();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = window.upgrade() {
            let descriptor = JobRunDescriptor {
                active: true,
                source: "p5".into(),
                ..Default::default()
            };
            crate::jobs::apply_job_run(
                &ui,
                &descriptor,
                &snapshot,
                &"P5 Job output".into(),
                running,
            );
            ui.set_current_route(Route::JobRun);
            log::info!("[P5][58] ui_commit_us={}", began.elapsed().as_micros());
        }
    });
}
