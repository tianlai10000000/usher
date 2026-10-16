// Settings module - Responsible for configuration read/write
mod ai_agents;
pub(crate) mod request_preferences;
pub(crate) mod request_preferences_ui;
mod save_worker;
pub use ai_agents::{AiAgentConfiguration, AiAgentHeader, AiAgentProfile, MAX_AI_AGENT_HEADERS};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use config::{Config, File as ConfigFile, FileFormat};
use serde::{Deserialize, Serialize};
use slint::Global;

use crate::{AppState, MainWindow};

/// Application settings structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// Theme mode: 0=light, 1=dark, 2=auto
    #[serde(default = "default_theme_mode")]
    pub theme_mode: i32,

    /// UI language preference: `auto` or a locale id from i18n/manifest.toml.
    #[serde(default = "default_language")]
    pub language: String,

    #[serde(default)]
    pub http: HttpSettings,

    #[serde(default)]
    pub update: UpdateSettings,

    #[serde(default)]
    pub diagnostics: DiagnosticsSettings,

    /// Play system UI click sounds (breadcrumb Back for now)
    #[serde(default = "default_ui_sound_effects")]
    pub ui_sound_effects: bool,

    /// Navigation drawer expanded state
    #[serde(default = "default_navigation_drawer_expanded")]
    pub navigation_drawer_expanded: bool,

    /// Application-level artifact preview placement and provider policy.
    #[serde(default)]
    pub preview: PreviewSettings,

    /// Git platform credentials
    #[serde(default)]
    pub git: GitSettings,

    /// OpenAI-compatible AI Agent settings, serialized as `[ai.agent]`.
    #[serde(default)]
    pub ai: AiSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSettings {
    /// Auto update enabled
    #[serde(default = "default_auto_update")]
    pub auto_update: bool,

    /// Update release channel: `stable` or `alpha`.
    #[serde(default = "default_update_channel")]
    pub update_channel: String,
}

/// Decimal milliseconds are stored as text to preserve arbitrarily large
/// nonnegative integer input without a TOML/i32 numeric ceiling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpSettings {
    #[serde(
        default = "default_http_request_timeout_ms",
        deserialize_with = "deserialize_http_timeout"
    )]
    pub request_timeout_ms: String,
}

impl Default for HttpSettings {
    fn default() -> Self {
        Self {
            request_timeout_ms: default_http_request_timeout_ms(),
        }
    }
}

fn default_http_request_timeout_ms() -> String {
    "30000".into()
}

fn deserialize_http_timeout<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Milliseconds {
        Text(String),
        Integer(u64),
    }
    let input = match Milliseconds::deserialize(deserializer)? {
        Milliseconds::Text(value) => value,
        Milliseconds::Integer(value) => value.to_string(),
    };
    request_preferences::normalize_timeout_ms(&input).ok_or_else(|| {
        serde::de::Error::custom("request timeout must be nonnegative integer milliseconds")
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsSettings {
    /// Standard-output application logs
    #[serde(default = "default_application_console_logging")]
    pub application_console_logging: bool,

    /// Plain-text application file logs (`usher.log`)
    #[serde(default = "default_application_file_logging")]
    pub application_file_logging: bool,

    /// Structured tracing file logs (`usher_tracing.log`)
    #[serde(default = "default_tracing_logger_active")]
    pub tracing_logger_active: bool,

    /// Verbose terminal logging active
    #[serde(default = "default_terminal_verbose_logging")]
    pub terminal_verbose_logging: bool,

    /// Periodic process and UI memory diagnostics
    #[serde(default = "default_memory_diagnostics_logging")]
    pub memory_diagnostics_logging: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewSettings {
    /// `docked` (split layout, default), `floating`, or `detached`.
    #[serde(default = "default_preview_presentation_mode")]
    pub presentation_mode: String,
    #[serde(default)]
    pub providers: PreviewProviderSettings,
}

impl Default for PreviewSettings {
    fn default() -> Self {
        Self {
            presentation_mode: default_preview_presentation_mode(),
            providers: PreviewProviderSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewProviderSettings {
    #[serde(default = "default_preview_provider_enabled")]
    pub markdown: bool,
    #[serde(default = "default_preview_provider_enabled")]
    pub image: bool,
    #[serde(default = "default_preview_provider_enabled")]
    pub plain_text: bool,
}

impl Default for PreviewProviderSettings {
    fn default() -> Self {
        Self {
            markdown: true,
            image: true,
            plain_text: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiSettings {
    #[serde(default)]
    pub agent: AiAgentConfiguration,
    #[serde(default)]
    pub chat: AiChatSettings,
}

/// Chat/RAG backend connection. There is intentionally no compiled-in default
/// endpoint: a hardcoded address would leak deployment topology into release
/// binaries and cannot be corrected without shipping a new build.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiChatSettings {
    #[serde(default)]
    pub rag_api_url: String,
}

/// Connection settings shared by OpenAI-compatible providers such as DeepSeek.
/// The API key is persisted alongside the existing Git tokens in the local
/// settings file and must never be written to application logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiAgentSettings {
    /// Selected provider preset: custom, deepseek, or openai.
    #[serde(default = "default_ai_agent_provider")]
    pub provider: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub headers: Vec<AiAgentHeader>,
    /// Preferred AI response locale. `auto` follows the active UI locale.
    #[serde(
        default = "default_ai_agent_reply_language",
        deserialize_with = "deserialize_ai_agent_reply_language"
    )]
    pub reply_language: String,
}

fn default_ai_agent_provider() -> String {
    "custom".to_string()
}

fn default_ai_agent_reply_language() -> String {
    usher_i18n::AUTO_LANGUAGE.to_string()
}

fn deserialize_ai_agent_reply_language<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    String::deserialize(deserializer).map(|value| normalize_ai_agent_reply_language(&value))
}

impl Default for AiAgentSettings {
    fn default() -> Self {
        Self {
            provider: default_ai_agent_provider(),
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            headers: Vec::new(),
            reply_language: default_ai_agent_reply_language(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitSettings {
    #[serde(
        default = "default_pr_merge_method",
        deserialize_with = "deserialize_pr_merge_method"
    )]
    pub default_pr_merge_method: String,
    #[serde(default)]
    pub platform: GitPlatformSettings,
}

fn default_pr_merge_method() -> String {
    "merge".into()
}

fn deserialize_pr_merge_method<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    Ok(crate::git::PullRequestMergeMethod::parse(&value)
        .unwrap_or(crate::git::PullRequestMergeMethod::Merge)
        .as_str()
        .into())
}

impl Default for GitSettings {
    fn default() -> Self {
        Self {
            default_pr_merge_method: default_pr_merge_method(),
            platform: GitPlatformSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct GitPlatformSettings {
    #[serde(default)]
    pub github: GitTokenSettings,
    #[serde(default)]
    pub gitlab: GitTokenSettings,
    #[serde(default)]
    pub gitee: GitTokenSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct GitTokenSettings {
    #[serde(default)]
    pub token: String,
    /// Separate credential for Gitee Enterprise. Other providers currently
    /// leave this field empty and continue to use `token`.
    #[serde(default)]
    pub enterprise_token: String,
    /// Optional API root for an enterprise/self-hosted provider. This is a
    /// complete API base, for example `https://git.example.com/api/v4`.
    #[serde(default)]
    pub api_base_url: String,
    /// Optional enterprise/organization identifier required by a provider.
    #[serde(default)]
    pub enterprise: String,
}

#[derive(Debug, Clone, Default)]
pub struct GitHostedRepositoryConfig {
    pub api_base_url: String,
    pub enterprise: String,
}

fn default_theme_mode() -> i32 {
    0 // Default to light theme
}

fn default_language() -> String {
    usher_i18n::AUTO_LANGUAGE.to_string()
}

fn default_auto_update() -> bool {
    true // Default to enabled
}

fn default_update_channel() -> String {
    default_update_channel_for_version(env!("CARGO_PKG_VERSION")).to_string()
}

fn default_update_channel_for_version(version: &str) -> &'static str {
    if version.contains('-') {
        "alpha"
    } else {
        "stable"
    }
}

fn default_application_console_logging() -> bool {
    true
}

fn default_application_file_logging() -> bool {
    cfg!(debug_assertions)
}

fn default_tracing_logger_active() -> bool {
    true // Default to enabled
}

fn default_terminal_verbose_logging() -> bool {
    false // Default to disabled
}

fn default_memory_diagnostics_logging() -> bool {
    cfg!(debug_assertions)
}

fn default_ui_sound_effects() -> bool {
    false // Opt-in until more controls use it
}

fn default_navigation_drawer_expanded() -> bool {
    true // Start with the primary navigation visible on first launch
}

fn default_preview_presentation_mode() -> String {
    "docked".to_string()
}

fn default_preview_provider_enabled() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme_mode: default_theme_mode(),
            language: default_language(),
            http: HttpSettings::default(),
            update: UpdateSettings::default(),
            diagnostics: DiagnosticsSettings::default(),
            ui_sound_effects: default_ui_sound_effects(),
            navigation_drawer_expanded: default_navigation_drawer_expanded(),
            preview: PreviewSettings::default(),
            git: GitSettings::default(),
            ai: AiSettings::default(),
        }
    }
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            auto_update: default_auto_update(),
            update_channel: default_update_channel(),
        }
    }
}

impl Default for DiagnosticsSettings {
    fn default() -> Self {
        Self {
            application_console_logging: default_application_console_logging(),
            application_file_logging: default_application_file_logging(),
            tracing_logger_active: default_tracing_logger_active(),
            terminal_verbose_logging: default_terminal_verbose_logging(),
            memory_diagnostics_logging: default_memory_diagnostics_logging(),
        }
    }
}

fn deserialize_settings(config: Config) -> Result<AppSettings, String> {
    let mut value = config
        .try_deserialize::<toml::Value>()
        .map_err(|error| error.to_string())?;
    let root = value.as_table_mut().ok_or("Settings must be a table")?;
    for (section, keys) in [
        ("update", &["auto_update", "update_channel"][..]),
        (
            "diagnostics",
            &[
                "application_console_logging",
                "application_file_logging",
                "tracing_logger_active",
                "terminal_verbose_logging",
                "memory_diagnostics_logging",
            ][..],
        ),
    ] {
        for key in keys {
            if let Some(legacy) = root.remove(*key) {
                let target = root
                    .entry(section)
                    .or_insert_with(|| toml::Value::Table(Default::default()));
                let table = target
                    .as_table_mut()
                    .ok_or("Settings section must be a table")?;
                // Explicit section values take precedence over legacy root keys.
                table.entry(*key).or_insert(legacy);
            }
        }
    }
    value
        .try_into()
        .map_err(|error: toml::de::Error| error.to_string())
}

/// Settings manager - handles all configuration operations
pub struct SettingsManager {
    settings: Arc<RwLock<AppSettings>>,
    config_path: PathBuf,
    /// Serializes writers: `save()` is reachable from the UI timer, several
    /// `spawn_blocking` tasks, and shutdown at the same time, and concurrent
    /// `fs::write` calls to one path interleave into invalid TOML.
    write_lock: Mutex<()>,
    /// A corrupt source must be preserved before defaults can be persisted. If
    /// the backup rename fails, block writes for this process rather than
    /// truncating the only copy of the user's credentials.
    writes_blocked: bool,
    // One composition-owned listener; no feature dependency or callback while
    // the Settings write lock is held. No credentials are passed to it.
    git_change_listener: OnceLock<Arc<dyn Fn() + Send + Sync>>,
}

struct SettingsLoad {
    settings: AppSettings,
    writes_blocked: bool,
}

impl SettingsManager {
    /// Create a new settings manager and load settings from file
    #[cfg(not(test))]
    pub fn new() -> Self {
        let config_path = Self::get_config_path();
        Self::with_config_path(config_path)
    }

    fn with_config_path(config_path: PathBuf) -> Self {
        let loaded = Self::load_from_file(&config_path);

        Self {
            settings: Arc::new(RwLock::new(loaded.settings)),
            config_path,
            write_lock: Mutex::new(()),
            git_change_listener: OnceLock::new(),
            writes_blocked: loaded.writes_blocked,
        }
    }

    /// Get the configuration file path
    fn get_config_path() -> PathBuf {
        // Use directories crate to get platform-specific config directory
        // macOS: ~/Library/Application Support/com.usher.app/
        // Linux: ~/.config/usher/
        // Windows: C:\Users\<user>\AppData\Roaming\usher\
        if let Some(config_dir) = crate::app_paths::config_dir() {
            config_dir.join("settings.toml")
        } else {
            // Fallback to current directory
            PathBuf::from("settings.toml")
        }
    }

    /// Load settings from configuration file
    fn load_from_file(config_path: &Path) -> SettingsLoad {
        // Ensure config directory exists
        let parent = parent_directory(config_path);
        if !parent.exists()
            && let Err(e) = fs::create_dir_all(parent)
        {
            log::error!("[Settings] Failed to create config directory: {}", e);
            return SettingsLoad {
                settings: AppSettings::default(),
                writes_blocked: true,
            };
        }

        // If config file doesn't exist, return default settings
        if !config_path.exists() {
            log::info!("[Settings] Config file not found, using defaults");
            return SettingsLoad {
                settings: AppSettings::default(),
                writes_blocked: false,
            };
        }

        // Use config crate to load settings
        match Config::builder()
            .add_source(ConfigFile::from(config_path).format(FileFormat::Toml))
            .build()
        {
            Ok(config) => match deserialize_settings(config) {
                Ok(mut settings) => {
                    migrate_legacy_gitee_token(&mut settings);
                    settings.ai.agent = settings.ai.agent.normalized();
                    log::debug!("[Settings] Loaded settings from {:?}", config_path);
                    SettingsLoad {
                        settings,
                        writes_blocked: false,
                    }
                }
                Err(_) => preserve_corrupt_settings(config_path, "deserialization"),
            },
            Err(_) => preserve_corrupt_settings(config_path, "parsing"),
        }
    }

    /// Save current settings to configuration file
    pub fn save(&self) -> Result<(), String> {
        let _write_guard = self
            .write_lock
            .lock()
            .map_err(|e| format!("Failed to acquire settings write lock: {e}"))?;
        if self.writes_blocked {
            return Err(
                "Settings writes are disabled because the corrupt source file could not be preserved"
                    .to_string(),
            );
        }

        let settings = self
            .settings
            .read()
            .map_err(|e| format!("Failed to acquire read lock: {e}"))?
            .clone();

        // Ensure config directory exists
        let parent = parent_directory(&self.config_path);
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }

        // Serialize to TOML
        let toml_content =
            toml::to_string_pretty(&settings).map_err(|e| format!("Failed to serialize: {}", e))?;

        usher_fs::atomic_write_private(&self.config_path, toml_content.as_bytes())
            .map_err(|e| format!("Failed to write config file: {}", e))?;

        log::info!("[Settings] Saved settings to {:?}", self.config_path);
        Ok(())
    }

    /// Get current settings (clone)
    pub fn get(&self) -> AppSettings {
        self.settings.read().map(|s| s.clone()).unwrap_or_default()
    }

    /// Run `f` against the live settings under the read lock, without cloning
    /// the whole tree. Mirrors `get()`'s tolerance for a poisoned lock by
    /// handing `f` default settings instead of panicking. `f` must stay cheap
    /// and non-blocking: it runs while the read lock is held.
    pub fn with_settings<R>(&self, f: impl FnOnce(&AppSettings) -> R) -> R {
        match self.settings.read() {
            Ok(settings) => f(&settings),
            Err(_) => f(&AppSettings::default()),
        }
    }

    /// Convenience wrapper over [`Self::with_settings`] for the Git subtree.
    pub fn with_git<R>(&self, f: impl FnOnce(&GitSettings) -> R) -> R {
        self.with_settings(|settings| f(&settings.git))
    }

    fn set_request_preferences(&self, state: &request_preferences::RequestPreferences) {
        if let Ok(mut settings) = self.settings.write() {
            settings.http.request_timeout_ms = state.http_request_timeout_ms.clone();
            settings.git.default_pr_merge_method = state.default_pr_merge_method.as_str().into();
        }
    }

    /// Update theme mode and save
    pub fn set_theme_mode(&self, mode: i32) {
        if let Ok(mut settings) = self.settings.write() {
            settings.theme_mode = mode;
        }
    }

    pub fn set_language(&self, language: &str) {
        if let Ok(mut settings) = self.settings.write() {
            settings.language = language.to_string();
        }
    }

    /// Update auto update setting
    pub fn set_auto_update(&self, enabled: bool) {
        if let Ok(mut settings) = self.settings.write() {
            settings.update.auto_update = enabled;
        }
    }

    pub fn set_update_channel(&self, channel: &str) {
        if let Ok(mut settings) = self.settings.write() {
            settings.update.update_channel = if channel == "alpha" {
                "alpha".to_string()
            } else {
                "stable".to_string()
            };
        }
    }

    pub fn set_application_console_logging(&self, enabled: bool) {
        if let Ok(mut settings) = self.settings.write() {
            settings.diagnostics.application_console_logging = enabled;
        }
    }

    pub fn set_application_file_logging(&self, enabled: bool) {
        if let Ok(mut settings) = self.settings.write() {
            settings.diagnostics.application_file_logging = enabled;
        }
    }

    /// Update tracing logger active setting
    pub fn set_tracing_logger_active(&self, enabled: bool) {
        if let Ok(mut settings) = self.settings.write() {
            settings.diagnostics.tracing_logger_active = enabled;
        }
    }

    /// Update terminal verbose logging setting
    pub fn set_terminal_verbose_logging(&self, enabled: bool) {
        if let Ok(mut settings) = self.settings.write() {
            settings.diagnostics.terminal_verbose_logging = enabled;
        }
    }

    pub fn set_memory_diagnostics_logging(&self, enabled: bool) {
        if let Ok(mut settings) = self.settings.write() {
            settings.diagnostics.memory_diagnostics_logging = enabled;
        }
    }

    /// Update UI sound effects setting
    pub fn set_ui_sound_effects(&self, enabled: bool) {
        if let Ok(mut settings) = self.settings.write() {
            settings.ui_sound_effects = enabled;
        }
    }

    /// Update the docked application-sidebar preference.
    pub fn set_navigation_drawer_expanded(&self, expanded: bool) {
        if let Ok(mut settings) = self.settings.write() {
            settings.navigation_drawer_expanded = expanded;
        }
    }

    pub fn set_preview_presentation_mode(&self, mode: &str) {
        if let Ok(mut settings) = self.settings.write() {
            settings.preview.presentation_mode =
                crate::artifact_preview::presentation::PresentationMode::from_setting(mode)
                    .setting_value()
                    .to_string();
        }
    }

    pub fn set_preview_provider_enabled(&self, provider_id: &str, enabled: bool) {
        if let Ok(mut settings) = self.settings.write() {
            match provider_id {
                usher_preview::MARKDOWN_PROVIDER_ID => {
                    settings.preview.providers.markdown = enabled
                }
                usher_preview::IMAGE_PROVIDER_ID => settings.preview.providers.image = enabled,
                usher_preview::PLAIN_TEXT_PROVIDER_ID => {
                    settings.preview.providers.plain_text = enabled
                }
                _ => (),
            }
        }
    }

    pub(crate) fn register_git_change_listener(
        &self,
        listener: Arc<dyn Fn() + Send + Sync>,
    ) -> bool {
        self.git_change_listener.set(listener).is_ok()
    }
    fn notify_git_changed(&self, changed: bool) {
        if changed && let Some(listener) = self.git_change_listener.get() {
            listener();
        }
    }
    /// Notify after an effective memory change, independently of save success.
    pub fn set_git_platform_tokens(
        &self,
        github: &str,
        gitlab: &str,
        gitee: &str,
        gitee_enterprise: &str,
    ) {
        let changed = if let Ok(mut settings) = self.settings.write() {
            let before = settings.git.clone();
            settings.git.platform.github.token = github.trim().to_string();
            settings.git.platform.gitlab.token = gitlab.trim().to_string();
            settings.git.platform.gitee.token = gitee.trim().to_string();
            settings.git.platform.gitee.enterprise_token = gitee_enterprise.trim().to_string();
            settings.git != before
        } else {
            false
        };
        self.notify_git_changed(changed);
    }
    pub fn set_git_hosted_repository_configs(
        &self,
        github_api_base_url: &str,
        github_enterprise: &str,
        gitlab_api_base_url: &str,
        gitlab_enterprise: &str,
        gitee_api_base_url: &str,
        gitee_enterprise: &str,
    ) {
        let changed = if let Ok(mut settings) = self.settings.write() {
            let before = settings.git.clone();
            settings.git.platform.github.api_base_url = normalize_api_base_url(github_api_base_url);
            settings.git.platform.github.enterprise = github_enterprise.trim().to_string();
            settings.git.platform.gitlab.api_base_url = normalize_api_base_url(gitlab_api_base_url);
            settings.git.platform.gitlab.enterprise = gitlab_enterprise.trim().to_string();
            settings.git.platform.gitee.api_base_url = normalize_api_base_url(gitee_api_base_url);
            settings.git.platform.gitee.enterprise = gitee_enterprise.trim().to_string();
            settings.git != before
        } else {
            false
        };
        self.notify_git_changed(changed);
    }

    /// Replace the complete named-profile registry and its active selection.
    pub fn set_ai_agent_settings(&self, configuration: AiAgentConfiguration) {
        if let Ok(mut settings) = self.settings.write() {
            settings.ai.agent = configuration.normalized();
        }
    }
}

fn preserve_corrupt_settings(config_path: &Path, failure_stage: &str) -> SettingsLoad {
    let file_name = config_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "settings.toml".to_string());
    let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%3fZ");
    let backup_path = config_path.with_file_name(format!(
        "{file_name}.corrupt-{timestamp}-{}",
        std::process::id()
    ));

    let writes_blocked = match fs::rename(config_path, &backup_path) {
        Ok(()) => {
            log::error!(
                "[Settings] Settings {} failed; the invalid file was preserved at {:?}",
                failure_stage,
                backup_path,
            );
            false
        }
        Err(rename_error) => {
            log::error!(
                "[Settings] Settings {} failed and the invalid file could not be preserved; writes are disabled for this process (backup: {})",
                failure_stage,
                rename_error
            );
            true
        }
    };

    SettingsLoad {
        settings: AppSettings::default(),
        writes_blocked,
    }
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

// Global settings manager instance
#[cfg(not(test))]
lazy_static::lazy_static! {
    static ref SETTINGS_MANAGER: SettingsManager = SettingsManager::new();
}

#[cfg(test)]
lazy_static::lazy_static! {
    // Keep the directory alive as long as the global manager. Unit tests must
    // neither inherit workstation settings nor persist into the user's file.
    static ref TEST_SETTINGS_DIR: tempfile::TempDir = tempfile::Builder::new()
        .prefix("usher-unit-test-settings-")
        .tempdir()
        .expect("create isolated unit-test settings directory");
    static ref SETTINGS_MANAGER: SettingsManager = SettingsManager::with_config_path(
        TEST_SETTINGS_DIR.path().join("settings.toml"),
    );
}

/// Get the global settings manager
pub fn get_settings_manager() -> &'static SettingsManager {
    &SETTINGS_MANAGER
}

pub(crate) fn load_http_request_timeout() -> std::time::Duration {
    request_preferences::current_timeout()
}

pub(crate) fn load_default_pr_merge_method() -> crate::git::PullRequestMergeMethod {
    request_preferences::current_merge_method()
}

/// Save theme mode setting asynchronously
pub fn save_theme_mode(mode: i32) {
    get_settings_manager().set_theme_mode(mode);
    schedule_settings_save();
}

pub fn save_language(language: &str) {
    get_settings_manager().set_language(language);
    schedule_settings_save();
}

/// Save auto update setting asynchronously
pub fn save_auto_update(enabled: bool) {
    get_settings_manager().set_auto_update(enabled);
    schedule_settings_save();
}

pub fn save_update_channel(channel: &str) {
    get_settings_manager().set_update_channel(channel);
    schedule_settings_save();
}

pub fn save_application_console_logging(enabled: bool) {
    get_settings_manager().set_application_console_logging(enabled);
    schedule_settings_save();
}

pub fn save_application_file_logging(enabled: bool) {
    get_settings_manager().set_application_file_logging(enabled);
    schedule_settings_save();
}

/// Save tracing logger active setting asynchronously
pub fn save_tracing_logger_active(enabled: bool) {
    get_settings_manager().set_tracing_logger_active(enabled);
    schedule_settings_save();
}

/// Save terminal verbose logging setting asynchronously
pub fn save_terminal_verbose_logging(enabled: bool) {
    get_settings_manager().set_terminal_verbose_logging(enabled);
    schedule_settings_save();
}

pub fn save_memory_diagnostics_logging(enabled: bool) {
    get_settings_manager().set_memory_diagnostics_logging(enabled);
    schedule_settings_save();
}

fn schedule_settings_save() {
    save_worker::request();
}

pub(crate) fn shutdown_saves() {
    save_worker::shutdown();
}

/// Save UI sound effects setting asynchronously
pub fn save_ui_sound_effects(enabled: bool) {
    get_settings_manager().set_ui_sound_effects(enabled);
    schedule_settings_save();
}

/// Persist sidebar docking without blocking the Slint event loop.
pub fn save_navigation_drawer_expanded(expanded: bool) {
    get_settings_manager().set_navigation_drawer_expanded(expanded);
    schedule_settings_save();
}

pub fn save_preview_presentation_mode(mode: &str) -> Result<(), String> {
    get_settings_manager().set_preview_presentation_mode(mode);
    get_settings_manager().save()
}

pub fn save_preview_provider_enabled(provider_id: &str, enabled: bool) -> Result<(), String> {
    get_settings_manager().set_preview_provider_enabled(provider_id, enabled);
    get_settings_manager().save()
}

/// Update and persist Git platform tokens.
pub fn save_git_platform_tokens(
    github: &str,
    gitlab: &str,
    gitee: &str,
    gitee_enterprise: &str,
) -> Result<(), String> {
    get_settings_manager().set_git_platform_tokens(github, gitlab, gitee, gitee_enterprise);
    get_settings_manager().save()
}

pub fn save_git_hosted_repository_configs(
    github_api_base_url: &str,
    github_enterprise: &str,
    gitlab_api_base_url: &str,
    gitlab_enterprise: &str,
    gitee_api_base_url: &str,
    gitee_enterprise: &str,
) -> Result<(), String> {
    get_settings_manager().set_git_hosted_repository_configs(
        github_api_base_url,
        github_enterprise,
        gitlab_api_base_url,
        gitlab_enterprise,
        gitee_api_base_url,
        gitee_enterprise,
    );
    get_settings_manager().save()
}

/// Update and persist OpenAI-compatible AI Agent settings.
pub fn save_ai_agent_settings(configuration: &AiAgentConfiguration) -> Result<(), String> {
    get_settings_manager().set_ai_agent_settings(configuration.clone());
    get_settings_manager().save()
}

/// Load settings from file (can be called before MainWindow is created)
/// This is useful for setting app appearance before showing the window
pub fn load_settings() -> AppSettings {
    get_settings_manager().get()
}

/// Resolve the chat RAG endpoint: settings first, then the `USHER_RAG_API_URL`
/// environment variable. Returns `None` when neither is configured.
pub fn load_chat_rag_api_url() -> Option<String> {
    let configured = get_settings_manager().get().ai.chat.rag_api_url;
    let trimmed = configured.trim();
    if !trimmed.is_empty() {
        return Some(trimmed.to_string());
    }
    std::env::var("USHER_RAG_API_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn load_git_hosted_repository_config(provider: &str) -> GitHostedRepositoryConfig {
    get_settings_manager().with_git(|git| {
        let platform = match provider {
            "github" => &git.platform.github,
            "gitlab" => &git.platform.gitlab,
            "gitee" => &git.platform.gitee,
            _ => return GitHostedRepositoryConfig::default(),
        };
        GitHostedRepositoryConfig {
            api_base_url: normalize_api_base_url(platform.api_base_url.as_str()),
            enterprise: platform.enterprise.trim().to_string(),
        }
    })
}

/// Setup settings module - Apply loaded settings to AppState and MainWindow
///
/// Note: This function only applies settings to UI state.
/// Callbacks for saving settings are set up in lib.rs to coordinate with other UI logic.
pub fn setup_settings(app: &MainWindow, settings: &AppSettings) {
    // Apply loaded settings to AppState
    let app_state = AppState::get(app);
    app_state.set_theme_mode(settings.theme_mode);
    app_state.set_http_request_timeout_ms(settings.http.request_timeout_ms.clone().into());
    app_state.set_default_pr_merge_method(settings.git.default_pr_merge_method.clone().into());
    crate::i18n::sync_main_window(app, &settings.language);
    app_state.set_auto_update(settings.update.auto_update);
    app_state.set_update_channel(settings.update.update_channel.clone().into());
    app_state.set_application_console_logging(settings.diagnostics.application_console_logging);
    app_state.set_application_file_logging(settings.diagnostics.application_file_logging);
    app_state.set_tracing_logger_active(settings.diagnostics.tracing_logger_active);
    app_state.set_terminal_verbose_logging(settings.diagnostics.terminal_verbose_logging);
    app_state.set_memory_diagnostics_logging(settings.diagnostics.memory_diagnostics_logging);
    app_state.set_ui_sound_effects(settings.ui_sound_effects);
    app_state.set_artifact_preview_presentation_mode(
        crate::artifact_preview::presentation::PresentationMode::from_setting(
            &settings.preview.presentation_mode,
        )
        .index(),
    );
    app_state.set_artifact_preview_markdown_enabled(settings.preview.providers.markdown);
    app_state.set_artifact_preview_image_enabled(settings.preview.providers.image);
    app_state.set_artifact_preview_plain_text_enabled(settings.preview.providers.plain_text);
    app_state.set_git_github_token(settings.git.platform.github.token.clone().into());
    app_state.set_git_gitlab_token(settings.git.platform.gitlab.token.clone().into());
    app_state.set_git_gitee_token(settings.git.platform.gitee.token.clone().into());
    app_state.set_git_gitee_enterprise_token(
        settings.git.platform.gitee.enterprise_token.clone().into(),
    );
    app_state.set_git_github_api_base_url(settings.git.platform.github.api_base_url.clone().into());
    app_state.set_git_github_enterprise(settings.git.platform.github.enterprise.clone().into());
    app_state.set_git_gitlab_api_base_url(settings.git.platform.gitlab.api_base_url.clone().into());
    app_state.set_git_gitlab_enterprise(settings.git.platform.gitlab.enterprise.clone().into());
    app_state.set_git_gitee_api_base_url(settings.git.platform.gitee.api_base_url.clone().into());
    app_state.set_git_gitee_enterprise(settings.git.platform.gitee.enterprise.clone().into());
    crate::settings_page::apply_ai_agent_configuration_to_window(app, &settings.ai.agent);

    // Restore the persisted drawer state before the first frame. This snaps
    // shell geometry to its final position instead of running the interactive
    // toggle animation during startup.
    app.invoke_restore_navigation_drawer_state(settings.navigation_drawer_expanded);

    log::info!(
        "[Settings] Applied settings: theme_mode={}, auto_update={}, application_console_logging={}, application_file_logging={}, tracing_logger_active={}, terminal_verbose_logging={}, memory_diagnostics_logging={}, ui_sound_effects={}, navigation_drawer_expanded={}, git_tokens=({}/{}/{}), ai.agent=({}/{}/{})",
        settings.theme_mode,
        settings.update.auto_update,
        settings.diagnostics.application_console_logging,
        settings.diagnostics.application_file_logging,
        settings.diagnostics.tracing_logger_active,
        settings.diagnostics.terminal_verbose_logging,
        settings.diagnostics.memory_diagnostics_logging,
        settings.ui_sound_effects,
        settings.navigation_drawer_expanded,
        if settings.git.platform.github.token.is_empty() {
            "empty"
        } else {
            "set"
        },
        if settings.git.platform.gitlab.token.is_empty() {
            "empty"
        } else {
            "set"
        },
        if settings.git.platform.gitee.token.is_empty() {
            "empty"
        } else {
            "set"
        },
        if settings
            .ai
            .agent
            .active()
            .is_none_or(|active| active.base_url.is_empty())
        {
            "empty"
        } else {
            "set"
        },
        if settings
            .ai
            .agent
            .active()
            .is_none_or(|active| active.api_key.is_empty())
        {
            "empty"
        } else {
            "set"
        },
        if settings
            .ai
            .agent
            .active()
            .is_none_or(|active| active.model.is_empty())
        {
            "empty"
        } else {
            "set"
        },
    );
}

/// Before Gitee public and enterprise credentials were split, the settings UI
/// labelled the single token as an enterprise token. Move that value only when
/// an enterprise API root proves that this was an enterprise configuration.
fn migrate_legacy_gitee_token(settings: &mut AppSettings) {
    let gitee = &mut settings.git.platform.gitee;
    if gitee.enterprise_token.trim().is_empty()
        && !gitee.api_base_url.trim().is_empty()
        && !gitee.token.trim().is_empty()
    {
        gitee.enterprise_token = std::mem::take(&mut gitee.token);
    }
}

fn normalize_api_base_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

fn normalize_ai_agent_reply_language(value: &str) -> String {
    let value = value.trim();
    if value.eq_ignore_ascii_case(usher_i18n::AUTO_LANGUAGE) {
        return usher_i18n::AUTO_LANGUAGE.to_string();
    }
    usher_i18n::available_locales()
        .iter()
        .find(|locale| {
            locale.id.eq_ignore_ascii_case(value)
                || locale
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(value))
        })
        .map(|locale| locale.id.to_string())
        .unwrap_or_else(default_ai_agent_reply_language)
}

pub(crate) fn normalize_ai_agent_settings(mut settings: AiAgentSettings) -> AiAgentSettings {
    settings.provider = match settings.provider.as_str() {
        "deepseek" | "openai" => settings.provider,
        _ => default_ai_agent_provider(),
    };
    settings.reply_language = normalize_ai_agent_reply_language(&settings.reply_language);
    settings
}

#[cfg(test)]
mod tests {
    #[test]
    fn request_preferences_round_trip_and_accept_legacy_absence_or_numeric_milliseconds() {
        let defaults: super::AppSettings = toml::from_str("").unwrap();
        assert_eq!(defaults.http.request_timeout_ms, "30000");
        assert_eq!(defaults.git.default_pr_merge_method, "merge");
        let numeric: super::AppSettings = toml::from_str(
            "[http]\nrequest_timeout_ms = 0\n[git]\ndefault_pr_merge_method = 'fast-forward'",
        )
        .unwrap();
        assert_eq!(numeric.http.request_timeout_ms, "0");
        assert_eq!(numeric.git.default_pr_merge_method, "fast_forward");
        for method in ["merge", "squash", "rebase", "fast_forward"] {
            let mut settings = defaults.clone();
            settings.http.request_timeout_ms = "9".repeat(1000);
            settings.git.default_pr_merge_method = method.into();
            let restored: super::AppSettings =
                toml::from_str(&toml::to_string(&settings).unwrap()).unwrap();
            assert_eq!(restored.http, settings.http);
            assert_eq!(restored.git.default_pr_merge_method, method);
        }
        for invalid in ["-1", "1.5", "'abc'", "'-1'", "''"] {
            assert!(
                toml::from_str::<super::AppSettings>(&format!(
                    "[http]\nrequest_timeout_ms = {invalid}"
                ))
                .is_err()
            );
        }
    }
    use super::*;

    #[test]
    fn global_test_settings_use_an_isolated_directory_and_default_http_timeout() {
        let manager = get_settings_manager();
        assert_eq!(
            manager.config_path,
            TEST_SETTINGS_DIR.path().join("settings.toml")
        );
        assert_ne!(manager.config_path, SettingsManager::get_config_path());
        assert!(TEST_SETTINGS_DIR.path().is_dir());
        manager.with_settings(|settings| {
            assert_eq!(settings.http, HttpSettings::default());
        });
    }

    #[test]
    fn navigation_drawer_is_expanded_by_default() {
        assert!(AppSettings::default().navigation_drawer_expanded);
    }

    #[test]
    fn language_defaults_to_system_preference() {
        assert_eq!(AppSettings::default().language, usher_i18n::AUTO_LANGUAGE);
    }

    #[test]
    fn preview_defaults_to_docked_with_all_builtin_providers_enabled() {
        let preview = AppSettings::default().preview;
        assert_eq!(preview.presentation_mode, "docked");
        assert!(preview.providers.markdown);
        assert!(preview.providers.image);
        assert!(preview.providers.plain_text);
    }

    #[test]
    fn legacy_settings_without_preview_section_receive_preview_defaults() {
        let settings: AppSettings = toml::from_str(
            r#"
theme_mode = 1
auto_update = false
"#,
        )
        .unwrap();
        assert_eq!(settings.preview.presentation_mode, "docked");
        assert!(settings.preview.providers.markdown);
        assert!(settings.preview.providers.image);
        assert!(settings.preview.providers.plain_text);
        assert!(settings.diagnostics.application_console_logging);
        assert_eq!(
            settings.diagnostics.application_file_logging,
            cfg!(debug_assertions)
        );
        assert!(settings.diagnostics.tracing_logger_active);
        assert!(!settings.diagnostics.terminal_verbose_logging);
        assert_eq!(
            settings.diagnostics.memory_diagnostics_logging,
            cfg!(debug_assertions)
        );
        assert_eq!(settings.update.update_channel, default_update_channel());
    }

    #[test]
    fn update_channel_default_tracks_the_build_version_kind() {
        assert_eq!(default_update_channel_for_version("1.2.3"), "stable");
        assert_eq!(default_update_channel_for_version("1.2.3-alpha.4"), "alpha");
        assert_eq!(default_update_channel_for_version("1.2.3-rc.1"), "alpha");
    }

    #[test]
    fn update_channel_setting_normalizes_unknown_values() {
        let manager = SettingsManager::with_config_path(
            unique_test_directory("channel").join("settings.toml"),
        );

        manager.set_update_channel("alpha");
        assert_eq!(manager.get().update.update_channel, "alpha");
        manager.set_update_channel("nightly");
        assert_eq!(manager.get().update.update_channel, "stable");
    }

    #[test]
    fn writes_granular_logging_settings() {
        let settings = AppSettings {
            diagnostics: DiagnosticsSettings {
                application_console_logging: false,
                application_file_logging: false,
                tracing_logger_active: false,
                terminal_verbose_logging: false,
                memory_diagnostics_logging: false,
            },
            ..Default::default()
        };

        let serialized = toml::to_string(&settings).unwrap();
        assert!(serialized.contains("application_console_logging = false"));
        assert!(serialized.contains("application_file_logging = false"));
        assert!(serialized.contains("tracing_logger_active = false"));
        assert!(serialized.contains("terminal_verbose_logging = false"));
        assert!(serialized.contains("memory_diagnostics_logging = false"));
        let document: toml::Value = toml::from_str(&serialized).unwrap();
        assert!(document.get("application_console_logging").is_none());
        assert!(document.get("auto_update").is_none());
        assert_eq!(
            document["diagnostics"]["application_console_logging"].as_bool(),
            Some(false)
        );
        assert_eq!(document["update"]["auto_update"].as_bool(), Some(true));
    }

    #[test]
    fn migrates_legacy_sections_and_preserves_values_on_save() {
        let directory = unique_test_directory("settings-sections");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.toml");
        fs::write(
            &path,
            r#"
theme_mode = 1
auto_update = false
update_channel = "alpha"
application_console_logging = false
application_file_logging = false
tracing_logger_active = false
terminal_verbose_logging = true
memory_diagnostics_logging = false
"#,
        )
        .unwrap();
        let manager = SettingsManager::with_config_path(path.clone());
        let settings = manager.get();
        assert!(!settings.update.auto_update);
        assert_eq!(settings.update.update_channel, "alpha");
        assert!(!settings.diagnostics.application_console_logging);
        assert!(!settings.diagnostics.application_file_logging);
        assert!(!settings.diagnostics.tracing_logger_active);
        assert!(settings.diagnostics.terminal_verbose_logging);
        assert!(!settings.diagnostics.memory_diagnostics_logging);
        manager.save().unwrap();
        let saved: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        for key in [
            "auto_update",
            "update_channel",
            "application_console_logging",
            "application_file_logging",
            "tracing_logger_active",
            "terminal_verbose_logging",
            "memory_diagnostics_logging",
        ] {
            assert!(saved.get(key).is_none(), "legacy key remains: {key}");
        }
        let reloaded = SettingsManager::with_config_path(path).get();
        assert_eq!(
            toml::to_string(&settings).unwrap(),
            toml::to_string(&reloaded).unwrap()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn section_values_override_legacy_keys_with_per_field_fallback() {
        let config = Config::builder()
            .add_source(ConfigFile::from_str(
                r#"
auto_update = true
update_channel = "alpha"
application_console_logging = true
terminal_verbose_logging = true
[update]
auto_update = false
[diagnostics]
application_console_logging = false
"#,
                FileFormat::Toml,
            ))
            .build()
            .unwrap();
        let settings = deserialize_settings(config).unwrap();
        assert!(!settings.update.auto_update);
        assert_eq!(settings.update.update_channel, "alpha");
        assert!(!settings.diagnostics.application_console_logging);
        assert!(settings.diagnostics.terminal_verbose_logging);
        assert!(settings.diagnostics.tracing_logger_active);
    }

    #[test]
    fn writes_preview_as_nested_toml_tables() {
        let mut settings = AppSettings::default();
        settings.preview.presentation_mode = "detached".into();
        settings.preview.providers.image = false;

        let serialized = toml::to_string(&settings).unwrap();
        assert!(serialized.contains("[preview]"));
        assert!(serialized.contains("presentation_mode = \"detached\""));
        assert!(serialized.contains("[preview.providers]"));
        assert!(serialized.contains("image = false"));
    }

    #[test]
    fn preview_placement_round_trips_all_modes_without_changing_provider_settings() {
        for mode in ["docked", "floating", "detached"] {
            let mut settings = AppSettings::default();
            settings.preview.presentation_mode = mode.into();
            settings.preview.providers.image = false;
            let serialized = toml::to_string(&settings).unwrap();
            let restored: AppSettings = toml::from_str(&serialized).unwrap();
            assert_eq!(restored.preview.presentation_mode, mode);
            assert!(!restored.preview.providers.image);
        }
    }

    #[test]
    fn writes_ai_agent_as_nested_toml_table() {
        let settings = AppSettings {
            ai: AiSettings {
                agent: AiAgentSettings {
                    provider: "deepseek".into(),
                    base_url: "https://api.example.com/v1".into(),
                    api_key: "secret".into(),
                    model: "example-model".into(),
                    headers: Vec::new(),
                    reply_language: "zh-CN".into(),
                }
                .into(),
                chat: AiChatSettings::default(),
            },
            ..Default::default()
        };
        let serialized = toml::to_string(&settings).unwrap();
        assert!(serialized.contains("[ai.agent]"));
        assert!(serialized.contains("reply_language = \"zh-CN\""));
        assert!(!serialized.contains("[ai_agent]"));
    }

    #[test]
    fn legacy_ai_agent_settings_default_reply_language_to_auto() {
        let settings: AppSettings = toml::from_str(
            r#"
[ai.agent]
provider = "openai"
model = "gpt-4o-mini"
"#,
        )
        .unwrap();

        assert_eq!(
            settings.ai.agent.active().unwrap().reply_language,
            usher_i18n::AUTO_LANGUAGE
        );
    }

    #[test]
    fn ai_agent_reply_language_is_normalized_during_deserialization() {
        for (configured, expected) in [("zh-Hans", "zh-CN"), ("unsupported", "auto")] {
            let settings: AppSettings = toml::from_str(&format!(
                r#"
[ai.agent]
reply_language = "{configured}"
"#
            ))
            .unwrap();

            assert_eq!(settings.ai.agent.active().unwrap().reply_language, expected);
        }
    }

    #[test]
    fn ai_agent_reply_language_is_normalized_when_updated() {
        let manager = SettingsManager::with_config_path(
            unique_test_directory("reply-language").join("settings.toml"),
        );

        manager.set_ai_agent_settings(
            AiAgentSettings {
                reply_language: "zh-Hans".into(),
                ..Default::default()
            }
            .into(),
        );
        assert_eq!(
            manager.get().ai.agent.active().unwrap().reply_language,
            "zh-CN"
        );
        manager.set_ai_agent_settings(
            AiAgentSettings {
                reply_language: "unsupported".into(),
                ..Default::default()
            }
            .into(),
        );
        assert_eq!(
            manager.get().ai.agent.active().unwrap().reply_language,
            usher_i18n::AUTO_LANGUAGE
        );
    }

    #[test]
    fn writes_hosting_configuration_with_each_platform() {
        let mut settings = AppSettings::default();
        settings.git.platform.gitee.api_base_url =
            normalize_api_base_url(" https://gitee.example.com/api/v8/ ");
        settings.git.platform.gitee.enterprise = "example-enterprise".into();

        let serialized = toml::to_string(&settings).unwrap();
        assert!(serialized.contains("[git.platform.gitee]"));
        assert!(serialized.contains("api_base_url = \"https://gitee.example.com/api/v8\""));
        assert!(serialized.contains("enterprise = \"example-enterprise\""));
    }

    #[test]
    fn migrates_legacy_gitee_enterprise_token_without_reusing_it_for_public_api() {
        let mut settings = AppSettings::default();
        settings.git.platform.gitee.api_base_url = "https://gitee.example.com/api/v8".into();
        settings.git.platform.gitee.token = "legacy-enterprise-token".into();

        migrate_legacy_gitee_token(&mut settings);

        assert!(settings.git.platform.gitee.token.is_empty());
        assert_eq!(
            settings.git.platform.gitee.enterprise_token,
            "legacy-enterprise-token"
        );
    }

    #[test]
    fn keeps_legacy_public_gitee_token_when_no_enterprise_api_is_configured() {
        let mut settings = AppSettings::default();
        settings.git.platform.gitee.token = "public-token".into();

        migrate_legacy_gitee_token(&mut settings);

        assert_eq!(settings.git.platform.gitee.token, "public-token");
        assert!(settings.git.platform.gitee.enterprise_token.is_empty());
    }

    #[test]
    fn invalid_settings_are_preserved_before_defaults_can_be_saved() {
        let test_dir = unique_test_directory("corrupt");
        fs::create_dir_all(&test_dir).unwrap();
        let config_path = test_dir.join("settings.toml");
        fs::write(&config_path, b"[git\ninvalid =").unwrap();

        let manager = SettingsManager::with_config_path(config_path.clone());
        assert!(!manager.writes_blocked);
        assert!(!config_path.exists());
        let preserved = fs::read_dir(&test_dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .find(|name| name.starts_with("settings.toml.corrupt-"));
        assert!(preserved.is_some());

        manager.set_theme_mode(2);
        manager.save().unwrap();
        let restored: AppSettings =
            toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(restored.theme_mode, 2);

        fs::remove_dir_all(test_dir).unwrap();
    }

    #[test]
    fn settings_save_replaces_the_file_without_leaving_temporary_files() {
        let test_dir = unique_test_directory("atomic");
        fs::create_dir_all(&test_dir).unwrap();
        let config_path = test_dir.join("settings.toml");
        let manager = SettingsManager::with_config_path(config_path.clone());

        manager.set_theme_mode(1);
        manager.save().unwrap();
        manager.set_theme_mode(2);
        manager.save().unwrap();

        let saved: AppSettings =
            toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(saved.theme_mode, 2);
        assert_eq!(fs::read_dir(&test_dir).unwrap().count(), 1);

        fs::remove_dir_all(test_dir).unwrap();
    }

    fn unique_test_directory(label: &str) -> PathBuf {
        static NEXT_TEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        std::env::temp_dir().join(format!(
            "usher-settings-{label}-{}-{}",
            std::process::id(),
            NEXT_TEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }
    #[test]
    fn git_memory_change_notifies_before_failed_save_and_outside_write_lock() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let directory = unique_test_directory("git-change-notify");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.toml");
        let manager = Arc::new(SettingsManager::with_config_path(path.clone()));
        fs::create_dir(&path).unwrap();
        let notifications = Arc::new(AtomicUsize::new(0));
        let observed = notifications.clone();
        let weak = Arc::downgrade(&manager);
        assert!(manager.register_git_change_listener(Arc::new(move || {
            let manager = weak.upgrade().unwrap();
            manager.with_git(|git| assert_eq!(git.platform.github.token, "new-token"));
            observed.fetch_add(1, Ordering::SeqCst);
        })));
        manager.set_git_platform_tokens("new-token", "", "", "");
        assert_eq!(notifications.load(Ordering::SeqCst), 1);
        assert!(manager.save().is_err());
        assert_eq!(manager.get().git.platform.github.token, "new-token");
        manager.set_git_platform_tokens(" new-token ", "", "", "");
        assert_eq!(
            notifications.load(Ordering::SeqCst),
            1,
            "normalized same value does not invalidate"
        );
        manager.set_git_hosted_repository_configs(
            "https://host.example/api/v3",
            "org",
            "",
            "",
            "",
            "",
        );
        assert_eq!(notifications.load(Ordering::SeqCst), 2);
        assert!(manager.save().is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
