//! Retired sessions remain owned until the asynchronous shutdown barrier.
use super::session::PtySession;

#[derive(Default)]
pub(super) struct SessionRetirement {
    pub(super) closing: bool,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl SessionRetirement {
    pub(super) fn retire(&mut self, mut session: PtySession, reason: &'static str) {
        self.tasks.retain(|task| !task.is_finished());
        self.tasks.push(
            crate::app_runtime::shared_runtime().spawn_blocking(move || {
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.terminate()))
                    .is_err()
                {
                    log::error!("[Terminal] PTY termination panicked ({reason})");
                }
            }),
        );
    }

    pub(super) fn take_tasks(&mut self) -> Vec<tokio::task::JoinHandle<()>> {
        std::mem::take(&mut self.tasks)
    }
}

pub(super) async fn drain(shared: super::SharedTerminalState) {
    let tasks = {
        let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
        state.retirement.closing = true;
        super::terminate_session_locked(&mut state);
        super::reset_terminal_view_state(&mut state);
        state.retirement.take_tasks()
    };
    for task in tasks {
        if let Err(error) = task.await {
            log::error!("[Terminal] PTY termination worker failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;

    #[test]
    fn shutdown_waits_for_retired_sessions_without_holding_presentation_lock() {
        let shared = Arc::new(Mutex::new(super::super::TerminalDrawerState::default()));
        let (release, gate) = mpsc::channel();
        shared.lock().unwrap().retirement.tasks.push(
            crate::app_runtime::shared_runtime().spawn_blocking(move || {
                gate.recv().unwrap();
            }),
        );
        let state = shared.clone();
        let (done, finished) = mpsc::channel();
        crate::app_runtime::shared_runtime().spawn(async move {
            drain(state).await;
            done.send(()).unwrap();
        });
        assert!(finished.recv_timeout(Duration::from_millis(30)).is_err());
        let started = std::time::Instant::now();
        loop {
            if shared
                .try_lock()
                .is_ok_and(|state| state.retirement.closing)
            {
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(2));
            std::thread::yield_now();
        }
        release.send(()).unwrap();
        finished.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(shared.lock().unwrap().retirement.tasks.is_empty());
        crate::app_runtime::shared_runtime().block_on(drain(shared.clone()));
        i_slint_backend_testing::init_no_event_loop();
        let ui = crate::MainWindow::new().unwrap();
        super::super::ensure_session_started(&ui, shared.clone());
        assert!(shared.lock().unwrap().session.is_none());
    }
}
