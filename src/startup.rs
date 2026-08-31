use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::{diagnostics, platform, single_instance};

#[derive(Clone)]
pub(crate) struct StartupTimeline {
    inner: Arc<Mutex<TimelineState>>,
}

struct TimelineState {
    started: Instant,
    previous: Instant,
    logger_ready: bool,
    pending: Vec<Checkpoint>,
}

struct Checkpoint {
    name: &'static str,
    elapsed: Duration,
    delta: Duration,
}

type DeferredTask = Box<dyn FnOnce() + Send + 'static>;

static FIRST_FRAME_RENDERED: AtomicBool = AtomicBool::new(false);
static AFTER_FIRST_FRAME_TASKS: OnceLock<Mutex<Vec<DeferredTask>>> = OnceLock::new();

pub(crate) fn run_after_first_frame(task: impl FnOnce() + Send + 'static) {
    if FIRST_FRAME_RENDERED.load(Ordering::Acquire) {
        defer_to_next_loop_turn(task);
        return;
    }
    let tasks = AFTER_FIRST_FRAME_TASKS.get_or_init(|| Mutex::new(Vec::new()));
    let Ok(mut tasks) = tasks.lock() else {
        defer_to_next_loop_turn(task);
        return;
    };
    if FIRST_FRAME_RENDERED.load(Ordering::Acquire) {
        drop(tasks);
        defer_to_next_loop_turn(task);
    } else {
        tasks.push(Box::new(task));
    }
}

pub(crate) fn mark_first_frame_rendered() {
    if FIRST_FRAME_RENDERED.swap(true, Ordering::AcqRel) {
        return;
    }
    let tasks = AFTER_FIRST_FRAME_TASKS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .map(|mut tasks| std::mem::take(&mut *tasks))
        .unwrap_or_default();
    for task in tasks {
        defer_to_next_loop_turn(task);
    }
}

pub(crate) fn defer_to_next_loop_turn(task: impl FnOnce() + Send + 'static) {
    if let Err(error) = slint::invoke_from_event_loop(task) {
        log::warn!("[Startup] Failed to defer event-loop work: {error}");
    }
}

pub(crate) fn initialize_process() -> Option<(StartupTimeline, diagnostics::RunGuard)> {
    let timeline = StartupTimeline::new();
    timeline.checkpoint("process_entered");
    match single_instance::acquire_instance_lock() {
        Ok(true) => timeline.checkpoint("instance_lock_acquired"),
        Ok(false) => {
            if let Err(error) = single_instance::request_existing_instance_activation() {
                eprintln!("[Usher] Failed to activate the existing instance: {error:#}");
            }
            eprintln!("[Usher] Another instance is already running, exiting this process.");
            return None;
        }
        Err(error) => {
            eprintln!("[Usher] Failed to acquire instance lock (continuing anyway): {error}");
        }
    }

    let run_guard = diagnostics::begin_run().unwrap_or_else(|error| {
        eprintln!("[Usher] Failed to initialize crash sentinel: {error:#}");
        diagnostics::RunGuard::default()
    });
    timeline.checkpoint("crash_sentinel_ready");
    Some((timeline, run_guard))
}

pub(crate) fn exit_failure(context: &str, error: &dyn std::fmt::Display) -> ! {
    let message = format!("{context}: {error}");
    eprintln!("[Usher] {message}");
    platform::show_startup_error(&message);
    let _ = single_instance::release_instance_lock();
    std::process::exit(1);
}

impl StartupTimeline {
    pub(crate) fn new() -> Self {
        let now = Instant::now();
        Self {
            inner: Arc::new(Mutex::new(TimelineState {
                started: now,
                previous: now,
                logger_ready: false,
                pending: Vec::new(),
            })),
        }
    }

    pub(crate) fn checkpoint(&self, name: &'static str) {
        let now = Instant::now();
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        let checkpoint = Checkpoint {
            name,
            elapsed: now.saturating_duration_since(state.started),
            delta: now.saturating_duration_since(state.previous),
        };
        state.previous = now;
        if state.logger_ready {
            log_checkpoint(&checkpoint);
        } else {
            state.pending.push(checkpoint);
        }
    }

    pub(crate) fn logger_ready(&self) {
        let pending = {
            let Ok(mut state) = self.inner.lock() else {
                return;
            };
            state.logger_ready = true;
            std::mem::take(&mut state.pending)
        };
        for checkpoint in pending {
            log_checkpoint(&checkpoint);
        }
    }
}

fn log_checkpoint(checkpoint: &Checkpoint) {
    log::info!(
        "[Startup][Timeline] checkpoint={} elapsed_ms={} delta_ms={}",
        checkpoint.name,
        checkpoint.elapsed.as_millis(),
        checkpoint.delta.as_millis()
    );
}

#[cfg(test)]
mod tests {
    use super::StartupTimeline;

    #[test]
    fn checkpoints_can_be_buffered_until_the_logger_is_ready() {
        let timeline = StartupTimeline::new();
        timeline.checkpoint("one");
        timeline.checkpoint("two");
        assert_eq!(timeline.inner.lock().unwrap().pending.len(), 2);

        timeline.logger_ready();
        let state = timeline.inner.lock().unwrap();
        assert!(state.logger_ready);
        assert!(state.pending.is_empty());
    }
}
