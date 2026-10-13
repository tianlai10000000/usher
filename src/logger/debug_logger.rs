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
//!     console_output: true,
//!     file_output: true,
//!     log_file_path: Some("/path/to/app.log".into()),
//!     log_per_day: false,
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
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

/// Logger configuration
#[derive(Clone, Debug)]
pub struct DebugLogConfig {
    /// Log level
    pub level: LevelFilter,
    /// Whether to write to standard output
    pub console_output: bool,
    /// Whether to write to the application log file
    pub file_output: bool,
    /// Log file path
    pub log_file_path: Option<PathBuf>,
    /// Whether to organize logs by date (YYYY-MM-DD/usher.log)
    pub log_per_day: bool,
}

impl Default for DebugLogConfig {
    fn default() -> Self {
        Self {
            level: LevelFilter::Info,
            console_output: true,
            file_output: false,
            log_file_path: None,
            log_per_day: false,
        }
    }
}

/// Global logger initialization state
static LOGGER_INITIALIZED: OnceLock<bool> = OnceLock::new();
static CONSOLE_OUTPUT_ENABLED: AtomicBool = AtomicBool::new(true);
static FILE_OUTPUT_ENABLED: AtomicBool = AtomicBool::new(false);
static APPLICATION_LOG_FILE: OnceLock<Weak<Mutex<LazyLogFile>>> = OnceLock::new();

/// Buffer capacity for the log file writer. Records are flushed eagerly for
/// warn/error levels and on process shutdown, so the buffer only defers the
/// low-severity chatter that used to hit the disk line by line.
const LOG_FILE_BUFFER_CAPACITY: usize = 64 * 1024;
const LOG_FILE_FLUSH_INTERVAL: Duration = Duration::from_secs(1);

struct LazyLogFile {
    path: PathBuf,
    writer: Option<BufWriter<File>>,
    last_flush: Instant,
}

impl LazyLogFile {
    fn writer(&mut self) -> io::Result<&mut BufWriter<File>> {
        if self.writer.is_none() {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            self.writer = Some(BufWriter::with_capacity(LOG_FILE_BUFFER_CAPACITY, file));
        }
        Ok(self.writer.as_mut().expect("log file initialized"))
    }
}

impl Write for LazyLogFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !file_output_enabled() {
            return Ok(buf.len());
        }
        // write_all keeps a formatted record intact; a raw `write` may report
        // a short write and truncate the line.
        self.writer()?.write_all(buf)?;
        if self.last_flush.elapsed() >= LOG_FILE_FLUSH_INTERVAL {
            self.writer()?.flush()?;
            self.last_flush = Instant::now();
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !file_output_enabled() {
            return Ok(());
        }
        self.writer()?.flush()
    }
}

/// Flush the real sink, including INFO records written after shutdown tasks.
/// fern's callback output does not forward `log::Log::flush` to this writer.
pub(crate) fn flush_application_log() {
    if let Some(log_file) = APPLICATION_LOG_FILE.get().and_then(Weak::upgrade)
        && let Ok(mut file) = log_file.lock()
    {
        let _ = file.flush();
    }
}

pub fn set_console_output_enabled(enabled: bool) {
    CONSOLE_OUTPUT_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn set_file_output_enabled(enabled: bool) {
    FILE_OUTPUT_ENABLED.store(enabled, Ordering::Relaxed);
}

fn console_output_enabled() -> bool {
    CONSOLE_OUTPUT_ENABLED.load(Ordering::Relaxed)
}

fn file_output_enabled() -> bool {
    FILE_OUTPUT_ENABLED.load(Ordering::Relaxed)
}

/// Get the default log file path
/// Uses the directories crate to get the system standard log directory
///
/// # Arguments
/// * `log_per_day` - If true, organizes logs by date (YYYY-MM-DD/usher.log)
pub fn get_default_log_path(log_per_day: bool) -> PathBuf {
    if let Some(data_dir) = crate::app_paths::data_local_dir() {
        let base_log_dir = data_dir.join("logs");

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
pub fn setup_debug_logger(config: DebugLogConfig) -> Result<(), String> {
    set_console_output_enabled(config.console_output);
    set_file_output_enabled(config.file_output);

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
        .filter(|_| console_output_enabled())
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
        .level_for("wgpu", LevelFilter::Warn)
        .level_for("wgpu_core", LevelFilter::Warn)
        .level_for("wgpu_hal", LevelFilter::Warn)
        .level_for("wgpu_types", LevelFilter::Warn)
        .level_for("naga", LevelFilter::Warn)
        .chain(std::io::stdout());

    // Create main dispatch
    let mut main_dispatch = fern::Dispatch::new().chain(console_dispatch);

    let resolved_log_path = config
        .log_file_path
        .clone()
        .unwrap_or_else(|| get_default_log_path(config.log_per_day));

    let log_file = Arc::new(Mutex::new(LazyLogFile {
        path: resolved_log_path.clone(),
        writer: None,
        last_flush: Instant::now(),
    }));
    let log_file_handle = Arc::downgrade(&log_file);
    let shutdown_log_file = log_file.clone();
    crate::shutdown::register_post_shutdown_task("flush_debug_log_file", move || {
        if let Ok(mut file) = shutdown_log_file.lock() {
            let _ = file.flush();
        }
    });

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
        .filter(|_| file_output_enabled())
        .level(config.level)
        .level_for("reqwest", LevelFilter::Warn)
        .level_for("hyper", LevelFilter::Warn)
        .level_for("hyper_util", LevelFilter::Warn)
        .level_for("h2", LevelFilter::Warn)
        .level_for("rustls", LevelFilter::Warn)
        .level_for("tokio_util", LevelFilter::Warn)
        .level_for("tungstenite", LevelFilter::Warn)
        .level_for("tracing", LevelFilter::Warn)
        .level_for("wgpu", LevelFilter::Warn)
        .level_for("wgpu_core", LevelFilter::Warn)
        .level_for("wgpu_hal", LevelFilter::Warn)
        .level_for("wgpu_types", LevelFilter::Warn)
        .level_for("naga", LevelFilter::Warn)
        .chain(fern::Output::call(move |record| {
            let Ok(mut file) = log_file.lock() else {
                return;
            };
            // `record.args()` already carries the formatted line from the
            // dispatch formatter above.
            let _ = writeln!(file, "{}", record.args());
            // Warnings and errors are flushed immediately so a crash right
            // after them cannot lose the interesting tail of the log.
            if record.level() <= log::Level::Warn {
                let _ = file.flush();
            }
        }));

    main_dispatch = main_dispatch.chain(file_dispatch);

    // Apply logger configuration
    main_dispatch
        .apply()
        .map_err(|e| format!("Failed to apply logger configuration: {}", e))?;
    let _ = APPLICATION_LOG_FILE.set(log_file_handle);

    // Mark as initialized
    LOGGER_INITIALIZED.set(true).ok();

    // TODO: Print actual resolved config values
    log::debug!(
        "Debug Logger initialized successfully! - Level: {:?}, PerDay: {:?}, ConsoleOutput: {:?}, FileOutput: {:?}, Log file path: {:?}",
        config.level,
        config.log_per_day,
        config.console_output,
        config.file_output,
        resolved_log_path
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
        let path = get_default_log_path(false);
        assert!(path.to_string_lossy().contains("usher.log"));
    }
}
