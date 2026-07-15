// Settings module - Responsible for configuration read/write
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use config::{Config, File, FileFormat};
use directories::ProjectDirs;
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

    /// Tracing logger active
    #[serde(default = "default_tracing_logger_active")]
    pub tracing_logger_active: bool,

    /// Verbose terminal logging active
    #[serde(default = "default_terminal_verbose_logging")]
    pub terminal_verbose_logging: bool,

    /// Navigation drawer expanded state
    #[serde(default = "default_navigation_drawer_expanded")]
    pub navigation_drawer_expanded: bool,

    /// Git platform credentials
    #[serde(default)]
    pub git: GitSettings,

    /// OpenAI-compatible AI Agent settings, serialized as `[ai.agent]`.
    #[serde(default)]
    pub ai: AiSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiSettings {
    #[serde(default)]
    pub agent: AiAgentSettings,
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
    /// Optional API root for an enterprise/self-hosted provider. This is a
    /// complete API base, for example `https://git.example.com/api/v4`.
    #[serde(default)]
    pub api_base_url: String,
    /// Optional enterprise/organization identifier required by a provider.
    #[serde(default)]
    pub enterprise: String,
}

#[derive(Debug, Clone, Default)]
pub struct GitPlatformTokens {
    pub github: String,
    pub gitlab: String,
    pub gitee: String,
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

fn default_tracing_logger_active() -> bool {
    true // Default to enabled
}

fn default_terminal_verbose_logging() -> bool {
    false // Default to disabled
}

fn default_navigation_drawer_expanded() -> bool {
    true // Default to expanded
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme_mode: default_theme_mode(),
            auto_update: default_auto_update(),
            tracing_logger_active: default_tracing_logger_active(),
            terminal_verbose_logging: default_terminal_verbose_logging(),
            navigation_drawer_expanded: default_navigation_drawer_expanded(),
            git: GitSettings::default(),
            ai: AiSettings::default(),
        }
    }
}

/// Settings manager - handles all configuration operations
pub struct SettingsManager {
    settings: Arc<RwLock<AppSettings>>,
    config_path: PathBuf,
}

impl SettingsManager {
    /// Create a new settings manager and load settings from file
    pub fn new() -> Self {
        let config_path = Self::get_config_path();
        let settings = Self::load_from_file(&config_path);

        Self {
            settings: Arc::new(RwLock::new(settings)),
            config_path,
        }
    }

    /// Get the configuration file path
    fn get_config_path() -> PathBuf {
        // Use directories crate to get platform-specific config directory
        // macOS: ~/Library/Application Support/com.usher.app/
        // Linux: ~/.config/usher/
        // Windows: C:\Users\<user>\AppData\Roaming\usher\
        if let Some(proj_dirs) = ProjectDirs::from("com", "usher", "app") {
            let config_dir = proj_dirs.config_dir();
            config_dir.join("settings.toml")
        } else {
            // Fallback to current directory
            PathBuf::from("settings.toml")
        }
    }

    /// Load settings from configuration file
    fn load_from_file(config_path: &PathBuf) -> AppSettings {
        // Ensure config directory exists
        if let Some(parent) = config_path.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    log::error!("[Settings] Failed to create config directory: {}", e);
                    return AppSettings::default();
                }
            }
        }

        // If config file doesn't exist, return default settings
        if !config_path.exists() {
            log::info!("[Settings] Config file not found, using defaults");
            return AppSettings::default();
        }

        // Use config crate to load settings
        match Config::builder()
            .add_source(File::from(config_path.as_path()).format(FileFormat::Toml))
            .build()
        {
            Ok(config) => match config.try_deserialize::<AppSettings>() {
                Ok(settings) => {
                    log::debug!("[Settings] Loaded settings from {:?}", config_path);
                    settings
                }
                Err(e) => {
                    log::error!("[Settings] Failed to deserialize settings: {}", e);
                    AppSettings::default()
                }
            },
            Err(e) => {
                log::error!("[Settings] Failed to load config: {}", e);
                AppSettings::default()
            }
        }
    }

    /// Save current settings to configuration file
    pub fn save(&self) -> Result<(), String> {
        let settings = self
            .settings
            .read()
            .map_err(|e| format!("Failed to acquire read lock: {}", e))?;

        // Ensure config directory exists
        if let Some(parent) = self.config_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create config directory: {}", e))?;
            }
        }

        // Serialize to TOML
        let toml_content = toml::to_string_pretty(&*settings)
            .map_err(|e| format!("Failed to serialize: {}", e))?;

        // Write to file
        fs::write(&self.config_path, toml_content)
            .map_err(|e| format!("Failed to write config file: {}", e))?;

        log::info!("[Settings] Saved settings to {:?}", self.config_path);
        Ok(())
    }

    /// Get current settings (clone)
    pub fn get(&self) -> AppSettings {
        self.settings.read().map(|s| s.clone()).unwrap_or_default()
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

    /// Update git platform tokens
    pub fn set_git_platform_tokens(&self, github: &str, gitlab: &str, gitee: &str) {
        if let Ok(mut settings) = self.settings.write() {
            settings.git.platform.github.token = github.trim().to_string();
            settings.git.platform.gitlab.token = gitlab.trim().to_string();
            settings.git.platform.gitee.token = gitee.trim().to_string();
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
    Timer::single_shot(std::time::Duration::from_millis(100), || {
        if let Err(e) = get_settings_manager().save() {
            log::error!("[Settings] Failed to save settings: {}", e);
        }
    });
}

/// Save auto update setting asynchronously
pub fn save_auto_update(enabled: bool) {
    get_settings_manager().set_auto_update(enabled);
    Timer::single_shot(std::time::Duration::from_millis(100), || {
        if let Err(e) = get_settings_manager().save() {
            log::error!("[Settings] Failed to save settings: {}", e);
        }
    });
}

/// Save tracing logger active setting asynchronously
pub fn save_tracing_logger_active(enabled: bool) {
    get_settings_manager().set_tracing_logger_active(enabled);
    Timer::single_shot(std::time::Duration::from_millis(100), || {
        if let Err(e) = get_settings_manager().save() {
            log::error!("[Settings] Failed to save settings: {}", e);
        }
    });
}

/// Save terminal verbose logging setting asynchronously
pub fn save_terminal_verbose_logging(enabled: bool) {
    get_settings_manager().set_terminal_verbose_logging(enabled);
    Timer::single_shot(std::time::Duration::from_millis(100), || {
        if let Err(e) = get_settings_manager().save() {
            log::error!("[Settings] Failed to save settings: {}", e);
        }
    });
}

/// Save git platform tokens asynchronously
pub fn save_git_platform_tokens(github: &str, gitlab: &str, gitee: &str) {
    get_settings_manager().set_git_platform_tokens(github, gitlab, gitee);
    Timer::single_shot(std::time::Duration::from_millis(100), || {
        if let Err(e) = get_settings_manager().save() {
            log::error!("[Settings] Failed to save settings: {}", e);
        }
    });
}

pub fn save_git_hosted_repository_configs(
    github_api_base_url: &str,
    github_enterprise: &str,
    gitlab_api_base_url: &str,
    gitlab_enterprise: &str,
    gitee_api_base_url: &str,
    gitee_enterprise: &str,
) {
    get_settings_manager().set_git_hosted_repository_configs(
        github_api_base_url,
        github_enterprise,
        gitlab_api_base_url,
        gitlab_enterprise,
        gitee_api_base_url,
        gitee_enterprise,
    );
    Timer::single_shot(std::time::Duration::from_millis(100), || {
        if let Err(error) = get_settings_manager().save() {
            log::error!("[Settings] Failed to save hosted repository configuration: {error}");
        }
    });
}

/// Save OpenAI-compatible AI Agent settings asynchronously.
pub fn save_ai_agent_settings(provider: &str, base_url: &str, api_key: &str, model: &str) {
    get_settings_manager().set_ai_agent_settings(provider, base_url, api_key, model);
    Timer::single_shot(std::time::Duration::from_millis(100), || {
        if let Err(e) = get_settings_manager().save() {
            log::error!("[Settings] Failed to save AI Agent settings: {}", e);
        }
    });
}

/// Load settings from file (can be called before MainWindow is created)
/// This is useful for setting app appearance before showing the window
pub fn load_settings() -> AppSettings {
    get_settings_manager().get()
}

pub fn load_git_platform_tokens() -> GitPlatformTokens {
    let settings = get_settings_manager().get();
    GitPlatformTokens {
        github: settings.git.platform.github.token.trim().to_string(),
        gitlab: settings.git.platform.gitlab.token.trim().to_string(),
        gitee: settings.git.platform.gitee.token.trim().to_string(),
    }
}

pub fn load_git_hosted_repository_config(provider: &str) -> GitHostedRepositoryConfig {
    let settings = get_settings_manager().get();
    let platform = match provider {
        "github" => &settings.git.platform.github,
        "gitlab" => &settings.git.platform.gitlab,
        "gitee" => &settings.git.platform.gitee,
        _ => return GitHostedRepositoryConfig::default(),
    };
    GitHostedRepositoryConfig {
        api_base_url: normalize_api_base_url(platform.api_base_url.as_str()),
        enterprise: platform.enterprise.trim().to_string(),
    }
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
    app_state.set_tracing_logger_active(settings.tracing_logger_active);
    app_state.set_terminal_verbose_logging(settings.terminal_verbose_logging);
    app_state.set_git_github_token(settings.git.platform.github.token.clone().into());
    app_state.set_git_gitlab_token(settings.git.platform.gitlab.token.clone().into());
    app_state.set_git_gitee_token(settings.git.platform.gitee.token.clone().into());
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

    // Apply drawer expanded state to MainWindow
    app.set_navigation_drawer_expanded(settings.navigation_drawer_expanded);

    log::info!(
        "[Settings] Applied settings: theme_mode={}, auto_update={}, tracing_logger_active={}, terminal_verbose_logging={}, navigation_drawer_expanded={}, git_tokens=({}/{}/{}), ai.agent=({}/{}/{})",
        settings.theme_mode,
        settings.auto_update,
        settings.tracing_logger_active,
        settings.terminal_verbose_logging,
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
    fn writes_ai_agent_as_nested_toml_table() {
        let settings = AppSettings {
            ai: AiSettings {
                agent: AiAgentSettings {
                    provider: "deepseek".into(),
                    base_url: "https://api.example.com/v1".into(),
                    api_key: "secret".into(),
                    model: "example-model".into(),
                },
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
}
