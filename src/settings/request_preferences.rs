//! Shared, automatically persisted HTTP and PR command preferences.
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use usher_state_store::{PersistentStore, Reduction, StoreListener, StoreSnapshot, Subscription};

use super::{AppSettings, get_settings_manager};
use crate::git::PullRequestMergeMethod;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestPreferences {
    pub http_request_timeout_ms: String,
    pub default_pr_merge_method: PullRequestMergeMethod,
}

impl From<&AppSettings> for RequestPreferences {
    fn from(settings: &AppSettings) -> Self {
        Self {
            http_request_timeout_ms: normalize_timeout_ms(&settings.http.request_timeout_ms)
                .unwrap_or_else(|| "30000".into()),
            default_pr_merge_method: PullRequestMergeMethod::parse(
                &settings.git.default_pr_merge_method,
            )
            .unwrap_or(PullRequestMergeMethod::Merge),
        }
    }
}

enum PreferenceAction {
    Timeout(String),
    MergeMethod(PullRequestMergeMethod),
}

#[derive(Clone)]
pub(crate) struct RequestPreferencesFacade {
    store: PersistentStore<RequestPreferences, PreferenceAction, ()>,
    commands: Arc<PreferenceCommands>,
}

pub(crate) type SaveReceipt = mpsc::Receiver<Result<(), String>>;

struct PreferenceCommand {
    action: PreferenceAction,
    completion: mpsc::Sender<Result<(), String>>,
}

struct PreferenceCommands {
    sender: Mutex<Option<mpsc::SyncSender<PreferenceCommand>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

static REGISTERED_STATE: OnceLock<RequestPreferencesFacade> = OnceLock::new();

impl RequestPreferencesFacade {
    pub(crate) fn open(settings: &AppSettings) -> std::io::Result<Self> {
        let facade = Self::with_persistence(settings.into(), |state| {
            get_settings_manager().set_request_preferences(state);
            get_settings_manager().save()
        })?;
        let _ = REGISTERED_STATE.set(facade.clone());
        Ok(facade)
    }

    fn with_persistence(
        initial: RequestPreferences,
        persist: impl Fn(&RequestPreferences) -> Result<(), String> + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let store = PersistentStore::new(
            initial,
            reduce_preferences,
            "request-preferences",
            persist,
            |_, error| log::error!("[Settings] Request preferences persistence failed: {error}"),
        )?;
        let (sender, receiver) = mpsc::sync_channel::<PreferenceCommand>(64);
        let worker_store = store.clone();
        let worker = std::thread::Builder::new()
            .name("request-preference-commands".into())
            .spawn(move || {
                while let Ok(command) = receiver.recv() {
                    worker_store.dispatch(command.action);
                    let result = worker_store
                        .retry_failed()
                        .and_then(|_| worker_store.flush());
                    let _ = command.completion.send(result);
                }
            })?;
        Ok(Self {
            store,
            commands: Arc::new(PreferenceCommands {
                sender: Mutex::new(Some(sender)),
                worker: Mutex::new(Some(worker)),
            }),
        })
    }

    pub(crate) fn snapshot(&self) -> StoreSnapshot<RequestPreferences> {
        self.store.snapshot()
    }

    pub(crate) fn subscribe(&self, listener: StoreListener<RequestPreferences>) -> Subscription {
        self.store.subscribe(listener)
    }

    /// UI callbacks enqueue synchronously; reduction and persistence run on owned
    /// workers. FIFO admission preserves successive edits and shutdown drains them.
    pub(crate) fn submit_timeout(&self, input: &str) -> Result<SaveReceipt, String> {
        let value = normalize_timeout_ms(input).ok_or("invalid request timeout")?;
        self.submit(PreferenceAction::Timeout(value))
    }

    pub(crate) fn submit_merge_method(
        &self,
        method: PullRequestMergeMethod,
    ) -> Result<SaveReceipt, String> {
        self.submit(PreferenceAction::MergeMethod(method))
    }

    fn submit(&self, action: PreferenceAction) -> Result<SaveReceipt, String> {
        let (completion, receipt) = mpsc::channel();
        let sender = self
            .commands
            .sender
            .lock()
            .map_err(|_| "preference command lock poisoned")?;
        sender
            .as_ref()
            .ok_or("request preferences are closed")?
            .try_send(PreferenceCommand { action, completion })
            .map_err(|error| format!("request preference admission failed: {error}"))?;
        Ok(receipt)
    }

    pub(crate) fn shutdown(&self) {
        self.commands
            .sender
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let mut worker = self
            .commands
            .worker
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(worker) = worker.take() {
            if worker.join().is_err() {
                log::error!("[Settings] Preference command worker panicked");
            }
        }
        self.store.shutdown();
    }
}

/// Only worker callers may wait for a durable save receipt.
pub(crate) fn wait_for_save(receipt: Result<SaveReceipt, String>) -> Result<(), String> {
    receipt?
        .recv()
        .map_err(|_| "request preference command worker stopped".to_string())?
}

fn reduce_preferences(
    current: &RequestPreferences,
    action: PreferenceAction,
) -> Reduction<RequestPreferences, ()> {
    let mut next = current.clone();
    match action {
        PreferenceAction::Timeout(value) => next.http_request_timeout_ms = value,
        PreferenceAction::MergeMethod(value) => next.default_pr_merge_method = value,
    }
    if next == *current {
        Reduction::unchanged(next, ())
    } else {
        Reduction::changed(next, ())
    }
}

pub(crate) fn normalize_timeout_ms(input: &str) -> Option<String> {
    if input.is_empty() || !input.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let normalized = input.trim_start_matches('0');
    Some(
        if normalized.is_empty() {
            "0"
        } else {
            normalized
        }
        .into(),
    )
}

/// Preserve the decimal setting exactly, while saturating only the runtime's
/// finite Duration representation. Input must be validated canonical decimal.
/// The HTTP layer handles unreachable deadlines.
pub(crate) fn timeout_duration(input: &str) -> Duration {
    let milliseconds = input.bytes().try_fold(0_u128, |value, byte| {
        value
            .checked_mul(10)?
            .checked_add(u128::from(byte.checked_sub(b'0')?))
    });
    match milliseconds {
        Some(ms) if ms / 1000 <= u128::from(u64::MAX) => {
            Duration::new((ms / 1000) as u64, ((ms % 1000) * 1_000_000) as u32)
        }
        _ => Duration::MAX,
    }
}

pub(crate) fn current_timeout() -> Duration {
    REGISTERED_STATE
        .get()
        .map(|state| timeout_duration(&state.snapshot().state().http_request_timeout_ms))
        .unwrap_or_else(|| {
            get_settings_manager()
                .with_settings(|settings| timeout_duration(&settings.http.request_timeout_ms))
        })
}

pub(crate) fn current_merge_method() -> PullRequestMergeMethod {
    REGISTERED_STATE
        .get()
        .map(|state| state.snapshot().state().default_pr_merge_method)
        .unwrap_or_else(|| {
            get_settings_manager().with_git(|git| {
                PullRequestMergeMethod::parse(&git.default_pr_merge_method)
                    .unwrap_or(PullRequestMergeMethod::Merge)
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn timeout_validation_accepts_zero_and_unbounded_decimal_integers() {
        for invalid in ["", "-1", "+1", "1.5", "1e3", " 1000", "１０００", "10ms"] {
            assert!(normalize_timeout_ms(invalid).is_none(), "{invalid}");
        }
        assert_eq!(normalize_timeout_ms("000"), Some("0".into()));
        assert_eq!(normalize_timeout_ms("00030000"), Some("30000".into()));
        let huge = "9".repeat(1000);
        assert_eq!(normalize_timeout_ms(&huge), Some(huge.clone()));
        assert_eq!(timeout_duration(&huge), Duration::MAX);
        assert_eq!(timeout_duration("0"), Duration::ZERO);
        assert_eq!(timeout_duration("30001"), Duration::from_millis(30001));
        assert_eq!(
            timeout_duration("18446744073709551615999"),
            Duration::new(u64::MAX, 999_000_000)
        );
    }

    #[test]
    fn changed_preferences_notify_and_automatically_persist_without_losing_other_fields() {
        let persisted = Arc::new(Mutex::new(Vec::new()));
        let saved = persisted.clone();
        let state = RequestPreferencesFacade::with_persistence(
            (&AppSettings::default()).into(),
            move |state| {
                saved.lock().unwrap().push(state.clone());
                Ok(())
            },
        )
        .unwrap();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let observer = observed.clone();
        let _subscription = state.subscribe(Arc::new(move |snapshot| {
            observer
                .lock()
                .unwrap()
                .push((snapshot.revision(), snapshot.state().clone()));
        }));
        assert!(persisted.lock().unwrap().is_empty());
        wait_for_save(state.submit_timeout("0")).unwrap();
        wait_for_save(state.submit_merge_method(PullRequestMergeMethod::Rebase)).unwrap();
        wait_for_save(state.submit_timeout("000")).unwrap();
        assert_eq!(state.snapshot().revision(), 2);
        assert_eq!(observed.lock().unwrap().len(), 3);
        assert_eq!(
            persisted.lock().unwrap().last().unwrap(),
            state.snapshot().state()
        );
        assert_eq!(state.snapshot().state().http_request_timeout_ms, "0");
        state.shutdown();
    }

    #[test]
    fn accepted_commands_keep_click_order_and_are_drained_on_shutdown() {
        let saved = Arc::new(Mutex::new(None));
        let persisted = saved.clone();
        let state = RequestPreferencesFacade::with_persistence(
            (&AppSettings::default()).into(),
            move |value| {
                *persisted.lock().unwrap() = Some(value.clone());
                Ok(())
            },
        )
        .unwrap();
        let first = state.submit_timeout("1000");
        let second = state.submit_timeout("2000");
        let merge = state.submit_merge_method(PullRequestMergeMethod::Rebase);
        state.shutdown();
        wait_for_save(first).unwrap();
        wait_for_save(second).unwrap();
        wait_for_save(merge).unwrap();
        let final_state = saved.lock().unwrap().clone().unwrap();
        assert_eq!(final_state.http_request_timeout_ms, "2000");
        assert_eq!(
            final_state.default_pr_merge_method,
            PullRequestMergeMethod::Rebase
        );
        assert!(state.submit_timeout("3000").is_err());
    }

    #[test]
    fn same_value_save_retries_a_failed_write_without_a_new_revision() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let failing = Arc::new(AtomicBool::new(true));
        let failure = failing.clone();
        let state = RequestPreferencesFacade::with_persistence(
            (&AppSettings::default()).into(),
            move |_| {
                if failure.load(Ordering::SeqCst) {
                    Err("injected preference failure".into())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap();
        assert_eq!(
            wait_for_save(state.submit_timeout("0")).unwrap_err(),
            "injected preference failure"
        );
        let revision = state.snapshot().revision();
        failing.store(false, Ordering::SeqCst);
        wait_for_save(state.submit_timeout("0")).unwrap();
        assert_eq!(state.snapshot().revision(), revision);
        state.shutdown();
    }
}
