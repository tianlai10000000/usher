//! Explicit offline fixtures; never runs in ordinary application sessions.
use super::*;
use crate::{MainWindow, Route};
use slint::ComponentHandle;
use std::time::{Duration, Instant};

pub(super) fn setup(ui: &MainWindow, service: Arc<HookService>) {
    let batch = std::env::var("USHER_P6_BENCHMARK").unwrap_or_default();
    if !matches!(batch.as_str(), "68" | "69")
        || !crate::app_paths::benchmark_offline_mode()
        || std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_none()
    {
        return;
    }
    let weak = ui.as_weak();
    crate::startup::run_after_first_frame(move || {
        slint::Timer::single_shot(Duration::from_secs(2), move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.window().set_size(slint::LogicalSize::new(1100., 700.));
            heartbeat(weak.clone(), batch.clone(), Instant::now(), Instant::now());
            if batch == "69" {
                crate::app_runtime::shared_runtime().spawn(state_workload(service, weak));
                return;
            }
            crate::app_runtime::shared_runtime().spawn(async move {
                for cycle in 0.. {
                    let start = Instant::now();
                    let run = runner::benchmark_journal(&service).await;
                    let id = run.id.clone();
                    let weak = weak.clone();
                    let batch = batch.clone();
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            for _ in 0..12 {
                                bridge::open_target(&ui, &format!("hook-run:{id}"));
                            }
                            await_replay(weak, batch, id, cycle, start, tx);
                        }
                    });
                    if rx.await.is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    if !crate::p6_benchmark::next_cycle(cycle + 1) {
                        break;
                    }
                }
                log::info!("[P6][{batch}] complete");
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.invoke_navigate(Route::Project);
                    }
                });
            });
        });
    });
}

static WRITES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static PROJECTIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub(super) fn record_write() {
    if enabled_state_fixture() {
        WRITES.fetch_add(1, Ordering::Relaxed);
    }
}
pub(super) fn record_projection() {
    if enabled_state_fixture() {
        PROJECTIONS.fetch_add(1, Ordering::Relaxed);
    }
}
fn enabled_state_fixture() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("USHER_P6_BENCHMARK").as_deref() == Ok("69")
            && crate::app_paths::benchmark_offline_mode()
    })
}

async fn flush(service: Arc<HookService>) {
    tokio::task::spawn_blocking(move || service.state.flush().unwrap())
        .await
        .unwrap();
}

async fn state_workload(service: Arc<HookService>, weak: slint::Weak<MainWindow>) {
    let preparation = service.clone();
    tokio::task::spawn_blocking(move || {
        for index in 0..32 {
            preparation
                .save(HookDefinition {
                    id: format!("hook-fixture-{index}"),
                    script: "#".repeat(MAX_SCRIPT_BYTES),
                    ..Default::default()
                })
                .unwrap();
        }
        let hook = preparation.state.snapshot().state().hooks[0].clone();
        for index in 0..64 {
            let mut run = HookRun::queued(
                &hook,
                &HookTrigger::new(PR_REVIEW_APPROVED, "fixture", serde_json::json!({})),
            );
            run.id = format!("run-history-{index}");
            run.conclusion = Some(HookConclusion::Success);
            run.finished_at_ms = Some(now_ms());
            preparation.state.dispatch(Action::Run(run)).unwrap();
        }
        preparation.state.flush().unwrap();
    })
    .await
    .unwrap();
    for cycle in 0.. {
        let start = Instant::now();
        let initial = service.state.snapshot();
        let mut runs = Vec::new();
        for index in 0..2 {
            let mut run = HookRun::queued(
                &initial.state().hooks[index],
                &HookTrigger::new(PR_REVIEW_APPROVED, "fixture", serde_json::json!({})),
            );
            run.started_at_ms = Some(now_ms());
            run.seq = 1;
            write_snapshot(&service, &run).await;
            service.state.dispatch(Action::Run(run.clone())).unwrap();
            runs.push(run);
        }
        flush(service.clone()).await;
        let selected = runs[0].id.clone();
        let display = weak.clone();
        let id = selected.clone();
        let (opened, opening) = tokio::sync::oneshot::channel();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = display.upgrade() {
                bridge::open_target(&ui, &format!("hook-run:{id}"));
            }
            let _ = opened.send(());
        });
        if opening.await.is_err() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        let writes = WRITES.load(Ordering::Relaxed);
        let projections = PROJECTIONS.load(Ordering::Relaxed);
        let first = runs.remove(0);
        let second = runs.remove(0);
        let (first, second) = tokio::join!(
            progress(service.clone(), first),
            progress(service.clone(), second)
        );
        flush(service.clone()).await;
        log::info!(
            "[P6][69] output_writes={} prepared={}",
            WRITES.load(Ordering::Relaxed) - writes,
            PROJECTIONS.load(Ordering::Relaxed) - projections
        );
        for mut run in [first, second] {
            run.seq += 1;
            run.finished_at_ms = Some(now_ms());
            run.conclusion = Some(HookConclusion::Success);
            write_snapshot(&service, &run).await;
            service.state.dispatch(Action::Run(run)).unwrap();
        }
        flush(service.clone()).await;
        let display = weak.clone();
        let (done, waited) = tokio::sync::oneshot::channel();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = display.upgrade() {
                bridge::open_target(&ui, &format!("hook-run:{selected}"));
            }
            await_replay(display, "69".into(), selected, cycle, start, done);
        });
        if waited.await.is_err() {
            return;
        }
        if !crate::p6_benchmark::next_cycle(cycle + 1) {
            break;
        }
    }
    log::info!("[P6][69] complete");
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.invoke_navigate(Route::Project);
        }
    });
}

async fn progress(service: Arc<HookService>, mut run: HookRun) -> HookRun {
    let initial = service.state.snapshot();
    let mut shared = 0;
    for index in 0..100 {
        run.seq += 1;
        let mut text = format!("snapshot {index}\n");
        text.push_str(&"stable line for incremental parsing\n".repeat(1000));
        text.truncate(MAX_TAIL_BYTES);
        run.log = Arc::new(text);
        service.state.dispatch(Action::Run(run.clone())).unwrap();
        if service.state.snapshot().state().hooks.as_ptr() == initial.state().hooks.as_ptr() {
            shared += 1;
        }
        if index % 10 == 9 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    log::info!("[P6][69] shared_snapshots={shared}");
    run
}

async fn write_snapshot(service: &HookService, run: &HookRun) {
    tokio::fs::create_dir_all(&service.directory).await.unwrap();
    let mut bytes = Vec::new();
    for (seq, kind, data) in [
        (
            run.seq - 1,
            "output",
            serde_json::json!({"stream": "stdout", "text": run.log.as_str()}),
        ),
        (
            run.seq,
            if run.conclusion.is_some() {
                "hook.finished"
            } else {
                "execution.started"
            },
            serde_json::json!({"conclusion": run.conclusion}),
        ),
    ] {
        let event = HookEvent {
            run_id: run.id.clone(),
            hook_id: run.hook_id.clone(),
            event_id: run.event_id.clone(),
            seq,
            time_ms: now_ms(),
            kind: kind.into(),
            data,
        };
        serde_json::to_writer(&mut bytes, &event).unwrap();
        bytes.push(b'\n');
    }
    tokio::fs::write(service.directory.join(format!("{}.jsonl", run.id)), bytes)
        .await
        .unwrap();
}

fn await_replay(
    weak: slint::Weak<MainWindow>,
    batch: String,
    id: String,
    cycle: usize,
    start: Instant,
    done: tokio::sync::oneshot::Sender<()>,
) {
    slint::Timer::single_shot(Duration::from_millis(50), move || {
        if weak.upgrade().is_none() {
            return;
        }
        if bridge::benchmark_replay_ready(&id) {
            log::info!(
                "[P6][{batch}] cycle={cycle} completed_ms={}",
                start.elapsed().as_millis()
            );
            let _ = done.send(());
        } else if start.elapsed() < Duration::from_secs(25) {
            await_replay(weak, batch, id, cycle, start, done);
        }
    });
}

fn heartbeat(weak: slint::Weak<MainWindow>, batch: String, start: Instant, last: Instant) {
    slint::Timer::single_shot(Duration::from_millis(50), move || {
        if weak.upgrade().is_none() || !crate::p6_benchmark::heartbeat_active(55) {
            return;
        }
        log::info!("[P6][{batch}] heartbeat_us={}", last.elapsed().as_micros());
        heartbeat(weak, batch, start, Instant::now());
    });
}
