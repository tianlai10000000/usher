//! Explicit offline retention fixture through the real JobRun route and worker.
use super::*;
use crate::{JobLogLineItem, JobLogWindow, Route};
use slint::{Model, ModelRc};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub(super) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("USHER_P6_BENCHMARK").as_deref() == Ok("70")
            && crate::app_paths::benchmark_offline_mode()
            && std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_some()
    })
}

#[derive(Clone, Copy, Default)]
pub(super) struct Retained {
    pub source: usize,
    pub measurement: usize,
    pub wrapped: usize,
}

thread_local! {
    static READY: RefCell<Option<(ModelRc<JobLogLineItem>, Retained)>> = const { RefCell::new(None) };
}

pub(super) fn observe(model: &ModelRc<JobLogLineItem>, window: &JobLogWindow, bytes: Retained) {
    READY.with(|slot| {
        *slot.borrow_mut() =
            (!window.loading && window.rows.row_count() > 0).then(|| (model.clone(), bytes));
    });
}

pub(super) fn setup(ui: &MainWindow) {
    if !enabled() {
        return;
    }
    let weak = ui.as_weak();
    crate::startup::run_after_first_frame(move || {
        slint::Timer::single_shot(Duration::from_secs(2), move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.window().set_size(slint::LogicalSize::new(1100., 700.));
            heartbeat(weak.clone(), Instant::now(), Instant::now());
            open(weak, 0, 0, Instant::now());
        });
    });
}

fn heartbeat(weak: slint::Weak<MainWindow>, start: Instant, last: Instant) {
    slint::Timer::single_shot(Duration::from_millis(50), move || {
        if weak.upgrade().is_none() || !crate::p6_benchmark::heartbeat_active(85) {
            return;
        }
        log::info!("[P6][70] heartbeat_us={}", last.elapsed().as_micros());
        heartbeat(weak, start, Instant::now());
    });
}

fn open(weak: slint::Weak<MainWindow>, cycle: usize, source: usize, started: Instant) {
    crate::app_runtime::shared_runtime().spawn_blocking(move || {
        let mut log = String::new();
        for row in 0..512 {
            let prefix = if row % 7 == 0 { "::warning::" } else { "" };
            log.push_str(&format!("{prefix}cycle={cycle} source={source} row={row} "));
            log.push_str(&"value /src/path {\"key\":42} \u{6d88}\u{606f} ".repeat(64));
            log.push('\n');
        }
        let bytes = log.len();
        let mut step = JobStepData::queued(JobStepDefinition::new("Retention fixture", vec![]), "");
        step.set_log(log);
        step.log_requested = true;
        step.finish(JobConclusion::Success, None, "", true);
        let mut job = JobSnapshot::queued(Vec::new());
        job.finish(JobConclusion::Success, Duration::from_secs(1), vec![step]);
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            READY.with(|slot| slot.borrow_mut().take());
            let descriptor = JobRunDescriptor {
                active: true,
                source: format!("retention-{cycle}-{source}"),
                ..Default::default()
            };
            apply_job_run(&ui, &descriptor, &job, &LocalizedText::default(), false);
            ui.set_current_route(Route::JobRun);
            log::info!("[P6][70] source={source} raw_bytes={bytes}");
            await_layout(weak, cycle, source, started, Instant::now());
        });
    });
}

fn await_layout(
    weak: slint::Weak<MainWindow>,
    cycle: usize,
    source: usize,
    started: Instant,
    opened: Instant,
) {
    slint::Timer::single_shot(Duration::from_millis(50), move || {
        let Some(ui) = weak.upgrade() else { return };
        let item = ui.get_job_run_steps().row_data(0).unwrap();
        let ready = READY.with(|slot| {
            slot.borrow()
                .as_ref()
                .and_then(|(model, bytes)| (*model == item.log_lines).then_some(*bytes))
        });
        if let Some(bytes) = ready {
            assert_eq!(item.log_lines.row_count(), 512);
            assert!(
                item.log_lines
                    .row_data(511)
                    .unwrap()
                    .text
                    .contains(&format!("source={source} row=511"))
            );
            log::info!(
                "[P6][70] layout_ready cycle={cycle} source={source} source_bytes={} measurement_bytes={} wrapped_bytes={}",
                bytes.source,
                bytes.measurement,
                bytes.wrapped
            );
            if source < 15 {
                open(weak, cycle, source + 1, started);
            } else {
                log::info!(
                    "[P6][70] cycle={cycle} completed_ms={}",
                    started.elapsed().as_millis()
                );
                if crate::p6_benchmark::next_cycle(cycle + 1) {
                    open(weak, cycle + 1, 0, Instant::now());
                } else {
                    log::info!("[P6][70] complete");
                }
            }
        } else if opened.elapsed() < Duration::from_secs(10) {
            await_layout(weak, cycle, source, started, opened);
        } else {
            log::error!("[P6][70] layout_timeout cycle={cycle} source={source}");
        }
    });
}
