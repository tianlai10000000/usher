//! Debug Logger - Centralized developer debugging log system
//!
//! # Features
//! - Multiple log levels: Error, Warn, Info, Debug, Trace
//! - Optional file output
//! - Usable in both Rust and Slint
//! - Thread-safe global configuration
//!
//! # Usage Example
//! ```rust
//! use crate::debug_logger::{init_logger, LogConfig};
//!
//! // Initialize the logger
//! init_logger(LogConfig {
//!     level: log::LevelFilter::Debug,
//!     write_to_file: true,
//!     log_file_path: Some("/path/to/app.log".into()),
//! }).expect("Failed to init logger");
//!
//! // Use standard log macros
//! log::info!("Application started");
//! log::debug!("Debug info: {}", some_value);
//! log::error!("Error occurred: {}", error);
//! ```

use chrono::Local;
use fern::colors::{Color, ColoredLevelConfig};
use log::LevelFilter;
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Logger configuration
#[derive(Clone, Debug)]
pub struct LogConfig {
    /// Log level
    pub level: LevelFilter,
    /// Whether to write to file
    pub write_to_file: bool,
    /// Log file path (if write_to_file is true)
    pub log_file_path: Option<PathBuf>,
    /// Whether to organize logs by date (YYYY-MM-DD/usher.log)
    pub log_per_day: bool,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: LevelFilter::Info,
            write_to_file: false,
            log_file_path: None,
            log_per_day: false,
        }
    }
}

/// Global logger initialization state
static LOGGER_INITIALIZED: OnceLock<bool> = OnceLock::new();

/// Get the default log file path
/// Uses the directories crate to get the system standard log directory
///
/// # Arguments
/// * `log_per_day` - If true, organizes logs by date (YYYY-MM-DD/usher.log)
pub fn get_default_log_path(log_per_day: bool) -> PathBuf {
    if let Some(proj_dirs) = directories::ProjectDirs::from("com", "usher", "app") {
        let base_log_dir = proj_dirs.data_local_dir().join("logs");

        let log_dir = if log_per_day {
            // Create date-based subdirectory: logs/YYYY-MM-DD/
            let date_str = Local::now().format("%Y-%m-%d").to_string();
            base_log_dir.join(date_str)
        } else {
            base_log_dir
        };

        // Ensure the directory exists
        std::fs::create_dir_all(&log_dir).ok();
        log_dir.join("usher.log")
    } else {
        // Fallback to current directory
        if log_per_day {
            let date_str = Local::now().format("%Y-%m-%d").to_string();
            PathBuf::from(date_str).join("usher.log")
        } else {
            PathBuf::from("usher.log")
        }
    }
}

/// Initialize the logging system
///
/// # Arguments
/// * `config` - Logger configuration
///
/// # Returns
/// * `Ok(())` - Initialization successful
/// * `Err(String)` - Initialization failed, returns error message
///
/// # Note
/// This function can only be called once. Subsequent calls will return Ok but won't reconfigure.
pub fn init_logger(config: LogConfig) -> Result<(), String> {
    // Check if already initialized
    if LOGGER_INITIALIZED.get().is_some() {
        return Ok(());
    }

    // Set up color configuration (for console output)
    let colors = ColoredLevelConfig::new()
        .error(Color::Red)
        .warn(Color::Yellow)
        .info(Color::Green)
        .debug(Color::Blue)
        .trace(Color::Magenta);

    // Base log formatter (for console, with colors)
    let console_dispatch = fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "[{}] [{}] [{}] {}",
                Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                colors.color(record.level()),
                record.target(),
                message
            ))
        })
        .level(config.level)
        // Suppress verbose logs from third-party libraries
        .level_for("reqwest", LevelFilter::Warn)
        .level_for("hyper", LevelFilter::Warn)
        .level_for("hyper_util", LevelFilter::Warn)
        .level_for("h2", LevelFilter::Warn)
        .level_for("rustls", LevelFilter::Warn)
        .level_for("tokio_util", LevelFilter::Warn)
        .level_for("tungstenite", LevelFilter::Warn)
        .level_for("tracing", LevelFilter::Warn)
        .chain(std::io::stdout());

    // Create main dispatch
    let mut main_dispatch = fern::Dispatch::new().chain(console_dispatch);

    // If file output is enabled
    if config.write_to_file {
        let log_per_day = config.log_per_day;
        let log_path = config
            .log_file_path
            .clone()
            .unwrap_or_else(|| get_default_log_path(log_per_day));

        // Ensure log directory exists
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create log directory: {}", e))?;
        }

        // Open log file (append mode)
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| format!("Failed to open log file: {}", e))?;

        // File log formatter (without colors)
        let file_dispatch = fern::Dispatch::new()
            .format(|out, message, record| {
                out.finish(format_args!(
                    "[{}] [{}] [{}] {}",
                    Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                    record.level(),
                    record.target(),
                    message
                ))
            })
            .level(config.level)
            // Suppress verbose logs from third-party libraries
            .level_for("reqwest", LevelFilter::Warn)
            .level_for("hyper", LevelFilter::Warn)
            .level_for("hyper_util", LevelFilter::Warn)
            .level_for("h2", LevelFilter::Warn)
            .level_for("rustls", LevelFilter::Warn)
            .level_for("tokio_util", LevelFilter::Warn)
            .level_for("tungstenite", LevelFilter::Warn)
            .level_for("tracing", LevelFilter::Warn)
            .chain(log_file);

        main_dispatch = main_dispatch.chain(file_dispatch);

        // Note: Using println here since logger is being configured
        // After apply(), we use log::info! instead
        println!("[Logger] Log file path: {:?}", log_path);
    }

    // Apply logger configuration
    main_dispatch
        .apply()
        .map_err(|e| format!("Failed to apply logger configuration: {}", e))?;

    // Mark as initialized
    LOGGER_INITIALIZED.set(true).ok();

    log::info!(
        "Logger initialized - Level: {:?}, WriteToFile: {}",
        config.level,
        config.write_to_file
    );

    Ok(())
}

/// Parse log level from string
///
/// Supported values (case-insensitive):
/// - "error" / "0" -> Error
/// - "warn" / "1" -> Warn
/// - "info" / "2" -> Info
/// - "debug" / "3" -> Debug
/// - "trace" / "4" -> Trace
pub fn parse_level(level_str: &str) -> LevelFilter {
    match level_str.to_lowercase().as_str() {
        "error" | "0" => LevelFilter::Error,
        "warn" | "warning" | "1" => LevelFilter::Warn,
        "info" | "2" => LevelFilter::Info,
        "debug" | "3" => LevelFilter::Debug,
        "trace" | "4" => LevelFilter::Trace,
        "off" => LevelFilter::Off,
        _ => LevelFilter::Info,
    }
}

/// Convert log level to integer (for Slint binding)
pub fn level_to_i32(level: LevelFilter) -> i32 {
    match level {
        LevelFilter::Off => -1,
        LevelFilter::Error => 0,
        LevelFilter::Warn => 1,
        LevelFilter::Info => 2,
        LevelFilter::Debug => 3,
        LevelFilter::Trace => 4,
    }
}

/// Convert integer to log level (for Slint binding)
pub fn i32_to_level(level: i32) -> LevelFilter {
    match level {
        0 => LevelFilter::Error,
        1 => LevelFilter::Warn,
        2 => LevelFilter::Info,
        3 => LevelFilter::Debug,
        4 => LevelFilter::Trace,
        _ => LevelFilter::Info,
    }
}

// ========= Slint Integration Helper Functions ========= //

/// Slint logging function - Error level
pub fn slint_log_error(message: &str) {
    log::error!("[Slint] {}", message);
}

/// Slint logging function - Warn level
pub fn slint_log_warn(message: &str) {
    log::warn!("[Slint] {}", message);
}

/// Slint logging function - Info level
pub fn slint_log_info(message: &str) {
    log::info!("[Slint] {}", message);
}

/// Slint logging function - Debug level
pub fn slint_log_debug(message: &str) {
    log::debug!("[Slint] {}", message);
}

/// Slint logging function - Trace level
pub fn slint_log_trace(message: &str) {
    log::trace!("[Slint] {}", message);
}

// ========= Convenience Macros (Optional) ========= //

/// Convenience macro: log with module prefix
#[macro_export]
macro_rules! debug_log {
    ($level:ident, $($arg:tt)*) => {
        log::$level!($($arg)*)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_level() {
        assert_eq!(parse_level("error"), LevelFilter::Error);
        assert_eq!(parse_level("WARN"), LevelFilter::Warn);
        assert_eq!(parse_level("Info"), LevelFilter::Info);
        assert_eq!(parse_level("debug"), LevelFilter::Debug);
        assert_eq!(parse_level("trace"), LevelFilter::Trace);
        assert_eq!(parse_level("unknown"), LevelFilter::Info);
    }

    #[test]
    fn test_level_conversion() {
        assert_eq!(level_to_i32(LevelFilter::Error), 0);
        assert_eq!(level_to_i32(LevelFilter::Info), 2);
        assert_eq!(i32_to_level(0), LevelFilter::Error);
        assert_eq!(i32_to_level(2), LevelFilter::Info);
    }

    #[test]
    fn test_default_log_path() {
        let path = get_default_log_path();
        assert!(path.to_string_lossy().contains("usher.log"));
    }
}
