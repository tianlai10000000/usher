//! Bounded cancellable computation on the shared blocking pool.
use std::sync::{Arc, Condvar, Mutex};

trait Stop: Send + Sync {
    fn stop(&self);
}
type Workers = Mutex<Vec<std::sync::Weak<dyn Stop>>>;
static WORKERS: std::sync::OnceLock<Workers> = std::sync::OnceLock::new();
fn register(worker: Arc<dyn Stop>) {
    let workers = WORKERS.get_or_init(|| {
        crate::shutdown::register_pre_shutdown_task("cancel_latest_computations", || {
            let live: Vec<_> = WORKERS
                .get()
                .unwrap()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .filter_map(std::sync::Weak::upgrade)
                .collect();
            for worker in live {
                worker.stop();
            }
        });
        Mutex::new(Vec::new())
    });
    let mut workers = workers.lock().unwrap_or_else(|p| p.into_inner());
    workers.retain(|worker| worker.strong_count() != 0);
    workers.push(Arc::downgrade(&worker));
}

struct State<I, O> {
    generation: u64,
    pending: Option<(u64, I)>,
    output: Option<(u64, O)>,
    active: bool,
    queued: bool,
    closed: bool,
}
struct Shared<I, O> {
    state: Mutex<State<I, O>>,
    idle: Condvar,
}
impl<I: Send, O: Send> Stop for Shared<I, O> {
    fn stop(&self) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.closed = true;
        state.pending = None;
        state.output = None;
        while state.active {
            state = self.idle.wait(state).unwrap_or_else(|p| p.into_inner());
        }
    }
}
pub(crate) struct LatestCompute<I, O> {
    shared: Arc<Shared<I, O>>,
}
impl<I, O> Clone for LatestCompute<I, O> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}
pub(crate) struct ComputationToken<I, O> {
    shared: Arc<Shared<I, O>>,
    generation: u64,
}
impl<I, O> ComputationToken<I, O> {
    pub(crate) fn cancelled(&self) -> bool {
        let state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        state.closed || state.generation != self.generation
    }
}
impl<I: Send + 'static, O: Send + 'static> LatestCompute<I, O> {
    pub(crate) fn new() -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                generation: 0,
                pending: None,
                output: None,
                active: false,
                queued: false,
                closed: false,
            }),
            idle: Condvar::new(),
        });
        register(shared.clone());
        Self { shared }
    }
    pub(crate) fn submit(
        &self,
        input: I,
        compute: impl Fn(I, &ComputationToken<I, O>) -> Option<O> + Send + Sync + 'static,
        wake: impl Fn() + Send + Sync + 'static,
    ) {
        let mut state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.closed {
            return;
        }
        state.generation += 1;
        let generation = state.generation;
        state.pending = Some((generation, input));
        state.output = None;
        if state.active {
            return;
        }
        state.active = true;
        drop(state);
        let shared = self.shared.clone();
        super::shared_runtime().spawn_blocking(move || {
            loop {
                let (generation, input) = {
                    let mut state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
                    let Some(input) = state.pending.take() else {
                        state.active = false;
                        shared.idle.notify_all();
                        return;
                    };
                    input
                };
                let token = ComputationToken {
                    shared: shared.clone(),
                    generation,
                };
                let output = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    compute(input, &token)
                })) {
                    Ok(output) => output,
                    Err(_) => {
                        log::error!("[LatestCompute] computation panicked");
                        None
                    }
                };
                let notify = {
                    let mut state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
                    if !state.closed && state.generation == generation {
                        if let Some(output) = output {
                            state.output = Some((generation, output));
                            !std::mem::replace(&mut state.queued, true)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };
                if notify {
                    wake();
                }
            }
        });
    }
    #[cfg(test)]
    pub(crate) fn take(&self) -> Option<O> {
        self.acknowledge();
        self.take_output()
    }
    pub(crate) fn acknowledge(&self) {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .queued = false;
    }
    pub(crate) fn take_output(&self) -> Option<O> {
        let mut state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        let (generation, value) = state.output.take()?;
        (!state.closed && generation == state.generation).then_some(value)
    }
    pub(crate) fn cancel(&self) {
        let mut state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        state.generation += 1;
        state.pending = None;
        state.output = None;
    }
    #[cfg(test)]
    pub(crate) fn shutdown(&self) {
        self.shared.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;
    #[test]
    fn busy_worker_keeps_only_latest_input_and_shutdown_cancels() {
        let worker = LatestCompute::<usize, usize>::new();
        let (start, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let gate = Arc::new(Mutex::new(gate));
        let (notify, ready) = mpsc::channel();
        worker.submit(
            0,
            move |value, token| {
                if value == 0 {
                    start.send(()).unwrap();
                    gate.lock().unwrap().recv().unwrap();
                }
                (!token.cancelled()).then_some(value)
            },
            move || {
                notify.send(()).unwrap();
            },
        );
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        for value in 1..1000 {
            worker.submit(value, |value, _| Some(value), || {});
        }
        assert_eq!(
            worker
                .shared
                .state
                .lock()
                .unwrap()
                .pending
                .as_ref()
                .map(|p| p.1),
            Some(999)
        );
        release.send(()).unwrap();
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(worker.take(), Some(999));
        worker.shutdown();
        worker.submit(1001, |value, _| Some(value), || {});
        assert!(worker.take().is_none());
    }
    #[test]
    fn stale_queued_output_is_released_on_cancel() {
        let worker = LatestCompute::<usize, usize>::new();
        let (notify, ready) = mpsc::channel();
        worker.submit(
            1,
            |value, _| Some(value),
            move || {
                notify.send(()).unwrap();
            },
        );
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        worker.cancel();
        assert!(worker.take().is_none());
        worker.shutdown();
    }
}
