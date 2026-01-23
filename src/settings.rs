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
}

fn default_theme_mode() -> i32 {
    0 // Default to light theme
}

fn default_auto_update() -> bool {
    true // Default to enabled
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme_mode: default_theme_mode(),
            auto_update: default_auto_update(),
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
                    eprintln!("[Settings] Failed to create config directory: {}", e);
                    return AppSettings::default();
                }
            }
        }

        // If config file doesn't exist, return default settings
        if !config_path.exists() {
            println!("[Settings] Config file not found, using defaults");
            return AppSettings::default();
        }

        // Use config crate to load settings
        match Config::builder()
            .add_source(File::from(config_path.as_path()).format(FileFormat::Toml))
            .build()
        {
            Ok(config) => match config.try_deserialize::<AppSettings>() {
                Ok(settings) => {
                    println!("[Settings] Loaded settings from {:?}", config_path);
                    settings
                }
                Err(e) => {
                    eprintln!("[Settings] Failed to deserialize settings: {}", e);
                    AppSettings::default()
                }
            },
            Err(e) => {
                eprintln!("[Settings] Failed to load config: {}", e);
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

        println!("[Settings] Saved settings to {:?}", self.config_path);
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
            eprintln!("[Settings] Failed to save settings: {}", e);
        }
    });
}

/// Save auto update setting asynchronously
pub fn save_auto_update(enabled: bool) {
    get_settings_manager().set_auto_update(enabled);
    Timer::single_shot(std::time::Duration::from_millis(100), || {
        if let Err(e) = get_settings_manager().save() {
            eprintln!("[Settings] Failed to save settings: {}", e);
        }
    });
}

/// Load settings from file (can be called before MainWindow is created)
/// This is useful for setting app appearance before showing the window
pub fn load_settings() -> AppSettings {
    get_settings_manager().get()
}

/// Setup settings module - Apply loaded settings to AppState
///
/// Note: This function only applies settings to UI state.
/// Callbacks for saving settings are set up in lib.rs to coordinate with other UI logic.
pub fn setup_settings(app: &MainWindow, settings: &AppSettings) {
    // Apply loaded settings to AppState
    let app_state = AppState::get(app);
    app_state.set_theme_mode(settings.theme_mode);
    app_state.set_auto_update(settings.auto_update);

    println!(
        "[Settings] Applied settings: theme_mode={}, auto_update={}",
        settings.theme_mode, settings.auto_update
    );
}
