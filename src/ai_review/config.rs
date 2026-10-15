use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use usher_ai_review::{
    MAX_RULE_BYTES, ProjectReviewKey, ReviewConfigAction, ReviewConfigError,
    ReviewConfigurationState, ReviewRule, RuleBinding, RuleId, reduce_configuration,
};
use usher_state_store::{PersistentStore, StoreSnapshot, Subscription};

use super::storage::{JsonLoad, load_json_preserving_corrupt};

type ConfigStore =
    PersistentStore<ReviewConfigurationState, ReviewConfigAction, Result<(), ReviewConfigError>>;

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
    pub(crate) const fn message_key(self) -> &'static str {
        match self {
            Self::EmptyPath => "settings.ai_review.validation.empty_path",
            Self::RelativePath => "settings.ai_review.validation.relative_path",
            Self::NotMarkdown => "settings.ai_review.validation.not_markdown",
            Self::NotFound => "settings.ai_review.validation.not_found",
            Self::NotAFile => "settings.ai_review.validation.not_a_file",
            Self::TooLarge => "settings.ai_review.validation.too_large",
            Self::Inaccessible => "settings.ai_review.validation.inaccessible",
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
    store: ConfigStore,
}

impl ReviewConfigFacade {
    #[cfg(test)]
    pub(super) fn isolated(path: PathBuf) -> Self {
        Self {
            store: PersistentStore::new(
                ReviewConfigurationState::default().normalize(),
                reduce_configuration,
                "ai-review-isolated-config",
                move |state| persist_configuration(&path, state),
                |_, error| log::error!("[AI Review][Test Config] Persistence failed: {error}"),
            )
            .unwrap(),
        }
    }

    pub(crate) fn open_default() -> Self {
        let path = default_config_path();
        let (initial, writes_allowed) = load_configuration(&path);
        let store = PersistentStore::new(
            initial,
            reduce_configuration,
            "ai-review-config",
            move |state| {
                if writes_allowed {
                    persist_configuration(&path, state)
                } else {
                    Ok(())
                }
            },
            |_, error| log::error!("[AI Review][Config] Persistence failed: {error}"),
        )
        .expect("failed to start AI review config worker");
        Self { store }
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
        self.store.shutdown();
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
    let data = serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?;
    usher_fs::atomic_write(path, &data).map_err(|error| error.to_string())
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
