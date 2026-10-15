//! Prioritized cleanup phases with an owned, shared background barrier.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use super::CleanupMode;

mod phase;
pub(super) use phase::{CleanupTask, ShutdownStage};
use phase::{Phase, run_stage_tasks};

#[derive(Default)]
pub(super) struct CleanupRegistry {
    pub(super) pre_shutdown: Vec<CleanupTask>,
    pub(super) background_shutdown: Vec<CleanupTask>,
    pub(super) ui_shutdown: Vec<CleanupTask>,
    pub(super) post_shutdown: Vec<CleanupTask>,
}

enum TailState {
    Pending(CleanupRegistry),
    Running(std::thread::ThreadId),
    Complete,
}

pub(super) struct CleanupRun {
    pre: Phase,
    background: Phase,
    tail: Mutex<TailState>,
    tail_complete: Condvar,
    started: Instant,
}

impl CleanupRun {
    pub(super) fn new(mut registry: CleanupRegistry) -> Arc<Self> {
        Arc::new(Self {
            pre: Phase::new(
                ShutdownStage::Pre,
                std::mem::take(&mut registry.pre_shutdown),
            ),
            background: Phase::new(
                ShutdownStage::Background,
                std::mem::take(&mut registry.background_shutdown),
            ),
            tail: Mutex::new(TailState::Pending(registry)),
            tail_complete: Condvar::new(),
            started: Instant::now(),
        })
    }

    /// Run short producer stops on the caller's original UI thread.
    pub(super) fn stop_producers(&self) {
        self.pre.run();
    }

    pub(super) fn is_current_owner(&self) -> bool {
        self.pre.is_current_owner()
            || self.background.is_current_owner()
            || matches!(*self.tail.lock().unwrap_or_else(|p| p.into_inner()),
                TailState::Running(owner) if owner == std::thread::current().id())
    }

    /// Every caller shares the same phase result. Dropping one waiter cannot
    /// cancel its owned blocking work or let another caller skip the barrier.
    pub(super) async fn drain(self: &Arc<Self>) {
        let run = self.clone();
        crate::app_runtime::shared_runtime()
            .spawn_blocking(move || {
                run.pre.run();
                run.background.run();
            })
            .await
            .expect("cleanup background owner panicked outside a guarded callback");
    }

    #[cfg(test)]
    pub(super) fn finish(&self, mode: CleanupMode) -> bool {
        self.finish_with(mode, || {})
    }

    /// Tail callbacks and finalization share a completion barrier too; another
    /// exit path must not return while the owner is still flushing its tail.
    pub(super) fn finish_with(&self, mode: CleanupMode, finalize: impl FnOnce()) -> bool {
        if !self.background.is_complete() {
            return false;
        }
        let mut state = self.tail.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            match &*state {
                TailState::Complete => return false,
                TailState::Running(owner) if *owner == std::thread::current().id() => return false,
                TailState::Running(_) => {
                    state = self
                        .tail_complete
                        .wait(state)
                        .unwrap_or_else(|p| p.into_inner())
                }
                TailState::Pending(_) => break,
            }
        }
        let TailState::Pending(mut registry) =
            std::mem::replace(&mut *state, TailState::Running(std::thread::current().id()))
        else {
            unreachable!()
        };
        drop(state);
        if mode.runs_ui_cleanup() {
            run_stage_tasks(ShutdownStage::Ui, &mut registry.ui_shutdown);
        } else {
            log::warn!(
                "[Shutdown] Background fallback skipped {} UI/AppKit cleanup task(s)",
                registry.ui_shutdown.len()
            );
        }
        run_stage_tasks(ShutdownStage::Post, &mut registry.post_shutdown);
        log::info!(
            "[Shutdown] Cleanup pipeline finished in {} ms",
            self.started.elapsed().as_millis()
        );
        if catch_unwind(AssertUnwindSafe(finalize)).is_err() {
            log::error!("[Shutdown] Final cleanup callback panicked");
        }
        *self.tail.lock().unwrap_or_else(|p| p.into_inner()) = TailState::Complete;
        self.tail_complete.notify_all();
        true
    }
}

#[cfg(test)]
mod tests;
