use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use usher_ai_review::{
    MAX_RULE_BYTES, ProjectReviewKey, ReviewConfigAction, ReviewConfigError,
    ReviewConfigurationState, ReviewRule, RuleBinding, RuleId, reduce_configuration,
};
use usher_state_store::{LocalStateStore, StateStore, StoreSnapshot, Subscription};

use super::storage::{JsonLoad, load_json_preserving_corrupt};

type ConfigStore =
    dyn StateStore<ReviewConfigurationState, ReviewConfigAction, Result<(), ReviewConfigError>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuleFileValidationError {
    EmptyPath,
    RelativePath,
    NotMarkdown,
    NotFound,
    NotAFile,
    TooLarge,
    Inaccessible,
}

impl RuleFileValidationError {
    pub(crate) const fn user_message(self) -> &'static str {
        match self {
            Self::EmptyPath => "Enter the Markdown file path.",
            Self::RelativePath => "Use an absolute file path.",
            Self::NotMarkdown => "Choose a .md or .markdown file.",
            Self::NotFound => "Choose an existing Markdown file.",
            Self::NotAFile => "The path must point to a file.",
            Self::TooLarge => "The Markdown file must be 512 KiB or smaller.",
            Self::Inaccessible => "The Markdown file cannot be accessed.",
        }
    }
}

pub(crate) fn validate_rule_file(path: &Path) -> Result<(), RuleFileValidationError> {
    if path.as_os_str().is_empty() {
        return Err(RuleFileValidationError::EmptyPath);
    }
    if !path.is_absolute() {
        return Err(RuleFileValidationError::RelativePath);
    }
    let is_markdown = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown")
        });
    if !is_markdown {
        return Err(RuleFileValidationError::NotMarkdown);
    }
    let metadata = fs::metadata(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => RuleFileValidationError::NotFound,
        _ => RuleFileValidationError::Inaccessible,
    })?;
    if !metadata.is_file() {
        return Err(RuleFileValidationError::NotAFile);
    }
    if metadata.len() > MAX_RULE_BYTES {
        return Err(RuleFileValidationError::TooLarge);
    }
    Ok(())
}

#[derive(Clone)]
pub(crate) struct ReviewConfigFacade {
    store: Arc<ConfigStore>,
    worker: Arc<PersistenceWorker>,
    _commit_subscription: Arc<Subscription>,
}

impl ReviewConfigFacade {
    pub(crate) fn open_default() -> Self {
        let path = default_config_path();
        let (initial, writes_allowed) = load_configuration(&path);
        let store: Arc<ConfigStore> = Arc::new(LocalStateStore::new(initial, reduce_configuration));
        let worker = Arc::new(PersistenceWorker::start(path, writes_allowed));
        let sender = worker.sender.clone();
        let subscription = store.observe_commits(Arc::new(move |snapshot| {
            let _ = sender.send(Some(snapshot.state_arc()));
        }));
        Self {
            store,
            worker,
            _commit_subscription: Arc::new(subscription),
        }
    }

    pub(crate) fn snapshot(&self) -> StoreSnapshot<ReviewConfigurationState> {
        self.store.snapshot()
    }

    pub(crate) fn dispatch(&self, action: ReviewConfigAction) -> Result<(), ReviewConfigError> {
        self.store.dispatch(action).output
    }

    pub(crate) fn subscribe(
        &self,
        listener: Arc<dyn Fn(StoreSnapshot<ReviewConfigurationState>) + Send + Sync>,
    ) -> Subscription {
        self.store.subscribe(listener)
    }

    pub(crate) fn new_rule_id(&self) -> RuleId {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis());
        let sequence = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        RuleId(format!("rule-{millis:x}-{sequence:x}"))
    }

    pub(crate) fn upsert_rule(
        &self,
        id: Option<RuleId>,
        name: String,
        path: PathBuf,
    ) -> Result<RuleId, ReviewConfigError> {
        let id = id.unwrap_or_else(|| self.new_rule_id());
        self.dispatch(ReviewConfigAction::UpsertRule(ReviewRule {
            id: id.clone(),
            name,
            markdown_path: path,
        }))?;
        Ok(id)
    }

    pub(crate) fn delete_rule(&self, id: RuleId) -> Result<(), ReviewConfigError> {
        let state = self.snapshot();
        let replacement_default = if state.state().default_rule_id.as_ref() == Some(&id) {
            state
                .state()
                .rules
                .iter()
                .find(|rule| rule.id != id)
                .map(|rule| rule.id.clone())
        } else {
            state.state().default_rule_id.clone()
        };
        self.dispatch(ReviewConfigAction::DeleteRule {
            id,
            replacement_default,
        })
    }

    pub(crate) fn set_default_rule(&self, id: Option<RuleId>) -> Result<(), ReviewConfigError> {
        self.dispatch(ReviewConfigAction::SetDefaultRule(id))
    }

    pub(crate) fn set_project_rule(
        &self,
        project: ProjectReviewKey,
        binding: RuleBinding,
    ) -> Result<(), ReviewConfigError> {
        self.dispatch(ReviewConfigAction::SetProjectRule { project, binding })
    }

    pub(crate) fn set_max_concurrent_tasks(&self, value: u8) -> Result<(), ReviewConfigError> {
        self.dispatch(ReviewConfigAction::SetMaxConcurrentTasks(value))
    }

    pub(crate) fn shutdown(&self) {
        self.worker.shutdown();
    }
}

struct PersistenceWorker {
    sender: mpsc::Sender<Option<Arc<ReviewConfigurationState>>>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl PersistenceWorker {
    fn start(path: PathBuf, writes_allowed: bool) -> Self {
        let (sender, receiver) = mpsc::channel::<Option<Arc<ReviewConfigurationState>>>();
        let handle = std::thread::Builder::new()
            .name("ai-review-config".into())
            .spawn(move || {
                while let Ok(message) = receiver.recv() {
                    let Some(mut latest) = message else {
                        break;
                    };
                    while let Ok(Some(newer)) = receiver.try_recv() {
                        latest = newer;
                    }
                    if !writes_allowed {
                        continue;
                    }
                    if let Err(error) = persist_configuration(&path, &latest) {
                        log::error!("[AI Review][Config] Persistence failed: {error}");
                    }
                }
            })
            .expect("failed to start AI review config worker");
        Self {
            sender,
            handle: Mutex::new(Some(handle)),
        }
    }

    fn shutdown(&self) {
        let _ = self.sender.send(None);
        if let Some(handle) = self.handle.lock().ok().and_then(|mut handle| handle.take()) {
            let _ = handle.join();
        }
    }
}

fn default_config_path() -> PathBuf {
    crate::app_paths::config_dir()
        .map(|dir| dir.join("ai-review.json"))
        .unwrap_or_else(|| PathBuf::from("ai-review.json"))
}

pub(crate) fn default_session_dir() -> PathBuf {
    crate::app_paths::data_local_dir()
        .map(|dir| dir.join("ai-review").join("sessions"))
        .unwrap_or_else(|| PathBuf::from("ai-review/sessions"))
}

fn load_configuration(path: &Path) -> (ReviewConfigurationState, bool) {
    match load_json_preserving_corrupt::<ReviewConfigurationState>(path) {
        Ok(JsonLoad::Loaded(state)) => (state.normalize(), true),
        Ok(JsonLoad::Missing) => (ReviewConfigurationState::default().normalize(), true),
        Ok(JsonLoad::CorruptPreserved { backup, error }) => {
            log::error!(
                "[AI Review][Config] Invalid configuration preserved at {}: {error}",
                backup.display()
            );
            (ReviewConfigurationState::default().normalize(), true)
        }
        Err(error) => {
            log::error!("[AI Review][Config] {error}");
            (ReviewConfigurationState::default().normalize(), false)
        }
    }
}

fn persist_configuration(path: &Path, state: &ReviewConfigurationState) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "configuration path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let data = serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, data).map_err(|error| error.to_string())?;
    fs::rename(&temporary, path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_test_dir(label: &str) -> PathBuf {
        static NEXT_TEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let sequence = NEXT_TEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "usher-ai-review-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    #[test]
    fn persistence_round_trip_normalizes_concurrency() {
        let dir = std::env::temp_dir().join(format!(
            "usher-ai-review-config-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        let state = ReviewConfigurationState {
            max_concurrent_tasks: 99,
            ..Default::default()
        };
        persist_configuration(&path, &state).unwrap();
        assert_eq!(load_configuration(&path).0.max_concurrent_tasks, 8);
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(dir);
    }

    #[test]
    fn invalid_configuration_is_preserved_before_defaults_are_loaded() {
        let dir = unique_test_dir("corrupt-config");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        fs::write(&path, b"{invalid").unwrap();

        let (state, writes_allowed) = load_configuration(&path);
        assert_eq!(state, ReviewConfigurationState::default().normalize());
        assert!(writes_allowed);
        assert!(!path.exists());
        let backup = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|candidate| {
                candidate
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("config.json.corrupt-"))
            })
            .expect("corrupt configuration backup");
        assert_eq!(fs::read(&backup).unwrap(), b"{invalid");

        fs::remove_file(backup).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn rule_file_validation_accepts_an_existing_markdown_file() {
        let dir = unique_test_dir("valid-rule");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rules.MD");
        fs::write(&path, "Review carefully.").unwrap();

        assert_eq!(validate_rule_file(&path), Ok(()));

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(dir);
    }

    #[test]
    fn rule_file_validation_rejects_invalid_paths_and_files() {
        assert_eq!(
            validate_rule_file(Path::new("rules.md")),
            Err(RuleFileValidationError::RelativePath)
        );

        let dir = unique_test_dir("invalid-rule");
        fs::create_dir_all(&dir).unwrap();
        let text_path = dir.join("rules.txt");
        fs::write(&text_path, "Review carefully.").unwrap();
        assert_eq!(
            validate_rule_file(&text_path),
            Err(RuleFileValidationError::NotMarkdown)
        );
        assert_eq!(
            validate_rule_file(&dir.join("missing.md")),
            Err(RuleFileValidationError::NotFound)
        );

        let directory_with_markdown_suffix = dir.join("directory.md");
        fs::create_dir_all(&directory_with_markdown_suffix).unwrap();
        assert_eq!(
            validate_rule_file(&directory_with_markdown_suffix),
            Err(RuleFileValidationError::NotAFile)
        );

        let large_path = dir.join("large.md");
        let large_file = fs::File::create(&large_path).unwrap();
        large_file.set_len(MAX_RULE_BYTES + 1).unwrap();
        assert_eq!(
            validate_rule_file(&large_path),
            Err(RuleFileValidationError::TooLarge)
        );

        let _ = fs::remove_file(text_path);
        let _ = fs::remove_file(large_path);
        let _ = fs::remove_dir(directory_with_markdown_suffix);
        let _ = fs::remove_dir(dir);
    }
}
