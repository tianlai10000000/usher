//! Priority ordering and shared completion of one cleanup phase.
use super::super::CleanupFn;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Condvar, Mutex};
use std::time::Instant;

pub(in crate::shutdown) struct CleanupTask {
    pub(in crate::shutdown) name: &'static str,
    pub(in crate::shutdown) priority: i32,
    pub(in crate::shutdown) order: u64,
    pub(in crate::shutdown) callback: CleanupFn,
}

#[derive(Copy, Clone)]
pub(in crate::shutdown) enum ShutdownStage {
    Pre,
    Background,
    Ui,
    Post,
}

impl ShutdownStage {
    pub(in crate::shutdown) fn as_str(self) -> &'static str {
        match self {
            Self::Pre => "pre_shutdown",
            Self::Background => "background_shutdown",
            Self::Ui => "ui_shutdown",
            Self::Post => "post_shutdown",
        }
    }
}

enum PhaseState {
    Pending(Vec<CleanupTask>),
    Running(std::thread::ThreadId),
    Complete,
}

pub(super) struct Phase {
    stage: ShutdownStage,
    state: Mutex<PhaseState>,
    complete: Condvar,
}

impl Phase {
    pub(super) fn is_complete(&self) -> bool {
        matches!(
            *self.state.lock().unwrap_or_else(|p| p.into_inner()),
            PhaseState::Complete
        )
    }
    pub(super) fn is_current_owner(&self) -> bool {
        matches!(*self.state.lock().unwrap_or_else(|p| p.into_inner()),
            PhaseState::Running(owner) if owner == std::thread::current().id())
    }

    pub(super) fn new(stage: ShutdownStage, tasks: Vec<CleanupTask>) -> Self {
        Self {
            stage,
            state: Mutex::new(PhaseState::Pending(tasks)),
            complete: Condvar::new(),
        }
    }

    pub(super) fn run(&self) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            match &*state {
                PhaseState::Complete => return,
                PhaseState::Running(_) => {
                    state = self.complete.wait(state).unwrap_or_else(|p| p.into_inner());
                }
                PhaseState::Pending(_) => break,
            }
        }
        let PhaseState::Pending(mut tasks) = std::mem::replace(
            &mut *state,
            PhaseState::Running(std::thread::current().id()),
        ) else {
            unreachable!()
        };
        drop(state);
        run_stage_tasks(self.stage, &mut tasks);
        *self.state.lock().unwrap_or_else(|p| p.into_inner()) = PhaseState::Complete;
        self.complete.notify_all();
    }
}

pub(super) fn run_stage_tasks(stage: ShutdownStage, tasks: &mut Vec<CleanupTask>) {
    let started = Instant::now();
    tasks.sort_by(|a, b| b.priority.cmp(&a.priority).then(b.order.cmp(&a.order)));
    log::info!(
        "[Shutdown] Stage '{}' started (tasks: {})",
        stage.as_str(),
        tasks.len()
    );
    for task in tasks.drain(..) {
        let task_start = Instant::now();
        log::info!(
            "[Shutdown] Stage '{}' task '{}' started (priority: {}, order: {})",
            stage.as_str(),
            task.name,
            task.priority,
            task.order
        );
        if catch_unwind(AssertUnwindSafe(|| (task.callback)())).is_err() {
            log::error!(
                "[Shutdown] Stage '{}' task '{}' panicked after {} ms",
                stage.as_str(),
                task.name,
                task_start.elapsed().as_millis()
            );
        } else {
            log::info!(
                "[Shutdown] Stage '{}' task '{}' finished in {} ms",
                stage.as_str(),
                task.name,
                task_start.elapsed().as_millis()
            );
        }
    }
    log::info!(
        "[Shutdown] Stage '{}' finished in {} ms",
        stage.as_str(),
        started.elapsed().as_millis()
    );
}
