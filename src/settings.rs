// Settings module - Responsible for configuration read/write
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use config::{Config, File as ConfigFile, FileFormat};
use serde::{Deserialize, Serialize};
use slint::{Global, Timer};

use crate::{AppState, MainWindow};

/// Application settings structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// Theme mode: 0=light, 1=dark, 2=auto
    #[serde(default = "default_theme_mode")]
    pub theme_mode: i32,

    /// Auto update enabled
    #[serde(default = "default_auto_update")]
    pub auto_update: bool,

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
pub struct PreviewSettings {
    /// `docked` (default) or `detached`.
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
    pub agent: AiAgentSettings,
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
}

fn default_ai_agent_provider() -> String {
    "custom".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitSettings {
    #[serde(default)]
    pub platform: GitPlatformSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitPlatformSettings {
    #[serde(default)]
    pub github: GitTokenSettings,
    #[serde(default)]
    pub gitlab: GitTokenSettings,
    #[serde(default)]
    pub gitee: GitTokenSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

fn default_auto_update() -> bool {
    true // Default to enabled
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
            auto_update: default_auto_update(),
            application_console_logging: default_application_console_logging(),
            application_file_logging: default_application_file_logging(),
            tracing_logger_active: default_tracing_logger_active(),
            terminal_verbose_logging: default_terminal_verbose_logging(),
            memory_diagnostics_logging: default_memory_diagnostics_logging(),
            ui_sound_effects: default_ui_sound_effects(),
            navigation_drawer_expanded: default_navigation_drawer_expanded(),
            preview: PreviewSettings::default(),
            git: GitSettings::default(),
            ai: AiSettings::default(),
        }
    }
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
}

struct SettingsLoad {
    settings: AppSettings,
    writes_blocked: bool,
}

static SETTINGS_TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

impl SettingsManager {
    /// Create a new settings manager and load settings from file
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
            Ok(config) => match config.try_deserialize::<AppSettings>() {
                Ok(mut settings) => {
                    migrate_legacy_gitee_token(&mut settings);
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

        atomic_write(&self.config_path, toml_content.as_bytes())
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

    /// Update theme mode and save
    pub fn set_theme_mode(&self, mode: i32) {
        if let Ok(mut settings) = self.settings.write() {
            settings.theme_mode = mode;
        }
    }

    /// Update auto update setting
    pub fn set_auto_update(&self, enabled: bool) {
        if let Ok(mut settings) = self.settings.write() {
            settings.auto_update = enabled;
        }
    }

    pub fn set_application_console_logging(&self, enabled: bool) {
        if let Ok(mut settings) = self.settings.write() {
            settings.application_console_logging = enabled;
        }
    }

    pub fn set_application_file_logging(&self, enabled: bool) {
        if let Ok(mut settings) = self.settings.write() {
            settings.application_file_logging = enabled;
        }
    }

    /// Update tracing logger active setting
    pub fn set_tracing_logger_active(&self, enabled: bool) {
        if let Ok(mut settings) = self.settings.write() {
            settings.tracing_logger_active = enabled;
        }
    }

    /// Update terminal verbose logging setting
    pub fn set_terminal_verbose_logging(&self, enabled: bool) {
        if let Ok(mut settings) = self.settings.write() {
            settings.terminal_verbose_logging = enabled;
        }
    }

    pub fn set_memory_diagnostics_logging(&self, enabled: bool) {
        if let Ok(mut settings) = self.settings.write() {
            settings.memory_diagnostics_logging = enabled;
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
            settings.preview.presentation_mode = if mode == "detached" {
                "detached".to_string()
            } else {
                "docked".to_string()
            };
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

    /// Update git platform tokens
    pub fn set_git_platform_tokens(
        &self,
        github: &str,
        gitlab: &str,
        gitee: &str,
        gitee_enterprise: &str,
    ) {
        if let Ok(mut settings) = self.settings.write() {
            settings.git.platform.github.token = github.trim().to_string();
            settings.git.platform.gitlab.token = gitlab.trim().to_string();
            settings.git.platform.gitee.token = gitee.trim().to_string();
            settings.git.platform.gitee.enterprise_token = gitee_enterprise.trim().to_string();
        }
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
        if let Ok(mut settings) = self.settings.write() {
            settings.git.platform.github.api_base_url = normalize_api_base_url(github_api_base_url);
            settings.git.platform.github.enterprise = github_enterprise.trim().to_string();
            settings.git.platform.gitlab.api_base_url = normalize_api_base_url(gitlab_api_base_url);
            settings.git.platform.gitlab.enterprise = gitlab_enterprise.trim().to_string();
            settings.git.platform.gitee.api_base_url = normalize_api_base_url(gitee_api_base_url);
            settings.git.platform.gitee.enterprise = gitee_enterprise.trim().to_string();
        }
    }

    /// Update the OpenAI-compatible AI Agent connection settings.
    pub fn set_ai_agent_settings(
        &self,
        provider: &str,
        base_url: &str,
        api_key: &str,
        model: &str,
    ) {
        if let Ok(mut settings) = self.settings.write() {
            settings.ai.agent.provider = match provider {
                "deepseek" | "openai" => provider.to_string(),
                _ => default_ai_agent_provider(),
            };
            settings.ai.agent.base_url = base_url.trim().trim_end_matches('/').to_string();
            settings.ai.agent.api_key = api_key.trim().to_string();
            settings.ai.agent.model = model.trim().to_string();
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

fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = parent_directory(path);

    let (temporary_path, mut temporary_file) = create_temporary_file(path)?;
    let write_result: io::Result<()> = (|| {
        temporary_file.write_all(contents)?;
        temporary_file.sync_all()?;
        drop(temporary_file);
        replace_file(&temporary_path, path)?;

        #[cfg(unix)]
        File::open(parent)?.sync_all()?;

        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    write_result
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn create_temporary_file(path: &Path) -> io::Result<(PathBuf, File)> {
    let Some(file_name) = path.file_name() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "settings path has no file name",
        ));
    };

    for _ in 0..16 {
        let sequence = SETTINGS_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut temporary_name = OsString::from(file_name);
        temporary_name.push(format!(".tmp-{}-{sequence}", std::process::id()));
        let temporary_path = path.with_file_name(temporary_name);
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary_path)
        {
            Ok(file) => return Ok((temporary_path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique settings temporary file",
    ))
}

#[cfg(not(target_os = "windows"))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(target_os = "windows")]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

// Global settings manager instance
lazy_static::lazy_static! {
    static ref SETTINGS_MANAGER: SettingsManager = SettingsManager::new();
}

/// Get the global settings manager
pub fn get_settings_manager() -> &'static SettingsManager {
    &SETTINGS_MANAGER
}

/// Save theme mode setting asynchronously
pub fn save_theme_mode(mode: i32) {
    get_settings_manager().set_theme_mode(mode);
    schedule_settings_save();
}

/// Save auto update setting asynchronously
pub fn save_auto_update(enabled: bool) {
    get_settings_manager().set_auto_update(enabled);
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
    Timer::single_shot(std::time::Duration::from_millis(100), || {
        crate::app_runtime::shared_runtime().spawn(async {
            match tokio::task::spawn_blocking(|| get_settings_manager().save()).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    log::error!("[Settings] Failed to save settings: {error}");
                }
                Err(error) => {
                    log::error!("[Settings] Save worker failed: {error}");
                }
            }
        });
    });
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
pub fn save_ai_agent_settings(
    provider: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
) -> Result<(), String> {
    get_settings_manager().set_ai_agent_settings(provider, base_url, api_key, model);
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
    app_state.set_auto_update(settings.auto_update);
    app_state.set_application_console_logging(settings.application_console_logging);
    app_state.set_application_file_logging(settings.application_file_logging);
    app_state.set_tracing_logger_active(settings.tracing_logger_active);
    app_state.set_terminal_verbose_logging(settings.terminal_verbose_logging);
    app_state.set_memory_diagnostics_logging(settings.memory_diagnostics_logging);
    app_state.set_ui_sound_effects(settings.ui_sound_effects);
    app_state.set_artifact_preview_presentation_mode(
        if settings.preview.presentation_mode == "detached" {
            1
        } else {
            0
        },
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
    app_state.set_ai_agent_base_url(settings.ai.agent.base_url.clone().into());
    app_state.set_ai_agent_api_key(settings.ai.agent.api_key.clone().into());
    app_state.set_ai_agent_model(settings.ai.agent.model.clone().into());
    app_state.set_ai_agent_provider(settings.ai.agent.provider.clone().into());
    app.set_settings_ai_agent_provider_index(provider_index(&settings.ai.agent));

    // Restore the persisted drawer state before the first frame. This snaps
    // shell geometry to its final position instead of running the interactive
    // toggle animation during startup.
    app.invoke_restore_navigation_drawer_state(settings.navigation_drawer_expanded);

    log::info!(
        "[Settings] Applied settings: theme_mode={}, auto_update={}, application_console_logging={}, application_file_logging={}, tracing_logger_active={}, terminal_verbose_logging={}, memory_diagnostics_logging={}, ui_sound_effects={}, navigation_drawer_expanded={}, git_tokens=({}/{}/{}), ai.agent=({}/{}/{})",
        settings.theme_mode,
        settings.auto_update,
        settings.application_console_logging,
        settings.application_file_logging,
        settings.tracing_logger_active,
        settings.terminal_verbose_logging,
        settings.memory_diagnostics_logging,
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
        if settings.ai.agent.base_url.is_empty() {
            "empty"
        } else {
            "set"
        },
        if settings.ai.agent.api_key.is_empty() {
            "empty"
        } else {
            "set"
        },
        if settings.ai.agent.model.is_empty() {
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

fn provider_index(settings: &AiAgentSettings) -> i32 {
    match settings.provider.as_str() {
        "deepseek" => 1,
        "openai" => 2,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_drawer_is_expanded_by_default() {
        assert!(AppSettings::default().navigation_drawer_expanded);
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
        assert!(settings.application_console_logging);
        assert_eq!(settings.application_file_logging, cfg!(debug_assertions));
        assert!(settings.tracing_logger_active);
        assert!(!settings.terminal_verbose_logging);
        assert_eq!(settings.memory_diagnostics_logging, cfg!(debug_assertions));
    }

    #[test]
    fn writes_granular_logging_settings() {
        let settings = AppSettings {
            application_console_logging: false,
            application_file_logging: false,
            tracing_logger_active: false,
            terminal_verbose_logging: false,
            memory_diagnostics_logging: false,
            ..Default::default()
        };

        let serialized = toml::to_string(&settings).unwrap();
        assert!(serialized.contains("application_console_logging = false"));
        assert!(serialized.contains("application_file_logging = false"));
        assert!(serialized.contains("tracing_logger_active = false"));
        assert!(serialized.contains("terminal_verbose_logging = false"));
        assert!(serialized.contains("memory_diagnostics_logging = false"));
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
    fn writes_ai_agent_as_nested_toml_table() {
        let settings = AppSettings {
            ai: AiSettings {
                agent: AiAgentSettings {
                    provider: "deepseek".into(),
                    base_url: "https://api.example.com/v1".into(),
                    api_key: "secret".into(),
                    model: "example-model".into(),
                },
                chat: AiChatSettings::default(),
            },
            ..Default::default()
        };
        let serialized = toml::to_string(&settings).unwrap();
        assert!(serialized.contains("[ai.agent]"));
        assert!(!serialized.contains("[ai_agent]"));
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
        std::env::temp_dir().join(format!(
            "usher-settings-{label}-{}-{}",
            std::process::id(),
            SETTINGS_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }
}
